"""Isolated real Runner fixtures shared by native embodiment acceptance probes."""
import json
import os
from pathlib import Path
import socket
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]


class ReferenceRunner:
    """Own one fresh untrained CPU-reference brain, never an external endpoint."""
    def __init__(self, profile, instance, output):
        self.output = output
        output.mkdir(parents=True, exist_ok=True)
        self.record = json.loads((ROOT / 'sim/content/embodiments/generated' / (profile + '.json')).read_text())
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        io = self.record['io']
        self.contract = dict(brain_id=instance, brain_generation=1, model_digest=self.record['digest'],
            io_digest=io['digest'], s_names=[s['name'] for s in io['sensors']], o_names=io['output_names'],
            host='127.0.0.1', port=port)
        self.manifest = output / 'brain.json'
        self.manifest.write_text(json.dumps(self.contract, indent=2) + '\n')
        self.command = [str(ROOT / 'target/release/examples/nn_tcp_server'), '--tcp', f'127.0.0.1:{port}',
            '--config', str(ROOT / 'sim/content/embodiments/generated' / (profile + '.config.json'))]
        self.process = None
        self.log = None

    def __enter__(self):
        self.log = (self.output / 'runner.log').open('w')
        self.process = subprocess.Popen(self.command, cwd=ROOT, env=dict(os.environ,
            NM_AER_REFERENCE_CONTRACT=str(self.manifest.resolve()), RAYON_NUM_THREADS='2',
            NM_DISABLE_OPENCL='1'), stdout=self.log, stderr=subprocess.STDOUT)
        try:
            deadline = time.monotonic() + 45
            while time.monotonic() < deadline:
                if self.process.poll() is not None:
                    raise RuntimeError('Runner exited; see runner.log')
                text = (self.output / 'runner.log').read_text()
                if 'listening on' in text:
                    if 'transition=CPU_reference' not in text:
                        raise RuntimeError('CPU reference backend not evidenced')
                    return self
                time.sleep(.02)
            raise RuntimeError('Runner readiness timeout')
        except BaseException:
            self.close()
            raise

    def close(self):
        if self.process is not None and self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
        if self.log is not None:
            self.log.close()

    def __exit__(self, *_):
        self.close()
