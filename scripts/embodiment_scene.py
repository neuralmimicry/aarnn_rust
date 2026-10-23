"""Shared, deterministic SI habitats for the canonical native model pipeline.

Reuse the existing visual/ecological catalogue. Cues are engineering proxies;
scenery never supplies motor commands, food energy or biological capabilities.
"""
import copy
import math
from pathlib import Path
from sim_content import compile_catalog, primitive

ROOT = Path(__file__).resolve().parents[1]
VERSION = 'native-habitats-v1'


def geom_kind(root, geom):
    if geom.get('type'):return geom.get('type')
    inherited=root.find(f'.//default[@class="{geom.get("class", "")}"]/geom')
    return inherited.get('type','sphere') if inherited is not None else 'sphere'


def description(profile):
    catalogue = compile_catalog()
    original = next((p for p in catalogue['profiles'] if p['id'] == profile), None)
    if original:
        habitat = copy.deepcopy(next(h for h in catalogue['habitats'] if h['id'] == original['habitat']))
        scale = {'celegans': .65, 'drosophila_banc': .55, 'drosophila_fafb': .55, 'zebrafish': .375, 'hexapod': 1.3, 'nao': 1.5}[profile]
    else:
        kind = 'orchard' if profile == 'crow_corvus_corone' else 'terrain' if profile.startswith('ugot_') else 'room'
        habitat = copy.deepcopy(next(h for h in catalogue['habitats'] if h['id'] == kind))
        scale = 2.8 if profile == 'crow_corvus_corone' else 1.3 if profile.startswith('ugot_') else 3.0
        if profile == 'crow_corvus_corone':
            habitat.update(id='woodland', title='Carrion crow woodland edge', cues=['visible food', 'water', 'branches and perches', 'optic-flow landmarks', 'light and shade'])
            habitat['objects'] = [o for o in habitat['objects'] if not o['id'].startswith(('fruit_', 'fruit_bruise_', 'stem_', 'leaf_'))]
            for i, (x,y,z) in enumerate([(.42,.22,.12),(-.35,.28,.24),(.25,-.42,.18)]):
                # Horizontal branch native capsule, no magnetic grasp or support.
                habitat['objects'].append(primitive(f'perch_{i}','capsule',[x,y,z],[.34,.018,.018],[.31,.22,.12],collision=True))
            for i in range(8):
                habitat['objects'].append(primitive(f'food_{i}','sphere',[.20+i*.025,-.12,.009],[.012,.012,.018],[.66,.32,.10],collision=True,cue='chemical',radius=.12,strength=.3))
            habitat['objects'].append(primitive('water_bowl','cylinder',[-.3,-.25,.008],[.12,.12,.016],[.2,.49,.65],collision=True))
        elif not profile.startswith('ugot_'):
            # Keep furniture clear of the initial appliance/manipulator envelope.
            for o in habitat['objects']:
                if o['collision'] and o['id'] != 'substrate':
                    o['position'][0] += .25 if o['position'][0] >= 0 else -.25
    objects = []
    if profile=='celegans':
        for i,(x,y) in enumerate([(.37,0),(.39,.10),(-.37,-.06),(.06,.17)]):
            habitat['objects'].append(primitive(f'food_contact_{i}','sphere',[x,y,.028],[.055,.055,.056],[.69,.63,.28],collision=True,cue='chemical',radius=.13,strength=.35))
    for o in habitat['objects']:
        if o['id'] == 'substrate':
            continue  # Canonical floor remains the only support plane.
        item = copy.deepcopy(o)
        item['position'] = [v*scale for v in o['position']]
        item['size'] = [v*scale for v in o['size']]
        item['radius'] *= scale
        if profile=='zebrafish' and item['id']=='water_volume':
            item['position'][2]=.20;item['size'][2]=.40
        if profile=='zebrafish' and item['id'].startswith('surface_ripple_'):item['position'][2]=.401
        # Nearby food also has tangible/ray-visible colony tops. A label alone
        # must not be mistaken for a stimulus in the native proximity channels.
        if profile == 'celegans' and o['id'].startswith(('bacterial_lawn_', 'colony_')):
            item['collision'] = True
        objects.append(item)
    return dict(version=VERSION, habitat_id=habitat['id'], title=habitat['title'], half_extent_m=scale,
                catalogue_digest=catalogue['digest'], substrate=habitat['substrate'], cues=habitat['cues'], objects=objects,
                scientific_status='illustrative ecological scenery and ideal sensory proxies; no measured chemistry, nutrition, temperature or fluid model')


def install(profile, root, element):
    scene = description(profile)
    world = root.find('worldbody')
    assets = root.find('asset')
    if assets is None:
        assets = element(root, 'asset')
    element(assets, 'texture', name='habitat_grid', type='2d', builtin='checker', rgb1=scene['substrate'], rgb2=[min(1,v*1.12) for v in scene['substrate']], width=512, height=512)
    element(assets, 'material', name='habitat_substrate', texture='habitat_grid', texrepeat=[12,12], reflectance=.08, roughness=.9)
    for geom in world.findall('geom'):
        if geom_kind(root,geom) == 'plane':
            geom.set('material','habitat_substrate')
            geom.set('rgba','1 1 1 1')
    for o in scene['objects']:
        shape = o['shape']; half = [v/2 for v in o['size']]
        attrs = dict(name='habitat_'+o['id'], pos=o['position'], rgba=o['colour']+[.16 if o['material']=='water' else 1],
                     contype=1 if o['collision'] else 0, conaffinity=1 if o['collision'] else 0,
                     group=0 if o['collision'] else 2)
        if shape == 'box':
            attrs.update(type='box',size=half,quat=[math.cos(o['yaw']/2),0,0,math.sin(o['yaw']/2)])
        elif shape == 'capsule':
            x,y,z=o['position']; attrs.pop('pos')
            attrs.update(type='capsule',fromto=[x-half[0],y,z,x+half[0],y,z],size=half[1])
        elif shape == 'cylinder':
            attrs.update(type='cylinder',size=[half[0],half[2]])
        else:
            attrs.update(type='ellipsoid',size=half)
        element(world,'geom',**attrs)
        if o['cue'] in ('light','heat'):
            p=list(o['position']);p[2]+=max(.06,scene['half_extent_m']*.12)
            element(world,'light',name='cue_'+o['id'],pos=p,dir=[0,0,-1],diffuse=[.9,.28,.08] if o['cue']=='heat' else [.9,.85,.62],directional='false',castshadow='false')
    visual = root.find('visual')
    if visual is None:
        visual=element(root,'visual')
    for tag,attrs in [('headlight',dict(ambient=[.35,.35,.35],diffuse=[.7,.7,.7],specular=[.1,.1,.1])),('rgba',dict(haze=[.83,.87,.84,1])),('quality',dict(shadowsize=2048))]:
        child=visual.find(tag)
        if child is None:element(visual,tag,**attrs)
        else:
            for key,value in attrs.items():child.set(key,' '.join(map(str,value)) if isinstance(value,list) else str(value))
    return scene
