#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Leg kinematics of the Freenove-class hexapod body record.

Geometry from ``embodiment_legacy`` (coxa 33, femur 90, tibia 110 mm; mounts
at 36 / 90 / 144 deg from the body axis). Shared by the gait probe
(``scripts/qa/hexapod_gait_probe.py``, which drives the body directly) and
the CPG network builder (``scripts/build_hexapod_cpg_network_json.py``,
which turns the same foot targets into motor-cell drives), so the network
walks the geometry the probe measured.
"""
import math

from embodiment_legacy import HEX_LINKS, HEX_MOUNTS

LEGS = [(s, r) for s in "lr" for r in "fmr"]
# Tripod groups: lf, rm, lr swing together; rf, lm, rr in antiphase.
TRIPOD = {("l", "f"): 0, ("r", "m"): 0, ("l", "r"): 0, ("r", "f"): 1, ("l", "m"): 1, ("r", "r"): 1}


def leg_ik(side, x_f, y_o, z):
    """Joint angles (coxa, femur, tibia) for a foot target in the leg frame:
    x_f forward, y_o outward from the coxa joint, z up (feet below: z < 0)."""
    sign = 1.0 if side == "l" else -1.0
    l1, l2, l3 = HEX_LINKS
    coxa = -sign * math.atan2(x_f, y_o)
    h = math.hypot(x_f, y_o) - l1
    d2 = h * h + z * z
    c = (d2 - l2 * l2 - l3 * l3) / (2 * l2 * l3)
    c = min(1.0, max(-1.0, c))
    psi = -math.acos(c)                       # tibia relative to the femur (knee up)
    phi = math.atan2(z, h) - math.atan2(l3 * math.sin(psi), l2 + l3 * math.cos(psi))
    return coxa, sign * phi, sign * (psi + math.pi / 2)


def joint_targets(side, row, sx=0.0, lift=0.0, stride=0.05, turn=0.0, height=0.095, step_height=0.04,
                  reach=0.09):
    """(coxa, femur, tibia) for one leg.

    ``sx``: stride fraction, -0.5 (foot back) .. +0.5 (foot forward) along
    the body x; ``turn`` rotates the stance about the body centre by
    ``turn * sx`` rad; ``lift``: 0 (on the ground) .. 1 (``step_height`` up).
    The neutral foot sits ``reach`` beyond the coxa link, ``height`` below
    the coxa joint."""
    mx, my, yaw = HEX_MOUNTS[row]
    sign = 1.0 if side == "l" else -1.0
    ang = sign * yaw
    rad = math.hypot(mx, my) + HEX_LINKS[0] + reach
    bx, by = rad * math.cos(ang), rad * math.sin(ang)
    dx, rot = stride * sx, turn * sx
    fx = bx * math.cos(rot) - by * math.sin(rot) + dx
    fy = bx * math.sin(rot) + by * math.cos(rot)
    fz = -height + step_height * lift
    # Into the leg frame: origin at the coxa joint, outward = sign * y.
    lx, ly = fx - mx, fy - sign * my
    fr = sign * yaw - sign * math.pi / 2
    x_f = lx * math.cos(-fr) - ly * math.sin(-fr)
    y_l = lx * math.sin(-fr) + ly * math.cos(-fr)
    return leg_ik(side, x_f, sign * y_l, fz)
