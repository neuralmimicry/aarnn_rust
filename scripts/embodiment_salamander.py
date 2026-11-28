# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Salamander-style amphibious crawler (``salamander_crawler``, v1).

An original engineering body in the repository's metre scale, not a measured
animal: a sprawling quadruped whose axial bending and leg coordination must be
composed together (salamanders walk with a standing body wave and trotting,
girdle-coordinated limbs; Ijspeert et al. 2007, Science 315:1416).

Mechanics (all values are design assumptions):

* trunk: four rigid ellipsoid segments in a chain along -x (head, shoulder
  girdle, pelvic girdle, tail; 0.32 m) joined by three yaw hinges
  (``neck_yaw``, ``trunk_yaw``, ``tail_yaw``). The head is the free root, as
  the fish's ``segment_00``. Segment masses are ~1.2 x the water mass of their
  ellipsoids (a slightly negatively buoyant amphibian); total 0.376 kg;
* legs: front pair on the shoulder girdle, hind pair on the pelvic girdle,
  sprawling (upper limb lateral, lower limb vertical, rigid elbow/knee).
  Two DoF each: ``<leg>_coxa`` (hip yaw, protraction/retraction) and
  ``<leg>_lift`` (hip elevation/depression about the horizontal axis normal
  to the limb). Spherical feet with friction 1.0;
* drives: position servos (hobby-servo class, full torque at 0.15 rad error,
  6 rad/s), 0.4 N m axial and 0.3 N m legs. Joint axes are mirrored so that
  +q has one anatomical meaning on both sides: axial +q bends the body
  concave to the left, coxa +q protracts (foot forward), lift +q elevates
  (foot up). The built pose (all joints 0: straight trunk, legs normal to
  the body, feet on the floor) is the servo centre;
* no self-collision (as the other reduced rigs); every body collides with
  the habitat.

IO contract (``salamander-crawler-v1``), legacy-native measurement kinds so
the native adapters and the fish/fly/hexapod circuit blocks can bind:

* 11 joint angles (``legacy-angle``), 4 foot contacts (``legacy-contact``
  regions at the feet), head IMU (``legacy-accel``/``legacy-gyro``), head
  touch front/left/right (``legacy-contact`` regions on the head shell,
  excluding its underside, with a ``direction`` for circuit builders), a
  ring of 16 horizontal range rays on the head (``legacy-range``, 0.9 m,
  three body lengths) and one downward ray at the snout; the ``needs``
  group is appended by the compiler (``embodiment_scene.needs_sensors``);
* 22 outputs: one antagonistic pair per actuated joint, readout
  ``(a, +1), (b, -1)``: the target is the joint centre plus
  ``(rate_a - rate_b) x half range``, so equal (or no) activity holds the
  built pose.
"""
import math

PROFILE = 'salamander_crawler'
ALIASES = (PROFILE, 'salamander')
VERSION = 'salamander-crawler-v1'

SKIN = [.10, .085, .07, 1]       # dark dorsal skin
SPOT = [.93, .72, .12, 1]        # yellow dorsal spots (visual only)
BELLY = [.55, .45, .30, 1]
# Head: the free root; ellipsoid half axes (x, y, z) in m.
HEAD = dict(half=(.030, .021, .014), mass=.050)
# Chained segments: (body, joint, joint position in the parent frame (x),
# geom centre in the body frame (x), half axes, mass, joint range rad).
SEGMENTS = (
    ('shoulder', 'neck_yaw', -.032, -.036, (.038, .026, .018), .090, .6),
    ('pelvis', 'trunk_yaw', -.072, -.036, (.038, .026, .018), .090, .6),
    ('tail', 'tail_yaw', -.072, -.056, (.058, .016, .012), .045, .8),
)
# Legs: hip position in the girdle frame (x), lateral hip offset, upper-limb
# reach, lower-limb drop, radii and masses (coxa hub, upper, lower, foot).
LEGS = ('lf', 'lh', 'rf', 'rh')
HIP_X = {'f': -.014, 'h': -.058}
HIP_Y = .024
LIMB = dict(reach=.040, drop=.034, upper_radius=.0055, lower_radius=.0045, foot_radius=.006,
            hub_radius=.007, hub_mass=.006, upper_mass=.010, lower_mass=.006, foot_mass=.002)
COXA_RANGE = .9
LIFT_RANGE = .7
# Feet rest 2 mm above the floor at load.
SPAWN_Z = round(LIMB['drop'] + LIMB['foot_radius'] + .002, 6)
SERVO = dict(axial_effort=.4, leg_effort=.3, stall_rad=.15, kv_ratio=.02, speed=6.0, damping=.01, armature=2e-4)
JOINTS = tuple(s[1] for s in SEGMENTS) + tuple(f'{leg}_{kind}' for leg in LEGS for kind in ('coxa', 'lift'))
# Output channel pair per joint: (direction of +q, direction of -q).
DIRECTIONS = {'yaw': ('left', 'right'), 'coxa': ('protract', 'retract'), 'lift': ('elevate', 'depress')}
# Head exteroception.
RAY_COUNT = 16
RAY_REACH_M = .9
RAY_MOUNT = (0., 0., .006)
DOWN_RAY = dict(position=(.026, 0., -.005), reach=.15)


def build(element, base_world):
    """MJCF root of the crawler in the repository's base world."""
    root, world = base_world(PROFILE)
    actuators = element(root, 'actuator')
    element(root, 'size', nuser_actuator=1)

    def collide(geom):
        # Robot geoms collide with the habitat (type 1) but not each other.
        geom.set('contype', '2')
        geom.set('conaffinity', '1')
        return geom

    def decor(body, name, shape, pos, size, rgba, **kw):
        # Massless, non-colliding illustration (group 2: rays skip it).
        if pos is not None:
            kw['pos'] = pos
        return element(body, 'geom', name=name, type=shape, size=size, rgba=rgba, mass=0, contype=0,
                       conaffinity=0, group=2, **kw)

    def servo(joint, effort, limit):
        # Velocity limit in actuator user[0], as the other servo bodies.
        kp = effort / SERVO['stall_rad']
        element(actuators, 'position', name=joint, joint=joint, ctrlrange=[-limit, limit], kp=kp,
                kv=kp * SERVO['kv_ratio'], forcerange=[-effort, effort], user=SERVO['speed'])

    def hinge(body, name, axis, limit):
        element(body, 'joint', name=name, axis=axis, range=[-limit, limit], damping=SERVO['damping'],
                armature=SERVO['armature'])

    head = element(world, 'body', name='head', pos=[0, 0, SPAWN_Z])
    element(head, 'freejoint', name='root')
    collide(element(head, 'geom', name='head_collision', type='ellipsoid', size=HEAD['half'], mass=HEAD['mass'],
                    rgba=SKIN))
    a, b, c = HEAD['half']
    for side, sign in (('left', 1), ('right', -1)):
        decor(head, f'head_eye_{side}', 'sphere', [.4 * a, sign * .7 * b, .55 * c], [.0045], [.02, .02, .02, 1])
        decor(head, f'head_spot_{side}', 'ellipsoid', [-.3 * a, sign * .45 * b, .75 * c], [.008, .005, .002], SPOT)
    bodies = {'head': head}
    parent = head
    for name, joint, offset, centre, half, mass, limit in SEGMENTS:
        body = element(parent, 'body', name=name, pos=[offset, 0, 0])
        # Axis -z: +q bends the body concave to the left.
        hinge(body, joint, [0, 0, -1], limit)
        collide(element(body, 'geom', name=name + '_collision', type='ellipsoid', pos=[centre, 0, 0], size=half,
                        mass=mass, rgba=SKIN))
        decor(body, name + '_belly', 'ellipsoid', [centre, 0, -.55 * half[2]], [.9 * half[0], .8 * half[1], .45 * half[2]],
              BELLY)
        for k, (dx, dy) in enumerate(((-.5, .45), (.15, -.5), (.55, .35))):
            decor(body, f'{name}_spot_{k}', 'ellipsoid', [centre + dx * half[0], dy * half[1], .82 * half[2]],
                  [.18 * half[0], .22 * half[1], .12 * half[2]], SPOT)
        servo(joint, SERVO['axial_effort'], limit)
        bodies[name] = body
        parent = body
    m = LIMB
    for leg in LEGS:
        side = 1 if leg[0] == 'l' else -1
        girdle = bodies['shoulder' if leg[1] == 'f' else 'pelvis']
        coxa = element(girdle, 'body', name=f'{leg}_coxa_link', pos=[HIP_X[leg[1]], side * HIP_Y, 0])
        # Mirrored axes: coxa +q protracts, lift +q elevates, on both sides.
        hinge(coxa, f'{leg}_coxa', [0, 0, -side], COXA_RANGE)
        collide(element(coxa, 'geom', name=f'{leg}_coxa_collision', type='sphere', size=[m['hub_radius']],
                        mass=m['hub_mass'], rgba=SKIN))
        limb = element(coxa, 'body', name=f'{leg}_limb_link', pos=[0, 0, 0])
        hinge(limb, f'{leg}_lift', [side, 0, 0], LIFT_RANGE)
        y = side * m['reach']
        collide(element(limb, 'geom', name=f'{leg}_upper_collision', type='capsule', fromto=[0, 0, 0, 0, y, 0],
                        size=[m['upper_radius']], mass=m['upper_mass'], rgba=SKIN))
        collide(element(limb, 'geom', name=f'{leg}_lower_collision', type='capsule', fromto=[0, y, 0, 0, y, -m['drop']],
                        size=[m['lower_radius']], mass=m['lower_mass'], rgba=SKIN))
        collide(element(limb, 'geom', name=f'{leg}_foot', type='sphere', pos=[0, y, -m['drop']], size=[m['foot_radius']],
                        mass=m['foot_mass'], rgba=BELLY, friction=[1., .01, .001]))
        # Four splayed toes, pointing forward-outward (visual only).
        for k, toe in enumerate((-.6, -.2, .2, .6)):
            tip = [round(.012 * math.sin(toe + .5), 6), round(y + side * .012 * math.cos(toe + .5), 6), -m['drop'] - .004]
            decor(limb, f'{leg}_toe_{k}', 'capsule', None, [.0016], BELLY, fromto=[0, y, -m['drop'] - .003] + tip)
        servo(f'{leg}_coxa', SERVO['leg_effort'], COXA_RANGE)
        servo(f'{leg}_lift', SERVO['leg_effort'], LIFT_RANGE)
    order = [a.get('joint') for a in actuators]
    if tuple(order) != JOINTS:
        raise ValueError(f'salamander actuator order {order} differs from the IO contract')
    return root


def output_names():
    """Two channels per joint, +q direction first (``sal_o_NN_<joint>_<dir>``)."""
    out = []
    for joint in JOINTS:
        for word in DIRECTIONS[joint.rsplit('_', 1)[1]]:
            out.append(f'sal_o_{len(out):02}_{joint}_{word}')
    return out


def readout(joint):
    """Antagonistic readout of one joint: channel a x +1 (+q), b x -1 (-q)."""
    i = JOINTS.index(joint)
    return [dict(channel=2 * i, coefficient=1.0), dict(channel=2 * i + 1, coefficient=-1.0)]


def transducer():
    return dict(version=VERSION, tau_s=.02, stale_s=.1,
                readout='antagonist position: target = joint centre + (a - b) x half range; a drives +q, b drives -q',
                neutral='equal or no antagonist activity holds the built pose; a disarmed body is passive',
                fidelity='designed engineering body; no biological calibration or learned task claim')


def _sensor(index, name, kind, body, **kwargs):
    return dict(name=f'sal_s_{index:02}_{name}', kind='legacy-' + kind, body=body, units='normalised', range=[0, 1],
                rate_hz=500, missing='fault-disarm', **kwargs)


def _v(values):
    return [round(float(v), 6) + 0. for v in values]


def sensors():
    """Ordered core sensors (the compiler appends the ``needs`` group)."""
    out = []

    def add(name, kind, body, **kwargs):
        out.append(_sensor(len(out), name, kind, body, **kwargs))
    for joint in JOINTS:
        add(joint, 'angle', 'head', joint=joint)
    m = LIMB
    for leg in LEGS:
        y = (1 if leg[0] == 'l' else -1) * m['reach']
        r = m['foot_radius'] * 2
        add(f'{leg}_foot', 'contact', f'{leg}_limb_link',
            region_m=[_v([-r, y - r, -m['drop'] - r]), _v([r, y + r, -m['drop'] + .004])])
    for i, axis in enumerate('xyz'):
        add(f'accel.{axis}', 'accel', 'head', axis=i, mode='native-z-up')
    for i, axis in enumerate('xyz'):
        add(f'gyro.{axis}', 'gyro', 'head', axis=i, mode='native-z-up')
    # Touch regions on the head shell; the underside (z below -c/2) is left
    # out so a head resting on the floor reads no touch.
    a, b, c = HEAD['half']
    front = [_v([.6 * a, -b - .01, -.5 * c]), _v([a + .01, b + .01, c + .01])]
    left = [_v([-a - .01, .4 * b, -.5 * c]), _v([.6 * a, b + .01, c + .01])]
    right = [_v([-a - .01, -b - .01, -.5 * c]), _v([.6 * a, -.4 * b, c + .01])]
    add('touch_front', 'contact', 'head', direction=[1., 0., 0.], region_m=front)
    add('touch_left', 'contact', 'head', direction=[0., 1., 0.], region_m=left)
    add('touch_right', 'contact', 'head', direction=[0., -1., 0.], region_m=right)
    # Ring of horizontal rays: prox_00 ahead, counter-clockwise (left) in
    # 22.5 deg steps; reading 1 - distance / reach, rays skip the own body.
    for i in range(RAY_COUNT):
        angle = 2 * math.pi * i / RAY_COUNT
        add(f'prox_{i:02}', 'range', 'head', position_m=list(RAY_MOUNT), direction=_v([math.cos(angle), math.sin(angle), 0]),
            angle_rad=RAY_REACH_M)
    add('prox_down', 'range', 'head', position_m=list(DOWN_RAY['position']), direction=[0., 0., -1.],
        angle_rad=DOWN_RAY['reach'])
    return out
