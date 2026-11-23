#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""AARNN algorithm-depth study for the C. elegans connectome in MuJoCo.

Two complementary measurements:

1. Closed loop (``--closed-loop``): for every depth level the managed
   launcher runs the 302-neuron brain against the MuJoCo worm in lock-step
   (the body waits for each brain reply, so simulated time runs slower than
   wall-clock time). The native adapter records full ``qpos`` and muscle
   activations; each run is rendered frame by frame to an MP4 that replays at
   true simulation speed. A same-binary passive control runs once.
2. Open loop (``--open-loop``): ``examples/celegans_depth_probe`` replays a
   scripted stimulus protocol through the network for every depth level and
   for the full factorial of depth-gated mechanisms at depth 3 (2^7 = 128
   permutations), reporting neural statistics and numerical health.

Biological mimicry is scored against published ranges for an adult
hermaphrodite crawling on agar (see BIOLOGY below). The robot body is
0.34 m long, so absolute speeds are reported in body lengths per second.
This is engineering evidence, not biological calibration.
"""
import argparse
import concurrent.futures as cf
import importlib.util
import itertools
import json
import math
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

import numpy as np


def _json(o):
    return o.item() if hasattr(o, 'item') else str(o)

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('probe', ROOT / 'scripts/qa/probe_celegans_mujoco_behaviour.py')
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)

DEPTHS = (0, 1, 2, 3, 4, 5)
# Depth-gated mechanisms switchable without changing the depth (net paths).
FACTORS = {
    'stp': ('aarnn_bio.stp_enabled', 'true', 'false'),
    'adaptive_threshold': ('aarnn_bio.adaptive_threshold_enabled', 'true', 'false'),
    'homeostasis': ('aarnn_bio.homeostasis_gain', '0.25', '0.0'),
    'izh_refractory': ('aarnn_bio.izh_refractory_ms', '2.0', '0.0'),
    'active_dendrites': ('aarnn_bio.dendritic_active_enabled', 'true', 'false'),
    'nmda_voltage_gate': ('aarnn_nmda_voltage_sensitivity', '0.02', '0.0'),
    'neuromodulation': ('aarnn_bio.neuromodulation_enabled', 'true', 'false'),
}
# Adult C. elegans on agar (forward crawling), literature ranges.
BIOLOGY = {
    'undulation_hz': (0.25, 0.7, 'Fang-Yen 2010; Berri 2009 (agar 0.3-0.5 Hz)'),
    'wavelength_body_lengths': (0.5, 0.9, 'Berri 2009 (agar ~0.65 L)'),
    'curvature_amplitude_kL': (3.0, 9.0, 'Stephens 2008 (peak |kL| ~ 5-8)'),
    'forward_wave_fraction': (0.6, 1.0, 'head-to-tail propagation dominates crawling'),
    'dv_muscle_correlation': (-1.0, -0.2, 'dorsal/ventral antiphase (cross-inhibition)'),
    'speed_body_lengths_s': (0.05, 0.4, 'Stephens 2008 (~0.1-0.2 L/s)'),
    'reversals_per_min': (0.5, 6.0, 'Gray 2005 (1-4 /min off food)'),
}


# --network / --config / --set select the brain under study (e.g. the graded
# motor-neuron build); defaults are the spiking v2 network and its config.
SOURCES = dict(network=None, config=None, overrides=[])


def network_source():
    """Frozen-morphology snapshot when present (identical anatomy for every
    variant; see examples/celegans_depth_probe.rs --export-frozen)."""
    if SOURCES['network']:
        return Path(SOURCES['network']).resolve()
    frozen = ROOT / 'network_celegans_frozen.json'
    return frozen if frozen.exists() else ROOT / 'network_celegans.json'


def graft_cell_models(build_path, output):
    """Frozen v2 anatomy with the cell models of another importer build.

    Morphology regrowth is not reproducible, so a fresh build (e.g.
    ``--motor-model graded``) would also change anatomy. This copies only the
    per-type biology, the type of every cell and the import labels onto the
    frozen snapshot, after checking that the wiring is identical; the result
    differs from the frozen brain in cell dynamics and, if requested, in
    weight values or removed connections (never added ones).
    """
    base = json.loads(network_source().read_text())
    build = json.loads(Path(build_path).read_text())
    def flat(m):
        return [x for part in (m if isinstance(m, list) else [m]) for x in part['data']]
    presence = {'w_in': 'p_in', 'w_hh_rec': 'p_rec', 'w_out': 'p_out'}
    for key in ('w_in', 'w_hh_rec', 'w_out', 'w_gap'):
        a, b = flat(base[key]), flat(build[key])
        # The frozen export re-serialises f64 weights (1-ulp differences).
        if len(a) != len(b) or any(abs(x - y) > 1e-12 for x, y in zip(a, b)):
            # The frozen morphology holds a route per synapse and silently
            # has none for a new one, so the build may change values and
            # remove entries (a zero weight on an existing route carries
            # nothing; e.g. sensory gains, gap conductances, isolated-circuit
            # tests) but never add one. Electrical conductances are re-read
            # from w_gap on every import.
            subset = len(a) == len(b) and all(abs(y) <= 1e-12 or abs(x) > 1e-12 for x, y in zip(a, b))
            if not subset:
                raise SystemExit(f'{build_path}: {key} adds connections absent from the frozen network')
            base[key] = build[key]
            if key in presence:
                base[presence[key]] = build[presence[key]]
            if key == 'w_gap' and build['connectome_labels'].get('electrical_synapses'):
                base['connectome_labels']['electrical_synapses'] = build['connectome_labels']['electrical_synapses']
    bl, ll = base['connectome_labels'], build['connectome_labels']
    if bl['hidden_nodes'] != ll['hidden_nodes'] or bl['output_nodes'] != ll['output_nodes']:
        raise SystemExit('cell order differs')
    base['net']['aarnn_bio'] = build['net']['aarnn_bio']
    base['net']['neuron_types'] = build['net']['neuron_types']
    for group in ('sensory_nodes', 'output_nodes'):
        for node, src in zip(base['topo'][group], build['topo'][group]):
            node['type_name'] = src['type_name']
    for node, src in zip(base['topo']['layers'][0], build['topo']['layers'][0]):
        node['type_name'] = src['type_name']
    bl['weights'] = ll['weights']
    bl['cell_models_from'] = dict(path=str(build_path), sha256=probe.digest(Path(build_path)))
    output.mkdir(parents=True, exist_ok=True)
    net_path, cfg_path = output / 'network_grafted.json', output / 'config_grafted.json'
    net_path.write_text(json.dumps(base))
    cfg_path.write_text(json.dumps(base['net']))
    return net_path, cfg_path


def config_source():
    return Path(SOURCES['config']).resolve() if SOURCES['config'] else ROOT / 'webots_world/configs/config_celegans_mujoco.json'


def write_variant(path, depth, overrides=()):
    overrides = list(SOURCES['overrides']) + list(overrides)
    for src, dst in ((network_source(), path / 'network.json'), (config_source(), path / 'config.json')):
        data = json.loads(src.read_text())
        net = data['net'] if 'net' in data else data
        net['aarnn_layer_depth'] = depth
        for key, value in overrides:
            node = net
            parts = key.split('.')
            for p in parts[:-1]:
                node = node[p]
            node[parts[-1]] = json.loads(value)
        dst.write_text(json.dumps(data))
    return path / 'network.json', path / 'config.json'


def closed_loop_run(name, out, steps, every, timeout, network=None, config=None, silent=False):
    out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, NM_DISABLE_OPENCL='1', RAYON_NUM_THREADS=os.environ.get('RAYON_NUM_THREADS', '2'),
               NM_AUTO_AARNN_DEPTH='0', NM_AER_REFERENCE_DATA_TIMEOUT_MS='60000')
    done = out / 'run.json'
    if done.exists() and (out / 'trace/celegans_0.jsonl').exists():
        try:
            if json.loads(done.read_text())['worlds'][0]['status'] == 'pass':
                return dict(name=name, reused=True, out=str(out))
        except (KeyError, ValueError, IndexError):
            pass
    for stale in (done, out / 'run.log'):
        stale.unlink(missing_ok=True)
    shutil.rmtree(out / 'trace', ignore_errors=True)
    sdk = Path(env.get('AARNN_MUJOCO_DIR', str(Path.home() / 'Developer/mujoco-3.17.0')))
    env.update(AARNN_MUJOCO_DIR=str(sdk), LD_LIBRARY_PATH=str(sdk / 'lib') + (':' + env['LD_LIBRARY_PATH'] if env.get('LD_LIBRARY_PATH') else ''))
    if silent:
        command = ['bash', 'scripts/run_sim.sh', '--sim', 'mujoco', '--robots', 'celegans=1', '--no-build', '--control', 'silent',
                   '--headless', '--steps', str(steps), '--output', str(out / 'run.json'), '--trace', str(out / 'trace'),
                   '--trace-every', str(every)]
    else:
        command = ['bash', 'scripts/run_sim.sh', '--sim', 'mujoco', '--robots', 'celegans=1', '--no-build',
                   '--tcp-base-port', str(probe.free_port()), '--tcp-ready-timeout', '300', '--headless', '--steps', str(steps),
                   '--lockstep', '--output', str(out / 'run.json'), '--trace', str(out / 'trace'), '--trace-every', str(every),
                   '--network-map', f'celegans_0={network}', '--config-map', f'celegans_0={config}']
        env['NM_CLUSTER_NODES'] = '1'
    started = time.time()
    probe.run(command, out / 'run.log', timeout, env)
    return dict(name=name, command=command, wall_s=time.time() - started, out=str(out))


def phase_wave(q, t):
    """Wave frequency, wavelength (L), forward fraction and curvature from joint angles.

    ``q`` columns are hinges tail -> head (spine_02..24). The complex
    analytic signal of each hinge (Hilbert via FFT) gives a phase per hinge;
    the phase gradient along the body gives the wavelength and the sign of
    propagation: head-to-tail waves have phase increasing towards the tail.
    """
    centred = q - q.mean(axis=0)
    n, m = centred.shape
    if n < 32 or centred.std() < 1e-4:
        return dict(undulation_hz=None, wavelength_body_lengths=None, forward_wave_fraction=None, curvature_amplitude_kL=float(24 * centred.std(axis=0).mean() * math.sqrt(2)))
    spec = np.fft.fft(centred, axis=0)
    h = np.zeros(n)
    h[0] = 1
    h[1:(n + 1) // 2] = 2
    if n % 2 == 0:
        h[n // 2] = 1
    analytic = np.fft.ifft(spec * h[:, None], axis=0)
    phase = np.unwrap(np.angle(analytic), axis=1)
    # position of hinge j (tail->head) as body fraction from the head
    ap = np.array([(23.5 - (j + 1)) / 24 for j in range(m)])
    slope = np.array([np.polyfit(ap, phase[k], 1)[0] for k in range(n)])  # rad per body length
    dt = float(np.median(np.diff(t)))
    inst_freq = np.diff(np.unwrap(np.angle(analytic.mean(axis=1)))) / (2 * math.pi * dt)
    power = np.abs(np.fft.rfft(centred, axis=0)).sum(axis=1)
    freqs = np.fft.rfftfreq(n, dt)
    band = (freqs > 0.05) & (freqs < 5)
    f = float(freqs[band][np.argmax(power[band])]) if band.any() else None
    # A head-to-tail wave delays the tail: phase decreases with ap for e^{i(wt - kx)}.
    forward = float(np.mean(slope < 0))
    lam = float(np.median(2 * math.pi / np.maximum(np.abs(slope), 1e-6)))
    kL = float(24 * np.abs(analytic).mean())  # hinge angle * segments = curvature * L
    return dict(undulation_hz=f, wavelength_body_lengths=lam, forward_wave_fraction=forward, curvature_amplitude_kL=kL,
                mean_inst_freq_hz=float(np.median(np.abs(inst_freq))) if len(inst_freq) else None)


def mimicry(trace_path, sim):
    header, records = probe.load(trace_path)
    t = np.array([r['time_s'] for r in records])
    q = np.array([r['joint_q'] for r in records])
    wave = phase_wave(q, t)
    out = dict(wave)
    act = [r.get('muscle_activation') for r in records]
    if act and act[0] is not None:
        a = np.array(act).reshape(len(act), 24, 4)
        d = a[:, :, :2].mean(axis=2)
        v = a[:, :, 2:].mean(axis=2)
        cors = [np.corrcoef(d[:, r], v[:, r])[0, 1] for r in range(24) if d[:, r].std() > 1e-6 and v[:, r].std() > 1e-6]
        out['dv_muscle_correlation'] = float(np.mean(cors)) if cors else None
        out['mean_muscle_activation'] = float(a.mean())
    out['speed_body_lengths_s'] = sim['move']['net_body_lengths'] / max(sim['simulated_s'], 1e-9)
    # Reversals: sign changes of centroid velocity projected on the body axis (hysteresis 0.2 L/s).
    xyz = np.array([r['body_xyz'] for r in records])
    centroid = xyz[:, :24, :2].mean(axis=1)
    axis = xyz[:, 23, :2] - xyz[:, 0, :2]
    axis /= np.maximum(np.linalg.norm(axis, axis=1, keepdims=True), 1e-9)
    vel = np.gradient(centroid, t, axis=0)
    k = max(1, int(0.5 / float(np.median(np.diff(t)))))
    proj = np.convolve((vel * axis).sum(axis=1), np.ones(k) / k, mode='same') / probe.BODY_LENGTH_M
    state, rev = 0, 0
    for x in proj:
        s = 1 if x > 0.02 else -1 if x < -0.02 else 0
        if s and s != state:
            rev += state != 0
            state = s
    minutes = max((t[-1] - t[0]) / 60, 1e-9)
    out['reversals_per_min'] = rev / minutes
    out['forward_time_fraction'] = float(np.mean(proj > 0.02))
    # Sensory ecology: time with the head on food / in repellent, mean food odour.
    names = header['sensor_names']
    s = np.array([r['sensors'] for r in records if len(r['sensors']) == len(names)])
    if s.size:
        def col(n):
            return s[:, names.index(n)] if n in names else None
        food = col('celegans_s_12_taste_front_center')
        rep = col('celegans_s_13_taste_front_left')
        odour = col('celegans_s_15_chem_left')
        out['head_on_food_fraction'] = float(np.mean(food > 0.4)) if food is not None else None
        out['head_in_repellent_fraction'] = float(np.mean(rep > 0.3)) if rep is not None else None
        out['odour_gain'] = float(odour[-len(odour) // 4:].mean() - odour[:len(odour) // 4].mean()) if odour is not None else None
    score, n = 0, 0
    within = {}
    for key, (lo, hi, _) in BIOLOGY.items():
        val = out.get(key)
        ok = val is not None and lo <= val <= hi
        within[key] = ok
        score += ok
        n += 1
    out['within_biological_range'] = within
    out['mimicry_score'] = f'{score}/{n}'
    return out


def open_loop(output, jobs, ms):
    exe = ROOT / 'target/release/examples/celegans_depth_probe'
    if not exe.exists():
        raise SystemExit('build examples/celegans_depth_probe first (cargo build --release --features growth3d,morpho --example celegans_depth_probe)')
    tasks = []
    for d in DEPTHS:
        tasks.append((f'depth{d}', d, []))
        tasks.append((f'depth{d}_seed2', d, []))
    names = list(FACTORS)
    for bits in itertools.product((1, 0), repeat=len(names)):
        label = 'd3_' + ''.join(str(b) for b in bits)
        sets = [f'{FACTORS[n][0]}={FACTORS[n][1] if b else FACTORS[n][2]}' for n, b in zip(names, bits)]
        tasks.append((label, 3, sets))
    out_dir = output / 'open-loop'
    out_dir.mkdir(parents=True, exist_ok=True)

    def one(task):
        label, d, sets = task
        path = out_dir / f'{label}.json'
        seed = 11 if label.endswith('seed2') else 7
        cmd = [str(exe), '--network', str(network_source()), '--depth', str(d), '--ms', str(ms), '--seed', str(seed),
               '--out', str(path)]
        for s in [f'{k}={v}' for k, v in SOURCES['overrides']] + list(sets):
            cmd += ['--set', s]
        env = dict(os.environ, NM_DISABLE_OPENCL='1', RAYON_NUM_THREADS='1')
        subprocess.run(cmd, cwd=ROOT, env=env, check=True, capture_output=True, timeout=1800)
        return label, d, sets, path

    results = []
    with cf.ThreadPoolExecutor(jobs) as pool:
        for label, d, sets, path in pool.map(one, tasks):
            results.append(summarise_open(label, d, sets, path))
    # Determinism / level-equivalence cross-checks.
    rasters = {r['label']: json.loads(Path(r['path']).read_text())['raster_hidden'] for r in results if r['label'].startswith('depth')}
    checks = {
        'depth4_equals_depth3_bitwise': rasters.get('depth4') == rasters.get('depth3'),
        'depth5_equals_depth3_bitwise': rasters.get('depth5') == rasters.get('depth3'),
        'no_nonfinite_state_any_run': all(r['nonfinite'] == 0 for r in results),
        'no_synchronous_bursts_any_depth': all(r['synchronous_frames'] == 0 for r in results if r['label'].startswith('depth')),
    }
    for r in results:
        r.pop('path')
    return dict(results=results, checks=checks, factors=FACTORS)


def summarise_open(label, d, sets, path):
    sys.path.insert(0, str(ROOT / 'scripts'))
    import celegans_anatomy as A
    data = json.loads(Path(path).read_text())
    names = data['hidden_names']
    rate = np.array(data['hidden_rate_hz'])
    # Graded motor neurons do not spike: their activity is the spike-equivalent
    # rate (mean release x release_max_hz) written by the probe.
    equiv = np.array(data.get('hidden_equiv_rate_hz', data['hidden_rate_hz']))
    graded = np.array(data.get('hidden_graded', [False] * len(names)), dtype=bool)
    cls = {}
    for n, r in zip(names, equiv):
        cls.setdefault(A.neuron_class(n), []).append(r)
    raster = np.array(data['raster_output']) if data['raster_output'] else np.zeros((0, 2))
    outputs = data['output_names']
    dorsal = [i for i, n in enumerate(outputs) if n.startswith('MD')]
    ventral = [i for i, n in enumerate(outputs) if n.startswith('MV') and n != 'MVULVA']
    bins = np.arange(0, data['ms'] + 50, 50)
    dv = None
    wave_window = None
    if raster.size:
        hd = np.histogram(raster[np.isin(raster[:, 1], dorsal), 0], bins)[0]
        hv = np.histogram(raster[np.isin(raster[:, 1], ventral), 0], bins)[0]
        if hd.std() > 0 and hv.std() > 0:
            dv = float(np.corrcoef(hd, hv)[0, 1])
        late = raster[:, 0] >= 6000
        if late.any():
            wave_window = float(late.sum() / max(1, (data['ms'] - 6000) / 1000) / len(outputs))
    stim = {}
    hraster = np.array(data['raster_hidden']) if data['raster_hidden'] else np.zeros((0, 2))
    for cls_name, window in (('ASH', (1000, 1300)), ('PLM', (2000, 2300)), ('AWA', (3000, 4000)), ('ASE', (4000, 5000)), ('ASJ', (5000, 5500))):
        idx = [i for i, n in enumerate(names) if A.neuron_class(n) == cls_name]
        if hraster.size:
            sel = np.isin(hraster[:, 1], idx)
            inside = ((hraster[:, 0] >= window[0]) & (hraster[:, 0] < window[1]) & sel).sum() / ((window[1] - window[0]) / 1000) / len(idx)
            base = ((hraster[:, 0] < 1000) & sel).sum() / 1.0 / len(idx)
            stim[cls_name] = dict(stimulus_hz=float(inside), baseline_hz=float(base))
    return dict(label=label, depth=d, overrides=sets, path=str(path), ms_per_frame=data['ms_per_frame'],
                hidden_mean_hz=float(rate[~graded].mean()) if (~graded).any() else 0.0,
                silent_neurons=int((equiv == 0).sum()), graded_neurons=int(graded.sum()),
                command_hz={c: float(np.mean(cls.get(c, [0]))) for c in ('AVA', 'AVB', 'AVD', 'AVE', 'PVC')},
                motor_hz={c: float(np.mean(cls.get(c, [0]))) for c in ('DA', 'DB', 'DD', 'VA', 'VB', 'VD', 'AS')},
                muscle_frame_hz=float(np.mean(data['output_frame_rate_hz'])), dv_output_correlation=dv,
                muscle_hz_during_bend_wave=wave_window, sensory_responses=stim,
                synchronous_frames=data['synchronous_frames'], nonfinite=data['health']['nonfinite'],
                v_range=[data['health']['v_min'], data['health']['v_max']], i_h0_abs_max=data['health']['i_h0_abs_max'])


def render_report(output, report):
    """Markdown table plus comparison figure for the depth study."""
    lines = ['# AARNN depth study: C. elegans connectome in MuJoCo', '',
             f"Network: `{report.get('network')}` (sha256 `{report.get('network_sha256', '')[:16]}...`)", '']
    closed = report.get('closed_loop', {}).get('runs', {})
    if closed:
        keys = list(BIOLOGY)
        lines += ['## Closed loop (lock-step, frame-by-frame video)', '',
                  '| depth | mimicry | ' + ' | '.join(keys) + ' | muscle act. | net move (L) | slowdown | video |',
                  '|---' * (len(keys) + 6) + '|']
        for name, r in sorted(closed.items(), key=lambda kv: kv[1]['depth']):
            m = r['mimicry']
            cells = []
            for k in keys:
                v = m.get(k)
                ok = m['within_biological_range'].get(k)
                cells.append('n/a' if v is None else f"{v:.3g}{' ✓' if ok else ''}")
            video = r.get('video', {}).get('video', '')
            lines.append(f"| {r['depth']} | {m['mimicry_score']} | " + ' | '.join(cells)
                         + f" | {m.get('mean_muscle_activation', 0):.3f} | {r['move']['net_body_lengths']:.3f} | {r['transport']['slowdown']:.1f}x | {Path(video).name if video else ''} |")
        lines += ['', 'Biological ranges (adult hermaphrodite on agar): ' + '; '.join(f"{k} {lo}-{hi} ({ref})" for k, (lo, hi, ref) in BIOLOGY.items()), '']
        passive = report['closed_loop'].get('passive', {})
        lines += [f"Passive control: net move {passive.get('move', {}).get('net_body_lengths', 0):.3f} L, wriggling joints {passive.get('wriggling_joints')}", '']
    ol = report.get('open_loop')
    if ol:
        lines += ['## Open loop (scripted stimuli, 8 s, numerical health)', '', '| run | hidden Hz | muscles Hz | D/V corr | AVA | AVB | DB | VB | DD | VD | ASH touch/base | nonfinite | sync frames | ms/frame |', '|---' * 14 + '|']
        for r in ol['results']:
            if not r['label'].startswith('depth') or r['label'].endswith('seed2'):
                continue
            ash = r['sensory_responses'].get('ASH', {})
            dv = r['dv_output_correlation']
            lines.append(f"| {r['label']} | {r['hidden_mean_hz']:.1f} | {r['muscle_frame_hz']:.2f} | {'n/a' if dv is None else f'{dv:.2f}'} | "
                         f"{r['command_hz']['AVA']:.1f} | {r['command_hz']['AVB']:.1f} | {r['motor_hz']['DB']:.1f} | {r['motor_hz']['VB']:.1f} | "
                         f"{r['motor_hz']['DD']:.1f} | {r['motor_hz']['VD']:.1f} | {ash.get('stimulus_hz', 0):.0f}/{ash.get('baseline_hz', 0):.0f} | {r['nonfinite']} | {r['synchronous_frames']} | {r['ms_per_frame']:.2f} |")
        lines += ['', 'Cross-checks: ' + ', '.join(f"{k}={v}" for k, v in ol['checks'].items()), '']
        fac = [r for r in ol['results'] if r['label'].startswith('d3_')]
        if fac:
            names = list(FACTORS)
            lines += ['### Depth-3 factorial (all 2^7 mechanism permutations): main effects', '',
                      '| mechanism | Δ hidden Hz (on-off) | Δ muscle Hz | Δ D/V corr |', '|---|---|---|---|']
            for k, n in enumerate(names):
                on = [r for r in fac if r['label'][3 + k] == '1']
                off = [r for r in fac if r['label'][3 + k] == '0']
                mean = lambda rs, key: float(np.mean([x[key] for x in rs if x[key] is not None])) if rs else float('nan')
                lines.append(f"| {n} | {mean(on, 'hidden_mean_hz') - mean(off, 'hidden_mean_hz'):+.2f} | {mean(on, 'muscle_frame_hz') - mean(off, 'muscle_frame_hz'):+.3f} | "
                             f"{mean(on, 'dv_output_correlation') - mean(off, 'dv_output_correlation'):+.3f} |")
            lines.append('')
    (output / 'REPORT.md').write_text('\n'.join(lines) + '\n')
    try:
        import matplotlib
        matplotlib.use('Agg')
        import matplotlib.pyplot as plt
    except ImportError:
        return
    if closed:
        runs = sorted(closed.values(), key=lambda r: r['depth'])
        fig, ax = plt.subplots(1, 4, figsize=(20, 4.5))
        d = [r['depth'] for r in runs]
        ax[0].bar(d, [int(r['mimicry']['mimicry_score'].split('/')[0]) for r in runs], color='#3b7dd8')
        ax[0].set_title('Biological-range metrics met (of 7)'); ax[0].set_xlabel('AARNN depth')
        ax[1].plot(d, [r['mimicry'].get('mean_muscle_activation', 0) for r in runs], 'o-', label='muscle activation')
        ax[1].plot(d, [r['mimicry']['curvature_amplitude_kL'] / 10 for r in runs], 's-', label='curvature |kL| / 10')
        ax[1].set_title('Motor output'); ax[1].legend(fontsize=8); ax[1].set_xlabel('AARNN depth')
        ax[2].plot(d, [r['mimicry'].get('dv_muscle_correlation') or 0 for r in runs], 'o-', color='#c0392b')
        ax[2].axhspan(-1, -0.2, color='green', alpha=.12, label='biological antiphase'); ax[2].legend(fontsize=8)
        ax[2].set_title('Dorsal/ventral muscle correlation'); ax[2].set_xlabel('AARNN depth')
        ax[3].plot(d, [r['transport']['slowdown'] for r in runs], 'o-', color='#555')
        ax[3].set_title('Wall-clock / simulated time'); ax[3].set_xlabel('AARNN depth')
        fig.tight_layout(); fig.savefig(output / 'comparison.png', dpi=110); plt.close(fig)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--output', type=Path, required=True)
    ap.add_argument('--closed-loop', action='store_true')
    ap.add_argument('--open-loop', action='store_true')
    ap.add_argument('--depths', type=int, nargs='+', default=list(DEPTHS))
    ap.add_argument('--steps', type=int, default=15000, help='2 ms physics steps per closed-loop run (default 30 s)')
    ap.add_argument('--trace-every', type=int, default=20, help='20 steps = 25 video frames per simulated second')
    ap.add_argument('--jobs', type=int, default=6)
    ap.add_argument('--timeout', type=int, default=14400)
    ap.add_argument('--open-ms', type=int, default=8000)
    ap.add_argument('--no-video', action='store_true')
    ap.add_argument('--analyse-only', action='store_true')
    ap.add_argument('--network', help='network snapshot under study (default: frozen spiking v2 network)')
    ap.add_argument('--config', help='matching run configuration (default: webots_world/configs/config_celegans_mujoco.json)')
    ap.add_argument('--set', action='append', default=[], metavar='NET.PATH=JSON', help='override applied to every variant')
    ap.add_argument('--passive', type=Path, help='reuse the passive control of an earlier study directory')
    ap.add_argument('--cell-models-from', type=Path,
                    help='importer build whose cell models are grafted onto the frozen v2 anatomy (see graft_cell_models)')
    ap.add_argument('--title', default='AARNN', help='video title prefix')
    args = ap.parse_args()
    SOURCES.update(network=args.network, config=args.config,
                   overrides=[tuple(s.split('=', 1)) for s in args.set])
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    if args.cell_models_from:
        net_path, cfg_path = graft_cell_models(args.cell_models_from.resolve(), output)
        SOURCES.update(network=str(net_path), config=str(cfg_path))
    report = dict(schema='celegans-depth-study-v1', biology=BIOLOGY, network=str(network_source()), network_sha256=probe.digest(network_source()),
                  config=str(config_source()), overrides=args.set,
                  git_revision=subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=ROOT, capture_output=True, text=True).stdout.strip())
    if args.open_loop:
        report['open_loop'] = open_loop(output, args.jobs, args.open_ms)
        (output / 'open_loop.json').write_text(json.dumps(report['open_loop'], indent=2, default=_json) + '\n')
    if args.closed_loop:
        runs = {}
        if not args.analyse_only:
            jobs = []
            with cf.ThreadPoolExecutor(args.jobs) as pool:
                if not args.passive:
                    jobs.append(pool.submit(closed_loop_run, 'passive', output / 'passive', args.steps, args.trace_every, 1800, silent=True))
                for d in args.depths:
                    var = output / f'depth{d}'
                    var.mkdir(parents=True, exist_ok=True)
                    net, cfg = write_variant(var, d)
                    jobs.append(pool.submit(closed_loop_run, f'depth{d}', var, args.steps, args.trace_every, args.timeout, net, cfg))
                for j in jobs:
                    r = j.result()
                    runs[r['name']] = r
        silent = probe.analyse((args.passive.resolve() if args.passive else output / 'passive') / 'trace/celegans_0.jsonl')
        silent.pop('series')
        closed = {}
        for d in args.depths:
            trace = output / f'depth{d}/trace/celegans_0.jsonl'
            sim = probe.analyse(trace)
            checks, claims = probe.verdict(sim, silent)
            sim.pop('series')
            world = json.loads((output / f'depth{d}/run.json').read_text())['worlds'][0]
            closed[f'depth{d}'] = dict(depth=d, claims=claims, checks=checks, mimicry=mimicry(trace, sim),
                                       move=sim['move'], explore=sim['explore'],
                                       wriggle={k: v for k, v in sim['wriggle'].items() if not isinstance(v, list)},
                                       transport=dict(exchanges=world['transport']['exchanges'], applied=world['applied'], expired=world['expired'],
                                                      simulated_s=world['time_s'], wall_s=world['elapsed_s'],
                                                      slowdown=world['elapsed_s'] / max(world['time_s'], 1e-9)),
                                       run=runs.get(f'depth{d}'))
            if not args.no_video:
                video = output / f'depth{d}/celegans_depth{d}.mp4'
                venv = ROOT / 'target/robot-mujoco-venv/bin/python'
                r = subprocess.run([str(venv), str(ROOT / 'scripts/qa/render_celegans_trace_video.py'), str(trace), str(video),
                                    '--title', f'{args.title} depth {d}'], cwd=ROOT, capture_output=True, text=True, env=dict(os.environ, MUJOCO_GL='egl'))
                closed[f'depth{d}']['video'] = json.loads(r.stdout.strip().splitlines()[-1]) if r.returncode == 0 else dict(error=r.stderr[-2000:])
        report['closed_loop'] = dict(passive=dict(move=silent['move'], explore=silent['explore'],
                                                  wriggling_joints=silent['wriggle']['wriggling_joints']), runs=closed)
        (output / 'closed_loop.json').write_text(json.dumps(report['closed_loop'], indent=2, default=_json) + '\n')
    if not args.open_loop and (output / 'open_loop.json').exists():
        report['open_loop'] = json.loads((output / 'open_loop.json').read_text())
    if not args.closed_loop and (output / 'closed_loop.json').exists():
        report['closed_loop'] = json.loads((output / 'closed_loop.json').read_text())
    (output / 'study.json').write_text(json.dumps(report, indent=2, default=_json) + '\n')
    render_report(output, report)
    print(json.dumps({k: v for k, v in report.items() if k in ('network_sha256',)}, indent=2))


if __name__ == '__main__':
    main()
