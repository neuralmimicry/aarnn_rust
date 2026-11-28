#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Open-loop test of learned foraging / recharging (aarnn_circuit.add_learned_foraging).

The network runs in the Rust probe (``target/release/examples/celegans_depth_probe``);
this script only writes the sensory schedule and reads the series back.

1. Caretaker phase (``--cycles`` cycles, plastic):
   - the reserve (``needs_energy``) drains 0.9 -> 0.35 over ``--drain-s`` with
     faint, slowly wandering cues (0-0.25 per side);
   - then the caretaker presents food / plugs the robot in for ``--feed-s``:
     ``needs_consume`` = 1, both cues = 1, the reserve refills to 0.95.
   The learned network is exported.
2. Frozen tests (plasticity off), on the naive and on the learned network:
   - hungry (reserve 0.3) and sated (reserve 0.95);
   - the cue rises 0 -> 1 over 20 s (approach), holds 5 s, falls 1 -> 0 over
     20 s (leaving), pause 10 s; three repeats.
   Reported per condition:
   - value cell V by cue band;
   - RISE on the way up, FALL on the way down;
   - spontaneous turn-bout clock (BOUT_on) activity on the way up vs down.

  python3 scripts/qa/forage_openloop.py NETWORK OUTDIR [--cycles 6] [--set k=v ...]
"""
import argparse
import json
import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PROBE = ROOT / "target/release/examples/celegans_depth_probe"
DT = 0.02


def schedule(names, rows_fn, total_s, rest):
    """rows_fn(t) -> {channel: value}; every other channel at its rest value."""
    out, t = [], 0.0
    while t < total_s:
        v = list(rest)
        for ch, x in rows_fn(t).items():
            v[names.index(ch)] = x
        out.append((t, v))
        t += DT
    return out


def write_input(path, profile, names, outputs, rows):
    with open(path, "w") as f:
        f.write(json.dumps({"profile": profile, "sensor_names": names, "output_names": outputs}) + "\n")
        for k, (t, v) in enumerate(rows):
            f.write(json.dumps({"time_s": round(t, 3), "step": k * 10, "sensors": [round(x, 4) for x in v]}) + "\n")


def run_probe(net, inp, out, ms, sets, export=None, seed=1):
    cmd = [str(PROBE), "--network", str(Path(net).resolve()), "--ms", str(int(ms)), "--input", str(inp),
           "--out", str(out / "o.json"), "--series", str(out / "s.jsonl"), "--series-ms", "100", "--no-raster",
           "--seed", str(seed)]
    for kv in sets:
        cmd += ["--set", kv]
    if export:
        cmd += ["--export-after", str(Path(export).resolve())]
    p = subprocess.run(cmd, cwd=ROOT, env=dict(os.environ, NM_DISABLE_OPENCL="1", RAYON_NUM_THREADS="1"),
                       capture_output=True, text=True)
    if p.returncode:
        raise SystemExit(p.stderr[-1500:])
    o = json.loads((out / "o.json").read_text())
    ser = [json.loads(line) for line in open(out / "s.jsonl")]
    return o["hidden_names"], ser


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("network")
    ap.add_argument("out")
    ap.add_argument("--cycles", type=int, default=6)
    ap.add_argument("--drain-s", type=float, default=40.0)
    ap.add_argument("--feed-s", type=float, default=20.0)
    ap.add_argument("--set", action="append", default=[])
    ap.add_argument("--seed", type=int, default=1)
    a = ap.parse_args()
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    snap = json.loads(Path(a.network).read_text())
    profile = snap["connectome_labels"].get("profile") or "hexapod"
    io = json.loads((ROOT / f"sim/content/embodiments/generated/{profile}.json").read_text())["io"]
    names = [s["name"] for s in io["sensors"]]
    # Legacy profiles label sensory nodes with neural names (e.g. the fly's
    # imported connectome IDs); optional groups such as `needs` keep their
    # record names, so the needs channels are checked by name.
    if not all(n in snap["connectome_labels"]["sensory_nodes"] for n in ("needs_energy", "needs_consume", "needs_cue_left", "needs_cue_right")):
        raise SystemExit("network lacks the needs channels")
    # Rest values: joints and IMU mid-range, range sensors far (code 0), needs neutral.
    # legacy-range codes nearness (far = 0); archetype range_ray codes distance / 3 m (far = 1).
    rest = [0.0 if s["kind"] in ("legacy-range", "needs-consume", "needs-cue") else
            1.0 if s["kind"] in ("needs-energy", "range_ray") else 0.5 for s in io["sensors"]]
    cyc = a.drain_s + a.feed_s

    def learn(t):
        k, u = int(t // cyc), t % cyc
        if u < a.drain_s:
            e = 0.9 - 0.55 * u / a.drain_s
            # faint wandering cues
            import math
            cl = 0.125 + 0.125 * math.sin(0.7 * t + k)
            cr = 0.125 + 0.125 * math.sin(0.5 * t + 2 * k)
            return {"needs_energy": e, "needs_consume": 0.0, "needs_cue_left": cl, "needs_cue_right": cr}
        e = 0.35 + 0.6 * (u - a.drain_s) / a.feed_s
        return {"needs_energy": e, "needs_consume": 1.0, "needs_cue_left": 1.0, "needs_cue_right": 1.0}

    learn_s = a.cycles * cyc + 5.0
    (out / "learn").mkdir(exist_ok=True)
    write_input(out / "learn/input.jsonl", profile, names, io["output_names"], schedule(names, learn, learn_s, rest))
    learned = out / "learned.json"
    run_probe(a.network, out / "learn/input.jsonl", out / "learn", learn_s * 1000, a.set, export=learned, seed=a.seed)

    # Test: cue up 20 s, hold 5, down 20, pause 10; x3; hungry then sated.
    period = 55.0

    def cue_at(u):
        if u < 20:
            return u / 20
        if u < 25:
            return 1.0
        if u < 45:
            return 1.0 - (u - 25) / 20
        return 0.0

    def phase(u):
        return "up" if u < 20 else "hold" if u < 25 else "down" if u < 45 else "pause"

    def test(t):
        half = 3 * period
        e = 0.3 if t < half else 0.95
        u = (t % half) % period
        c = cue_at(u)
        return {"needs_energy": e, "needs_consume": 0.0, "needs_cue_left": c, "needs_cue_right": c}

    test_s = 2 * 3 * period
    rows = schedule(names, test, test_s, rest)
    frozen = a.set + ["aarnn_plasticity_enabled=false"]
    report = {}
    nets = [("naive", a.network), ("learned", learned)]
    # Newborns (growth): test a copy with every newborn silenced.
    n0 = len(snap["connectome_labels"]["hidden_nodes"])
    grown = len(json.loads(learned.read_text())["topo"]["layers"][0]) - n0
    if grown > 0:
        import sys
        sys.path.insert(0, str(ROOT / "scripts/qa"))
        from growth_learning_nao import knockout
        knockout(learned, out / "learned_ko.json", n0)
        nets.append(("knockout", out / "learned_ko.json"))
    report["hidden_grown"] = grown
    for label, net in nets:
        d = out / f"test_{label}"
        d.mkdir(exist_ok=True)
        write_input(d / "input.jsonl", profile, names, io["output_names"], rows)
        hn, ser = run_probe(net, d / "input.jsonl", d, test_s * 1000, frozen, seed=a.seed)
        ix = {n: hn.index(n) for n in ("V", "RISE", "FALL", "BOUT_on", "HUNGER") if n in hn}
        res = {}
        for state, lo, hi in (("hungry", 0.0, 3 * period), ("sated", 3 * period, 6 * period)):
            bands = {b: [] for b in ("cue<0.25", "0.25-0.5", "0.5-0.75", ">=0.75")}
            ph = {p: {"RISE": [], "FALL": [], "BOUT_on": []} for p in ("up", "down", "pause")}
            onsets, seen = [], set()
            hunger = []
            for r in ser:
                t = r["t_ms"] / 1000.0
                if not (lo + 2.0 <= t < hi):
                    continue
                u = (t - lo) % period
                c = cue_at(u)
                h = r["hidden_hz"]
                key = "cue<0.25" if c < 0.25 else "0.25-0.5" if c < 0.5 else "0.5-0.75" if c < 0.75 else ">=0.75"
                bands[key].append(h[ix["V"]])
                p = phase(u)
                rep_k = int((t - lo) // period)
                if p == "up" and rep_k not in seen and h[ix["V"]] > 2.0:
                    seen.add(rep_k)
                    onsets.append(c)
                if p in ph:
                    for n in ("RISE", "FALL", "BOUT_on"):
                        if n in ix:
                            ph[p][n].append(h[ix[n]])
                hunger.append(h[ix["HUNGER"]])
            mean = lambda xs: sum(xs) / len(xs) if xs else 0.0
            res[state] = dict(V_by_cue={k: round(mean(v), 2) for k, v in bands.items()},
                              onset_cue=round(mean(onsets), 3) if onsets else None,
                              by_phase={p: {n: round(mean(v), 2) for n, v in q.items()} for p, q in ph.items()},
                              hunger_hz=round(mean(hunger), 2))
        report[label] = res
    # learned weights CF -> V
    o = json.loads((out / "learn/o.json").read_text())
    report["learn"] = dict(wall_s=o.get("wall_s"))
    (out / "summary.json").write_text(json.dumps(report, indent=1))
    print(f"newborns grown during learning: {grown}")
    for label in [n for n, _ in nets]:
        for state in ("hungry", "sated"):
            r = report[label][state]
            print(f"{label:8s} {state:7s} HUNGER {r['hunger_hz']:5.2f} Hz  onset cue {r['onset_cue']}  V by cue {r['V_by_cue']}  "
                  f"up: RISE {r['by_phase']['up']['RISE']} BOUT_on {r['by_phase']['up']['BOUT_on']}  "
                  f"down: FALL {r['by_phase']['down']['FALL']} BOUT_on {r['by_phase']['down']['BOUT_on']}")


if __name__ == "__main__":
    main()
