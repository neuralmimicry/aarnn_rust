#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Parity of the Isaac runner's sensory port and ABI view against the Rust owner.

Runs under the pinned MuJoCo reference venv (no Isaac needed):

  target/robot-mujoco-venv/bin/python sim/isaacsim/tests/test_senses_parity.py \
      [--trace MUJOCO_TRACE.jsonl]

* vision Cell: the Rust unit-test vectors of crates/aarnn-embodiment/src/vision.rs;
* stateless projections: replay a native ``aarnn-mujoco --trace`` file,
  re-pose MuJoCo at each recorded qpos and compare the Python port with the
  Rust-submitted sensor vector (joint angles exactly; contacts/ranges, which
  the Rust owner reads from the pose one step earlier, by agreement rate);
* ABI: open an unbound session through ctypes and check the split
  apply/submit clock and silent outputs.
"""
import argparse
import json
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, str(HERE.parent))
import senses  # noqa: E402

GENERATED = ROOT / 'sim/content/embodiments/generated'


def test_cell_vectors():
    c = senses.Cell()
    assert c.sample(0.0, 0.002)[:2] == [0.0, 0.0]
    assert c.sample(1.0, 0.002)[:2] == [1.0, 0.0]
    assert c.sample(1.0, 0.002)[:2] == [0.0, 0.0]
    dark = senses.Cell()
    dark.sample(1.0, 0.002)
    assert dark.sample(0.0, 0.002)[:2] == [0.0, 1.0]
    try:
        dark.sample(float('nan'), 0.002)
    except ValueError:
        pass
    else:
        raise AssertionError('NaN luminance accepted')


def test_servo_speed_vectors():
    import run_isaacsim as runner
    # crates/aarnn-embodiment/src/motor.rs motor_speed_saturation_keeps_braking_and_passive_motion
    assert not runner.speed_allows(1., 0., 8., 7.5)
    assert runner.speed_allows(-1., 0., 8., 7.5)
    assert not runner.speed_allows(-1., 0., -8., 7.5)
    assert runner.speed_allows(1., 0., -8., 7.5)
    assert not runner.speed_allows(float('nan'), 0., 0., 7.5)


def mujoco_state(geometry, record):
    """Build a senses.State from MuJoCo data (contacts from mj_forward)."""
    import mujoco
    m, d = geometry.m, geometry.d
    mujoco.mj_forward(m, d)
    nbody = m.nbody
    contact = np.zeros(nbody, dtype=bool)
    points = {}
    for i in range(d.ncon):
        c = d.contact[i]
        for g, other in ((c.geom1, c.geom2), (c.geom2, c.geom1)):
            if g >= 0:
                b = int(m.geom_bodyid[g])
                contact[b] = True
                points.setdefault(b, []).append((c.pos.copy(), geometry.geom_name(other)))
    return senses.State(qpos=d.qpos.copy(), qvel=d.qvel.copy(), body_pos=d.xpos.copy(),
                        body_rot=d.xmat.reshape(-1, 3, 3).copy(), com_vel=np.zeros((nbody, 6)),
                        proper_acc=np.zeros((nbody, 3)), contact=contact, contact_points=points)


def test_trace_replay(trace):
    lines = Path(trace).read_text().splitlines()
    header = json.loads(lines[0])
    profile = header['profile']
    record = json.loads((GENERATED / f'{profile}.json').read_text())
    geometry = senses.Geometry(GENERATED / f'{profile}.xml')
    port = senses.Senses(record, geometry)
    kinds = [s['kind'] for s in record['io']['sensors']]
    angle = [i for i, k in enumerate(kinds) if k == 'legacy-angle']
    ranges = [i for i, k in enumerate(kinds) if k == 'legacy-range']
    contacts = [i for i, k in enumerate(kinds) if k == 'legacy-contact']
    worst, compared = 0.0, 0
    agree = {'range': [0, 0], 'contact': [0, 0]}
    for step, line in enumerate(lines[1:]):
        r = json.loads(line)
        if r.get('sensor_source') != 'submitted' or not r.get('sensors'):
            continue
        geometry.d.qpos[:] = r['qpos']
        geometry.d.qvel[:] = 0
        st = mujoco_state(geometry, record)
        values = port.measure(st, step)
        for i in angle:
            worst = max(worst, abs(values[i] - r['sensors'][i]))
        # The Rust owner reads xpos/contacts left by mj_step, i.e. the pose
        # one 2 ms step before the recorded qpos. Lightly loaded feet make and
        # break MuJoCo contact between steps, so contacts are compared by rate.
        for key, columns in (('range', ranges), ('contact', contacts)):
            for i in columns:
                agree[key][0] += abs(values[i] - r['sensors'][i]) < 0.02
                agree[key][1] += 1
        compared += 1
    assert compared > 10, 'trace has too few submitted samples'
    assert worst < 1e-12, f'joint-angle projection differs from Rust owner by {worst}'
    rates = {k: (a / n if n else None) for k, (a, n) in agree.items()}
    if rates['range'] is not None:
        assert rates['range'] > 0.99, f"range agreement only {rates['range']:.3f}"
    if rates['contact'] is not None:
        assert rates['contact'] > 0.8, f"contact agreement only {rates['contact']:.3f}"
    return dict(profile=profile, samples=compared, angle_channels=len(angle), angle_max_abs_error=worst,
                range_agreement=rates['range'], contact_agreement_one_step_lag=rates['contact'])


def test_abi_split_clock():
    sys.path.insert(0, str(HERE.parent))
    import embodiment_ffi as ffi
    record = json.loads((GENERATED / 'hexapod.json').read_text())
    n_s, n_a = len(record['io']['sensors']), len(record['actuators'])
    session = ffi.Session(GENERATED / 'hexapod.xml', None, control='silent')
    try:
        assert session.submit(0, np.full(n_s, 0.5)) is False
        code, controls = session.apply(0, np.full(n_s, 0.5), n_a + 2)
        assert code == 0 and not controls.any()
        assert not session.motor_mask(n_a).any()
        try:
            session.apply(0, np.full(n_s, 0.5), n_a + 2)
        except RuntimeError:
            pass
        else:
            raise AssertionError('replayed step accepted')
        status = session.status()
        assert len(status['applied_spikes']) == len(record['io']['output_names'])
        assert len(status['motor_rates']) == n_a
    finally:
        session.close()


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--trace', help='native aarnn-mujoco trace (hexapod) to replay')
    args = ap.parse_args()
    test_cell_vectors()
    print('vision cell vectors: ok')
    test_servo_speed_vectors()
    print('servo speed saturation vectors: ok')
    test_abi_split_clock()
    print('ABI split apply/submit clock: ok')
    if args.trace:
        print('trace replay:', json.dumps(test_trace_replay(args.trace)))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
