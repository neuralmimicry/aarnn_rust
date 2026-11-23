#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Measure each wheel actuator's effect on the base (forward and yaw signs).

Wheel axes, mirrored mounts and mecanum rollers differ between robots, so a
designed controller should not guess which torque sign drives forward or
turns left. For each wheel-like actuator (name contains ``wheel`` or ends in
``_motor``) this applies a positive torque alone for 0.6 s from rest, with
the other actuators idle, and records the base displacement along its own
heading and its yaw change. It also casts the archetype range rays from the
adapter's origin (root + 0.1 m up, root body excluded) at rest, to show
whether any ray starts inside the robot.

  MUJOCO_GL=egl target/robot-mujoco-venv/bin/python scripts/qa/calibrate_wheels.py PROFILE [--json OUT]
"""
import argparse
import json
import math
from pathlib import Path

import mujoco
import numpy as np

ROOT = Path(__file__).resolve().parents[2]
GEN = ROOT / 'sim/content/embodiments/generated'


def wheel_like(name):
    return 'wheel' in name or name.endswith('_motor')


def yaw_of(m, d, body):
    x = d.xmat[body].reshape(3, 3)[:, 0]
    return math.atan2(x[1], x[0])


def probe(profile, seconds=0.6, ctrl=0.5):
    record = json.loads((GEN / f'{profile}.json').read_text())
    model = mujoco.MjModel.from_xml_path(str(GEN / f'{profile}.xml'))
    data = mujoco.MjData(model)
    root = 1
    out = {'profile': profile, 'wheels': {}, 'rays': []}
    acts = [a['name'] for a in record['actuators']]
    for name in acts:
        if not wheel_like(name):
            continue
        mujoco.mj_resetData(model, data)
        mujoco.mj_forward(model, data)
        for _ in range(100):  # settle on the ground
            mujoco.mj_step(model, data)
        p0 = data.xpos[root].copy()
        y0 = yaw_of(model, data, root)
        heading = np.array([math.cos(y0), math.sin(y0)])
        aid = mujoco.mj_name2id(model, mujoco.mjtObj.mjOBJ_ACTUATOR, name)
        for _ in range(int(seconds / model.opt.timestep)):
            data.ctrl[:] = 0
            data.ctrl[aid] = ctrl
            mujoco.mj_step(model, data)
        d = data.xpos[root][:2] - p0[:2]
        dyaw = (yaw_of(model, data, root) - y0 + math.pi) % (2 * math.pi) - math.pi
        out['wheels'][name] = dict(forward_m=float(d @ heading), lateral_m=float(d @ [-heading[1], heading[0]]),
                                   yaw_rad=float(dyaw))
    def run(pattern):
        mujoco.mj_resetData(model, data)
        mujoco.mj_forward(model, data)
        for _ in range(100):
            mujoco.mj_step(model, data)
        p0 = data.xpos[root].copy(); y0 = yaw_of(model, data, root)
        heading = np.array([math.cos(y0), math.sin(y0)])
        for _ in range(int(seconds / model.opt.timestep)):
            data.ctrl[:] = 0
            for n, u in pattern.items():
                data.ctrl[mujoco.mj_name2id(model, mujoco.mjtObj.mjOBJ_ACTUATOR, n)] = u
            mujoco.mj_step(model, data)
        d = data.xpos[root][:2] - p0[:2]
        return float(d @ heading), float((yaw_of(model, data, root) - y0 + math.pi) % (2 * math.pi) - math.pi)
    wheels = out['wheels']
    if wheels:
        fmax = max(abs(w['forward_m']) for w in wheels.values())
        ymax = max(abs(w['yaw_rad']) for w in wheels.values())
        forward = {n: math.copysign(1.0, w['forward_m']) if abs(w['forward_m']) >= 0.2 * fmax else 0.0 for n, w in wheels.items()}
        side = {}
        for n in wheels:
            jid = mujoco.mj_name2id(model, mujoco.mjtObj.mjOBJ_JOINT, next(a['joint'] for a in record['actuators'] if a['name'] == n))
            side[n] = 1.0 if (data.xpos[model.jnt_bodyid[jid]] - data.xpos[root]) @ data.xmat[root].reshape(3, 3)[:, 1] > 0 else -1.0
        candidates = {
            'yaw_sign': {n: math.copysign(1.0, w['yaw_rad']) if abs(w['yaw_rad']) >= 0.2 * ymax else 0.0 for n, w in wheels.items()},
            'skid': {n: forward[n] * (-side[n]) for n in wheels},
        }
        out['forward_pattern'] = forward
        fwd, fyaw = run({n: ctrl * u for n, u in forward.items()})
        out['forward_test'] = dict(forward_m=fwd, yaw_rad=fyaw)
        best = None
        for name, pat in candidates.items():
            f, y = run({n: ctrl * u for n, u in pat.items()})
            if best is None or abs(y) > abs(best[2]):
                best = (name, pat, y, f)
        name, pat, y, f = best
        out['turn_left_pattern'] = {n: u * math.copysign(1.0, y) for n, u in pat.items()}
        out['turn_test'] = dict(kind=name, yaw_rad=abs(y), forward_m=f)
        mujoco.mj_resetData(model, data)
    mujoco.mj_resetData(model, data)
    mujoco.mj_forward(model, data)
    pos, rot = data.xpos[root], data.xmat[root].reshape(3, 3)
    origin = pos + rot @ np.array([0, 0, 0.1])
    geomid = np.zeros(1, dtype=np.int32)
    for s in record['io']['sensors']:
        if s['kind'] != 'range_ray':
            continue
        a = s['angle_rad']
        direction = rot @ np.array([math.cos(a), math.sin(a), 0.0])
        # Same rule as the adapter's ray_beyond_self: step over own geometry.
        o, ex, travelled, dist, hit, body = origin.copy(), root, 0.0, -1.0, None, None
        for _ in range(32):
            dd = mujoco.mj_ray(model, data, o, direction, np.array([1, 1, 0, 1, 1, 1], dtype=np.uint8), 1, ex, geomid)
            if dd < 0:
                break
            b = int(model.geom_bodyid[geomid[0]])
            if b == 0 or model.body_rootid[b] != model.body_rootid[root]:
                dist, hit, body = travelled + dd, mujoco.mj_id2name(model, mujoco.mjtObj.mjOBJ_GEOM, int(geomid[0])), mujoco.mj_id2name(model, mujoco.mjtObj.mjOBJ_BODY, b)
                break
            travelled += dd + 1e-6; o = o + direction * (dd + 1e-6); ex = b
        out['rays'].append(dict(name=s['name'], angle_rad=a, distance_m=float(dist) if dist >= 0 else None, geom=hit, body=body))
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('profiles', nargs='+')
    ap.add_argument('--json')
    args = ap.parse_args()
    results = [probe(p) for p in args.profiles]
    for r in results:
        print(r['profile'])
        for n, w in r['wheels'].items():
            print(f"  {n:22s} forward {w['forward_m']:+.3f} m  lateral {w['lateral_m']:+.3f} m  yaw {w['yaw_rad']:+.3f} rad")
        if r['wheels']:
            print('  forward pattern', r['forward_pattern'], r['forward_test'])
            print('  turn-left pattern', r['turn_left_pattern'], r['turn_test'])
        for ray in r['rays']:
            print(f"  ray {ray['name']:12s} {math.degrees(ray['angle_rad']):6.1f} deg -> {ray['distance_m'] if ray['distance_m'] is None else round(ray['distance_m'], 3)} {ray['body'] or ''}")
    if args.json:
        Path(args.json).write_text(json.dumps(results, indent=1))


if __name__ == '__main__':
    main()
