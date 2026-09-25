#!/usr/bin/env python3
"""Retain repeatable native-mesh and browser anatomy regression evidence."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]


def capture(command):
    return subprocess.check_output(command, cwd=ROOT, text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--browser', action='store_true')
    parser.add_argument('--sustained', action='store_true', help='capture 1,200 steps of shipped-network growth and render the progression')
    args = parser.parse_args()
    base = ROOT / 'target/qa/anatomy'
    base.mkdir(parents=True, exist_ok=True)
    output = Path(tempfile.mkdtemp(prefix='run-', dir=base))
    result = dict(scenario='MORPH-VIS-002' if args.sustained else 'MORPH-VIS-001', schema_version=1, seed=42 if args.sustained else 0,
                  started_utc=datetime.now(timezone.utc).isoformat(),
                  git_revision=capture(['git', 'rev-parse', 'HEAD']),
                  dirty=bool(capture(['git', 'status', '--porcelain'])),
                  platform=platform.platform(),
                  cargo=capture(['cargo', '--version']), node=capture(['node', '--version']),
                  browser_requested=args.browser, native_evidence='CPU mesh builder',
                  mobile='not exercised by this fixture lane; device evidence required separately', commands=[], fixture_digests={})
    for name in ['qa/scenarios/MORPH-VIS-001.toml', 'qa/scenarios/MORPH-VIS-002.toml', 'config.json', 'qa/fixtures/morphology/anatomical-stability.json', 'scripts/qa/test_anatomical_render.cjs', 'src/engine.rs', 'src/runner.rs', 'src/ui.rs', 'src/ui/anatomy.rs', 'src/morphology.rs', 'src/morphology_contract.rs', 'web_ui/app.js']:
        result['fixture_digests'][name] = hashlib.sha256((ROOT / name).read_bytes()).hexdigest()
    commands = [(['cargo', 'test', '--locked', '--features', 'ui,engine_runtime,morpho', 'anatomy_', '--lib'], 300)]
    if args.sustained:
        commands.extend([
            (['cargo', 'test', '--locked', '--features', 'ui,engine_runtime,morpho', 'anatomy_sustained_growth_capture', '--lib', '--', '--ignored'], 300),
            (['cargo', 'test', '--locked', '--features', 'ui,engine_runtime,morpho', 'anatomy_captured_growth_meshes', '--lib', '--', '--ignored'], 120),
        ])
    commands.append((['node', 'scripts/qa/test_anatomical_render.cjs'] + (['--browser'] if args.browser else []) + (['--sustained'] if args.sustained else []), 90))
    env = dict(os.environ, ANATOMY_QA_DIR=str(output))
    if args.sustained:
        env['NM_MORPHO_ASYNC'] = '0'
    passed = True
    with (output / 'run.log').open('w') as log:
        for command, timeout in commands:
            print('Running:', ' '.join(command), flush=True)
            started = time.monotonic()
            try:
                with subprocess.Popen(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=os.name == 'posix') as process:
                    try:
                        code, reason = process.wait(timeout=timeout), None
                    except subprocess.TimeoutExpired:
                        # Cargo/Chrome launch children: stop this QA process
                        # group as well, never leave an orphaned test running.
                        if os.name == 'posix':
                            try:
                                os.killpg(process.pid, signal.SIGKILL)
                            except ProcessLookupError:
                                pass
                        else:
                            process.kill()
                        process.wait()
                        raise
            except (OSError, subprocess.TimeoutExpired) as error:
                code, reason = 1, str(error)
            result['commands'].append(dict(command=command, exit_code=code, seconds=time.monotonic()-started, reason=reason))
            passed &= code == 0
            if code != 0:
                break  # later render checks depend on complete capture files
    result['passed'] = passed
    (output / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(('PASS' if passed else 'FAIL') + ': ' + str(output), flush=True)
    return 0 if passed else 1


if __name__ == '__main__':
    raise SystemExit(main())
