#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""NAO righting learned by experience: plastic network, replay, teacher off.

The network (``build_nao_walk_network_json.py --getup --getup-back
--getup-learn 1``) has body-state band cells (gravity sense and joint
sense) whose silent synapses onto one learning cell L per righting movement
are the only plastic ones; the timed keyframe chain is the teacher (its
handover cells X drive L and the LC modulatory cell that gates plasticity).

  make         plastic copy of a network (mask = L cells, LC gates)
  replay       replay a closed-loop experience trace (the robot falling and
               righting with the teacher on, ``NM_MUJOCO_REFALL_S``) through
               the plastic network open loop and export what it learned
  teacher-off  silence the teacher (the chain's handover cells X): each
               movement then starts when its learning cell recognises the
               body state, not when a timer says
  weights      summary of the learned band -> L synapses

  python3 scripts/qa/nao_getup_learning.py make RAW.json PLASTIC.json --config-in C.json --config-out C2.json
  python3 scripts/qa/nao_getup_learning.py replay PLASTIC.json RUN_DIR LEARNED.json
  python3 scripts/qa/nao_getup_learning.py teacher-off LEARNED.json TESTNET.json
"""
import argparse
import json
import os
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PROBE = ROOT / "target/release/examples/celegans_depth_probe"


def plastic_fields(hidden, outputs, eta, plastic=True, w_max=0.0):
    learn = [h.startswith(("L_getup_", "L_roll_", "L_rollm_")) for h in hidden]
    if not any(learn):
        raise SystemExit("no learning cells L_*: build with --getup-learn 1")
    # Only the body-state synapses learn (and newborns' synapses: newborns go
    # through a critical period); an entry cell's control wiring (mode,
    # adaptation, inhibition, oscillator bias) is fixed. With every input
    # plastic, that wiring grew to the clamp in 50 s and latched L_getup_00.
    pre = [h.startswith("B_") for h in hidden]
    return dict(
        aarnn_plasticity_enabled=plastic, aarnn_graded_plasticity_enabled=True,
        aarnn_plastic_cells=dict(enabled=True, hidden=[learn], outputs=[False] * len(outputs), newborns=True,
                                 pre_hidden=[pre]),
        aarnn_plasticity_w_max=w_max,
        aarnn_rate_homeostasis_tau_ms=600000.0, aarnn_rate_homeostasis_gain=0.0,
        aarnn_plasticity_eta=eta,
        aarnn_modulatory_cells=dict(enabled="LC" in hidden, hidden=[[h == "LC" for h in hidden]],
                                    outputs=[False] * len(outputs)),
        aarnn_modulator_tau_ms=1000.0, aarnn_modulator_ref_hz=10.0,
    )


def make(a):
    snap = json.loads(Path(a.raw).read_text())
    lab = snap["connectome_labels"]
    # Cap: a learned body-state synapse delivers at most --w-max-mv mV/ms
    # (the band -> L weights are drive_weight(w0) at full band release).
    gain = float((snap["net"].get("aarnn_bio") or {}).get("synaptic_gain", 100.0))
    w_max = a.w_max_mv / (20.0 / 1000.0 * 5.52 * gain) if a.w_max_mv > 0 else 0.0
    fields = plastic_fields(lab["hidden_nodes"], lab["output_nodes"], a.eta, bool(a.plastic), w_max)
    snap["net"].update(fields)
    for t in [snap["net"].get("aarnn_bio") or {}] + [t.get("bio_params") or {} for t in snap["net"].get("neuron_types") or []]:
        t["neuromodulation_enabled"] = False
    lab["learning"] = dict(plastic_cells=[h for h in lab["hidden_nodes"] if h.startswith("L_")], eta=a.eta,
                           teacher="timed keyframe chain (X handover cells)", modulatory_cells=["LC"],
                           rule="generic AARNN plasticity (matrix STDP + dendritic-bouton Hebbian/STDP/Oja mix)")
    Path(a.out).write_text(json.dumps(snap))
    if a.config_in:
        cfg = json.loads(Path(a.config_in).read_text())
        cfg.update(fields)
        Path(a.config_out).write_text(json.dumps(cfg, indent=1))
    print(f"wrote {a.out}: {sum(fields['aarnn_plastic_cells']['hidden'][0])} plastic cells, eta {a.eta}")


def replay(a):
    trace = next((Path(a.run) / "trace").glob("*.jsonl"))
    with open(trace) as f, tempfile.NamedTemporaryFile("w", suffix=".jsonl", delete=False) as g:
        head = json.loads(f.readline())
        g.write(json.dumps({"profile": "nao", "sensor_names": head["sensor_names"],
                            "output_names": head["output_names"], "trace_every_steps": head["trace_every_steps"]}) + "\n")
        t_end = 0.0
        for line in f:
            try:
                r = json.loads(line)
            except json.JSONDecodeError:
                break
            if r.get("sensors"):
                g.write(json.dumps({"time_s": r["time_s"], "step": r["step"], "sensors": r["sensors"]}) + "\n")
                t_end = r["time_s"]
        inp = g.name
    out = Path(a.learned)
    with tempfile.TemporaryDirectory() as tmp:
        cmd = [str(PROBE), "--network", str(Path(a.plastic).resolve()), "--ms", str(int(t_end * 1000)), "--input", inp,
               "--out", f"{tmp}/o.json", "--series", f"{tmp}/s.jsonl", "--series-ms", "100", "--no-raster",
               "--seed", "1", "--export-after", str(out.resolve())]
        env = dict(os.environ, NM_DISABLE_OPENCL="1", RAYON_NUM_THREADS="2")
        p = subprocess.run(cmd, cwd=ROOT, env=env, capture_output=True, text=True)
        os.unlink(inp)
        if p.returncode:
            raise SystemExit(p.stderr[-2000:])
    print(f"replayed {t_end:.0f} s of experience; learned network: {out}")
    weights(argparse.Namespace(network=str(out), base=a.plastic))


def teacher_off(a):
    snap = json.loads(Path(a.learned).read_text())
    n = 0
    for t in snap["net"].get("neuron_types") or []:
        if t.get("name", "").startswith("Seq:") and t["name"].endswith(":X"):
            t["bio_params"]["tonic_current"] = -100.0
            n += 1
    if n == 0:
        raise SystemExit("no teacher (Seq:*:X) cell types found")
    snap["net"]["aarnn_plasticity_enabled"] = bool(a.keep_learning)
    snap["connectome_labels"].setdefault("learning", {})["teacher_off"] = True
    Path(a.out).write_text(json.dumps(snap))
    print(f"wrote {a.out}: teacher silenced ({n} handover types), plasticity {'on' if a.keep_learning else 'frozen'}")


def weights(a):
    """Learned band -> L drive per learning cell: total, and the strongest bands."""
    def mat(path):
        s = json.loads(Path(path).read_text())
        w = s["w_hh_rec"][0]
        return s["connectome_labels"]["hidden_nodes"], w["rows"], w["cols"], w["data"]
    hidden, rows, cols, data = mat(a.network)
    base = mat(a.base)[3] if a.base else None
    bands = [i for i, h in enumerate(hidden) if h.startswith("B_")]
    for j, h in enumerate(hidden):
        if not h.startswith("L_"):
            continue
        # orientation: data[row * cols + col]; find which index is the target
        vals = [(data[j * cols + i], data[i * cols + j]) for i in bands]
        use = 0 if sum(abs(v[0]) for v in vals) >= sum(abs(v[1]) for v in vals) else 1
        w = [v[use] for v in vals]
        w0 = [(base[j * cols + i] if use == 0 else base[i * cols + j]) for i in bands] if base else None
        top = sorted(zip(w, [hidden[i] for i in bands]), reverse=True)[:4]
        ratio = (sum(w) / sum(w0)) if w0 and sum(w0) else float("nan")
        print(f"{h:12s} sum {sum(w):8.4f}  x{ratio:5.2f} of initial  top {[(n, round(v, 4)) for v, n in top]}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    m = sub.add_parser("make")
    m.add_argument("raw")
    m.add_argument("out")
    m.add_argument("--eta", type=float, default=0.0)
    m.add_argument("--plastic", type=int, default=1)
    m.add_argument("--w-max-mv", type=float, default=0.3,
                   help="cap of a learned body-state synapse (mV/ms at full band release; 0 = runtime clamp)")
    m.add_argument("--config-in")
    m.add_argument("--config-out")
    r = sub.add_parser("replay")
    r.add_argument("plastic")
    r.add_argument("run")
    r.add_argument("learned")
    t = sub.add_parser("teacher-off")
    t.add_argument("learned")
    t.add_argument("out")
    t.add_argument("--keep-learning", type=int, default=0)
    w = sub.add_parser("weights")
    w.add_argument("network")
    w.add_argument("--base")
    a = ap.parse_args()
    {"make": make, "replay": replay, "teacher-off": teacher_off, "weights": weights}[a.cmd](a)


if __name__ == "__main__":
    main()
