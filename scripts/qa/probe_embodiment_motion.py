#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Profile-agnostic locomotion metrics from a native MuJoCo trace.

Every ``aarnn-mujoco --trace`` record carries all body positions, the
actuated joint angles and the full ``qpos``. This probe measures what any
embodiment's movement can be judged by, without profile-specific code:

* locomotion of the root body (first body): net displacement, path length,
  mean speed, in metres and in body lengths (the body's initial extent), and
  travel speed (path of the root averaged over one locomotor cycle; used for
  the target range);
* posture: root height relative to the start, lowest height, whether the body
  fell (root below 50 % of its initial height; only for bodies standing at
  least 10 % of their length);
* joint activity: per-joint angle range, number of joints moving > 0.05 rad,
  dominant joint frequency (FFT of centred angles), mean |phase| between
  joints with mirrored ``_l``/``_r``-style names (alternation = pi);
* exploration: area of the root's convex hull and grid cells visited (cells
  of one body length).

It does not decide what is biomimetic for a species; ``TARGETS`` lists the
published ranges used by the summary for the profiles that have them.

usage: probe_embodiment_motion.py TRACE.jsonl [--json]
"""
import argparse
import json
import math
import re
from pathlib import Path

import numpy as np

# Published locomotion ranges per profile (speed in body lengths / s,
# dominant joint/body frequency in Hz). Engineering targets, not calibration.
TARGETS = {
    'celegans': dict(speed=(0.05, 0.4), freq=(0.25, 0.7), ref='Stephens 2008; Fang-Yen 2010'),
    'hexapod': dict(speed=(0.1, 1.0), freq=(0.5, 3.0), ref='Freenove hexapod tripod gait; insect stepping 1-10 Hz'),
    # The zebrafish surrogate is adult-proportioned (8 cm trunk): adult routine
    # swimming, not larval beat-and-glide (10-40 Hz tail beats).
    'zebrafish': dict(speed=(0.3, 3.0), freq=(3.0, 12.0), ref='adult routine swimming ~0.5-2 L/s, tail beat ~4-10 Hz (Plaut 2000; Palstra et al. 2010)'),
    'drosophila_banc': dict(speed=(0.5, 10.0), freq=(5.0, 20.0), ref='walking ~1-3 cm/s (Strauss & Heisenberg 1990)'),
    'drosophila_fafb': dict(speed=(0.5, 10.0), freq=(5.0, 20.0), ref='walking ~1-3 cm/s (Strauss & Heisenberg 1990)'),
    'nao': dict(speed=(0.05, 0.3), freq=(0.5, 2.0), ref='NAO walking ~0.1 m/s'),
    # Fliers are judged in m/s over the airborne part of the trace (their
    # spread-wing extent is not a body length).
    'crow_corvus_corone': dict(speed_mps=(8.0, 14.0), freq=(3.0, 5.0),
                               ref='engineering range for a 0.5 kg corvid in flapping flight (~8-14 m/s, ~3-5 Hz; Pennycuick 2008 scaling)'),
}


def load(path):
    lines = Path(path).read_text().splitlines()
    header = json.loads(lines[0])
    return header, [json.loads(x) for x in lines[1:]]


def convex_area(points):
    pts = sorted(set(map(tuple, np.round(points, 6))))
    if len(pts) < 3:
        return 0.0

    def cross(o, a, b):
        return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    lower, upper = [], []
    for p in pts:
        while len(lower) >= 2 and cross(lower[-2], lower[-1], p) <= 0:
            lower.pop()
        lower.append(p)
    for p in reversed(pts):
        while len(upper) >= 2 and cross(upper[-2], upper[-1], p) <= 0:
            upper.pop()
        upper.append(p)
    hull = lower[:-1] + upper[:-1]
    return 0.5 * abs(sum(hull[i][0] * hull[i - 1][1] - hull[i - 1][0] * hull[i][1] for i in range(len(hull))))


def mirrored_pairs(names):
    pairs = []
    for i, n in enumerate(names):
        for pat, rep in ((r'(^|_)l(_|$)', r'\1r\2'), (r'left', 'right'), (r'_L(\d|_|$)', r'_R\1'), (r'^l_', 'r_')):
            m = re.sub(pat, rep, n)
            if m != n and m in names:
                pairs.append((i, names.index(m)))
                break
    return pairs


def analyse(path):
    header, recs = load(path)
    recs = [r for r in recs if r.get('body_xyz')]
    if len(recs) < 8:
        raise SystemExit('trace too short')
    t = np.array([r['time_s'] for r in recs])
    xyz = np.array([r['body_xyz'] for r in recs])
    root = xyz[:, 0, :]
    q = np.array([r['joint_q'] for r in recs]) if recs[0].get('joint_q') else np.zeros((len(recs), 0))
    extent = np.ptp(xyz[0], axis=0)
    length = float(max(extent[0], extent[1], 1e-6))
    # Robots spawn above the ground and settle onto their feet: posture is
    # judged against the root height 0.5 s in, not the spawn height.
    settle = int(np.searchsorted(t, t[0] + 0.5))
    height0 = float(root[min(settle, len(root) - 1), 2])
    disp = root[-1, :2] - root[0, :2]
    path_len = float(np.linalg.norm(np.diff(root[:, :2], axis=0), axis=1).sum())
    dur = float(t[-1] - t[0]) or 1e-9
    names = header.get('actuated_joints') or [f'j{i}' for i in range(q.shape[1])]
    out = dict(profile=header.get('profile'), duration_s=dur, body_length_m=length,
               net_displacement_m=float(np.linalg.norm(disp)), net_displacement_L=float(np.linalg.norm(disp) / length),
               path_length_L=path_len / length, mean_speed_L_s=float(np.linalg.norm(disp) / length / dur),
               path_speed_L_s=path_len / length / dur,
               height_start_m=height0, height_min_m=float(root[:, 2].min()),
               # Falls only mean something for bodies standing >= 10 % of their length.
               fell=bool(height0 > 0.1 * length and root[settle:, 2].min() < 0.5 * height0))
    if q.size:
        centred = q - q.mean(axis=0)
        rng = np.ptp(q, axis=0)
        out['joints'] = int(q.shape[1])
        out['moving_joints'] = int((rng > 0.05).sum())
        out['mean_joint_range_rad'] = float(rng.mean())
        dt = float(np.median(np.diff(t)))
        spec = np.abs(np.fft.rfft(centred, axis=0)).sum(axis=1)
        freqs = np.fft.rfftfreq(len(t), dt)
        # Ignore steering/posture drifts well below the species' locomotor
        # band (a fish's slow turning bend otherwise outweighs its tail beat).
        target = TARGETS.get(header.get('profile'))
        floor = max(1.0 / dur, 0.5 * target['freq'][0]) if target else 1.0 / dur
        band = freqs > floor
        out['dominant_joint_hz'] = float(freqs[band][np.argmax(spec[band])]) if band.any() else None
        phases = []
        an = np.angle(np.fft.fft(centred, axis=0))
        k = int(np.argmax(spec[band])) + int(np.argmax(band)) if band.any() else 0
        for i, j in mirrored_pairs(list(names)):
            if rng[i] > 0.05 and rng[j] > 0.05 and k:
                d = (an[k, i] - an[k, j] + math.pi) % (2 * math.pi) - math.pi
                phases.append(abs(d))
        out['mirrored_pairs'] = len(phases)
        out['mirrored_phase_rad'] = float(np.mean(phases)) if phases else None
    # Travel speed: path length after averaging out one locomotor cycle, so
    # neither body wiggle (inflates raw path) nor turning back (deflates net
    # displacement) distorts how fast the animal actually travels.
    hz = out.get('dominant_joint_hz') or (TARGETS.get(out['profile'], {}).get('freq', (1.0,))[0])
    dt_rec = float(np.median(np.diff(t)))
    win = max(1, int(round(1.0 / max(hz, 1e-3) / dt_rec)))
    if len(root) > 2 * win:
        kernel = np.ones(win) / win
        smooth = np.column_stack([np.convolve(root[:, k], kernel, mode='valid') for k in range(2)])
        travel = float(np.linalg.norm(np.diff(smooth, axis=0), axis=1).sum())
        out['travel_speed_L_s'] = travel / length / (dt_rec * (len(smooth) - 1))
    # Fliers released in the air: time and distance before the root first
    # comes within 0.2 m of the ground (only for starts above 1 m).
    if root[0, 2] > 1.0:
        low = np.nonzero(root[:, 2] < 0.2)[0]
        end = int(low[0]) if len(low) else len(root) - 1
        out['airborne_s'] = float(t[end] - t[0])
        out['airborne_path_m'] = float(np.linalg.norm(np.diff(root[:end + 1, :2], axis=0), axis=1).sum())
        out['airborne_height_m'] = [float(root[:end + 1, 2].min()), float(root[:end + 1, 2].max())]
        if end > 1:
            v = np.linalg.norm(np.diff(root[:end + 1], axis=0), axis=1) / np.diff(t[:end + 1])
            out['airborne_speed_m_s'] = float(np.median(v))
        if q.size and end > 8:
            seg = q[:end + 1] - q[:end + 1].mean(axis=0)
            fa = np.fft.rfftfreq(len(seg), float(np.median(np.diff(t))))
            sa = np.abs(np.fft.rfft(seg, axis=0)).sum(axis=1)
            ok = fa > 1.0
            out['airborne_joint_hz'] = float(fa[ok][np.argmax(sa[ok])]) if ok.any() else None
    grid = {(int(math.floor(x / length)), int(math.floor(y / length))) for x, y in root[:, :2]}
    out['cells_visited'] = len(grid)
    out['hull_area_L2'] = convex_area(root[:, :2] / length)
    target = TARGETS.get(out['profile'])
    if target:
        if 'speed_mps' in target:
            lo, hi = target['speed_mps']
            speed = out.get('airborne_speed_m_s', path_len / dur)
            out['speed_in_range'] = lo <= speed <= hi
        else:
            lo, hi = target['speed']
            out['speed_in_range'] = lo <= out.get('travel_speed_L_s', out['mean_speed_L_s']) <= hi
        hz = out.get('airborne_joint_hz', out.get('dominant_joint_hz'))
        if hz is not None:
            flo, fhi = target['freq']
            out['freq_in_range'] = flo <= hz <= fhi
        out['target_ref'] = target['ref']
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('trace')
    ap.add_argument('--json', action='store_true')
    args = ap.parse_args()
    out = analyse(args.trace)
    if args.json:
        print(json.dumps(out))
    else:
        for k, v in out.items():
            print(f'{k:22s} {v}')


if __name__ == '__main__':
    main()
