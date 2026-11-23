#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Compare open-loop NAO learning runs (scripts/qa/nao_learn_openloop.py).

For each condition directory (paired / CS-only / frozen ...), per approach:

* onset_m   - distance at which the obstacle-side learning-turn cell LT first
              exceeds an absolute threshold (half the frozen network's reflex
              peak, so every condition is read with the same ruler);
* lt_far_hz - mean LT activity while the obstacle is 0.6-1.2 m away, beyond
              the innate reflex (< 0.4 m): the learned, anticipatory part;
* w_sum     - summed sonar-group -> LT efficacy at the end of the approach
              (from the probe's --watch-rows log).

  python3 scripts/qa/nao_learn_report.py DIR_PAIRED DIR_CSONLY DIR_FROZEN [--labels a b c] [--json OUT]
"""
import argparse
import json
from pathlib import Path

import numpy as np

EPISODE_S = 30.0
APPROACH_S = 20.0
T0 = 3.0
FAR_M = 2.2


def load(d):
    d = Path(d)
    o = json.loads((d / "o.json").read_text())
    ser = [json.loads(l) for l in open(d / "s.jsonl")]
    summ = json.loads((d / "summary.json").read_text()) if (d / "summary.json").exists() else {}
    return o["hidden_names"], np.array([r["t_ms"] for r in ser]) / 1000.0, \
        np.array([r["hidden_hz"] for r in ser], float), [r.get("weights") or {} for r in ser], summ


def near_of(d):
    inp = Path(d) / "input.jsonl"
    # the hold distance is the minimum scripted distance (both sides share REACH 2.55 m)
    rows = [json.loads(l) for l in open(inp)][1:]
    return rows


def analyse(d, thr, near_m):
    hn, T, H, W, _ = load(d)
    eps = []
    for e in range(int((T[-1] - T0) // EPISODE_S)):
        side = "l" if e % 2 == 0 else "r"
        lt = H[:, hn.index(f"LT_{side}")]
        u = T - T0 - e * EPISODE_S
        app = (u >= 0) & (u < APPROACH_S)
        dist = FAR_M - (FAR_M - near_m) * u / APPROACH_S
        on = np.flatnonzero(app & (lt > thr))
        far = app & (dist >= 0.6) & (dist <= 1.2)
        end = np.flatnonzero(app)
        w = W[end[-1]].get(f"LT_{side}", {}) if end.size else {}
        cs = {k: v for k, v in w.items() if k.startswith("CS_")}
        eps.append(dict(episode=e, side=side, onset_m=round(float(dist[on[0]]), 3) if on.size else None,
                        lt_far_hz=round(float(lt[far].mean()), 3) if far.any() else 0.0,
                        w_sum=round(float(sum(cs.values())), 4), w_max=round(float(max(cs.values(), default=0)), 4),
                        w_nonsilent=sum(1 for v in cs.values() if v > 1e-3)))
    return eps


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("dirs", nargs="+")
    ap.add_argument("--labels", nargs="+")
    ap.add_argument("--near", nargs="+", type=float, help="hold distance per dir (default 0.25, csonly 0.5)")
    ap.add_argument("--frozen", help="dir whose reflex peak sets the threshold (default: last dir)")
    ap.add_argument("--json")
    a = ap.parse_args()
    labels = a.labels or [Path(d).name for d in a.dirs]
    nears = a.near or [0.5 if "csonly" in d else 0.25 for d in a.dirs]
    hn, T, H, _, _ = load(a.frozen or a.dirs[-1])
    peak = max(H[:, hn.index("LT_l")].max(), H[:, hn.index("LT_r")].max())
    thr = 0.5 * peak
    out = dict(threshold_hz=thr, frozen_peak_hz=peak, conditions={})
    print(f"threshold {thr:.2f} Hz (half the frozen reflex peak {peak:.2f} Hz)")
    for d, lab, nm in zip(a.dirs, labels, nears):
        eps = analyse(d, thr, nm)
        out["conditions"][lab] = dict(dir=str(d), near_m=nm, episodes=eps)
        print(f"\n## {lab} (hold {nm} m)")
        print("ep side onset_m  LT@0.6-1.2m  w_sum   w_max  nonsilent")
        for r in eps:
            print(f"{r['episode']:2d}  {r['side']}   {str(r['onset_m']):6s}  {r['lt_far_hz']:7.2f}    "
                  f"{r['w_sum']:.4f}  {r['w_max']:.4f}  {r['w_nonsilent']}")
    if a.json:
        Path(a.json).write_text(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
