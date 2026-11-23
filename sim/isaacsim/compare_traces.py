#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Side-by-side locomotion parity of two physics owners' traces of one body.

usage: compare_traces.py REFERENCE.jsonl CANDIDATE.jsonl [--run-json A B] [--json]

Uses scripts/qa/probe_embodiment_motion.py for the profile-agnostic metrics
and adds gait detail from the shared trace schema: per-joint stepping
frequency, foot duty factor (legacy foot-contact sensors), left/right tripod
anti-phase, root height/roll/pitch statistics and heading change. With
``--run-json`` the owners' evidence files add brain latency/exchange counts.
"""
import argparse
import json
import math
import sys
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts/qa'))
import probe_embodiment_motion as probe  # noqa: E402


def quat_to_rpy(q):
    w, x, y, z = q
    roll = math.atan2(2 * (w * x + y * z), 1 - 2 * (x * x + y * y))
    pitch = math.asin(max(-1.0, min(1.0, 2 * (w * y - z * x))))
    yaw = math.atan2(2 * (w * z + x * y), 1 - 2 * (y * y + z * z))
    return roll, pitch, yaw


def gait(path):
    header, recs = probe.load(path)
    recs = [r for r in recs if r.get('body_xyz')]
    t = np.array([r['time_s'] for r in recs])
    settle = t >= t[0] + 0.5
    out = {}
    if recs[0].get('qpos') and len(recs[0]['qpos']) >= 7:
        rpy = np.array([quat_to_rpy(r['qpos'][3:7]) for r in recs])
        z = np.array([r['qpos'][2] for r in recs])
        yaw = np.unwrap(rpy[:, 2])
        out.update(height_mean_m=float(z[settle].mean()), height_std_m=float(z[settle].std()),
                   roll_std_deg=float(np.degrees(rpy[settle, 0].std())),
                   pitch_std_deg=float(np.degrees(rpy[settle, 1].std())),
                   heading_change_deg=float(np.degrees(yaw[-1] - yaw[0])))
    names = header.get('actuated_joints') or []
    q = np.array([r['joint_q'] for r in recs])
    coxa = [i for i, n in enumerate(names) if n.endswith('_coxa')]
    if coxa:
        dt = float(np.median(np.diff(t)))
        freqs = []
        for i in coxa:
            x = q[settle, i] - q[settle, i].mean()
            spec = np.abs(np.fft.rfft(x))
            f = np.fft.rfftfreq(len(x), dt)
            band = f > 0.2
            freqs.append(float(f[band][np.argmax(spec[band])]))
        out['coxa_step_hz'] = [round(v, 3) for v in freqs]
        legs = {n.split('_')[-2]: i for i, n in enumerate(names) if n.endswith('_coxa')}
        if {'lf', 'rf', 'lm', 'rm'} <= set(legs):
            # Tripod: lf/rm/lr move together, opposite rf/lm/rr.
            def phase(a, b):
                xa = q[settle, legs[a]] - q[settle, legs[a]].mean()
                xb = q[settle, legs[b]] - q[settle, legs[b]].mean()
                return float(np.corrcoef(xa, xb)[0, 1])
            out['coxa_corr_lf_lm'] = phase('lf', 'lm')
            out['coxa_corr_lf_rf'] = phase('lf', 'rf')
    sensors = header.get('sensor_names') or []
    feet = [i for i, n in enumerate(sensors) if n.endswith('_foot')]
    if feet and recs[0].get('sensors'):
        s = np.array([r['sensors'] for r in recs if r.get('sensors')])
        out['foot_duty_factor'] = [round(float(v), 3) for v in (s[:, feet] > 0.5).mean(axis=0)]
        out['mean_feet_down'] = float((s[:, feet] > 0.5).sum(axis=1).mean())
    return out


def latency(path):
    if not path:
        return {}
    w = json.loads(Path(path).read_text())['worlds'][0]
    t = w.get('transport') or {}
    lat = sorted(t.get('latency_us') or [])
    return dict(physics=w.get('physics'), exchanges=t.get('exchanges'), output_spikes=t.get('output_spikes'),
                output_code=t.get('output_code'), wall_s=w.get('elapsed_s'),
                median_latency_ms=lat[len(lat) // 2] / 1000 if lat else None,
                p95_latency_ms=lat[int(0.95 * len(lat))] / 1000 if lat else None,
                applied=w.get('applied'), expired=w.get('expired'), active_steps=w.get('active_steps'))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('reference')
    ap.add_argument('candidate')
    ap.add_argument('--run-json', nargs=2, metavar=('REF', 'CAND'))
    ap.add_argument('--json', action='store_true')
    a = ap.parse_args()
    rows = {}
    for label, trace, run in (('reference', a.reference, a.run_json[0] if a.run_json else None),
                              ('candidate', a.candidate, a.run_json[1] if a.run_json else None)):
        m = probe.analyse(trace)
        m.update(gait(trace))
        m.update(latency(run))
        rows[label] = m
    if a.json:
        print(json.dumps(rows, indent=2))
        return 0
    keys = [k for k in rows['reference'] if k in rows['candidate']]
    width = max(len(k) for k in keys)
    print(f"{'metric':{width}}  {'reference':>22}  {'candidate':>22}")
    for k in keys:
        def fmt(v):
            if isinstance(v, float):
                return f'{v:.4g}'
            return str(v)
        print(f'{k:{width}}  {fmt(rows["reference"][k]):>22}  {fmt(rows["candidate"][k]):>22}')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
