#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Report for the "groups of groups" and storage studies (23 Nov 2026).

* modular  (target/qa/scaling_modular/{clump,mod20,mod12,parts}): same cells,
  inputs and patterns per seed; recurrent wiring = interwoven clump (20
  random partners per cell), groups of 20 or 12 cells fully wired inside with
  2-synapse attachments between groups (collections of 20/12 groups), or none.
* hierarchy (target/qa/scaling_hierarchy): N = 320, sparse groups (each pair
  inside a group connected with p = 0.5) of G cells, collections of B groups.
* storage  (target/qa/storage): capacity (largest load recalled >= 90 %) of
  sparse 8-cell experiences vs the number of excitatory neurons N, with an
  inhibitory interneuron pool (20 % of cells).

Decoding: 32 patterns, leave-one-out nearest centroid (chance 1/32).
Paired tests: exact two-sided sign test over (N, seed) pairs.

  python3 scripts/qa/modular_storage_report.py
"""
import importlib.util
import json
import math
from collections import defaultdict
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("ss", ROOT / "scripts/qa/scaling_study.py")
ss = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ss)
OUT = ROOT / "target/qa/modular_storage_report.json"


def sign_p(dif):
    pos = int(np.sum(dif > 0))
    n = int(np.sum(dif != 0))
    if n == 0:
        return 1.0, 0, 0
    tail = sum(math.comb(n, i) for i in range(max(pos, n - pos), n + 1)) / 2 ** n
    return min(1.0, 2 * tail), pos, n


def acc_of(path):
    d = json.loads(Path(path).read_text())
    return ss.decode(d["records"], "evoked", 32)


def modular():
    D = ROOT / "target/qa/scaling_modular"
    res = defaultdict(dict)
    for c in ("clump", "mod20", "mod12", "parts"):
        for f in (D / c).glob("run_N*_s*.json"):
            n, s = f.stem.split("_")[1:3]
            res[(int(n[1:]), int(s[1:]))][c] = acc_of(f)
    sizes = sorted({n for n, _ in res})
    table = {}
    for n in sizes:
        row = {}
        for c in ("clump", "mod20", "mod12", "parts"):
            v = [res[(n, s)][c] for s in (1, 2, 3) if c in res.get((n, s), {})]
            if v:
                row[c] = float(np.mean(v))
        table[n] = row
    tests = {}
    for a, b in (("mod20", "clump"), ("mod20", "parts"), ("clump", "parts"), ("mod12", "parts"), ("mod20", "mod12")):
        pairs = [(v[a], v[b]) for v in res.values() if a in v and b in v]
        if pairs:
            dif = np.array([x - y for x, y in pairs])
            p, pos, nz = sign_p(dif)
            tests[f"{a}_vs_{b}"] = dict(pairs=len(pairs), mean_diff=float(dif.mean()), better=pos, nonzero=nz,
                                        sign_p=p, rel_gain=float(np.mean([x for x, _ in pairs]) /
                                                                 max(np.mean([y for _, y in pairs]), 1e-9) - 1))
    return dict(table=table, tests=tests)


def hierarchy():
    D = ROOT / "target/qa/scaling_hierarchy"
    groups = defaultdict(list)
    for f in D.glob("*.json"):
        key = f.stem.rsplit("_s", 1)[0]
        groups[key].append(acc_of(f))
    return {k: dict(mean=float(np.mean(v)), sd=float(np.std(v)), n=len(v)) for k, v in sorted(groups.items())}


def storage():
    D = ROOT / "target/qa/storage"
    by = defaultdict(list)
    for f in D.glob("run_N*_s*.json"):
        d = json.loads(f.read_text())
        by[d["neurons"]].append(d)
    rows = {}
    for n in sorted(by):
        caps = [d["capacity"] for d in by[n]]
        bits = [d["bits_at_capacity"] for d in by[n]]
        rows[n] = dict(capacity=float(np.mean(caps)), capacity_sd=float(np.std(caps)), seeds=len(caps),
                       bits=float(np.mean(bits)), bits_per_neuron=float(np.mean(bits)) / n,
                       total_cells=by[n][0].get("total_cells"))
    ns = [n for n in rows if rows[n]["capacity"] > 0]
    fit = {}
    if len(ns) >= 3:
        x = np.log(ns)
        for key in ("capacity", "bits"):
            y = np.log([rows[n][key] for n in ns])
            k, c = np.polyfit(x, y, 1)
            fit[key] = dict(exponent=float(k), prefactor=float(math.exp(c)))
    return dict(rows=rows, fit=fit)


def main():
    out = dict(modular=modular(), hierarchy=hierarchy(), storage=storage())
    OUT.write_text(json.dumps(out, indent=1))
    m = out["modular"]
    print("## modular: 32-pattern accuracy (mean of seeds)")
    for n, row in m["table"].items():
        print(n, {k: round(v, 3) for k, v in row.items()})
    for k, t in m["tests"].items():
        print(f"{k}: diff {t['mean_diff']:+.3f} ({100 * t['rel_gain']:+.0f} %), better {t['better']}/{t['nonzero']}, "
              f"sign p={t['sign_p']:.2g}")
    print("## hierarchy (N = 320)")
    for k, v in out["hierarchy"].items():
        print(f"{k}: {v['mean']:.3f} ± {v['sd']:.3f} (n={v['n']})")
    print("## storage")
    for n, r in out["storage"]["rows"].items():
        print(n, r)
    print("fit", out["storage"]["fit"])


if __name__ == "__main__":
    main()
