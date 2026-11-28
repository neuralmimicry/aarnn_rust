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
  rays and thorax touch on one side excite the ipsilateral ``RS`` (turn away);
* the pivot (``REV_<leg>_<phase>``; ``--touch-escape 1``, ``--forage-steer-mode
  pivot``): the touch escape and the learned food steering turn the fly by
  making the inner legs step backwards while the outer legs walk on.

Readouts are the policy's antagonistic pairs (channel a x 0.62, b x -0.38):
graded activations map a joint about its mid-range.
"""
import argparse
import json
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aarnn_circuit import (Circuit, add_hindbrain, add_learned_anticipation, add_learned_foraging, add_touch_escape, add_turn_bouts,  # noqa: E402
                           add_leg_oscillators, apply_learner, drive_weight, graded, sensor_weight)

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
    hb = add_hindbrain(c, GAIN, mlr_release=args.mlr_release, rs_plateau_mv=args.rs_plateau, rs_mutual=args.rs_mutual)
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
    learn = None
    if args.learn_groups > 0:
        # Learning by experience (aarnn_circuit.add_learned_anticipation): a
        # near detector on the forward rays teaches LT_side, which drives RS.
        rays = [s for s in model["io"]["sensors"] if s["kind"] == "legacy-range" and s.get("direction")]
        reach = float(rays[0].get("angle_rad") or 0.08)
        ang = {s["name"]: math.atan2(s["direction"][1], s["direction"][0]) for s in rays}
        fr = [n for n, a in ang.items() if math.cos(a) > 0.95]
        side_rays = {sd: [n for n, a in ang.items() if 0.95 >= math.cos(a) > 0.6 and (math.sin(a) > 0) == (sd == "l")][:3] + fr
                     for sd in "lr"}
        add_learned_anticipation(c, GAIN, side_rays, lambda d: 1.0 - d / reach, False, args.learn_teach_m,
                                 args.learn_teach_m + 0.005, args.learn_teach_m + 0.02, args.learn_groups,
                                 {sd: [(rs[sd], args.learn_rs_mv)] for sd in "lr"})
        learn = dict(profile=args.profile, encoding="inverse", reach_m=reach, channels=side_rays, teach_m=args.learn_teach_m,
                     near_m=args.learn_teach_m + 0.005, far_m=args.learn_teach_m + 0.02, protocol_far_m=reach,
                     protocol_near_m=0.02)
    bouts = None
    if args.explore:
        # Spontaneous run-and-turn bouts (shared hindbrain, zebrafish-style).
        bouts = add_turn_bouts(c, GAIN, hb, on_ms=args.explore_on_ms, off_ms=args.explore_off_ms)
    forage = None
    if args.forage:
        # Learned foraging (needs-v1; aarnn_circuit.add_learned_foraging): feeding
        # halts locomotion; the value of the food odour is learned from eating.
        forage = add_learned_foraging(c, GAIN, mlr, bouts["on"] if bouts else None, hunger_below=args.forage_hunger,
                                      halt=[rs["l"], rs["r"], artr["l"], artr["r"]], steer=args.forage_steer > 0)
        if args.forage_steer > 0 and args.forage_steer_mode == "cut":
            # Learned steering: shortening one side's strides turns the fly
            # toward that side (RS_l shortens the right legs and turns right),
            # so STEER_l (odour stronger on the left) shortens the left strides.
            for leg in LEGS:
                inner = forage["steer"]["l"] if leg[0] == "l" else forage["steer"]["r"]
                for ch in channels(f"{leg}_coxa"):
                    c.connect(inner, ch, w(-args.forage_steer))
        if learn is None:
            learn = dict(profile=args.profile)
    rev_cells = {}

    def reverse_cells():
        """The pivot. Like the moonwalker descending neurons (Bidaye et al.
        2014), a turn command makes the legs on the inner side of the turn
        step backwards (retract in swing, protract in stance) while the outer
        legs walk on, so the fly turns without shortening the outer strides.
        REV_<leg>_<phase> is an AND cell, on only while both its phase cell and
        a turn command (the touch escape, the learned steering) fire. Made
        once, on first use."""
        if not rev_cells:
            c.type("Leg:reverse", **graded(tau_ms=10.0, tonic=-args.reverse_gate), **common)
            for leg in LEGS:
                protract = -1 if leg[0] == "l" else 1
                x, y = positions[leg]
                for phase, direction, mv in (("sw", -protract, args.coxa), ("st", protract, 2.0 * args.coxa)):
                    r = c.cell(f"REV_{leg}_{phase}", "Leg:reverse", leg, (x, y, -0.002))
                    c.connect(E[leg, phase], r, w(args.reverse_in))
                    drive_joint(r, f"{leg}_coxa", direction, mv)
                    rev_cells[leg, phase] = r
        return rev_cells
    if args.touch_escape:
        # Innate touch escape (aarnn_circuit.add_touch_escape): the antennae and
        # front legs touching a surface (horizontal rays within 60 deg of
        # ahead at >= touch_ref, about 1 cm beyond the head) or the thorax's
        # own touch fires the escape cell of that side. It silences the
        # opposite turn and the learned steering, and pivots the fly away.
        touch = {"l": [], "r": [], "f": []}
        for s in model["io"]["sensors"]:
            d = s.get("direction")
            if not d:
                continue
            if s["kind"] == "legacy-range" and (len(d) < 3 or abs(d[2]) < 1e-9):
                a = math.atan2(d[1], d[0])
                if math.cos(a) >= math.cos(math.radians(60.0)):
                    side = "f" if abs(math.sin(a)) < 1e-6 else "l" if math.sin(a) > 0 else "r"
                    touch[side].append((s["name"], args.touch_ref))
            elif s["kind"] == "legacy-contact":
                side = "l" if d[1] > 0.5 else "r" if d[1] < -0.5 else "f" if d[0] > 0.5 else None
                if side:
                    touch[side].append((s["name"], 0.5))
        esc = add_touch_escape(c, GAIN, hb, touch, suppress=list(forage["steer"].values()) if forage else None,
                               quiet=[forage["feed"]] if forage else None, touch_tau_ms=args.touch_tau)
        # The escape turns by the pivot (reverse_cells). Cutting the inner
        # side's strides (the RS turn) left the fly pinned: with its head on
        # the wall, the outer legs' forward push only pressed that side harder
        # into the wall (26 Nov closed loop).
        rev = reverse_cells()
        for leg in LEGS:
            inner = esc["esc"]["r"] if leg[0] == "l" else esc["esc"]["l"]   # ESC_l turns right: right legs reverse
            c.connect(inner, f"D_{leg}_sw", w(-30.0))   # no forward protraction while reversed
            for phase in ("sw", "st"):
                c.connect(inner, rev[leg, phase], w(args.reverse_in))
    if forage and args.forage_steer > 0 and args.forage_steer_mode == "pivot":
        # Learned steering by the pivot: STEER_l (odour stronger on the left)
        # makes the left legs step backwards, graded by its release, while the
        # right legs walk on, so the fly turns left. Each reversing swing also
        # holds back that leg's forward protraction (REV_sw -> D_sw), which
        # would otherwise cancel the backward swing. Cutting the inner strides
        # instead (mode "cut") turned the fly only at 12 mV/ms and halved its
        # walking speed, so the learned fly reached fruit later than the naive
        # one (26 Nov).
        rev = reverse_cells()
        for leg in LEGS:
            for phase in ("sw", "st"):
                c.connect(forage["steer"][leg[0]], rev[leg, phase], w(args.forage_steer))
            c.connect(rev[leg, "sw"], f"D_{leg}_sw", w(-args.pivot_protract))
    labels = {"design": "Drosophila tripod walking: per-leg escape half-centres, swing-swing neighbour inhibition, shared hindbrain",
              "parameters": vars(args)}
    c.write(Path(args.output), Path(args.config_out) if args.config_out else None, synaptic_gain=GAIN, depth=args.depth, labels=labels)
    if learn:
        apply_learner(Path(args.output), Path(args.config_out) if args.config_out else None, GAIN, learn)
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
    ap.add_argument("--explore", type=int, default=0, help="1: spontaneous run-and-turn bouts (shared hindbrain)")
    ap.add_argument("--explore-on-ms", type=float, default=2000.0)
    ap.add_argument("--explore-off-ms", type=float, default=12000.0)
    ap.add_argument("--forage", type=int, default=0, help="1: learned foraging (needs-v1 channels; food-odour value learned from eating)")
    ap.add_argument("--forage-hunger", type=float, default=0.5)
    ap.add_argument("--forage-steer", type=float, default=0.0,
                    help="learned steering toward the stronger odour (mV/ms; 0 = off): mode cut, STEER -> inner-side coxa "
                         "strides; mode pivot, STEER -> the inner legs' reversal AND cells")
    ap.add_argument("--forage-steer-mode", default="cut", choices=("cut", "pivot"),
                    help="cut: shorten the inner strides; pivot: the inner legs step backwards (as the touch escape)")
    ap.add_argument("--pivot-protract", type=float, default=30.0,
                    help="pivot steering: a reversing swing -> that leg's protraction delay cell (inhibitory mV/ms)")
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
    ap.add_argument("--rs-mutual", type=float, default=6.0,
                    help="RS_l <-> RS_r inhibition; near a wall rays + LT drive both RS past 6 (use ~20 for a decisive turn)")
    ap.add_argument("--wall-rs", type=float, default=3.0)
    ap.add_argument("--touch-rs", type=float, default=4.0)
    ap.add_argument("--learn-groups", type=int, default=0, help="anticipation learning: range cells per side (0 = off)")
    ap.add_argument("--learn-teach-m", type=float, default=0.04, help="innate near detector (teacher) distance (m)")
    ap.add_argument("--learn-rs-mv", type=float, default=3.0)
    ap.add_argument("--touch-escape", type=int, default=0, help="1: innate touch escape (pivot away from a surface the antennae touch)")
    ap.add_argument("--touch-ref", type=float, default=0.45, help="ray value that counts as touch (0.45 = 4.4 cm from the thorax)")
    ap.add_argument("--touch-tau", type=float, default=1000.0,
                    help="touch cell decay (ms): how long the escape outlasts the contact (400 left the fly wall-following at 6-9 cm)")
    ap.add_argument("--reverse-in", type=float, default=10.0, help="phase cell / escape cell -> reversal AND cell (mV/ms each)")
    ap.add_argument("--reverse-gate", type=float, default=12.0, help="reversal AND cell threshold (mV/ms): needs both inputs")
    args = ap.parse_args()
    args.output = args.output or f"network_{args.profile}_walk.json"
    args.config_out = args.config_out or f"webots_world/configs/config_{args.profile}_walk.json"
    c = build(args)
    print(f"wrote {args.output}: {len(c.cells)} cells, {len(c.syn)} synapses, {len(c.sense)} sensory projections")


if __name__ == "__main__":
    main()
