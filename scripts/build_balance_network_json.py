#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Self-balancing explorer (UGOT balance car and wheel-leg).

An inverted pendulum on two wheels: the closest biomimicry is upright
posture held by vestibulospinal reflexes. The IMU's gravity direction plays
the otoliths (absolute tilt), the gyro the semicircular canals (tilt rate):

* ``B+``/``B-``: a push-pull pair of graded vestibulospinal cells,
  B+ = k_tilt * tilt + k_rate * pitch rate (B- the mirror image), each with a
  little co-activation (muscle tone) so small corrections are not lost in the
  release threshold; they drive both wheels forward/backward so the wheels
  roll under the falling centre of mass (positive torque rolls forward);
* speed through lean: the ``MLR`` command shifts the balance set point
  forward (a balancing body moves by leaning), and back-EMF wheel damping
  turns a steady lean into a steady speed;
* steering: the shared hindbrain (``add_hindbrain``) - range-ray proximity to
  a bistable winner-take-all ``RS`` turn and ``ARTR`` arcs - drives a wheel
  differential (left turn: right wheel forward, left wheel back);
* the wheel-leg's legs are held by ripple-safe servo reflexes (``add_servo_reflex``).

Lagged-PD feasibility (MuJoCo, true tilt, 30-40 ms lag): kp 10, kd 0.15-0.3
(ctrl units per rad, per rad/s) balances a 6 degree start and a 3 N push;
kd 0 falls. Balancing IMU ranges: gravity +-0.5 g (g_x = sin(pitch)), gyro
+-8 rad/s.
"""
import argparse
import json
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aarnn_circuit import (Circuit, add_hindbrain, add_servo_reflex, bistable_band,  # noqa: E402
                           drive_weight, graded, measure_sensor_relay, sensor_weight, steady_release)

ROOT = Path(__file__).resolve().parents[1]
GAIN = 100.0
RAY_RANGE_M = 3.0


def build(args):
    model = json.loads((ROOT / f"sim/content/embodiments/generated/{args.profile}.json").read_text())
    c = Circuit.for_embodiment(args.profile, exact_cells=True)

    def w(mv):
        return drive_weight(mv, GAIN)
    common = dict(nmda_ratio=0.0, synaptic_gain=GAIN)
    hb = add_hindbrain(c, GAIN, mlr_release=args.mlr_release, rs_plateau_mv=args.rs_plateau)
    mlr, rs, artr = hb["mlr"], hb["rs"], hb["artr"]
    c.type("Motor:push_pull", **graded(tau_ms=args.tau_out, release_hz=500.0), **common)
    for o in model["io"]["output_names"]:
        c.output(o, "Motor:push_pull")
    # Vestibular relays (otolith: gravity x; canal: gyro y). A sensor arrives
    # as a ~250 Hz pulse train: a cell cancelling its 0.5 baseline with a large
    # tonic sees its net current flip sign every few ms and, through the
    # conductance membrane, reports a duty cycle (B+ and B- summed to 1 and
    # ignored tilt). Relays take the sensor with excitation only and a tonic
    # below the ripple trough, so their net input never changes sign; their
    # continuous graded release then feeds the push-pull pair.
    # Excitation only, so the pulse-train ripple never turns the net input
    # inhibitory (inhibitory conductance is ~7x more effective per mV/ms and
    # wins the average: cancelling relays read low and flat). The operating
    # point is set where biology sets it, in the release threshold: half
    # release at the upright membrane potential, with a steep slope.
    w_relay = args.relay_drive
    g0 = 5.0 * w_relay * 0.5 / 70.0
    v0 = -70.0 / (1.0 + g0)
    relay_bio = dict(graded_release_v_half_mv=v0, graded_release_slope_mv=args.relay_slope_mv)
    c.type("Vestibular:relay", **graded(tau_ms=args.tau_relay), **common, **relay_bio)
    # Operating point and slope measured in the runtime (pulse-train input).
    r_lo, r0, r_hi = measure_sensor_relay(args.profile, "body_gravity_x", [0.45, 0.5, 0.55], w_relay,
                                          0.0, args.tau_relay, **relay_bio)
    slope = (r_hi - r_lo) / 0.1                      # release per sensor unit
    io = {s["name"]: s for s in model["io"]["sensors"]}
    per_rad = 1.0 / (io["body_gravity_x"]["range"][1] - io["body_gravity_x"]["range"][0])      # sin(pitch) -> sensor
    per_rad_s = 1.0 / (io["body_gyro_y"]["range"][1] - io["body_gyro_y"]["range"][0])
    k_tilt = args.tilt_gain / (per_rad * slope)
    k_rate = args.rate_gain / (per_rad_s * slope)
    relays = {}
    for sensor in ("body_gravity_x", "body_gyro_y"):
        relays[sensor] = c.cell(f"R_{sensor}", "Vestibular:relay", "vestibular")
        c.sensor(sensor, relays[sensor], sensor_weight(w_relay, GAIN))
    tone = args.tone
    io_names = {s["name"]: s for s in model["io"]["sensors"]}
    wheel_count = sum(1 for s in model["io"]["sensors"] if s["kind"] == "angular_velocity" and s["joint"].startswith("wheel"))
    vel_range = next(s["range"] for s in model["io"]["sensors"] if s["kind"] == "angular_velocity" and s["joint"].startswith("wheel"))
    k_speed_pre = args.tilt_gain * args.speed_lean / (((1.0 / args.wheel_radius) / (vel_range[1] - vel_range[0])) * slope) if wheel_count else 0.0
    base = r0 * (k_tilt + k_rate + k_speed_pre)
    c.type("Vestibular:B+", **graded(tau_ms=args.tau_b, tonic=tone - base + args.trim), **common)
    c.type("Vestibular:B-", **graded(tau_ms=args.tau_b, tonic=tone + base - args.trim), **common)
    bp = c.cell("B+", "Vestibular:B+", "vestibular")
    bn = c.cell("B-", "Vestibular:B-", "vestibular")
    for sensor, k in (("body_gravity_x", k_tilt), ("body_gyro_y", k_rate)):
        c.connect(relays[sensor], bp, w(k))
        c.connect(relays[sensor], bn, w(-k))
    # Outer speed loop: lean back in proportion to forward wheel speed (as a
    # balancing rider does), so a steady lean command becomes a steady speed
    # instead of a runaway. Wheel speed reaches the pair through a relay of
    # the same excitation-only design (mean of the wheels' velocity channels).
    wheel_sensors = [s["name"] for s in model["io"]["sensors"]
                     if s["kind"] == "angular_velocity" and s["joint"].startswith("wheel")]
    rv = c.cell("R_wheel_speed", "Vestibular:relay", "vestibular")
    for sensor in wheel_sensors:
        c.sensor(sensor, rv, sensor_weight(w_relay / len(wheel_sensors), GAIN))
    vel_range = io[wheel_sensors[0]]["range"]
    per_m_s = (1.0 / args.wheel_radius) / (vel_range[1] - vel_range[0])
    k_speed = args.tilt_gain * args.speed_lean / (per_m_s * slope)
    c.connect(rv, bp, w(k_speed))
    c.connect(rv, bn, w(-k_speed))
    base_speed = r0 * k_speed
    calibration = dict(relay_release=[r_lo, r0, r_hi], slope=slope, k_tilt=k_tilt, k_rate=k_rate, k_speed=k_speed)
    # Lean forward to move: the MLR command offsets the tilt set point.
    c.connect(mlr, bp, w(-args.lean / args.mlr_release))
    c.connect(mlr, bn, w(args.lean / args.mlr_release))
    wheels = [a["name"] for a in model["actuators"] if a["name"].startswith("wheel")]
    side = {}
    for a in wheels:
        joint = next(j for j in model["joints"] if j["name"] == a)
        body = model["bodies"][joint["body"] - 1]
        side[a] = 1.0 if body["world_position"][1] > 0 else -1.0     # +1 left wheel
    for a in wheels:
        pos, neg = f"{a}_positive", f"{a}_negative"
        c.connect(bp, pos, w(args.k_out))
        c.connect(bp, neg, w(-args.k_out))
        c.connect(bn, neg, w(args.k_out))
        c.connect(bn, pos, w(-args.k_out))
        # Turning: RS_r (obstacle right) turns left = right wheel forward,
        # left wheel back; RS_l the reverse; ARTR gently.
        for cell, mv in ((rs["r"], args.turn), (rs["l"], -args.turn), (artr["r"], args.artr_turn), (artr["l"], -args.artr_turn)):
            signed = -side[a] * mv
            c.connect(cell, pos, w(signed))
            c.connect(cell, neg, w(-signed))
    for a in model["actuators"]:
        if a["name"] in wheels:
            continue
        # Ripple-safe servo (30 ms relays): add_stretch_reflex rectifies the
        # sensors' pulse ripple into co-contraction in exact cells (~2 N m/rad
        # on the OEM UGOT servo instead of ~6.5).
        add_servo_reflex(c, GAIN, a["name"], f"{a['name']}_positive", f"{a['name']}_negative",
                         f"{a['name']}_angle", f"{a['name']}_angular_velocity", ref_rad=0.0,
                         kp_mv_per_rad=args.leg_kp, kd_mv_per_rad_s=args.leg_kd, out_mv=args.leg_out)
    # Range rays (distance / 3 m) -> proximity -> RS (turn away).
    d_on = args.onset_m
    p_on = 1.0 - d_on / RAY_RANGE_M
    w_p = 5.68 / p_on
    _, hi = bistable_band(args.rs_plateau)
    w_prs = hi / steady_release(w_p * p_on)
    c.type("Sense:proximity", **graded(tau_ms=20.0, tonic=w_p), **common)
    for s in model["io"]["sensors"]:
        if s["kind"] != "range_ray":
            continue
        a = s["angle_rad"] % (2 * math.pi)
        cell = c.cell(f"P_{s['name']}", "Sense:proximity", "sensory")
        c.sensor(s["name"], cell, sensor_weight(-w_p, GAIN))
        if math.cos(a) > 0.9:
            for k in "lr":
                c.connect(cell, rs[k], w(0.8 * w_prs))
        elif math.cos(a) > -0.1 and abs(math.sin(a)) > 0.1:
            c.connect(cell, rs["l" if math.sin(a) > 0 else "r"], w(w_prs * (1.0 if math.cos(a) > 0.5 else 0.5)))
    labels = {"design": "vestibulospinal balance (otolith tilt + canal rate push-pull), lean-to-move, shared hindbrain",
              "parameters": vars(args), "calibration": calibration}
    c.write(Path(args.output), Path(args.config_out), synaptic_gain=GAIN, depth=0, labels=labels)
    return c


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("profile", choices=("ugot_balance", "ugot_wheelleg"))
    ap.add_argument("--leg-kp", type=float, default=150.0, help="wheel-leg leg servo stiffness (mV/ms per rad)")
    ap.add_argument("--leg-kd", type=float, default=3.0)
    ap.add_argument("--leg-out", type=float, default=20.0)
    ap.add_argument("--output")
    ap.add_argument("--config-out")
    ap.add_argument("--tilt-gain", type=float, default=100.0, help="B drive per rad of tilt (mV/ms)")
    # 4.5 since the OEM wheel back-EMF (2 kgf cm at 360 rpm: 0.0052 N m s/rad,
    # was 0.0078 at 25 rad/s): at 3.0 the lighter damping left the pitch loop
    # underdamped and the robot fell within 2 s.
    ap.add_argument("--rate-gain", type=float, default=4.5, help="B drive per rad/s of pitch rate (mV/ms)")
    ap.add_argument("--relay-drive", type=float, default=20.0, help="relay excitation per sensor unit (mV/ms)")
    ap.add_argument("--relay-slope-mv", type=float, default=1.5, help="relay release slope (mV)")
    ap.add_argument("--tau-relay", type=float, default=10.0)
    ap.add_argument("--speed-lean", type=float, default=0.1, help="lean-back per m/s of forward speed (rad)")
    ap.add_argument("--wheel-radius", type=float, default=0.05)
    ap.add_argument("--tone", type=float, default=2.0, help="co-activation of B+ and B- (mV/ms)")
    ap.add_argument("--trim", type=float, default=0.0, help="set-point trim (mV/ms, + leans back)")
    ap.add_argument("--tau-b", type=float, default=5.0)
    ap.add_argument("--tau-out", type=float, default=5.0)
    ap.add_argument("--k-out", type=float, default=12.0)
    ap.add_argument("--mlr-release", type=float, default=0.5)
    ap.add_argument("--lean", type=float, default=3.0, help="MLR -> forward lean (mV/ms at B; tilt-gain mV/ms per rad)")
    ap.add_argument("--turn", type=float, default=6.0)
    ap.add_argument("--artr-turn", type=float, default=4.0)
    ap.add_argument("--rs-plateau", type=float, default=18.0)
    ap.add_argument("--onset-m", type=float, default=0.45)
    ap.add_argument("--posture-kp", type=float, default=150.0)
    ap.add_argument("--posture-kd", type=float, default=20.0)
    ap.add_argument("--posture-out", type=float, default=8.0)
    args = ap.parse_args()
    args.output = args.output or f"network_{args.profile}_balance.json"
    args.config_out = args.config_out or f"webots_world/configs/config_{args.profile}_balance.json"
    c = build(args)
    print(f"wrote {args.output}: {len(c.cells)} cells, {len(c.syn)} synapses, {len(c.sense)} sensory projections")


if __name__ == "__main__":
    main()
