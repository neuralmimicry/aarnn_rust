#!/usr/bin/env python3
"""Real imported Rust snapshots -> companion JAR -> bounded Minecraft-shaped frames.

Runs fresh local processes. Never attaches to a user's running brain or writes a model.
"""
import gc
import hashlib
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import sys
import time
import urllib.request
import re
import traceback

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
from robot_profiles import PROFILES
from minecraft import java


def port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def ready(process, number, timeout):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f'Process exited before readiness: {process.returncode}')
        try:
            with socket.create_connection(('127.0.0.1', number), timeout=.2):
                return
        except OSError:
            time.sleep(.1)
    raise TimeoutError('Process readiness deadline exceeded')


def stop(process):
    if process.poll() is None:
        process.terminate()
        try: process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill(); process.wait()


def main():
    output = Path(os.environ['NM_MINECRAFT_RESULT_DIR'])
    executable = ROOT / 'target/release/examples/nn_tcp_server'
    if not executable.is_file():
        raise RuntimeError('Build: cargo build --locked --release --features ui,robot_io --example nn_tcp_server')
    java_path = java().get('path')
    if not java_path: raise RuntimeError('Java 21+ required')
    content = json.loads((ROOT / 'sim/content/compiled.generated.json').read_text())
    fixtures = json.loads((ROOT / 'sim/minecraft/build/reference.json').read_text())
    selected = os.environ.get('NM_MINECRAFT_TEST_PROFILES', '')
    names = selected.split(',') if selected else [p['id'] for p in content['profiles']]
    if len(set(names)) != len(names) or any(name not in PROFILES for name in names):
        raise RuntimeError('Select unique canonical Minecraft profile IDs')
    reports = []
    for profile in (p for p in content['profiles'] if p['id'] in names):
        try:
            name = profile['id']; p = PROFILES[name]; model = ROOT / p.network_rel
            if not model.is_file(): raise RuntimeError(f'Required model unavailable: {model.name}')
            with model.open('rb') as stream:
                digest = hashlib.file_digest(stream, 'sha256').hexdigest()
            # The canonical snapshot begins with its small `net` config. Rust validates
            # the complete snapshot; avoid constructing a second 700 MB JSON object graph.
            with model.open() as stream: prefix = stream.read(65536)
            match = re.match(r'\s*\{\s*"net"\s*:\s*', prefix)
            if not match: raise RuntimeError('Expected canonical snapshot config prefix')
            net, _ = json.JSONDecoder().raw_decode(prefix, match.end())
            if (net['num_sensory_neurons'], net['num_output_neurons']) != (p.sensory, p.output):
                raise RuntimeError(f'{name}: snapshot dimensions mismatch; no automatic resizing permitted')
            del net, prefix; gc.collect()
            backend_port, bridge_port = port(), port()
            token = secrets.token_urlsafe(32)
            env = dict(os.environ, AARNN_MINECRAFT_TOKEN=token, RAYON_NUM_THREADS='4', NM_REALTIME_IPC='1')
            started = time.monotonic()
            with (output / f'{name}-rust.log').open('w') as rust_log, (output / f'{name}-bridge.log').open('w') as bridge_log:
                rust = subprocess.Popen([str(executable), '--tcp', f'127.0.0.1:{backend_port}',
                    '--sensory', str(p.sensory), '--output', str(p.output), '--config', str(ROOT / p.config_rel),
                    '--network', str(model)], cwd=output, env=env, stdout=rust_log, stderr=subprocess.STDOUT)
                bridge = None
                try:
                    ready(rust, backend_port, 180)
                    bridge = subprocess.Popen([java_path, '-jar', str(ROOT / 'sim/minecraft/build/libs/aarnn-minecraft-1.21.1-0.1.0-bridge.jar'),
                        '--port', str(bridge_port), '--base-port', str(backend_port), '--profiles', name],
                        cwd=output, env=env, stdout=bridge_log, stderr=subprocess.STDOUT)
                    ready(bridge, bridge_port, 75)
                    case = next(c for c in fixtures['cases'] if c['profile'] == name)
                    responses, latencies = [], []
                    step = 0
                    for index in range(16):
                        values = case['frames'][index % len(case['frames'])]['values']
                        body = dict(network_id=name, node_id=None, step_index=step, time_ms=step, dt_ms=1,
                                    content_digest=content['digest'], capture_sequence=index,
                                    capture_nanos=index*50000000, input_values=values)
                        request = urllib.request.Request(f'http://127.0.0.1:{bridge_port}/api/aer/infer',
                            data=json.dumps(body).encode(), headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'})
                        start = time.monotonic()
                        with urllib.request.urlopen(request, timeout=65) as response:
                            data = json.loads(response.read(65537))
                        latencies.append(time.monotonic()-start)
                        assert data['network_id'] == name
                        assert all(isinstance(i,int) and 0 <= i < p.output for i in data['output_spike_indices'])
                        assert not responses or data['output_step_index'] > responses[-1]['output_step_index']
                        responses.append(data);step=max(step+1,data['output_step_index']+1)
                    reports.append(dict(profile=name, status='pass', sensory=p.sensory, output=p.output,
                        snapshot_sha256=digest, frames=len(responses), output_spikes=sum(len(r['output_spike_indices']) for r in responses),
                        output_digest=hashlib.sha256(json.dumps(responses,sort_keys=True).encode()).hexdigest(),
                        max_round_trip_seconds=max(latencies), elapsed_seconds=time.monotonic()-started,
                        biological_validation='not claimed; snapshot load and bounded sensor/output transport only'))
                    (output / f'{name}-responses.json').write_text(json.dumps(responses,indent=2))
                    print(f'{name}: Rust snapshot and 16 companion round trips passed', flush=True)
                finally:
                    if bridge is not None: stop(bridge)
                    stop(rust)
            log = (output / f'{name}-rust.log').read_text()
            if any(marker in log.lower() for marker in ['failed parsing snapshot', 'startup snapshot import failed', 'continuing with defaults', 'failed reading snapshot']):
                raise RuntimeError(f'{name}: Rust model load fallback detected')
            (output / 'neural.json').write_text(json.dumps(dict(content_digest=content['digest'], profiles=reports),indent=2))
        except Exception as error:
            traceback.print_exc()
            reports.append(dict(profile=profile['id'], status='fail', reason=str(error)))
            (output / 'neural.json').write_text(json.dumps(dict(content_digest=content['digest'], profiles=reports),indent=2))
    return 0 if all(r['status']=='pass' for r in reports) else 1


if __name__ == '__main__':
    sys.exit(main())
