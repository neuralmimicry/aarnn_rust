#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Closed-loop C. elegans AARNN -> MuJoCo behaviour evidence.

Runs ``network_celegans.json`` through the managed launcher (real cluster
worker, TCP/IPC bridge, named 48/96 contract: 24 exteroceptive/inertial and 24
proprioceptive inputs, muscle response v3, no programmed twitch or gait) and a
same-binary passive ``silent`` control. A
read-only native trace measures four declared claims:

* sense   - physical measurements vary and reach the bound brain;
* wriggle - neural muscle output produces alternating dorsoventral bending;
* move    - the body centroid travels well beyond the passive control;
* explore - the head visits new ground, turns and meets new sensory states.

Thresholds are fixed below before any run. This is engineering closed-loop
evidence only: it is not biological calibration, a learned task, Webots parity
or a production commit/effect claim. Uses private ports and task-owned
processes; preserved sources are hashed before and after.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import signal
import socket
import subprocess
import time

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
DT = 0.002
BODY_LENGTH_M = 0.34
CELL_M = 0.02
THRESHOLDS = {
    'sense_min_varying_channels': 8,
    'sense_min_exteroceptive_varying': 3,
    'wriggle_min_joint_fraction': 0.5,
    'wriggle_min_amplitude_rad': 0.02,
    'wriggle_min_reversals': 4,
    'move_min_net_body_lengths': 0.25,
    'move_min_ratio_to_passive': 10.0,
    'explore_min_head_cells_ratio_to_passive': 3.0,
    'explore_min_total_turning_rad': math.pi,
    'explore_min_sensory_states_ratio_to_passive': 2.0,
}
SOURCES = ('network_celegans.json', 'webots_world/configs/config_celegans_mujoco.json',
           'sim/content/embodiments/generated/celegans.json',
           'sim/content/embodiments/generated/celegans.xml')


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def free_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def run(command, log, timeout, env):
    with log.open('w') as handle:
        process = subprocess.Popen(command, cwd=ROOT, env=env, stdout=handle,
                                   stderr=subprocess.STDOUT, start_new_session=True)
        try:
            process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
            raise TimeoutError(f'{command[:4]} exceeded {timeout}s; see {log}')
    if process.returncode != 0:
        raise AssertionError(f'exit {process.returncode}; see {log}')


def simulate(output, steps, every, nodes, startup, pacing, timeout, network=None, config=None):
    env = dict(os.environ, NM_DISABLE_OPENCL='1', RAYON_NUM_THREADS=os.environ.get('RAYON_NUM_THREADS', '2'))
    sdk = Path(env.get('AARNN_MUJOCO_DIR', str(Path.home() / 'Developer/mujoco-3.17.0')))
    env.update(AARNN_MUJOCO_DIR=str(sdk),
               LD_LIBRARY_PATH=str(sdk / 'lib') + (':' + env['LD_LIBRARY_PATH'] if env.get('LD_LIBRARY_PATH') else ''))
    runs = {}
    tcp_port = free_port()
    neural = ['bash', 'scripts/run_sim.sh', '--sim', 'mujoco', '--robots', 'celegans=1', '--no-build',
              '--tcp-base-port', str(tcp_port), '--tcp-ready-timeout', str(startup),
              '--headless', '--steps', str(steps), '--' + pacing,
              '--output', str(output / 'neural.json'), '--trace', str(output / 'trace-neural'),
              '--trace-every', str(every)]
    if nodes > 1:
        neural += ['--nodes', str(nodes)]
    # Candidate snapshots are selected per brain; preserved files stay untouched.
    if network:
        neural += ['--network-map', f'celegans_0={network}']
    if config:
        neural += ['--config-map', f'celegans_0={config}']
    run(neural, output / 'neural.log', startup + timeout, dict(env, NM_CLUSTER_NODES=str(nodes)))
    text = (output / 'neural.log').read_text()
    runtime = Path(re.search(r'MuJoCo logs and evidence: (.+)', text)[1])
    worker = (runtime / 'webots_celegans_0.log').read_text()
    with socket.socket() as sock:
        assert sock.connect_ex(('127.0.0.1', tcp_port)) != 0, 'bridge leaked'
    runs['neural'] = dict(command=neural, runtime=str(runtime), pacing=pacing,
                          worker_consumed_sensory_frames=worker.count('consumed AER sensory frame network=celegans_0'),
                          worker_cpu_reference='transition=CPU_reference' in worker,
                          start_reached_workers=f'Start reached all {nodes} distributed worker(s)' in text)
    # Same native binary, same model and step budget; no brain and no NPC drive.
    silent = ['bash', 'scripts/run_sim.sh', '--sim', 'mujoco', '--robots', 'celegans=1', '--no-build',
              '--control', 'silent', '--headless', '--steps', str(steps),
              '--output', str(output / 'silent.json'), '--trace', str(output / 'trace-silent'),
              '--trace-every', str(every)]
    run(silent, output / 'silent.log', 600, env)
    runs['silent'] = dict(command=silent)
    return runs


def load(path):
    lines = path.read_text().splitlines()
    header = json.loads(lines[0])
    records = [json.loads(line) for line in lines[1:]]
    if not records:
        raise AssertionError(f'{path}: no trace records')
    return header, records


def hysteresis_reversals(x, h):
    state, count = 0, 0
    for v in x:
        s = 1 if v > h else -1 if v < -h else 0
        if s and s != state:
            count += state != 0
            state = s
    return count


def analyse(path):
    header, records = load(path)
    ids = [i for _, i in sorted(header['segment_body_ids'])]
    if len(ids) != 24:
        raise AssertionError('C. elegans trace requires 24 named segments')
    xyz = np.array([r['body_xyz'] for r in records])[:, [i - 1 for i in ids], :]
    t = np.array([r['time_s'] for r in records])
    q = np.array([r['joint_q'] for r in records])
    names = header['sensor_names']
    sensed = [r for r in records if len(r['sensors']) == len(names)]
    s = np.array([r['sensors'] for r in sensed]) if sensed else np.zeros((0, len(names)))
    spikes = np.array([r['applied_spikes'] for r in records], dtype=float)
    outputs = header['output_names']
    centroid = xyz[:, :, :2].mean(axis=1)
    head = xyz[:, -1, :2]
    neck = xyz[:, 16, :2]
    steps = np.diff(centroid, axis=0)
    heading = np.unwrap(np.arctan2(*(head - neck).T[::-1]))
    # Body-frame bending: centred joint angles; hysteresis rejects solver jitter.
    centred = q - q.mean(axis=0)
    amplitude = centred.std(axis=0)
    reversals = np.array([hysteresis_reversals(centred[:, j], THRESHOLDS['wriggle_min_amplitude_rad'])
                          for j in range(q.shape[1])])
    wriggling = (amplitude >= THRESHOLDS['wriggle_min_amplitude_rad']) & (reversals >= THRESHOLDS['wriggle_min_reversals'])
    # Travelling wave: lag of peak cross-correlation between neighbouring joints.
    sample_dt = float(np.median(np.diff(t))) if len(t) > 1 else DT
    max_lag = max(1, int(2.0 / sample_dt))
    lags = []
    for j in range(q.shape[1] - 1):
        a, b = centred[:, j], centred[:, j + 1]
        if a.std() < 1e-9 or b.std() < 1e-9:
            continue
        a, b = (a - a.mean()) / a.std(), (b - b.mean()) / b.std()
        corr = [np.mean(a[max(0, -k):len(a) - max(0, k)] * b[max(0, k):len(b) - max(0, -k)])
                for k in range(-max_lag, max_lag + 1)]
        lags.append((int(np.argmax(corr)) - max_lag) * sample_dt)
    # Eigen-shape phase: rotation of the first two principal bending modes.
    cycles, pc2_fraction = 0.0, 0.0
    if q.shape[0] > 3 and np.any(amplitude > 0):
        _, sv, vt = np.linalg.svd(centred, full_matrices=False)
        modes = centred @ vt[:2].T
        pc2_fraction = float((sv[:2] ** 2).sum() / (sv ** 2).sum())
        cycles = float(abs(np.unwrap(np.arctan2(modes[:, 1], modes[:, 0]))[-1]
                           - np.arctan2(modes[0, 1], modes[0, 0])) / (2 * math.pi))
    dominant = None
    if len(t) > 16:
        spectrum = np.abs(np.fft.rfft(centred, axis=0)).sum(axis=1)
        frequency = np.fft.rfftfreq(centred.shape[0], sample_dt)
        band = (frequency > 0.05) & (frequency < 10)
        if band.any():
            dominant = float(frequency[band][np.argmax(spectrum[band])])
    # Dorsoventral output alternation from applied named muscle spikes.
    rate = np.diff(spikes, axis=0) / sample_dt
    dorsal = [i for i, n in enumerate(outputs) if n.split('_')[-1][:2] == 'MD']
    ventral = [i for i, n in enumerate(outputs) if n.split('_')[-1][:2] == 'MV' and 'VULVA' not in n]
    d, v = rate[:, dorsal].sum(axis=1), rate[:, ventral].sum(axis=1)
    dv_corr = float(np.corrcoef(d, v)[0, 1]) if d.std() > 0 and v.std() > 0 else None
    varying = [names[i] for i in range(len(names)) if s.shape[0] and np.ptp(s[:, i]) > 0.01]
    exteroceptive = [n for n in varying if not re.search(r'_(accel|gyro)\.', n)]
    states = len({tuple(row) for row in np.round(s, 1)}) if s.shape[0] else 0
    cells = {tuple(c) for c in np.floor(head / CELL_M).astype(int)}
    rows = len(records)
    return dict(
        trace=str(path), records=rows, simulated_s=float(t[-1] - t[0]) if rows > 1 else 0.0,
        disarmed_fraction=float(np.mean([r['disarmed'] for r in records])),
        npc_records=int(sum(r['npc_control'] for r in records)),
        sense=dict(captures=len(sensed), varying_channels=varying, exteroceptive_varying=exteroceptive,
                   distinct_states_0p1=states,
                   touch_front_frames=int((s[:, names.index('celegans_s_06_touch_front')] > 0.5).sum()) if s.shape[0] else 0,
                   touch_rear_frames=int((s[:, names.index('celegans_s_07_touch_rear')] > 0.5).sum()) if s.shape[0] else 0,
                   range_detection_fraction={n: float((s[:, i] > 0).mean()) for i, n in enumerate(names)
                                             if s.shape[0] and re.search(r'(taste|chem|flow|far)', n)},
                   min=s.min(axis=0).round(4).tolist() if s.shape[0] else [],
                   max=s.max(axis=0).round(4).tolist() if s.shape[0] else []),
        wriggle=dict(joint_amplitude_rad=amplitude.round(4).tolist(), joint_reversals=reversals.tolist(),
                     wriggling_joints=int(wriggling.sum()), joint_count=int(q.shape[1]),
                     dominant_frequency_hz=dominant, neighbour_lag_s_median=float(np.median(lags)) if lags else None,
                     neighbour_lag_same_sign_fraction=float(max(np.mean(np.array(lags) > 0), np.mean(np.array(lags) < 0))) if lags else None,
                     two_mode_variance_fraction=pc2_fraction, two_mode_phase_cycles=cycles,
                     output_spikes=int(spikes[-1].sum()), active_output_channels=int((spikes[-1] > 0).sum()),
                     dorsal_spikes=int(spikes[-1, dorsal].sum()), ventral_spikes=int(spikes[-1, ventral].sum()),
                     dorsal_ventral_rate_correlation=dv_corr),
        move=dict(start_xy=centroid[0].round(4).tolist(), end_xy=centroid[-1].round(4).tolist(),
                  net_m=float(np.linalg.norm(centroid[-1] - centroid[0])),
                  net_body_lengths=float(np.linalg.norm(centroid[-1] - centroid[0]) / BODY_LENGTH_M),
                  max_excursion_m=float(np.linalg.norm(centroid - centroid[0], axis=1).max()),
                  path_m=float(np.linalg.norm(steps, axis=1).sum()),
                  mean_speed_mm_s=float(1000 * np.linalg.norm(centroid[-1] - centroid[0]) / max(t[-1] - t[0], 1e-9))),
        explore=dict(head_cells=len(cells), cell_m=CELL_M,
                     total_turning_rad=float(np.abs(np.diff(heading)).sum()),
                     net_heading_change_rad=float(heading[-1] - heading[0]),
                     head_bbox_m=(np.ptp(head, axis=0)).round(4).tolist()),
        series=dict(t=t, centroid=centroid, head=head, q=q, s=s, rate=rate, d=d, v=v, xyz=xyz, names=names, heading=heading),
    )


def verdict(neural, silent):
    th = THRESHOLDS
    eps = 1e-6
    checks = {
        'sense.varying_channels': len(neural['sense']['varying_channels']) >= th['sense_min_varying_channels'],
        'sense.exteroceptive_varying': len(neural['sense']['exteroceptive_varying']) >= th['sense_min_exteroceptive_varying'],
        'sense.captures_reached_brain': neural['sense']['captures'] > 0,
        'wriggle.joint_fraction': neural['wriggle']['wriggling_joints'] >= th['wriggle_min_joint_fraction'] * neural['wriggle']['joint_count'],
        'wriggle.passive_control_still': silent['wriggle']['wriggling_joints'] == 0,
        'wriggle.neural_output_drives': neural['wriggle']['output_spikes'] > 0 and neural['npc_records'] == 0,
        'move.net_body_lengths': neural['move']['net_body_lengths'] >= th['move_min_net_body_lengths'],
        'move.ratio_to_passive': neural['move']['net_m'] >= th['move_min_ratio_to_passive'] * max(silent['move']['net_m'], eps),
        'explore.head_cells_ratio': neural['explore']['head_cells'] >= th['explore_min_head_cells_ratio_to_passive'] * max(silent['explore']['head_cells'], 1),
        'explore.turning': neural['explore']['total_turning_rad'] >= th['explore_min_total_turning_rad'],
        'explore.sensory_states_ratio': neural['sense']['distinct_states_0p1'] >= th['explore_min_sensory_states_ratio_to_passive'] * max(silent['sense']['distinct_states_0p1'], 1),
    }
    claims = {c: all(v for k, v in checks.items() if k.startswith(c + '.')) for c in ('sense', 'wriggle', 'move', 'explore')}
    return checks, claims


def plot(output, neural, silent):
    try:
        import matplotlib
        matplotlib.use('Agg')
        import matplotlib.pyplot as plt
    except ImportError:
        return None
    n, p = neural['series'], silent['series']
    fig, ax = plt.subplots(2, 3, figsize=(18, 10))
    a = ax[0, 0]
    a.plot(n['centroid'][:, 0], n['centroid'][:, 1], lw=1, label='AARNN centroid')
    a.plot(n['head'][:, 0], n['head'][:, 1], lw=.4, alpha=.6, label='AARNN head')
    a.plot(p['centroid'][:, 0], p['centroid'][:, 1], 'k.', ms=3, label='passive control')
    for k in (0, len(n['xyz']) // 2, -1):
        a.plot(n['xyz'][k, :, 0], n['xyz'][k, :, 1], '-', lw=2, alpha=.7)
    a.set_aspect('equal'); a.set_title('Trajectory and body postures (m)'); a.legend(fontsize=7)
    a = ax[0, 1]
    im = a.imshow(n['q'].T, aspect='auto', origin='lower', cmap='coolwarm',
                  extent=[n['t'][0], n['t'][-1], 2, 24], vmin=-0.38, vmax=0.38)
    a.set_title('Bending kymograph: spine joint angle (rad)'); a.set_xlabel('s'); a.set_ylabel('joint (tail -> head)')
    fig.colorbar(im, ax=a)
    a = ax[0, 2]
    k = max(1, int(0.2 / float(np.median(np.diff(n['t'])))))
    smooth = lambda x: np.convolve(x, np.ones(k) / k, mode='same')
    a.plot(n['t'][1:], smooth(n['d']), label='dorsal MD* spikes/s')
    a.plot(n['t'][1:], smooth(n['v']), label='ventral MV* spikes/s')
    a.set_title('Applied AARNN muscle output'); a.legend(fontsize=7)
    a = ax[1, 0]
    if n['s'].shape[0]:
        im = a.imshow(n['s'].T, aspect='auto', origin='lower', cmap='viridis', vmin=0, vmax=1)
        a.set_yticks(range(len(n['names']))); a.set_yticklabels([x.split('_', 3)[-1] for x in n['names']], fontsize=6)
        fig.colorbar(im, ax=a)
    a.set_title('Sensory frames submitted to brain (normalised)'); a.set_xlabel('capture')
    a = ax[1, 1]
    a.plot(n['t'], np.linalg.norm(n['centroid'] - n['centroid'][0], axis=1), label='AARNN')
    a.plot(p['t'], np.linalg.norm(p['centroid'] - p['centroid'][0], axis=1), label='passive control')
    a.set_title('Centroid distance from start (m)'); a.set_xlabel('s'); a.legend(fontsize=7)
    a = ax[1, 2]
    a.plot(n['t'], n['heading'], label='AARNN heading (rad)')
    a.plot(p['t'], p['heading'], label='passive control')
    a.set_title('Head heading, unwrapped'); a.set_xlabel('s'); a.legend(fontsize=7)
    fig.tight_layout()
    path = output / 'behaviour.png'
    fig.savefig(path, dpi=110)
    plt.close(fig)
    return str(path)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--output', type=Path, required=True, help='new or empty evidence directory')
    parser.add_argument('--steps', type=int, default=30000, help='2 ms physics steps (default 60 s)')
    parser.add_argument('--trace-every', type=int, default=25)
    parser.add_argument('--nodes', type=int, choices=[1, 3], default=1)
    parser.add_argument('--startup-timeout', type=int, default=300)
    parser.add_argument('--pacing', choices=['realtime', 'lockstep'], default='lockstep',
                        help='realtime: wall-clock body clock (brain may lag); lockstep: body waits for each reply')
    parser.add_argument('--timeout', type=int, default=7200, help='wall-clock budget for the neural run (s)')
    parser.add_argument('--network', type=Path, help='candidate snapshot for celegans_0 (default: preserved)')
    parser.add_argument('--config', type=Path, help='candidate configuration for celegans_0')
    parser.add_argument('--analyse-only', action='store_true', help='re-analyse traces already in --output')
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    before = {p: digest(ROOT / p) for p in SOURCES}
    runs = None
    if not args.analyse_only:
        if any(output.iterdir()):
            raise SystemExit(f'{output} must be new or empty')
        runs = simulate(output, args.steps, args.trace_every, args.nodes, args.startup_timeout, args.pacing, args.timeout,
                        args.network and args.network.resolve(), args.config and args.config.resolve())
        if args.network:
            runs['neural']['network'] = dict(path=str(args.network.resolve()), sha256=digest(args.network.resolve()))
        world = json.loads((output / 'neural.json').read_text())['worlds'][0]
        assert world['status'] == 'pass' and world['controller_mode'] == 'AER1-reference', world['status']
        assert world['npc_steps'] == 0 and world['transport']['fault'] is None
        runs['neural'].update(exchanges=world['transport']['exchanges'], output_spikes=world['transport']['output_spikes'],
                              applied=world['applied'], expired=world['expired'], active_steps=world['active_steps'],
                              pre_admission_gaps=world['transport']['pre_admission_gaps'])
    neural = analyse(output / 'trace-neural/celegans_0.jsonl')
    silent = analyse(output / 'trace-silent/celegans_0.jsonl')
    checks, claims = verdict(neural, silent)
    figure = plot(output, neural, silent)
    for item in (neural, silent):
        item.pop('series')
    assert before == {p: digest(ROOT / p) for p in SOURCES}, 'preserved source changed'
    report = dict(schema_version=1, scenario='CELEGANS-MUJOCO-BEHAVIOUR-001',
                  scope='engineering closed-loop mechanism evidence; not biological calibration, learned task, Webots parity or production effect authority',
                  git_revision=subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=ROOT, capture_output=True, text=True).stdout.strip(),
                  git_dirty=bool(subprocess.run(['git', 'status', '--porcelain'], cwd=ROOT, capture_output=True, text=True).stdout.strip()),
                  preserved_sources=before, thresholds=THRESHOLDS, runs=runs, neural=neural, passive_control=silent,
                  checks=checks, claims=claims, figure=figure,
                  status='pass' if all(claims.values()) else 'fail')
    (output / 'behaviour.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(dict(status=report['status'], claims=claims, checks=checks,
                          move=neural['move'], explore=neural['explore'],
                          wriggle={k: v for k, v in neural['wriggle'].items() if not isinstance(v, list)},
                          passive=dict(move=silent['move'], wriggling_joints=silent['wriggle']['wriggling_joints'],
                                       head_cells=silent['explore']['head_cells']),
                          report=str(output / 'behaviour.json'), figure=figure), indent=2))
    raise SystemExit(0 if report['status'] == 'pass' else 1)


if __name__ == '__main__':
    main()
