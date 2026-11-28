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
from aarnn_circuit import (Circuit, add_hindbrain, add_leg_oscillators, add_motor_sequence,  # noqa: E402
                           add_sensor_relay, bistable_band, drive_weight, graded, measure_drive_curve,
                           measure_sequence, posture_drive, release_tonic, steady_release)
from build_nao_stand_network_json import posture_angle  # noqa: E402
from nao_getup_program import sequence as getup_sequence  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
GAIN = 100.0


def _sonar_spec():
    io = json.loads((ROOT / "sim/content/embodiments/generated/nao.json").read_text())["io"]
    s = next(x for x in io["sensors"] if x["name"] == "Sonar/Left")
    return s["angle_rad"], s.get("min_m") or 0.0


# NAO V6 chest sonar (from the body record): range 0.20-0.80 m, read as
# 1 - distance / reach (0 = nothing within reach).
SONAR_REACH_M, SONAR_MIN_M = _sonar_spec()
BUMPERS = {"l": ("LFoot/Bumper/Left", "LFoot/Bumper/Right"), "r": ("RFoot/Bumper/Left", "RFoot/Bumper/Right")}


def sonar_value(d):
    """Normalised sonar reading at distance d (m)."""
    return 1.0 - min(max(d, SONAR_MIN_M), SONAR_REACH_M) / SONAR_REACH_M


def add_sonar_avoidance(c, args, dcells):
    """Sonar obstacle avoidance through the shared hindbrain.

    Each chest sonar (normalised 1 - distance / reach, NAO V6: 0.2-0.8 m,
    a 60 deg cone aimed 26 deg to its side and 14 deg down) reaches an excitation-only relay centred on the onset distance; a
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
    s_on = sonar_value(args.onset_m)
    _, hi_knee = bistable_band(args.rs_plateau)
    w_prs = hi_knee / steady_release(6.0)
    learn = getattr(args, "learn_groups", 0) > 0
    for name, side in (("Sonar/Left", "l"), ("Sonar/Right", "r")):
        if learn:
            break   # the far-sonar turn is learned, not innate (see below)
        relay, r0, slope = add_sensor_relay(c, GAIN, name, s_on, tau_ms=30.0)
        # P reaches 6 mV/ms when the reading is 0.1 (0.08 m) nearer than onset.
        k = 6.0 / max(slope * 0.1, 1e-3)
        tname = f"Sense:sonar:{side}"
        c.type(tname, **graded(tau_ms=20.0, tonic=-k * r0), **common)
        cell = c.cell(f"P_sonar_{side}", tname, "sensory")
        c.connect(relay, cell, drive_weight(k, GAIN))
        c.connect(cell, rs[side], drive_weight(w_prs, GAIN))
    if learn:
        # Learning by experience (distributed adaptive control): no innate
        # far-sonar turn. A learning-turn cell LT per side drives that
        # side's turn cell RS exactly as the innate P cell did; its teaching
        # input is the innate near reflex (N cells, below), its candidate
        # input a group of distance-tuned sonar cells whose synapses onto LT
        # start silent and are the only plastic synapses (plastic-cell mask
        # on LT). Each group has `learn_groups` cells (the group-size
        # threshold), sparse inside (each pair with probability
        # `learn_within_p`), and the two groups form one collection joined
        # by a 2-synapse attachment.
        import random
        rnd = random.Random(7)
        c.type("Learn:turn", **graded(tau_ms=20.0), **common)
        if args.learn_teacher:
            # Teacher-gated learning: the innate near reflex (N cells, below)
            # also drives a noradrenergic locus coeruleus cell, the network's
            # modulatory cell (aarnn_modulatory_cells), whose release gates
            # plasticity - aversive events gate synaptic plasticity through
            # neuromodulators, so mere exposure to the sonar does not learn.
            c.type("Modulator:LC", **graded(tau_ms=50.0), **common)
            hb["lc"] = c.cell("LC", "Modulator:LC", "hindbrain")
        hb["lt"] = {}
        hb["cs"] = {}
        g = args.learn_groups
        for name, side in (("Sonar/Left", "l"), ("Sonar/Right", "r")):
            lt = c.cell(f"LT_{side}", "Learn:turn", "hindbrain")
            c.connect(lt, rs[side], drive_weight(w_prs, GAIN))
            if args.lt_cross > 0:
                # Contralateral (commissural) inhibition: a learned turn also
                # suppresses the opposite turn command, so with obstacles on
                # both sides the nearer one wins. Without it, both learned
                # turns fired in a corner and a fast-turning body rocked there
                # (24 Nov: 760 s at one spot). RS is not plastic.
                other = "r" if side == "l" else "l"
                c.connect(lt, rs[other], drive_weight(-args.lt_cross * w_prs, GAIN))
            hb["lt"][side] = lt
            cells = []
            for k in range(g):
                d = args.learn_near_m + (args.learn_far_m - args.learn_near_m) * k / max(1, g - 1)
                cell, _, _ = add_sensor_relay(c, GAIN, name, sonar_value(d), tau_ms=30.0,
                                              name=f"CS_{side}_{k:02d}")
                c.connect(cell, lt, drive_weight(args.learn_w0, GAIN))
                cells.append(cell)
            for a in cells:
                for b in cells:
                    if a != b and rnd.random() < args.learn_within_p:
                        c.connect(a, b, drive_weight(args.learn_within_mv, GAIN))
            hb["cs"][side] = cells
        for (a, b) in ((hb["cs"]["l"][0], hb["cs"]["r"][0]), (hb["cs"]["r"][-1], hb["cs"]["l"][-1])):
            c.connect(a, b, drive_weight(args.learn_within_mv, GAIN))
    if args.force_turn:
        # Test drive: a tonic command latches one turn cell (measure turning).
        c.type("Test:force_turn", **graded(tau_ms=20.0, tonic=release_tonic(0.9)), **common)
        f = c.cell("FORCE_TURN", "Test:force_turn", "hindbrain")
        c.connect(f, rs[args.force_turn], drive_weight(w_prs, GAIN))
    if args.yaw_turn > 0:
        # Hip yaw-pitch steering (NAO turns with this one motor): during a
        # turn it alternates with the gait phase, +-yaw_turn. Timing decides
        # it (programmed probe, scripts/qa/nao_turn_probe.py, 24 Nov): in
        # phase with the half-centres the body turned ~1 deg/s, lagged by
        # ~0.075 cycle (150 ms at 0.5 Hz) +-11 deg/s both ways, nearly in
        # place, no falls in 40 s; later lags tipped the body on left turns.
        # Yaw delay cells YD (--yaw-lag-ms) supply the lag; AND cells (lagged
        # phase x turn command) gate the alternation.
        c.type("Gait:yaw_delay", **graded(tau_ms=args.yaw_lag_ms), **common)
        e = {}
        for ph in "AB":
            yd = c.cell(f"YD_{ph}", "Gait:yaw_delay", "spinal")
            c.connect(dcells.get(f"E_{ph}"), yd, drive_weight(12.0, GAIN))
            e[ph] = yd
        c.type("Turn:and", **graded(tau_ms=10.0, tonic=-args.and_threshold), **common)
        and_high = steady_release(12.0 * 0.95 + 12.0 * 0.8 - args.and_threshold)
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
        s_near = sonar_value(args.near_m)
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
            if learn:
                # Innate teaching input: the near reflex turns away too.
                c.connect(near, hb["lt"][side], drive_weight(args.learn_teach_mv, GAIN))
                if args.near_veto > 0:
                    # Near-side priority: an obstacle inside the near reflex
                    # on one side vetoes the turn toward that side (RS_l turns
                    # right, so the right near cell vetoes RS_l), whatever a
                    # learned turn from a farther obstacle on the other side
                    # asks. In a corner the learned turn from the far side
                    # (and its cross-inhibition) fought the near reflex and
                    # the body rocked in place (24 Nov, V6c: 230 s). On RS,
                    # which is not plastic: every synapse onto LT learns.
                    other = "l" if side == "r" else "r"
                    c.connect(near, rs[other], drive_weight(-args.near_veto * w_prs, GAIN))
                if args.near_turn > 0:
                    # Direct innate escape turn (the reflex arc exists whatever
                    # is learned): N_s -> RS_s, strong enough to beat a learned
                    # turn's cross-inhibition. Through LT alone, a learned turn
                    # from a farther obstacle on the other side (a wall at
                    # 0.47 m) cross-inhibited the escape from the near one (the
                    # target ball at 0.3 m) and the body could not turn
                    # (24 Nov, V6d: 175 s).
                    c.connect(near, rs[side], drive_weight(args.near_turn * w_prs, GAIN))
                if "lc" in hb:
                    c.connect(near, hb["lc"], drive_weight(args.learn_teach_mv, GAIN))
        if args.touch > 0:
            # Touch withdrawal (innate, like a nociceptive/tactile withdrawal
            # reflex): the NAO V6's two toe bumpers per foot are ON/OFF
            # switches on the toe shell. A touch on a foot drives a touch cell
            # T of that side, which kicks the back-off bout and the turn away
            # (RS_l turns right), as the near sonar does. It catches what the
            # chest sonar cannot (a low obstacle wedged against the feet).
            # Not a teacher by default: the learned cells are sonar distance
            # cells, and pairing them with touches they cannot predict would
            # teach turns at arbitrary distances.
            for side, names in BUMPERS.items():
                c.type(f"Sense:touch:{side}", **graded(tau_ms=20.0), **common)
                t = c.cell(f"T_{side}", f"Sense:touch:{side}", "sensory")
                for name in names:
                    relay, r0, slope = add_sensor_relay(c, GAIN, name, 0.5, tau_ms=30.0, name=f"R_touch_{name}")
                    # a press (+0.5 above the switch midpoint) gives ~12 mV/ms
                    k = 12.0 / max(slope * 0.5, 1e-3)
                    c.types[f"Sense:touch:{side}"]["tonic_current"] = c.types[f"Sense:touch:{side}"].get(
                        "tonic_current", 0.0) - k * r0
                    c.connect(relay, t, drive_weight(k, GAIN))
                c.connect(t, back, drive_weight(args.touch_back_kick, GAIN))
                c.connect(t, rs[side], drive_weight(args.touch_turn_gain * w_prs, GAIN))
                if learn and args.touch_teaches and "lc" in hb:
                    c.connect(t, hb["lc"], drive_weight(args.learn_teach_mv, GAIN))
                hb.setdefault("touch", {})[side] = t
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
        hb["rev"] = rev
    # RS_l turns right: shorten the right stride (inner), slightly the left.
    for rs_side, inner, outer in (("l", "R", "L"), ("r", "L", "R")):
        for ph in "AB":
            c.connect(rs[rs_side], dcells[inner, ph], drive_weight(-args.turn, GAIN))
            if args.turn_outer > 0:
                c.connect(rs[rs_side], dcells[outer, ph], drive_weight(-args.turn_outer, GAIN))
    if args.backup > 0 and args.reverse_steer > 0:
        # Steering while reversing. Walking backwards, a shorter inner stride
        # turns the body the other way, so every back-off bout undid the turn
        # the forward steps had made and a head-on wall trapped the body
        # (MuJoCo, 23 Nov: 460-750 s rocking at 0.3-0.4 m). Two fixes failed:
        # mirroring the turn during the bout tipped the body over backwards
        # (76-101 s, 3 of 3 runs), and cancelling the turn command's stride
        # inhibition doubled the reverse speed (-12 vs -5.5 cm/s) and tipped it
        # over in the first bout - that inhibition also brakes the reversal
        # the back-off drive was tuned with. So the bout keeps the brake and
        # loses only the asymmetry: an AND cell per side (turn command x
        # reversal level, the Y cells' design) gives the outer stride the
        # inner stride's inhibition while reversing. The body backs slowly and
        # straight; forward steps turn, backward steps no longer undo it.
        # Threshold above either input alone (RS, REV each <= ~0.9 release x 12).
        c.type("Turn:rev_and", **graded(tau_ms=10.0, tonic=-args.rev_and_threshold), **common)
        q_high = steady_release(12.0 * 0.8 + 12.0 * 0.8 - args.rev_and_threshold)
        extra = (args.turn - args.turn_outer) * 0.8 / max(q_high, 1e-3)
        for rs_side, inner, outer in (("l", "R", "L"), ("r", "L", "R")):
            q = c.cell(f"Q_{rs_side}", "Turn:rev_and", "hindbrain")
            c.connect(rs[rs_side], q, drive_weight(12.0, GAIN))
            c.connect(hb["rev"], q, drive_weight(12.0, GAIN))
            for ph in "AB":
                c.connect(q, dcells[outer, ph], drive_weight(-extra, GAIN))
    return hb


def add_auditory_orienting(c, args):
    """Innate auditory orienting (record sensor group "hearing", NAO V6 head
    microphones). Mammalian brainstem layout, same shared cell types as
    everything else:

    * cochlear nucleus: one excitation-only relay per microphone and band,
      centred on ``--hearing-ref`` (normalised level; 0.3 is ~44 dB SPL);
    * lateral superior olive: per band, ``ILD_l`` is excited by the left
      microphones and inhibited by the right ones (interaural level
      difference; larger at high frequencies, the head shadow), ``ILD_r`` the
      mirror;
    * medial superior olive: ``ITD_l`` reads the front and rear interaural
      time differences (above 0.5: the sound reaches the left first);
    * superior colliculus: ``OR_l`` / ``OR_r`` sum both cues and turn the head
      toward the sound (HeadYaw; returned for the output wiring).

    Returns {side: orienting cell} or {} when the body has no hearing group.
    """
    io = json.loads((ROOT / "sim/content/embodiments/generated/nao.json").read_text())["io"]
    names = {s["name"] for s in io["sensors"] if s.get("group") == "hearing"}
    if not names:
        return {}
    common = dict(nmda_ratio=0.0, synaptic_gain=GAIN)
    mics = {"l": ("FL", "RL"), "r": ("FR", "RR")}
    bands = sorted({int(n.split("/")[2][:-2]) for n in names if not n.startswith("Micro/ITD")})
    relays = {}
    for side, pair in mics.items():
        for mic in pair:
            for band in bands:
                cell, r0, slope = add_sensor_relay(c, GAIN, f"Micro/{mic}/{band}Hz", args.hearing_ref, tau_ms=30.0,
                                                   name=f"CN_{mic}_{band}")
                relays[mic, band] = (cell, r0, slope)
    orient = {}
    for side in "lr":
        c.type(f"Auditory:SC:{side}", **graded(tau_ms=60.0), **common)
        orient[side] = c.cell(f"OR_{side}", f"Auditory:SC:{side}", "hindbrain")
    other = {"l": "r", "r": "l"}
    for band in bands:
        for side in "lr":
            c.type(f"Auditory:LSO:{side}:{band}", **graded(tau_ms=20.0), **common)
            ild = c.cell(f"ILD_{side}_{band}", f"Auditory:LSO:{side}:{band}", "hindbrain")
            for mic in mics[side]:
                cell, r0, slope = relays[mic, band]
                k = args.hearing_gain / max(slope * 0.1, 1e-3)
                c.connect(cell, ild, drive_weight(k, GAIN))
            for mic in mics[other[side]]:
                cell, r0, slope = relays[mic, band]
                k = args.hearing_gain / max(slope * 0.1, 1e-3)
                c.connect(cell, ild, drive_weight(-k, GAIN))
            # Head shadow: high bands carry the level cue.
            c.connect(ild, orient[side], drive_weight(args.hearing_gain * min(1.0, band / 1000.0), GAIN))
    for pair in ("Front", "Rear"):
        name = f"Micro/ITD/{pair}"
        if name not in names:
            continue
        # Above 0.5 the sound reached the left microphone first.
        cell, r0, slope = add_sensor_relay(c, GAIN, name, 0.5, tau_ms=30.0, name=f"SOC_{pair}")
        k = args.hearing_gain / max(slope * 0.1, 1e-3)
        for side, sign in (("l", 1.0), ("r", -1.0)):
            c.type(f"Auditory:MSO:{side}:{pair}", **graded(tau_ms=20.0, tonic=-sign * k * r0), **common)
            itd = c.cell(f"ITD_{side}_{pair}", f"Auditory:MSO:{side}:{pair}", "hindbrain")
            c.connect(cell, itd, drive_weight(sign * k, GAIN))
            c.connect(itd, orient[side], drive_weight(args.hearing_gain, GAIN))
    return orient


# Fall recovery (innate righting programs). The torso accelerometer reads
# [-20, 20] m/s2 as [0, 1]; accelerometer.x = -g sin(pitch): lying chest-down
# ~0.255, chest-up ~0.745, upright ~0.5 (walking vibration +-0.125).
ACCEL_RANGE = 20.0
FALL_LEVELS = {"front": 0.40, "back": 0.60}     # accelerometer.x past ~30 deg of pitch either way
FALL_Z_LEVEL = 0.62                             # accelerometer.z: under ~5 m/s2 along the body axis (tilt > ~60 deg)
FALL_TEST = {"front": 0.255, "back": 0.745}     # lying values (calibration input)


def add_fall_detector(c, side, tau_ms=600.0):
    """FALL_<side>: on while the torso lies chest-down (front) or chest-up
    (back). Otolith-like: gravity no longer along the body axis
    (accelerometer.z low) and on the chest or back axis (accelerometer.x),
    through relays in the vestibular nuclei. Lying face-down in the closed
    loop, accelerometer.x read ~0.30 +- 0.06 (contact vibration), not the
    ideal 0.255: one 0.35 threshold on x alone gave a weak, late trigger;
    the z term also keeps walking jolts upright from triggering."""
    common = dict(nmda_ratio=0.0, synaptic_gain=GAIN)
    tonic = 0.0
    c.type(f"Sense:fall_{side}", **graded(tau_ms=tau_ms), **common)
    fall = c.cell(f"FALL_{side}", f"Sense:fall_{side}", "sensory")
    for channel, level, sgn, name in (("accelerometer.x", FALL_LEVELS[side], -1.0 if side == "front" else 1.0,
                                       "R_fall_x" if side == "front" else "R_fall_x_back"),
                                      ("accelerometer.z", FALL_Z_LEVEL, -1.0, "R_fall_z" if side == "front" else "R_fall_z_back")):
        relay, r0, slope = add_sensor_relay(c, GAIN, channel, level, tau_ms=100.0, name=name)
        k = 6.0 / max(slope * 0.05, 1e-3)     # 6 mV/ms 0.05 past the level (2 m/s2)
        c.connect(relay, fall, drive_weight(sgn * k, GAIN))
        tonic -= sgn * k * r0
    c.types[f"Sense:fall_{side}"]["tonic_current"] = tonic - 3.0     # both cues needed
    return fall


def calibrate_sequence(side, name, durations, cache):
    """Adaptation time constants giving ``durations`` (s) for a keyframe
    chain, measured in the runtime (a fall detector and the chain alone),
    cached in ``cache`` by the durations."""
    key = f"{side}:{name}:" + ",".join(f"{d:.3f}" for d in durations)
    store = json.loads(cache.read_text()) if cache.exists() else {}
    if key in store:
        return store[key]["taus_ms"]
    taus = [d / 0.6 * 1000.0 for d in durations]
    taus[-1] = durations[-1] / 1.15 * 1000.0
    for _ in range(4):
        t = Circuit.for_embodiment("nao", exact_cells=True)
        fall = add_fall_detector(t, side)
        seq = add_motor_sequence(t, GAIN, name, taus, fall)
        t.type("Motor:x", **graded(tau_ms=15.0, release_hz=500.0), nmda_ratio=0.0, synaptic_gain=GAIN)
        for o in t.outputs:
            t.output(o, "Motor:x")
        times = measure_sequence(t, seq, "accelerometer.x", FALL_TEST[side], ms=int(1000 * sum(durations)) + 6000)
        got = [off - on for on, off in times]
        if any(g != g for g in got):
            raise SystemExit(f"{name}: keyframe chain stalled during calibration: {times}")
        err = max(abs(g - d) / d for g, d in zip(got, durations))
        print(f"  {name} calibration: worst duration error {100 * err:.1f} %")
        if err < 0.05:
            break
        taus = [tau * d / g for tau, d, g in zip(taus, durations, got)]
    store[key] = dict(taus_ms=taus, measured_s=got)
    cache.write_text(json.dumps(store, indent=1))
    return taus


# Body-state code for learned sequencing: band cells on the gravity sense
# and the main joints' angle sense, (low, high) edges in sensor units per
# band. legacy-angle reads (q + 3.14) / 6.28.
def _ang(q):
    return (q + 3.14) / 6.28


# Gravity on the body axes, measured lying in the closed loop (24 Nov):
# accelerometer.x prone 0.31, upright ~0.41, supine 0.75; accelerometer.y
# on the right side 0.32, centred 0.50-0.56, on the left side 0.70;
# accelerometer.z upright 0.72, lying ~0.49. The first code stopped at
# accelerometer.x 0.54 and had no y: lying supine or on a side, no band was
# on, and nothing could learn what to do there (M3 never rolled, 24 Nov).
STATE_CHANNELS = {
    "accelerometer.x": [0.22, 0.30, 0.38, 0.46, 0.54, 0.62, 0.70, 0.80],
    "accelerometer.y": [0.22, 0.40, 0.60, 0.78],
    "accelerometer.z": [0.45, 0.55, 0.65, 0.80],
    "LKneePitchS": [_ang(q) for q in (-0.1, 0.7, 1.4, 2.15)],
    "LHipPitchS": [_ang(q) for q in (-1.6, -1.0, -0.4, 0.5)],
    "LHipYawPitchS": [_ang(q) for q in (-1.2, -0.5, 0.2, 0.8)],
    "LShoulderPitchS": [_ang(q) for q in (-2.1, -0.7, 0.7, 2.1)],
    "LElbowRollS": [_ang(q) for q in (-1.6, -1.0, -0.5, 0.0)],
    "LAnklePitchS": [_ang(q) for q in (-1.2, -0.5, 0.2, 0.95)],
}


STATE_BAND_INH = 1.373 / 0.9      # balanced band subtraction in the spatial layout


def add_body_state_cells(c):
    """Band cells B_<channel>_<k>: on while the channel lies between two
    edges (relay at the lower edge AND NOT relay at the upper edge; each
    relay is a steep excitation-only cell, ``add_sensor_relay``). Together
    they code where the body is - its orientation to gravity and its joint
    configuration - with no reference to any program. Returns the band cells."""
    common = dict(nmda_ratio=0.0, synaptic_gain=GAIN)
    c.type("State:band", **graded(tau_ms=30.0, tonic=-3.0), **common)
    bands = []
    for ch, edges in STATE_CHANNELS.items():
        short = ch.replace("accelerometer.", "acc").replace("S", "") if ch.endswith("S") else ch.replace("accelerometer.", "acc")
        relays = [add_sensor_relay(c, GAIN, ch, e, tau_ms=50.0, name=f"R_state_{short}_{k}")[0] for k, e in enumerate(edges)]
        for k in range(len(edges) - 1):
            b = c.cell(f"B_{short}_{k}", "State:band", "sensory")
            c.connect(relays[k], b, drive_weight(20.0, GAIN))
            # The relays' edges are soft (0.2-0.4 just below their level), so
            # the subtraction must balance: in the spatial layout (active
            # dendrites) excitation delivers ~1.373x nominal and inhibition
            # ~0.9x (aarnn_circuit EXC_/INH_EFFECTIVE); unscaled, every band
            # whose lower relay was on stayed on (24 Nov).
            c.connect(relays[k + 1], b, drive_weight(-20.0 * STATE_BAND_INH, GAIN))
            bands.append(b)
    return bands


def add_righting_exploration(c, args, rm, lcells, seqs):
    """Righting by exploration and progress-gated learning (learning by
    experience; user, 24 Nov).

    * LYING: gravity off the body's long axis (accelerometer.z low), any
      side - righting mode even on a side, where neither fall detector fires.
    * Progress (innate values, vestibular): P_up while accelerometer.z rises
      (fast relay above a slow relay of the same sense), P_ventral while
      accelerometer.x falls toward chest-down during LYING (the righting
      reflex's goal). Both drive LC, the modulator that gates plasticity:
      band -> L synapses change only when a movement made progress.
    * Exploration: MOVING (any keyframe on); STUCK integrates righting mode
      without movement and without progress, and lifts every L cell; three
      slow escape oscillators with unrelated periods bias the L cells
      differently over time, so a different movement wins each time the body
      is stuck (trial movements).
    """
    common = dict(nmda_ratio=0.0, synaptic_gain=GAIN)

    def relay(channel, level, tau, name, slope_mv=8.0):
        cell, r0, slope = add_sensor_relay(c, GAIN, channel, level, tau_ms=tau, slope_mv=slope_mv, name=name)
        return cell, r0, slope
    # Lying (any side).
    rz, r0, slope = relay("accelerometer.z", FALL_Z_LEVEL, 100.0, "R_lying_z", slope_mv=1.5)
    k = 6.0 / max(slope * 0.05, 1e-3)
    c.type("Sense:lying", **graded(tau_ms=600.0, tonic=k * r0), **common)
    lying = c.cell("LYING", "Sense:lying", "sensory")
    c.connect(rz, lying, drive_weight(-k, GAIN))
    c.connect(lying, rm, drive_weight(12.0, GAIN))
    # Progress as band transitions (no subtraction of equal signals: a fast
    # minus slow relay of one sense saturated, as excitation and inhibition
    # are not symmetric and spatial attenuation adds to the mismatch). A slow
    # trace S of each gravity band; a transition cell fires when the next
    # band toward the goal comes on while the trace of the band it left is
    # still up. Upright: accelerometer.z bands rising. Chest-down while
    # lying: accelerometer.x bands falling.
    c.type("Prog:trace", **graded(tau_ms=args.getup_progress_trace_ms), **common)
    # A transition needs both the new band (~1) and the old band's trace
    # (~0.5-0.9): threshold 1.3 x one input.
    g = args.getup_progress_gain
    c.type("Prog:transition", **graded(tau_ms=50.0, tonic=-1.3 * g), **common)
    c.type("Prog:transition3", **graded(tau_ms=50.0, tonic=-2.3 * g), **common)
    c.type("Prog:out", **graded(tau_ms=50.0, tonic=-2.0), **common)
    p_up = c.cell("P_up", "Prog:out", "sensory")
    p_v = c.cell("P_ventral", "Prog:out", "sensory")

    def band_names(prefix):
        return sorted((n for n, *_ in c.cells if n.startswith(prefix)), key=lambda n: int(n.rsplit("_", 1)[1]))
    # Binarised copies of the gravity bands (the graded bands, ~0.4-0.7,
    # are context for learning; for a clean AND the progress path needs
    # all-or-none band signals).
    # The binarised band integrates over --getup-progress-hold-ms, so a band
    # counts only once the body has stayed in it: with 20 ms, the body's own
    # shaking flickered bands and P_up fired with every exploratory movement
    # (LC on in every fall, M3, 24 Nov).
    # A band counts only when it dominates (release > --getup-progress-band-min):
    # the relays' edges are soft, so lying at accelerometer.z 0.50 the next band
    # up sat at ~0.3 release and a 0.1 threshold binarised it on - P_up fired
    # ~8 Hz the whole time the body lay still (M4 replay, 24 Nov).
    c.type("Prog:bin", **graded(tau_ms=args.getup_progress_hold_ms, tonic=-60.0 * args.getup_progress_band_min),
           **common)
    for prefix, out, rising in (("B_accz_", p_up, True), ("B_accx_", p_v, False)):
        bands = band_names(prefix)
        binary, traces = {}, {}
        for b in bands:
            binary[b] = c.cell("BB" + b[1:], "Prog:bin", "sensory")
            c.connect(b, binary[b], drive_weight(60.0, GAIN))
            traces[b] = c.cell("TR" + b[1:], "Prog:trace", "sensory")
            # Strong drive: the trace's membrane sits far above its release
            # point and holds ~1-2 s after the band switches off, so a body
            # passing quickly through a band still counts as having been there.
            c.connect(binary[b], traces[b], drive_weight(40.0, GAIN))
        for k in range(len(bands) - 1):
            came_from, goes_to = (bands[k], bands[k + 1]) if rising else (bands[k + 1], bands[k])
            t = c.cell(f"PT_{prefix[2:-1]}_{k}", "Prog:transition" if rising else "Prog:transition3", "sensory")
            c.connect(binary[goes_to], t, drive_weight(args.getup_progress_gain, GAIN))
            c.connect(traces[came_from], t, drive_weight(args.getup_progress_gain, GAIN))
            if not rising:     # and lying: three inputs must coincide
                c.connect(lying, t, drive_weight(args.getup_progress_gain, GAIN))
            c.connect(t, out, drive_weight(12.0, GAIN))
    for pc in (p_up, p_v):
        c.connect(pc, "LC", drive_weight(12.0, GAIN))
    # Movement and stuck.
    c.type("Explore:moving", **graded(tau_ms=100.0, tonic=-3.0), **common)
    moving = c.cell("MOVING", "Explore:moving", "hindbrain")
    for seq in seqs.values():
        # From the motor relays M (silent at rest): the idle leak of 20
        # keyframe cells held MOVING on and STUCK never fired (24 Nov).
        for mc in seq["M"]:
            c.connect(mc, moving, drive_weight(12.0, GAIN))
    c.type("Explore:stuck", **graded(tau_ms=args.getup_stuck_ms, tonic=-6.0), **common)
    stuck = c.cell("STUCK", "Explore:stuck", "hindbrain")
    c.connect(rm, stuck, drive_weight(12.0, GAIN))
    c.connect(moving, stuck, drive_weight(-24.0, GAIN))
    c.connect(p_up, stuck, drive_weight(-12.0, GAIN))
    # One program at a time: every running program's gate cuts every
    # program's start pulse, and while anything moves no entry cell can
    # newly win (the latched one sits on its plateau's upper branch and
    # holds). Without these a new choice started a second program on top of
    # a running one (24 Nov).
    starts = [n for n, *_ in c.cells if n.startswith(("ST_getup_", "ST_roll_", "ST_rollm_"))]
    for seq in seqs.values():
        for stc in starts:
            c.connect(seq["G"], stc, drive_weight(-30.0, GAIN))
    for lc in lcells:
        c.connect(moving, lc, drive_weight(-args.getup_busy, GAIN))
    for lc in lcells:
        # Righting mode holds each entry cell inside its bistable band (off
        # until kicked, then latched); a choice is made when the body is
        # stuck (STUCK kick + oscillator bias) or when the learned body
        # state alone lifts an entry past its upper knee.
        c.connect(rm, lc, drive_weight(args.getup_entry_rest + 1.0, GAIN))
        c.connect(stuck, lc, drive_weight(args.getup_explore_kick, GAIN))
    # Pseudo-random bias: three escape half-centre oscillators.
    import random
    rnd = random.Random(args.getup_explore_seed)
    halves = []
    for j, tau in enumerate((1700.0, 2900.0, 4300.0)):
        c.type(f"Explore:E{j}", **graded(tau_ms=50.0, plateau_mv=30.0, tonic=4.0), **common)
        c.type(f"Explore:E{j}_lead", **graded(tau_ms=50.0, plateau_mv=30.0, tonic=4.3), **common)
        c.type(f"Explore:A{j}", **graded(tau_ms=tau), **common)
        e = [c.cell(f"EXO_{j}_{h}", f"Explore:E{j}_lead" if h == "a" else f"Explore:E{j}", "hindbrain") for h in "ab"]
        a = [c.cell(f"EXO_A_{j}_{h}", f"Explore:A{j}", "hindbrain") for h in "ab"]
        for h in (0, 1):
            c.connect(e[h], a[h], drive_weight(10.0, GAIN))
            c.connect(a[h], e[h], drive_weight(-9.0, GAIN))
            c.connect(e[h], e[1 - h], drive_weight(-4.0, GAIN))
        halves.append(e)
    for lc in lcells:
        for e in halves:
            c.connect(e[rnd.randrange(2)], lc, drive_weight(args.getup_explore_bias * rnd.random(), GAIN))


def add_getup(c, args, acts):
    """Innate righting: lying chest-up rolls onto the front (``roll``),
    lying chest-down runs the get-up (``getup``: hands and knees, crouch over
    the feet, squat, rise). Programs found on the MuJoCo body
    (scripts/qa/nao_getup_probe.py --staged); each is a keyframe-cell chain
    (``add_motor_sequence``) whose cells drive the joints' posture cells.
    Returns ([(K cell, {joint: target})], [gate cells])."""
    front = json.loads(Path(args.getup).read_text())["program"]
    lo = {n: a["control_range"][0] for n, a in acts.items()}
    hi = {n: a["control_range"][1] for n, a in acts.items()}
    posture = {n: min(max(posture_angle(n), lo[n]), hi[n]) for n in acts}
    cache = Path(args.getup).with_suffix(".taus.json")
    plans = [("front", "getup", getup_sequence(front, posture, lo, hi))]
    if args.getup_back:
        back = json.loads(Path(args.getup_back).read_text())
        roll = back["program"][:back["roll_frames"]]
        frames = getup_sequence(roll, posture, lo, hi)[:len(roll)]     # the roll only; the front program follows
        plans.append(("back", "roll", frames))
        if args.getup_explore:
            # The roll's mirror image (left and right swapped): rolling the
            # other way, so a body on either side can roll onto its front.
            mroll = [[f[0]] + list(f[11:21]) + list(f[1:11]) for f in roll]
            plans.append(("none", "rollm", getup_sequence(mroll, posture, lo, hi)[:len(mroll)]))
    drives, gates, triggers, falls = [], {}, {}, []
    bands = add_body_state_cells(c) if args.getup_learn else []
    if bands and "LC" not in c.index:
        c.type("Modulator:LC", **graded(tau_ms=50.0), nmda_ratio=0.0, synaptic_gain=GAIN)
        c.cell("LC", "Modulator:LC", "hindbrain")
    seqs = {}
    for side, name, frames in plans:
        if side == "none":
            # No innate trigger: this program is entered only through its
            # learning cells (selection or exploration).
            if "NO_TRIGGER" not in c.index:
                c.type("Seq:none", **graded(tau_ms=20.0, tonic=-10.0), nmda_ratio=0.0, synaptic_gain=GAIN)
                c.cell("NO_TRIGGER", "Seq:none", "hindbrain")
            fall = "NO_TRIGGER"
            taus = calibrate_sequence("back", name, [d for d, _ in frames], cache)
        else:
            fall = add_fall_detector(c, side)
            taus = calibrate_sequence(side, name, [d for d, _ in frames], cache)
            falls.append(fall)
        trigger = fall
        if args.getup_explore and side != "none":
            # Exploring, every start goes through the exclusive entry choice:
            # the innate rule (chest-down -> get-up, chest-up -> roll) becomes
            # a preference onto that program's first entry cell (below), not a
            # direct trigger that could start a program on top of another.
            if "NO_TRIGGER" not in c.index:
                c.type("Seq:none", **graded(tau_ms=20.0, tonic=-10.0), nmda_ratio=0.0, synaptic_gain=GAIN)
                c.cell("NO_TRIGGER", "Seq:none", "hindbrain")
            trigger = "NO_TRIGGER"
        seq = add_motor_sequence(c, GAIN, name, taus, trigger)
        seq["fall"] = fall if side != "none" else None
        seqs[name] = seq
        drives += list(zip(seq["M"], [t for _, t in frames]))
        gates[name], triggers[name] = seq["G"], seq["T"]
        if bands:
            # Learned sequencing (learning by experience): a learning cell L_i
            # per movement after the first starts that movement; its silent
            # synapses from the body-state band cells are the only plastic
            # ones. The timed chain is the teacher: its handover cell X_{i-1}
            # drives L_i and the LC modulatory cell (which gates plasticity),
            # so L_i learns the body state in which movement i began. With
            # the teacher silenced (X tonic off), movements follow the body.
            c.type("Learn:seq", **graded(tau_ms=20.0, tonic=-args.getup_learn_threshold), nmda_ratio=0.0,
                   synaptic_gain=GAIN)
            if args.getup_explore:
                # Exploring: learning cells only at the program's entry points;
                # once entered, a program runs through on its own handovers.
                # One L per entry, latched for an eligibility window (22 mV
                # plateau, ended by its own adaptation LA, --getup-eligibility-ms)
                # so progress anywhere in that window - including the rise at
                # the end of a program - pairs with the choice that led to it.
                # L only starts its program (a ~300 ms start pulse), never
                # holds a keyframe: L cells holding keyframes latched two
                # movements on together and the body froze (24 Nov).
                common = dict(nmda_ratio=0.0, synaptic_gain=GAIN)
                if "Learn:entry" not in c.types:
                    c.type("Learn:entry", **graded(tau_ms=30.0, plateau_mv=22.0, tonic=-args.getup_entry_rest), **common)
                    c.type("Learn:entry_adapt", **graded(tau_ms=args.getup_eligibility_ms), **common)
                    # A start needs the entry to have won (release above
                    # --getup-start-min): at tonic -2 a losing entry idling at
                    # ~0.2 release (leak, oscillator bias) started its program
                    # whenever the running one ended, so the front get-up
                    # re-entered at keyframe 4 every ~11 s and the rolls that
                    # had won never ran (M4 closed loop, 24 Nov).
                    # Drive 24 mV/ms at full entry release against a threshold
                    # at --getup-start-min: an idling entry (~0.3) gets nothing,
                    # a winner (~0.9) a pulse as strong as the old 12 - 2
                    # (with 12 x release - 7.2 the pulse peaked at 0.2 release
                    # and started nothing).
                    c.type("Learn:start", **graded(tau_ms=10.0, tonic=-24.0 * args.getup_start_min), **common)
                    c.type("Learn:start_adapt", **graded(tau_ms=300.0), **common)
                entries = {"getup": args.getup_entries_front, "roll": [0], "rollm": [0]}[name]
                for i in entries:
                    lcell = c.cell(f"L_{name}_{i:02d}", "Learn:entry", "spinal")
                    la = c.cell(f"LA_{name}_{i:02d}", "Learn:entry_adapt", "spinal")
                    c.connect(lcell, la, drive_weight(10.0, GAIN))
                    c.connect(la, lcell, drive_weight(-args.getup_entry_adapt, GAIN))
                    st = c.cell("ST" + lcell[1:], "Learn:start", "spinal")
                    sa = c.cell("SA" + lcell[1:], "Learn:start_adapt", "spinal")
                    c.connect(lcell, st, drive_weight(24.0, GAIN))
                    c.connect(st, sa, drive_weight(10.0, GAIN))
                    c.connect(sa, st, drive_weight(-30.0, GAIN))
                    c.connect(st, seq["K"][i], drive_weight(4.0, GAIN))
                    # A true one-shot: the program's gate (on while it runs and
                    # ~1.5 s after) and the entry keyframe's motor relay cut the
                    # pulse. With its own adaptation alone, a pulse fed by a
                    # latched L settled part-on and held keyframe 0 on (24 Nov).
                    c.connect(seq["G"], st, drive_weight(-30.0, GAIN))
                    c.connect(seq["M"][i], st, drive_weight(-30.0, GAIN))
                    for b in bands:
                        c.connect(b, lcell, drive_weight(args.getup_learn_w0, GAIN))
                    if i == entries[0] and seq.get("fall"):
                        c.connect(seq["fall"], lcell, drive_weight(args.getup_innate_prior, GAIN))
                continue
            for i in range(1, len(seq["K"])):
                lcell = c.cell(f"L_{name}_{i:02d}", "Learn:seq", "spinal")
                c.connect(lcell, seq["K"][i], drive_weight(4.0, GAIN))
                # X releases ~0.5-0.6 at a handover: teach with twice the margin.
                c.connect(seq["X"][i - 1], lcell, drive_weight(2.0 * (args.getup_learn_threshold + 8.0), GAIN))
                c.connect(seq["X"][i - 1], "LC", drive_weight(12.0, GAIN))
                for b in bands:
                    c.connect(b, lcell, drive_weight(args.getup_learn_w0, GAIN))
    if bands and args.getup_explore and args.getup_innate_veto > 0 and "roll" in seqs:
        # Innate righting is keyed to the gravity sense (labyrinthine
        # righting): lying on the back vetoes the front get-up's entries and
        # lying face-down vetoes the rolls'. Exploration and learning choose
        # where neither detector is clearly on (on a side, propped on a
        # fixture). With only the 0.8 preference, the 10 s front program held
        # the body most of the time while supine and the rolls that won were
        # cut by its gate (M4 closed loop, 24 Nov).
        for n, *_ in list(c.cells):
            if n.startswith("L_getup_"):
                c.connect("FALL_back", n, drive_weight(-args.getup_innate_veto, GAIN))
            elif n.startswith(("L_roll_", "L_rollm_")):
                c.connect("FALL_front", n, drive_weight(-args.getup_innate_veto, GAIN))
    if bands and args.getup_select:
        # Competitive selection: the plasticity normalises each learning
        # cell's inputs (it learns which body state, not how strongly), so L
        # cells do not fire on an absolute threshold. A righting-mode cell RM
        # (on while lying or while a program runs) brings every L near its
        # threshold, and lateral inhibition between L cells lets the one
        # whose learned state best matches the body win and start its
        # movement (competitive learning).
        common = dict(nmda_ratio=0.0, synaptic_gain=GAIN)
        c.type("Learn:mode", **graded(tau_ms=100.0, tonic=-3.0), **common)
        rm = c.cell("RM", "Learn:mode", "hindbrain")
        for cell in falls + list(gates.values()):
            c.connect(cell, rm, drive_weight(12.0, GAIN))
        lcells = [n for n, *_ in c.cells if n.startswith(("L_getup_", "L_roll_", "L_rollm_"))]
        if args.getup_explore:
            add_righting_exploration(c, args, rm, lcells, seqs)
        # Lateral inhibition through a thresholded relay per learning cell
        # (silent at rest): from the L cells themselves, the idle leak of 19
        # others held every L below threshold and nothing started (24 Nov).
        c.type("Learn:relay", **graded(tau_ms=10.0, tonic=-8.0), **common)
        relay = {}
        for lc in lcells:
            relay[lc] = c.cell("LR" + lc[1:], "Learn:relay", "spinal")
            c.connect(lc, relay[lc], drive_weight(60.0, GAIN))
        for lc in lcells:
            if not args.getup_explore:
                c.connect(rm, lc, drive_weight(args.getup_learn_threshold - args.getup_select_margin, GAIN))
            for other in lcells:
                if other != lc:
                    c.connect(relay[other], lc, drive_weight(-args.getup_select_inhibition, GAIN))
    # A running program blocks the other's trigger (the roll passes through
    # chest-down before it ends; the get-up waits for the roll's gate).
    if "roll" in gates:
        c.connect(gates["roll"], triggers["getup"], drive_weight(-20.0, GAIN))
        c.connect(gates["getup"], triggers["roll"], drive_weight(-20.0, GAIN))
    if "rollm" in gates:
        c.connect(gates["rollm"], triggers["getup"], drive_weight(-20.0, GAIN))
        c.connect(gates["rollm"], triggers["roll"], drive_weight(-20.0, GAIN))
    return drives, list(gates.values()) + falls


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
    orient = add_auditory_orienting(c, args) if args.hearing else {}
    if args.avoid:
        dcells["E_A"], dcells["E_B"] = ea, eb
        hb = add_sonar_avoidance(c, args, dcells)
        del dcells["E_A"], dcells["E_B"]
    getup_drives, getup_gates = add_getup(c, args, acts) if args.getup else ([], [])
    curve = measure_drive_curve("nao", tau_ms=args.out_tau) if getup_drives else None
    for g in getup_gates:
        # A body lying down does not walk, and a running righting program
        # owns the body: the fall detectors and the program gates silence
        # the walk command, both half-centres (the stride, yaw and reversal
        # cells follow them) and the back-off bout. Lying face-down, the
        # chest sonar sees the floor and latched the back-off, and the gait
        # started before the program (24 Nov, first closed-loop test).
        for cell in (walk, ea, eb):
            c.connect(g, cell, drive_weight(-20.0, GAIN))
        if "BACK" in c.index:     # a 30 mV plateau latch: hold it off harder
            c.connect(g, "BACK", drive_weight(-40.0, GAIN))
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
        if orient and o == "HeadYaw":
            # Orienting: the head turns toward the louder / earlier side
            # (+yaw = left), at most --hearing-yaw rad.
            high = steady_release(args.hearing_gain * 2.0)
            for side, delta in (("l", args.hearing_yaw), ("r", -args.hearing_yaw)):
                c.connect(orient[side], o, drive_weight((release_tonic(r(q0 + delta)) - tonic) / max(high, 1e-3), GAIN))
        for m_cell, targets in getup_drives:
            if o in targets and abs(targets[o] - q0) > 1e-6:
                c.connect(m_cell, o, drive_weight(posture_drive(curve, r(q0), r(targets[o])), GAIN))
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
    ap.add_argument("--onset-m", type=float, default=0.7,
                    help="sonar distance at which proximity starts (m; within the V6 sonar's 0.2-0.8 m)")
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
    ap.add_argument("--reverse-steer", type=int, default=0,
                    help="1: back off straight and slow (outer stride gets the inner stride's turn inhibition while reversing) so backing no longer undoes the turn")
    ap.add_argument("--rev-and-threshold", type=float, default=11.5, help="reverse-steer AND cell threshold (mV/ms)")
    ap.add_argument("--yaw-turn", type=float, default=0.15,
                    help="hip yaw-pitch turn alternation (+-rad); 0 = stride steering only (~0.4 deg/s)")
    ap.add_argument("--yaw-lag-ms", type=float, default=400.0,
                    help="yaw delay cell tau (ms): the alternation's lag behind the phase (closed loop, 50 s forced turns: 150 ms +3.8 / -7.2 deg/s, 300 ms +5.8 / -9.9, 400 ms +6.6, 500 ms +7.1; no falls)")
    ap.add_argument("--force-turn", choices=("l", "r"), default=None,
                    help="test: latch RS_l (turn right) or RS_r (turn left) to measure the turn rate")
    ap.add_argument("--and-threshold", type=float, default=13.0)
    ap.add_argument("--learn-groups", type=int, default=0,
                    help="learning by experience: cells per distance-tuned sonar group (0 = innate far-sonar turn)")
    ap.add_argument("--learn-near-m", type=float, default=0.45, help="nearest tuned distance of a group (m)")
    ap.add_argument("--learn-far-m", type=float, default=0.75,
                    help="farthest tuned distance of a group (m; the V6 sonar reads nothing beyond 0.8 m)")
    ap.add_argument("--learn-w0", type=float, default=0.05, help="initial (silent) sonar-group -> LT drive (mV/ms)")
    ap.add_argument("--learn-teach-mv", type=float, default=12.0, help="innate near reflex -> LT drive (mV/ms)")
    ap.add_argument("--getup", help="front get-up program (JSON, scripts/qa/nao_getup_probe.py) played by a "
                                    "keyframe-cell chain when the torso lies chest-down")
    ap.add_argument("--getup-back", help="back get-up program (JSON with roll_frames): its roll is played when the "
                                         "torso lies chest-up; the front get-up follows")
    ap.add_argument("--getup-learn", type=int, default=0,
                    help="1: body-state band cells and learning cells L (plastic) that learn when to start each "
                         "righting movement from the timed chain (teacher)")
    ap.add_argument("--getup-learn-w0", type=float, default=0.1, help="initial band -> L drive (mV/ms; 0.3 lifted latched entry cells past their knee on their own)")
    ap.add_argument("--getup-explore", type=int, default=0,
                    help="1: exploration + progress-gated learning (mirrored roll, lying sense, progress -> LC, "
                         "stuck-driven trial movements); needs --getup-learn 1 --getup-back")
    ap.add_argument("--getup-entries-front", type=lambda v: [int(x) for x in v.split(",")], default=[0, 4, 6],
                    help="entry keyframes of the front get-up when exploring (start, crouch, squat)")
    ap.add_argument("--getup-entry-rest", type=float, default=1.0,
                    help="entry cell tonic inhibition; righting mode adds rest + 1 (inside the 22 mV band)")
    ap.add_argument("--getup-entry-adapt", type=float, default=20.0,
                    help="LA -| L: ends the eligibility window (mV/ms; 4-8 left an entry latched for good in the spatial layout, 24 Nov)")
    ap.add_argument("--getup-innate-prior", type=float, default=0.8,
                    help="fall detector -> its program's first entry cell (the innate preference, mV/ms)")
    ap.add_argument("--getup-busy", type=float, default=3.0,
                    help="MOVING -| every entry cell: no new choice while moving; the latched one holds (mV/ms)")
    ap.add_argument("--getup-eligibility-ms", type=float, default=5000.0, help="entry adaptation tau (ms)")
    ap.add_argument("--getup-progress-gain", type=float, default=10.0, help="band / trace -> transition drive (mV/ms)")
    ap.add_argument("--getup-progress-trace-ms", type=float, default=1500.0, help="band trace (ms): how recent a band counts")
    ap.add_argument("--getup-innate-veto", type=float, default=6.0,
                    help="lying on the back inhibits the front get-up's entries, face-down the rolls' (mV/ms; 0 = off)")
    ap.add_argument("--getup-start-min", type=float, default=0.45,
                    help="entry-cell release needed for its one-shot start pulse (exploring)")
    ap.add_argument("--getup-progress-band-min", type=float, default=0.6,
                    help="release a gravity band needs to count as the body's band (progress path)")
    ap.add_argument("--getup-progress-hold-ms", type=float, default=300.0,
                    help="binarised gravity band time constant (ms): how long the body must stay in a band for progress")
    ap.add_argument("--getup-stuck-ms", type=float, default=1500.0, help="STUCK integration (ms)")
    ap.add_argument("--getup-explore-kick", type=float, default=1.0, help="STUCK -> every L (mV/ms): with the bias, past the upper knee")
    ap.add_argument("--getup-explore-bias", type=float, default=1.0, help="max oscillator bias onto an L (mV/ms)")
    ap.add_argument("--getup-explore-seed", type=int, default=5)
    ap.add_argument("--getup-select", type=int, default=1,
                    help="1: righting-mode enable + lateral inhibition between learning cells (competitive selection)")
    ap.add_argument("--getup-select-margin", type=float, default=2.0,
                    help="how far below its threshold the righting mode holds each L cell (mV/ms)")
    ap.add_argument("--getup-select-inhibition", type=float, default=40.0,
                    help="L -> other L inhibition via the L relays (mV/ms; 6 let two choices latch together in the spatial layout)")
    ap.add_argument("--getup-learn-threshold", type=float, default=8.0, help="L tonic inhibition (mV/ms)")
    ap.add_argument("--near-turn", type=float, default=2.0,
                    help="near sonar cell -> own-side turn command (direct innate escape), in units of the LT -> RS drive")
    ap.add_argument("--near-veto", type=float, default=2.0,
                    help="near sonar cell -> opposite turn command (the turn toward it) inhibition, in units of the LT -> RS drive")
    ap.add_argument("--lt-cross", type=float, default=1.0,
                    help="learned turn -> opposite turn command inhibition, in units of the LT -> RS drive")
    ap.add_argument("--learn-teacher", type=int, default=1,
                    help="1: the near reflex drives a modulatory locus coeruleus cell LC (teacher-gated learning)")
    ap.add_argument("--learn-within-p", type=float, default=0.5, help="within-group connection probability")
    ap.add_argument("--learn-within-mv", type=float, default=0.3, help="within-group synapse drive (mV/ms)")
    ap.add_argument("--touch", type=int, default=1, help="toe-bumper touch withdrawal reflex (1 on, 0 off; needs --backup 1)")
    ap.add_argument("--touch-back-kick", type=float, default=8.0, help="touch cell drive onto BACK (mV/ms)")
    ap.add_argument("--touch-turn-gain", type=float, default=3.0,
                    help="touch cell -> turn cell drive, in units of the sonar proximity drive (3: smallest that latches RS through the spinal-brainstem path at --spatial 1)")
    ap.add_argument("--touch-teaches", type=int, default=0, help="1: touch also drives the LC teacher")
    ap.add_argument("--hearing", type=int, default=1,
                    help="auditory orienting from the V6 head microphones when the body record has the hearing group (1 on)")
    ap.add_argument("--hearing-ref", type=float, default=0.3, help="cochlear-nucleus relay centre (normalised level; 0.3 ~ 44 dB SPL)")
    ap.add_argument("--hearing-gain", type=float, default=6.0, help="auditory drive per 0.1 level difference (mV/ms)")
    ap.add_argument("--hearing-yaw", type=float, default=0.5, help="head yaw toward a lateral sound (rad)")
    ap.add_argument("--solve", action="store_true", help="solve each pair against the idle cell's residual")
    ap.add_argument("--trim", type=float, default=0.6, help="stride trim: right x (1+trim), left x (1-trim); 0.6 walks ~straight (0.5: -60 deg, 0.7: +38 deg in 40 s)")
    args = ap.parse_args()
    c = build(args)
    print(f"wrote {args.output}: {len(c.cells)} cells, {len(c.syn)} synapses, {len(c.sense)} sensory projections")


if __name__ == "__main__":
    main()
