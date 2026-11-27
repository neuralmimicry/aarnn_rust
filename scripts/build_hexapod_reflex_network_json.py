#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Hexapod walking circuit: proprioceptive swing/stance half-centres.

The insect-walking counterpart of the C. elegans locomotor circuit (see
``scripts/aarnn_circuit.py``): no central pattern generator; each leg steps
because sensory events switch bistable graded half-centres (Cruse's walking
rules; Ekeberg et al. 2004), and inhibition between neighbouring legs'
swing units makes the two alternating leg sets of a tripod gait.

Per leg (lf, lm, lr, rf, rm, rr):

* ``SW`` (swing) and ``ST`` (stance): graded, bistable (plateau), tonic bias
  inside their hysteresis band, mutual inhibition;
* ``HI`` / ``LO``: coxa-angle detectors past +/- ``extreme_angle`` (the
  legacy-angle sensor maps -3.14..3.14 rad to 0..1);
  posterior extreme -> SW, anterior extreme -> ST (mirrored left/right, from
  the measured MJCF kinematics: left coxa + moves the foot backward, right
  coxa + forward; left femur + lifts, right femur + lowers);
* foot contact excites ST and suppresses SW (a loaded leg keeps standing);
* graded joint outputs held at mid-range (release 0.5 = the MJCF reference
  posture) by tonic current, decoded by the body over the full joint range
  (``output_code = pulse_density``). Swing protracts and lifts, stance
  retracts and presses.

All weights are set from target drives in mV/ms (NMDA off so the synaptic
kernel is fixed): see ``DRIVE``.
"""
import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aarnn_circuit import RELEASE_HZ, SENSOR_HZ, Circuit, drive_weight, graded, release_tonic, sensor_weight  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
LEGS = ("lf", "lm", "lr", "rf", "rm", "rr")
# Neighbours whose swing units inhibit each other (two-colourable -> tripod).
NEIGHBOURS = [("lf", "lm"), ("lm", "lr"), ("rf", "rm"), ("rm", "rr"), ("lf", "rf"), ("lm", "rm"), ("lr", "rr")]

GAIN = 100.0          # synaptic_gain of every type


def w_graded(mv: float) -> float:
    """Weight giving `mv` mV/ms at full presynaptic release."""
    return drive_weight(mv, GAIN)


def w_sensor(mv: float, gain: float = GAIN) -> float:
    """Weight giving `mv` mV/ms at sensor value 1.0 onto a cell of `gain`."""
    return sensor_weight(mv, gain)


# legacy-angle sensors keep the Webots DeviceMapper convention:
# value = (q + 3.14) / 6.28 (adapters/mujoco/src/legacy.rs).
ANGLE_HALF_RANGE = 3.14
DETECTOR_GAIN = 500.0


def build(args):
    model = json.loads((ROOT / "sim/content/embodiments/generated/hexapod.json").read_text())
    sensors = [s["name"] for s in model["io"]["sensors"]]
    outputs = list(model["io"]["output_names"])
    c = Circuit("hexapod", sensors, outputs)
    c.type("Leg:half_centre", **graded(tau_ms=args.tau, release_hz=RELEASE_HZ, plateau_mv=args.plateau, tonic=args.bias),
           nmda_ratio=0.0, synaptic_gain=GAIN)
    # Coxa extreme detectors, drive = k (theta - theta_pep) for HI and
    # k (-theta_aep - theta) for LO with theta = 2 * 3.14 * v - 3.14.
    k = args.detector_slope
    c.type("Leg:detector_hi", **graded(tau_ms=10.0, release_hz=RELEASE_HZ, tonic=-k * (ANGLE_HALF_RANGE + args.extreme_angle)),
           nmda_ratio=0.0, synaptic_gain=DETECTOR_GAIN)
    c.type("Leg:detector_lo", **graded(tau_ms=10.0, release_hz=RELEASE_HZ, tonic=k * (ANGLE_HALF_RANGE - args.extreme_angle)),
           nmda_ratio=0.0, synaptic_gain=DETECTOR_GAIN)
    c.type("Motor:joint", **graded(tau_ms=20.0, release_hz=SENSOR_HZ, tonic=release_tonic(0.5)),
           nmda_ratio=0.0, synaptic_gain=GAIN)
    for o in outputs:
        c.output(o, "Motor:joint")

    def joint(leg, part):
        return next(o for o in outputs if o.endswith(f"_{leg}_{part}"))

    def sensor(leg, part):
        return next(s for s in sensors if s.endswith(f"_{leg}_{part}"))
    # The drives below were tuned on the v2 body (legacy proto ranges: coxa
    # span 1.4 rad, femur 1.2 rad). An output's release maps over the body's
    # actual joint range, so scale each drive by design span / actual span to
    # keep the same joint angles on any body (Freenove v3: +-pi/2, ~2.2-2.6x
    # wider; unscaled, the legs over-swung and the robot spun at -15 deg/s).
    acts = {a["name"]: a for a in model["actuators"]}

    def span(part):
        name = next(o for o in outputs if o.endswith(f"_lf_{part}"))
        lo, hi = acts[name]["control_range"]
        return hi - lo
    coxa_scale = args.coxa_span_ref / span("coxa")
    femur_scale = args.femur_span_ref / span("femur")
    for i, leg in enumerate(LEGS):
        left = leg.startswith("l")
        x = {"f": 1.0, "m": 0.0, "r": -1.0}[leg[1]]
        y = 1.0 if left else -1.0
        sw = c.cell(f"{leg}_SW", "Leg:half_centre", leg, (x, y, 0.1))
        st = c.cell(f"{leg}_ST", "Leg:half_centre", leg, (x, y, -0.1))
        hi = c.cell(f"{leg}_HI", "Leg:detector_hi", leg, (x, y, 0.2))
        lo = c.cell(f"{leg}_LO", "Leg:detector_lo", leg, (x, y, -0.2))
        coxa = sensor(leg, "coxa")
        c.sensor(coxa, hi, w_sensor(2 * ANGLE_HALF_RANGE * k, DETECTOR_GAIN))
        c.sensor(coxa, lo, w_sensor(-2 * ANGLE_HALF_RANGE * k, DETECTOR_GAIN))
        # Left: protracted = coxa low; right: protracted = coxa high.
        pep, aep = (hi, lo) if left else (lo, hi)
        c.connect(pep, sw, w_graded(args.pep))
        c.connect(aep, st, w_graded(args.aep))
        c.connect(sw, st, w_graded(-args.mutual))
        c.connect(st, sw, w_graded(-args.mutual))
        foot = sensor(leg, "foot")
        c.sensor(foot, st, w_sensor(args.load))
        c.sensor(foot, sw, w_sensor(-args.load))
        # Motor: swing protracts + lifts; stance retracts + presses.
        sgn_pro = -1.0 if left else 1.0      # coxa activation change for protraction
        sgn_lift = 1.0 if left else -1.0     # femur activation change for lifting
        c.connect(sw, joint(leg, "coxa"), w_graded(sgn_pro * args.motor * coxa_scale))
        c.connect(st, joint(leg, "coxa"), w_graded(-sgn_pro * args.motor * coxa_scale))
        c.connect(sw, joint(leg, "femur"), w_graded(sgn_lift * args.lift * femur_scale))
        c.connect(st, joint(leg, "femur"), w_graded(-sgn_lift * args.press * femur_scale))
    for a, b in NEIGHBOURS:
        c.connect(f"{a}_SW", f"{b}_SW", w_graded(-args.inter))
        c.connect(f"{b}_SW", f"{a}_SW", w_graded(-args.inter))
    labels = {"design": "proprioceptive swing/stance half-centres (Cruse; Ekeberg 2004)",
              "parameters": vars(args) | {"output": str(args.output)}}
    c.write(Path(args.output), Path(args.config_out) if args.config_out else None,
            synaptic_gain=GAIN, depth=args.depth, labels=labels)
    return c


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--output", default="network_hexapod_reflex.json")
    ap.add_argument("--config-out", default=None)
    ap.add_argument("--depth", type=int, default=3)
    ap.add_argument("--tau", type=float, default=20.0, help="half-centre membrane tau (ms)")
    ap.add_argument("--plateau", type=float, default=22.0, help="half-centre plateau (mV); band ~0.5-1.6 mV/ms")
    ap.add_argument("--bias", type=float, default=1.0, help="half-centre tonic drive (mV/ms), inside the band")
    ap.add_argument("--detector-slope", type=float, default=60.0, help="detector drive per radian beyond the extreme (mV/ms/rad)")
    ap.add_argument("--extreme-angle", type=float, default=0.35, help="coxa angle of the anterior/posterior extreme (rad)")
    ap.add_argument("--pep", type=float, default=14.0, help="posterior extreme -> swing drive (mV/ms)")
    ap.add_argument("--aep", type=float, default=4.0, help="anterior extreme -> stance drive (mV/ms)")
    ap.add_argument("--mutual", type=float, default=3.0, help="swing<->stance inhibition (mV/ms)")
    ap.add_argument("--load", type=float, default=1.0, help="foot contact -> stance / -swing (mV/ms)")
    ap.add_argument("--inter", type=float, default=4.0, help="neighbouring swing inhibition (mV/ms)")
    ap.add_argument("--motor", type=float, default=6.0, help="coxa drive from swing/stance (mV/ms)")
    ap.add_argument("--lift", type=float, default=8.0, help="femur lift in swing (mV/ms)")
    ap.add_argument("--press", type=float, default=1.5, help="femur press in stance (mV/ms)")
    ap.add_argument("--coxa-span-ref", type=float, default=1.4, help="coxa range (rad) the drives were tuned for (v2 body)")
    ap.add_argument("--femur-span-ref", type=float, default=1.2, help="femur range (rad) the drives were tuned for (v2 body)")
    args = ap.parse_args()
    c = build(args)
    print(f"wrote {args.output}: {len(c.cells)} cells, {len(c.syn)} synapses, {len(c.sense)} sensory projections")


if __name__ == "__main__":
    main()
