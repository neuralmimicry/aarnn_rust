#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
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
BALANCING = ('ugot_balance', 'ugot_wheelleg')
from embodiment_legacy import LEGACY, projection as legacy_projection
NEW = tuple(r['id'] for r in json.loads((PACK / 'manifests/robots.json').read_text())['robots'])
# Bodies designed in this repository, each compiled from its own spec module
# (scripts/embodiment_salamander.py); not part of the LEC research manifest.
from embodiment_salamander import PROFILE as SALAMANDER, ALIASES as SALAMANDER_ALIASES
DESIGNED = (SALAMANDER,)


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


# Reduced crow flight mechanics v2. A programmed-control feasibility study
# (quasi-steady panels, 20 ms transducer, frame latency) found v1 unable to
# fly: 0.15 N m shoulders let the lift fold the wings to the joint stop (the
# tip panel then acted 0.3 m above the centre of mass and pitched the bird
# nose-up into a stall), and the flat-plate polar (L/D < 4) cannot be flapped
# to level flight within 25 W. Changes and reasons:
# * shoulder flap 1.5 N m: half the weight at mid-span is ~0.5 N m, stroke
#   reversal a further ~0.6 N m (pectoralis ~1-2 N m for a 0.5 kg bird);
#   sweep 0.4 and elbow 0.6 N m hold drag and outer-wing lift; twist 0.4 N m;
# * series elasticity: a shoulder spring (0.79 N m/rad, rest 0.245 rad below
#   horizontal) resonates the wing near 4 Hz and carries the mean lift, so
#   the motors supply aerodynamic rather than inertial work (the |tau w|
#   power budget charges braking as metabolic cost);
# * passive impedance on the structural wing, twist and tail joints
#   (co-contracted muscle and ligaments): the brain's 20 ms transducer
#   cannot stiffen them against stroke loads; the brain sets their angles by
#   torque against the spring (equilibrium-point control);
# * legs 0.3 N m: 0.06 N m let the standing crow fold onto its breast;
# * wing joint damping 0.005 N m s/rad (0.05 dissipated ~12 W at 4 Hz).
CROW_FLIGHT = dict(
    # Released 3 m up near the south-west corner of the field, heading across
    # its 113 m diagonal (yaw 45 deg) at the 10.5 m/s cruise speed.
    spawn_m=[-34.0, -34.0, 3.0],
    spawn_quat=[0.9238795325112867, 0.0, 0.0, 0.3826834323650898],
    launch_mps=[7.424621202458749, 7.424621202458749, 0.0],
    gear=dict(shoulder_flap=1.5, shoulder_sweep=.4, humerus_twist=.4, elbow_flex=.6, wrist_fold=.15, wrist_twist=.2,
              leg=.3, neck_yaw=.06, neck_pitch=.06, neck_roll=.06, beak_gape=.06, tail_pitch=.15, tail_yaw=.06, tail_fan=.06),
    # (stiffness N m/rad, damping N m s/rad, spring rest rad; right side mirrored)
    impedance=dict(shoulder_flap=(.787, .005, -.245), shoulder_sweep=(5., .08, 0.), humerus_twist=(1., .01, 0.),
                   elbow_flex=(9., .06, 0.), wrist_fold=(1., .01, 0.), wrist_twist=(.5, .006, 0.),
                   tail_pitch=(.3, .004, 0.), tail_yaw=(.3, .004, 0.), tail_fan=(.3, .004, 0.)),
)


def crow_flight_scene():
    """Engine-independent air model and launch of the crow (scene record).

    ``quasi-steady-panel-v2``: per panel, in its own frame (X chord, Y span,
    Z normal), ``CL = clamp(a alpha, +-CLmax) cos alpha`` and
    ``CD = CD0 + k CL^2 + c_s sin^2(max(|alpha| - alpha_stall, 0))`` at the
    relative wind of the panel point, lift perpendicular to it
    (``aarnn_embodiment::force::polar_panel_force``). The wing polar is an
    attached-flow bird wing (a = 4.6/rad for aspect ratio 6.3, k = 1/(pi 0.8 AR)
    = 0.063, CD0 0.025; L/D ~9 with body drag) instead of v1's flat plate (L/D < 4).
    Each wing has three panels (inner, mid, outer) at the centres of its
    boxes; the tail fan panel, a vertical tail-base/rump panel (the folded
    tail and undertail area: directional stability, without which a banked
    crow sideslipped into a spin) and isotropic body drag (CD 0.25 on the
    torso frontal area) complete the set. Quasi-steady: no added mass, wake,
    delayed stall or ground effect; hovering and take-off from standstill
    are outside the model.
    """
    panels = []
    for side, s in (('left', 1), ('right', -1)):
        for link, y, area in (('humerus_twist', .07, .0263), ('elbow_flex', .07, .0245), ('wrist_twist', .08, .024)):
            panels.append(dict(body=f'{side}_wing_{link}_link', point_m=[0., s * y, 0.], area_m2=area, polar='wing'))
    panels += [dict(body='tail_fan_link', point_m=[-.035, 0., 0.], area_m2=.0143, polar='tail'),
               dict(body='tail_pitch_link', point_m=[-.06, 0., 0.], area_m2=.012, polar='keel',
                    quaternion_wxyz=[math.sqrt(.5), -math.sqrt(.5), 0., 0.]),
               dict(body='torso', point_m=[0., 0., 0.], area_m2=.01123, polar='body')]
    polars = dict(wing=dict(lift_slope_per_rad=4.6, lift_max=1.5, drag_zero=.025, drag_induced=.063, drag_stall=1.2),
                  tail=dict(lift_slope_per_rad=3., lift_max=1.2, drag_zero=.03, drag_induced=.1, drag_stall=1.2),
                  keel=dict(lift_slope_per_rad=3., lift_max=1., drag_zero=.02, drag_induced=.1, drag_stall=1.2),
                  body=dict(lift_slope_per_rad=0., lift_max=0., drag_zero=.25, drag_induced=0., drag_stall=0.))
    return dict(aerodynamics=dict(model='quasi-steady-panel-v2', density_kg_m3=1.225, polars=polars, panels=panels,
                                  note='quasi-steady blade elements; no unsteady lift, added mass or wake; '
                                       'polar is an engineering estimate, not a measured crow wing'),
                launch=dict(root_linear_velocity_mps=CROW_FLIGHT['launch_mps'],
                            note='released in level flight from the spawn pose; engines set the free root velocity at load and reset'))


def make_crow():
    root, world = base_world('crow_corvus_corone')
    # The ground plane is infinite for contact; draw it over the whole field,
    # lit by a sun rather than the 4 m spotlight over the origin.
    world.find("geom[@name='ground']").set('size', '45 45 .1')
    sun = world.find('light')
    sun.set('directional', 'true'); sun.set('dir', '0.3 0.2 -1'); sun.set('diffuse', '0.8 0.8 0.75')
    # Flight mechanics v2 (see CROW_FLIGHT and crow_flight_scene above). The
    # crow is released in level flight from the spawn pose with the scene's
    # launch velocity.
    body = element(world, 'body', name='torso', pos=CROW_FLIGHT['spawn_m'], quat=CROW_FLIGHT['spawn_quat'])
    element(body, 'freejoint', name='root')
    # Wing and tail mass moved to the torso (flight muscles sit on the
    # keel): each wing ~6.7 % of body mass, most of it proximal.
    element(body, 'geom', name='torso_skin', type='ellipsoid', size=[.12, .055, .065], mass=.324, rgba=[.025, .025, .03, 1])
    actuators = element(root, 'actuator')
    joints = json.loads((PACK / 'spec/crow-io.proposed.json').read_text())['controlled_joints']
    bodies = {}
    # Joint chains preserve independent wing/leg synergies. Tiny proximal
    # bodies have explicit masses; each rigid part receives mass only once.
    def chain(parent, names, positions, sizes, masses, axes, geom_pos=None):
        for i, (name, pos, size, mass, axis) in enumerate(zip(names, positions, sizes, masses, axes)):
            child = element(parent, 'body', name=name + '_link', pos=pos)
            key = name.split('_', 2)[-1] if '_wing_' in name else 'leg' if '_leg_' in name else name
            stiffness, damping, springref = CROW_FLIGHT['impedance'].get(key, (0., None, 0.))
            joint = dict(name=name, axis=axis, range=[-1.5, 1.5])
            if stiffness:
                # Intrinsic stiffness of co-contracted muscle and ligaments
                # (passive, integrated by the engine): the brain's 20 ms
                # transducer cannot stiffen a wing against 4 Hz stroke loads.
                joint.update(stiffness=stiffness, springref=springref * (-1 if name.startswith('right_') else 1))
            if damping is not None:
                joint.update(damping=damping)
            element(child, 'joint', **joint)
            element(child, 'geom', name=name + '_geom', type='box', size=size, mass=mass, rgba=[.03, .03, .035, 1],
                    **({'pos': geom_pos[i]} if geom_pos and geom_pos[i] else {}))
            element(actuators, 'motor', name=name, joint=name, ctrlrange=[-1, 1], gear=CROW_FLIGHT['gear'].get(key, .06))
            bodies[name] = child
            parent = child
        return parent
    neck = chain(body, joints[:4], [[.11, 0, .035], [0, 0, 0], [0, 0, 0], [.04, 0, -.01]],
                 [[.012]*3, [.012]*3, [.034, .025, .027], [.035, .012, .006]], [.005, .005, .04, .005],
                 [[0, 0, 1], [0, 1, 0], [1, 0, 0], [0, 1, 0]])
    for sign, start in [(1, 4), (-1, 10)]:
        # Wing boxes span their own segment (shoulder-elbow, elbow-wrist,
        # wrist-tip) with a corvid's broad chord: span 0.97 m, area 0.15 m^2
        # (wing loading ~33 N/m^2). The quasi-steady panels sit at the box
        # centres, on each twist axis, so twist joints feel no aerodynamic
        # moment and the shoulder carries the lift at mid-span, not the tip.
        wing = chain(body, joints[start:start+6], [[0, sign*.045, .025], [0, 0, 0], [0, 0, 0], [0, sign*.14, 0], [0, sign*.14, 0], [0, 0, 0]],
                     [[.012]*3, [.012]*3, [.094, .07, .004], [.0875, .07, .003], [.012]*3, [.075, .08, .002]],
                     [.002, .002, .016, .008, .002, .004],
                     [[1, 0, 0], [0, 0, 1], [0, 1, 0], [1, 0, 0], [0, 0, 1], [0, 1, 0]],
                     geom_pos=[None, None, [0, sign*.07, 0], [0, sign*.07, 0], None, [0, sign*.08, 0]])
        element(wing, 'site', name=('left' if sign == 1 else 'right')+'_wing_panel', pos=[0, sign*.08, 0], size=.005)
    # Tail hinged 2 cm further aft (rump); fan 0.013 x 0.011 m panel area.
    chain(body, joints[16:19], [[-.13, 0, 0], [0, 0, 0], [0, 0, 0]], [[.008]*3, [.008]*3, [.065, .055, .003]], [.002, .002, .011],
          [[0, 1, 0], [0, 0, 1], [1, 0, 0]], geom_pos=[None, None, [-.035, 0, 0]])
    for sign, start, side in [(1, 19, 'left'), (-1, 25, 'right')]:
        chain(body, joints[start:start+6], [[-.02, sign*.035, -.04], [0, 0, 0], [.025, 0, -.035], [-.01, 0, -.045], [.02, 0, -.012], [-.02, 0, 0]],
              [[.008]*3, [.008, .008, .02], [.008, .008, .025], [.018, .009, .006], [.012, .012, .003], [.012, .003, .003]],
              [.002, .003, .006, .006, .003, .0025], [[0, 1, 0], [1, 0, 0], [0, 1, 0], [0, 1, 0], [0, 1, 0], [0, 1, 0]])
        toes = bodies[joints[start+4]]
        for i in range(3):
            element(toes, 'geom', name=f'{side}_toe{i+1}', type='capsule', fromto=[0, (i-1)*.01, 0, .035, (i-1)*.014, 0], size=.002, mass=.0005)
    # Sum of explicit link masses = 0.510 kg including all eight toes.
    # Lateral eyes, one on each side of the head, each axis 0.45 rad (26 deg)
    # outward: with ~91 deg horizontal per eye (fovy 75, 4:3) the fields
    # overlap ~40 deg in front, the corvid binocular field (~37-40 deg; v1
    # mounted both retinas at one point looking straight ahead, so the two
    # 96-cell retinas saw the same image).
    for side, yaw in [('left', .45), ('right', -.45)]:
        sign = 1 if side == 'left' else -1
        element(bodies['neck_roll'], 'camera', name='retina_'+side, pos=[.03, sign*.012, .01],
                xyaxes=[round(math.sin(yaw), 6), round(-math.cos(yaw), 6), 0, 0, 0, 1], fovy=75)
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
        if profile == SALAMANDER:
            from embodiment_salamander import build
            return build(element, base_world)
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
    if profile == 'crow_corvus_corone':
        scene.update(crow_flight_scene())
        scene['scientific_status'] += '; quasi-steady panel aerodynamics (engineering air model, not CFD)'
    elif profile.startswith('drosophila'):
        from embodiment_legacy import fly_aerodynamics
        scene.update(fly_aerodynamics())
        scene['scientific_status'] += '; quasi-steady wing-blade aerodynamics (engineering air model, not CFD)'
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
        # Balancing bases set their IMU to posture scale (+-8 rad/s, +-0.5 g):
        # a +-40 rad/s gyro moves a pulse-coded channel by 1/80 per rad/s.
        gyro_range=[-8,8] if profile in BALANCING else [-40,40]
        sensors.extend(dict(name=f'body_gyro_{axis}',kind='body_gyro',axis=i,units='rad/s',range=gyro_range,rate_hz=500,missing='fault-disarm') for i,axis in enumerate('xyz'))
        if profile in BALANCING:
            # Self-balancing bases carry an accelerometer in their IMU: the
            # gravity direction in body axes (the vestibular otolith signal).
            # A gyro alone gives no absolute tilt reference (integration drifts).
            sensors.extend(dict(name=f'body_gravity_{axis}',kind='body_gravity',axis=i,units='g',range=[-.5,.5],rate_hz=500,missing='fault-disarm') for i,axis in enumerate('xyz'))
    output_names = [m['name']+suffix for m in motors for suffix in ('_positive', '_negative')]
    if profile in LEGACY:
        original_sensors, original_outputs = legacy_projection(profile)
        sensors = [dict(name=n,kind='legacy-projection-unresolved',units='legacy-normalised',range=[0,1],rate_hz=500,missing='unavailable-disarm') for n in original_sensors]
        output_names = original_outputs
    if profile == 'celegans':
        from embodiment_legacy import celegans_sensors
        sensors = celegans_sensors()
        for motor in motors: motor['encoding']='celegans-muscle-v3-sim-v1'
    from embodiment_neural import PROFILES as NEURAL_LEGACY, VERSION as LEGACY_VERSION
    if profile in NEURAL_LEGACY:
        from embodiment_neural import sensors as legacy_sensors, readout, neural_names
        sensors=legacy_sensors(profile)
        for motor in motors:
            motor.update(encoding=LEGACY_VERSION,readout=readout(profile,motor['joint']),position_readout=True)
        for i,motor in enumerate(motors):motor['velocity_limit']=float(model.actuator_user[i,0])
        if profile=='nao':
            # Servo wake-up: targets blend from the rest (reference) pose to the
            # brain's posture over 1 s after arming, as NAO interpolates on
            # wake-up; a graded brain's silent first frames would otherwise
            # command every joint to its lower limit.
            for i,motor in enumerate(motors):
                jid=int(model.actuator_trnid[i,0])
                motor['wake']={'from':float(model.qpos0[model.jnt_qposadr[jid]]),'seconds':1.0}
                # Servo low-pass (200 ms): at the generic 20 ms decode the
                # pulse-density ripple (+-5 % of a joint's range, +-0.1 rad on
                # the knee) topples the biped within ~4 s; at 100 ms the
                # residual ripple makes the feet stick-slip (9 mm/s backward
                # creep), at 200 ms 1 mm/s. Muscles likewise smooth spike
                # trains (50-100 ms twitches, summing in tetanus).
                motor['decode_tau_s']=.2
    if profile == 'crow_corvus_corone':
        proposal = json.loads((PACK / 'spec/crow-io.proposed.json').read_text())
        output_names = proposal['output_names']
        known = {s['name']: s for s in sensors}
        sensors = [known.get(n, dict(name=n, kind='projection', units='normalised', range=[0, 1], rate_hz=500, missing='unavailable-disarm')) for n in proposal['sensor_names']]
    if profile == SALAMANDER:
        # Designed contract: legacy-native measurement kinds and antagonistic
        # position readouts (embodiment_salamander module docstring).
        import embodiment_salamander as salamander
        sensors = salamander.sensors()
        output_names = salamander.output_names()
        for i, motor in enumerate(motors):
            motor.update(encoding=salamander.VERSION, readout=salamander.readout(motor['joint']), position_readout=True)
            motor['velocity_limit'] = float(model.actuator_user[i, 0])
    from embodiment_scene import needs_sensors
    sensors = sensors + needs_sensors(profile, scene['half_extent_m'])
    io = dict(schema_version=1, digest_encoding='embodiment-binary-v1', mapping_version=profile+'-io-v1', sensors=sensors, output_names=output_names,
              sensory_base=4096, output_base=16384, dt_us=2000,
              transducer=dict(version='antagonist-rate-v1', tau_s=.02, stale_s=.1, neutral='zero effort; passive dynamics continue'))
    io['digest'] = digest(io)
    if profile in NEURAL_LEGACY:
        io['neural_sensory_names'],io['neural_output_names']=neural_names(profile)
        # Optional sensor groups (hearing, vision...) keep their record names
        # as neural names; a preserved network binds only its own core list.
        io['neural_sensory_names']+=[s['name'] for s in sensors if s.get('group') and s['name'] not in io['neural_sensory_names']]
        io['mapping_version']=LEGACY_VERSION+'/'+profile
        io['transducer']=dict(version=LEGACY_VERSION,tau_s=.02,stale_s=.1,neutral='zero active drive on empty history; no twitch or oscillator',fidelity='versioned engineering projection; no learned task or biological calibration claim')
        io['digest']=digest({k:v for k,v in io.items() if k!='digest'})
    if profile == SALAMANDER:
        io['mapping_version'] = salamander.VERSION
        io['transducer'] = salamander.transducer()
        io['digest'] = digest({k: v for k, v in io.items() if k != 'digest'})
    if profile == 'celegans':
        io['mapping_version']='celegans-simulation-v2'
        io['transducer']=dict(version='celegans-muscle-v3-sim-v1',muscle_tau_s=.055,spine_tau_s=.080,twitch_fraction=.3,row_order='row 01 = head (segment_23)',stale_s=.1,neutral='zero active effort; no twitch fallback',calibration='functional surrogate; twitch summation, head-to-tail rows; not measured muscle physiology')
        io['digest']=digest({k:v for k,v in io.items() if k!='digest'})
    source = source_model(profile)
    record = dict(schema_version=1, digest_encoding='embodiment-binary-v1', compiler='embodiment-v1', profile_id=profile,
                  variant_id='tapered-cuticle-v3' if profile=='celegans' else 'wheeled' if profile == 'galbot_g1_golf' else 'community-estimate' if source else 'reduced-31-v1' if profile == 'crow_corvus_corone' else 'lec_brochure' if profile == 'xbot_s_pro' else 'manufacturer_640mm_depth' if profile == 'keenon_s100' else 'without_squeegee' if profile == 'keenon_c40' else 'original-v1',
                  fidelity='official-description' if profile == 'galbot_g1_golf' else 'functional-surrogate',
                  exact_replica='pending OEM variant and calibration',
                  controller_transfer='celegans-muscle-v3-sim-v1; analytic stimulus fields, proprioception and resistive-force substrate; uncalibrated' if profile=='celegans' else LEGACY_VERSION+'; native MuJoCo engineering adapter; calibration unvalidated' if profile in NEURAL_LEGACY else 'untrained',
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
    if profile == SALAMANDER:
        record.update(exact_replica='designed body; not a measured animal',
                      controller_transfer=salamander.VERSION+'; untrained; native MuJoCo engineering body, calibration unvalidated',
                      source='scripts/embodiment_salamander.py:build',
                      source_digest=digest([sha(Path(__file__)), sha(ROOT/'scripts/embodiment_salamander.py')]))
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
    if profile.startswith('drosophila'):
        # Fly mechanics v3: resonant-flight-v1 motor (stroke springs from the
        # compiled wing inertia), applied body-side by every engine.
        from embodiment_legacy import fly_flight
        record['flight'] = fly_flight(model, mujoco, profile)
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


def products(profiles=FIRST+tuple(p for p in NEW+LEGACY+DESIGNED if p not in FIRST)):
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
    for profile in NEW+LEGACY+DESIGNED:
        path=OUT/(profile+'.json')
        encoded=result.get(path)
        record=json.loads(encoded) if encoded else json.loads(path.read_text()) if path.exists() else None
        if record:
            old=PROFILES.get(profile)
            entries.append(dict(id=profile,aliases=list(old.aliases) if old else list(SALAMANDER_ALIASES) if profile==SALAMANDER else [profile],sensory=len(record['io']['sensors']),output=len(record['io']['output_names']),
                                model=profile+'.xml',record=profile+'.json',config=profile+'.config.json',network=old.network_rel if profile in LEGACY else '',io_digest=record['io']['digest'],model_digest=record['digest']))
    result[OUT/'registry.generated.json']=json.dumps(dict(schema_version=1,profiles=entries),indent=2)+'\n'
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--profiles', nargs='+', default=list(FIRST + tuple(p for p in NEW+LEGACY+DESIGNED if p not in FIRST)))
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
