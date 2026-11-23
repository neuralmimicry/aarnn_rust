#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Neuron-by-neuron sweep: is the network more than the sum of its parts?

For every N = 1..64 and seed, `examples/aarnn_scaling_probe` ran the same
cells, input wiring, patterns and noise twice (target/run_1by1.sh):

* network - recurrent synapses on (20 per cell, or all-to-all below 21
  cells; 20 % inhibitory);
* parts   - the same cells with every recurrent synapse removed, so each
  neuron responds only to its own inputs.

Information about which of 32 patterns was shown is estimated from
leave-one-out nearest-centroid decoding accuracy with Fano's lower bound,
I >= log2 P - H(e) - e log2(P - 1), e = error rate (no plug-in bias). The
strict "sum of the parts" is also computed: every neuron decoded on its
own, its information summed over neurons.

  python3 scripts/qa/scaling_1by1.py   # writes target/qa/scaling_1by1/summary.json
"""
import importlib.util
import json
import math
from collections import defaultdict
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
D = ROOT / "target/qa/scaling_1by1"
spec = importlib.util.spec_from_file_location("ss", ROOT / "scripts/qa/scaling_study.py")
ss = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ss)


def fano_bits(acc, P):
    e = 1.0 - acc
    if acc <= 1.0 / P:
        return 0.0
    h = 0.0 if e in (0.0, 1.0) else -(e * math.log2(e) + (1 - e) * math.log2(1 - e))
    return max(0.0, math.log2(P) - h - e * math.log2(P - 1))


def single_cell_bits(records, P):
    """Fano bits of each neuron decoded alone (1-D nearest centroid on its
    spike count, leave-one-out)."""
    rows = [r for r in records if r["pattern"] < P]
    X = np.array([r["evoked"] for r in rows], dtype=float)
    y = np.array([r["pattern"] for r in rows])
    out = []
    for j in range(X.shape[1]):
        x = X[:, j]
        correct = 0
        for i in range(len(y)):
            m = np.ones(len(y), bool)
            m[i] = False
            cents = np.array([x[m & (y == p)].mean() if (m & (y == p)).any() else np.inf for p in range(P)])
            d = np.abs(cents - x[i])
            best = np.flatnonzero(d == d.min())
            correct += int(y[i] in best and len(best) == 1)
        out.append(fano_bits(correct / len(y), P))
    return out


def main():
    P = 32
    res = defaultdict(lambda: defaultdict(list))
    for cond in ("network", "parts"):
        for f in sorted((D / cond).glob("run_N*_s*.json")):
            d = json.loads(f.read_text())
            rec = d["records"]
            acc = ss.decode(rec, "evoked", P)
            row = dict(acc=acc, bits=fano_bits(acc, P), rate=d["stim_rate_hz"])
            if cond == "parts":
                row["sum_single_bits"] = float(sum(single_cell_bits(rec, P)))
            res[d["neurons"]][cond].append(row)
    summary = {}
    for n in sorted(res):
        s = {}
        for cond, rows in res[n].items():
            s[cond] = {k: dict(mean=float(np.mean([r[k] for r in rows])), sd=float(np.std([r[k] for r in rows])),
                               n=len(rows)) for k in rows[0]}
        summary[n] = s
    (D / "summary.json").write_text(json.dumps(summary, indent=1))
    print("| N | network acc | parts acc | network bits | parts bits | sum of single cells (bits) |")
    print("|---|---|---|---|---|---|")
    for n, s in summary.items():
        if "network" in s and "parts" in s:
            print(f"| {n} | {s['network']['acc']['mean']:.3f} | {s['parts']['acc']['mean']:.3f} | "
                  f"{s['network']['bits']['mean']:.2f} | {s['parts']['bits']['mean']:.2f} | "
                  f"{s['parts'].get('sum_single_bits', {}).get('mean', float('nan')):.2f} |")


if __name__ == "__main__":
    main()
