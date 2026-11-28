# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
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
CROW_FIELD_M = 40.0
# Energy needs (needs-v1): biomimetic bodies feed on the habitat's food,
# machines recharge at a charging dock. The crow keeps its own energy channels;
# fixed-base machines cannot reach a dock.
NEEDS_VERSION = 'needs-v1'
ANIMALS = ('celegans', 'drosophila_banc', 'drosophila_fafb', 'zebrafish', 'salamander_crawler')
NO_NEEDS = ('crow_corvus_corone', 'cofe_robot_cafe', 'cofe_smart_bar', 'xbot_ic60', 'xbot_s_pro', 'botinkit_omni')


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
        # The salamander crawler forages on the fly's orchard floor (fallen
        # fruit is its food source), at the walkers' 1.3 scale.
        salamander = profile == 'salamander_crawler'
        kind = 'orchard' if profile == 'crow_corvus_corone' or salamander else 'terrain' if profile.startswith('ugot_') else 'room'
        habitat = copy.deepcopy(next(h for h in catalogue['habitats'] if h['id'] == kind))
        scale = 2.8 if profile == 'crow_corvus_corone' else 1.3 if profile.startswith('ugot_') or salamander else 3.0
        if salamander:
            pass
        elif profile == 'crow_corvus_corone':
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
                # Walls stay put: shifting them as furniture opened 1.5 m gaps
                # in the room's corners that explorers drove out through.
                if o['collision'] and o['id'] != 'substrate' and not o['id'].startswith('rim_'):
                    o['position'][0] += .25 if o['position'][0] >= 0 else -.25
    if profile not in ANIMALS and profile not in NO_NEEDS:
        # Charging dock: a floor pad (no collision, so the body can drive or
        # walk onto it) at the free floor point with the most clearance from
        # every solid object's edge (and from the rims), away from the spawn
        # point at the centre. The first placement measured from object
        # centres and picked a corner 0.30 m from the hexapod's slalom post,
        # where the robot wedged beside the dock for 800 s (25 Nov).
        solid = [o for o in habitat['objects'] if o['collision'] and o['id'] != 'substrate' and not o['id'].startswith('rim_')]
        def clearance(c):
            edge = min((math.hypot(c[0]-o['position'][0], c[1]-o['position'][1]) - .5 * max(o['size'][:2])
                        for o in solid), default=1.0)
            return min(edge, .9 - max(abs(c[0]), abs(c[1])))
        grid = [(i / 20, j / 20) for i in range(-16, 17) for j in range(-16, 17) if math.hypot(i / 20, j / 20) >= .45]
        x, y = max(grid, key=lambda c: (round(clearance(c), 3), -abs(c[0]) - abs(c[1])))
        habitat['objects'].append(primitive('charging_dock', 'box', [x, y, .004], [.14, .14, .008], [.12, .55, .9],
                                            collision=False, material='plastic', cue='beacon', radius=.3, strength=1.))
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
        if profile=='crow_corvus_corone' and item['id'].startswith('rim_'):
            # A crow cruises at ~10 m/s and turns on a ~15-25 m radius, so a
            # 5.6 m enclosure is crossed in half a second. The woodland edge
            # is an 80 m field bounded by an 8 m tree line (dark foliage,
            # retina-visible against ground and sky); the scaled perches,
            # food, water and landmarks stay at its centre.
            k=int(item['id'][-1]);sign=1 if k in (0,2) else -1;half=CROW_FIELD_M
            along,across=[2*half,1.],[1.,2*half]
            item['position']=[0,sign*half,4.] if k<2 else [sign*half,0,4.]
            item['size']=(along if k<2 else across)+[8.]
            item['colour']=[.10,.18,.08];item['material']='foliage'
        elif profile=='zebrafish' and item['id'].startswith('rim_'):
            # Aquarium walls rise above the water line (surface .4 m) so the
            # fish's range rays see its habitat's limits and it cannot swim
            # out over a wall; the low front rim becomes a clear glass pane.
            item['size'][2]=.42;item['position'][2]=.21
            if item['id']=='rim_1':item['material']='glass'
        elif profile.startswith('ugot_') and item['id'].startswith('rim_'):
            # Archetype range rays sit 0.1 m above the body origin: a UGOT
            # walker standing on its OEM legs (0.144 m) casts them at 0.244 m,
            # over the catalogue's 0.234 m terrain rim, so it never saw its
            # arena. The rims rise to 0.42 m (clear front pane) for every
            # UGOT body.
            item['size'][2]=.42;item['position'][2]=.21
            if item['id']=='rim_1':item['material']='glass'
        elif profile!='celegans' and item['id']=='rim_1':
            # The catalogue's low front rim (a camera opening) let explorers
            # drive or walk out of their habitat; it is a full-height clear
            # pane for every body except the worm's agar plate.
            tall=max(o['size'][2] for o in habitat['objects'] if o['id'].startswith('rim_'))*scale
            item['size'][2]=tall;item['position'][2]=tall/2;item['material']='glass'
        # Nearby food also has tangible/ray-visible colony tops. A label alone
        # must not be mistaken for a stimulus in the native proximity channels.
        if profile == 'celegans' and o['id'].startswith(('bacterial_lawn_', 'colony_')):
            item['collision'] = True
        objects.append(item)
    scene = dict(version=VERSION, habitat_id=habitat['id'], title=habitat['title'],
                 half_extent_m=CROW_FIELD_M if profile=='crow_corvus_corone' else scale,
                 catalogue_digest=catalogue['digest'], substrate=habitat['substrate'], cues=habitat['cues'], objects=objects,
                 scientific_status='illustrative ecological scenery and ideal sensory proxies; no measured chemistry, nutrition, temperature or fluid model')
    if profile not in NO_NEEDS:
        scene['needs'] = needs_block(profile, objects, scene['half_extent_m'])
    if profile == 'celegans':
        scene.update(celegans_stimuli(objects))
        scene['version'] = CELEGANS_VERSION
        scene['scientific_status'] = ('analytic stimulus fields (normalised [0,1]) derived from the habitat objects; '
                                      'qualitative agar-plate assays, not measured chemistry or temperature')
    return scene


CELEGANS_VERSION = 'celegans-habitat-v3'


def needs_block(profile, objects, half):
    """Energy needs of a body (scene record, read by every engine).

    Engineering proxy, not calibrated metabolism or battery models:
    - store = 1 W/kg x total mass x 240 s;
    - drain = basal x (1 + effort), effort = mechanical power / (2 W/kg x mass), at most 1;
    - refill from empty in 20 s at a source, only below full.
    Food items (animals) or the charging dock (machines) are consumed within
    a tenth of the habitat's half extent of the body's root, and sensed
    through a Gaussian odour / beacon field of 0.35 half extents. For the
    first 600 s a caretaker presents food or plugs the robot in whenever the
    reserve is below 40 %.
    """
    animal = profile in ANIMALS
    if animal:
        sources = [o for o in objects if o.get('cue') == 'chemical' or o['id'].startswith(('prey_', 'fruit_', 'food_'))]
    else:
        sources = [o for o in objects if o['id'] == 'charging_dock']
    reach = .1 * half
    return dict(version=NEEDS_VERSION, resource='food' if animal else 'power',
                basal_w_per_kg=1.0, endurance_s=240.0, effort_ref_w_per_kg=2.0, refill_s=20.0, start_fraction=1.0,
                sources=[dict(id=o['id'], position_m=[round(v, 6) for v in o['position']],
                              radius_m=round(reach + .5 * max(o['size'][:2]), 6), field_radius_m=round(.35 * half, 6))
                         for o in sources],
                caretaker=dict(until_s=600.0, threshold=.4, feed_s=20.0),
                scientific_status='engineering proxy: energy store, drain and refill are not calibrated metabolism or battery '
                                  'models; early-life caretaker presents food or docks the body')


def needs_sensors(profile, half):
    """Optional sensor group `needs`: the energy reserve (interoception /
    battery state of charge), refill (taste of food / charge current) and a
    left/right cue pair (odour receptors or dock-beacon receivers aimed
    45 degrees outward). Networks built without them bind without them."""
    if profile in NO_NEEDS:
        return []
    resource = 'food' if profile in ANIMALS else 'power'
    body = 'segment_23' if profile == 'celegans' else None   # the worm's head
    off = .03 * half
    common = dict(units='normalised', range=[0, 1], rate_hz=500, missing='fault-disarm', group='needs')
    out = [dict(name='needs_energy', kind='needs-energy', mode=resource, **common),
           dict(name='needs_consume', kind='needs-consume', mode=resource, **common)]
    for side, sign in (('left', 1), ('right', -1)):
        cue = dict(name=f'needs_cue_{side}', kind='needs-cue', mode=resource, position_m=[off, sign * off, 0.],
                   direction=[.7071, sign * .7071, 0.], **common)
        if body:
            cue['body'] = body
        out.append(cue)
    return out


def celegans_stimuli(objects):
    """Analytic stimulus fields for the worm's chemical, thermal, light and O2 senses.

    Values are normalised to [0, 1] and sampled by the native adapter at each
    sensor mount (``celegans-field-v2``). Shapes: ``gaussian`` (centre, sigma
    radii), ``ellipse`` (plateau over the semi-axes with a soft ``edge``),
    ``ring`` (band of ``width`` at ``radius``), ``linear`` (``offset`` plus
    ``gradient`` . xy) and ``constant``. Fields sum and are clamped.
    Biological reading (agar-plate assays): volatile food odour diffuses from
    bacterial lawns (AWA/AWC), soluble food cues and salt sit on the lawn and
    in a salt spot (ASE), soluble repellent (copper ring / SDS spot) is sensed
    by ASH/ADL at the nose and PHA/PHB at the tail, temperature has a plate
    gradient plus a warm patch and a cool refuge (AFD), light falls under the
    beacon (LITE-1 cells) and O2 is depleted inside dense lawns (URX/AQR/PQR).
    Normalised temperature maps 0 -> 15 C and 1 -> 27 C.
    """
    by_id = {o['id']: o for o in objects}
    lawns = [by_id[k] for k in sorted(by_id) if k.startswith('bacterial_lawn_')]
    stimuli = []
    for i, lawn in enumerate(lawns):
        x, y = lawn['position'][:2]
        ax, ay = lawn['size'][0] / 2, lawn['size'][1] / 2
        strength = [0.8, 0.6, 0.5][i % 3]
        # Odour plume ~2 body-widths beyond the lawn edge: distinct gradients
        # towards each lawn instead of one saturated band.
        stimuli.append(dict(field='attractant_volatile', shape='gaussian', center=[x, y], radii=[ax + .06, ay + .06], strength=strength, source=lawn['id']))
        stimuli.append(dict(field='attractant_soluble', shape='ellipse', center=[x, y], radii=[ax, ay], edge=.03, strength=.85, source=lawn['id']))
        stimuli.append(dict(field='oxygen', shape='ellipse', center=[x, y], radii=[ax, ay], edge=.03, strength=-.35, source=lawn['id']))
    stimuli.append(dict(field='oxygen', shape='constant', strength=.6, source='ambient air (21 % O2 -> 0.6)'))
    stimuli.append(dict(field='attractant_soluble', shape='gaussian', center=[-.06, -.30], radii=[.08, .08], strength=.6, source='salt_spot'))
    if lawns:
        x, y = lawns[-1]['position'][:2]
        stimuli.append(dict(field='repellent_soluble', shape='ring', center=[x, y], radius=max(lawns[-1]['size'][:2]) / 2 + .045, width=.018, strength=1.0, source='copper_ring'))
    stimuli.append(dict(field='repellent_soluble', shape='gaussian', center=[-.40, .07], radii=[.06, .06], strength=1.0, source='repellent_spot'))
    stimuli.append(dict(field='temperature', shape='linear', offset=.5, gradient=[.30 / .65, 0.], source='plate thermal gradient'))
    if 'warm_patch' in by_id:
        x, y = by_id['warm_patch']['position'][:2]
        stimuli.append(dict(field='temperature', shape='gaussian', center=[x, y], radii=[.10, .10], strength=.35, source='warm_patch'))
    if 'cool_moist_refuge' in by_id:
        x, y = by_id['cool_moist_refuge']['position'][:2]
        stimuli.append(dict(field='temperature', shape='gaussian', center=[x, y], radii=[.10, .10], strength=-.35, source='cool_moist_refuge'))
    if 'light_beacon' in by_id:
        x, y = by_id['light_beacon']['position'][:2]
        stimuli.append(dict(field='light', shape='gaussian', center=[x, y], radii=[.13, .13], strength=1.0, source='light_beacon'))
    # 40 m/s^2 for 6 ms: a ~1.6 mm hop; the proper-acceleration step seen by
    # the vibration channel is the excess over gravity (~30 m/s^2).
    taps = dict(period_s=15.0, first_s=7.5, duration_s=.006, acceleration_mps2=40.0,
                note='plate tap: vertical whole-body acceleration pulse (touch-receptor tap response)')
    # Resistive-force substrate (agar groove): per-segment tangential and
    # normal drag. Crawling on agar has a normal/tangential ratio of ~10-40;
    # magnitudes are scaled so the 6 mN m hinge servos can still bend the body.
    # A programmed 0.5 Hz, 0.65 L head-to-tail wave crawls forward at
    # 0.16 L/s with these values (worm on agar ~0.1-0.2 L/s); the reversed
    # wave crawls backward and isotropic drag gives no net thrust.
    rft = dict(tangential_N_s_per_m=.01, normal_N_s_per_m=.2, ratio=20.0,
               note='resistive force theory for a nematode on agar (Wallace 1968; Berri 2009); calibrated with a programmed travelling wave')
    return dict(stimuli=stimuli, taps=taps, resistive_force=rft)


def celegans_markers():
    """Visual-only markers for stimuli that have no catalogue geometry."""
    return [('salt_spot', 'cylinder', [-.06, -.30, .0012], [.16, .16, .0024], [.92, .92, .95]),
            ('repellent_spot', 'cylinder', [-.40, .07, .0012], [.12, .12, .0024], [.55, .22, .62])]


def fish_hydrodynamics(root):
    """Engine-independent water model for the fish (resistive-ellipsoid-v1).

    Every body with an ellipsoid ``*_collision`` geom is a hydrodynamic
    ellipsoid (body-aligned, centred at ``offset_m``). Each engine applies, per
    ellipsoid and in its own axes, quadratic drag
    ``-0.5 rho Cd_k A_k |u_k| u_k`` with projected areas ``A = pi (bc, ac, ab)``
    and ``Cd = (tangential, normal, normal)``, a quadratic rotational drag, and
    neutral buoyancy (displaced volume = mass / rho) inside the water column.
    Normal drag ten times the tangential drag lets a head-to-tail body wave
    push the fish forward (Taylor 1952; Lighthill 1971); isotropic drag cannot.
    Calibration: a programmed 6 Hz, 1 L wave with the native servos swims
    ~0.6 L/s head-first (adult zebrafish routine swimming ~0.5-2 L/s).
    """
    bodies = []
    for body in root.iter('body'):
        for geom in body.findall('geom'):
            if geom.get('name', '').endswith('_collision') and geom.get('type') == 'ellipsoid':
                if geom.get('quat') or geom.get('euler'):
                    raise ValueError('hydrodynamic ellipsoids must be body-aligned: ' + geom.get('name'))
                bodies.append(dict(body=body.get('name'), half_size_m=[float(v) for v in geom.get('size').split()],
                                   offset_m=[float(v) for v in geom.get('pos', '0 0 0').split()]))
    return dict(model='resistive-ellipsoid-v1', density_kg_m3=1000.0, surface_z_m=.4, bottom_z_m=0.0,
                submergence_band_m=.012, drag_coefficients=[.1, 1.0, 1.0], rotational_coefficient=1.0,
                # Centre of buoyancy 0.5 mm dorsal to each centre of mass (the
                # swim bladder lies dorsally): without it the trunk is passively
                # unstable in pitch (centre of vertical drag ahead of the COM)
                # and dives or climbs within ~5 s, as larvae would without
                # active correction (Ehrlich & Schoppik 2017).
                buoyancy_offset_m=[0.0, 0.0, .0005],
                bodies=bodies, note='quasi-steady resistive hydrodynamics, no added mass or vortex wake; '
                'buoyancy is neutral by construction (mass = rho x ellipsoid volume)')


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
        attrs = dict(name='habitat_'+o['id'], pos=o['position'], rgba=o['colour']+[.16 if o['material']=='water' else .2 if o['material']=='glass' else 1],
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
    if profile == 'celegans':
        for name, shape, pos, size, rgba in celegans_markers():
            element(world, 'geom', name='habitat_'+name, type=shape, pos=pos, size=[size[0]/2, size[2]/2], rgba=rgba+[.55],
                    contype=0, conaffinity=0, group=2)
        for st in scene.get('stimuli', []):
            if st.get('shape') == 'ring':
                cx, cy = st['center']; r = st['radius']
                for k in range(36):
                    a0 = 2*math.pi*k/36; a1 = 2*math.pi*(k+1)/36
                    element(world, 'geom', name=f'habitat_{st["source"]}_{k:02}', type='capsule',
                            fromto=[cx+r*math.cos(a0), cy+r*math.sin(a0), .0012, cx+r*math.cos(a1), cy+r*math.sin(a1), .0012],
                            size=st['width']/2, rgba=[.72,.45,.2,.8], contype=0, conaffinity=0, group=2)
    if profile == 'zebrafish':
        scene['hydrodynamics'] = fish_hydrodynamics(root)
        scene['scientific_status'] += '; resistive ellipsoid hydrodynamics (engineering fluid model, not CFD)'
    visual = root.find('visual')
    if visual is None:
        visual=element(root,'visual')
    entries=[('headlight',dict(ambient=[.35,.35,.35],diffuse=[.7,.7,.7],specular=[.1,.1,.1])),('rgba',dict(haze=[.83,.87,.84,1])),('quality',dict(shadowsize=2048))]
    if profile in ('celegans', 'zebrafish'):
        # 720p offscreen framebuffer for frame-by-frame replay video.
        entries.append(('global',dict(offwidth=1280,offheight=720)))
    for tag,attrs in entries:
        child=visual.find(tag)
        if child is None:element(visual,tag,**attrs)
        else:
            for key,value in attrs.items():child.set(key,' '.join(map(str,value)) if isinstance(value,list) else str(value))
    return scene
