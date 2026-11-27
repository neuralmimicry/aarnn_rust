#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Open-loop test of learned food seeking in the C. elegans connectome.

The worm keeps its connectome. Only the synapses from the food-sensing
amphid neurons (AWA, AWC, ASE) onto the first-layer interneurons (AIY, AIA,
AIB, AIZ) learn (STDP by default), and only while the global dopamine pool is raised
(`aarnn_modulator_pool=dopamine`). Learning at these synapses matches
C. elegans food-odour and salt learning (Tomioka et al. 2006; Kauffman et
al. 2010; Hukema et al. 2008). Food raises dopamine through the neuromodulator
drives, which stands in for the CEP / ADE / PDE cells that sense bacteria.

The network runs in the Rust probe (``target/release/examples/celegans_depth_probe``);
this script only writes the learner fields and the sensory schedule, then
reads the series back.

1. Caretaker phase (``--cycles`` cycles, plastic):
   - ``--drain-s`` of faint, slowly wandering odour on both amphids;
   - then ``--feed-s`` of food at the nose (taste and odour at 1), the
     caretaker's feed.
   The learned network is exported.
2. Frozen tests (plasticity off), naive and learned. Odour on both sides
   rises from the background to 1 over 20 s, holds 5 s, falls over 20 s,
   then pauses 10 s; three repeats. Taste stays at rest.
   Reported: interneuron and command-cell rates by phase and odour band,
   and the learned weight change at the plastic synapses.

  python3 scripts/qa/celegans_forage_openloop.py NETWORK OUTDIR [--cycles 6] [--set k=v ...]
"""
import argparse
import json
import math
import os
import statistics
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from aarnn_circuit import sensory_modality  # noqa: E402

PROBE = ROOT / "target/release/examples/celegans_depth_probe"
DT = 0.02
POST = ("AIY", "AIA", "AIB", "AIZ")
PRE = ("AWA", "AWC", "ASE")
WATCH = ("AIY", "AIZ", "AIA", "AIB", "RIA", "AVB", "AVA", "SMD", "RIM")
TASTE = "celegans_s_12_taste_front_center"
ODOUR = ("celegans_s_15_chem_left", "celegans_s_16_chem_right")


def cls(name):
    """Bilateral class of a cell name (AIYL -> AIY)."""
    return name[:-1] if name[-1:] in "LR" and len(name) > 3 else name


def learner_fields(hidden, n_out, w_max, threshold, ref, eta, rule):
    return dict(
        aarnn_plasticity_enabled=True,
        aarnn_plastic_cells=dict(enabled=True, hidden=[[cls(h) in POST for h in hidden]], outputs=[False] * n_out,
                                 newborns=False, newborn_outputs=False, pre_hidden=[[cls(h) in PRE for h in hidden]]),
        aarnn_matrix_plasticity_rule=rule, aarnn_plasticity_w_max=w_max, aarnn_plasticity_eta=eta,
        aarnn_rate_homeostasis_gain=0.0,
        aarnn_modulatory_cells=dict(enabled=False, hidden=[], outputs=[], newborns=False, pre_hidden=[], newborn_outputs=False),
        aarnn_modulator_pool="dopamine", aarnn_modulator_pool_threshold=threshold, aarnn_modulator_pool_ref=ref)


def rest_values(names, trace):
    """Each channel's median over a closed-loop walk (the habitat's own background)."""
    with open(trace) as f:
        head = json.loads(f.readline())
        ix = [head["sensor_names"].index(n) for n in names]
        cols = [[] for _ in names]
        for line in f:
            try:
                r = json.loads(line)
            except json.JSONDecodeError:
                break
            if r.get("sensors"):
                for k, i in enumerate(ix):
                    cols[k].append(r["sensors"][i])
    return [statistics.median(c) if c else 0.0 for c in cols]


def write_input(path, names, outputs, fn, total_s, rest):
    with open(path, "w") as f:
        f.write(json.dumps({"profile": "celegans", "sensor_names": names, "output_names": outputs}) + "\n")
        k, t = 0, 0.0
        while t < total_s:
            v = list(rest)
            for ch, x in fn(t).items():
                v[names.index(ch)] = x
            f.write(json.dumps({"time_s": round(t, 3), "step": k * 10, "sensors": [round(x, 4) for x in v]}) + "\n")
            k += 1
            t += DT


def run_probe(net, inp, out, ms, sets, tags, export=None, seed=1):
    trace = out / "neuromod.jsonl"
    trace.unlink(missing_ok=True)
    cmd = [str(PROBE), "--network", str(Path(net).resolve()), "--ms", str(int(ms)), "--input", str(inp),
           "--out", str(out / "o.json"), "--series", str(out / "s.jsonl"), "--series-ms", "100", "--no-raster",
           "--seed", str(seed), "--set", "aarnn_sensory_modalities=" + json.dumps(tags)]
    for kv in sets:
        cmd += ["--set", kv]
    if export:
        cmd += ["--export-after", str(Path(export).resolve())]
    env = dict(os.environ, NM_DISABLE_OPENCL="1", RAYON_NUM_THREADS="2", NM_NEUROMOD_TRACE=str(trace),
               NM_NEUROMOD_SAMPLE_EVERY="20")
    p = subprocess.run(cmd, cwd=ROOT, env=env, capture_output=True, text=True)
    if p.returncode:
        raise SystemExit(p.stderr[-2000:])
    o = json.loads((out / "o.json").read_text())
    ser = [json.loads(line) for line in open(out / "s.jsonl")]
    nm = [json.loads(line) for line in trace.read_text().splitlines() if line.strip()] if trace.exists() else []
    return o["hidden_names"], ser, nm


def plastic_weights(snap):
    """Sum of w_hh_rec[post, pre] over the plastic pairs, per post class."""
    hidden = snap["connectome_labels"]["hidden_nodes"]
    m = snap["w_hh_rec"][0]
    cols, data = m["cols"], m["data"]
    out = {c: 0.0 for c in POST}
    for i, post in enumerate(hidden):
        if cls(post) not in POST:
            continue
        for j, pre in enumerate(hidden):
            if cls(pre) in PRE:
                out[cls(post)] += data[i * cols + j]
    return {k: round(v, 3) for k, v in out.items()}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("network")
    ap.add_argument("out")
    ap.add_argument("--cycles", type=int, default=6)
    ap.add_argument("--drain-s", type=float, default=40.0)
    ap.add_argument("--feed-s", type=float, default=20.0)
    ap.add_argument("--rest-trace", default=str(ROOT / "target/qa/neuromod/celegans_cl/walk_mod_v3/trace/celegans_0.jsonl"))
    ap.add_argument("--w-max", type=float, default=0.6)
    ap.add_argument("--eta", type=float, default=0.0)
    # Oja with spike-coded pre/post (1 ms steps, 5-10 Hz cells) decays every
    # synapse toward P(pre spike | post spike) ~ 0.01: on 26 Nov it cut all 26
    # food-sense -> interneuron synapses by 60-70 %. Hebb counts only same-step
    # coincidences. STDP (traces) potentiates causal sensory -> interneuron
    # pairs: dopamine-gated STDP (Izhikevich 2007).
    ap.add_argument("--rule", default="stdp", choices=("stdp", "hebb", "oja"))
    ap.add_argument("--pool-threshold", type=float, default=1.5)
    ap.add_argument("--pool-ref", type=float, default=0.3)
    ap.add_argument("--set", action="append", default=[])
    ap.add_argument("--seed", type=int, default=1)
    a = ap.parse_args()
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    sys.stdout.reconfigure(line_buffering=True)
    snap = json.loads(Path(a.network).read_text())
    labels = snap["connectome_labels"]
    names = labels["sensory_nodes"]
    hidden = labels["hidden_nodes"]
    io = json.loads((ROOT / "sim/content/embodiments/generated/celegans.json").read_text())["io"]
    by_name = {s["name"]: s for s in io["sensors"]}
    tags = [sensory_modality(by_name[n]) if n in by_name else "other" for n in names]
    rest = rest_values(names, a.rest_trace)
    odour_rest = sum(rest[names.index(n)] for n in ODOUR) / 2
    print(f"rest: taste {rest[names.index(TASTE)]:.3f}, odour {odour_rest:.3f}")

    snap["net"].update(learner_fields(hidden, len(labels["output_nodes"]), a.w_max, a.pool_threshold, a.pool_ref, a.eta, a.rule))
    learner = out / "learner.json"
    learner.write_text(json.dumps(snap))
    cyc = a.drain_s + a.feed_s

    def learn(t):
        k, u = int(t // cyc), t % cyc
        if u < a.drain_s:
            w = 0.1 * math.sin(0.7 * t + k)
            return {ODOUR[0]: odour_rest + w, ODOUR[1]: odour_rest - w}
        return {TASTE: 1.0, ODOUR[0]: 1.0, ODOUR[1]: 1.0}

    learn_s = a.cycles * cyc + 5.0
    (out / "learn").mkdir(exist_ok=True)
    write_input(out / "learn/input.jsonl", names, io["output_names"][: len(labels["output_nodes"])], learn, learn_s, rest)
    learned = out / "learned.json"
    _, _, nm = run_probe(learner, out / "learn/input.jsonl", out / "learn", learn_s * 1000, a.set, tags, export=learned,
                         seed=a.seed)
    gate = {"drain": [], "feed": []}
    for r in nm:
        t = r["t_ms"] / 1000.0
        if t >= a.cycles * cyc:
            continue
        gate["feed" if t % cyc >= a.drain_s else "drain"].append(r["dopamine"])
    mean = lambda xs: sum(xs) / len(xs) if xs else float("nan")
    open_frac = {k: round(sum(1 for x in v if x > a.pool_threshold) / max(1, len(v)), 3) for k, v in gate.items()}
    print(f"dopamine during learning: drain {mean(gate['drain']):.3f}, feed {mean(gate['feed']):.3f}; "
          f"gate open: drain {open_frac['drain']}, feed {open_frac['feed']}")
    w0, w1 = plastic_weights(snap), plastic_weights(json.loads(learned.read_text()))
    print(f"plastic weight sums (pre {'/'.join(PRE)} -> post): before {w0}, after {w1}")

    period = 55.0

    def odour_at(u):
        lo = odour_rest
        if u < 20:
            return lo + (1 - lo) * u / 20
        if u < 25:
            return 1.0
        if u < 45:
            return 1.0 - (1 - lo) * (u - 25) / 20
        return lo

    def phase(u):
        return "up" if u < 20 else "hold" if u < 25 else "down" if u < 45 else "pause"

    test_s = 3 * period
    report = dict(rest_odour=round(odour_rest, 3), dopamine_learn={k: round(mean(v), 3) for k, v in gate.items()},
                  gate_open=open_frac, weights_before=w0, weights_after=w1)
    for label, net in (("naive", learner), ("learned", learned)):
        d = out / f"test_{label}"
        d.mkdir(exist_ok=True)
        write_input(d / "input.jsonl", names, io["output_names"][: len(labels["output_nodes"])],
                    lambda t: {ODOUR[0]: odour_at(t % period), ODOUR[1]: odour_at(t % period)}, test_s, rest)
        hn, ser, _ = run_probe(net, d / "input.jsonl", d, test_s * 1000, a.set + ["aarnn_plasticity_enabled=false"], tags,
                               seed=a.seed)
        groups = {c: [i for i, h in enumerate(hn) if cls(h) == c] for c in WATCH}
        by_phase = {p: {c: [] for c in WATCH} for p in ("up", "hold", "down", "pause")}
        bands = {b: {c: [] for c in WATCH} for b in ("rest", "mid", "high")}
        for r in ser:
            t = r["t_ms"] / 1000.0
            if t < 2.0:
                continue
            u = t % period
            o = odour_at(u)
            band = "high" if o >= 0.8 else "mid" if o > odour_rest + 0.05 else "rest"
            for c, ii in groups.items():
                if ii:
                    v = mean([r["hidden_hz"][i] for i in ii])
                    by_phase[phase(u)][c].append(v)
                    bands[band][c].append(v)
        report[label] = dict(by_phase={p: {c: round(mean(v), 2) for c, v in q.items()} for p, q in by_phase.items()},
                             by_odour={b: {c: round(mean(v), 2) for c, v in q.items()} for b, q in bands.items()})
    (out / "summary.json").write_text(json.dumps(report, indent=1))
    for label in ("naive", "learned"):
        print(label)
        for b in ("rest", "mid", "high"):
            print(f"  odour {b:4s}: " + "  ".join(f"{c} {v:5.2f}" for c, v in report[label]["by_odour"][b].items()))
        for p in ("up", "down"):
            print(f"  {p:4s}      : " + "  ".join(f"{c} {v:5.2f}" for c, v in report[label]["by_phase"][p].items()))


if __name__ == "__main__":
    main()
