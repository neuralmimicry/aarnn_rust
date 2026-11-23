#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Slow bipedal walk for the NAO (OEM mechanics, v2 surrogate), exact cells.

Layers (shared blocks from ``aarnn_circuit``):

* posture: the StandInit tonics of ``build_nao_stand_network_json``;
* walk command ``WALK``: a slow graded cell (tau ``--walk-tau``) that lets
  the body settle after the servo wake-up before the rhythm starts, as the
  mesencephalic locomotor region gates spinal rhythm generators;
* rhythm: one half-centre pair (``add_leg_oscillators`` with a single
  "gait" unit, 30 mV plateaus) alternating A = body over the left foot,
  right leg swinging, and B, its mirror, at ~0.5 Hz (adaptation 1.9 s,
  balanced lead);
* lateral weight shift: HipRoll -sway / AnkleRoll +sway on both legs in A
  (mirrored in B) carries the trunk over the stance foot;
* swing: the swinging leg flexes hip/knee/ankle (lift) in its own phase;
* progression: delay cells ``D_A``/``D_B`` (tau ``--lag``) lag the phase by
  roughly a quarter cycle, protracting the swing hip and retracting the
  stance hip (ankles compensate). A square phase without the lag advanced
  0.10 m in 20 s on a programmed model; with a 0.3 s lag 0.55 m. The sign of
  the progression term sets walking forward (+1) or backward (-1);
* obstacle avoidance (``add_sonar_avoidance``): the chest sonars drive the
  shared hindbrain's bistable turn cells, which shorten one side's stride;
  very near, a back-off bout reverses the stride for a few seconds.

Each joint's output cell is one graded cell whose density sets its target
(lo + r (hi - lo)); a term's weight is the release-curve offset
(``release_tonic``) for its angle change from the posture, divided by the
presynaptic cell's high release (E 0.97, D 0.90, measured). Synaptic drive
adds to the tonic as current in exact cells, excitatory or inhibitory
(measured: target densities 0.419 / 0.2 reached as 0.417 / 0.199).

  python3 scripts/build_nao_walk_network_json.py
"""
import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aarnn_circuit import (Circuit, add_hindbrain, add_leg_oscillators, add_sensor_relay, bistable_band,  # noqa: E402
                           drive_weight, graded, release_tonic, steady_release)
from build_nao_stand_network_json import posture_angle  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
GAIN = 100.0
SONAR_REACH_M = 2.55


def add_sonar_avoidance(c, args, dcells):
    """Sonar obstacle avoidance through the shared hindbrain.

    Each chest sonar (normalised 1 - distance / 2.55 m, aimed 20 deg to its
    side) reaches an excitation-only relay centred on the onset distance; a
    proximity cell ``P`` turns on as the reading passes it (k * (relay -
    r0)) and drives the bistable reticulospinal turn cell of its side
    (``RS_l`` for an obstacle on the left, which turns the body right). A
    turn shortens the stride on the side to turn towards: ``RS_l`` inhibits
    the right leg's stride delay cells (and, weaker, the left's, so the body
    pivots rather than walks on), ``RS_r`` the mirror. The RS hysteresis
    carries a turn until the sonar clears; ARTR picks the side for a head-on
    wall.
    """
    common = dict(nmda_ratio=0.0, synaptic_gain=GAIN)
    hb = add_hindbrain(c, GAIN, plateau_mv=30.0, mlr_release=0.5, rs_plateau_mv=args.rs_plateau)
    rs = hb["rs"]
    s_on = 1.0 - args.onset_m / SONAR_REACH_M
    _, hi_knee = bistable_band(args.rs_plateau)
    w_prs = hi_knee / steady_release(6.0)
    for name, side in (("Sonar/Left", "l"), ("Sonar/Right", "r")):
        relay, r0, slope = add_sensor_relay(c, GAIN, name, s_on, tau_ms=30.0)
        # P reaches 6 mV/ms when the reading is 0.1 (0.255 m) nearer than onset.
        k = 6.0 / max(slope * 0.1, 1e-3)
        tname = f"Sense:sonar:{side}"
        c.type(tname, **graded(tau_ms=20.0, tonic=-k * r0), **common)
        cell = c.cell(f"P_sonar_{side}", tname, "sensory")
        c.connect(relay, cell, drive_weight(k, GAIN))
        c.connect(cell, rs[side], drive_weight(w_prs, GAIN))
    if args.yaw_turn > 0:
        # Hip yaw-pitch steering (NAO turns with this one motor): during a
        # turn it alternates with the gait phase (programmed: -0.3 rad in A,
        # +0.3 in B turned 39 deg in 28 s to the right; the mirror turned
        # left). AND cells (phase x turn command) gate the alternation.
        c.type("Turn:and", **graded(tau_ms=10.0, tonic=-args.and_threshold), **common)
        and_high = steady_release(12.0 * 0.95 + 12.0 * 0.8 - args.and_threshold)
        e = {"A": dcells.get("E_A"), "B": dcells.get("E_B")}
        for rs_side, sign in (("l", -1.0), ("r", 1.0)):     # RS_l turns right
            for ph, phase_sign in (("A", 1.0), ("B", -1.0)):
                y = c.cell(f"Y_{rs_side}_{ph}", "Turn:and", "spinal")
                c.connect(e[ph], y, drive_weight(12.0, GAIN))
                c.connect(rs[rs_side], y, drive_weight(12.0, GAIN))
                hb.setdefault("yaw_cells", []).append((y, sign * phase_sign * args.yaw_turn, and_high))
    if args.backup > 0:
        # Back-off bout (C. elegans reversal, a person stepping back): when a
        # sonar reads nearer than --near-m, a latching BACK cell (30 mV
        # plateau) with its own adaptation (--backup-ms) suppresses the stride
        # cells' normal drive and drives each from the opposite half-centre
        # (X AND cells), which reverses the stride: the body walks backward
        # for a few seconds while the turn command keeps steering.
        s_near = 1.0 - args.near_m / SONAR_REACH_M
        c.type("Hindbrain:BACK", **graded(tau_ms=50.0, plateau_mv=30.0), **common)
        c.type("Hindbrain:BACK_adapt", **graded(tau_ms=args.backup_ms), **common)
        back = c.cell("BACK", "Hindbrain:BACK", "hindbrain")
        back_a = c.cell("BACK_A", "Hindbrain:BACK_adapt", "hindbrain")
        c.connect(back, back_a, drive_weight(10.0, GAIN))
        c.connect(back_a, back, drive_weight(-args.backup_adapt, GAIN))
        for name, side in (("Sonar/Left", "l"), ("Sonar/Right", "r")):
            relay, r0, slope = add_sensor_relay(c, GAIN, name, s_near, tau_ms=30.0, name=f"R_near_{name}")
            k = 6.0 / max(slope * 0.05, 1e-3)
            c.type(f"Sense:sonar_near:{side}", **graded(tau_ms=20.0, tonic=-k * r0), **common)
            near = c.cell(f"N_sonar_{side}", f"Sense:sonar_near:{side}", "sensory")
            c.connect(relay, near, drive_weight(k, GAIN))
            c.connect(near, back, drive_weight(args.back_kick, GAIN))
        # BACK decides; a slow reversal cell REV (--rev-tau, no plateau)
        # carries the gating, so the stride passes through zero before it
        # reverses and returns the same way: switching in one step threw a
        # protracted leg back and tipped the body over (2 of 3 early runs).
        c.type("Gait:reverse_level", **graded(tau_ms=args.rev_tau), **common)
        rev = c.cell("REV", "Gait:reverse_level", "spinal")
        c.connect(back, rev, drive_weight(12.0, GAIN))
        c.type("Gait:back_and", **graded(tau_ms=10.0, tonic=-13.0), **common)
        e = {"A": dcells["E_A"], "B": dcells["E_B"]}
        for ph, other in (("A", "B"), ("B", "A")):
            x = c.cell(f"X_{ph}", "Gait:back_and", "spinal")
            c.connect(e[other], x, drive_weight(12.0, GAIN))
            c.connect(rev, x, drive_weight(12.0, GAIN))
            for side in "LR":
                c.connect(rev, dcells[side, ph], drive_weight(-24.0, GAIN))
                c.connect(x, dcells[side, ph], drive_weight(args.back_drive, GAIN))
        hb["back"] = back
    # RS_l turns right: shorten the right stride (inner), slightly the left.
    for rs_side, inner, outer in (("l", "R", "L"), ("r", "L", "R")):
        for ph in "AB":
            c.connect(rs[rs_side], dcells[inner, ph], drive_weight(-args.turn, GAIN))
            if args.turn_outer > 0:
                c.connect(rs[rs_side], dcells[outer, ph], drive_weight(-args.turn_outer, GAIN))
    return hb


def build(args):
    model = json.loads((ROOT / "sim/content/embodiments/generated/nao.json").read_text())
    acts = {a["name"]: a for a in model["actuators"]}
    c = Circuit.for_embodiment("nao", exact_cells=True)
    common = dict(nmda_ratio=0.0, synaptic_gain=GAIN)
    c.type("Command:WALK", **graded(tau_ms=args.walk_tau, tonic=release_tonic(args.walk)), **common)
    walk = c.cell("WALK", "Command:WALK", "hindbrain")
    E, _, _ = add_leg_oscillators(c, GAIN, ("gait",), [], walk, args.drive, drive_release=args.walk,
                                  tau_a_swing_ms=args.tau_a, tau_a_stance_ms=args.tau_a)
    # The oscillator's lead side carries +0.3 mV/ms to break the start-up
    # symmetry, which lengthens phase A (1.08 vs 0.92 s) and curves the walk;
    # giving the other side most of it keeps a lead of 0.05 and equal phases.
    c.types["Leg:E"]["tonic_current"] = args.lead_balance
    ea, eb = E["gait", "sw"], E["gait", "st"]
    # Stride delay cells, one pair per leg so the turn command can shorten
    # one side's stride (identical dynamics on both sides when unopposed).
    c.type("Gait:delay", **graded(tau_ms=args.lag), **common)
    dcells = {}
    for side in "LR":
        for ph, e in (("A", ea), ("B", eb)):
            dcells[side, ph] = c.cell(f"D_{side}_{ph}", "Gait:delay", "spinal")
            c.connect(e, dcells[side, ph], drive_weight(12.0, GAIN))
    hb = None
    if args.avoid:
        dcells["E_A"], dcells["E_B"] = ea, eb
        hb = add_sonar_avoidance(c, args, dcells)
        del dcells["E_A"], dcells["E_B"]
    sway, lift, step = args.sway, args.lift, args.step * args.direction
    # Per joint, the angle change wanted in phase A and in phase B from each
    # cell pair (half-centres E, delay cells D).
    want = {}   # joint -> {"E": [dA, dB], "D": [dA, dB]}

    def add(joint, pair, d_a, d_b):
        w = want.setdefault(joint, {"E": [0.0, 0.0], "D": [0.0, 0.0]})[pair]
        w[0] += d_a
        w[1] += d_b
    for side in "LR":
        add(side + "HipRoll", "E", -sway, sway)
        add(side + "AnkleRoll", "E", sway, -sway)
    for side, in_a in (("R", 1.0), ("L", 0.0)):
        swing = (in_a, 1.0 - in_a)
        add(side + "HipPitch", "E", -lift * swing[0], -lift * swing[1])
        add(side + "KneePitch", "E", 2 * lift * swing[0], 2 * lift * swing[1])
        add(side + "AnklePitch", "E", -lift * swing[0], -lift * swing[1])
    # Stride trim: a longer right stride turns the body left (steering by
    # stride asymmetry; +trim counters the gait's measured right curve).
    for side, sgn, scale in (("R", 1.0, 1.0 + args.trim), ("L", -1.0, 1.0 - args.trim)):
        add(side + "HipPitch", "D", -sgn * step * scale, sgn * step * scale)
        add(side + "AnklePitch", "D", sgn * step * scale, -sgn * step * scale)
    # Weights: each phase's offset divided by its cell's high release
    # (measured: E 0.97, delay 0.90). Solving each pair jointly against the
    # idle cell's residual (--solve) matched the targets better but walked
    # worse (fell in 3/3 40 s runs; per-phase weights stood in 3/3).
    levels = {"E": (args.e_on, args.e_off), "D": (args.d_on, args.d_off)}
    def cells(pair, joint):
        side = joint[0]
        return (ea, eb) if pair == "E" else (dcells[side, "A"], dcells[side, "B"])
    for o in c.outputs:
        lo, hi = acts[o]["control_range"]
        q0 = min(max(posture_angle(o), lo), hi)

        def r(q):
            return min(max((q - lo) / (hi - lo), 0.01), 0.99)
        tonic = release_tonic(r(q0))
        c.type(f"Posture:{o}", **graded(tau_ms=args.out_tau, release_hz=500.0, tonic=tonic), **common)
        c.output(o, f"Posture:{o}")
        for pair, (d_a, d_b) in want.get(o, {}).items():
            if d_a == 0.0 and d_b == 0.0:
                continue
            i_a = release_tonic(r(q0 + d_a)) - tonic
            i_b = release_tonic(r(q0 + d_b)) - tonic
            on, off = levels[pair]
            if args.solve:
                det = on * on - off * off
                w_a = (on * i_a - off * i_b) / det
                w_b = (on * i_b - off * i_a) / det
            else:
                w_a, w_b = i_a / on, i_b / on
            for cell, mv in zip(cells(pair, o), (w_a, w_b)):
                if abs(mv) > 1e-9:
                    c.connect(cell, o, drive_weight(mv, GAIN))
        if hb and o.endswith("HipYawPitch"):
            for y, delta, high in hb.get("yaw_cells", []):
                c.connect(y, o, drive_weight((release_tonic(r(q0 + delta)) - tonic) / high, GAIN))
    labels = {"design": "NAO slow walk: StandInit posture + half-centre gait (sway, swing, lagged progression)",
              "parameters": vars(args)}
    c.write(Path(args.output), Path(args.config_out), synaptic_gain=GAIN, depth=0, labels=labels)
    return c


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--output", default="network_nao_walk.json")
    ap.add_argument("--config-out", default="webots_world/configs/config_nao_walk.json")
    ap.add_argument("--walk", type=float, default=0.5, help="walk command release")
    ap.add_argument("--walk-tau", type=float, default=3000.0, help="walk command rise (ms): settle before stepping")
    ap.add_argument("--drive", type=float, default=4.0)
    ap.add_argument("--tau-a", type=float, default=1900.0, help="half-centre adaptation (ms): ~0.5 Hz with --lead-balance 0.25")
    ap.add_argument("--lead-balance", type=float, default=0.25, help="tonic on the non-lead half-centre (lead has 0.3)")
    ap.add_argument("--lag", type=float, default=300.0, help="progression delay cell tau (ms)")
    ap.add_argument("--sway", type=float, default=0.30, help="lateral weight shift, hip/ankle roll (rad)")
    ap.add_argument("--lift", type=float, default=0.25, help="swing flexion (rad; knee gets 2x)")
    ap.add_argument("--step", type=float, default=0.10, help="hip protraction/retraction (rad)")
    ap.add_argument("--direction", type=float, default=1.0, help="+1 forward, -1 backward")
    ap.add_argument("--out-tau", type=float, default=15.0)
    ap.add_argument("--e-on", type=float, default=0.97, help="half-centre high release (measured)")
    ap.add_argument("--e-off", type=float, default=0.06)
    ap.add_argument("--d-on", type=float, default=0.90, help="delay cell high release (measured)")
    ap.add_argument("--d-off", type=float, default=0.10)
    ap.add_argument("--avoid", type=int, default=1, help="sonar obstacle avoidance (1 on, 0 off)")
    ap.add_argument("--onset-m", type=float, default=0.9, help="sonar distance at which proximity starts (m)")
    ap.add_argument("--rs-plateau", type=float, default=18.0)
    ap.add_argument("--turn", type=float, default=12.0, help="turn command onto the inner stride cells (mV/ms)")
    ap.add_argument("--turn-outer", type=float, default=4.0, help="turn command onto the outer stride cells (mV/ms)")
    ap.add_argument("--backup", type=int, default=1, help="back-off bouts when a sonar reads nearer than --near-m (1 on)")
    ap.add_argument("--near-m", type=float, default=0.4)
    ap.add_argument("--backup-ms", type=float, default=8000.0, help="back-off bout adaptation (ms): ~4 s bouts with --backup-adapt 12")
    ap.add_argument("--backup-adapt", type=float, default=12.0, help="BACK adaptation weight (mV/ms): sets the bout length")
    ap.add_argument("--rev-tau", type=float, default=600.0, help="stride-reversal level time constant (ms): smooth forward<->backward")
    ap.add_argument("--back-kick", type=float, default=4.0, help="near-proximity drive onto BACK (mV/ms)")
    ap.add_argument("--back-drive", type=float, default=32.0, help="AND cell drive onto the stride cells while backing (41: faster corner escape but fell head-on)")
    ap.add_argument("--yaw-turn", type=float, default=0.0, help="hip yaw-pitch turn alternation (rad); 0 = stride steering only")
    ap.add_argument("--and-threshold", type=float, default=13.0)
    ap.add_argument("--solve", action="store_true", help="solve each pair against the idle cell's residual")
    ap.add_argument("--trim", type=float, default=0.6, help="stride trim: right x (1+trim), left x (1-trim); 0.6 walks ~straight (0.5: -60 deg, 0.7: +38 deg in 40 s)")
    args = ap.parse_args()
    c = build(args)
    print(f"wrote {args.output}: {len(c.cells)} cells, {len(c.syn)} synapses, {len(c.sense)} sensory projections")


if __name__ == "__main__":
    main()
