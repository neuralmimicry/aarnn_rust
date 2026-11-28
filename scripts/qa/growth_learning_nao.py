#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Learning through growth: NAO obstacle anticipation, growth on vs off.

The NAO learns by experience to turn before its innate near reflex: the reflex
(back off and turn) drives LC, the modulatory cell that gates plasticity, and
the silent synapses from distance-tuned sonar cells onto the learning-turn
cells LT strengthen when they predict it. The designed sonar groups cover a
fixed range of distances, so how early LT can learn to fire is bounded by the
designed anatomy. Growth adds neurons at energy peaks; a newborn inherits its
parent's inputs (lineage) with its own sensitivity, starts with silent output
synapses, and is plastic (a critical period). If growth carries learning, the
grown network anticipates earlier, and silencing its newborns takes the gain
away.

Per condition:
  1. learn: ``nao_learn_openloop.py`` (paired approaches, open loop), learned
     network exported;
  2. test: the learned network, plasticity frozen, newborns intact;
  3. knockout: the same with every newborn silenced (its type's tonic -100).

  python3 scripts/qa/growth_learning_nao.py NET OUTDIR NAME [--episodes 24] [--set k=v ...]
"""
import argparse
import json
import os
import statistics as st
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OPENLOOP = {"nao": ROOT / "scripts/qa/nao_learn_openloop.py", "hexapod": ROOT / "scripts/qa/hexapod_learn_openloop.py",
            "generic": ROOT / "scripts/qa/body_learn_openloop.py"}
NEWBORN = "Growth:newborn"


def run(net, out, episodes, sets, export=None, body="nao", seed=1):
    cmd = [sys.executable, str(OPENLOOP[body]), str(net), str(out), "--episodes", str(episodes), "--seed", str(seed)]
    for kv in sets:
        cmd += ["--set", kv]
    if export:
        cmd += ["--export-after", str(export)]
    p = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
    (Path(out).parent / (Path(out).name + ".log")).write_text(p.stdout + p.stderr)
    if p.returncode:
        raise SystemExit(f"{out}: {p.stderr[-1500:]}")
    return json.loads((Path(out) / "summary.json").read_text())


def onsets(summary):
    return [e["onset_m"] or 0.0 for e in summary["episodes"]]


def lt_bands(run_dir, body, ep_s=30.0, approach=20.0, far=2.2):
    """Mean learning-turn activity (Hz) by obstacle distance - far 1.5-2.2 m,
    mid 0.75-1.5 m, near < 0.75 m - and in the pause with no obstacle, over
    the run's last quarter of episodes. Anticipation raises far/mid activity
    while the pause stays quiet; a tonically active LT is not learning."""
    import numpy as np
    near = 0.35 if body == "nao" else 0.30
    edges = (1.5, 0.75)
    summ = json.loads((Path(run_dir) / "summary.json").read_text())
    if body == "generic":
        far, near = summ["protocol"]["far_m"], summ["protocol"]["near_m"]
        edges = (near + 2 * (far - near) / 3, near + (far - near) / 3)
    o = json.loads((Path(run_dir) / "o.json").read_text())
    hn = o["hidden_names"]
    idx = [i for i, n in enumerate(hn) if n in ("LT_l", "LT_r", "LT") or n.startswith("LT_")]
    ser = [json.loads(line) for line in open(Path(run_dir) / "s.jsonl")]
    T = np.array([r["t_ms"] for r in ser]) / 1000.0
    lt = np.array([max(r["hidden_hz"][i] for i in idx) for r in ser])
    u = (T - 3.0) % ep_s
    e = ((T - 3.0) // ep_s).astype(int)
    n_ep = max(1, int(e.max()))
    late = (T > 3.0) & (e >= n_ep - max(1, n_ep // 4))
    d = np.where(u < approach, far - (far - near) * u / approach, np.nan)
    pause = (u >= ep_s - 2.0)

    def band(mask):
        m = late & mask
        return float(lt[m].mean()) if m.any() else float("nan")
    return dict(far=band(d >= edges[0]), mid=band((d >= edges[1]) & (d < edges[0])), near=band(d < edges[1]),
                pause=band(pause))


def knockout(net, out, n0):
    """Silence every newborn (the cells past the designed n0), whatever its type:
    each gets a copy of its type with tonic -100 (newborns may inherit their
    parent's type, so silencing the generic newborn type is not enough)."""
    s = json.loads(Path(net).read_text())
    nodes = s["topo"]["layers"][0]
    types = {t["name"]: t for t in s["net"].get("neuron_types") or []}
    n = 0
    for node in nodes[n0:]:
        base = node.get("type_name") or NEWBORN
        if base not in types:
            continue
        ko = f"{base}@knockout"
        if ko not in types:
            t = json.loads(json.dumps(types[base]))
            t["name"] = ko
            t["bio_params"]["tonic_current"] = -100.0
            types[ko] = t
        node["type_name"] = ko
        n += 1
    s["net"]["neuron_types"] = list(types.values())
    Path(out).write_text(json.dumps(s))
    return n


def lt_inputs(net):
    """Summed |weight| onto LT from designed sonar cells, from newborns."""
    s = json.loads(Path(net).read_text())
    names = s["connectome_labels"]["hidden_nodes"]
    w = s["w_hh_rec"][0]
    rows, cols, data = w["rows"], w["cols"], w["data"]
    nodes = s["topo"]["layers"][0]
    n0 = len(names)
    res = {}
    targets = ([(n[3:], n, f"CS_{n[3:]}_") for n in names if n.startswith("LT_")] if any(n.startswith("LT_") for n in names)
               else [("l", "LT", "CS_")])
    for side, lt_name, cs_prefix in targets:
        j = names.index(lt_name)
        cs = sum(abs(data[j * cols + i]) for i in range(n0) if names[i].startswith(cs_prefix))
        nb = [abs(data[j * cols + i]) for i in range(n0, min(cols, len(nodes)))]
        res[side] = dict(designed_sonar=cs, newborn=sum(nb), newborn_n=sum(x > 1e-6 for x in nb),
                         newborn_max=max(nb) if nb else 0.0)
    return res


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("net")
    ap.add_argument("outdir")
    ap.add_argument("name")
    ap.add_argument("--episodes", type=int, default=24)
    ap.add_argument("--test-episodes", type=int, default=4)
    ap.add_argument("--set", action="append", default=[])
    ap.add_argument("--body", default="nao", choices=["nao", "hexapod", "generic"])
    ap.add_argument("--seed", type=int, default=1)
    a = ap.parse_args()
    out = Path(a.outdir)
    out.mkdir(parents=True, exist_ok=True)
    learned = out / f"{a.name}_learned.json"
    learn = run(a.net, out / f"{a.name}_learn", a.episodes, a.set, export=learned, body=a.body, seed=a.seed)
    freeze = [kv for kv in a.set if not kv.startswith("aarnn_plasticity_enabled=")] + ["aarnn_plasticity_enabled=false"]
    test = run(learned, out / f"{a.name}_test", a.test_episodes, freeze, body=a.body, seed=a.seed)
    ko_net = out / f"{a.name}_learned_ko.json"
    n0 = len(json.loads(Path(a.net).read_text())["connectome_labels"]["hidden_nodes"])
    k = knockout(learned, ko_net, n0)
    ko = run(ko_net, out / f"{a.name}_ko", a.test_episodes, freeze, body=a.body, seed=a.seed) if k else None
    o = onsets(learn)
    q = max(1, len(o) // 4)
    res = dict(name=a.name, sets=a.set, episodes=a.episodes, grown=learn.get("hidden_grown"),
               learn_onsets=o, learn_first=st.mean(o[:q]), learn_last=st.mean(o[-q:]),
               test_onset=st.mean(onsets(test)),
               knockout_onset=st.mean(onsets(ko)) if ko else None,
               lt_inputs=lt_inputs(learned),
               bands=dict(learn=lt_bands(out / f"{a.name}_learn", a.body), test=lt_bands(out / f"{a.name}_test", a.body),
                          knockout=lt_bands(out / f"{a.name}_ko", a.body) if ko else None))
    (out / f"{a.name}_result.json").write_text(json.dumps(res, indent=1))
    print(f"{a.name:14s} grown {res['grown']!s:>4s}  learn first {res['learn_first']:.2f} last {res['learn_last']:.2f}  "
          f"test {res['test_onset']:.2f}  knockout {res['knockout_onset'] if ko else float('nan'):.2f}  "
          + " ".join(f"LT_{k} inputs: sonar {v['designed_sonar']:.4f} newborn {v['newborn']:.4f} ({v['newborn_n']})"
                     for k, v in res['lt_inputs'].items()))
    for ph in ("learn", "test", "knockout"):
        b = res["bands"][ph]
        if b:
            print(f"   {ph:9s} LT Hz: far {b['far']:5.2f} mid {b['mid']:5.2f} near {b['near']:5.2f} pause {b['pause']:5.2f}")


if __name__ == "__main__":
    main()
