#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Freenove hexapod walking circuit: a tripod rhythm on the kit's own senses.

The stock Freenove Big Hexapod (Raspberry Pi) walks open loop: its software
steps a fixed gait cycle through leg inverse kinematics, and its 18 servos
report nothing back (PWM position servos, no angle or load feedback; no foot
switches). Its senses are an MPU6050 (accelerometer + gyro), an HC-SR04
ultrasonic ranger and a camera. This circuit walks that body the same way,
as a central pattern generator, and uses only those senses:

* rhythm: one half-centre pair (``add_leg_oscillators`` on a single
  "gait" unit, as the NAO walk): phase A swings tripod lf/rm/lr while
  rf/lm/rr stand, phase B the reverse;
* lift: the half-centres themselves raise the swinging tripod's feet;
* stride: lagged delay cells (``D``) carry the protraction of the swinging
  legs and the retraction of the standing ones, so a foot lifts before it
  swings forward;
* posture: each joint's motor cell holds the stance by tonic current;
* joint targets come from the body's leg kinematics
  (``scripts/hexapod_kinematics.py``, the geometry the gait probe measured:
  ~5 cm/s at 1 Hz with the coxa 0.10 m above the feet);
* obstacle turn (HC-SR04, as the kit's own ultrasonic mode) through the
  shared hindbrain: the proximity cell ``P`` sets a bistable turn cell
  (``RS``; ARTR picks the side), which stops the stride and gates turn
  cells (``X``) that rotate the stance in place for a turn bout.

The joint-angle and foot-contact channels of the body record (add-ons the
stock kit lacks) are not used.
"""
import argparse
import json
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aarnn_circuit import (Circuit, add_hindbrain, add_learned_foraging, add_leg_oscillators, add_sensor_relay,  # noqa: E402
                           add_turn_bouts, bistable_band,
                          drive_weight, graded, release_tonic, steady_release)
from hexapod_kinematics import LEGS, TRIPOD, joint_targets  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
GAIN = 100.0
KINDS = ("coxa", "femur", "tibia")


def build(args):
    model = json.loads((ROOT / "sim/content/embodiments/generated/hexapod.json").read_text())
    acts = {a["name"]: a for a in model["actuators"]}
    sensors = {s["name"]: s for s in model["io"]["sensors"]}
    c = Circuit.for_embodiment("hexapod", exact_cells=True)
    common = dict(nmda_ratio=0.0, synaptic_gain=GAIN)
    c.type("Command:WALK", **graded(tau_ms=args.walk_tau, tonic=release_tonic(args.walk)), **common)
    walk = c.cell("WALK", "Command:WALK", "brain", (0.0, 0.0, 0.03))
    E, _, _ = add_leg_oscillators(c, GAIN, ("gait",), [], walk, args.drive, drive_release=args.walk,
                                  tau_a_swing_ms=args.tau_a, tau_a_stance_ms=args.tau_a,
                                  positions={"gait": (0.0, 0.0)})
    c.types["Leg:E"]["tonic_current"] = args.lead_balance
    phase = {0: E["gait", "sw"], 1: E["gait", "st"]}     # phase k swings tripod k
    # Stride: a chain of --stride-chain lagged interneurons per side and
    # phase; the last one drives the joints. One first-order cell gave an
    # exponential stride (a shove at each phase switch, then a near stop)
    # that stalled on the habitat's ribbed mat (24 Nov) where the probe's
    # constant-speed stride crossed; a two-cell chain gives a smooth S-shaped
    # protraction and retraction.
    c.type("Gait:delay", **graded(tau_ms=args.lag), **common)
    dcell, chain = {}, []
    for k in (0, 1):
        for side in "lr":
            prev = phase[k]
            for n in range(args.stride_chain):
                cell = c.cell(f"D{n}_{side}_{k}" if args.stride_chain > 1 else f"D_{side}_{k}", "Gait:delay", "spinal",
                              (0.0, 0.02 if side == "l" else -0.02, 0.01 * k + 0.003 * n))
                c.connect(prev, cell, drive_weight(12.0, GAIN))
                chain.append(cell)
                prev = cell
            dcell[side, k] = prev

    xcell, zcell = {}, {}
    if args.avoid:
        # HC-SR04 (legacy-range, value = 1 - distance / reach; the record's
        # two channels are the transducer pair of the one ranger). Through
        # the shared hindbrain: an excitation-only relay per channel, a
        # proximity cell P that turns on as the echo passes --near-m, the
        # bistable turn cells RS_l / RS_r (RS_l turns right, RS_r left) and
        # ARTR choosing the side (one ranger cannot tell left from right).
        # A turn bout (BOUT, below) carries a turn on after the echo is lost
        # (the HC-SR04 is blind beyond ~22 deg of incidence) and the turn
        # repeats while the obstacle is still ahead.
        hb = add_hindbrain(c, GAIN, plateau_mv=30.0, mlr_release=0.5, rs_plateau_mv=args.rs_plateau, rs_mutual=args.rs_mutual)
        rs = hb["rs"]
        sonar = [n for n, sp in sensors.items() if sp["kind"] == "legacy-range"]
        reach = sensors[sonar[0]]["angle_rad"]
        s_on = 1.0 - args.near_m / reach
        _, hi_knee = bistable_band(args.rs_plateau)
        w_prs = hi_knee / steady_release(6.0)
        c.type("Sense:sonar", **graded(tau_ms=20.0), **common)
        prox = c.cell("P_sonar", "Sense:sonar", "sensory", (0.10, 0.0, 0.05))
        tonic = 0.0
        for n in sonar:
            relay, r0, slope = add_sensor_relay(c, GAIN, n, s_on, tau_ms=30.0)
            # P reaches 6 mV/ms when the echo is --near-band metres nearer than onset.
            k = 6.0 / max(slope * args.near_band / reach, 1e-3) / len(sonar)
            c.connect(relay, prox, drive_weight(k, GAIN))
            tonic -= k * r0
        c.types["Sense:sonar"]["tonic_current"] = tonic
        # Turn bout: P switches on a bistable BOUT cell, ended by its own
        # adaptation after ~--bout-ms; while on it holds whichever RS won
        # (input inside the RS band: it keeps the winner on but cannot wake
        # the loser), so a turn keeps its direction for the whole bout and
        # does not stop when the echo is lost. An adaptation on RS itself
        # (24 Nov, first version) handed the turn to the other side mid-bout
        # and undid it.
        # 22 mV plateau (band ~0.46-1.57 mV/ms) with a tonic inside the band: off
        # until P kicks it, then on until the adaptation pushes it below the
        # lower knee (a 30 mV plateau held itself on against any adaptation).
        c.type("Hindbrain:bout", **graded(tau_ms=50.0, plateau_mv=22.0, tonic=args.bout_tonic), **common)
        c.type("Hindbrain:bout_adapt", **graded(tau_ms=args.bout_ms), **common)
        bout = c.cell("BOUT", "Hindbrain:bout", "hindbrain")
        bout_a = c.cell("BOUT_A", "Hindbrain:bout_adapt", "hindbrain")
        c.connect(prox, bout, drive_weight(args.bout_kick, GAIN))
        c.connect(bout, bout_a, drive_weight(10.0, GAIN))
        c.connect(bout_a, bout, drive_weight(-args.bout_adapt, GAIN))
        lo_knee, _ = bistable_band(args.rs_plateau)
        for side in "lr":
            c.connect(prox, rs[side], drive_weight(w_prs, GAIN))
            c.connect(bout, rs[side], drive_weight(lo_knee + args.bout_hold * (hi_knee - lo_knee), GAIN))
            for dc in chain:
                c.connect(rs[side], dc, drive_weight(-args.near_stop, GAIN))
        if args.learn_groups > 0:
            # Learning by experience (same pattern and algorithms as the NAO,
            # insect anatomy): the innate near turn (P) is the teacher. P
            # drives a dopaminergic teaching cell LC (the network's modulatory
            # cell, gating plasticity), and a learning-turn cell LT - a
            # mushroom-body output neuron - drives the turn machinery exactly
            # as P does. LT's candidate inputs are distance-tuned sonar cells
            # CS (Kenyon-cell-like, a sparse group joined inside) whose
            # synapses onto LT start silent and are the only plastic ones;
            # when an echo at their distance predicts the near turn, they
            # strengthen and the hexapod learns to turn earlier.
            c.type("Modulator:LC", **graded(tau_ms=50.0), **common)
            lc = c.cell("LC", "Modulator:LC", "hindbrain")
            c.connect(prox, lc, drive_weight(12.0, GAIN))
            c.type("Learn:turn", **graded(tau_ms=20.0), **common)
            lt = c.cell("LT", "Learn:turn", "hindbrain")
            # The innate near turn teaches LT (as the NAO's near reflex does):
            # LT is on during the reflex, so the sonar cells active with it
            # pair with it under the LC gate. Without this drive LT stayed
            # at 0.5 Hz and nothing learned.
            c.connect(prox, lt, drive_weight(args.learn_teach_mv, GAIN))
            c.connect(lt, bout, drive_weight(args.bout_kick, GAIN))
            for side in "lr":
                c.connect(lt, rs[side], drive_weight(w_prs, GAIN))
            import random
            rnd = random.Random(7)
            cells = []
            for k in range(args.learn_groups):
                d = args.learn_near_m + (args.learn_far_m - args.learn_near_m) * k / max(1, args.learn_groups - 1)
                cell, _, _ = add_sensor_relay(c, GAIN, sonar[0], 1.0 - d / reach, tau_ms=30.0, name=f"CS_{k:02d}")
                c.connect(cell, lt, drive_weight(args.learn_w0, GAIN))
                cells.append(cell)
            for a_ in cells:
                for b_ in cells:
                    if a_ != b_ and rnd.random() < args.learn_within_p:
                        c.connect(a_, b_, drive_weight(args.learn_within_mv, GAIN))
        if args.explore:
            # Spontaneous run-and-turn bouts (shared hindbrain, zebrafish
            # style): a bout clock kicks both RS cells and the ARTR side
            # turns. The stock kit has no leg touch sense, and a leg wedged
            # on something the narrow HC-SR04 cone misses (a slalom post
            # beside the front leg, 24 Nov: 96 s jammed) is only freed by a
            # turn the senses did not ask for.
            bouts = add_turn_bouts(c, GAIN, hb, on_ms=args.explore_on_ms, off_ms=args.explore_off_ms)
        if args.forage:
            # Learned recharging (needs-v1): innate charging reflex, hunger and
            # run-and-turn trend cells; the dock beacon's value is learned
            # from charging (aarnn_circuit.add_learned_foraging).
            forage = add_learned_foraging(c, GAIN, walk, bouts["on"] if args.explore else None, levels=args.forage_levels,
                                          hunger_below=args.forage_hunger, w0_mv=args.learn_w0,
                                          teach_mv=args.learn_teach_mv, steer=args.forage_steer > 0)
            if args.forage_steer > 0:
                # Learned steering through the bistable turn cells: STEER_l
                # (dock cue stronger on the left) excites RS_r, which turns
                # the hexapod left; a strong learned difference flips the turn
                # decision toward the dock, as an obstacle flips it away.
                c.connect(forage["steer"]["l"], rs["r"], drive_weight(args.forage_steer, GAIN))
                c.connect(forage["steer"]["r"], rs["l"], drive_weight(args.forage_steer, GAIN))
        if args.backup:
            # Back-off (C. elegans reversal before a turn; an insect backing
            # out of a dead end): BACK, kicked by a strong bout onset (the
            # exploration clock or the sonar bout), stops the forward stride
            # and gates AND cells (Z) that reverse it: the swinging tripod
            # retracts, the standing one protracts. A wedged leg is pulled
            # clear while the turn command steers.
            c.type("Gait:BACK", **graded(tau_ms=50.0, tonic=-args.back_threshold), **common)
            back = c.cell("BACK", "Gait:BACK", "hindbrain")
            c.connect(bout, back, drive_weight(args.back_kick, GAIN))
            if args.explore:
                c.connect(bouts["on"], back, drive_weight(args.back_kick, GAIN))
            for dc in chain:
                c.connect(back, dc, drive_weight(-args.near_stop, GAIN))
            if args.back_onset_ms > 0:
                # Back-off at the bout's onset only (incoherent feed-forward:
                # the bout excites BACK fast and, through the slow BACK_I,
                # inhibits it after ~--back-onset-ms), so a long or renewed
                # bout reverses once and then turns in place. A BACK held for
                # the whole bout reversed and turned at once, and the left
                # turn made no headway against a wall (25 Nov, hexapod
                # growth seed 2: +0.6 deg/s left against -3.4 right).
                c.type("Gait:back_inh", **graded(tau_ms=args.back_onset_ms), **common)
                back_i = c.cell("BACK_I", "Gait:back_inh", "hindbrain")
                c.connect(bout, back_i, drive_weight(10.0, GAIN))
                if args.explore:
                    c.connect(bouts["on"], back_i, drive_weight(10.0, GAIN))
                c.connect(back_i, back, drive_weight(-args.back_onset_inh, GAIN))
            c.type("Gait:back_and", **graded(tau_ms=args.lag, tonic=-args.and_threshold), **common)
            for kk in (0, 1):
                zcell[kk] = c.cell(f"Z_{kk}", "Gait:back_and", "spinal")
                c.connect(phase[kk], zcell[kk], drive_weight(args.and_input, GAIN))
                c.connect(back, zcell[kk], drive_weight(args.and_input, GAIN))
        # Turn cells: a phase AND a turn command (each alone stays below the
        # threshold), lagged like the stride cells.
        c.type("Gait:turn_and", **graded(tau_ms=args.lag, tonic=-args.and_threshold), **common)
        for side in "lr":
            for kk in (0, 1):
                xcell[side, kk] = c.cell(f"X_{side}_{kk}", "Gait:turn_and", "spinal")
                c.connect(phase[kk], xcell[side, kk], drive_weight(args.and_input, GAIN))
                c.connect(rs[side], xcell[side, kk], drive_weight(args.and_input, GAIN))

    geo = dict(height=args.height, reach=args.reach, step_height=args.step_height)
    on = {"E": args.e_on, "D": args.d_on, "X": args.d_on}
    for side, row in LEGS:
        leg = side + row
        tripod = TRIPOD[(side, row)]
        q0 = joint_targets(side, row, **geo)
        q_lift = joint_targets(side, row, lift=1.0, **geo)
        # Stride trim: right stride x (1 + trim), left x (1 - trim); + turns left.
        stride = args.stride * (1.0 + (args.trim if side == "r" else -args.trim))
        q_pro = joint_targets(side, row, sx=0.5, stride=stride, **geo)
        q_ret = joint_targets(side, row, sx=-0.5, stride=stride, **geo)
        # Reversed stride: trimmed like the forward one unless --back-untrim
        # (the trim corrects the forward gait's drift; reversed, it yaws).
        z_stride = args.stride if args.back_untrim else stride
        z_pro = joint_targets(side, row, sx=0.5, stride=z_stride, **geo)
        z_ret = joint_targets(side, row, sx=-0.5, stride=z_stride, **geo)
        # RS_l (obstacle on the left) turns right (-yaw), RS_r left.
        t_pro = {d: joint_targets(side, row, sx=0.5, stride=0.0, turn=sg * args.turn, **geo) for d, sg in (("l", -1), ("r", 1))}
        t_ret = {d: joint_targets(side, row, sx=-0.5, stride=0.0, turn=sg * args.turn, **geo) for d, sg in (("l", -1), ("r", 1))}
        x = {"f": 0.076, "m": 0.0, "r": -0.076}[row]
        y = (1.0 if side == "l" else -1.0) * {"f": 0.055, "m": 0.085, "r": 0.055}[row]
        for j, kind in enumerate(KINDS):
            o = next(n for n in c.outputs if n.endswith(f"_{leg}_{kind}"))
            lo, hi = acts[o]["control_range"]

            def r(q):
                return min(max((q - lo) / (hi - lo), 0.01), 0.99)
            tonic = release_tonic(r(q0[j]))
            c.type(f"Posture:{o}", **graded(tau_ms=args.out_tau, release_hz=500.0, tonic=tonic), **common)
            c.output(o, f"Posture:{o}")

            def drive(cell, q, pair):
                mv = (release_tonic(r(q)) - tonic) / on[pair]
                if abs(mv) > 1e-6:
                    c.connect(cell, o, drive_weight(mv, GAIN))
            # Lift while this leg's tripod swings.
            drive(phase[tripod], q_lift[j], "E")
            # Stride: protract in the own swing phase, retract in the other.
            drive(dcell[side, tripod], q_pro[j], "D")
            drive(dcell[side, 1 - tripod], q_ret[j], "D")
            if zcell:
                # Reversed stride: retract while swinging, protract in stance.
                drive(zcell[tripod], z_ret[j], "X")
                drive(zcell[1 - tripod], z_pro[j], "X")
            for d in ("l", "r") if xcell else ():
                drive(xcell[d, tripod], t_pro[d][j], "X")
                drive(xcell[d, 1 - tripod], t_ret[d][j], "X")
        del x, y
    labels = {"design": "Freenove hexapod tripod CPG (half-centre rhythm, lagged stride, IK posture; HC-SR04 turn)",
              "parameters": vars(args) | {"output": str(args.output)}}
    c.write(Path(args.output), Path(args.config_out) if args.config_out else None,
            synaptic_gain=GAIN, depth=0, labels=labels)
    return c


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--output", default="network_hexapod_cpg.json")
    ap.add_argument("--config-out", default=None)
    ap.add_argument("--walk", type=float, default=0.5, help="walk command release")
    ap.add_argument("--walk-tau", type=float, default=1000.0, help="walk command rise (ms): stand before stepping")
    ap.add_argument("--drive", type=float, default=4.0)
    ap.add_argument("--tau-a", type=float, default=950.0, help="half-centre adaptation (ms); NAO 1900 ms ~ 0.5 Hz")
    ap.add_argument("--lead-balance", type=float, default=0.25)
    ap.add_argument("--lag", type=float, default=150.0, help="stride delay cell tau (ms)")
    ap.add_argument("--stride-chain", type=int, default=2, help="lagged interneurons in each stride chain (1 = single delay cell)")
    ap.add_argument("--stride", type=float, default=0.05, help="stride length along the body (m)")
    ap.add_argument("--trim", type=float, default=0.2, help="stride trim: right x (1+trim), left x (1-trim); + turns left (0.2 walked straight, 24 Nov: 0.1 -52 deg, 0.3 +59 deg in 30 s)")
    ap.add_argument("--height", type=float, default=0.10, help="coxa joint height above the feet (m)")
    ap.add_argument("--reach", type=float, default=0.09, help="foot distance beyond the coxa link (m)")
    ap.add_argument("--step-height", type=float, default=0.04, help="swing lift (m; the kit uses 40 mm)")
    ap.add_argument("--turn", type=float, default=0.3, help="stance rotation per stride while turning (rad; + = left)")
    ap.add_argument("--out-tau", type=float, default=15.0)
    ap.add_argument("--e-on", type=float, default=0.97, help="half-centre high release (measured on the NAO walk)")
    ap.add_argument("--d-on", type=float, default=0.90, help="delay cell high release (measured on the NAO walk)")
    ap.add_argument("--avoid", type=int, default=1, help="HC-SR04 obstacle turn (1 on)")
    ap.add_argument("--near-m", type=float, default=0.40, help="echo distance that starts the turn (m)")
    ap.add_argument("--near-band", type=float, default=0.05, help="echo distance past onset at which P reaches 6 mV/ms (m)")
    ap.add_argument("--rs-plateau", type=float, default=18.0, help="RS turn cell plateau (mV): the bistable band")
    ap.add_argument("--bout-ms", type=float, default=6500.0, help="turn bout adaptation (ms)")
    ap.add_argument("--bout-adapt", type=float, default=2.0, help="BOUT adaptation weight (mV/ms): ends a bout")
    ap.add_argument("--bout-tonic", type=float, default=1.0, help="BOUT tonic, inside its bistable band (mV/ms)")
    ap.add_argument("--rs-mutual", type=float, default=12.0, help="RS_l <-> RS_r inhibition (mV/ms)")
    ap.add_argument("--bout-kick", type=float, default=4.0, help="P -> BOUT drive (mV/ms)")
    ap.add_argument("--bout-hold", type=float, default=0.5, help="BOUT -> RS hold, as a fraction of the way across the RS band")
    ap.add_argument("--near-stop", type=float, default=20.0, help="RS -> stride cell inhibition (mV/ms)")
    ap.add_argument("--explore", type=int, default=1, help="spontaneous turn bouts (1 on)")
    ap.add_argument("--explore-on-ms", type=float, default=2000.0, help="turn bout adaptation (ms)")
    ap.add_argument("--explore-off-ms", type=float, default=12000.0, help="run adaptation (ms): ~7 s between bouts")
    ap.add_argument("--backup", type=int, default=1, help="back-off bouts with the turn bouts (1 on)")
    ap.add_argument("--back-kick", type=float, default=16.0, help="bout onset -> BACK drive (mV/ms)")
    ap.add_argument("--back-onset-ms", type=float, default=0.0,
                    help="BACK only at a bout's onset: slow feed-forward inhibition after this long (ms; 0 = BACK for the whole bout)")
    ap.add_argument("--back-onset-inh", type=float, default=40.0, help="BACK_I -> BACK inhibition (mV/ms)")
    ap.add_argument("--back-untrim", type=int, default=1,
                    help="1 (default since 25 Nov): the reversed stride ignores --trim, which corrects only the forward gait; 0 = old behaviour (reverse + left turn made no yaw)")
    ap.add_argument("--back-threshold", type=float, default=6.0, help="BACK tonic inhibition: only a strong bout onset reverses (mV/ms)")
    ap.add_argument("--and-threshold", type=float, default=14.0)
    ap.add_argument("--and-input", type=float, default=13.0, help="phase / RS -> turn cell drive (mV/ms each)")
    ap.add_argument("--forage", type=int, default=0,
                    help="1: learned recharging at the dock (needs-v1 channels; value of the beacon cue learned from charging)")
    ap.add_argument("--forage-levels", type=int, default=6, help="cue intensity bands per side")
    ap.add_argument("--forage-hunger", type=float, default=0.5, help="energy reserve below which the body is hungry")
    ap.add_argument("--forage-steer", type=float, default=0.0,
                    help="learned steering toward the stronger dock cue: STEER -> opposite RS turn cell (mV/ms); 0 = off")
    ap.add_argument("--learn-groups", type=int, default=0,
                    help="learning by experience: distance-tuned sonar cells (CS) onto a learning-turn cell LT (0 = off)")
    ap.add_argument("--learn-near-m", type=float, default=0.45, help="nearest tuned distance (m)")
    ap.add_argument("--learn-far-m", type=float, default=0.75, help="farthest tuned distance (m)")
    ap.add_argument("--learn-w0", type=float, default=0.05, help="initial (silent) CS -> LT drive (mV/ms)")
    ap.add_argument("--learn-teach-mv", type=float, default=12.0, help="innate near turn (P) -> LT teaching drive (mV/ms)")
    ap.add_argument("--learn-rule", default="oja", help="matrix plasticity rule of the learner (oja / hebb / stdp)")
    ap.add_argument("--learn-w-max-mv", type=float, default=1.5, help="cap of a learned CS -> LT synapse (mV/ms)")
    ap.add_argument("--learn-within-p", type=float, default=0.5)
    ap.add_argument("--learn-within-mv", type=float, default=0.3)
    ap.add_argument("--plastic", type=int, default=1, help="with --learn-groups: 1 learns, 0 frozen control")
    ap.add_argument("--layout", type=int, default=1,
                    help="1: place the cells in insect anatomy (scripts/layout_designed_circuit.py, in place)")
    args = ap.parse_args()
    c = build(args)
    print(f"wrote {args.output}: {len(c.cells)} cells, {len(c.syn)} synapses, {len(c.sense)} sensory projections")
    if args.layout:
        import subprocess
        subprocess.run([sys.executable, str(ROOT / "scripts/layout_designed_circuit.py"), str(args.output), str(args.output),
                        "--profile", "hexapod"], check=True)
    if args.learn_groups > 0 or args.forage:
        # Teacher-gated plasticity onto LT (anticipation) and V (foraging)
        # only (same fields as the NAO learner).
        snap = json.loads(Path(args.output).read_text())
        hidden = snap["connectome_labels"]["hidden_nodes"]
        outs = snap["connectome_labels"]["output_nodes"]
        fields = dict(
            aarnn_plasticity_enabled=bool(args.plastic), aarnn_graded_plasticity_enabled=True,
            # Only the sonar-cell (CS) and cue-conjunction (CF) synapses (and
            # newborns') onto LT / V learn; the teachers' synapses (P -> LT,
            # FEED -> V) are fixed, as a climbing fibre is.
            # Newborns (niche growth): inputs fixed, outputs silent and plastic
            # (25 Nov rule; plastic newborn inputs ran away).
            aarnn_plastic_cells=dict(enabled=True, hidden=[[h in ("LT", "V") for h in hidden]], outputs=[False] * len(outs),
                                     newborns=False, newborn_outputs=True,
                                     pre_hidden=[[h.startswith(("CS_", "CF_")) for h in hidden]]),
            aarnn_matrix_plasticity_rule=args.learn_rule,
            aarnn_plasticity_w_max=args.learn_w_max_mv / (20.0 / 1000.0 * 5.52 * GAIN),
            aarnn_rate_homeostasis_tau_ms=600000.0, aarnn_rate_homeostasis_gain=0.0, aarnn_plasticity_eta=0.0,
            # LC (near-reflex teacher) and DA_R (reward: energy gain) open the
            # plasticity gate; the gate averages their release.
            aarnn_modulatory_cells=dict(enabled=True, hidden=[[h in ("LC", "DA_R") for h in hidden]], outputs=[False] * len(outs)),
            aarnn_modulator_tau_ms=1000.0, aarnn_modulator_ref_hz=10.0)
        snap["net"].update(fields)
        # Neuromodulation stays enabled (25 Nov; no effect at depth 0, see aarnn_circuit.apply_learner).
        snap["connectome_labels"]["learning"] = dict(plastic_cells=[x for x, on in (("LT", args.learn_groups > 0), ("V", args.forage)) if on],
                                                     modulatory_cells=[x for x in ("LC", "DA_R") if x in hidden], teacher="P_sonar",
                                                     forage_teacher="FEED" if args.forage else None,
                                                     groups=args.learn_groups, plastic=bool(args.plastic))
        Path(args.output).write_text(json.dumps(snap))
        if args.config_out:
            cfg = json.loads(Path(args.config_out).read_text())
            cfg.update(fields)
            Path(args.config_out).write_text(json.dumps(cfg, indent=1))


if __name__ == "__main__":
    main()
