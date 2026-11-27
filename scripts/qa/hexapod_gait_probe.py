#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Programmed tripod gait for the Freenove-class hexapod (feasibility probe).

Drives the generated hexapod (MuJoCo) with a tripod gait through per-leg
inverse kinematics (coxa 33, femur 90, tibia 110 mm), the way the kit's own
software walks (feet ~140 mm radially out from each coxa, 40 mm step height),
through the servo speed limit, without a brain. Reports forward speed, turn
rate, body height and falls, so the body and the AARNN gait networks can be
checked against what the hardware geometry allows.

  target/robot-mujoco-venv/bin/python scripts/qa/hexapod_gait_probe.py --stride 0.05 --seconds 20
"""
import argparse
import json
import math
import sys
from pathlib import Path

import mujoco
import numpy as np

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from hexapod_kinematics import LEGS, TRIPOD, joint_targets  # noqa: E402


def run(a):
    m = mujoco.MjModel.from_xml_path(str(ROOT / "sim/content/embodiments/generated/hexapod.xml"))
    if a.effort or a.stall:
        for i in range(m.nu):
            effort = a.effort or float(m.actuator_forcerange[i][1])
            kp = effort / (a.stall or (float(m.actuator_forcerange[i][1]) / float(m.actuator_gainprm[i][0])))
            m.actuator_forcerange[i] = [-effort, effort]
            m.actuator_gainprm[i][0] = kp
            m.actuator_biasprm[i][1] = -kp
            m.actuator_biasprm[i][2] = -kp * 0.02
    d = mujoco.MjData(m)
    acts = {mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_ACTUATOR, i): i for i in range(m.nu)}

    def act(side, row, kind):
        return next(i for n, i in acts.items() if n.endswith(f"_{side}{row}_{kind}"))
    dt = m.opt.timestep
    vmax = m.actuator_user[:, 0].copy()
    decoded = np.zeros(m.nu)
    steps = int(a.seconds / dt)
    pos0 = None
    heading, last = 0.0, None
    zs, fell = [], False
    for k in range(steps):
        t = k * dt
        ramp = min(1.0, max(0.0, (t - a.settle) / 1.0))
        want = np.zeros(m.nu)
        for side, row in LEGS:
            if a.gait == "wave":
                # Wave gait: one leg swings at a time (duty 5/6), rear to front
                # on each side, the sides half a cycle apart.
                order = {("l", "r"): 0, ("l", "m"): 1, ("l", "f"): 2, ("r", "r"): 3, ("r", "m"): 4, ("r", "f"): 5}
                ph = (t - a.settle) / a.period + order[(side, row)] / 6.0
                duty = 5.0 / 6.0
            else:
                ph = (t - a.settle) / a.period + 0.5 * TRIPOD[(side, row)]
                duty = 0.5
            u = ph % 1.0
            # Stance (u < duty): foot moves back along the body x; swing: forward and up.
            if t < a.settle:
                sx, lift = 0.0, 0.0
            elif u < duty:
                sx, lift = 0.5 - u / duty, 0.0
            else:
                v = (u - duty) / (1.0 - duty)
                # Lift fast, carry the foot high, set it down late (a trapezoid,
                # as real gaits do): a sine swing dragged the feet near both ends.
                sx = -0.5 + min(1.0, max(0.0, (v - 0.15) / 0.7))
                lift = min(1.0, math.sin(math.pi * v) * a.lift_shape)
            q = joint_targets(side, row, sx=ramp * sx, lift=ramp * lift, stride=a.stride, turn=a.turn,
                              height=a.height, step_height=a.lift, reach=a.reach)
            for kind, qv in zip(("coxa", "femur", "tibia"), q):
                want[act(side, row, kind)] = qv
        want = np.clip(want, m.actuator_ctrlrange[:, 0], m.actuator_ctrlrange[:, 1])
        decoded = decoded + np.clip(want - decoded, -vmax * dt, vmax * dt)
        d.ctrl[:] = decoded
        mujoco.mj_step(m, d)
        w, x, y, z = d.qpos[3:7]
        yaw_b = math.atan2(2 * (w * z + x * y), 1 - 2 * (y * y + z * z))
        if last is not None:
            heading += (yaw_b - last + math.pi) % (2 * math.pi) - math.pi
        last = yaw_b
        if t >= a.settle + 1.0 and pos0 is None:
            pos0, h0 = d.qpos[:2].copy(), heading
        if t > a.settle + 1.0:
            zs.append(float(d.qpos[2]))
        if a.trace and k % int(a.trace / dt) == 0:
            print(json.dumps(dict(t=round(t, 2), x=round(float(d.qpos[0]), 3), y=round(float(d.qpos[1]), 3),
                                  z=round(float(d.qpos[2]), 3))), file=sys.stderr)
        if d.xmat[1][8] < 0.3:
            fell = True
            break
    walk_s = max(1e-9, k * dt - a.settle - 1.0)
    disp = d.qpos[:2] - pos0 if pos0 is not None else np.zeros(2)
    return dict(gait=a.gait, stride=a.stride, turn=a.turn, period=a.period, height=a.height, fell=fell,
                speed_mps=round(float(np.linalg.norm(disp)) / walk_s, 4),
                forward_mps=round(float(disp[0]) / walk_s, 4),
                turn_deg_s=round(math.degrees(heading - h0) / walk_s, 2) if pos0 is not None else None,
                body_z=[round(min(zs), 3), round(max(zs), 3)] if zs else None)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--seconds", type=float, default=20.0)
    ap.add_argument("--settle", type=float, default=2.0)
    ap.add_argument("--period", type=float, default=1.0, help="gait cycle (s)")
    ap.add_argument("--stride", type=float, default=0.05, help="stride length along the body x (m)")
    ap.add_argument("--turn", type=float, default=0.0, help="yaw per stride (rad)")
    ap.add_argument("--height", type=float, default=0.06, help="coxa joint height above the feet (m)")
    ap.add_argument("--lift", type=float, default=0.04, help="step height (m; the kit uses 40 mm)")
    ap.add_argument("--reach", type=float, default=0.107, help="foot distance beyond the coxa link (m; kit: 140 - 33 mm)")
    ap.add_argument("--gait", choices=("tripod", "wave"), default="tripod")
    ap.add_argument("--lift-shape", type=float, default=3.0, help="swing lift profile: min(1, k sin(pi v)); 1 = sine")
    ap.add_argument("--effort", type=float, default=0.0, help="override servo stall torque (N m)")
    ap.add_argument("--stall", type=float, default=0.0, help="override error (rad) at which a servo reaches stall torque")
    ap.add_argument("--trace", type=float, default=0.0, help="print the body position every this many seconds (stderr)")
    a = ap.parse_args()
    print(json.dumps(run(a)))


if __name__ == "__main__":
    main()
