#!/usr/bin/env python3
"""Run one simulator QA lane and retain a unique, machine-readable evidence bundle."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def capture(command):
    try:
        return subprocess.check_output(command, cwd=ROOT, text=True, stderr=subprocess.DEVNULL, timeout=10).strip()
    except (OSError, subprocess.SubprocessError):
        return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--lane', choices=['contract', 'browser', 'webots'], default='contract')
    args = parser.parse_args()
    commands = {
        'contract': ([sys.executable, 'scripts/qa/test_simulator_content.py'], 120),
        'browser': (['node', 'scripts/qa/test_simulator_browser.cjs'], 150),
        'webots': ([sys.executable, 'scripts/qa/probe_simulator_webots.py'], 180),
    }
    command, timeout = commands[args.lane]
    base = ROOT / 'target/qa/simulator-content'
    base.mkdir(parents=True, exist_ok=True)
    output = Path(tempfile.mkdtemp(prefix=args.lane + '-', dir=base))
    env = dict(os.environ, NM_SIM_CONTENT_RESULT_DIR=str(output))
    started = time.monotonic()
    result = dict(scenario='SIM-CONTENT-001', schema_version=1, lane=args.lane,
                  started_utc=datetime.now(timezone.utc).isoformat(), seed=481516,
                  git_revision=capture(['git', 'rev-parse', 'HEAD']), git_dirty=bool(capture(['git', 'status', '--porcelain'])),
                  platform=dict(os=platform.system(), architecture=platform.machine(), device_class='host'),
                  toolchains=dict(python=platform.python_version(), node=capture(['node', '--version']), cargo=capture(['cargo', '--version'])),
                  scenario_digest=sha(ROOT / 'qa/scenarios/SIM-CONTENT-001.toml'),
                  config_digest=sha(ROOT / 'sim/content/catalog.json'),
                  input_digest=sha(ROOT / 'scripts/qa/test_simulator_sensors.cjs'),
                  content_digest=json.loads((ROOT / 'sim/content/compiled.generated.json').read_text())['digest'],
                  capabilities=dict(node=shutil.which('node') is not None, webots=shutil.which('webots') is not None),
                  neural_state_digest=None, event_digest=None, checkpoint=None,
                  neural_execution='none; content/adapter evidence only', command=command, timeout_seconds=timeout)
    code = 1
    with (output / 'run.log').open('w') as log:
        try:
            process = subprocess.Popen(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                code = process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
                result['reason'] = 'lane timeout'
        except OSError as error:
            result['reason'] = str(error)
    result.setdefault('reason', 'all lane checks passed' if code == 0 else 'lane failed; see run.log')
    result.update(status='pass' if code == 0 else 'fail', exit_code=code, elapsed_seconds=time.monotonic() - started)
    try:
        import resource
        usage = resource.getrusage(resource.RUSAGE_CHILDREN)
        result['resources'] = dict(child_cpu_seconds=usage.ru_utime + usage.ru_stime, peak_child_rss_bytes=usage.ru_maxrss * (1 if sys.platform == 'darwin' else 1024))
    except ImportError:
        result['resources'] = dict(reason='host does not expose POSIX child resource usage')
    result['artifacts'] = sorted(p.name for p in output.iterdir()) + ['result.json']
    (output / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print((output / 'run.log').read_text(), end='')
    print(f"{args.lane}: {result['status']}; evidence {output}")
    return 0 if code == 0 else 1


if __name__ == '__main__':
    sys.exit(main())
