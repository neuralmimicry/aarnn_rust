#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Zebrafish swimming circuit: spinal half-centres, descending command,
reticulospinal steering and a slow turn-preference oscillator.

Same building blocks as the worm and hexapod circuits (``aarnn_circuit``):
graded cells, bistable (plateau) half-centres with reciprocal inhibition, a
tonic command setting the behavioural state and a pulse-density motor code.
Fish swim with an intrinsic spinal rhythm (fictive swimming persists without
sensory feedback), so here the half-centres are switched by their own slow
adaptation rather than by proprioception:

* per tail segment (0 head .. 7 tail) and side: ``E`` (V2a-like excitatory
  premotor cell, bistable, escape/release), ``A`` (slow adaptation, e.g. the
  Ca2+-activated K+ current that ends a burst), ``C`` (commissural inhibitory,
  V0d-like) and the graded motor pool (the ``tail_l/r`` output);
* rostro-caudal coupling: weak descending ipsilateral excitation, crossed
  caudal inhibition and a 10 % rostral drive gradient give a head-to-tail wave
  of about one body length (~0.1 cycle per segment);
* ``MLR`` (nMLF/MLR-like command) holds the spinal drive (swim speed);
* ``RS_l``/``RS_r`` (reticulospinal) bias the motor pools of one side: a
  sustained bend superimposed on the wave turns the fish. They are bistable
  with mutual inhibition (a winner-take-all turn decision with hysteresis),
  excited by the ipsilateral range ray (wall ahead-left -> turn away);
* ``ARTR_l``/``ARTR_r``, a slow half-centre oscillator (seconds-long
  adaptation) like the larval zebrafish anterior rhombencephalic turning region
  (Dunn et al. 2016), alternates the preferred turn direction every ~10-20 s:
  it bends the fish gently towards one side (chains of same-side turns, i.e.
  exploration) and decides head-on wall turns through a subthreshold RS bias;
* non-locomotor fins, jaw and opercula are held at mid-range.

Readout convention (legacy projection): channel ``tail_l{i}`` drives tail
joint ``i`` positive, which bends the tail to the fish's right and turns it
clockwise (rightward) when sustained.

Parameters were designed in a Python mirror of the graded-cell equations
(4-4.5 Hz, ~0.1 cycle per segment lag) and are expressed as input currents
(mV/ms at full presynaptic release); see ``drive_weight``.
"""
import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aarnn_circuit import Circuit, add_hindbrain, drive_weight, graded, release_tonic, sensor_weight  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
SEGMENTS = 8
GAIN = 100.0


def build(args):
    model = json.loads((ROOT / "sim/content/embodiments/generated/zebrafish.json").read_text())
    sensors = [s["name"] for s in model["io"]["sensors"]]
    outputs = list(model["io"]["output_names"])
    c = Circuit("zebrafish", sensors, outputs)

    def w(mv):
        return drive_weight(mv, GAIN)

    def ws(mv):
        return sensor_weight(mv, GAIN)
    common = dict(nmda_ratio=0.0, synaptic_gain=GAIN)
    c.type("Spinal:E", **graded(tau_ms=args.tau_e, plateau_mv=args.plateau), **common)
    # Deterministic left/right asymmetry so the first burst has a side.
    c.type("Spinal:E_lead", **graded(tau_ms=args.tau_e, plateau_mv=args.plateau, tonic=0.3), **common)
    c.type("Spinal:A", **graded(tau_ms=args.tau_a), **common)
    c.type("Spinal:C", **graded(tau_ms=args.tau_c), **common)
    c.type("Motor:tail", **graded(tau_ms=args.tau_m, release_hz=500.0), **common)
    c.type("Motor:hold", **graded(tau_ms=20.0, release_hz=500.0, tonic=release_tonic(0.5)), **common)
    c.type("Motor:rest", **graded(tau_ms=20.0, release_hz=500.0), **common)

    tail = {(s, i): next(o for o in outputs if o.endswith(f"_tail_{s}{i}")) for s in "lr" for i in range(SEGMENTS)}
    for o in outputs:
        if o in tail.values():
            c.output(o, "Motor:tail")
        elif any(k in o for k in ("pec_fin", "jaw", "operculum")):
            c.output(o, "Motor:hold")   # single-sign readouts: 0.5 = mid-range
        else:
            c.output(o, "Motor:rest")   # antagonist pairs at rest = neutral; no trunk stiffening

    # Shared hindbrain: MLR command, bistable RS turn decision, ARTR.
    hb = add_hindbrain(c, GAIN, plateau_mv=args.plateau, mlr_release=args.mlr_release, rs_mutual=args.rs_mutual,
                       artr_drive=args.artr_drive, artr_tau_a_ms=args.artr_tau_a, artr_w_ea=args.w_ea,
                       artr_w_ae=args.w_ae, artr_mutual=args.artr_mutual, artr_rs=args.artr_rs, pos=(0.04, 0.0, 0.0))
    mlr, rs, artr = hb["mlr"], hb["rs"], hb["artr"]

    E, A, C = {}, {}, {}
    for i in range(SEGMENTS):
        x = 0.04 - 0.08 / 9 * (i + 1)
        for s in "lr":
            y = 0.002 if s == "l" else -0.002
            E[s, i] = c.cell(f"E_{s}{i}", "Spinal:E_lead" if (s, i) == ("l", 0) else "Spinal:E", f"spinal_{i}", (x, y, 0.001))
            A[s, i] = c.cell(f"A_{s}{i}", "Spinal:A", f"spinal_{i}", (x, y, 0.0))
            C[s, i] = c.cell(f"C_{s}{i}", "Spinal:C", f"spinal_{i}", (x, -y, -0.001))
    for i in range(SEGMENTS):
        drive = args.drive * (1.0 + args.drive_gradient * (SEGMENTS - 1 - i) / (SEGMENTS - 1))
        # Motor pool gain grows head to tail: joint amplitude ~0.08 -> 0.5 rad.
        motor = args.motor_head + (args.motor_tail - args.motor_head) * i / (SEGMENTS - 1)
        for s, o in (("l", "r"), ("r", "l")):
            c.connect(mlr, E[s, i], w(drive / args.mlr_release))
            c.connect(E[s, i], A[s, i], w(args.w_ea))
            c.connect(A[s, i], E[s, i], w(-args.w_ae))
            c.connect(E[s, i], C[s, i], w(args.w_ec))
            c.connect(C[s, i], E[o, i], w(-args.w_ce))
            c.connect(E[s, i], tail[s, i], w(motor))
            c.connect(rs[s], tail[s, i], w(args.rs_motor))
            c.connect(artr[s], tail[s, i], w(args.artr_motor))
            if i + 1 < SEGMENTS:
                c.connect(E[s, i], E[s, i + 1], w(args.w_desc))
                c.connect(C[s, i], E[o, i + 1], w(-args.w_cdesc))
    # Range rays (1 - distance / 0.3 m): a wall ahead-left raises the left
    # reticulospinal bias, which bends the fish rightward, away from it.
    ray = {s: next(n for n in sensors if n.endswith(f"_olfactory_{s}")) for s in "lr"}
    for s in "lr":
        c.sensor(ray[s], rs[s], ws(args.wall_rs))
    labels = {"design": "zebrafish spinal half-centre CPG with reticulospinal steering and ARTR exploration",
              "parameters": vars(args) | {"output": str(args.output)}}
    c.write(Path(args.output), Path(args.config_out) if args.config_out else None,
            synaptic_gain=GAIN, depth=args.depth, labels=labels)
    return c


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--output", default="network_zebrafish_swim.json")
    ap.add_argument("--config-out", default=None)
    ap.add_argument("--depth", type=int, default=3)
    ap.add_argument("--tau-e", type=float, default=15.0)
    ap.add_argument("--tau-a", type=float, default=70.0, help="spinal adaptation tau (ms): sets the beat frequency")
    ap.add_argument("--tau-c", type=float, default=8.0)
    ap.add_argument("--tau-m", type=float, default=15.0)
    ap.add_argument("--plateau", type=float, default=22.0)
    ap.add_argument("--drive", type=float, default=4.0, help="spinal drive at the tail segment (mV/ms)")
    ap.add_argument("--drive-gradient", type=float, default=0.1)
    ap.add_argument("--mlr-release", type=float, default=0.5)
    ap.add_argument("--w-ea", type=float, default=10.0)
    ap.add_argument("--w-ae", type=float, default=9.0)
    ap.add_argument("--w-ec", type=float, default=10.0)
    ap.add_argument("--w-ce", type=float, default=4.0)
    ap.add_argument("--w-desc", type=float, default=0.5)
    ap.add_argument("--w-cdesc", type=float, default=1.0)
    ap.add_argument("--motor-head", type=float, default=2.2)
    ap.add_argument("--motor-tail", type=float, default=5.0)
    ap.add_argument("--rs-motor", type=float, default=3.0, help="reticulospinal -> ipsilateral motor pools (mV/ms)")
    ap.add_argument("--wall-rs", type=float, default=5.0,
                    help="range ray value 1.0 -> RS drive (mV/ms); RS switches on near 1.56 (ray ~0.3, ~0.2 m) and off below 0.46")
    ap.add_argument("--rs-mutual", type=float, default=6.0, help="RS_l <-> RS_r inhibition (winner-take-all)")
    ap.add_argument("--artr-motor", type=float, default=1.0, help="ARTR -> ipsilateral motor pools: gentle exploratory arcs")
    ap.add_argument("--artr-drive", type=float, default=2.0)
    ap.add_argument("--artr-tau-a", type=float, default=5000.0, help="ARTR adaptation tau (ms): ~10-20 s alternation")
    ap.add_argument("--artr-mutual", type=float, default=4.0)
    ap.add_argument("--artr-rs", type=float, default=1.0, help="ARTR -> RS, subthreshold: picks the side of a head-on turn")
    args = ap.parse_args()
    c = build(args)
    print(f"wrote {args.output}: {len(c.cells)} cells, {len(c.syn)} synapses, {len(c.sense)} sensory projections")


if __name__ == "__main__":
    main()
