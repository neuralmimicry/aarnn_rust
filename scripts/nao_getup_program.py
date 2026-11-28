#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""NAO get-up keyframes: the shared definition of a fall-recovery program.

A program is a list of keyframes ``[duration_s, g1 .. g10]`` (symmetric: the
right joint mirrors the left) or ``[duration_s, l1 .. l10, r1 .. r10]``
(asymmetric, e.g. a roll), over ``GROUPS``; joints outside the groups stay in
the StandInit posture. Every program ends in a fixed tail: the balanced
flat-foot ``SQUAT`` held ``SQUAT_HOLD_S``, then a linear rise to StandInit
over ``RISE_S`` in four steps, then StandInit.

``scripts/qa/nao_getup_probe.py`` searches programs on the MuJoCo body;
``scripts/build_nao_walk_network_json.py --getup`` plays one from the
network (a keyframe-cell chain, ``aarnn_circuit.add_motor_sequence``). Both
use ``sequence`` so the network replays exactly the verified targets.
"""

# Joint groups: (left joint, mirror sign for the right joint).
GROUPS = [("ShoulderPitch", 1), ("ShoulderRoll", -1), ("ElbowYaw", -1), ("ElbowRoll", -1), ("HipYawPitch", 1),
          ("HipRoll", -1), ("HipPitch", 1), ("KneePitch", 1), ("AnklePitch", 1), ("AnkleRoll", -1)]
STAND_S = 2.5          # hold the final StandInit this long (probe scoring)
SUCCESS_HEIGHT = 0.28  # torso height (m) of an upright NAO (standing ~0.31-0.33)
# A balanced flat-footed squat (solved by forward kinematics: feet flat when
# torso pitch + hip + knee + ankle = 0, centre of mass over the feet): from it
# the NAO rises to StandInit in 2-4 s (24 Nov: upright 0.99, torso 0.32 m).
SQUAT = [0.3, 0.1, 0.0, -0.3, 0.0, 0.0, -1.05, 2.0, -0.95, 0.0]
SQUAT_HOLD_S = 1.5
RISE_S = 3.0
RISE_STEPS = 4


def frame_targets(frame, posture, lo, hi):
    """{joint: target rad} for one keyframe's group values (10 or 20), over
    the StandInit ``posture`` {joint: rad}, clipped to [lo, hi]."""
    want = dict(posture)
    left = list(frame[:len(GROUPS)])
    right = list(frame[len(GROUPS):2 * len(GROUPS)]) if len(frame) >= 2 * len(GROUPS) else left
    for (joint, mirror), lv, rv in zip(GROUPS, left, right):
        for side, sgn, value in (("L", 1.0, lv), ("R", float(mirror), rv)):
            name = side + joint
            if name in want:
                want[name] = min(max(value * sgn, lo[name]), hi[name])
    return want


def sequence(program, posture, lo, hi):
    """[(duration_s, {joint: target})]: the program, the squat hold and the
    rise; StandInit follows (the posture itself)."""
    frames = [(max(0.2, float(f[0])), frame_targets(f[1:], posture, lo, hi)) for f in program]
    squat = frame_targets(SQUAT, posture, lo, hi)
    frames.append((SQUAT_HOLD_S, squat))
    for k in range(1, RISE_STEPS + 1):
        frames.append((RISE_S / RISE_STEPS, {n: squat[n] + (posture[n] - squat[n]) * k / RISE_STEPS for n in squat}))
    return frames
