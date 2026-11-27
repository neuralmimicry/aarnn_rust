#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Spatial-mode calibration of designed circuits.

Designed circuits (``scripts/aarnn_circuit.py``) are calibrated with point
cells, where a drive in mV/ms is exact. Laid out in 3D with morphology and
conduction delays (``layout_designed_circuit.py --spatial``), every synapse
gets its own route: axon and dendrite length, dendritic compartment gain,
myelination, fatigue, release probability. Dendritic compartments also
integrate their inputs differently. A designed circuit then no longer does
what it was designed to do (24 Nov, measured):
- band subtractions failed (excitation ~1.37x and inhibition ~0.9x of
  nominal);
- plateau latches held against several times their designed inhibition;
- the Freenove hexapod CPG turned +82 deg instead of -4 deg.

This tool brings a spatial circuit back to its design, so that learning,
placement and growth start from circuits that work as designed:

  gains NET              dump every morphological synapse's route attenuation,
                         delay and release probability, as the runtime computes
                         them (probe ``--dump-synapse-gains``); summary by sign
  route NET OUT          divide each designed weight by its synapse's route
                         attenuation x release probability (exact, per synapse)
  compare FLAT SPAT RUN  replay a recorded closed-loop trace through the
                         point-cell and the spatial network; per-cell mean
                         activity in each and the worst mismatches
  fit FLAT SPAT RUN OUT  route correction, then per-cell offset correction (each
                         mismatched cell gets its own copy of its type with a
                         corrected tonic) for --iters passes, matching the
                         point-cell activity on the trace

FLAT is the network as built (point cells); SPAT is the same network laid
out with ``--spatial`` (and the body's attenuation / release settings). RUN
is a closed-loop run directory whose trace supplies the sensor stream.

  python3 scripts/calibrate_spatial_network.py fit target/qa/x/raw.json target/qa/x/raw_spatial.json \\
      target/qa/x/run_dir target/qa/x/raw_spatial_cal.json --iters 3
"""
import argparse
import glob
import json
import os
import statistics as st
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PROBE = ROOT / "target/release/examples/celegans_depth_probe"
ENV = dict(os.environ, NM_DISABLE_OPENCL="1", RAYON_NUM_THREADS="2")


def dump_gains(net):
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "gains.jsonl"
        p = subprocess.run([str(PROBE), "--network", str(net), "--dump-synapse-gains", str(out)],
                           cwd=ROOT, env=ENV, capture_output=True, text=True)
        if p.returncode:
            raise SystemExit(p.stderr[-1500:])
        return [json.loads(line) for line in out.read_text().splitlines() if line.strip()]


def gains(a):
    rows = dump_gains(a.net)
    if a.out:
        Path(a.out).write_text("\n".join(json.dumps(r) for r in rows) + "\n")
    if not rows:
        print("no morphological synapses (point cells: nothing to calibrate)")
        return
    exc = [r["attenuation"] * r["p_release"] for r in rows if r["weight"] > 0]
    inh = [r["attenuation"] * r["p_release"] for r in rows if r["weight"] < 0]
    print(f"{len(rows)} synapses; effective gain (attenuation x p_release):")
    for tag, v in (("excitatory", exc), ("inhibitory", inh)):
        if v:
            print(f"  {tag:10s} n={len(v):5d} median {st.median(v):.3f} min {min(v):.3f} max {max(v):.3f}")
    print(f"  delays: median {st.median(r['delay_steps'] for r in rows)} steps, max {max(r['delay_steps'] for r in rows)}")


def matrix_entry(snap, r):
    """(matrix dict, flat index) of a morphological synapse; rows = target."""
    hidden_layers = snap["net"].get("num_hidden_layers", 1)
    if r["pre_layer"] == -1 and r["post_layer"] == 0:
        m = snap["w_in"]
    elif r["pre_layer"] == 0 and r["post_layer"] == 0:
        m = snap["w_hh_rec"][0]
    elif r["pre_layer"] == hidden_layers - 1 and r["post_layer"] == hidden_layers:
        m = snap["w_out"]
    else:
        return None, None
    return m, r["post_id"] * m["cols"] + r["pre_id"]


def route_correct(spatial, out, clamp=(0.5, 2.0)):
    snap = json.loads(Path(spatial).read_text())
    rows = dump_gains(spatial)
    per = {}
    for r in rows:
        m, idx = matrix_entry(snap, r)
        if m is None:
            continue
        per.setdefault((id(m), idx), (m, idx, []))[2].append(r["attenuation"] * max(r["p_release"], 1e-3))
    n = 0
    for m, idx, g in per.values():
        eff = sum(g) / len(g)
        scale = 1.0 / min(max(eff, clamp[0]), clamp[1])
        if abs(m["data"][idx]) > 1e-12 and abs(scale - 1.0) > 1e-6:
            m["data"][idx] *= scale
            n += 1
    snap.setdefault("connectome_labels", {})["spatial_calibration"] = dict(route_corrected_synapses=n)
    Path(out).write_text(json.dumps(snap))
    return n


def route(a):
    n = route_correct(a.net, a.out)
    print(f"wrote {a.out}: {n} weights divided by their route attenuation x release probability")


def replay(net, run, t_max):
    trace = glob.glob(str(Path(run) / "trace" / "*.jsonl"))[0]
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        with open(trace) as fh, open(tmp / "in.jsonl", "w") as g:
            head = json.loads(fh.readline())
            g.write(json.dumps({"profile": head.get("profile", "nao"), "sensor_names": head["sensor_names"],
                                "output_names": head["output_names"],
                                "trace_every_steps": head["trace_every_steps"]}) + "\n")
            t_end = 0.0
            for line in fh:
                try:
                    r = json.loads(line)
                except json.JSONDecodeError:
                    break
                if r.get("sensors") and r["time_s"] <= t_max:
                    g.write(json.dumps({"time_s": r["time_s"], "step": r["step"], "sensors": r["sensors"]}) + "\n")
                    t_end = r["time_s"]
        p = subprocess.run([str(PROBE), "--network", str(net), "--ms", str(int(t_end * 1000)), "--input",
                            str(tmp / "in.jsonl"), "--out", str(tmp / "o.json"), "--series", str(tmp / "s.jsonl"),
                            "--series-ms", "100", "--no-raster", "--seed", "1"],
                           cwd=ROOT, env=ENV, capture_output=True, text=True)
        if p.returncode:
            raise SystemExit(p.stderr[-1500:])
        ser = [json.loads(x) for x in open(tmp / "s.jsonl")]
    n_h = min(len(r["hidden_hz"]) for r in ser)
    hidden = [st.mean(r["hidden_hz"][j] for r in ser) for j in range(n_h)]
    outs = [st.mean(r["output_activation"][k] for r in ser) for k in range(len(ser[0]["output_activation"]))]
    return hidden, outs


def compare_nets(flat, spatial, run, t_max):
    snap = json.loads(Path(flat).read_text())
    names = snap["connectome_labels"]["hidden_nodes"]
    onames = snap["connectome_labels"]["output_nodes"]
    hf, of = replay(flat, run, t_max)
    hs, os_ = replay(spatial, run, t_max)
    rows = [(names[j], hf[j], hs[j]) for j in range(min(len(names), len(hf), len(hs)))]
    orows = [(onames[k], of[k], os_[k]) for k in range(min(len(onames), len(of), len(os_)))]
    return rows, orows


def report(rows, orows, top=12):
    err = [abs(s - f) for _, f, s in rows]
    oerr = [abs(s - f) for _, f, s in orows]
    print(f"hidden: mean |spatial - flat| {st.mean(err):.2f} Hz, cells off by > 2 Hz: {sum(e > 2 for e in err)}/{len(err)}")
    if oerr:
        print(f"outputs: mean |spatial - flat| activation {st.mean(oerr):.4f}, > 0.02: {sum(e > 0.02 for e in oerr)}/{len(oerr)}")
    for name, f, s in sorted(rows, key=lambda r: -abs(r[2] - r[1]))[:top]:
        print(f"  {name:22s} flat {f:6.2f}  spatial {s:6.2f}")


def compare(a):
    rows, orows = compare_nets(a.flat, a.spatial, a.run, a.t_max)
    report(rows, orows)


def fit(a):
    """Route correction, then per-cell tonic corrections toward the point-cell activity."""
    route_correct(a.spatial, a.out)
    flat_snap = json.loads(Path(a.flat).read_text())
    graded_hz = 20.0
    for it in range(a.iters):
        rows, orows = compare_nets(a.flat, a.out, a.run, a.t_max)
        print(f"pass {it}:", end=" ")
        report(rows, orows, top=0 if it < a.iters - 1 else 8)
        snap = json.loads(Path(a.out).read_text())
        types = {t["name"]: t for t in snap["net"]["neuron_types"]}
        nodes = snap["topo"]["layers"][0]
        onodes = snap["topo"]["output_nodes"]
        changed = 0
        for j, (name, f, s) in enumerate(rows):
            d = (f - s) / graded_hz           # release fraction to recover
            if abs(f - s) <= a.tol_hz:
                continue
            changed += _shift(types, snap, nodes[j], name, a.gain * d)
        for k, (name, f, s) in enumerate(orows):
            if abs(f - s) <= a.tol_out:
                continue
            changed += _shift(types, snap, onodes[k], name, a.gain * (f - s))
        snap["net"]["neuron_types"] = list(types.values())
        cal = snap["connectome_labels"].setdefault("spatial_calibration", {})
        cal["tonic_passes"] = it + 1
        Path(a.out).write_text(json.dumps(snap))
        print(f"  corrected {changed} cells")
        if changed == 0:
            break
    rows, orows = compare_nets(a.flat, a.out, a.run, a.t_max)
    print("final:", end=" ")
    report(rows, orows)
    del flat_snap


def _shift(types, snap, node, cell, delta):
    """Give `cell` its own copy of its type with tonic + delta (mV/ms)."""
    base = node.get("type_name")
    if base is None or base not in types:
        return 0
    own = base if base.endswith("@" + cell) else f"{base}@{cell}"
    if own not in types:
        types[own] = json.loads(json.dumps(types[base]))
        types[own]["name"] = own
    bio = types[own]["bio_params"]
    bio["tonic_current"] = float(bio.get("tonic_current", 0.0)) + delta
    node["type_name"] = own
    return 1


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("gains")
    g.add_argument("net")
    g.add_argument("--out")
    r = sub.add_parser("route")
    r.add_argument("net")
    r.add_argument("out")
    for name in ("compare", "fit"):
        c = sub.add_parser(name)
        c.add_argument("flat")
        c.add_argument("spatial")
        c.add_argument("run")
        if name == "fit":
            c.add_argument("out")
            c.add_argument("--iters", type=int, default=3)
            c.add_argument("--gain", type=float, default=4.0, help="tonic change (mV/ms) per unit release mismatch")
            c.add_argument("--tol-hz", type=float, default=1.0)
            c.add_argument("--tol-out", type=float, default=0.01)
        c.add_argument("--t-max", type=float, default=120.0, help="seconds of the trace to replay")
    a = ap.parse_args()
    {"gains": gains, "route": route, "compare": compare, "fit": fit}[a.cmd](a)


if __name__ == "__main__":
    main()
