#!/usr/bin/env python3
"""Real preserved-network one/three-worker MuJoCo launcher acceptance.

Uses private local ports and task-owned processes only. Viewer evidence is
separate from neural/mechanical exchange and from learned task performance.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import socket
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def probe(output, viewer=False, profile='celegans', workers=None):
    brain=profile+'_0'
    sources = [ROOT / ('network_'+profile+'.json'), ROOT / ('webots_world/configs/config_'+profile+'_webots.json'), ROOT / 'webots_world/worlds/multi_neuroworld.wbt']
    before = {str(p.relative_to(ROOT)): digest(p) for p in sources}
    samples = []
    for index, nodes in enumerate(workers or ((3, 1, 3) if viewer else (3, 1))):
        interactive = viewer and index > 0
        name = ('viewer-default' if nodes == 1 else 'viewer-three') if interactive else f'workers-{nodes}'
        evidence = output / (name + '.json')
        log = output / (name + '.log')
        tcp_port = port()
        startup_budget=300 if profile.startswith('drosophila') or profile=='zebrafish' else 120
        command = ['bash', 'scripts/run_sim.sh', '--sim', 'mujoco', '--robots', profile+'=1', '--no-build', '--tcp-base-port', str(tcp_port), '--tcp-ready-timeout', str(startup_budget), '--output', str(evidence)]
        if nodes == 3:
            command += ['--nodes', '3']
        if interactive:
            command += ['--screenshot', str(output / (name + '.ppm'))]
        else:
            command += ['--headless', '--steps', '1500', '--realtime']
        env = dict(os.environ, NM_DISABLE_OPENCL='1', RAYON_NUM_THREADS='2', NM_CLUSTER_NODES='1')
        start = time.monotonic()
        child = None
        with log.open('w') as handle:
            process = subprocess.Popen(command, cwd=ROOT, env=env, stdout=handle, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                deadline = start + 720
                while process.poll() is None:
                    text = log.read_text()
                    if interactive and ('MuJoCo progress '+brain+':') in text and child is None:
                        # The default viewer survived >2500 actual physics
                        # steps, beyond the former 1000-step automatic exit.
                        ready = re.search(r'^\{.*"instance_id":"'+re.escape(brain)+r'".*\}$', text, re.MULTILINE)
                        assert ready, 'native readiness/PID evidence missing'
                        child = json.loads(ready[0])['pid']
                        assert Path(f'/proc/{child}/exe').resolve().name == 'aarnn-mujoco'
                        os.kill(child, signal.SIGTERM)
                    if time.monotonic() >= deadline:
                        raise TimeoutError(f'{name}: see {log}')
                    time.sleep(.05)
                assert process.returncode == 0, f'{name}: exit {process.returncode}; see {log}'
            finally:
                if process.poll() is None:
                    os.killpg(process.pid, signal.SIGTERM)
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        os.killpg(process.pid, signal.SIGKILL)
                        process.wait()
        world = json.loads(evidence.read_text())['worlds'][0]
        assert world['status'] == 'pass' and world['brain_id'] == brain
        assert world['controller_mode'] == 'AER1-reference' and world['npc_steps'] == 0
        assert world['transport']['ready'] and world['transport']['fault'] is None
        assert world['transport']['exchanges'] >= 10
        assert world['transport']['output_spikes'] > 0 and world['applied'] > 0 and world['active_steps'] > 0
        text = log.read_text()
        assert f'Start reached all {nodes} distributed worker(s)' in text
        assert f'distributed placement confirmed for all 1 network(s)' in text
        runtime = Path(re.search(r'MuJoCo logs and evidence: (.+)', text)[1])
        if nodes == 3:
            assert 'Distributed across 3 nodes' in text
        worker = (runtime / ('webots_'+brain+'.log')).read_text()
        assert 'transition=CPU_reference' in worker
        assert ('consumed AER sensory frame network='+brain) in worker
        # Cleanup must release this run's bridge and workers, not just exit UI.
        with socket.socket() as sock:
            assert sock.connect_ex(('127.0.0.1', tcp_port)) != 0, 'bridge leaked'
        processes = subprocess.check_output(['ps', '-eo', 'args='], text=True)
        assert not any(str(runtime) in line and ('/aarnn_rust ' in line or '/run_webot.sh ' in line) for line in processes.splitlines()), 'run-owned cluster leaked'
        if interactive:
            assert world['steps'] >= 2500 and (output / (name + '.ppm')).stat().st_size > 100000
        samples.append(dict(nodes=nodes, command=command, evidence=str(evidence), log=str(log), runtime=str(runtime), interactive=interactive, elapsed_s=time.monotonic()-start, exchanges=world['transport']['exchanges'], active_steps=world['active_steps']))
    assert before == {str(p.relative_to(ROOT)): digest(p) for p in sources}, 'preserved source/world changed'
    return dict(status='pass', profile=profile, scope='preserved-network native AER1 simulation mechanics/launcher; learned tasks and production commitment unvalidated', preserved_sources=before, runs=samples, viewer='pass' if viewer else 'not_run')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--viewer', action='store_true')
    parser.add_argument('--profile',choices=['celegans','hexapod','drosophila_banc','drosophila_fafb','nao','zebrafish'],default='celegans')
    parser.add_argument('--workers',type=int,nargs='+',choices=[1,3])
    args = parser.parse_args()
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=True)
    result = probe(args.output, args.viewer,args.profile,args.workers)
    (args.output / 'managed.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
