#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Salamander crawler circuit (profile ``salamander_crawler``, G2).

A new body style composed from circuit blocks that the connectome explorer
found recurring across the existing designed networks (motif library,
``aarnn-explorer library``):

* **shared hindbrain** (``add_hindbrain``; fish, fly, hexapod): ``MLR``
  locomotor command, bistable ``RS`` turn decision, ``ARTR`` exploration
  oscillator, plus spontaneous turn bouts (``add_turn_bouts``);
* **axial segments** (zebrafish spinal block): left/right swing half-centres
  ``E/A/C`` per axial joint (neck, trunk, tail), with descending ipsilateral
  excitation and crossed inhibition, giving a head-to-tail travelling wave;
* **limb oscillators** (``add_leg_oscillators``; fly/hexapod): swing/stance
  half-centres for the four legs. Neighbour inhibition on the graph lf-rf,
  lh-rh, lf-lh, rf-rh (two-colourable) gives a diagonal trot (lf+rh / rf+lh);
* **girdle coupling** (Ijspeert et al. 2007): each girdle's stance
  half-centres excite the axial segment's half-centre on the same side
  (shoulder -> trunk, pelvis -> tail), so the body wave is entrained by the
  limbs. The trunk is concave on the side whose front leg is ending stance,
  as in the scripted gait that walked best (0.56 m in 5 s);
* **touch escape** (``add_touch_escape``; fish/fly): head touch and near
  rays fire ``ESC_side``, which bends the body away (C-bend through the axial
  motor pools of the opposite side) and turns through RS;
* **learned foraging** (``add_learned_foraging`` with steering): feeding
  halts locomotion; odour value is learned from eating; ``STEER_side`` bends
  the body toward the food side (axial steering, as in the fish, whose axial
  steering turned it toward food) and shortens that side's strides.

Joint convention (embodiment_salamander.py): axial +q = concave left, coxa
+q = protract, lift +q = elevate; channel 2i drives +q, 2i+1 drives -q.
"""
import argparse
import json
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aarnn_circuit import (Circuit, add_hindbrain, add_learned_foraging, add_leg_oscillators, add_touch_escape,  # noqa: E402
                           add_turn_bouts, apply_learner, drive_weight, graded, sensor_weight)

ROOT = Path(__file__).resolve().parents[1]
PROFILE = "salamander_crawler"
LEGS = ("lf", "lh", "rf", "rh")
NEIGHBOURS = [("lf", "rf"), ("lh", "rh"), ("lf", "lh"), ("rf", "rh")]
AXIAL = ("neck_yaw", "trunk_yaw", "tail_yaw")
GAIN = 100.0


def build(args):
    model = json.loads((ROOT / f"sim/content/embodiments/generated/{PROFILE}.json").read_text())
    io = model["io"]
    outputs = list(io["output_names"])
    acts = {a["name"]: a for a in model["actuators"]}
    c = Circuit.for_embodiment(PROFILE)
    sensors = [s["name"] for s in io["sensors"]]

    def w(mv):
        return drive_weight(mv, GAIN)

    def ws(mv):
        return sensor_weight(mv, GAIN)
    common = dict(nmda_ratio=0.0, synaptic_gain=GAIN)
    c.type("Motor:joint", **graded(tau_ms=args.tau_m, release_hz=500.0), **common)
    for o in outputs:
        c.output(o, "Motor:joint")

    def channels(joint):
        r = acts[joint]["readout"]
        pos = next(outputs[x["channel"]] for x in r if x["coefficient"] > 0)
        neg = next(outputs[x["channel"]] for x in r if x["coefficient"] < 0)
        return pos, neg

    def drive_joint(pre, joint, direction, mv):
        pos, neg = channels(joint)
        ago, ant = (pos, neg) if direction > 0 else (neg, pos)
        c.connect(pre, ago, w(mv))
        c.connect(pre, ant, w(-mv))

    hb = add_hindbrain(c, GAIN, mlr_release=args.mlr_release, rs_mutual=args.rs_mutual, pos=(0.02, 0.0, 0.0))
    mlr, rs, artr = hb["mlr"], hb["rs"], hb["artr"]

    # Axial segments (zebrafish spinal block), head to tail.
    c.type("Axial:E", **graded(tau_ms=args.tau_e, plateau_mv=args.plateau), **common)
    c.type("Axial:E_lead", **graded(tau_ms=args.tau_e, plateau_mv=args.plateau, tonic=0.3), **common)
    c.type("Axial:A", **graded(tau_ms=args.tau_a_axial), **common)
    c.type("Axial:C", **graded(tau_ms=8.0), **common)
    AE, AA, AC = {}, {}, {}
    for i, j in enumerate(AXIAL):
        x = 0.01 - 0.1 * i
        for s in "lr":
            y = 0.002 if s == "l" else -0.002
            AE[s, i] = c.cell(f"AX_E_{s}{i}", "Axial:E_lead" if (s, i) == ("l", 0) else "Axial:E", f"axial_{j}", (x, y, 0.001))
            AA[s, i] = c.cell(f"AX_A_{s}{i}", "Axial:A", f"axial_{j}", (x, y, 0.0))
            AC[s, i] = c.cell(f"AX_C_{s}{i}", "Axial:C", f"axial_{j}", (x, -y, -0.001))
    for i, j in enumerate(AXIAL):
        for s, o in (("l", "r"), ("r", "l")):
            c.connect(mlr, AE[s, i], w(args.axial_drive / args.mlr_release))
            c.connect(AE[s, i], AA[s, i], w(10.0))
            c.connect(AA[s, i], AE[s, i], w(-9.0))
            c.connect(AE[s, i], AC[s, i], w(10.0))
            c.connect(AC[s, i], AE[o, i], w(-4.0))
            # Left half-centre bends concave left (+q).
            drive_joint(AE[s, i], j, 1 if s == "l" else -1, args.axial_motor[i])
            if i + 1 < len(AXIAL):
                c.connect(AE[s, i], AE[s, i + 1], w(args.w_desc))
                c.connect(AC[s, i], AE[o, i + 1], w(-args.w_cdesc))

    # Limb oscillators (fly/hexapod block): diagonal trot on four legs.
    positions = {leg: (0.0 if leg[1] == "f" else -0.2, 0.008 if leg[0] == "l" else -0.008) for leg in LEGS}
    E, A, C = add_leg_oscillators(c, GAIN, LEGS, NEIGHBOURS, mlr, args.leg_drive, drive_release=args.mlr_release,
                                  plateau_mv=args.leg_plateau, tau_a_swing_ms=args.tau_a_swing,
                                  tau_a_stance_ms=args.tau_a_stance, inter=args.inter, inter_ex=args.inter_ex,
                                  positions=positions)
    c.type("Leg:delay", **graded(tau_ms=args.protract_delay), **common)
    for leg in LEGS:
        d = c.cell(f"D_{leg}_sw", "Leg:delay", leg)
        c.connect(E[leg, "sw"], d, w(8.0))
        drive_joint(d, f"{leg}_coxa", 1, args.coxa)                 # swing protracts
        drive_joint(E[leg, "st"], f"{leg}_coxa", -1, args.coxa)     # stance retracts
        drive_joint(E[leg, "sw"], f"{leg}_lift", 1, args.lift)      # swing lifts
        drive_joint(E[leg, "st"], f"{leg}_lift", -1, args.press)    # stance presses
    # Girdle coupling: stance of a girdle's leg entrains the same-side axial
    # half-centre (shoulder -> trunk, pelvis -> tail).
    for leg in LEGS:
        seg = 1 if leg[1] == "f" else 2
        c.connect(E[leg, "st"], AE[leg[0], seg], w(args.girdle))

    # Turning: RS_l (obstacle left) turns right: bend concave right and
    # shorten the right strides; ARTR gently.
    for s, o in (("l", "r"), ("r", "l")):
        for i, j in enumerate(AXIAL):
            drive_joint(rs[s], j, 1 if o == "l" else -1, args.turn_axial)
            drive_joint(artr[s], j, 1 if o == "l" else -1, args.artr_axial)
        for leg in LEGS:
            if leg[0] == o:
                for ch in channels(f"{leg}_coxa"):
                    c.connect(rs[s], ch, w(-args.turn_stride))
    # Range rays (1 - distance / reach) ahead-left / ahead-right -> RS.
    rays = {}
    for snr in io["sensors"]:
        d = snr.get("direction")
        if snr["kind"] == "legacy-range" and d and abs(d[2] if len(d) > 2 else 0.0) < 1e-9:
            rays[snr["name"]] = math.atan2(d[1], d[0])
    for name, a in rays.items():
        if math.cos(a) > 0.3 and abs(math.sin(a)) > 0.1:
            c.sensor(name, rs["l" if math.sin(a) > 0 else "r"], ws(args.wall_rs * math.cos(a)))
        elif math.cos(a) > 0.95:
            for k in "lr":
                c.sensor(name, rs[k], ws(args.wall_rs * 0.5))

    bouts = add_turn_bouts(c, GAIN, hb, on_ms=args.explore_on_ms, off_ms=args.explore_off_ms) if args.explore else None
    forage = add_learned_foraging(c, GAIN, mlr, bouts["on"] if bouts else None, hunger_below=0.5,
                                  halt=[rs["l"], rs["r"], artr["l"], artr["r"]], steer=True)
    # Food steering toward the stronger odour: bend toward it, shorten that
    # side's strides.
    for s in "lr":
        for i, j in enumerate(AXIAL):
            drive_joint(forage["steer"][s], j, 1 if s == "l" else -1, args.steer_axial)
        for leg in LEGS:
            if leg[0] == s:
                for ch in channels(f"{leg}_coxa"):
                    c.connect(forage["steer"][s], ch, w(-args.steer_stride))

    # Touch escape: head touch + near rays -> ESC_side -> RS turn and a C-bend
    # away through the opposite axial pools.
    touch = {"l": [], "r": [], "f": []}
    for snr in io["sensors"]:
        n, d = snr["name"], snr.get("direction")
        if snr["kind"] == "legacy-contact" and d and "touch" in n:
            side = "l" if d[1] > 0.5 else "r" if d[1] < -0.5 else "f" if d[0] > 0.5 else None
            if side:
                touch[side].append((n, 0.5))
        elif n in rays and math.cos(rays[n]) >= 0.5:
            a = rays[n]
            touch["f" if abs(math.sin(a)) < 1e-6 else "l" if math.sin(a) > 0 else "r"].append((n, args.touch_ref))
    esc = add_touch_escape(c, GAIN, hb, touch, suppress=list(forage["steer"].values()), quiet=[forage["feed"]],
                           touch_tau_ms=args.touch_tau)
    for s, o in (("l", "r"), ("r", "l")):
        for i, j in enumerate(AXIAL):
            drive_joint(esc["esc"][s], j, 1 if o == "l" else -1, args.escape_axial)

    learn = dict(profile=PROFILE)
    labels = {"design": "salamander crawler: hindbrain + axial travelling wave + diagonal-trot limbs + girdle coupling + touch escape + learned foraging (G2 composition)",
              "parameters": vars(args), "composition": {
                  "shared_hindbrain": ["MLR", "RS", "ARTR", "turn bouts"],
                  "axial_segments_from": "zebrafish spinal E/A/C block",
                  "limb_oscillators_from": "fly/hexapod add_leg_oscillators",
                  "escape_from": "add_touch_escape (fish/fly)", "foraging_from": "add_learned_foraging (fish/fly/keenon)"}}
    c.write(Path(args.output), Path(args.config_out) if args.config_out else None, synaptic_gain=GAIN, depth=args.depth, labels=labels)
    apply_learner(Path(args.output), Path(args.config_out) if args.config_out else None, GAIN, learn)
    return c


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--output", default="network_salamander_crawler.json")
    ap.add_argument("--config-out")
    ap.add_argument("--depth", type=int, default=0)
    ap.add_argument("--tau-m", type=float, default=5.0)
    ap.add_argument("--tau-e", type=float, default=15.0)
    ap.add_argument("--plateau", type=float, default=22.0)
    ap.add_argument("--tau-a-axial", type=float, default=300.0,
                    help="axial adaptation (ms): 300 lets the girdles entrain the wave 1:1 at the trot (~1.6 Hz); 150 ran it at 3 Hz")
    ap.add_argument("--axial-drive", type=float, default=3.0)
    ap.add_argument("--axial-motor", type=float, nargs=3, default=[4.0, 8.0, 8.0])
    ap.add_argument("--w-desc", type=float, default=2.0)
    ap.add_argument("--w-cdesc", type=float, default=1.0)
    ap.add_argument("--mlr-release", type=float, default=0.5)
    ap.add_argument("--rs-mutual", type=float, default=6.0)
    ap.add_argument("--leg-drive", type=float, default=4.0)
    ap.add_argument("--leg-plateau", type=float, default=22.0)
    ap.add_argument("--tau-a-swing", type=float, default=200.0)
    ap.add_argument("--tau-a-stance", type=float, default=300.0)
    ap.add_argument("--inter", type=float, default=3.0)
    ap.add_argument("--inter-ex", type=float, default=2.0)
    ap.add_argument("--protract-delay", type=float, default=40.0)
    ap.add_argument("--coxa", type=float, default=10.0)
    ap.add_argument("--lift", type=float, default=10.0)
    ap.add_argument("--press", type=float, default=3.0)
    ap.add_argument("--girdle", type=float, default=6.0, help="girdle stance -> same-side axial half-centre (mV/ms)")
    ap.add_argument("--turn-axial", type=float, default=3.0)
    ap.add_argument("--turn-stride", type=float, default=3.0)
    ap.add_argument("--artr-axial", type=float, default=0.8)
    ap.add_argument("--wall-rs", type=float, default=3.0)
    ap.add_argument("--explore", type=int, default=1)
    ap.add_argument("--explore-on-ms", type=float, default=2000.0)
    ap.add_argument("--explore-off-ms", type=float, default=12000.0)
    ap.add_argument("--steer-axial", type=float, default=3.0)
    ap.add_argument("--steer-stride", type=float, default=3.0)
    ap.add_argument("--touch-ref", type=float, default=0.9, help="near-ray value counted as touch")
    ap.add_argument("--touch-tau", type=float, default=600.0)
    ap.add_argument("--escape-axial", type=float, default=6.0)
    args = ap.parse_args()
    c = build(args)
    print(f"wrote {args.output}: {len(c.cells)} cells, {len(c.syn)} synapses, {len(c.sense)} sensory projections")


if __name__ == "__main__":
    main()
