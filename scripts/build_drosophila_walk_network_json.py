#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Drosophila walking circuit (BANC and FAFB bodies share the 418/48 contract).

The imported connectomes are too large to place on a node; this designed
circuit uses the same blocks as the worm, hexapod and fish (``aarnn_circuit``)
for the fly's legs. The fly body exposes no leg proprioceptors (only thorax
touch, a ring of range rays, eyes and an IMU), so each leg's swing/stance
half-centre is switched by its own adaptation, as in the fish spinal cord
(an intrinsic rhythm; walking flies have leg CPGs whose coordination sensory
feedback normally refines):

* per leg (lf, lm, lr, rf, rm, rr): ``SW``/``ST`` excitatory half-centres
  (bistable, escape by adaptation ``A``), crossed via inhibitory ``C`` cells;
* neighbouring legs' swing units inhibit each other (the leg graph is
  two-colourable), so legs alternate in two tripods (lf, rm, lr / rf, lm, rr),
  the gait of walking Drosophila (Strauss & Heisenberg 1990; Wosnitza 2013);
* swing protracts the coxa and lifts the femur, stance retracts and presses
  (signs measured on the MJCF: left coxa + retracts, right coxa + protracts;
  left femur + lifts, right femur + lowers); tibia and tarsus hold mid-range;
* the shared hindbrain (``add_hindbrain``): ``MLR`` sets the stepping drive,
  ``RS``/``ARTR`` steer by shortening the strides of the inner side; range
  rays and thorax touch on one side excite the ipsilateral ``RS`` (turn away).

Readouts are the policy's antagonistic pairs (channel a x 0.62, b x -0.38):
graded activations map a joint about its mid-range.
"""
import argparse
import json
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aarnn_circuit import Circuit, add_hindbrain, add_leg_oscillators, drive_weight, graded, sensor_weight  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
LEGS = ("lf", "lm", "lr", "rf", "rm", "rr")
NEIGHBOURS = [("lf", "lm"), ("lm", "lr"), ("rf", "rm"), ("rm", "rr"), ("lf", "rf"), ("lm", "rm"), ("lr", "rr")]
GAIN = 100.0


def build(args):
    model = json.loads((ROOT / f"sim/content/embodiments/generated/{args.profile}.json").read_text())
    outputs = list(model["io"]["output_names"])
    acts = {a["name"]: a for a in model["actuators"]}
    c = Circuit.for_embodiment(args.profile)

    def w(mv):
        return drive_weight(mv, GAIN)
    common = dict(nmda_ratio=0.0, synaptic_gain=GAIN)
    hb = add_hindbrain(c, GAIN, mlr_release=args.mlr_release, rs_plateau_mv=args.rs_plateau)
    mlr, rs, artr = hb["mlr"], hb["rs"], hb["artr"]
    c.type("Motor:leg", **graded(tau_ms=args.tau_m, release_hz=500.0), **common)
    for o in outputs:
        c.output(o, "Motor:leg")
    positions = {leg: ({"f": 1.0, "m": 0.0, "r": -1.0}[leg[1]] * 0.01, 0.008 if leg[0] == "l" else -0.008) for leg in LEGS}
    E, A, C = add_leg_oscillators(c, GAIN, LEGS, NEIGHBOURS, mlr, args.drive, drive_release=args.mlr_release,
                                  tau_e_ms=args.tau_e, plateau_mv=args.plateau, tau_a_swing_ms=args.tau_a_swing,
                                  tau_a_stance_ms=args.tau_a_stance, w_ea=args.w_ea, w_ae=args.w_ae, w_ec=args.w_ec,
                                  w_ce=args.w_ce, inter=args.inter, inter_ex=args.inter_ex, positions=positions)

    def channels(joint):
        r = acts[joint]["readout"]
        pos = next(outputs[x["channel"]] for x in r if x["coefficient"] > 0)
        neg = next(outputs[x["channel"]] for x in r if x["coefficient"] < 0)
        return pos, neg

    def coefficient(joint, sign):
        return max(abs(x["coefficient"]) for x in acts[joint]["readout"] if (x["coefficient"] > 0) == (sign > 0))

    strong = max(abs(x["coefficient"]) for a in acts.values() for x in a["readout"])

    def drive_joint(pre, joint, direction, mv):
        """Push the joint towards +direction: agonist channel up, antagonist
        down. The readout pairs are unequal (a x 0.62, b x -0.38), so a push
        through the weaker channel is scaled up; otherwise the side whose
        swing uses channel b strides and lifts less and the fly circles."""
        pos, neg = channels(joint)
        ago, ant = (pos, neg) if direction > 0 else (neg, pos)
        c.connect(pre, ago, w(mv * strong / coefficient(joint, direction)))
        c.connect(pre, ant, w(-mv))
    # Swing shaping: the lift leads protraction (foot up early in the swing,
    # down before stance), as in walking insects. Lift comes straight from
    # the swing cell and is cut back by its own adaptation; protraction goes
    # through a delay cell. Without this, lift and protraction moved in phase
    # and the foot dragged forward at the start of each swing.
    c.type("Leg:delay", **graded(tau_ms=args.protract_delay), **common)
    for leg in LEGS:
        left = leg[0] == "l"
        protract = -1 if left else 1      # coxa direction that moves the foot forward
        lift = 1 if left else -1          # femur direction that lifts the foot
        delay = c.cell(f"D_{leg}_sw", "Leg:delay", leg)
        c.connect(E[leg, "sw"], delay, w(args.delay_drive))
        drive_joint(delay, f"{leg}_coxa", protract, args.coxa)
        drive_joint(E[leg, "st"], f"{leg}_coxa", -protract, args.coxa)
        drive_joint(E[leg, "sw"], f"{leg}_femur", lift, args.lift)
        drive_joint(A[leg, "sw"], f"{leg}_femur", -lift, args.lift_adapt)
        drive_joint(E[leg, "st"], f"{leg}_femur", -lift, args.press)
        # Steering: RS_l (obstacle left) turns right by shortening the right
        # legs' strides (inner side), and vice versa; ARTR gently.
        pos, neg = channels(f"{leg}_coxa")
        inner = rs["l"] if not left else rs["r"]
        inner_artr = artr["l"] if not left else artr["r"]
        for ch in (pos, neg):
            c.connect(inner, ch, w(-args.turn))
            c.connect(inner_artr, ch, w(-args.artr_turn))
    # Exteroception: range rays (1 - distance / reach) and thorax touch.
    for s in model["io"]["sensors"]:
        if s["kind"] == "legacy-range" and s.get("direction"):
            ang = math.atan2(s["direction"][1], s["direction"][0])
            if math.cos(ang) > 0.3 and abs(math.sin(ang)) > 0.1:
                c.sensor(s["name"], rs["l" if math.sin(ang) > 0 else "r"], sensor_weight(args.wall_rs * math.cos(ang), GAIN))
            elif math.cos(ang) > 0.95:
                for k in "lr":
                    c.sensor(s["name"], rs[k], sensor_weight(args.wall_rs * 0.5, GAIN))
        elif s["kind"] == "legacy-contact" and s.get("direction"):
            dy = s["direction"][1]
            if abs(dy) > 0.5:
                c.sensor(s["name"], rs["l" if dy > 0 else "r"], sensor_weight(args.touch_rs, GAIN))
            elif s["direction"][0] > 0.5:
                for k in "lr":
                    c.sensor(s["name"], rs[k], sensor_weight(args.touch_rs * 0.5, GAIN))
    labels = {"design": "Drosophila tripod walking: per-leg escape half-centres, swing-swing neighbour inhibition, shared hindbrain",
              "parameters": vars(args)}
    c.write(Path(args.output), Path(args.config_out) if args.config_out else None, synaptic_gain=GAIN, depth=args.depth, labels=labels)
    return c


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--profile", default="drosophila_banc", choices=("drosophila_banc", "drosophila_fafb"))
    ap.add_argument("--output")
    ap.add_argument("--config-out")
    ap.add_argument("--depth", type=int, default=0,
                    help="AARNN depth; designed circuits are specified synapse by synapse at 0 (at 3 the extra stages delay coupling and one tripod locked in stance)")
    ap.add_argument("--tau-e", type=float, default=15.0)
    ap.add_argument("--tau-m", type=float, default=5.0, help="motor output membrane tau (ms)")
    ap.add_argument("--tau-a-swing", type=float, default=30.0, help="swing burst length (ms scale)")
    ap.add_argument("--tau-a-stance", type=float, default=45.0, help="stance burst length: duty factor > 0.5")
    ap.add_argument("--plateau", type=float, default=22.0)
    ap.add_argument("--drive", type=float, default=4.0)
    ap.add_argument("--mlr-release", type=float, default=0.5)
    ap.add_argument("--w-ea", type=float, default=10.0)
    ap.add_argument("--w-ae", type=float, default=9.0)
    ap.add_argument("--w-ec", type=float, default=10.0)
    ap.add_argument("--w-ce", type=float, default=4.0)
    ap.add_argument("--inter", type=float, default=3.0, help="neighbouring swing inhibition (tripod)")
    ap.add_argument("--inter-ex", type=float, default=2.0, help="swing -> neighbouring stance excitation")
    ap.add_argument("--coxa", type=float, default=12.0)
    ap.add_argument("--protract-delay", type=float, default=40.0, help="protraction delay cell tau (ms)")
    ap.add_argument("--delay-drive", type=float, default=8.0)
    ap.add_argument("--lift-adapt", type=float, default=8.0, help="swing adaptation -> lift cut-back (mV/ms)")
    ap.add_argument("--lift", type=float, default=12.0)
    ap.add_argument("--press", type=float, default=4.0)
    ap.add_argument("--turn", type=float, default=4.0)
    ap.add_argument("--artr-turn", type=float, default=0.8)
    ap.add_argument("--rs-plateau", type=float, default=18.0)
    ap.add_argument("--wall-rs", type=float, default=3.0)
    ap.add_argument("--touch-rs", type=float, default=4.0)
    args = ap.parse_args()
    args.output = args.output or f"network_{args.profile}_walk.json"
    args.config_out = args.config_out or f"webots_world/configs/config_{args.profile}_walk.json"
    c = build(args)
    print(f"wrote {args.output}: {len(c.cells)} cells, {len(c.syn)} synapses, {len(c.sense)} sensory projections")


if __name__ == "__main__":
    main()
