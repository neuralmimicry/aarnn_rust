#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Standing and balance circuit for the NAO (OEM mechanics, v2 surrogate).

NAO's joints are position servos: each output channel's pulse density r sets
its joint target lo + r (hi - lo). The circuit, in exact cells:

* posture: one graded output cell per joint whose tonic holds NAO's
  StandInit posture (knees bent, slight forward lean, arms down);
* vestibular ankle strategy: the accelerometer's body-axis gravity
  components (pitch: x, roll: y) reach excitation-only relays (30 ms, so
  the ~250 Hz pulse ripple is smoothed before any gain) centred on the
  standing operating point; a lean shifts both ankles' targets against it
  (pitch -> AnklePitch, roll -> AnkleRoll), the strategy people use for
  quiet-stance sway;
* rate damping: the gyro pitch/roll rates through relays centred on zero add
  a derivative term to the same ankle targets;
* hip strategy (optional, off by default): trunk pitch also moves HipPitch.

Status (2026-11-22): posture alone stands the OEM NAO for 20 s (trunk lean
6.4 +- 0.15 deg). The vestibular terms are off by default: every tested gain
(ankle 0.1-0.5 rad/rad, rate 0.03-0.08 rad per rad/s, relays 30-100 ms) did
worse than none - the accelerometer is dominated by contact vibration (+-5
m/s2 at a steady lean) and the relays' warm-up offsets the targets for the
first ~0.3 s, so the reflex either over-corrects the wake-up transient or
adds sway. Needs warm-up-matched offsets (a reference relay, as the crow
builder's V_ref) and gyro/otolith fusion before it helps.

Output density equals the graded release curve in exact cells (measured:
within 0.005 for tonics 0.5-30 mV/ms), so a target angle is set by
``release_tonic`` and corrections by the curve's local slope.

  python3 scripts/build_nao_stand_network_json.py --output network_nao_stand.json
"""
import argparse
import json
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aarnn_circuit import Circuit, add_sensor_relay, drive_weight, graded, release_tonic, steady_release  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
GAIN = 100.0
G = 9.81
ACCEL_RANGE = 20.0     # legacy-accel: [-20, 20] m/s2 -> [0, 1]
GYRO_RANGE = 10.0      # legacy-gyro: [-10, 10] rad/s -> [0, 1]
# NAO StandInit (Aldebaran posture library), rad; right-side rolls mirrored.
STAND_INIT = dict(HeadYaw=0.0, HeadPitch=0.0, HipYawPitch=0.0, HipRoll=0.0, HipPitch=-0.45, KneePitch=0.70,
                  AnklePitch=-0.35, AnkleRoll=0.0, ShoulderPitch=1.40, ShoulderRoll=0.25, ElbowYaw=-1.20,
                  ElbowRoll=-1.00, WristYaw=0.0)
MIRRORED = ("ShoulderRoll", "ElbowYaw", "ElbowRoll", "HipRoll", "AnkleRoll")


def posture_angle(joint):
    if "Phalanx" in joint:
        return 0.3
    key = joint[1:] if joint[0] in "LR" and joint[1:] in STAND_INIT else joint
    v = STAND_INIT[key]
    return -v if joint[0] == "R" and key in MIRRORED else v


def release_slope(r):
    """d(release)/d(input current) at release r (mV/ms)^-1."""
    i = release_tonic(min(max(r, 0.02), 0.98))
    return (steady_release(i + 0.05) - steady_release(i - 0.05)) / 0.1


def build(args):
    model = json.loads((ROOT / "sim/content/embodiments/generated/nao.json").read_text())
    acts = {a["name"]: a for a in model["actuators"]}
    c = Circuit.for_embodiment("nao", exact_cells=True)
    common = dict(nmda_ratio=0.0, synaptic_gain=GAIN)

    def w(mv):
        return drive_weight(mv, GAIN)
    # Reference lean of StandInit (measured on the OEM model: 6.4 deg forward).
    pitch0 = math.radians(args.lean_deg)
    s_pitch = 0.5 + (-G * math.sin(pitch0)) / (2 * ACCEL_RANGE)   # accel.x = -g sin(pitch)
    s_roll = 0.5
    relays = {}
    if args.ankle_gain > 0 or args.hip_gain > 0:
        relays["pitch"] = add_sensor_relay(c, GAIN, "accelerometer.x", s_pitch, tau_ms=args.relay_tau)
        relays["roll"] = add_sensor_relay(c, GAIN, "accelerometer.y", s_roll, tau_ms=args.relay_tau)
    if args.rate_gain > 0:
        relays["pitch_rate"] = add_sensor_relay(c, GAIN, "gyro.y", 0.5, tau_ms=args.relay_tau)
        relays["roll_rate"] = add_sensor_relay(c, GAIN, "gyro.x", 0.5, tau_ms=args.relay_tau)
    # Sensor units per physical unit near the operating point.
    per_rad = {"pitch": -G * math.cos(pitch0) / (2 * ACCEL_RANGE), "roll": G / (2 * ACCEL_RANGE)}
    per_rad_s = 1.0 / (2 * GYRO_RANGE)
    # Corrections (target rad per rad of lean / per rad/s of lean rate).
    # Lean forward (+pitch) -> AnklePitch + (toes press down, body pushed back);
    # lean right (+roll about x) -> AnkleRoll + on both feet.
    gains = {}
    for side in "LR":
        gains[f"{side}AnklePitch"] = [("pitch", args.ankle_gain, per_rad["pitch"]),
                                      ("pitch_rate", args.rate_gain, per_rad_s)]
        gains[f"{side}AnkleRoll"] = [("roll", args.ankle_gain * args.roll_ratio, per_rad["roll"]),
                                     ("roll_rate", args.rate_gain * args.roll_ratio, per_rad_s)]
        gains[f"{side}HipPitch"] = [("pitch", args.hip_gain, per_rad["pitch"])]
    for o in c.outputs:
        lo, hi = acts[o]["control_range"]
        q = min(max(posture_angle(o), lo), hi)
        r = (q - lo) / (hi - lo)
        tonic = release_tonic(r)
        slope = release_slope(r)                       # release per mV/ms
        terms = []
        for key, gain, units in gains.get(o, []):
            if gain <= 0 or key not in relays:
                continue
            cell, r0, s_relay = relays[key]
            # target change per relay release unit -> current per release unit
            d_release_per_relay = gain / (units * s_relay) / (hi - lo)
            k = d_release_per_relay / slope            # mV/ms per relay release
            terms.append((cell, k))
            tonic -= k * r0                            # cancel the relay's operating release
        c.type(f"Posture:{o}", **graded(tau_ms=args.out_tau, release_hz=500.0, tonic=tonic), **common)
        c.output(o, f"Posture:{o}")
        for cell, k in terms:
            c.connect(cell, o, w(k))
    labels = {"design": "NAO StandInit posture + vestibular ankle strategy (exact cells)", "parameters": vars(args)}
    c.write(Path(args.output), Path(args.config_out), synaptic_gain=GAIN, depth=0, labels=labels)
    return c


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--output", default="network_nao_stand.json")
    ap.add_argument("--config-out", default="webots_world/configs/config_nao_stand.json")
    ap.add_argument("--lean-deg", type=float, default=6.4, help="StandInit trunk lean (deg, measured on the OEM model)")
    ap.add_argument("--ankle-gain", type=float, default=0.0, help="ankle target rad per rad of lean (0 = off: see docstring)")
    ap.add_argument("--roll-ratio", type=float, default=1.0)
    ap.add_argument("--rate-gain", type=float, default=0.0, help="ankle target rad per rad/s of lean rate (0 = off)")
    ap.add_argument("--hip-gain", type=float, default=0.0, help="HipPitch rad per rad of trunk pitch")
    ap.add_argument("--relay-tau", type=float, default=30.0)
    ap.add_argument("--out-tau", type=float, default=15.0)
    args = ap.parse_args()
    c = build(args)
    print(f"wrote {args.output}: {len(c.cells)} cells, {len(c.syn)} synapses, {len(c.sense)} sensory projections")


if __name__ == "__main__":
    main()
