#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Neuron-count scaling study: pattern separation and fading memory of
recurrent AARNN networks as the number of cells grows.

Runs ``examples/aarnn_scaling_probe`` for every size and seed (fixed
in-degree: each cell gets the same number of synapses at every size, so only
the number of cells changes), then decodes which of P input patterns was
shown from (a) the cells' spike counts while it was on ("evoked") and (b)
the counts in the window after it ended ("memory", the recurrent network's
fading trace). Decoder: leave-one-out nearest centroid on cosine
similarity; chance = 1/P. Accuracy is reported for the first 8, 16 and 32
patterns of each run, so one run gives the curve for three loads.

  python3 scripts/qa/scaling_study.py run --sizes 16 32 64 128 256 512 1024 --seeds 1 2 3
  python3 scripts/qa/scaling_study.py report
"""
import argparse
import concurrent.futures as cf
import json
import os
import subprocess
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
PROBE = ROOT / "target/release/examples/aarnn_scaling_probe"
OUT = ROOT / "target/qa/scaling"


def decode(records, key, n_patterns):
    rows = [r for r in records if r["pattern"] < n_patterns]
    X = np.array([r[key] for r in rows], dtype=float)
    y = np.array([r["pattern"] for r in rows])
    norms = np.linalg.norm(X, axis=1, keepdims=True)
    X = X / np.where(norms > 0, norms, 1.0)
    correct = 0
    for i in range(len(y)):
        mask = np.ones(len(y), bool)
        mask[i] = False
        cents = []
        for p in range(n_patterns):
            sel = mask & (y == p)
            cents.append(X[sel].mean(axis=0) if sel.any() else np.zeros(X.shape[1]))
        C = np.array(cents)
        cn = np.linalg.norm(C, axis=1)
        sims = C @ X[i] / np.where(cn > 0, cn, 1.0)
        correct += int(np.argmax(sims) == y[i])
    return correct / len(y)


def participation(records, key):
    X = np.array([r[key] for r in records], dtype=float)
    X = X - X.mean(axis=0)
    s = np.linalg.svd(X, compute_uv=False) ** 2
    return float(s.sum() ** 2 / max((s ** 2).sum(), 1e-12))


def run_one(n, seed, extra):
    out = OUT / f"run_N{n}_s{seed}.json"
    if out.exists():
        return out
    cmd = [str(PROBE), "--neurons", str(n), "--seed", str(seed), "--out", str(out)] + extra
    env = dict(os.environ, NM_DISABLE_OPENCL="1", RAYON_NUM_THREADS="1")
    subprocess.run(cmd, cwd=ROOT, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
    return out


def report():
    rows = {}
    for f in sorted(OUT.glob("run_N*_s*.json")):
        d = json.loads(f.read_text())
        rec = d["records"]
        r = dict(rate=d["stim_rate_hz"], wall=d["wall_s"])
        for P in (8, 16, 32):
            if P <= d["patterns"]:
                r[f"evoked_{P}"] = decode(rec, "evoked", P)
                r[f"memory_{P}"] = decode(rec, "memory", P)
        r["dim_evoked"] = participation(rec, "evoked")
        rows.setdefault(d["neurons"], []).append(r)
    summary = {}
    for n in sorted(rows):
        rs = rows[n]
        summary[n] = {k: dict(mean=float(np.mean([r[k] for r in rs])), sd=float(np.std([r[k] for r in rs])),
                              n=len(rs)) for k in rs[0]}
    (OUT / "summary.json").write_text(json.dumps(summary, indent=1))
    keys = [k for k in next(iter(summary.values())) if k.startswith(("evoked", "memory"))] + ["dim_evoked", "rate"]
    print("| N | " + " | ".join(keys) + " |")
    print("|---|" + "---|" * len(keys))
    for n, s in summary.items():
        print(f"| {n} | " + " | ".join(f"{s[k]['mean']:.3f} ± {s[k]['sd']:.3f}" for k in keys) + " |")
    return summary


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("action", choices=("run", "report"))
    ap.add_argument("--sizes", nargs="+", type=int, default=[16, 32, 64, 128, 256, 512, 1024])
    ap.add_argument("--seeds", nargs="+", type=int, default=[1, 2, 3])
    ap.add_argument("--jobs", type=int, default=6)
    ap.add_argument("--probe-args", nargs=argparse.REMAINDER, default=[])
    a = ap.parse_args()
    OUT.mkdir(parents=True, exist_ok=True)
    if a.action == "run":
        jobs = sorted(((n, s) for n in a.sizes for s in a.seeds), key=lambda x: -x[0])   # largest first
        with cf.ThreadPoolExecutor(max_workers=a.jobs) as ex:
            for out in ex.map(lambda j: run_one(j[0], j[1], a.probe_args), jobs):
                print("done", out.name, flush=True)
    report()


if __name__ == "__main__":
    main()
