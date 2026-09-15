#!/usr/bin/env python3
"""Authoritative bounded procedural content compiler. No random global state or assets.

All primitive dimensions are full extents, Z-up; angles are radians around Z.
Visual anatomy is illustrative. This module never executes a neural network.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import math
from pathlib import Path
from robot_profiles import PROFILES

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / 'sim/content/catalog.json'


def primitive(name, shape, position, size, colour, *, yaw=0, collision=False,
              material='organic', cue='', radius=0, strength=0, anchor='root', internal=False):
    return dict(id=name, shape=shape, position=list(position), size=list(size),
                colour=list(colour), yaw=yaw, collision=collision, material=material,
                cue=cue, radius=radius, strength=strength, anchor=anchor, internal=internal)


def habitat_objects(h):
    out = []
    def add(name, shape, p, s, c, **kw):
        out.append(primitive(name, shape, p, s, c, **kw))
    floor = h['substrate']
    add('substrate', 'box', (0, 0, -.025), (2, 2, .05), floor, collision=True)
    height = h['wall_height']
    # Low front rim leaves the habitat legible; three high sides preserve optic flow.
    for i, (p,s) in enumerate([((0,1,height/2),(2,.025,height)),((0,-1,.035),(2,.025,.07)),
                               ((1,0,height/2),(.025,2,height)),((-1,0,height/2),(.025,2,height))]):
        add(f'rim_{i}', 'box', p, s, (.42,.5,.47), collision=True, material='stone')
    # Fixed low-discrepancy placement: repeatable microtexture, never a frame-time RNG.
    for i in range(56):
        x = ((i * 37 % 101) / 101 * 1.84) - .92
        y = ((i * 61 % 103) / 103 * 1.84) - .92
        shade = .88 + (i % 5) * .04
        add(f'grain_{i:02}', 'sphere', (x,y,.003), (.015,.011,.006),
            tuple(min(1,v*shade) for v in floor), material='stone')
    kind = h['id']
    if kind == 'agar':
        for i,(x,y,r) in enumerate([(.42,.2,.23),(-.48,-.24,.16),(.25,-.65,.13)]):
            add(f'bacterial_lawn_{i}', 'sphere', (x,y,.009), (r*2,r*1.6,.014),
                (.59,.66,.24), cue='chemical', radius=r*2, strength=.95)
            for j in range(13):
                a=j*2.39996; d=r*.8*math.sqrt((j+1)/14)
                add(f'colony_{i}_{j}', 'sphere', (x+math.cos(a)*d,y+math.sin(a)*d,.019),
                    (.027,.017,.01),(.75,.77,.35))
        add('warm_patch', 'box',(-.72,.52,.008),(.3,.35,.012),(.72,.35,.2),cue='heat',radius=.48,strength=.9)
        add('cool_moist_refuge','sphere',(.7,-.4,.015),(.4,.32,.025),(.25,.44,.4),cue='moisture',radius=.35,strength=.8)
        for i in range(8):
            add(f'touch_ridge_{i}','box',(-.35+i*.095,.58,.02),(.025,.22,.035),(.43,.39,.25),collision=True)
    elif kind == 'orchard':
        for i,(x,y) in enumerate([(.48,.34),(-.55,.2),(.5,-.6),(-.7,-.62)]):
            add(f'fruit_{i}','sphere',(x,y,.065),(.18,.16,.13),(.82,.34+i*.05,.09),collision=True,cue='chemical',radius=.35,strength=.9)
            add(f'fruit_bruise_{i}','sphere',(x+.02,y-.025,.123),(.1,.075,.014),(.27,.13,.055))
            add(f'stem_{i}','cylinder',(x,y,.3),(.025,.025,.58),(.29,.24,.09),collision=True)
            for j in range(4):
                a=i+j*1.7
                add(f'leaf_{i}_{j}','sphere',(x+.09*math.cos(a),y+.09*math.sin(a),.19+j*.08),
                    (.23,.08,.012),(.16+j*.02,.37+j*.025,.095),yaw=a)
                add(f'leaf_vein_{i}_{j}','box',(x+.09*math.cos(a),y+.09*math.sin(a),.199+j*.08),
                    (.2,.005,.003),(.43,.55,.16),yaw=a)
    elif kind == 'terrain':
        for i in range(5):
            add(f'graded_step_{i}','box',(.32+i*.1,.48,.012*(i+1)),(.1,.44,.024*(i+1)),(.48,.4,.27),collision=True)
        for i in range(14):
            x=-.65+(i%4)*.115; y=.13+(i//4)*.13
            add(f'gravel_{i}','sphere',(x,y,.022),(.07,.09,.044),(.38+i%3*.03,.36,.31),collision=True)
        for i in range(4):
            add(f'slalom_{i}','cylinder',(-.6+i*.36,-.57+(i%2)*.12,.16),(.09,.09,.32),(.76,.47,.12),collision=True)
        add('traction_mat','box',(.48,-.1,.006),(.48,.4,.012),(.13,.2,.23))
        for i in range(8):
            add(f'mat_rib_{i}','box',(.29+i*.05,-.1,.018),(.012,.4,.024),(.22,.29,.3),collision=True)
    elif kind == 'room':
        add('table_top','box',(.58,.51,.16),(.5,.55,.04),(.61,.4,.24),collision=True)
        for x in [.38,.78]:
            for y in [.3,.72]:
                add(f'table_leg_{x}_{y}','box',(x,y,.075),(.035,.035,.15),(.24,.25,.28),collision=True)
        for i,c in enumerate([(.85,.16,.12),(.16,.44,.8),(.85,.68,.12)]):
            add(f'reachable_object_{i}','sphere',(.43+i*.14,.5,.208),(.055,.055,.055),c,collision=True)
        for i in range(3):
            add(f'low_step_{i}','box',(-.63+i*.15,.52,.018*(i+1)),(.15,.4,.036*(i+1)),(.37,.45,.53),collision=True)
        for x in [-.38,.38]:
            add(f'door_post_{x}','box',(x,.86,.3),(.05,.07,.6),(.27,.35,.43),collision=True)
        add('door_lintel','box',(0,.86,.62),(.81,.07,.06),(.27,.35,.43),collision=True)
        add('target_ball','sphere',(.52,-.43,.075),(.15,.15,.15),(.85,.24,.16),collision=True)
    else:
        # Explicit freshwater volume: the fish is submerged below z=.625, not
        # presented in an empty tank. Transparent water does not occlude box rays.
        add('water_volume','box',(0,0,.3125),(2,2,.625),(.10,.43,.61),material='water')
        for i in range(18):
            x=-.7+(i%6)*.27; y=.45+(i//6)*.13
            add(f'pebble_{i}','sphere',(x,y,.03),(.11,.08,.06),(.4+i%3*.045,.4,.32),collision=True)
        for i,(x,y) in enumerate([(-.68,-.48),(.65,.4),(-.62,.5),(.67,-.52)]):
            for j in range(5):
                a=j*1.3
                add(f'plant_{i}_{j}','sphere',(x+.035*math.cos(a),y+.035*math.sin(a),.15+j*.017),
                    (.018,.038,.25+j*.03),(.12,.34+j*.025,.22),yaw=a)
        add('refuge_roof','box',(-.5,.13,.17),(.35,.36,.045),(.29,.31,.28),collision=True)
        for x in [-.655,-.345]:
            add(f'refuge_support_{x}','box',(x,.13,.08),(.04,.36,.16),(.3,.32,.29),collision=True)
        add('current_source','cylinder',(.84,.05,.3),(.08,.08,.16),(.25,.52,.66),cue='flow',radius=.9,strength=.8)
        for i in range(14):
            x=.12+(i%4)*.055; y=-.36+(i//4)*.075
            add(f'prey_{i}','sphere',(x,y,.26+(i%3)*.025),(.011,.011,.008),(.83,.52,.2),cue='chemical',radius=.09,strength=.13)
        # Surface highlights make the transparent volume legible.
        for i in range(12):
            add(f'surface_ripple_{i}','box',(-.83+i*.15,.87,.628),(.09,.008,.003),(.42,.73,.77),material='water')
    # High-contrast panels make retinal channels respond to movement and heading.
    for i in range(16):
        colour=(.87,.86,.7) if i%2 else (.075,.1,.13)
        add(f'optic_flow_panel_{i}','box',(-.9+i*.12,.979,height*.55),(.06,.012,height*.65),colour)
    add('light_beacon','sphere',(.73,.73,height+.08),(.075,.075,.075),(.99,.86,.52),cue='light',radius=1.8,strength=1,material='emissive')
    return out


def robot_parts(kind):
    out=[]
    def add(name,shape,p,s,c,**kw): out.append(primitive(name,shape,p,s,c,**kw))
    if kind=='worm':
        for i in range(24):
            x=.5-i/23; r=.053*(.65+.35*math.sin(math.pi*i/23)); a=f'segment_{i:02}'
            add(f'cuticle_{i}','sphere',(x,0,0),(.067,r*2,r*2),(.76,.64,.42),anchor=a)
            for q,(y,z) in enumerate([(r*.7,r*.7),(-r*.7,r*.7),(r*.7,-r*.7),(-r*.7,-r*.7)]):
                if i == 23 and q == 2: continue  # MVL24 is absent in the canonical muscle list.
                add(f'muscle_{i}_{q}','sphere',(x,y,z),(.045,.014,.014),(.8,.33,.23),anchor=a,internal=True)
            add(f'intestine_{i}','sphere',(x,0,.015),(.06,.026,.03),(.63,.46,.17),anchor=a,internal=True)
            add(f'annulus_{i}','box',(x,0,r*.99),(.006,r*1.3,.003),(.47,.45,.29),anchor=a)
        for i,x in enumerate([.42,.32]):
            add(f'pharynx_bulb_{i}','sphere',(x,0,.025),(.065,.053,.05),(.72,.79,.62),anchor='segment_02',internal=True)
        for j in range(16):
            a=2*math.pi*j/16
            add(f'nerve_ring_{j}','sphere',(.33,.043*math.cos(a),.043*math.sin(a)),(.014,.014,.014),(.38,.68,.76),anchor='segment_04',internal=True)
        for side in [-1,1]:
            add(f'amphid_{side}','sphere',(.515,side*.025,.01),(.014,.012,.012),(.4,.64,.68),anchor='segment_00')
            add(f'phasmid_{side}','sphere',(-.49,side*.018,.005),(.01,.01,.01),(.4,.64,.68),anchor='segment_23')
    elif kind=='fly':
        add('thorax','sphere',(0,0,0),(.4,.32,.3),(.43,.26,.11))
        add('abdomen','sphere',(-.36,0,-.01),(.53,.3,.27),(.48,.29,.12))
        for i in range(6):
            add(f'abdominal_band_{i}','sphere',(-.15-i*.071,0,.0),(.025,.29-i*.022,.275-i*.014),(.15,.1,.045))
        add('head','sphere',(.3,0,.045),(.28,.29,.27),(.53,.31,.13))
        for side in [-1,1]:
            add(f'eye_{side}','sphere',(.33,side*.125,.07),(.19,.12,.22),(.64,.075,.035))
            for j in range(32):
                a=j*2.39996; r=.082*math.sqrt((j+.5)/32)
                add(f'ommatidium_{side}_{j}','sphere',(.34+math.cos(a)*r,side*(.17+.016*(1-r/.09)),.07+math.sin(a)*r),(.021,.012,.021),(.81,.16+(j%3)*.02,.06))
            add(f'antenna_{side}','sphere',(.46,side*.06,.16),(.12,.025,.025),(.32,.2,.08))
            add(f'arista_{side}','box',(.5,side*.1,.23),(.008,.01,.15),(.24,.19,.08))
            add(f'haltere_{side}','sphere',(-.2,side*.25,.08),(.035,.045,.035),(.72,.54,.23))
            add(f'wing_{side}','sphere',(-.12,side*.42,.16),(.73,.43,.016),(.69,.76,.72),yaw=side*.45,anchor=f'wing_{side}',material='membrane')
            for j in range(4):
                add(f'wing_vein_{side}_{j}','box',(-.12,side*(.29+j*.075),.171),(.6,.005,.004),(.38,.43,.35),yaw=side*.45,anchor=f'wing_{side}')
            for leg in range(3):
                x=.12-leg*.22
                for j in range(3):
                    add(f'leg_{side}_{leg}_{j}','sphere',(x-j*.08,side*(.21+j*.13),-.14-j*.1),(.065,.21,.055),(.35,.22,.1),yaw=side*.4,anchor=f'leg_{side}_{leg}')
        add('proboscis','sphere',(.46,0,-.075),(.15,.05,.05),(.38,.22,.1))
    elif kind=='fish':
        for i in range(12):
            x=.42-i*.075; r=.13*(1-i/14); a=f'segment_{i:02}'
            add(f'myomere_{i}','sphere',(x,0,0),(.18,r*1.5,r*2),(.48+i*.014,.64+i*.009,.58+i*.012),anchor=a)
            for side in [-1,1]:
                add(f'neuromast_{i}_{side}','sphere',(x,side*r*.76,.015),(.014,.009,.014),(.17,.38,.47),anchor=a)
        for side in [-1,1]:
            add(f'eye_{side}','sphere',(.45,side*.082,.065),(.084,.06,.09),(.035,.065,.075),anchor='segment_00')
            add(f'iris_{side}','sphere',(.467,side*.107,.066),(.047,.017,.05),(.52,.68,.56),anchor='segment_00')
            add(f'operculum_{side}','sphere',(.29,side*.1,-.018),(.035,.022,.14),(.59,.4,.29),anchor='segment_02')
            add(f'pectoral_fin_{side}','sphere',(.18,side*.2,-.05),(.18,.3,.016),(.53,.7,.62),yaw=side*.5,anchor='segment_03',material='membrane')
        add('dorsal_fin','sphere',(-.12,0,.15),(.4,.015,.22),(.58,.72,.62),anchor='segment_06',material='membrane')
        add('caudal_fin','sphere',(-.51,0,0),(.21,.02,.32),(.56,.7,.61),anchor='segment_11',material='membrane')
        add('swim_bladder','sphere',(.12,0,.025),(.25,.065,.07),(.82,.84,.66),anchor='segment_04',internal=True)
    elif kind=='hexapod':
        for z in [-.06,.08]:
            add(f'deck_{z}','box',(0,0,z),(.72,.6,.04),(.065,.075,.085),material='metal')
        for side in [-1,1]:
            for leg in range(3):
                x=.27-leg*.27
                add(f'standoff_{side}_{leg}','cylinder',(x,side*.23,.01),(.025,.025,.13),(.7,.5,.18),material='metal')
                for j in range(3):
                    add(f'servo_{side}_{leg}_{j}','box',(x,side*(.36+j*.13),-.02-j*.1),(.095,.11,.095),(.15,.2,.24),anchor=f'leg_{side}_{leg}')
                    add(f'link_{side}_{leg}_{j}','sphere',(x,side*(.4+j*.13),-.07-j*.1),(.055,.21,.045),(.13,.14,.15),anchor=f'leg_{side}_{leg}')
                add(f'foot_{side}_{leg}','sphere',(x,side*.69,-.3),(.085,.085,.065),(.07,.065,.06),anchor=f'leg_{side}_{leg}')
                add(f'cable_{side}_{leg}','box',(x,side*.27,.11),(.02,.18,.016),(.85,.22,.04))
            add(f'ultrasonic_{side}','sphere',(.39,side*.1,.06),(.06,.12,.12),(.65,.69,.72),material='metal')
        add('camera','box',(.38,0,.17),(.08,.16,.1),(.09,.12,.14))
        add('lens','sphere',(.427,0,.18),(.013,.058,.058),(.055,.18,.25),material='glass')
    else:
        add('torso','sphere',(0,0,.58),(.22,.34,.34),(.8,.82,.81))
        add('chest_panel','sphere',(.11,0,.61),(.04,.2,.17),(.18,.38,.55))
        add('head','sphere',(0,0,.86),(.26,.28,.25),(.82,.84,.81),anchor='head')
        for side in [-1,1]:
            add(f'eye_ring_{side}','sphere',(.12,side*.073,.89),(.04,.067,.067),(.13,.39,.56),anchor='head')
            for j in range(2):
                add(f'arm_{side}_{j}','sphere',(j*.03,side*(.24+j*.025),.62-j*.16),(.095,.095,.22),(.78,.8,.8),anchor=f'arm_{side}')
                add(f'leg_{side}_{j}','sphere',(0,side*.095,.36-j*.17),(.11,.13,.22),(.78,.8,.8),anchor=f'leg_{side}')
                add(f'joint_{side}_{j}','sphere',(0,side*.095,.4-j*.18),(.12,.14,.10),(.14,.35,.51),anchor=f'leg_{side}')
            add(f'palm_{side}','sphere',(.055,side*.27,.31),(.07,.08,.1),(.73,.76,.75),anchor=f'arm_{side}')
            for j in range(3):
                add(f'finger_{side}_{j}','sphere',(.07+j*.014,side*.28,.27),(.012,.022,.065),(.56,.61,.61),anchor=f'arm_{side}')
            add(f'foot_{side}','box',(.04,side*.1,.04),(.22,.13,.07),(.68,.75,.78),anchor=f'leg_{side}')
            add(f'pressure_pad_{side}','box',(.04,side*.1,.005),(.19,.11,.01),(.13,.21,.27),anchor=f'leg_{side}')
        for z in [.83,.92]:
            add(f'camera_{z}','sphere',(.13,0,z),(.023,.035,.035),(.025,.07,.09),anchor='head')
    return out


def compile_catalog():
    data=json.loads(SOURCE.read_text())
    for h in data['habitats']: h['objects']=habitat_objects(h)
    for p in data['profiles']:
        ref=PROFILES[p['id']]; p.update(sensory=ref.sensory,output=ref.output)
        alignment=ROOT/Path(ref.config_rel).with_suffix('.io_alignment.json')
        if alignment.exists():
            d=json.loads(alignment.read_text()); s=d['sensory_channels']
            p['sensor_names']=[v if isinstance(v,str) else v['device_port'] for v in s]
            o=d.get('actuator_channels',d.get('output_channels',[]))
            p['output_names']=[v if isinstance(v,str) else v.get('actuator_name',v.get('device_port','')) for v in o]
        else:
            p['sensor_names']=[]; p['output_names']=[]
        if p['kind']=='worm':
            groups=('MDL','MDR','MVL','MVR')
            p['muscle_channels']=[[next((i for i,n in enumerate(p['output_names'])
                if n.endswith(f'_{g}{seg+1:02}')), -1) for g in groups] for seg in range(24)]
        p['parts']=robot_parts(p['kind'])
    payload=json.dumps(data,sort_keys=True,separators=(',',':')).encode()
    data['digest']=hashlib.sha256(payload).hexdigest()
    return data


def webots_nodes(objects, scale=1, origin=(0,0,0), frame='zup', detail=False, include_internal=False, name_prefix='nm_'):
    chunks=[]
    for o in objects:
        if o['internal'] and not include_internal: continue
        x,y,z=[origin[i]+o['position'][i]*scale for i in range(3)]
        sx,sy,sz=[v*scale for v in o['size']]
        if frame=='yup': y,z=z,-y; sy,sz=sz,sy
        color=' '.join(str(v) for v in o['colour'])
        if o['shape']=='box': geo=f'Box {{ size {sx:.6f} {sy:.6f} {sz:.6f} }}'; dims='1 1 1'
        else:
            # Unit sphere for ellipsoids; cylinder is authored Z-up by Webots.
            geo='Sphere { radius 0.5 subdivision 2 }' if o['shape']=='sphere' else 'Cylinder { radius 0.5 height 1 subdivision 16 }'
            dims=f'{sx:.6f} {sy:.6f} {sz:.6f}'
        emissive=f'emissiveColor {color}' if o['material']=='emissive' else ''
        if o['material']=='water': emissive += ' transparency 0.82'
        shape=f'Transform {{ scale {dims} children [ Shape {{ appearance PBRAppearance {{ baseColor {color} roughness 0.65 metalness {0.5 if o["material"]=="metal" else 0} {emissive} }} geometry {geo} }} ] }}'
        hull=f'boundingObject Box {{ size {sx:.6f} {sy:.6f} {sz:.6f} }}' if o['collision'] and not detail else ''
        axis='0 0 1' if frame=='zup' else '0 1 0'
        if o['id']=='water_volume':
            chunks.append(f'Fluid {{ name "nm_freshwater" translation {x:.6f} {y:.6f} {z:.6f} density 1000 viscosity 0.001 children [ {shape} ] boundingObject {geo} }}')
        else:
            chunks.append(f'Solid {{ name "{name_prefix}{o["id"]}" translation {x:.6f} {y:.6f} {z:.6f} rotation {axis} {o["yaw"]:.6f} children [ {shape} ] {hull} }}')
    return '\n'.join(chunks)


def outputs(data):
    raw=json.dumps(data,separators=(',',':'),ensure_ascii=True)
    yield ROOT/'web_ui/sim-content.generated.js', '// Generated by scripts/sim_content.py; do not edit.\n(function(r){r.NmSimContent='+raw+';})(typeof window!=="undefined"?window:globalThis);\n'
    yield ROOT/'sim/unity/Assets/NeuralMimicry/Resources/NmSimContent.json', raw+'\n'
    yield ROOT/'sim/unreal/Source/NmAerBridge/Private/NmSimContent.generated.inl', '// Generated by scripts/sim_content.py; do not edit.\nstatic const ANSICHAR* NmContentJson = R"NMCONTENT('+raw+')NMCONTENT";\n'
    yield ROOT/'sim/content/compiled.generated.json', raw+'\n'
    yield ROOT/'sim/minecraft/src/main/resources/aarnn/content.generated.json', raw+'\n'
    for h in data['habitats']:
        name='NmHabitat'+h['id'].title()
        yield ROOT/f'webots_world/protos/{name}.proto', f'#VRML_SIM R2025a utf8\n# Generated by scripts/sim_content.py; do not edit. Digest {data["digest"]}\nPROTO {name} [ field SFVec3f translation 0 0 0 field SFVec3f scale 1 1 1 ] {{\n Transform {{ translation IS translation scale IS scale children [\n{webots_nodes(h["objects"])}\n] }}\n}}\n'


def main():
    parser=argparse.ArgumentParser(description=__doc__); parser.add_argument('--check',action='store_true')
    args=parser.parse_args(); data=compile_catalog(); stale=[]
    for path,content in outputs(data):
        if args.check:
            if not path.exists() or path.read_text()!=content: stale.append(str(path.relative_to(ROOT)))
        else:
            path.parent.mkdir(parents=True,exist_ok=True); path.write_text(content)
    if stale: raise SystemExit('Stale simulation content: '+', '.join(stale))
    print('Simulation content v1 '+data['digest']+(' verified' if args.check else ' generated'))



def webots_habitats(entries, positions):
    """Same authored ecology as native/browser, scaled around each species group."""
    data=compile_catalog(); chunks=[]
    groups={}
    for entry,position in zip(entries,positions):
        kind=entry[4]; groups.setdefault(kind,[]).append(position)
    for kind,points in groups.items():
        key='drosophila_banc' if kind=='drosophila' else kind
        profile=next(p for p in data['profiles'] if p['id']==key)
        h=next(h for h in data['habitats'] if h['id']==profile['habitat'])
        x=sum(p[0] for p in points)/len(points); y=sum(p[1] for p in points)/len(points)
        radius=max(h['half_extent_m'],max(max(abs(px-x),abs(py-y)) for px,py in points)+h['half_extent_m'])
        chunks.append(f'# Shared habitat {h["id"]}; content {data["digest"]}')
        chunks.append(webots_nodes(h['objects'],radius,(x,y,0),name_prefix=f'nm_{h["id"]}_'))
        light=next(o for o in h['objects'] if o['cue']=='light')
        lx,ly,lz=light['position']
        chunks.append(f'PointLight {{ location {x+lx*radius:.5f} {y+ly*radius:.5f} {lz*radius:.5f} color 1 0.86 0.52 intensity 0.8 radius {radius*3:.4f} attenuation 0 0 1 }}')
    return '\n'.join(chunks)


def reference_world(kind, proto_name, proto_ref, controller_args=None):
    data=compile_catalog(); profile=next(p for p in data['profiles'] if p['id']==kind)
    habitat=next(h for h in data['habitats'] if h['id']==profile['habitat']); radius=habitat['half_extent_m']
    robot_kind='drosophila' if kind.startswith('drosophila') else kind
    height={'celegans':.038,'drosophila':.028,'zebrafish':.14,'hexapod':.19,'nao':.34}[robot_kind]
    args=' '.join(json.dumps(a) for a in (controller_args or ['NM_BRAINS=default']))
    rotation='1 0 0 1.570796' if robot_kind in {'celegans','drosophila'} else '0 0 1 0'
    physics = 'basicTimeStep 32'
    if robot_kind == 'zebrafish':
        physics += ''' gravity 2.2 contactProperties [ ContactProperties {
          material1 "water_body" coulombFriction 0.05 0.05 0.05 bounce 0.1 bounceVelocity 0.02
        } ]'''
    # Preserve the existing reduced-gravity fish approximation; no CFD is claimed.
    # Webots default camera looks down local -Z; rotation about X gives an oblique Z-up view.
    return f'''#VRML_SIM R2025a utf8
# Generated shared habitat; source sim/content/catalog.json; {data['digest']}
EXTERNPROTO "{proto_ref}"
WorldInfo {{ {physics} }}
Viewpoint {{ orientation 1 0 0 0.64 position 0 {-radius*2:.5f} {radius*1.6:.5f} }}
Background {{ skyColor [ 0.12 0.17 0.2 ] }}
DirectionalLight {{ direction -0.35 -0.5 -1 intensity 1 ambientIntensity 0.35 castShadows TRUE }}
{webots_habitats([(proto_name,kind,'default',height,robot_kind)],[(0,0)])}
{proto_name} {{ translation 0 0 {height} rotation {rotation} controller "nao_nn_controller_uds" controllerArgs [ {args} ] }}
'''


def worm_segment_details(segment, length, radius):
    """Detail attached to each existing ODE segment, without mass or extra devices."""
    source=robot_parts('worm'); authored=23-segment; x=.5-authored/23
    items=[]
    for item in source:
        if item['anchor'] != f'segment_{authored:02}' or item['id'].startswith('cuticle_'): continue
        item=dict(item); item['position']=list(item['position']); item['position'][0]-=x
        # Rebase the idealised axial pitch to the measured existing segment radius.
        item['size']=list(item['size']); item['position'][1]*=radius/(.053*length)
        item['position'][2]*=radius/(.053*length)
        items.append(item)
    return webots_nodes(items,length,frame='yup',detail=True,include_internal=True)


def surface_details(kind):
    """Shared surface landmarks supplement the established articulated Webots rig."""
    prefixes={'fly':('ommatidium_',),'fish':('neuromast_',),'hexapod':('cable_',)}[kind]
    parts=[p for p in robot_parts(kind) if p['id'].startswith(prefixes)]
    return parts


if __name__ == "__main__": main()
