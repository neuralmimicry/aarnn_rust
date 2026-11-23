#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Programmed flight proof for the Drosophila body (fly mechanics v3).

Runs the generated fly (``sim/content/embodiments/generated/<profile>.xml``)
in MuJoCo with exact Python ports of the body-side models the native adapter
applies: the ``resonant-flight-v1`` motor (``aarnn_embodiment::flight``),
the wing blades' ``quasi-steady-panel-v2`` panels
(``aarnn_embodiment::force::polar_panel_force``) and the legs' position
servos. A programmed controller stands in for the brain and writes the five
flight commands (power, amplitude_left/right, centre_left/right) through the
same 20 ms decoder as a graded brain:

* ``--controller sensor`` (default) sees only what the brain sees, as the
  body encodes it: the haltere-like gyro (+-10 rad/s), the accelerometer
  (+-20 m/s^2, which in flight senses drag, not tilt) and the ventral range
  ray (0.3 m), each through first-order cell and synapse lags (~45 ms in
  all), one 2 ms transport frame and the decoder. Roll and pitch: rate plus
  leaky-integrated rate (an attitude estimate); drift: accelerometer drag;
  height: ventral ray; yaw: rate damping; wall avoidance: the forward range
  rays turn the fly (differential stroke centre);
* ``--controller ideal``: true attitude and velocity, no lag (reference).

The fly stands, the motor is recruited at ``--takeoff`` s, and it flies for
the rest of ``--seconds``. Writes an ``aarnn-mujoco-trace-v2`` trace (for
``probe_embodiment_motion.py`` and ``render_trace_video.py``) and prints the
flight metrics. Run with the pinned MuJoCo environment:

  target/robot-mujoco-venv/bin/python scripts/qa/fly_flight_proof.py \\
      --seconds 12 --trace target/qa/stage4/fly-flight-proof/trace.jsonl
"""
import argparse
import json
import math
from pathlib import Path

import mujoco
import numpy as np

ROOT = Path(__file__).resolve().parents[2]
GENERATED = ROOT / 'sim/content/embodiments/generated'
RATE_TAU_S = 0.02  # aarnn_embodiment::motor::RATE_TAU_S


def polar_panel_force(v, R, area, density, polar):
    """Exact port of aarnn_embodiment::force::polar_panel_force."""
    slope, lift_max, drag_zero, drag_induced, drag_stall = polar
    speed = float(np.linalg.norm(v))
    if speed < 1e-9:
        return np.zeros(3)
    d = v / speed
    local = R.T @ v
    alpha = math.atan2(-local[2], abs(local[0]) + 1e-12)
    cl = max(-lift_max, min(lift_max, slope * alpha)) * max(math.cos(alpha), 0.0)
    stall = lift_max / slope if slope > 0 else math.inf
    cd = drag_zero + drag_induced * cl * cl + drag_stall * math.sin(max(abs(alpha) - stall, 0.0)) ** 2
    n = R[:, 2]
    lift = n - d * (n @ d)
    mag = float(np.linalg.norm(lift))
    lift = lift / mag if mag > 1e-9 else np.zeros(3)
    return 0.5 * density * speed * speed * area * (cl * lift - cd * d)


class Motor:
    """Exact port of aarnn_embodiment::flight::Flight (resonant-flight-v1)."""

    def __init__(self, f):
        self.f = f

    def recruitment(self, power):
        p0, p1 = self.f['recruitment']
        x = min(1.0, max(0.0, (power - p0) / (p1 - p0)))
        return x * x * (3 - 2 * x)

    def targets(self, cmd):
        f = self.f
        g = self.recruitment(min(1.0, max(0.0, cmd[0])))
        return [(g * f['amplitude_max_rad'] * min(1.0, max(0.0, cmd[1 + s])),
                 (1 - g) * f['fold_rad'] + g * f['centre_max_rad'] * min(1.0, max(-1.0, cmd[3 + s]))) for s in range(2)]

    def torques(self, t, cmd, wings):
        f = self.f
        targets = self.targets(cmd)
        g = self.recruitment(min(1.0, max(0.0, cmd[0])))
        omega = 2 * math.pi * f['wingbeat_hz']
        phase = omega * t
        limit = f['drive_torque_limit_n_m'] * g
        out = []
        for s in range(2):
            amp, centre = targets[s]
            (q, _), (theta, _) = wings[s]
            spring = f['stroke_stiffness_n_m_per_rad'] * (centre - q)
            ref = centre + amp * math.cos(phase)
            ref_rate = -amp * omega * math.sin(phase)
            drive = f['drive_stiffness_n_m_per_rad'] * (ref - q) + f['drive_damping_n_m_s_per_rad'] * ref_rate
            drive = max(-limit, min(limit, drive))
            lead_rate = -amp * omega * math.sin(phase + f['hinge_lead_rad'])
            hinge = g * f['hinge_angle_rad'] * math.tanh(lead_rate / f['hinge_rate_rad_s'])
            out.append((spring + drive, f['hinge_stiffness_n_m_per_rad'] * (hinge - theta)))
        return out


class Lag:
    def __init__(self, tau_ms):
        self.k = 1 - math.exp(-2.0 / tau_ms) if tau_ms > 0 else 1.0
        self.y = None

    def __call__(self, x):
        x = np.asarray(x, dtype=float)
        self.y = x if self.y is None else self.y + self.k * (x - self.y)
        return self.y


class Fly:
    def __init__(self, profile):
        self.record = json.loads((GENERATED / f'{profile}.json').read_text())
        self.m = mujoco.MjModel.from_xml_path(str(GENERATED / f'{profile}.xml'))
        self.d = mujoco.MjData(self.m)
        m = self.m
        self.motor = Motor(self.record['flight'])
        jid = lambda n: mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_JOINT, n)  # noqa: E731
        self.wings = [(jid(w['stroke_joint']), jid(w['pitch_joint'])) for w in self.record['flight']['wings']]
        aero = self.record['scene']['aerodynamics']
        self.density = aero['density_kg_m3']
        self.panels = [(mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_BODY, p['body']), np.array(p['point_m']), p['area_m2'],
                        [aero['polars'][p['polar']][k] for k in ('lift_slope_per_rad', 'lift_max', 'drag_zero', 'drag_induced', 'drag_stall')])
                       for p in aero['panels']]
        self.acts = self.record['actuators']
        self.thorax = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_BODY, 'thorax')
        self.rates = np.zeros(len(self.acts) + 5)   # decoded actuator + flight command rates
        mujoco.mj_forward(m, self.d)

    def step(self, leg_rates, commands):
        """One 2 ms step: decode (20 ms filter), legs, flight motor, panels."""
        m, d = self.m, self.d
        keep = np.array([math.exp(-0.002 / RATE_TAU_S)] * len(self.acts)
                        + [math.exp(-0.002 / c.get('decode_tau_s', RATE_TAU_S)) for c in self.record['flight']['commands']])
        target = np.concatenate([leg_rates, commands])
        self.rates = np.clip(self.rates * keep + (1 - keep) * target, -1, 1)
        for i, a in enumerate(self.acts):
            lo, hi = a['control_range']
            d.ctrl[i] = min(hi, max(lo, (lo + hi) / 2 + self.rates[i] * (hi - lo) / 2))
        d.qfrc_applied[:] = 0
        cmd = self.rates[len(self.acts):]
        state = [((d.qpos[m.jnt_qposadr[s]], d.qvel[m.jnt_dofadr[s]]), (d.qpos[m.jnt_qposadr[p]], d.qvel[m.jnt_dofadr[p]]))
                 for s, p in self.wings]
        for (s, p), (ts, tp) in zip(self.wings, self.motor.torques(d.time, cmd, state)):
            d.qfrc_applied[m.jnt_dofadr[s]] += ts
            d.qfrc_applied[m.jnt_dofadr[p]] += tp
        for body, local, area, polar in self.panels:
            R = d.xmat[body].reshape(3, 3)
            point = d.xpos[body] + R @ local
            vel = np.zeros(6)
            mujoco.mj_objectVelocity(m, d, mujoco.mjtObj.mjOBJ_BODY, body, vel, 0)
            v = vel[3:] + np.cross(vel[:3], point - d.xipos[body])
            F = polar_panel_force(v, R, area, self.density, polar)
            mujoco.mj_applyFT(m, d, F, np.zeros(3), point, body, d.qfrc_applied)
        mujoco.mj_step(m, d)

    def ray_beyond_self(self, point, direction, reach):
        """Distance to the first non-fly geom (as the native legacy range)."""
        m, d = self.m, self.d
        geom = np.zeros(1, dtype=np.int32)
        start, travelled = np.array(point, dtype=float), 0.0
        for _ in range(16):
            dist = mujoco.mj_ray(m, d, start, direction, None, 1, -1, geom)
            if dist < 0 or travelled + dist > reach:
                return reach
            if m.body_rootid[m.geom_bodyid[geom[0]]] == self.thorax:
                travelled += dist + 1e-4
                start = start + direction * (dist + 1e-4)
                continue
            return travelled + dist
        return reach

    def sensors(self):
        """Gyro, accelerometer (body frame, legacy encodings) and range rays."""
        m, d = self.m, self.d
        R = d.xmat[self.thorax].reshape(3, 3)
        vel = np.zeros(6)
        mujoco.mj_objectVelocity(m, d, mujoco.mjtObj.mjOBJ_BODY, self.thorax, vel, 0)
        gyro = R.T @ vel[:3]
        acc = np.zeros(6)
        mujoco.mj_rnePostConstraint(m, d)
        mujoco.mj_objectAcceleration(m, d, mujoco.mjtObj.mjOBJ_BODY, self.thorax, acc, 0)
        accel = R.T @ acc[3:]
        rays = {}
        for s in self.record['io']['sensors']:
            if s['kind'] == 'legacy-range':
                direction = R @ np.array(s['direction'], dtype=float)
                rays[s['name']] = 1 - self.ray_beyond_self(d.xpos[self.thorax], direction, s['angle_rad']) / s['angle_rad']
        return dict(gyro=np.clip((gyro + 10) / 20, 0, 1), accel=np.clip((accel + 20) / 40, 0, 1), rays=rays)


def attitude(d, body=1):
    R = d.xmat[body].reshape(3, 3)
    return math.atan2(R[2, 1], R[2, 2]), math.asin(max(-1, min(1, -R[2, 0]))), math.atan2(R[1, 0], R[0, 0])


class SensorController:
    """Programmed stand-in for the brain, on brain-visible sensors only."""

    def __init__(self, fly, a):
        self.a = a
        self.fly = fly
        self.relay = Lag(a.tau_relay)
        self.height_relay = Lag(a.tau_height)
        self.syn = Lag(5)
        self.integ = Lag(a.tau_int)
        self.drift = Lag(a.tau_drift)
        self.height_slow = Lag(a.tau_height_rate)
        self.out = [Lag(5), Lag(5), Lag(5)]
        from collections import deque
        self.history = deque(maxlen=1 + max(1, int(round(a.delay_ms / 2.0))))   # transport + extra pure delay
        names = [s['name'] for s in fly.record['io']['sensors'] if s['kind'] == 'legacy-range']
        self.ventral = 'dros_s_22_prox_12'
        dirs = {s['name']: s['direction'] for s in fly.record['io']['sensors'] if s['kind'] == 'legacy-range'}
        self.left = [n for n in names if n != self.ventral and dirs[n][0] > 0.3 and dirs[n][1] > 0.1]
        self.right = [n for n in names if n != self.ventral and dirs[n][0] > 0.3 and dirs[n][1] < -0.1]
        self.ahead = [n for n in names if n != self.ventral and dirs[n][0] > 0.95]

    def __call__(self, t):
        a = self.a
        s = self.fly.sensors()
        x = np.concatenate([s['gyro'], s['accel'],
                            [s['rays'][self.ventral], max(s['rays'][n] for n in self.left), max(s['rays'][n] for n in self.right),
                             max(s['rays'][n] for n in self.ahead)]])
        self.history.append(x)
        seen = self.history[0]
        r = self.syn(self.relay(seen))
        wx, wy, wz = (r[0:3] - 0.5) * 20                      # gyro: roll, pitch, yaw rate
        ax, ay = (r[3:5] - 0.5) * 40
        height = (1 - self.height_relay(seen[6])) * 0.3
        integ = self.integ(np.array([wx, wy])) * a.tau_int / 1000.0
        drift = self.drift(np.array([ax, ay]))
        roll_cmd = -(a.k_roll * wx + a.k_roll_int * integ[0]) - a.k_drift_y * drift[1]
        pitch_cmd = -(a.k_pitch * wy + a.k_pitch_int * integ[1]) + a.k_drift_x * drift[0] + a.pitch_trim
        # Measured (fixed body): +dc (left stroke back, right forward) yaws
        # right. Turning left (w_z > 0) is damped by +dc; a wall on the
        # right turns the fly left (-dc), a wall ahead turns it right.
        turn = a.k_wall * (r[7] - r[8]) + (a.k_wall * 0.5 if r[9] > 0.05 else 0.0)
        yaw_cmd = a.k_yaw * wz + turn
        climb = (height - self.height_slow(height)) / (a.tau_height_rate / 1000.0)   # transient: height rate
        lift = a.amplitude * (1 + np.clip(a.k_height * (a.height - height) - a.k_climb * climb, -0.3, 0.3))
        asym = float(np.clip(roll_cmd, -0.4, 0.4))
        c = float(np.clip(pitch_cmd, -0.5, 0.5))
        dc = float(np.clip(yaw_cmd, -0.3, 0.3))
        raw = np.array([lift * (1 + asym), lift * (1 - asym), c])
        for lag in self.out:
            raw = lag(raw)
        A_max, c_max = self.fly.record['flight']['amplitude_max_rad'], self.fly.record['flight']['centre_max_rad']
        return [raw[0] / A_max, raw[1] / A_max, (raw[2] + dc) / c_max, (raw[2] - dc) / c_max]


class IdealController:
    def __init__(self, fly, a):
        self.fly, self.a = fly, a

    def __call__(self, t):
        a, d = self.a, self.fly.d
        roll, pitch, _ = attitude(d)
        R = d.xmat[1].reshape(3, 3)
        w = d.qvel[3:6]                  # free joint: body-frame angular velocity
        v = R.T @ d.qvel[0:3]
        asym = float(np.clip(-(0.02 * w[0] + 0.3 * (roll - 0.3 * v[1])), -0.4, 0.4))
        c = float(np.clip(-(0.02 * w[1] + 0.3 * (pitch + 0.3 * v[0])), -0.5, 0.5))
        lift = a.amplitude * (1 + np.clip(2 * (a.height - d.qpos[2]) - 0.5 * d.qvel[2], -0.3, 0.3))
        A_max, c_max = self.fly.record['flight']['amplitude_max_rad'], self.fly.record['flight']['centre_max_rad']
        return [lift * (1 + asym) / A_max, lift * (1 - asym) / A_max, c / c_max, c / c_max]


def leg_rates(fly, flying):
    """Stand at mid-range; in flight tuck the legs (femur lifted)."""
    rates = np.zeros(len(fly.acts))
    if flying:
        for i, a in enumerate(fly.acts):
            if a['joint'].endswith('_femur'):
                rates[i] = 0.8 if a['joint'][0] == 'l' else -0.8
    return rates


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--profile', default='drosophila_banc', choices=('drosophila_banc', 'drosophila_fafb'))
    ap.add_argument('--seconds', type=float, default=12.0)
    ap.add_argument('--takeoff', type=float, default=1.0)
    ap.add_argument('--controller', choices=('sensor', 'ideal'), default='sensor')
    ap.add_argument('--trace')
    ap.add_argument('--every', type=int, default=5)
    ap.add_argument('--amplitude', type=float, default=0.85, help='hover stroke amplitude (rad)')
    ap.add_argument('--height', type=float, default=0.15, help='ventral-ray height set point (m)')
    ap.add_argument('--k-height', type=float, default=1.0, help='fractional amplitude per m of height error')
    ap.add_argument('--k-climb', type=float, default=0.4, help='height-rate damping (per m/s)')
    ap.add_argument('--tau-height-rate', type=float, default=80.0)
    ap.add_argument('--tau-height', type=float, default=30.0, help='ventral-ray relay tau (ms)')
    ap.add_argument('--tau-relay', type=float, default=10.0)
    ap.add_argument('--tau-int', type=float, default=1500.0)
    ap.add_argument('--tau-drift', type=float, default=100.0)
    ap.add_argument('--k-roll', type=float, default=0.04)
    ap.add_argument('--k-roll-int', type=float, default=0.1)
    ap.add_argument('--k-pitch', type=float, default=0.01)
    ap.add_argument('--k-pitch-int', type=float, default=0.05)
    ap.add_argument('--k-drift-y', type=float, default=0.03)
    ap.add_argument('--k-drift-x', type=float, default=0.0)
    ap.add_argument('--pitch-trim', type=float, default=0.0, help='stroke centre trim (rad, + nose down: forward flight)')
    ap.add_argument('--k-yaw', type=float, default=0.01)
    ap.add_argument('--k-wall', type=float, default=0.0)
    ap.add_argument('--noise', type=float, default=0.0, help='white noise (std) on each decoded command input')
    ap.add_argument('--delay-ms', type=float, default=2.0, help='sensor-to-brain delay (2 ms = one transport frame)')
    ap.add_argument('--decode-tau-ms', type=float, default=0.0, help='override the flight commands\' decode tau (ms)')
    ap.add_argument('--seed', type=int, default=1)
    a = ap.parse_args()
    fly = Fly(a.profile)
    if a.decode_tau_ms:
        for c in fly.record['flight']['commands']:
            c['decode_tau_s'] = a.decode_tau_ms / 1000.0
    m, d = fly.m, fly.d
    ctl = (SensorController if a.controller == 'sensor' else IdealController)(fly, a)
    rows, out = [], None
    rng = np.random.default_rng(a.seed)
    if a.trace:
        Path(a.trace).parent.mkdir(parents=True, exist_ok=True)
        out = open(a.trace, 'w')
        out.write(json.dumps(dict(schema='aarnn-mujoco-trace-v2', profile=a.profile, trace_every_steps=a.every,
                                  actuated_joints=[x['joint'] for x in fly.acts] + ['left_wing_flap', 'right_wing_flap'],
                                  source='scripts/qa/fly_flight_proof.py', controller=a.controller)) + '\n')
    wing_q = []
    steps = int(round(a.seconds / 0.002))
    for k in range(steps):
        t = d.time
        flying = t >= a.takeoff
        commands = [0.0] * 5
        if flying:
            amp_l, amp_r, c_l, c_r = ctl(t)
            commands = [1.0, amp_l, amp_r, c_l, c_r]
            if a.noise > 0:   # independent per-channel command noise (robustness)
                commands = list(np.array(commands) + rng.normal(0, a.noise, 5) * np.array([0, 1, 1, 1, 1]))
        else:
            ctl(t) if a.controller == 'sensor' else None
        fly.step(leg_rates(fly, flying), commands)
        sq = d.qpos[m.jnt_qposadr[fly.wings[0][0]]]
        wing_q.append(sq)
        roll, pitch, yaw = attitude(d)
        rows.append((d.time, *d.qpos[:3], roll, pitch, yaw))
        if out and k % a.every == 0:
            joints = [float(d.qpos[m.jnt_qposadr[mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_JOINT, j)]])
                      for j in [x['joint'] for x in fly.acts] + ['left_wing_flap', 'right_wing_flap']]
            out.write(json.dumps(dict(step=k + 1, time_s=d.time, body_xyz=d.xpos[1:].tolist(), joint_q=joints, qpos=d.qpos.tolist())) + '\n')
    if out:
        out.close()
    rows = np.array(rows)
    t, z = rows[:, 0], rows[:, 3]
    stand = float(np.median(z[(t > 0.5) & (t < a.takeoff)])) if a.takeoff > 0.6 else float(z[0])
    air = (t >= a.takeoff) & (z > stand + 0.02)
    upright = np.degrees(np.maximum(np.abs(rows[:, 4]), np.abs(rows[:, 5])))
    after = t >= a.takeoff + 0.5
    lost = np.nonzero(after & (upright > 60))[0]
    q = np.array(wing_q)[t >= a.takeoff + 0.5]
    spec = np.abs(np.fft.rfft(q - q.mean())) if len(q) > 16 else np.zeros(1)
    freqs = np.fft.rfftfreq(len(q), 0.002) if len(q) > 16 else np.zeros(1)
    path = float(np.linalg.norm(np.diff(rows[air][:, 1:3], axis=0), axis=1).sum()) if air.sum() > 1 else 0.0
    res = dict(profile=a.profile, controller=a.controller, seconds=a.seconds, takeoff_s=a.takeoff,
               mass_kg=float(sum(m.body_mass)), standing_height_m=round(stand, 4),
               time_aloft_s=round(float(air.sum() * 0.002), 2),
               first_liftoff_s=round(float(t[air][0]), 3) if air.any() else None,
               mean_height_aloft_m=round(float(z[air].mean()), 3) if air.any() else None,
               max_height_m=round(float(z.max()), 3),
               flight_path_m=round(path, 3), net_displacement_m=round(float(np.hypot(*(rows[-1, 1:3] - rows[0, 1:3]))), 3),
               attitude_rms_deg=round(float(np.sqrt(np.mean(upright[after] ** 2))), 1) if after.any() else None,
               attitude_max_deg=round(float(upright[after].max()), 1) if after.any() else None,
               lost_attitude_s=round(float(t[lost[0]]), 3) if len(lost) else None,
               wingbeat_hz=round(float(freqs[int(np.argmax(spec[1:])) + 1]), 2) if len(q) > 16 else None,
               stroke_amplitude_rad=round(float(np.percentile(np.abs(q - q.mean()), 98)), 3) if len(q) else None)
    print(json.dumps(res))


if __name__ == '__main__':
    main()
