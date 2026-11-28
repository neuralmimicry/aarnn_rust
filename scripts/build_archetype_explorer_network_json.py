#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Exploring circuit for the archetype robots (wheeled bases, arms).

Machines have no species, so the closest biomimicry is the vertebrate
locomotor hierarchy that the fish circuit uses, with the body-specific
"spinal" layer replaced by the robot's effectors:

* hindbrain (``aarnn_circuit.add_hindbrain``): ``MLR`` forward command,
  bistable winner-take-all ``RS_l``/``RS_r`` turn decision, ``ARTR`` slow
  left/right alternation for exploratory arcs and head-on tie-breaks;
* ``P_k``: one proximity cell per range ray (the archetype ray reports
  distance / 3 m, so proximity = 1 - value). Left rays (0 < angle < pi)
  excite ``RS_l`` (turn right, away), right rays ``RS_r``; the front ray
  brakes the forward drive and feeds both (the WTA picks a side);
* wheels: push-pull drive of each wheel's positive/negative channels
  (antagonist-rate-v1, decoded linearly for a graded brain). Forward and
  turning patterns are measured, not assumed, by
  ``scripts/qa/calibrate_wheels.py`` (axes, mirrored mounts, mecanum and omni
  bases differ). Wheel joint damping bounds the speed reached at a command;
* posture: every non-wheel joint (arms, mast, doors, grippers, torso) is held
  at its initial angle by a proportional-derivative stretch reflex built from
  two rectified error cells (agonist/antagonist), the spinal counterpart of a
  muscle-spindle reflex.

Run with the MuJoCo venv (calibration simulates the body):
  target/robot-mujoco-venv/bin/python scripts/build_archetype_explorer_network_json.py keenon_t8
"""
import argparse
import json
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parent / "qa"))
from aarnn_circuit import (Circuit, add_hindbrain, add_learned_anticipation, add_learned_foraging, add_stretch_reflex,  # noqa: E402
                           add_turn_bouts, apply_learner, bistable_band,
                           drive_weight, graded, sensor_weight, steady_release)

ROOT = Path(__file__).resolve().parents[1]
GAIN = 100.0
RAY_RANGE_M = 3.0


def body_length(model):
    xs = [b['world_position'][0] for b in model['bodies']]
    ys = [b['world_position'][1] for b in model['bodies']]
    return max(max(xs) - min(xs), max(ys) - min(ys), 0.1)


def build(args):
    from calibrate_wheels import probe, wheel_like
    model = json.loads((ROOT / f"sim/content/embodiments/generated/{args.profile}.json").read_text())
    sensors = [s["name"] for s in model["io"]["sensors"]]
    outputs = list(model["io"]["output_names"])
    acts = [a["name"] for a in model["actuators"]]
    if model["actuators"] and any(a.get("readout") for a in model["actuators"]):
        raise SystemExit("explicit readouts: not an archetype (antagonist-rate-v1) body")
    cal = probe(args.profile)
    c = Circuit(args.profile, sensors, outputs)

    def w(mv):
        return drive_weight(mv, GAIN)

    def ws(mv):
        return sensor_weight(mv, GAIN)
    common = dict(nmda_ratio=0.0, synaptic_gain=GAIN)
    hb = add_hindbrain(c, GAIN, mlr_release=args.mlr_release, rs_plateau_mv=args.rs_plateau)
    mlr, rs, artr = hb["mlr"], hb["rs"], hb["artr"]
    c.type("Motor:push_pull", **graded(tau_ms=15.0, release_hz=500.0), **common)
    for o in outputs:
        c.output(o, "Motor:push_pull")

    def channels(act):
        return f"{act}_positive", f"{act}_negative"

    def push_pull(pre, act, signed_mv):
        pos, neg = channels(act)
        c.connect(pre, pos, w(signed_mv))
        c.connect(pre, neg, w(-signed_mv))

    # Proximity cells: P = f(W (1 - ray)), half release at the turn-onset
    # distance. RS switches on at the upper knee of its bistable band.
    length = body_length(model)
    d_on = min(max(args.onset_lengths * length, 0.25), 1.2)
    p_on = 1.0 - d_on / RAY_RANGE_M
    w_p = 5.68 / p_on
    lo_knee, hi_knee = bistable_band(args.rs_plateau)
    w_prs = hi_knee / steady_release(w_p * p_on)
    c.type("Sense:proximity", **graded(tau_ms=20.0, tonic=w_p), **common)
    rays = [s for s in model["io"]["sensors"] if s["kind"] == "range_ray"]
    prox = {}
    for s in rays:
        a = s["angle_rad"] % (2 * math.pi)
        prox[s["name"]] = cell = c.cell(f"P_{s['name']}", "Sense:proximity", "sensory", (math.cos(a) * 0.01, math.sin(a) * 0.01, 0.0))
        c.sensor(s["name"], cell, sensor_weight(-w_p, GAIN, exact=True))
        facing = math.cos(a)
        side = math.sin(a)
        if facing > 0.9:      # straight ahead: both sides, WTA decides
            for k in "lr":
                c.connect(cell, rs[k], w(args.front_rs * w_prs))
        elif facing > -0.1 and abs(side) > 0.1:   # forward-lateral and lateral rays
            weight = w_prs * (1.0 if facing > 0.5 else args.lateral_rs)
            c.connect(cell, rs["l" if side > 0 else "r"], w(weight))
    front = [prox[s["name"]] for s in rays if math.cos(s["angle_rad"]) > 0.9]
    learn = None
    if args.learn_groups > 0:
        # Learning by experience (aarnn_circuit.add_learned_anticipation).
        fr = [s["name"] for s in rays if math.cos(s["angle_rad"]) > 0.9]
        side_rays = {sd: [s["name"] for s in rays if 0.9 >= math.cos(s["angle_rad"]) > 0.5
                          and (math.sin(s["angle_rad"]) > 0) == (sd == "l")] + fr for sd in "lr"}
        side_rays = {sd: v for sd, v in side_rays.items() if v}
        add_learned_anticipation(c, GAIN, side_rays, lambda d: d / RAY_RANGE_M, True, d_on, d_on + 0.05, d_on + 0.35,
                                 args.learn_groups, {sd: [(rs[sd], w_prs)] for sd in side_rays})
        learn = dict(profile=args.profile, encoding="direct", reach_m=RAY_RANGE_M, channels=side_rays, teach_m=d_on,
                     near_m=d_on + 0.05, far_m=d_on + 0.35, protocol_far_m=min(d_on + 1.2, RAY_RANGE_M),
                     protocol_near_m=max(d_on - 0.1, 0.05))

    bouts = None
    if args.explore:
        # Spontaneous run-and-turn bouts (shared hindbrain): a bout clock
        # kicks both RS cells and the ARTR side turns.
        bouts = add_turn_bouts(c, GAIN, hb, on_ms=args.explore_on_ms, off_ms=args.explore_off_ms)
    if args.forage:
        # Learned recharging at the dock (needs-v1; aarnn_circuit.add_learned_foraging):
        # charging stops the forward command; the beacon's value is learned.
        # Feeding halts locomotion: forward command, turns, and the stall /
        # reversal reflex (a stopped base otherwise reads as blocked and REV
        # backed it off the dock ~1 s after it stopped, 25 Nov). STALL and REV
        # are created below; the halt synapses are added after them.
        forage_halt = [rs["l"], rs["r"], artr["l"], artr["r"]]
        forage = add_learned_foraging(c, GAIN, mlr, bouts["on"] if bouts else None, hunger_below=args.forage_hunger,
                                      halt=forage_halt, steer=args.forage_steer > 0)
        if learn is None:
            learn = dict(profile=args.profile)

    wheels = [a for a in acts if wheel_like(a)]
    fwd = cal.get("forward_pattern", {})
    turn_left = cal.get("turn_left_pattern", {})
    for a in wheels:
        if fwd.get(a):
            push_pull(mlr, a, args.drive * fwd[a] / args.mlr_release)
            for cell in front:
                push_pull(cell, a, -args.brake * fwd[a])
        if turn_left.get(a) and args.forage and args.forage_steer > 0:
            # Learned steering: STEER_l (dock cue stronger on the left) -> turn left.
            push_pull(forage["steer"]["l"], a, args.forage_steer * turn_left[a])
            push_pull(forage["steer"]["r"], a, -args.forage_steer * turn_left[a])
        if turn_left.get(a):
            # RS_l: obstacle on the left -> turn right (negative yaw).
            push_pull(rs["l"], a, -args.turn * turn_left[a])
            push_pull(rs["r"], a, args.turn * turn_left[a])
            push_pull(artr["l"], a, -args.artr_turn * turn_left[a])
            push_pull(artr["r"], a, args.artr_turn * turn_left[a])

    # Stall reflex and reversal (the worm's AVA backward command, on wheels).
    # Per-wheel forward-speed cells V_k saturate near cruise, so one wheel
    # spinning freely (lifted, slipping) cannot mask the others; STALL is the
    # slow deficit of the command over summed wheel speed. When it charges
    # (the base is blocked by something its ray ring cannot see, such as a
    # table top), the bistable, self-terminating REV cell backs the base away
    # for about a second and the winner-take-all turn follows.
    fwd_wheels = [a for a in wheels if fwd.get(a)]
    if fwd_wheels:
        radius = []
        for a in fwd_wheels:
            joint = next(j for j in model["joints"] if j["name"] == next(x["joint"] for x in model["actuators"] if x["name"] == a))
            radius += [max(g["half_size_m"][0], 1e-3) for g in model["geoms"] if g["body"] == joint["body"] and g["type"] == 5]
        r_wheel = sum(radius) / len(radius) if radius else 0.05
        omega = max(abs(cal["forward_test"]["forward_m"]) / 0.6 / r_wheel, 0.5)
        w_speed = 5.68 / (omega / 80.0)   # half release at the calibrated speed
        c.type("Hindbrain:stall", **graded(tau_ms=args.stall_tau, tonic=args.stall_drive), **common)
        stall = c.cell("STALL", "Hindbrain:stall", "hindbrain")
        for a in fwd_wheels:
            # -speed_bias: the cancellation of the sensor's 0.5 baseline leaves
            # ~+3.6 mV/ms (ripple, conductance asymmetry); without it a
            # stalled wheel read as ~45 % of cruise and STALL never charged.
            c.type(f"Sense:speed:{a}", **graded(tau_ms=20.0, tonic=-0.5 * w_speed * fwd[a] - args.speed_bias), **common)
            v = c.cell(f"V_{a}", f"Sense:speed:{a}", "sensory")
            # Exact weights: the tonic cancels the sensor's 0.5 baseline.
            c.sensor(f"{a}_angular_velocity", v, sensor_weight(w_speed * fwd[a], GAIN, exact=True))
            c.connect(v, stall, w(-2.0 * args.stall_drive / len(fwd_wheels)))
        c.type("Hindbrain:REV", **graded(tau_ms=20.0, plateau_mv=22.0), **common)
        c.type("Hindbrain:REV_adapt", **graded(tau_ms=args.rev_tau), **common)
        rev = c.cell("REV", "Hindbrain:REV", "hindbrain")
        rev_a = c.cell("REV_A", "Hindbrain:REV_adapt", "hindbrain")
        c.connect(stall, rev, w(args.stall_rev))
        c.connect(rev, rev_a, w(10.0))
        c.connect(rev_a, rev, w(-9.0))
        for a in fwd_wheels:
            push_pull(rev, a, -args.reverse * fwd[a])
        for k in "lr":
            c.connect(rev, rs[k], w(args.stall_rs))
        if args.forage:
            for cell in (stall, rev):
                c.connect("FEED", cell, w(-30.0))

    # Posture: hold each non-wheel joint at its initial angle (qpos0 = 0 for
    # the archetype joints). Angle sensor (q + pi) / 2 pi; velocity (w + 40) / 80.
    held = [a for a in acts if a not in wheels]
    for a in held:
        pos, neg = channels(a)
        add_stretch_reflex(c, GAIN, a, pos, neg, f"{a}_angle", f"{a}_angular_velocity",
                           kp=args.posture_kp, kd=args.posture_kd, out_mv=args.posture_out)
    labels = {"design": "archetype explorer: shared hindbrain, calibrated wheel push-pull, stretch-reflex posture",
              "parameters": vars(args) | {"onset_m": d_on, "w_proximity": w_p, "w_proximity_rs": w_prs,
                                         "rs_band": [lo_knee, hi_knee]},
              "calibration": cal}
    c.write(Path(args.output), Path(args.config_out) if args.config_out else None, synaptic_gain=GAIN,
            depth=0, labels=labels)
    if learn:
        apply_learner(Path(args.output), Path(args.config_out) if args.config_out else None, GAIN, learn)
    return c, cal


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("profile")
    ap.add_argument("--output")
    ap.add_argument("--config-out")
    ap.add_argument("--mlr-release", type=float, default=0.5)
    ap.add_argument("--explore", type=int, default=0, help="1: spontaneous run-and-turn bouts (shared hindbrain)")
    ap.add_argument("--explore-on-ms", type=float, default=2000.0)
    ap.add_argument("--explore-off-ms", type=float, default=12000.0)
    ap.add_argument("--forage", type=int, default=0, help="1: learned recharging at the dock (needs-v1 channels)")
    ap.add_argument("--forage-hunger", type=float, default=0.5)
    ap.add_argument("--forage-steer", type=float, default=0.0,
                    help="learned steering toward the stronger cue: STEER -> wheel turn push-pull (mV/ms); 0 = off")
    ap.add_argument("--drive", type=float, default=4.0, help="forward push-pull drive at MLR release (mV/ms)")
    ap.add_argument("--brake", type=float, default=4.0, help="front proximity -> forward drive reduction (mV/ms)")
    ap.add_argument("--turn", type=float, default=7.0, help="RS -> wheel turning push-pull (mV/ms)")
    ap.add_argument("--artr-turn", type=float, default=1.0, help="ARTR -> gentle exploratory turning (mV/ms)")
    ap.add_argument("--rs-plateau", type=float, default=18.0, help="RS bistability (mV): 18 -> band ~1.34-1.76")
    ap.add_argument("--onset-lengths", type=float, default=2.0, help="turn onset distance in body lengths")
    ap.add_argument("--front-rs", type=float, default=0.8)
    ap.add_argument("--lateral-rs", type=float, default=0.5)
    ap.add_argument("--stall-drive", type=float, default=6.0, help="stall cell drive when the wheels are still (mV/ms)")
    ap.add_argument("--stall-tau", type=float, default=500.0)
    ap.add_argument("--speed-bias", type=float, default=4.0, help="measured offset removed from wheel-speed cells (mV/ms)")
    ap.add_argument("--stall-rev", type=float, default=6.0, help="STALL -> REV (REV switches on above ~1.6 mV/ms)")
    ap.add_argument("--rev-tau", type=float, default=1500.0, help="reversal adaptation: back-up duration scale (ms)")
    ap.add_argument("--reverse", type=float, default=10.0, help="REV -> backward wheel drive (mV/ms)")
    ap.add_argument("--stall-rs", type=float, default=4.0)
    ap.add_argument("--posture-kp", type=float, default=150.0, help="angle error gain (mV/ms per unit sensor)")
    ap.add_argument("--posture-kd", type=float, default=20.0)
    ap.add_argument("--posture-out", type=float, default=8.0)
    ap.add_argument("--learn-groups", type=int, default=0, help="anticipation learning: range cells per side (0 = off)")
    args = ap.parse_args()
    args.output = args.output or f"network_{args.profile}_explorer.json"
    args.config_out = args.config_out or f"webots_world/configs/config_{args.profile}_explorer.json"
    c, cal = build(args)
    print(f"wrote {args.output}: {len(c.cells)} cells, {len(c.syn)} synapses, {len(c.sense)} sensory projections;"
          f" forward {cal.get('forward_test')}, turn {cal.get('turn_test')}")


if __name__ == "__main__":
    main()
