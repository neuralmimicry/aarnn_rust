#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Carrion crow flight circuit (``crow_corvus_corone``, flight mechanics v2).

Same layered recipe as the worm, fish, fly and archetype circuits
(``aarnn_circuit``): graded cells, a bistable half-centre oscillator, reflexes
built from rectified error cells, a tonic behavioural command and a
pulse-density motor code (``antagonist-rate-v1``: torque = gear x
(density_positive - density_negative), low-passed 20 ms by the transducer).

* Wingbeat CPG (``add_leg_oscillators`` with one "limb", the wing pair):
  ``E_wing_st`` is the downstroke (power stroke), ``E_wing_sw`` the upstroke
  (recovery), each terminated by its own adaptation (``A``) and crossed by
  inhibitory ``C`` cells; extra command drive to the downstroke gives ~4 Hz
  with a ~60 % downstroke. Both wings share it: bilateral motor pools receive
  the same burst, as the avian flight CPG drives both pectorals in phase
  (per-wing angle servos, tried first, were unstable in the antisymmetric
  wing-body mode at this latency). The shoulder is series-elastic, tuned near
  the wingbeat, so the bursts supply aerodynamic work.
* Stroke-end proprioception (``BOT``/``TOP``, shoulder angles of both wings):
  the wing passing the bottom ends the downstroke, the top the upstroke,
  bounding the stroke to about +-0.6 rad.
* Launch: the crow is released in level flight (scene ``launch``). ``LAUNCH``
  holds the wingbeat and the exploratory bank off for ~0.5 s while the
  sensory relays reach their operating point (a glide on spread wings); a
  slow integrator of the flight command (``LAUNCH_T``) then silences it.
* Wing angle of attack: a tonic nose-up humerus twist (~0.3 rad, set by
  torque against the joint's passive stiffness), slightly less on the
  upstroke, modulated by the reflexes below.
* Vestibulo-motor reflexes (head gravity direction = otolith, head gyro =
  canal, airspeed): each sensor drives a relay cell ``V_*`` and push-pull
  error pairs compare relays with a reference relay ``V_ref`` (see build):
  - roll ``R_p``/``R_n``: wings level by differential twist (right wing down
    raises the right wing's angle of attack);
  - pitch ``P_p``/``P_n``: tail elevation from head pitch and pitch rate (a
    proportional-derivative tail reflex with a nose-down set point);
  - flight path ``G_p``/``G_n``: pitch minus body angle of attack adds lift
    when descending;
  - airspeed ``S_fast_p`` weakens the downstroke above 11.5 m/s,
    ``S_slow_p`` lowers the tail below 9 m/s (trades height for speed).
* Postural stretch reflexes (``add_stretch_reflex``) hold neck, beak and
  legs; structural wing joints, tail yaw and fan are held by their passive
  stiffness (the brain leaves their channels silent).
* Hindbrain (``add_hindbrain``): ``MLR`` is the flight command; bistable
  ``RS_l``/``RS_r`` bank away from a side whose lower visual field darkens
  (``D_l``/``D_r``: the dark tree line replacing the bright ground);
  ``ARTR`` alternates a gentle bank (exploratory arcs). Bank commands bias
  the roll reflex's set point.
* Respiration is driven by the downstroke (wingbeat-coupled breathing),
  which raises the body's muscular efficiency in the energy model.

Sensor encodings (crow projection, ``adapters/mujoco/src/world.rs``): joint
angle 0.5 + q/3, joint velocity 0.5 + w/80, head gyro 0.5 + w/80, head
gravity direction 0.5 + g/2, airspeed (torso frame) 0.5 + v/40. The crow is
released in level flight at 10.5 m/s (scene ``launch``); the circuit starts
in the flight state.

  python3 scripts/build_crow_network_json.py
"""
import argparse
import json
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aarnn_circuit import (Circuit, add_hindbrain, add_leg_oscillators, add_stretch_reflex,  # noqa: E402
                           drive_weight, graded, release_tonic, sensor_weight)

ROOT = Path(__file__).resolve().parents[1]
PROFILE = "crow_corvus_corone"
# Measured in the runtime (open-loop probe): a graded inhibitory synapse
# alone delivers 1.0x its nominal current, but against an equal excitatory
# one (the relay - V_ref cancellations below) only ~0.72x: +k -k left
# +0.28 k mV/ms for k = 5..50. Cancelling pairs are scaled for that.
INH_EXACT, INH_GRADED = 0.9, 0.72
GAIN = 100.0


def build(args):
    c = Circuit.for_embodiment(PROFILE)
    sensors = c.sensory

    def w(mv):
        return drive_weight(mv, GAIN)

    def ws(mv, exact=False):
        return sensor_weight(mv, GAIN, exact=exact)
    common = dict(nmda_ratio=0.0, synaptic_gain=GAIN)

    # --- Motor pools ---
    c.type("Motor:push_pull", **graded(tau_ms=args.tau_m, release_hz=500.0), **common)
    # Nose-up twist channels carry the tonic angle of attack.
    c.type("Motor:twist", **graded(tau_ms=args.tau_m, release_hz=500.0, tonic=release_tonic(args.twist)), **common)
    c.type("Motor:tail", **graded(tau_ms=args.tau_m, release_hz=500.0, tonic=release_tonic(args.tail_bias)), **common)
    c.type("Motor:rest", **graded(tau_ms=args.tau_m, release_hz=500.0), **common)
    twist_up = {s: f"{s}_wing_humerus_twist_negative" for s in ("left", "right")}
    for o in c.outputs:
        if o in twist_up.values():
            c.output(o, "Motor:twist")
        elif o == "tail_pitch_negative":
            c.output(o, "Motor:tail")
        else:
            c.output(o, "Motor:push_pull")

    # --- Hindbrain: flight command, turn decision, exploration ---
    hb = add_hindbrain(c, GAIN, mlr_release=args.mlr_release, rs_plateau_mv=args.rs_plateau,
                       artr_tau_a_ms=args.artr_tau_a, pos=(0.08, 0.0, 0.03))
    mlr, rs, artr = hb["mlr"], hb["rs"], hb["artr"]

    # --- Wingbeat CPG: one half-centre for both wings ---
    E, A, _ = add_leg_oscillators(c, GAIN, ("wing",), [], mlr, args.cpg_drive, drive_release=args.mlr_release,
                                  tau_e_ms=args.tau_e, tau_a_swing_ms=args.tau_up, tau_a_stance_ms=args.tau_down,
                                  w_ea=args.w_ea, w_ae=args.w_ae, w_ec=args.w_ec, w_ce=args.w_ce,
                                  positions={"wing": (0.0, 0.0)})
    down, up = E["wing", "st"], E["wing", "sw"]
    # Extra command drive to the downstroke: it lasts longer (more
    # adaptation to end it) and escapes sooner, a ~60 % downstroke as in
    # flapping birds; equal adaptation taus alone give equal phases.
    c.connect(mlr, down, w(args.down_drive / args.mlr_release))
    # Left flap + raises the left wing; right flap + lowers the right wing.
    c.connect(down, "left_wing_shoulder_flap_negative", w(args.flap_down))
    c.connect(down, "right_wing_shoulder_flap_positive", w(args.flap_down))
    c.connect(up, "left_wing_shoulder_flap_positive", w(args.flap_up))
    c.connect(up, "right_wing_shoulder_flap_negative", w(args.flap_up))
    # Slightly less nose-up twist on the upstroke (supination unloads it).
    for s in ("left", "right"):
        c.connect(up, twist_up[s], w(-args.twist_upstroke))
    c.connect(down, "respiration_drive", w(args.breath))
    # Launch glide: sensor-driven cells take ~0.3 s to reach their operating
    # point and the first downstroke (adaptation starting from rest) is twice
    # as long as the rest, which pitched the released crow into a dive. A
    # launch cell holds the wingbeat off while the reflexes settle (the crow
    # glides on spread wings); a slow integrator of the flight command
    # silences it for the rest of the flight.
    c.type("Command:launch", **graded(tau_ms=10.0, tonic=release_tonic(0.9)), **common)
    c.type("Command:launch_timer", **graded(tau_ms=args.launch_ms), **common)
    launch = c.cell("LAUNCH", "Command:launch", "hindbrain")
    timer = c.cell("LAUNCH_T", "Command:launch_timer", "hindbrain")
    c.connect(mlr, timer, w(12.0 / args.mlr_release))
    c.connect(timer, launch, w(-25.0))
    for e in (down, up):
        c.connect(launch, e, w(-args.launch_hold))
    for k in "lr":       # no exploratory bank during the launch glide
        c.connect(launch, artr[k], w(-args.launch_hold))

    # --- Vestibulo-motor reflexes ---
    # Sensor channels arrive as event trains (value x 500 Hz). A large tonic
    # cancelling a pulsatile 0.5 baseline flips the net input's sign between
    # events, and the conductance membrane then over-weights inhibition (a
    # tonic of -5 against +11 mV/ms of sensor drive gave 1.9, not 6.3). So
    # each sensor first drives a relay cell (excitation only, release ~0.5
    # at the level-flight value, smooth graded output); error cells sum
    # relays and a tonic offset, which cancel linearly.
    # Sensor-driven cells also reach their operating point slowly (~0.3-1 s
    # after start, measured), so a tonic offset would be wrong at launch: the
    # crow rolled over within 0.4 s. Offsets are therefore taken from a
    # reference relay V_ref on a channel that stays at its 0.5 midpoint (the
    # beak hinge velocity; the beak is held closed), which ramps up with the
    # others: every relay sits at V_ref's level at the design point.
    c.type("Relay:vestibular", **graded(tau_ms=args.tau_relay), **common)
    # The gyro channel encodes 1 rad/s as 1/80: its relay integrates longer.
    c.type("Relay:gyro", **graded(tau_ms=args.tau_gyro), **common)
    relay = {}
    for key, channel, value in (("gx", "head_gravity_direction_x", 0.5 - math.sin(args.pitch_target) / 2),
                                ("gy", "head_gravity_direction_y", 0.5), ("wy", "vestibular_gyro_y", 0.5),
                                ("az", "airspeed_z", 0.5 - args.cruise * math.sin(args.alpha) / 40),
                                ("ax", "airspeed_x", 0.5 + args.cruise * math.cos(args.alpha) / 40),
                                ("fl", "left_wing_shoulder_flap_angle", 0.5), ("fr", "right_wing_shoulder_flap_angle", 0.5),
                                ("ref", "beak_gape_angular_velocity", 0.5)):
        relay[key] = c.cell(f"V_{key}", "Relay:gyro" if key == "wy" else "Relay:vestibular", "vestibular")
        c.sensor(channel, relay[key], ws(release_tonic(0.5) / value, exact=True))

    def syn(k):
        """Graded synapse of k mV/ms at full release, inhibitory ones scaled for
        cancellation against excitation (INH_GRADED)."""
        return drive_weight(k if k >= 0 else k * INH_EXACT / INH_GRADED, GAIN, exact=True)

    def error_pair(name, terms, bias, region="vestibular", tau=None):
        """Push-pull cells for e = sum k (relay - V_ref) (mV/ms): ``<name>_p``
        releases with +e, ``<name>_n`` with -e; ``bias`` (tonic) keeps both in
        their linear range (no dead zone)."""
        cells = {}
        for sign, suffix in ((1, "p"), (-1, "n")):
            t = c.type(f"Reflex:{name}_{suffix}", **graded(tau_ms=tau or args.tau_r, tonic=bias), **common)
            cell = c.cell(f"{name}_{suffix}", t, region)
            for key, k in terms:
                c.connect(relay[key], cell, syn(sign * k))
            ref = -sum(sign * k for _, k in terms)
            if ref:
                c.connect(relay["ref"], cell, syn(ref))
            cells[suffix] = cell
        return cells

    # Relay sensitivities near release 0.5 (d release / d input ~0.13):
    # V_gx +0.64 per rad nose-down, V_gy -0.73 per rad right-wing-down,
    # V_wy +0.018 per rad/s nose-down rate, V_az -0.41 per rad angle of
    # attack, V_ax +0.024 per m/s.
    # Roll: R_p (right wing down) -> left twist relaxes, right twist up.
    roll = error_pair("R", [("gy", -args.kroll / 0.73)], args.bias)
    for cell, sgn in ((roll["p"], 1), (roll["n"], -1)):
        c.connect(cell, twist_up["left"], w(-sgn * args.roll_out))
        c.connect(cell, twist_up["right"], w(sgn * args.roll_out))
    # Pitch: P_p (nose below the set point, or pitching down) -> tail up.
    pitch = error_pair("P", [("gx", args.kpitch / 0.64), ("wy", args.kpitch_rate / 0.018)], args.bias)
    for cell, sgn in ((pitch["p"], 1), (pitch["n"], -1)):
        c.connect(cell, "tail_pitch_positive", w(sgn * args.pitch_out))
        c.connect(cell, "tail_pitch_negative", w(-sgn * args.pitch_out))
    # Flight path: G_p (descending: -gamma = alpha - theta > 0) -> more twist.
    gamma = error_pair("G", [("gx", args.kgamma / 0.64), ("az", -args.kgamma / 0.41)], args.bias)
    for s in ("left", "right"):
        c.connect(gamma["p"], twist_up[s], w(args.gamma_out))
        c.connect(gamma["n"], twist_up[s], w(-args.gamma_out))
    # Stroke-end proprioception (shoulder angle; mean elevation of both wings,
    # V_fl - V_fr ~ 0.98 x elevation): the wing passing the bottom ends the
    # downstroke, passing the top ends the upstroke. It bounds the stroke and
    # shortens the first downstroke after launch (its adaptation starts from
    # rest), which otherwise drove the wings to the stop and pitched the bird
    # into a dive.
    bottom = error_pair("BOT", [("fl", -args.kstroke / 0.98), ("fr", args.kstroke / 0.98)], -args.kstroke * args.stroke_bottom, region="wing")["p"]
    top = error_pair("TOP", [("fl", args.kstroke / 0.98), ("fr", -args.kstroke / 0.98)], -args.kstroke * args.stroke_top, region="wing")["p"]
    c.connect(bottom, down, w(-args.stroke_switch)); c.connect(bottom, up, w(args.stroke_switch))
    c.connect(top, up, w(-args.stroke_switch)); c.connect(top, down, w(args.stroke_switch))
    # Airspeed: overspeed weakens the downstroke, underspeed noses down.
    speed_fast = error_pair("S_fast", [("ax", args.kspeed / 0.024)], -args.kspeed * (args.v_fast - args.cruise), region="airspeed")
    speed_slow = error_pair("S_slow", [("ax", -args.kspeed / 0.024)], -args.kspeed * (args.cruise - args.v_slow), region="airspeed")
    fast, slow = speed_fast["p"], speed_slow["p"]
    c.connect(fast, "left_wing_shoulder_flap_negative", w(-args.fast_out))
    c.connect(fast, "right_wing_shoulder_flap_positive", w(-args.fast_out))
    c.connect(slow, "tail_pitch_negative", w(args.slow_out))
    c.connect(slow, "tail_pitch_positive", w(-args.slow_out))

    # --- Steering: bank commands bias the roll reflex set point ---
    # RS_l (motion on the left) banks right, away; RS_r banks left.
    for cell, k in ((rs["l"], args.rs_bank), (artr["l"], args.artr_bank)):
        c.connect(cell, twist_up["left"], w(k))
        c.connect(cell, twist_up["right"], w(-k))
    for cell, k in ((rs["r"], args.rs_bank), (artr["r"], args.artr_bank)):
        c.connect(cell, twist_up["right"], w(k))
        c.connect(cell, twist_up["left"], w(-k))
    if args.retina:
        # Looming darkness: the bright ground below the horizon is replaced
        # by the dark tree line (or another obstacle) as the crow closes on
        # it. The retina reports luminance changes only, so a slow leaky
        # integrator of OFF minus ON events over each half of the lower
        # visual field (both eyes; columns 0-5 look left of the beak, 6-11
        # right) reconstructs the darkening; the head's own pitch and roll
        # wobble moves the horizon both ways and cancels. A side that darkens
        # excites its RS cell, which banks the crow away (RS_l -> right).
        c.type("Vision:dark", **graded(tau_ms=args.tau_dark, tonic=args.dark_bias), **common)
        dark = {k: c.cell(f"D_{k}", "Vision:dark", "tectum") for k in "lr"}
        for n in sensors:
            if not n.startswith("retina_"):
                continue
            _, _, row, col, pol = n.split("_")
            if int(row) < args.dark_row:
                continue
            k = "l" if int(col) < 6 else "r"
            c.sensor(n, dark[k], ws(args.dark_gain if pol == "off" else -args.dark_gain))
        for k in "lr":
            c.connect(dark[k], rs[k], w(args.dark_rs))

    # --- Posture: neck, beak and legs held by stretch reflexes ---
    held = ["neck_yaw", "neck_pitch", "neck_roll", "beak_gape"] + [f"{s}_leg_{j}" for s in ("left", "right")
                                                                   for j in ("hip_pitch", "hip_roll", "knee_flex", "ankle_pitch",
                                                                             "front_toes_curl", "hallux_curl")]
    for j in held:
        add_stretch_reflex(c, GAIN, j, f"{j}_positive", f"{j}_negative", f"{j}_angle", f"{j}_angular_velocity",
                           kp=args.posture_kp, kd=args.posture_kd, out_mv=args.posture_out, region="posture")

    labels = {"design": "crow flight: bilateral wingbeat half-centre on a series-elastic shoulder, vestibular roll/pitch/"
                        "flight-path reflexes on twist and tail, airspeed reflex, posture reflexes, hindbrain steering",
              "parameters": vars(args)}
    c.write(Path(args.output), Path(args.config_out) if args.config_out else None, synaptic_gain=GAIN, depth=0, labels=labels)
    return c


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--output", default="network_crow_flight.json")
    ap.add_argument("--config-out", default="webots_world/configs/config_crow_flight.json")
    ap.add_argument("--tau-m", type=float, default=4.0, help="motor pool membrane tau (ms)")
    ap.add_argument("--tau-r", type=float, default=4.0, help="reflex (error) cell membrane tau (ms)")
    ap.add_argument("--tau-gyro", type=float, default=15.0, help="gyro relay tau (ms)")
    ap.add_argument("--tau-relay", type=float, default=10.0, help="sensor relay tau (ms): smooths 250 Hz sensor events")
    ap.add_argument("--mlr-release", type=float, default=0.5)
    ap.add_argument("--cpg-drive", type=float, default=4.0)
    ap.add_argument("--tau-e", type=float, default=15.0)
    ap.add_argument("--tau-down", type=float, default=95.0, help="downstroke adaptation tau (ms): burst ~3.6 x tau")
    ap.add_argument("--tau-up", type=float, default=85.0, help="upstroke adaptation tau (ms)")
    ap.add_argument("--down-drive", type=float, default=1.2, help="extra MLR drive to the downstroke cell (mV/ms)")
    ap.add_argument("--w-ea", type=float, default=10.0)
    ap.add_argument("--w-ae", type=float, default=10.0)
    ap.add_argument("--w-ec", type=float, default=10.0)
    ap.add_argument("--w-ce", type=float, default=5.0, help="crossed inhibition: one isolated half-centre needs a winner-take-all")
    ap.add_argument("--launch-ms", type=float, default=250.0, help="launch timer tau (ms): wingbeat starts after ~2.5 tau")
    ap.add_argument("--launch-hold", type=float, default=15.0)
    ap.add_argument("--kstroke", type=float, default=20.0, help="stroke-end cells: mV/ms per rad beyond the limit")
    ap.add_argument("--stroke-bottom", type=float, default=0.25, help="downstroke ends below this depression (rad)")
    ap.add_argument("--stroke-top", type=float, default=0.25, help="upstroke ends above this elevation (rad)")
    ap.add_argument("--stroke-switch", type=float, default=8.0)
    ap.add_argument("--flap-down", type=float, default=11.0, help="downstroke burst -> shoulder (mV/ms at full release)")
    ap.add_argument("--flap-up", type=float, default=8.0)
    ap.add_argument("--twist", type=float, default=0.62, help="tonic nose-up twist density (~0.3 rad against the spring)")
    ap.add_argument("--twist-upstroke", type=float, default=0.6, help="upstroke -> less nose-up twist (mV/ms)")
    ap.add_argument("--tail-bias", type=float, default=0.4, help="tonic tail-down density")
    ap.add_argument("--breath", type=float, default=6.0)
    ap.add_argument("--bias", type=float, default=5.0, help="reflex pair bias (mV/ms): removes the dead zone")
    ap.add_argument("--kroll", type=float, default=5.0, help="roll reflex: mV/ms per rad of bank")
    ap.add_argument("--roll-out", type=float, default=6.0)
    ap.add_argument("--kpitch", type=float, default=15.0, help="pitch reflex: mV/ms per rad")
    ap.add_argument("--kpitch-rate", type=float, default=1.4, help="pitch-rate term (mV/ms per rad/s)")
    ap.add_argument("--pitch-target", type=float, default=-0.126, help="nose-up pitch set point (rad)")
    ap.add_argument("--pitch-out", type=float, default=14.0)
    ap.add_argument("--kgamma", type=float, default=10.0, help="flight-path reflex: mV/ms per rad")
    ap.add_argument("--gamma-out", type=float, default=3.0)
    ap.add_argument("--cruise", type=float, default=10.5, help="design airspeed (m/s)")
    ap.add_argument("--alpha", type=float, default=0.1, help="design body angle of attack (rad)")
    ap.add_argument("--kspeed", type=float, default=4.0, help="airspeed cells: mV/ms per m/s")
    ap.add_argument("--v-fast", type=float, default=11.5)
    ap.add_argument("--v-slow", type=float, default=9.0)
    ap.add_argument("--fast-out", type=float, default=3.0)
    ap.add_argument("--slow-out", type=float, default=3.0)
    ap.add_argument("--rs-plateau", type=float, default=18.0)
    ap.add_argument("--rs-bank", type=float, default=5.0, help="RS -> differential twist (bank) (mV/ms)")
    ap.add_argument("--artr-bank", type=float, default=3.0)
    ap.add_argument("--artr-tau-a", type=float, default=5000.0)
    ap.add_argument("--retina", type=int, default=1)
    ap.add_argument("--tau-dark", type=float, default=800.0, help="looming-darkness integrator tau (ms)")
    ap.add_argument("--dark-bias", type=float, default=1.0)
    ap.add_argument("--dark-row", type=int, default=3, help="first retina row of the lower field (0 = top)")
    ap.add_argument("--dark-gain", type=float, default=60.0, help="OFF event -> darkness cell (mV/ms per unit at 500 Hz)")
    ap.add_argument("--dark-rs", type=float, default=4.0)
    ap.add_argument("--posture-kp", type=float, default=80.0)
    ap.add_argument("--posture-kd", type=float, default=10.0)
    ap.add_argument("--posture-out", type=float, default=8.0)
    args = ap.parse_args()
    c = build(args)
    print(f"wrote {args.output}: {len(c.cells)} cells, {len(c.syn)} synapses, {len(c.sense)} sensory projections")


if __name__ == "__main__":
    main()
