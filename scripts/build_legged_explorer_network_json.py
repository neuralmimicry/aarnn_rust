#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Walking explorer for the four-legged archetype robots (UGOT quadruped and
spider, OEM 2-servo legs: 13 kgf cm, 60 rpm), from the shared blocks in
``aarnn_circuit`` (exact cells):

* legs: ``add_leg_oscillators`` on the four-leg ring (front-left, rear-left,
  rear-right, front-right are neighbours) settles into the diagonal pairs of
  a trot (~1.9 Hz; exact cells need 30 mV half-centre plateaus);
* joints: torque motors, so each joint is a ripple-safe servo reflex
  (``add_servo_reflex``, 30 ms sensor relays) whose set point the oscillator
  moves through stride relays ``G`` (equilibrium-point control): swing lifts
  first and protracts through a delay cell, stance retracts. The dog stands
  hip-forward / knee-bent (-0.4 / 0.8 rad) so knee flexion lifts the foot;
* turning: the inner legs step backward (``Gr`` relays, an AND of their
  phase and the turn command), so the body pivots;
* hindbrain (``add_hindbrain`` + ``add_turn_bouts``): range-ray proximity ->
  bistable ``RS`` turn decision; a bout clock gives spontaneous turn bouts
  whose side the slow ARTR alternation picks (zebrafish run-and-turn), each
  opened by a short back-off of all legs (``BACKUP``), which frees a body
  blocked by obstacles its torso-height rays do not see (the terrain's 3.9 cm
  traction-mat ribs: impassable for 12 cm legs).

  python3 scripts/build_legged_explorer_network_json.py ugot_quadruped
"""
import argparse
import json
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aarnn_circuit import (Circuit, add_hindbrain, add_learned_anticipation, add_leg_oscillators, add_sensor_relay, apply_learner,  # noqa: E402
                           add_servo_reflex, add_turn_bouts, bistable_band, drive_weight, graded, steady_release)

ROOT = Path(__file__).resolve().parents[1]
GAIN = 100.0
RAY_RANGE_M = 3.0
# leg_<x>_<y>: x +1 front / -1 rear, y +1 left / -1 right.
LEGS = ("1_1", "-1_1", "-1_-1", "1_-1")
NEIGHBOURS = [("1_1", "-1_1"), ("-1_1", "-1_-1"), ("-1_-1", "1_-1"), ("1_-1", "1_1")]


def build(args):
    model = json.loads((ROOT / f"sim/content/embodiments/generated/{args.profile}.json").read_text())
    outputs = list(model["io"]["output_names"])
    c = Circuit.for_embodiment(args.profile, exact_cells=True)

    def w(mv):
        return drive_weight(mv, GAIN)
    common = dict(nmda_ratio=0.0, synaptic_gain=GAIN)
    # Exact cells need 30 mV plateaus for an escape half-centre (see
    # add_leg_oscillators); at 22 mV the ARTR pair stalls with both sides off.
    hb = add_hindbrain(c, GAIN, plateau_mv=args.artr_plateau, mlr_release=args.mlr_release, rs_plateau_mv=args.rs_plateau,
                       artr_tau_a_ms=args.artr_tau)
    mlr, rs, artr = hb["mlr"], hb["rs"], hb["artr"]
    backup = None
    if args.bout_kick > 0:
        bout = add_turn_bouts(c, GAIN, hb, on_ms=args.bout_on, off_ms=args.bout_off, kick_mv=args.bout_kick)
        if args.backup > 0:
            # Back off before turning (C. elegans reversal before the omega
            # turn; the fly's moonwalker command): a transient on the bout
            # clock's onset reverses every leg for ~0.5 s, freeing feet that
            # are hooked on an obstacle the rays do not see.
            c.type("Bout:backup", **graded(tau_ms=30.0), **common)
            c.type("Bout:backup_adapt", **graded(tau_ms=args.backup_ms), **common)
            backup = c.cell("BACKUP", "Bout:backup", "hindbrain")
            ba = c.cell("BACKUP_A", "Bout:backup_adapt", "hindbrain")
            c.connect(bout["on"], backup, w(10.0))
            c.connect(bout["on"], ba, w(10.0))
            c.connect(ba, backup, w(-24.0))
    c.type("Motor:push_pull", **graded(tau_ms=10.0, release_hz=500.0), **common)
    for o in outputs:
        c.output(o, "Motor:push_pull")

    positions = {leg: (0.01 * int(leg.split("_")[0]), 0.008 * int(leg.split("_")[1])) for leg in LEGS}
    E, A, _ = add_leg_oscillators(c, GAIN, LEGS, NEIGHBOURS, mlr, args.drive, drive_release=args.mlr_release,
                                  tau_a_swing_ms=args.tau_a_swing, tau_a_stance_ms=args.tau_a_stance,
                                  inter=args.inter, inter_ex=args.inter_ex, positions=positions)
    Q = {}
    dog = args.profile == "ugot_quadruped"
    for a in model["actuators"]:
        j = a["name"]
        # Standing posture: a straight leg lifts its foot only to second
        # order when the knee bends (0.27 rad -> 2.5 mm); a dog stands with
        # the hip forward and the knee bent so knee motion lifts first-order.
        hip = j.endswith("_0")
        left = j.split("_")[2] == "1"
        if dog:
            ref = args.hip_ref if hip else args.knee_ref
        else:
            ref = 0.0 if hip else args.knee_ref * (1 if left else -1)
        Q[j] = add_servo_reflex(c, GAIN, j, f"{j}_positive", f"{j}_negative", f"{j}_angle",
                                f"{j}_angular_velocity", ref_rad=ref, kp_mv_per_rad=args.kp,
                                kd_mv_per_rad_s=args.kd, out_mv=args.out)
    c.type("Leg:relay", **graded(tau_ms=5.0), **common)
    c.type("Leg:delay", **graded(tau_ms=max(args.protract_delay, 1.0)), **common)
    # Turning: the inner legs step backward (AND of their phase and the turn
    # command), so the body pivots instead of only shortening one side.
    # AND gate: the threshold sits just above either input alone (phase relay
    # at full release, or the turn command at RS's ~0.67 bout level).
    c.type("Leg:reverse", **graded(tau_ms=5.0, tonic=-args.reverse_threshold), **common)
    dog = args.profile == "ugot_quadruped"
    for leg in LEGS:
        left = leg.endswith("_1")
        # Measured on the MJCF (OEM 2-servo legs): dog hip + moves every foot
        # back and knee flexion (+) lifts; spider like the hexapod (left hip +
        # retracts, right protracts; knee + lifts left, lowers right).
        protract = -1 if (dog or left) else 1
        lift = 1 if (dog or left) else -1
        inner = rs["l"] if not left else rs["r"]
        inner_artr = artr["l"] if not left else artr["r"]
        for ph in ("sw", "st"):
            g = c.cell(f"G_{leg}_{ph}", "Leg:relay", leg)
            c.connect(E[leg, ph], g, w(args.relay))
            c.connect(inner, g, w(-args.turn))
            c.connect(inner_artr, g, w(-args.artr_turn))
            if backup:
                c.connect(backup, g, w(-args.turn))
            hip, knee = Q[f"leg_{leg}_0"], Q[f"leg_{leg}_1"]
            d_hip = protract if ph == "sw" else -protract
            d_lift = lift if ph == "sw" else -lift
            mv_lift = args.lift if ph == "sw" else args.press
            # Swing shaping (as for the fly): the lift leads, cut back by the
            # swing cell's adaptation; protraction follows through a delay cell.
            hip_src = g
            if ph == "sw" and args.protract_delay > 0:
                hip_src = c.cell(f"D_{leg}_sw", "Leg:delay", leg)
                c.connect(g, hip_src, w(args.relay))
            for src, (qp, qn), d, mv in ((hip_src, hip, d_hip, args.stride), (g, knee, d_lift, mv_lift)):
                if mv <= 0:
                    continue
                ago, ant = (qp, qn) if d > 0 else (qn, qp)
                c.connect(src, ago, w(mv))
                c.connect(src, ant, w(-mv))
            if args.reverse > 0:
                gr = c.cell(f"Gr_{leg}_{ph}", "Leg:reverse", leg)
                c.connect(E[leg, ph], gr, w(args.relay))
                c.connect(inner, gr, w(args.reverse_gate))
                if backup:
                    c.connect(backup, gr, w(args.reverse_gate))
                ago, ant = (hip[0], hip[1]) if -d_hip > 0 else (hip[1], hip[0])
                c.connect(gr, ago, w(args.reverse))
                c.connect(gr, ant, w(-args.reverse))
                if ph == "sw":
                    ago, ant = (knee[0], knee[1]) if d_lift > 0 else (knee[1], knee[0])
                    c.connect(gr, ago, w(args.lift))
                    c.connect(gr, ant, w(-args.lift))
            if ph == "sw" and args.lift_adapt > 0:
                ago, ant = (knee[0], knee[1]) if d_lift > 0 else (knee[1], knee[0])
                c.connect(A[leg, "sw"], ago, w(-args.lift_adapt))
                c.connect(A[leg, "sw"], ant, w(args.lift_adapt))
    # Range rays (distance / 3 m): a relay centred on the turn-onset distance;
    # proximity P = tonic - k * relay is on when the ray reads nearer.
    length = max(max(b["world_position"][0] for b in model["bodies"]) - min(b["world_position"][0] for b in model["bodies"]), 0.1)
    d_on = min(max(args.onset_lengths * length, 0.25), 1.2)
    s_on = d_on / RAY_RANGE_M
    _, hi_knee = bistable_band(args.rs_plateau)
    for s in model["io"]["sensors"]:
        if s["kind"] != "range_ray":
            continue
        a = s["angle_rad"] % (2 * math.pi)
        relay, r0, slope = add_sensor_relay(c, GAIN, s["name"], s_on)
        k = 6.0 / max(slope * 0.1, 1e-3)          # P reaches ~6 mV/ms 0.1 sensor units nearer than onset
        c.type(f"Sense:proximity:{s['name']}", **graded(tau_ms=20.0, tonic=k * r0), **common)
        cell = c.cell(f"P_{s['name']}", f"Sense:proximity:{s['name']}", "sensory")
        c.connect(relay, cell, w(-k))
        w_prs = hi_knee / steady_release(6.0)
        if math.cos(a) > 0.9:
            for side in "lr":
                c.connect(cell, rs[side], w(0.8 * w_prs))
        elif math.cos(a) > -0.1 and abs(math.sin(a)) > 0.1:
            c.connect(cell, rs["l" if math.sin(a) > 0 else "r"], w(w_prs * (1.0 if math.cos(a) > 0.5 else 0.5)))
    learn = None
    if args.learn_groups > 0:
        # Learning by experience (aarnn_circuit.add_learned_anticipation): a
        # near detector at the innate onset teaches LT_side through the
        # forward rays of that side (and the front ray).
        rays = [s for s in model["io"]["sensors"] if s["kind"] == "range_ray"]
        front = [s["name"] for s in rays if math.cos(s["angle_rad"]) > 0.9]
        side_rays = {sd: [s["name"] for s in rays if math.cos(s["angle_rad"]) > 0.5 and 0.9 >= math.cos(s["angle_rad"])
                          and (math.sin(s["angle_rad"]) > 0) == (sd == "l")] + front for sd in "lr"}
        w_prs = hi_knee / steady_release(6.0)
        add_learned_anticipation(c, GAIN, side_rays, lambda d: d / RAY_RANGE_M, True, d_on, d_on + 0.05, d_on + 0.35,
                                 args.learn_groups, {sd: [(rs[sd], w_prs)] for sd in "lr"})
        learn = dict(profile=args.profile, encoding="direct", reach_m=RAY_RANGE_M, channels=side_rays, teach_m=d_on,
                     near_m=d_on + 0.05, far_m=d_on + 0.35, protocol_far_m=min(d_on + 1.2, RAY_RANGE_M),
                     protocol_near_m=max(d_on - 0.1, 0.05))
    labels = {"design": "four-legged trot: leg escape half-centres, equilibrium-point stretch reflexes, shared hindbrain",
              "parameters": vars(args)}
    c.write(Path(args.output), Path(args.config_out), synaptic_gain=GAIN, depth=0, labels=labels)
    if learn:
        apply_learner(Path(args.output), Path(args.config_out), GAIN, learn)
    return c


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("profile", choices=("ugot_quadruped", "ugot_spider"))
    ap.add_argument("--output")
    ap.add_argument("--config-out")
    ap.add_argument("--mlr-release", type=float, default=0.5)
    ap.add_argument("--drive", type=float, default=4.0)
    ap.add_argument("--tau-a-swing", type=float, default=120.0)
    ap.add_argument("--tau-a-stance", type=float, default=160.0)
    ap.add_argument("--inter", type=float, default=3.0)
    ap.add_argument("--inter-ex", type=float, default=2.0)
    ap.add_argument("--kp", type=float, default=150.0, help="servo stiffness (mV/ms per rad)")
    ap.add_argument("--kd", type=float, default=3.0, help="servo damping (mV/ms per rad/s)")
    ap.add_argument("--out", type=float, default=20.0)
    ap.add_argument("--relay", type=float, default=8.0)
    ap.add_argument("--stride", type=float, default=18.0, help="hip set-point shift (mV/ms; kp mV/ms per rad)")
    ap.add_argument("--lift", type=float, default=24.0)
    ap.add_argument("--press", type=float, default=2.0)
    ap.add_argument("--hip-ref", type=float, default=-0.4, help="dog hip standing angle (rad)")
    ap.add_argument("--knee-ref", type=float, default=None, help="knee standing angle (rad); default 0.8 dog, 0.5 spider")
    ap.add_argument("--protract-delay", type=float, default=60.0, help="protraction delay cell tau (ms); 0 = off")
    ap.add_argument("--lift-adapt", type=float, default=12.0, help="swing adaptation -> lift cut-back (mV/ms)")
    ap.add_argument("--burst", type=float, default=0.0, help="relay -> direct torque burst (mV/ms)")
    ap.add_argument("--turn", type=float, default=14.0)
    ap.add_argument("--reverse", type=float, default=24.0, help="inner-leg backward stride while turning (mV/ms); 0 = off")
    ap.add_argument("--reverse-gate", type=float, default=16.0, help="turn command onto the reversal relays (mV/ms)")
    ap.add_argument("--reverse-threshold", type=float, default=11.0, help="reversal relay threshold (mV/ms)")
    ap.add_argument("--bout-kick", type=float, default=1.2, help="bout clock onto both RS (mV/ms); 0 = no spontaneous bouts")
    ap.add_argument("--bout-on", type=float, default=2500.0, help="bout clock on-adaptation (ms)")
    ap.add_argument("--bout-off", type=float, default=None,
                    help="bout clock off-adaptation (ms); default 3000 (dog), 6000 (spider: it turns slowly, so longer runs)")
    ap.add_argument("--backup", type=float, default=1.0, help="back off at each bout onset (1 = on, 0 = off)")
    ap.add_argument("--backup-ms", type=float, default=400.0, help="back-off transient adaptation (ms)")
    ap.add_argument("--artr-tau", type=float, default=30000.0, help="ARTR adaptation (ms): sets how many bouts turn one way")
    ap.add_argument("--artr-turn", type=float, default=1.0)
    ap.add_argument("--rs-plateau", type=float, default=18.0)
    ap.add_argument("--artr-plateau", type=float, default=30.0)
    ap.add_argument("--onset-lengths", type=float, default=2.0)
    ap.add_argument("--learn-groups", type=int, default=0, help="anticipation learning: range cells per side (0 = off)")
    args = ap.parse_args()
    if args.knee_ref is None:
        args.knee_ref = 0.5 if args.profile == "ugot_spider" else 0.8
    if args.bout_off is None:
        args.bout_off = 6000.0 if args.profile == "ugot_spider" else 3000.0
    args.output = args.output or f"network_{args.profile}_walk.json"
    args.config_out = args.config_out or f"webots_world/configs/config_{args.profile}_walk.json"
    c = build(args)
    print(f"wrote {args.output}: {len(c.cells)} cells, {len(c.syn)} synapses, {len(c.sense)} sensory projections")


if __name__ == "__main__":
    main()
