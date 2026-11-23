#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Drosophila flight circuit (fly mechanics v3, BANC and FAFB bodies).

The fly's wings are beaten by the body's ``resonant-flight-v1`` motor at the
thoracic resonance (asynchronous flight muscle); a 20 ms graded decoder
cannot carry a 40 Hz wingbeat, and no brain needs to. The brain commands the
motor as descending neurons and steering muscles do: flight power, each
wing's stroke amplitude and each wing's stroke centre (``flight_commands``
in ``scripts/embodiment_neural.py``: channels the walking circuit leaves
silent). The circuit (``aarnn_circuit`` blocks, exact cells):

* takeoff: ``TAKEOFF`` (a slow flight-motivation cell) rises while the fly
  stands (~4 s, until the attitude integrators have settled) and switches
  on ``FLIGHT``, a bistable plateau latch
  (DNg02-like flight-power command). ``FLIGHT`` drives the power output,
  the baseline stroke amplitude of both wings and tucks the legs (femora
  lifted), as flies do in flight;
* haltere-like rate relays (gyro, +-10 rad/s): roll rate and pitch rate
  through excitation-only relays (``add_sensor_relay``), each also feeding a
  slow leaky integrator (an attitude estimate, tau ~1.5 s, read against an
  identical input-free ``I_ref`` so their start-up cancels), into push-pull
  steering pairs: ``ROLL_l``/``ROLL_r`` set the left/right amplitude
  difference (more lift on the falling side), ``PITCH_b``/``PITCH_f`` move
  both stroke centres backward/forward (nose down/up), ``YAW_l``/``YAW_r``
  shift the stroke centres differentially (yaw damping and turning);
* drift: in flight the accelerometer senses aerodynamic drag, not tilt, so
  its lateral channel is an airspeed signal that banks the fly against
  sideways drift (the role ventral optic flow plays in flies);
* height: the ventral range ray (``dros_s_22_prox_12``, 0.3 m) holds the
  flight height through both amplitudes, with a transient (fast minus slow
  twin relay, the slow one through an inverting cell) that damps climb and
  sink;
* the shared hindbrain (``add_hindbrain``): forward range rays on one side
  excite the ipsilateral ``RS`` (turn away, through ``YAW``). Optional and
  off by default: ``MLR`` forward trim (``--forward-trim``), ``ARTR`` arcs
  (``--artr-turn``) and a ray-driven tilt away from walls (``--wall-pitch``,
  ``--wall-roll``; 0.06 tumbled the fly at a wall).

Gains are physical (asymmetry per rad/s, stroke-centre rad per rad/s, ...),
those of the programmed proof ``scripts/qa/fly_flight_proof.py``. The build
calibrates in the runtime itself (open-loop ``celegans_depth_probe``): every
steering cell's tonic is trimmed to its hover operating point, each steering
path's small-signal gain is measured and rescaled to the proof's, and the
climb transient's static residual is cancelled (snapshot labels
``calibration``). Deterministic: the defaults rebuild the shipped network.
"""
import argparse
import json
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aarnn_circuit import (Circuit, add_hindbrain, add_sensor_relay, bistable_band, drive_weight,  # noqa: E402
                           graded, measure_sensor_relay, release_tonic, sensor_weight, steady_release)

ROOT = Path(__file__).resolve().parents[1]
GAIN = 100.0
# Small-signal effectiveness of nominal inhibition vs excitation on an
# exact graded cell at release 0.5, measured with this circuit (the climb
# path's cancellation, see height_gain in the snapshot labels).
INH_AT_OP = float(__import__("os").environ.get("FLY_INH_AT_OP", "0.47"))
LEGS = ("lf", "lm", "lr", "rf", "rm", "rr")


def current_for(release: float, resistance: float = 5.0) -> float:
    """Steady excitatory current (mV/ms) giving a default graded cell ``release``."""
    lo, hi = 0.0, 200.0
    for _ in range(60):
        mid = 0.5 * (lo + hi)
        if steady_release(mid, resistance=resistance) < release:
            lo = mid
        else:
            hi = mid
    return 0.5 * (lo + hi)


def slope_at(current: float) -> float:
    """d release / d current (per mV/ms) of a default graded cell at ``current``."""
    h = 0.05
    return (steady_release(current + h) - steady_release(current - h)) / (2 * h)


def measure_cells(c, sensor_values, ms=3000, series=False):
    """Release of every hidden cell under constant sensor values, measured
    open-loop in the runtime (``celegans_depth_probe``, as measure_sensor_relay)."""
    import subprocess
    import tempfile
    names = list(c.sensory)
    rate = {cell: c.types[t].get("graded_release_max_hz", 20.0) for cell, t, _, _ in c.cells}
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        c.write(tmp / "net.json", synaptic_gain=GAIN, depth=0)
        with open(tmp / "in.jsonl", "w") as f:
            f.write(json.dumps({"profile": c.profile, "sensor_names": names, "output_names": c.outputs, "trace_every_steps": 10}) + "\n")
            values = [sensor_values[n] for n in names]
            for step in range(0, ms // 2 + 10, 10):
                f.write(json.dumps({"time_s": step * 0.002, "step": step, "sensors": values}) + "\n")
        subprocess.run([str(ROOT / "target/release/examples/celegans_depth_probe"), "--network", str(tmp / "net.json"),
                        "--ms", str(ms), "--input", str(tmp / "in.jsonl"), "--out", str(tmp / "o.json"),
                        "--series", str(tmp / "s.jsonl"), "--series-ms", "10", "--no-raster"], check=True, capture_output=True)
        rows = [json.loads(x)["hidden_hz"] for x in open(tmp / "s.jsonl")]
    cells = [cell for cell, _, _, _ in c.cells]
    tail = rows[int(len(rows) * 0.7):]
    final = {cell: sum(r[i] for r in tail) / len(tail) / rate[cell] for i, cell in enumerate(cells)}
    if not series:
        return final
    return {"final": final, "series": [(0.01 * (k + 1), {cell: r[i] / rate[cell] for i, cell in enumerate(cells)}) for k, r in enumerate(rows)]}


def twin_relay(c, channel, s_ref, drive_mv, slope_mv, tau_ms, name, region="sensory"):
    """A second excitation-only relay of ``channel`` (as add_sensor_relay, own
    cell name and time constant). Two relays of one sensor with the same
    calibration have the same steady state, so fast - slow is a pure
    transient (a rate signal) with no standing offset."""
    g0 = 5.0 * drive_mv * s_ref / 70.0
    v0 = -70.0 / (1.0 + g0)
    bio = dict(graded_release_v_half_mv=v0, graded_release_slope_mv=slope_mv)
    lo, mid, hi = measure_sensor_relay(c.profile, channel, [s_ref - 0.05, s_ref, s_ref + 0.05], drive_mv, 0.0, tau_ms, **bio)
    c.type(f"Relay:{name}", **graded(tau_ms=tau_ms), nmda_ratio=0.0, synaptic_gain=GAIN, **bio)
    cell = c.cell(name, f"Relay:{name}", region)
    c.sensor(channel, cell, sensor_weight(drive_mv, GAIN))
    return cell, mid, (hi - lo) / 0.1


def build(args):
    model = json.loads((ROOT / f"sim/content/embodiments/generated/{args.profile}.json").read_text())
    io = model["io"]
    outputs = list(io["output_names"])
    acts = {a["name"]: a for a in model["actuators"]}
    flight = model["flight"]
    cmd = {k["name"]: k["readout"] for k in flight["commands"]}
    sensors = {s["name"]: s for s in io["sensors"]}
    c = Circuit.for_embodiment(args.profile, exact_cells=True)

    def w(mv):
        return drive_weight(mv, GAIN)
    common = dict(nmda_ratio=0.0, synaptic_gain=GAIN)
    hb = add_hindbrain(c, GAIN, mlr_release=args.mlr_release, rs_plateau_mv=args.rs_plateau)
    mlr, rs, artr = hb["mlr"], hb["rs"], hb["artr"]
    c.type("Motor:out", **graded(tau_ms=args.tau_out, release_hz=500.0), **common)
    for o in outputs:
        c.output(o, "Motor:out")

    def channel(readout, sign=1):
        return next(outputs[x["channel"]] for x in readout if (x["coefficient"] > 0) == (sign > 0))
    power = [outputs[x["channel"]] for x in cmd["power"]]
    amp = {"l": channel(cmd["amplitude_left"]), "r": channel(cmd["amplitude_right"])}
    back = {"l": channel(cmd["centre_left"], 1), "r": channel(cmd["centre_right"], 1)}
    fwd = {"l": channel(cmd["centre_left"], -1), "r": channel(cmd["centre_right"], -1)}
    a_max, c_max = flight["amplitude_max_rad"], flight["centre_max_rad"]

    # --- takeoff and flight state -------------------------------------------------
    c.type("Flight:motivation", **graded(tau_ms=args.takeoff_tau, tonic=release_tonic(0.9)), **common)
    c.type("Flight:latch", **graded(tau_ms=20.0, plateau_mv=args.latch_plateau), **common)
    takeoff = c.cell("TAKEOFF", "Flight:motivation", "flight")
    fl = c.cell("FLIGHT", "Flight:latch", "flight")
    _, upper = bistable_band(args.latch_plateau)
    c.connect(takeoff, fl, w(upper / args.takeoff_threshold))
    r_flight = 1.0                                   # latched plateau release (saturated)

    # Output operating points (density = release of the graded output cell).
    a0 = args.amplitude / a_max
    i_a0 = current_for(a0)
    s_a = slope_at(i_a0)                             # amplitude density per mV/ms
    for ch in power:
        c.connect(fl, ch, w(args.power_mv))
    # Stroke centre channels sit at a common tone; u = back - forward.
    i_c0 = current_for(args.centre_tone)
    s_c = slope_at(i_c0)

    # --- push-pull steering pairs ---------------------------------------------------
    tone_i = current_for(0.5)
    s_pp = slope_at(tone_i)                          # push-pull release per mV/ms
    c.type("Steer:pp", **graded(tau_ms=args.tau_pp, tonic=tone_i), **common)
    # A push-pull release deviation dr drives an output by out_mv x dr.
    out_mv = args.steer_out_mv
    # Each flight output is driven by two pairs at their 0.5 tone (amplitude:
    # ROLL and LIFT_u; stroke centre: PITCH and YAW); FLIGHT supplies the rest.
    base_amp = i_a0 - 2 * 0.5 * out_mv
    base_c = i_c0 - 2 * 0.5 * out_mv
    if min(base_amp, base_c) < 0:
        raise ValueError("steering tone exceeds an output operating point: lower --steer-out-mv")
    for s in "lr":
        c.connect(fl, amp[s], w(base_amp / r_flight))
        c.connect(fl, back[s], w(base_c / r_flight))
        c.connect(fl, fwd[s], w(base_c / r_flight))

    pairs = {}
    for name in ("ROLL_l", "ROLL_r", "PITCH_b", "PITCH_f", "YAW_l", "YAW_r", "LIFT_u", "LIFT_d"):
        pairs[name] = c.cell(name, "Steer:pp", "steering")
    # Output wiring: ROLL_l raises the left amplitude, ROLL_r the right; LIFT
    # raises/lowers both; PITCH_b moves both strokes back (nose down);
    # YAW_r moves the left stroke back and the right forward (yaw right).
    for s, o in (("l", "r"), ("r", "l")):
        c.connect(pairs[f"ROLL_{s}"], amp[s], w(out_mv))
        c.connect(pairs["LIFT_u"], amp[s], w(out_mv))
        c.connect(pairs["LIFT_d"], amp[s], w(-out_mv * args.lift_down_scale))
        c.connect(pairs["PITCH_b"], back[s], w(out_mv))
        c.connect(pairs["PITCH_f"], fwd[s], w(out_mv))
    c.connect(pairs["YAW_r"], back["l"], w(out_mv))
    c.connect(pairs["YAW_r"], fwd["r"], w(out_mv))
    c.connect(pairs["YAW_l"], back["r"], w(out_mv))
    c.connect(pairs["YAW_l"], fwd["l"], w(out_mv))

    # A command x on a pair (ROLL_l = +x, ROLL_r = -x) changes the outputs by
    # out_mv x s_pp x dI x s_out. Per unit of the physical command:
    def pp_current(d_out_per_unit, s_out):
        return d_out_per_unit / (out_mv * s_pp * s_out)
    roll_mv = pp_current(a0, s_a)                    # per unit asymmetry (a = a0 (1 +- asym))
    pitch_mv = pp_current(0.5 / c_max, s_c)          # per rad of stroke centre (u split over 2 channels)
    lift_mv = pp_current(a0, s_a)                    # per unit fractional amplitude change

    # Inhibition onto a push-pull cell held at its 0.5 operating point is
    # INH_AT_OP x as effective as the same nominal excitation (measured in
    # the runtime: the climb transient's exc/inh cancellation below), so
    # mirrored inputs scale their inhibitory side by 1 / INH_AT_OP to stay
    # symmetric (otherwise a cancelling exc/inh pair adds a level-dependent
    # common mode).
    def inh(mv):
        return w(-mv / INH_AT_OP)

    def to_pair(cell_pos, cell_neg, relay, rel_slope, phys_per_sensor, gain_per_phys, mv_per_unit, r0):
        """relay -> (+, -) pair: current = gain x physical deviation."""
        k = gain_per_phys * mv_per_unit / (rel_slope / phys_per_sensor)   # mV/ms per unit relay release
        c.connect(relay, cell_pos, w(k))
        c.connect(relay, cell_neg, inh(k))
        return k * r0

    offsets = {n: 0.0 for n in pairs}

    def offset(pos, neg, base):
        offsets[pos] -= base
        offsets[neg] += base

    gyro_per_sensor = 20.0                           # rad/s per sensor unit
    accel_per_sensor = 40.0                          # m/s^2 per sensor unit
    relays = {}
    for name, key in (("roll", "dros_s_03_gyro.x"), ("yaw", "dros_s_03_gyro.y"), ("pitch_neg", "dros_s_03_gyro.z"),
                      ("lateral_neg", "dros_s_00_accel.z"), ("forward", "dros_s_00_accel.x")):
        cell, r0, slope = add_sensor_relay(c, GAIN, key, 0.5, drive_mv=args.relay_drive, slope_mv=args.relay_slope,
                                           tau_ms=args.tau_relay, region="halteres" if "gyro" in key else "sensory")
        relays[name] = (cell, r0, slope)
    # Integrators: slow graded cells holding release 0.5 at zero rate; their
    # release deviation is s_i x k_in x (relay deviation) filtered with tau.
    # The steering pairs read each integrator against I_ref, an identical
    # cell without sensory input: both rise together from rest (~3 s at a
    # 1.5 s tau), so the start-up transient cancels instead of commanding a
    # roll before takeoff.
    c.type("Halteres:integrator", **graded(tau_ms=args.tau_int, tonic=tone_i), **common)
    i_ref = c.cell("I_ref", "Halteres:integrator", "halteres")
    k_in = args.integrator_in_mv
    integ = {}
    for name in ("roll", "pitch_neg"):
        cell, r0, slope = relays[name]
        ic = c.cell(f"I_{name}", "Halteres:integrator", "halteres")
        c.connect(cell, ic, w(k_in))
        offsets.setdefault(ic, 0.0)
        integ[name] = (ic, slope * k_in * s_pp, r0)  # integrator release per unit sensor, steady
    # Integrator baseline: cancel k_in x r0 in its tonic (cell type per axis).
    for name, (ic, _, r0) in integ.items():
        c.type(f"Halteres:integrator:{name}", **graded(tau_ms=args.tau_int, tonic=tone_i - k_in * r0), **common)
        c.cells[c.index[ic]] = (ic, f"Halteres:integrator:{name}", "halteres", (0.0, 0.0, 0.0))

    # Roll: asym = -(k_roll w_x + k_roll_int phi) - k_drift a_y. ROLL_l raises the
    # left wing, so roll rate + (left side up) drives ROLL_r.
    cell, r0, slope = relays["roll"]
    base = to_pair(pairs["ROLL_r"], pairs["ROLL_l"], cell, slope, gyro_per_sensor, args.k_roll, roll_mv, r0)
    offset("ROLL_r", "ROLL_l", base)
    ic, s_int, r0i = integ["roll"]
    # Integrator release ~ s_int x sensor deviation, as a rate filtered over
    # tau: angle ~ tau x rate, so per unit integrator release the angle is
    # tau x gyro_per_sensor / s_int.
    k = args.k_roll_int * roll_mv * (args.tau_int / 1000.0) * gyro_per_sensor / s_int
    # Excitation only, one side each: at I_roll = I_ref both pairs receive
    # the same conductance, so the differential is exact at any baseline.
    c.connect(ic, pairs["ROLL_r"], w(2 * k))
    c.connect(i_ref, pairs["ROLL_l"], w(2 * k))
    # Lateral drift: sensor accel.z = -a_y; drifting left (+y) -> drag -y ->
    # a_y < 0 -> sensor up -> bank left side up (asym +, ROLL_l). The relay
    # is smoothed by a slow copy (tau_drift): the thorax also accelerates
    # sideways with every roll oscillation, which a fast drift path would
    # feed back into roll.
    dl, d0, s_dl = twin_relay(c, "dros_s_00_accel.z", 0.5, args.relay_drive, args.relay_slope, args.tau_drift, "D_lateral")
    base = to_pair(pairs["ROLL_l"], pairs["ROLL_r"], dl, s_dl, accel_per_sensor, args.k_drift_y, roll_mv, d0)
    offset("ROLL_l", "ROLL_r", base)
    # Pitch: c = -(k_pitch w_y + k_pitch_int theta) + k_drift_x a_x. Sensor
    # gyro.z = -w_y: nose-up rate raises it and needs nose down (PITCH_b).
    cell, r0, slope = relays["pitch_neg"]
    base = to_pair(pairs["PITCH_b"], pairs["PITCH_f"], cell, slope, gyro_per_sensor, args.k_pitch, pitch_mv, r0)
    offset("PITCH_b", "PITCH_f", base)
    ic, s_int, r0i = integ["pitch_neg"]
    k = args.k_pitch_int * pitch_mv * (args.tau_int / 1000.0) * gyro_per_sensor / s_int
    c.connect(ic, pairs["PITCH_b"], w(2 * k))
    c.connect(i_ref, pairs["PITCH_f"], w(2 * k))
    cell, r0, slope = relays["forward"]
    # Forward drift (a_x < 0, sensor down) -> nose up (PITCH_f).
    base = to_pair(pairs["PITCH_b"], pairs["PITCH_f"], cell, slope, accel_per_sensor, args.k_drift_x, pitch_mv, r0)
    offset("PITCH_b", "PITCH_f", base)
    # Yaw: turning left (w_z > 0, sensor up) -> yaw right (YAW_r).
    cell, r0, slope = relays["yaw"]
    base = to_pair(pairs["YAW_r"], pairs["YAW_l"], cell, slope, gyro_per_sensor, args.k_yaw, pitch_mv * 0.5, r0)
    offset("YAW_r", "YAW_l", base)

    # Height: v = 1 - h / reach; amplitude x (1 + k_h (h_ref - h) - k_climb h').
    ray = sensors["dros_s_22_prox_12"]
    reach = ray["angle_rad"]
    v_ref = 1.0 - args.height / reach
    # A 30 ms relay: the climb transient below differentiates the ray, and
    # a 10 ms relay's pulse ripple (~+-0.03 release) was amplified into
    # +-0.3 swings of the lift pair.
    rh, rh0, sh = add_sensor_relay(c, GAIN, ray["name"], v_ref, drive_mv=args.relay_drive, slope_mv=args.height_slope,
                                   tau_ms=args.tau_height, region="sensory")
    hs, hs0, s_hs = twin_relay(c, ray["name"], v_ref, args.relay_drive, args.height_slope, args.tau_climb, "H_slow")
    # LIFT_u = k_h x reach x (v - v_ref) (closer to the ground -> more lift).
    base = to_pair(pairs["LIFT_u"], pairs["LIFT_d"], rh, sh, reach, args.k_height, lift_mv, rh0)
    offset("LIFT_u", "LIFT_d", base)
    k_height_mv = base / rh0
    # Climb transient: h' = -reach v' ~ -reach (v_fast - v_slow) / tau.
    tau_s = args.tau_climb / 1000.0
    k = args.k_climb * lift_mv * reach / tau_s / sh
    # The slow relay is subtracted through an inverting cell (N_slow, held at
    # 0.5 by its tonic, inhibited by H_slow), so LIFT_u only ever sums
    # excitation: a large derivative weight cancelled by direct inhibition
    # left a level-dependent residual (conductance synapses) that tripled
    # the height gain. N_slow's runtime-measured slope sets its weight.
    c.type("Height:invert", **graded(tau_ms=5.0, tonic=tone_i), **common)
    n_slow = c.cell("N_slow", "Height:invert", "sensory")
    k_inv = 0.5 / max(s_hs, 1e-3) / s_pp       # ~-0.5 release per sensor unit
    c.connect(hs, n_slow, inh(k_inv))
    c.connect(rh, pairs["LIFT_u"], w(k))
    offset("LIFT_u", "LIFT_d", k * rh0)
    climb_k = k

    # Forward flight: MLR tilts the lift forward (strokes back = nose down).
    c.connect(mlr, pairs["PITCH_b"], w(args.forward_trim * pitch_mv / hb["mlr_release"]))
    c.connect(mlr, pairs["PITCH_f"], inh(args.forward_trim * pitch_mv / hb["mlr_release"]))
    # Walls: the forward-looking range rays excite the ipsilateral RS (turn
    # away); RS_l (obstacle left) yaws right.
    for s in io["sensors"]:
        if s["kind"] == "legacy-range" and s["name"] != ray["name"]:
            ang = math.atan2(s["direction"][1], s["direction"][0])
            if math.cos(ang) > 0.3 and abs(math.sin(ang)) > 0.1:
                c.sensor(s["name"], rs["l" if math.sin(ang) > 0 else "r"], args.wall_rs_w * math.cos(ang))
            elif math.cos(ang) > 0.95:
                for k_ in "lr":
                    c.sensor(s["name"], rs[k_], args.wall_rs_w * 0.5)
    # In flight the ring of range rays (0.08 m) also tilts the fly away
    # from a near wall, as a hovering animal sidesteps: a wall ahead pitches
    # the nose up (PITCH_f), behind down (PITCH_b), on the left banks the
    # left side up (ROLL_l, lift tilted right), on the right ROLL_r. Sensors
    # at 0 (nothing in reach) send no pulses, so this path is silent in
    # open air.
    rays = [x for x in io["sensors"] if x["kind"] == "legacy-range" and x["name"] != ray["name"]]
    per_wall = 1.0 / 4.0                             # ~4 rays of the ring see a broadside wall
    for x in rays:
        ang = math.atan2(x["direction"][1], x["direction"][0])
        ca, sa = math.cos(ang), math.sin(ang)
        pitch_cell = pairs["PITCH_f"] if ca > 0 else pairs["PITCH_b"]
        roll_cell = pairs["ROLL_l"] if sa > 0 else pairs["ROLL_r"]
        if abs(ca) > 0.2 and args.wall_pitch:
            c.sensor(x["name"], pitch_cell, sensor_weight(per_wall * args.wall_pitch * abs(ca) / (c_max * out_mv * s_c * s_pp), GAIN))
        if abs(sa) > 0.2 and args.wall_roll:
            c.sensor(x["name"], roll_cell, sensor_weight(per_wall * args.wall_roll * abs(sa) * 2 * a0 / (out_mv * s_a * s_pp), GAIN))
    c.connect(rs["l"], pairs["YAW_r"], w(args.turn * pitch_mv * 0.5))
    c.connect(rs["l"], pairs["YAW_l"], inh(args.turn * pitch_mv * 0.5))
    c.connect(rs["r"], pairs["YAW_l"], w(args.turn * pitch_mv * 0.5))
    c.connect(rs["r"], pairs["YAW_r"], inh(args.turn * pitch_mv * 0.5))
    c.connect(artr["l"], pairs["YAW_r"], w(args.artr_turn * pitch_mv * 0.5))
    c.connect(artr["r"], pairs["YAW_l"], w(args.artr_turn * pitch_mv * 0.5))

    # Pair and integrator tonics: tone minus the relays' baselines, then
    # trimmed in the runtime itself so that every steering cell sits at its
    # 0.5 operating point in level hover (the integrators' DC gain turns any
    # residual baseline error into a standing command).
    tonics = {cell: tone_i + offsets[name] for name, cell in pairs.items()}
    tonics[n_slow] = tone_i
    for name, (ic, _, r0) in integ.items():
        tonics[ic] = tone_i - k_in * r0

    def apply_tonics():
        for cell, tonic in tonics.items():
            region = c.cells[c.index[cell]][2]
            c.type(f"Tonic:{cell}", **graded(tau_ms=args.tau_int if cell.startswith("I_") else args.tau_pp, tonic=tonic), **common)
            c.cells[c.index[cell]] = (cell, f"Tonic:{cell}", region, (0.0, 0.0, 0.0))
    apply_tonics()
    # Steering is gated by flight: without FLIGHT the pairs' outputs only add
    # to channels whose baseline is FLIGHT (an unpowered motor ignores them).

    # Legs: tuck in flight (femora lifted: left femur +, right femur -).
    def leg_channels(joint):
        r = acts[joint]["readout"]
        return (next(outputs[x["channel"]] for x in r if x["coefficient"] > 0),
                next(outputs[x["channel"]] for x in r if x["coefficient"] < 0))
    # The readout pairs are unequal (a x 0.62, b x -0.38): the right femora
    # lift through the weaker channel, scaled so both sides tuck alike (an
    # asymmetric tuck shifts the centre of mass sideways and rolls the fly).
    for leg in LEGS:
        r = acts[f"{leg}_femur"]["readout"]
        pos, neg = leg_channels(f"{leg}_femur")
        lift_ch = pos if leg[0] == "l" else neg
        coeff = abs(next(x["coefficient"] for x in r if outputs[x["channel"]] == lift_ch))
        strong = max(abs(x["coefficient"]) for x in r)
        c.connect(fl, lift_ch, w(args.tuck_mv * strong / coeff))
    hover = {n: 0.0 for n in c.sensory}
    for n in hover:
        kind = sensors[n]["kind"]
        hover[n] = 0.5 if kind in ("legacy-gyro", "legacy-accel") else 0.0
    hover["dros_s_00_accel.y"] = (9.81 + 20.0) / 40.0       # vertical specific force = 1 g
    hover[ray["name"]] = v_ref
    trim = []

    step_gain = {cell: 1.0 for cell in tonics}
    last_err = {}

    def trim_tonics(iterations, tolerance=0.01):
        # Newton-like steps on each tonic with the nominal slope s_pp; a cell
        # stuck near zero or full release (slope ~0) gets a growing step,
        # an overshoot halves it.
        for _ in range(iterations):
            rel = measure_cells(c, hover, ms=6000)
            err = {cell: (rel["I_ref"] if cell.startswith("I_") else 0.5) - rel[cell] for cell in tonics}
            trim.append(max(abs(e) for e in err.values()))
            if trim[-1] < tolerance:
                break
            for cell, e in err.items():
                prev = last_err.get(cell)
                if prev is not None and prev * e < 0:
                    step_gain[cell] = max(0.25, step_gain[cell] * 0.5)
                elif prev is not None and abs(e) > 0.6 * abs(prev):
                    step_gain[cell] = min(16.0, step_gain[cell] * 2.0)
                last_err[cell] = e
                tonics[cell] += step_gain[cell] * e / s_pp
            apply_tonics()
    trim_tonics(args.trim_iterations)

    # End-to-end small-signal gain of every steering path, measured in the
    # runtime and rescaled to the programmed proof's gains: relay ripple,
    # release nonlinearity and conductance synapses made the designed weights
    # 1-4x off (roll rate ~4x). Each path: hold one sensor 0.03 off its hover
    # value, read the pair differential at steady state with the other
    # paths from that sensor removed, scale the path's synapses by
    # desired / measured.
    T = args.tau_int / 1000.0
    roll_r, roll_l, pb, pf, yr, yl = (pairs[n] for n in ("ROLL_r", "ROLL_l", "PITCH_b", "PITCH_f", "YAW_r", "YAW_l"))
    r_roll, r_pitch, r_yaw = relays["roll"][0], relays["pitch_neg"][0], relays["yaw"][0]
    i_roll, i_pitch = integ["roll"][0], integ["pitch_neg"][0]
    per_asym = 2 * a0 / (out_mv * s_a)          # (ROLL_l - ROLL_r) per unit amplitude asymmetry
    per_centre = 1.0 / (c_max * out_mv * s_c)   # (PITCH_b - PITCH_f) per rad of stroke centre
    paths = [
        # name, sensor, (pos, neg), path synapses, synapses removed meanwhile, desired d(pos - neg)/d sensor
        ("roll_rate", "dros_s_03_gyro.x", (roll_r, roll_l), [(r_roll, roll_r), (r_roll, roll_l)], [(i_roll, roll_r)],
         per_asym * args.k_roll * gyro_per_sensor),
        ("roll_integral", "dros_s_03_gyro.x", (roll_r, roll_l), [(i_roll, roll_r), (i_ref, roll_l)], [(r_roll, roll_r), (r_roll, roll_l)],
         per_asym * args.k_roll_int * T * gyro_per_sensor),
        ("lateral_drift", "dros_s_00_accel.z", (roll_l, roll_r), [(dl, roll_l), (dl, roll_r)], [],
         per_asym * args.k_drift_y * accel_per_sensor),
        ("pitch_rate", "dros_s_03_gyro.z", (pb, pf), [(r_pitch, pb), (r_pitch, pf)], [(i_pitch, pb)],
         per_centre * args.k_pitch * gyro_per_sensor),
        ("pitch_integral", "dros_s_03_gyro.z", (pb, pf), [(i_pitch, pb), (i_ref, pf)], [(r_pitch, pb), (r_pitch, pf)],
         per_centre * args.k_pitch_int * T * gyro_per_sensor),
        ("yaw_rate", "dros_s_03_gyro.y", (yr, yl), [(r_yaw, yr), (r_yaw, yl)], [], per_centre * 0.5 * args.k_yaw * gyro_per_sensor),
    ]
    gains = {}
    step = 0.03
    for name, sensor, (pos, neg), syns, removed, desired in paths:
        if desired == 0:
            continue
        saved = {k: c.syn[k] for k in removed}
        for k in removed:
            c.syn[k] = 0.0
        shifted = dict(hover)
        shifted[sensor] = hover[sensor] + step
        lo, hi = measure_cells(c, hover, ms=6000), measure_cells(c, shifted, ms=6000)
        measured = ((hi[pos] - hi[neg]) - (lo[pos] - lo[neg])) / step
        scale = desired / measured if measured * desired > 0 else 1.0
        scale = min(max(scale, 0.1), 10.0)
        for k in syns:
            c.syn[k] *= scale
        c.syn.update(saved)
        gains[name] = dict(desired=desired, measured=measured, scale=scale)
        trim_tonics(6)
    # N_slow -> LIFT_u weight: cancel the fast relay's static contribution
    # exactly (measured slopes of both at the operating point).
    dv = 0.04
    up = dict(hover)
    up[ray["name"]] = v_ref + dv
    lo_rel, hi_rel = measure_cells(c, hover, ms=6000), measure_cells(c, up, ms=6000)
    d_fast = (hi_rel[rh] - lo_rel[rh]) / dv
    d_inv = (hi_rel[n_slow] - lo_rel[n_slow]) / dv
    k_n = -climb_k * d_fast / d_inv if d_inv < 0 else 0.0
    c.connect(n_slow, pairs["LIFT_u"], w(k_n))
    tonics[pairs["LIFT_u"]] -= k_n * lo_rel[n_slow]
    apply_tonics()
    trim_tonics(3)
    hi_rel, lo_rel = measure_cells(c, up, ms=6000), measure_cells(c, hover, ms=6000)
    height_gain = dict(desired=a0 * args.k_height * reach / (out_mv * s_a), measured=(hi_rel["LIFT_u"] - lo_rel["LIFT_u"]) / dv,
                       fast_slope=d_fast, inverted_slope=d_inv, k_climb_mv=climb_k, k_inverted_mv=k_n)
    rel = measure_cells(c, hover, ms=8000, series=True)
    takeoff_s = next((t for t, r in rel["series"] if r["FLIGHT"] > 0.5), None)
    labels = {"design": "Drosophila flight: takeoff latch, haltere rate relays + leaky attitude integrators, "
                        "drag-sensing drift, ventral-ray height, push-pull steering of the resonant flight motor, shared hindbrain",
              "parameters": vars(args),
              "calibration": dict(trim_max_error=trim, takeoff_s=takeoff_s, height_gain=height_gain, path_gains=gains,
                                  hover_release={k: round(v, 4) for k, v in rel["final"].items()}, a0=a0, i_a0=i_a0, s_a=s_a, i_c0=i_c0, s_c=s_c, s_pp=s_pp, roll_mv=roll_mv, pitch_mv=pitch_mv,
                                  relays={k: v[1:] for k, v in relays.items()}, height_relay=(rh0, sh))}
    c.write(Path(args.output), Path(args.config_out) if args.config_out else None, synaptic_gain=GAIN, depth=0, labels=labels)
    return c


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--profile", default="drosophila_banc", choices=("drosophila_banc", "drosophila_fafb"))
    ap.add_argument("--output")
    ap.add_argument("--config-out")
    ap.add_argument("--takeoff-tau", type=float, default=5000.0,
                    help="flight motivation tau (ms): takeoff once the 1.5 s attitude integrators have settled (~4 s)")
    ap.add_argument("--takeoff-threshold", type=float, default=0.6, help="TAKEOFF release that latches FLIGHT")
    ap.add_argument("--latch-plateau", type=float, default=30.0, help="FLIGHT plateau (mV; exact cells need 26-35)")
    ap.add_argument("--power-mv", type=float, default=30.0)
    ap.add_argument("--amplitude", type=float, default=0.85, help="hover stroke amplitude (rad)")
    ap.add_argument("--centre-tone", type=float, default=0.5, help="stroke-centre channel density at zero centre")
    ap.add_argument("--steer-out-mv", type=float, default=5.0)
    ap.add_argument("--lift-down-scale", type=float, default=0.0)
    ap.add_argument("--tau-out", type=float, default=2.0)
    ap.add_argument("--tau-pp", type=float, default=2.0)
    ap.add_argument("--tau-relay", type=float, default=6.0)
    ap.add_argument("--relay-drive", type=float, default=20.0)
    ap.add_argument("--relay-slope", type=float, default=3.0)
    ap.add_argument("--height-slope", type=float, default=10.0)
    ap.add_argument("--tau-int", type=float, default=1500.0)
    ap.add_argument("--integrator-in-mv", type=float, default=10.0)
    ap.add_argument("--tau-climb", type=float, default=80.0)
    ap.add_argument("--tau-height", type=float, default=30.0, help="ventral-ray relay tau (ms)")
    ap.add_argument("--height", type=float, default=0.15)
    ap.add_argument("--k-height", type=float, default=1.0, help="fractional amplitude per m of height error")
    ap.add_argument("--k-climb", type=float, default=0.4, help="fractional amplitude per m/s of climb")
    ap.add_argument("--k-roll", type=float, default=0.04, help="amplitude asymmetry per rad/s of roll rate")
    ap.add_argument("--k-roll-int", type=float, default=0.1, help="asymmetry per rad of integrated roll")
    ap.add_argument("--k-drift-y", type=float, default=0.03, help="asymmetry per m/s^2 of lateral drag")
    ap.add_argument("--k-pitch", type=float, default=0.01, help="stroke centre (rad) per rad/s of pitch rate")
    ap.add_argument("--k-pitch-int", type=float, default=0.05)
    ap.add_argument("--k-drift-x", type=float, default=0.0)
    ap.add_argument("--tau-drift", type=float, default=100.0, help="lateral drag smoothing (ms)")
    ap.add_argument("--k-yaw", type=float, default=0.01, help="differential centre (rad) per rad/s of yaw rate")
    ap.add_argument("--forward-trim", type=float, default=0.0, help="MLR stroke-centre trim (rad): forward flight")
    ap.add_argument("--mlr-release", type=float, default=0.5)
    ap.add_argument("--rs-plateau", type=float, default=18.0)
    ap.add_argument("--wall-rs-w", type=float, default=0.06, help="range ray -> RS sensory weight")
    ap.add_argument("--turn", type=float, default=0.1, help="RS turn: differential centre (rad)")
    ap.add_argument("--wall-pitch", type=float, default=0.0,
                    help="stroke centre (rad) per unit wall proximity ahead/behind (0.06 tumbled the fly at a wall: untuned)")
    ap.add_argument("--wall-roll", type=float, default=0.0, help="amplitude asymmetry per unit wall proximity left/right (untuned)")
    ap.add_argument("--artr-turn", type=float, default=0.0)
    ap.add_argument("--tuck-mv", type=float, default=5.0)
    ap.add_argument("--trim-iterations", type=int, default=6)
    args = ap.parse_args()
    args.output = args.output or f"network_{args.profile}_flight.json"
    args.config_out = args.config_out or f"webots_world/configs/config_{args.profile}_flight.json"
    c = build(args)
    print(f"wrote {args.output}: {len(c.cells)} cells, {len(c.syn)} synapses, {len(c.sense)} sensory projections")


if __name__ == "__main__":
    main()
