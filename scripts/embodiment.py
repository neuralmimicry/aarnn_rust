#!/usr/bin/env python3
"""Versioned mechanical compiler, separate from illustrative habitat geometry.

Upstream MJCF bytes are never edited. Derived worlds, ordered IO and model
records have explicit hashes. Run with the pinned MuJoCo environment.
"""
from __future__ import annotations
import argparse
import copy
import hashlib
import json
import math
import struct
from pathlib import Path
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
PACK = ROOT / '.agent/robot-expansion'
OUT = ROOT / 'sim/content/embodiments/generated'
POLICY = ROOT / 'sim/content/embodiments/policy.json'
FIRST = ('galbot_g1_golf', 'ugot_mecanum', 'crow_corvus_corone')
from embodiment_legacy import LEGACY, projection as legacy_projection
NEW = tuple(r['id'] for r in json.loads((PACK / 'manifests/robots.json').read_text())['robots'])


def digest(value):
    """Model digest encoding v1: typed scalars, sorted UTF-8 keys, LE lengths.

    Floating-point formatting is deliberately absent; Rust and Python fixtures
    hash identical IEEE-754 bytes. This is not the neural StateDigest format.
    """
    def encode(item):
        if item is None:return b'n'
        if isinstance(item,bool):return b't' if item else b'f'
        if isinstance(item,int):return b'i'+str(item).encode()+b';'
        if isinstance(item,float):
            if not math.isfinite(item):raise ValueError('nonfinite model scalar')
            return b'd'+struct.pack('<d',item)
        if isinstance(item,str):
            raw=item.encode();return b's'+struct.pack('<Q',len(raw))+raw
        if isinstance(item,list):return b'a'+struct.pack('<Q',len(item))+b''.join(encode(x) for x in item)
        if isinstance(item,dict):return b'o'+struct.pack('<Q',len(item))+b''.join(encode(k)+encode(item[k]) for k in sorted(item))
        raise TypeError('unsupported model digest value')
    return hashlib.sha256(encode(value)).hexdigest()


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def element(parent, tag, **attrs):
    def val(v):
        if isinstance(v, (tuple, list)):
            return ' '.join(str(float(x)) for x in v)
        return str(v)
    return ET.SubElement(parent, tag, {k: val(v) for k, v in attrs.items()})


def source_model(profile):
    entries = {
        'galbot_g1_golf': 'assets/galbot-g1-golf/mjcf/galbot_one_golf.xml',
        'ugot_mecanum': 'assets/ugot-mecanum/model/ugot_mecanum.xml',
    }
    return PACK / entries[profile] if profile in entries else None


def verify_sources():
    lock = json.loads((PACK / 'manifests/assets.lock.json').read_text())
    for item in lock['assets']:
        path = PACK / item['path']
        if path.stat().st_size != item['size_bytes'] or sha(path) != item['sha256']:
            raise ValueError(f'pinned source mismatch: {path}')
    return len(lock['assets'])


def base_world(profile):
    root = ET.Element('mujoco', model=profile)
    element(root, 'compiler', angle='radian', autolimits='true')
    element(root, 'option', timestep=.002, gravity=[0, 0, -9.81], integrator='implicitfast')
    default = element(root, 'default')
    element(default, 'joint', damping=.05, armature=.0001)
    element(default, 'geom', friction=[.8, .01, .001], condim=3)
    world = element(root, 'worldbody')
    element(world, 'light', pos=[0, 0, 4], dir=[0, 0, -1])
    element(world, 'geom', name='ground', type='plane', size=[10, 10, .1], rgba=[.3, .35, .3, 1])
    # Ordinary collidable scenery. Neither supports the root secretly.
    return root, world


def make_crow():
    root, world = base_world('crow_corvus_corone')
    body = element(world, 'body', name='torso', pos=[0, 0, .25])
    element(body, 'freejoint', name='root')
    element(body, 'geom', name='torso_skin', type='ellipsoid', size=[.12, .055, .065], mass=.292, rgba=[.025, .025, .03, 1])
    actuators = element(root, 'actuator')
    joints = json.loads((PACK / 'spec/crow-io.proposed.json').read_text())['controlled_joints']
    bodies = {}
    # Joint chains preserve independent wing/leg synergies. Tiny proximal
    # bodies have explicit masses; each rigid part receives mass only once.
    def chain(parent, names, positions, sizes, masses, axes):
        for name, pos, size, mass, axis in zip(names, positions, sizes, masses, axes):
            child = element(parent, 'body', name=name + '_link', pos=pos)
            element(child, 'joint', name=name, axis=axis, range=[-1.5, 1.5])
            element(child, 'geom', name=name + '_geom', type='box', size=size, mass=mass, rgba=[.03, .03, .035, 1])
            element(actuators, 'motor', name=name, joint=name, ctrlrange=[-1, 1], gear=.15 if 'wing' in name else .06)
            bodies[name] = child
            parent = child
        return parent
    neck = chain(body, joints[:4], [[.11, 0, .035], [0, 0, 0], [0, 0, 0], [.04, 0, -.01]],
                 [[.012]*3, [.012]*3, [.034, .025, .027], [.035, .012, .006]], [.005, .005, .04, .005],
                 [[0, 0, 1], [0, 1, 0], [1, 0, 0], [0, 1, 0]])
    for sign, start in [(1, 4), (-1, 10)]:
        wing = chain(body, joints[start:start+6], [[0, sign*.045, .025], [0, 0, 0], [0, 0, 0], [0, sign*.14, 0], [0, sign*.14, 0], [0, 0, 0]],
                     [[.012]*3, [.012]*3, [.055, .07, .004], [.05, .07, .003], [.012]*3, [.045, .08, .002]],
                     [.002, .002, .02, .015, .002, .009],
                     [[1, 0, 0], [0, 0, 1], [0, 1, 0], [1, 0, 0], [0, 0, 1], [0, 1, 0]])
        element(wing, 'site', name=('left' if sign == 1 else 'right')+'_wing_panel', pos=[0, sign*.06, 0], size=.005)
    chain(body, joints[16:19], [[-.11, 0, 0], [0, 0, 0], [0, 0, 0]], [[.008]*3, [.008]*3, [.065, .045, .003]], [.002, .002, .011], [[0, 1, 0], [0, 0, 1], [1, 0, 0]])
    for sign, start, side in [(1, 19, 'left'), (-1, 25, 'right')]:
        chain(body, joints[start:start+6], [[-.02, sign*.035, -.04], [0, 0, 0], [.025, 0, -.035], [-.01, 0, -.045], [.02, 0, -.012], [-.02, 0, 0]],
              [[.008]*3, [.008, .008, .02], [.008, .008, .025], [.018, .009, .006], [.012, .012, .003], [.012, .003, .003]],
              [.002, .003, .006, .006, .003, .0025], [[0, 1, 0], [1, 0, 0], [0, 1, 0], [0, 1, 0], [0, 1, 0], [0, 1, 0]])
        toes = bodies[joints[start+4]]
        for i in range(3):
            element(toes, 'geom', name=f'{side}_toe{i+1}', type='capsule', fromto=[0, (i-1)*.01, 0, .035, (i-1)*.014, 0], size=.002, mass=.0005)
    # Sum of explicit link masses = 0.510 kg including all eight toes.
    for side, yaw in [('left', .45), ('right', -.45)]:
        element(bodies['neck_roll'], 'camera', name='retina_'+side, pos=[.03, 0, .01], xyaxes=[0, -1, 0, 0, 0, 1], fovy=75)
    # Synergy bodies overlap at shared pivots. Disable intra-bird collision in
    # this reduced model explicitly; ground, toes, obstacles and external bodies
    # still collide. Self-collision calibration is an open fidelity gate.
    for geom in body.findall('.//geom'):
        geom.set('contype', '2'); geom.set('conaffinity', '1')
    return root


def prepare_source(profile):
    source = source_model(profile)
    if not source:
        if profile in LEGACY:
            from embodiment_legacy import build
            return build(profile,element,base_world)
        if profile == 'crow_corvus_corone': return make_crow()
        from embodiment_archetypes import build
        descriptions = {r['id']: r for r in json.loads((PACK/'manifests/robots.json').read_text())['robots']}
        if profile not in descriptions: raise ValueError('unknown embodiment profile: '+profile)
        return build(profile, descriptions[profile], element, base_world)
    root = ET.parse(source).getroot()
    # Model dependencies remain immutable; relative references resolve from the
    # generated world, not cwd. This is a reproducible import, not a new asset.
    import os
    for item in root.findall('.//*[@file]'):
        item.set('file', os.path.relpath(source.parent / item.get('file'), OUT))
    compiler = root.find('compiler')
    if compiler is not None:
        compiler.set('meshdir', '.')
    option = root.find('option')
    if option is None:
        option = element(root, 'option')
    option.set('timestep', '.002')
    option.set('gravity', '0 0 -9.81')
    world = root.find('worldbody')
    from embodiment_scene import geom_kind
    if not any(geom_kind(root,g) == 'plane' or g.get('name') == 'platform' for g in world.findall('geom')):
        element(world, 'geom', name='ground', type='plane', size=[10, 10, .1], friction=[.8, .01, .001])
    return root


def compile_profile(profile, root):
    import mujoco
    from embodiment_anatomy import install as install_anatomy
    from embodiment_scene import install
    install_anatomy(profile,root,element)
    scene=install(profile,root,element)
    xml = ET.tostring(root, encoding='unicode')
    # Assets use paths relative to OUT, so a path-based compile is intentional.
    OUT.mkdir(parents=True, exist_ok=True)
    path = OUT / (profile + '.xml')
    # Temporary source never replaces an immutable brain or checkpoint.
    import tempfile
    with tempfile.NamedTemporaryFile(mode='w', suffix='.xml', dir=OUT) as temp:
        temp.write(xml); temp.flush()
        model = mujoco.MjModel.from_xml_path(temp.name)
    data = mujoco.MjData(model)
    mujoco.mj_forward(model, data)
    motors = []
    sensors = []
    for aid in range(model.nu):
        jid = int(model.actuator_trnid[aid, 0])
        name = mujoco.mj_id2name(model, mujoco.mjtObj.mjOBJ_ACTUATOR, aid)
        joint = mujoco.mj_id2name(model, mujoco.mjtObj.mjOBJ_JOINT, jid)
        bounds = model.actuator_ctrlrange[aid].tolist()
        motors.append(dict(name=name, joint=joint, control_range=bounds,
                           effort_range=model.actuator_forcerange[aid].tolist(),
                           gear=model.actuator_gear[aid].tolist(),
                           neutral=0, fault='zero-active-effort', encoding='antagonist-rate-v1'))
        for measure, units, limits in [('angle', 'rad', [-math.pi, math.pi]), ('angular_velocity', 'rad/s', [-40, 40])]:
            sensors.append(dict(name=f'{joint}_{measure}', kind=measure, joint=joint, units=units, range=limits, rate_hz=500, missing='fault-disarm'))
    if profile != 'crow_corvus_corone':
        sensors.extend(dict(name=f'proximity_{i}',kind='range_ray',units='m',range=[0,3],rate_hz=500,missing='fault-disarm',angle_rad=2*math.pi*i/8) for i in range(8))
        sensors.extend(dict(name=f'body_gyro_{axis}',kind='body_gyro',axis=i,units='rad/s',range=[-40,40],rate_hz=500,missing='fault-disarm') for i,axis in enumerate('xyz'))
    output_names = [m['name']+suffix for m in motors for suffix in ('_positive', '_negative')]
    if profile in LEGACY:
        original_sensors, original_outputs = legacy_projection(profile)
        sensors = [dict(name=n,kind='legacy-projection-unresolved',units='legacy-normalised',range=[0,1],rate_hz=500,missing='unavailable-disarm') for n in original_sensors]
        output_names = original_outputs
    if profile == 'celegans':
        from embodiment_legacy import celegans_sensors
        sensors = celegans_sensors()
        for motor in motors: motor['encoding']='celegans-muscle-v2-sim-v1'
    from embodiment_neural import PROFILES as NEURAL_LEGACY, VERSION as LEGACY_VERSION
    if profile in NEURAL_LEGACY:
        from embodiment_neural import sensors as legacy_sensors, readout, neural_names
        sensors=legacy_sensors(profile)
        for motor in motors:
            motor.update(encoding=LEGACY_VERSION,readout=readout(profile,motor['joint']),position_readout=True)
        for i,motor in enumerate(motors):motor['velocity_limit']=float(model.actuator_user[i,0])
    if profile == 'crow_corvus_corone':
        proposal = json.loads((PACK / 'spec/crow-io.proposed.json').read_text())
        output_names = proposal['output_names']
        known = {s['name']: s for s in sensors}
        sensors = [known.get(n, dict(name=n, kind='projection', units='normalised', range=[0, 1], rate_hz=500, missing='unavailable-disarm')) for n in proposal['sensor_names']]
    io = dict(schema_version=1, digest_encoding='embodiment-binary-v1', mapping_version=profile+'-io-v1', sensors=sensors, output_names=output_names,
              sensory_base=4096, output_base=16384, dt_us=2000,
              transducer=dict(version='antagonist-rate-v1', tau_s=.02, stale_s=.1, neutral='zero effort; passive dynamics continue'))
    io['digest'] = digest(io)
    if profile in NEURAL_LEGACY:
        io['neural_sensory_names'],io['neural_output_names']=neural_names(profile)
        io['mapping_version']=LEGACY_VERSION+'/'+profile
        io['transducer']=dict(version=LEGACY_VERSION,tau_s=.02,stale_s=.1,neutral='zero active drive on empty history; no twitch or oscillator',fidelity='versioned engineering projection; no learned task or biological calibration claim')
        io['digest']=digest({k:v for k,v in io.items() if k!='digest'})
    if profile == 'celegans':
        io['mapping_version']='celegans-simulation-proxies-v1'
        io['transducer']=dict(version='celegans-muscle-v2-sim-v1',muscle_tau_s=.055,spine_tau_s=.080,neural_gain=3,stale_s=.1,neutral='zero active effort; no twitch fallback',calibration='original functional surrogate; not measured biology or Webots physics parity')
        io['digest']=digest({k:v for k,v in io.items() if k!='digest'})
    source = source_model(profile)
    record = dict(schema_version=1, digest_encoding='embodiment-binary-v1', compiler='embodiment-v1', profile_id=profile,
                  variant_id='tapered-cuticle-v2' if profile=='celegans' else 'wheeled' if profile == 'galbot_g1_golf' else 'community-estimate' if source else 'reduced-31-v1' if profile == 'crow_corvus_corone' else 'lec_brochure' if profile == 'xbot_s_pro' else 'manufacturer_640mm_depth' if profile == 'keenon_s100' else 'without_squeegee' if profile == 'keenon_c40' else 'original-v1',
                  fidelity='official-description' if profile == 'galbot_g1_golf' else 'functional-surrogate',
                  exact_replica='pending OEM variant and calibration',
                  controller_transfer='celegans-muscle-v2-sim-v1; uncalibrated sensory proxies and mechanics' if profile=='celegans' else LEGACY_VERSION+'; native MuJoCo engineering adapter; calibration unvalidated' if profile in NEURAL_LEGACY else 'untrained',
                  source=str(source.relative_to(ROOT)) if source else 'scripts/embodiment.py:make_crow' if profile == 'crow_corvus_corone' else 'scripts/embodiment_legacy.py:build' if profile in LEGACY else 'scripts/embodiment_archetypes.py:build',
                  source_digest=sha(source) if source else digest([sha(Path(__file__)), sha(ROOT/'scripts/embodiment_archetypes.py'), sha(ROOT/'scripts/embodiment_legacy.py'), sha(PACK/'manifests/robots.json')]),
                  policy_digest=sha(POLICY), io=io, actuators=motors,
                  native_mjcf_sha256=hashlib.sha256((xml+'\n').encode()).hexdigest(),
                  source_licence='Apache-2.0' if profile == 'galbot_g1_golf' else 'MIT' if source else 'repository-original',
                  assumptions='sim/content/embodiments/policy.json',
                  total_mass_kg=float(sum(model.body_mass)),
                  scene=scene, scene_sources_sha256=digest([sha(ROOT/'scripts/embodiment_scene.py'),sha(ROOT/'scripts/sim_content.py'),sha(ROOT/'sim/content/catalog.json')]),
                  anatomy_sources_sha256=sha(ROOT/'scripts/embodiment_anatomy.py'),
                  neural_sources_sha256=sha(ROOT/'scripts/embodiment_neural.py'),
                  bodies=[], joints=[], geoms=[], constraints=[dict(tag=e.tag, **e.attrib) for e in root.findall('./equality/*')])
    for bid in range(1, model.nbody):
        record['bodies'].append(dict(name=mujoco.mj_id2name(model, mujoco.mjtObj.mjOBJ_BODY, bid), parent=int(model.body_parentid[bid]),
                                     position=model.body_pos[bid].tolist(), quaternion_wxyz=model.body_quat[bid].tolist(),
                                     world_position=data.xpos[bid].tolist(), world_quaternion_wxyz=data.xquat[bid].tolist(),
                                     mass_kg=float(model.body_mass[bid]), com_m=model.body_ipos[bid].tolist(),
                                     inertia_orientation_wxyz=model.body_iquat[bid].tolist(), diagonal_inertia_kgm2=model.body_inertia[bid].tolist()))
    for jid in range(model.njnt):
        record['joints'].append(dict(name=mujoco.mj_id2name(model, mujoco.mjtObj.mjOBJ_JOINT, jid), body=int(model.jnt_bodyid[jid]), type=int(model.jnt_type[jid]),
                                     position=model.jnt_pos[jid].tolist(), axis=model.jnt_axis[jid].tolist(), range=model.jnt_range[jid].tolist(), limited=bool(model.jnt_limited[jid])))
    for gid in range(model.ngeom):
        record['geoms'].append(dict(name=mujoco.mj_id2name(model, mujoco.mjtObj.mjOBJ_GEOM, gid), body=int(model.geom_bodyid[gid]), type=int(model.geom_type[gid]),
                                    position=model.geom_pos[gid].tolist(), quaternion_wxyz=model.geom_quat[gid].tolist(), half_size_m=model.geom_size[gid].tolist(),
                                    rgba=(scene['substrate']+[1]) if int(model.geom_type[gid])==0 else model.geom_rgba[gid].tolist(), collision=bool(model.geom_contype[gid] or model.geom_conaffinity[gid]), mesh_id=int(model.geom_dataid[gid])))
    record['digest'] = digest(record)
    if profile == 'celegans':
        assumptions = ROOT/'sim/content/embodiments/celegans-simulation-v2.json'
        record['profile_assumptions']=str(assumptions.relative_to(ROOT))
        record['profile_assumptions_sha256']=sha(assumptions)
        record['digest']=digest({k:v for k,v in record.items() if k!='digest'})
    if profile in NEURAL_LEGACY:
        assumptions=ROOT/'sim/content/embodiments/legacy-native-v1.json'
        record['profile_assumptions']=str(assumptions.relative_to(ROOT));record['profile_assumptions_sha256']=sha(assumptions)
        record['neural_targets']=['mujoco']
        record['digest']=digest({k:v for k,v in record.items() if k!='digest'})
    return xml, record


def products(profiles=FIRST+tuple(p for p in NEW+LEGACY if p not in FIRST)):
    verify_sources()
    result = {}
    for profile in profiles:
        xml, record = compile_profile(profile, prepare_source(profile))
        result[OUT / (profile+'.xml')] = xml+'\n'
        result[OUT / (profile+'.json')] = json.dumps(record, indent=2, allow_nan=False)+'\n'
        # Fresh untrained recipe only; no existing snapshot is imported/resized.
        config = dict(num_sensory_neurons=len(record['io']['sensors']), num_output_neurons=len(record['io']['output_names']),
                      num_hidden_layers=2, num_hidden_per_layer_initial=32, max_total_neurons=4096,
                      p_in=.15, p_hidden=.15, p_out=.15)
        result[OUT / (profile+'.config.json')] = json.dumps(config,indent=2)+'\n'
    # One generated registry is consumed by Rust and the compatibility Python
    # launcher. Existing neural files and aliases are read, never rewritten.
    from robot_profiles import PROFILES
    entries=[]
    for profile in NEW+LEGACY:
        path=OUT/(profile+'.json')
        encoded=result.get(path)
        record=json.loads(encoded) if encoded else json.loads(path.read_text()) if path.exists() else None
        if record:
            old=PROFILES.get(profile)
            entries.append(dict(id=profile,aliases=list(old.aliases) if old else [profile],sensory=len(record['io']['sensors']),output=len(record['io']['output_names']),
                                model=profile+'.xml',record=profile+'.json',config=profile+'.config.json',network=old.network_rel if profile in LEGACY else '',io_digest=record['io']['digest'],model_digest=record['digest']))
    result[OUT/'registry.generated.json']=json.dumps(dict(schema_version=1,profiles=entries),indent=2)+'\n'
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--profiles', nargs='+', default=list(FIRST + tuple(p for p in NEW+LEGACY if p not in FIRST)))
    args = parser.parse_args()
    stale = []
    for path, value in products(args.profiles).items():
        if args.check:
            if not path.exists() or path.read_text() != value: stale.append(str(path.relative_to(ROOT)))
        else:
            path.parent.mkdir(parents=True, exist_ok=True); path.write_text(value)
    if stale: raise SystemExit('stale embodiments: '+', '.join(stale))
    print('Embodiments verified' if args.check else 'Embodiments generated')

if __name__ == '__main__': main()
