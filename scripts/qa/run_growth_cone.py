#!/usr/bin/env python3
"""Retain bounded headless growth-cone and morphology admission evidence."""
from datetime import datetime, timezone
import hashlib
import json
import platform
import subprocess
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def capture(command):
    return subprocess.check_output(command, cwd=ROOT, text=True).strip()


def main():
    base = ROOT / 'target/qa/growth-cone'
    base.mkdir(parents=True, exist_ok=True)
    output = Path(tempfile.mkdtemp(prefix='run-', dir=base))
    command = ['cargo', 'test', '--locked', 'morphology_contract::', '--lib']
    result = dict(scenario='MORPH-GROW-001', schema_version=1, seed=42,
                  started_utc=datetime.now(timezone.utc).isoformat(),
                  git_revision=capture(['git', 'rev-parse', 'HEAD']),
                  dirty=bool(capture(['git', 'status', '--porcelain'])),
                  platform=platform.platform(), cargo=capture(['cargo', '--version']),
                  command=command, fixture_digests={},
                  profile='default host features; headless reference tests',
                  limitations='No live/distributed activation, calibrated biology, mobile execution or sustained-load latency evidence.')
    for name in ['qa/scenarios/MORPH-GROW-001.toml', 'qa/fixtures/morphology/growth-cone-v1.json',
                 'src/morphology_contract/growth_cone.rs', 'src/morphology_contract.rs', 'src/deterministic.rs']:
        result['fixture_digests'][name] = hashlib.sha256((ROOT / name).read_bytes()).hexdigest()
    started = time.monotonic()
    print('Running:', ' '.join(command), flush=True)
    with (output / 'run.log').open('w') as log:
        try:
            run = subprocess.run(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, timeout=600)
            result.update(exit_code=run.returncode, reason=None)
        except (OSError, subprocess.TimeoutExpired) as error:
            result.update(exit_code=1, reason=str(error))
    result.update(seconds=time.monotonic() - started, passed=result['exit_code'] == 0)
    (output / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(('PASS' if result['passed'] else 'FAIL') + ': ' + str(output), flush=True)
    return 0 if result['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
