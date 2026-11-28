#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Programmed NAO gait probe: how fast can this body turn?

Drives the generated NAO (MuJoCo, ``sim/content/embodiments/generated``) with
the walk network's own target pattern, without a brain, so turning strategies
can be compared quickly:

* StandInit posture (``build_nao_stand_network_json.posture_angle``);
* two half phases, A = body over the left foot with the right leg swinging,
  B its mirror, each ``--period / 2`` long;
* lateral weight shift (hip roll -sway / ankle roll +sway in A), swing flexion
  (hip -lift, knee +2 lift, ankle -lift on the swinging leg), stride through a
  phase lagged by ``--lag`` (hip -/+step, ankle +/-step), stride trim;
* turning: stride asymmetry (``--stride-turn``: the inner stride scaled by
  1 - x) and/or hip yaw-pitch (``--yaw``, rad, while the inner leg swings,
  back to zero in the other half phase - the NAO turns with this one motor);
* targets pass the adapter's servo path: 200 ms first-order decode and the
  record's per-joint speed limit.

Reports heading change (deg, deg/s), displacement and falls as JSON.

  target/robot-mujoco-venv/bin/python scripts/qa/nao_turn_probe.py --yaw 0.3 --seconds 30
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
from build_nao_stand_network_json import posture_angle  # noqa: E402


def smooth_square(t, period, lag=0.0, tau=0.06):
    """Phase indicator: ~1 in half phase A, ~0 in B, edges softened (a cell's
    rise), optionally lagged."""
    u = ((t - lag) / period) % 1.0
    edge = tau / period
    if u < 0.5:
        return min(1.0, u / edge) if u < edge else 1.0
    v = u - 0.5
    return max(0.0, 1.0 - v / edge) if v < edge else 0.0


def yaw_of(quat):
    w, x, y, z = quat
    return math.atan2(2 * (w * z + x * y), 1 - 2 * (y * y + z * z))


def run(a):
    m = mujoco.MjModel.from_xml_path(str(ROOT / "sim/content/embodiments/generated/nao.xml"))
    d = mujoco.MjData(m)
    if a.friction:
        for g in range(m.ngeom):
            name = mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_GEOM, g) or ""
            if "AnkleRoll" in name:
                m.geom_friction[g][0] = a.friction
    acts = {mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_ACTUATOR, i): i for i in range(m.nu)}
    lo, hi = m.actuator_ctrlrange[:, 0].copy(), m.actuator_ctrlrange[:, 1].copy()
    vmax = np.array([float(m.actuator_user[i][0]) if m.actuator_user.shape[1] else 4.0 for i in range(m.nu)])
    joint_q = {n: m.jnt_qposadr[m.actuator_trnid[i, 0]] for n, i in acts.items()}
    posture = np.array([min(max(posture_angle(n), lo[i]), hi[i]) for n, i in sorted(acts.items(), key=lambda kv: kv[1])])
    rest = np.array([d.qpos[joint_q[n]] for n, i in sorted(acts.items(), key=lambda kv: kv[1])])
    decoded = rest.copy()
    dt = m.opt.timestep
    steps = int(a.seconds / dt)
    t_gait = a.settle
    heading0, pos0, fell = None, None, False
    heading_unwrapped, last_yaw = 0.0, None
    inner = "R" if a.turn > 0 else "L"   # turn left: shorten the left stride, yaw while the left leg swings
    for k in range(steps):
        t = k * dt
        want = posture.copy()
        if t >= t_gait:
            tg = t - t_gait
            ramp = min(1.0, tg / 2.0)
            e = smooth_square(tg, a.period)                 # 1: phase A (left stance, right swing)
            dlag = smooth_square(tg, a.period, a.lag * a.period)
            ph = {"A": e, "B": 1.0 - e}
            dph = {"A": dlag, "B": 1.0 - dlag}

            def add(joint, value):
                i = acts.get(joint)
                if i is not None:
                    want[i] += ramp * value
            for side in "LR":
                add(side + "HipRoll", a.sway * (ph["B"] - ph["A"]))
                add(side + "AnkleRoll", a.sway * (ph["A"] - ph["B"]))
            for side, swing in (("R", ph["A"]), ("L", ph["B"])):
                add(side + "HipPitch", -a.lift * swing)
                add(side + "KneePitch", 2 * a.lift * swing)
                add(side + "AnklePitch", -a.lift * swing)
            for side, sgn, scale in (("R", 1.0, 1.0 + a.trim), ("L", -1.0, 1.0 - a.trim)):
                s = a.step * scale
                if a.turn and side == ("L" if a.turn > 0 else "R"):
                    s *= 1.0 - a.stride_turn
                add(side + "HipPitch", -sgn * s * (dph["A"] - dph["B"]))
                add(side + "AnklePitch", sgn * s * (dph["A"] - dph["B"]))
            if a.yaw and a.turn:
                # Open the hip yaw-pitch while the leg on the turn side swings
                # (B: left swings, for a left turn) and close it in the other half.
                swing_turn_side = ph["B"] if a.turn > 0 else ph["A"]
                y = a.yaw * smooth_square(tg, a.period, (0.5 if a.turn > 0 else 0.0) * a.period + a.yaw_lag * a.period)
                if a.yaw_mode == "alt":
                    # The network's Y cells: +yaw/2 in one half phase, -yaw/2 in the other.
                    y = y - a.yaw / 2.0
                add("LHipYawPitch", -y)
                add("RHipYawPitch", -y)
                if a.yaw_comp:
                    # The 45 deg yaw-pitch axis also rolls/pitches the legs:
                    # counter it on the hips (approximate decoupling).
                    for side in "LR":
                        add(side + "HipPitch", a.yaw_comp * y)
            want = np.clip(want, lo, hi)
        # Servo path: 200 ms decode, then the speed limit.
        alpha = dt / (0.2 + dt)
        target = decoded + alpha * (want - decoded)
        step = np.clip(target - decoded, -vmax * dt, vmax * dt)
        decoded = decoded + step
        d.ctrl[:] = decoded
        mujoco.mj_step(m, d)
        if d.qpos[2] < 0.2:
            fell = True
            break
        yaw = yaw_of(d.qpos[3:7])
        if last_yaw is not None:
            dy = (yaw - last_yaw + math.pi) % (2 * math.pi) - math.pi
            heading_unwrapped += dy
        last_yaw = yaw
        if t >= t_gait and heading0 is None:
            heading0, pos0 = heading_unwrapped, d.qpos[:2].copy()
    gait_s = max(1e-9, (k + 1) * dt - t_gait)
    turned = math.degrees(heading_unwrapped - (heading0 or 0.0))
    disp = float(np.linalg.norm(d.qpos[:2] - (pos0 if pos0 is not None else d.qpos[:2])))
    return dict(turn=a.turn, yaw=a.yaw, stride_turn=a.stride_turn, period=a.period, step=a.step, lift=a.lift,
                sway=a.sway, friction=a.friction, fell=fell, gait_s=round(gait_s, 2), turned_deg=round(turned, 1),
                deg_per_s=round(turned / gait_s, 2), displacement_m=round(disp, 3))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--seconds", type=float, default=30.0)
    ap.add_argument("--settle", type=float, default=3.0)
    ap.add_argument("--period", type=float, default=2.0, help="gait cycle (s): two half phases")
    ap.add_argument("--sway", type=float, default=0.30)
    ap.add_argument("--lift", type=float, default=0.25)
    ap.add_argument("--step", type=float, default=0.10)
    ap.add_argument("--trim", type=float, default=0.6)
    ap.add_argument("--lag", type=float, default=0.15, help="stride lag (fraction of the cycle)")
    ap.add_argument("--turn", type=float, default=0.0, help="+1 turn left, -1 right, 0 straight")
    ap.add_argument("--stride-turn", type=float, default=0.0, help="inner stride shortening (0..1)")
    ap.add_argument("--yaw", type=float, default=0.0, help="hip yaw-pitch opening while the inner leg swings (rad)")
    ap.add_argument("--yaw-lag", type=float, default=0.0, help="yaw phase lag (fraction of the cycle)")
    ap.add_argument("--yaw-mode", choices=("open", "alt"), default="open",
                    help="open: 0 -> yaw -> 0 (one-sided); alt: +/- yaw/2 alternating (the network's Y cells)")
    ap.add_argument("--yaw-comp", type=float, default=0.0, help="hip pitch compensation per rad of yaw")
    ap.add_argument("--friction", type=float, default=0.0, help="override sole friction (0 = model's)")
    a = ap.parse_args()
    print(json.dumps(run(a)))


if __name__ == "__main__":
    main()
