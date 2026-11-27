#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Closed-loop obstacle score of a Freenove hexapod walk (lock-step MuJoCo trace).

Per window: share of time the HC-SR04 reads nearer than --near-m (the innate
turn's range), entries into that range, and path walked. Learning to turn
earlier shows as fewer and shorter near episodes over the run.

  python3 scripts/qa/hexapod_near_score.py WALKDIR [--window-s 100] [--near-m 0.40]
"""
import argparse
import glob
import json
import math


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("walk")
    ap.add_argument("--window-s", type=float, default=100.0)
    ap.add_argument("--near-m", type=float, default=0.40)
    ap.add_argument("--reach-m", type=float, default=3.0)
    ap.add_argument("--cell-m", type=float, default=0.25)
    a = ap.parse_args()
    with open(glob.glob(a.walk + "/trace/*.jsonl")[0]) as fh:
        head = json.loads(fh.readline())
        names = head["sensor_names"]
        son = [i for i, n in enumerate(names) if "ultrasonic" in n]
        rows = []
        for line in fh:
            try:
                r = json.loads(line)
            except json.JSONDecodeError:
                break
            if r.get("sensors") and r.get("qpos"):
                rows.append(r)
    win = {}
    prev_near, prev_xy = False, None
    stretch, longest, t_prev, cells = 0.0, 0.0, None, set()
    for r in rows:
        dt = 0.0 if t_prev is None else r["time_s"] - t_prev
        t_prev = r["time_s"]
        cells.add((math.floor(r["qpos"][0] / a.cell_m), math.floor(r["qpos"][1] / a.cell_m)))
        k = int(r["time_s"] // a.window_s)
        d = min(a.reach_m * (1.0 - r["sensors"][i]) for i in son)
        near = d < a.near_m
        stretch = stretch + dt if near and prev_near else 0.0
        longest = max(longest, stretch)
        w = win.setdefault(k, dict(n=0, near=0, entries=0, path=0.0))
        w["n"] += 1
        w["near"] += near
        w["entries"] += near and not prev_near
        xy = (r["qpos"][0], r["qpos"][1])
        if prev_xy:
            w["path"] += math.hypot(xy[0] - prev_xy[0], xy[1] - prev_xy[1])
        prev_near, prev_xy = near, xy
    out = []
    for k in sorted(win):
        w = win[k]
        out.append(dict(t0=k * a.window_s, near_frac=w["near"] / max(1, w["n"]), entries=w["entries"], path_m=w["path"]))
        print(f"{k * a.window_s:6.0f}-{(k + 1) * a.window_s:4.0f} s  near {100 * w['near'] / max(1, w['n']):5.1f} %  "
              f"entries {w['entries']:3d}  path {w['path']:.2f} m")
    tot = sum(w["near"] for w in win.values()) / max(1, sum(w["n"] for w in win.values()))
    print(f"TOTAL near {100 * tot:.1f} %  entries {sum(w['entries'] for w in win.values())}  "
          f"longest stretch {longest:.1f} s  explored cells ({a.cell_m} m) {len(cells)}")


if __name__ == "__main__":
    main()
