#!/usr/bin/env python3
"""Actual two-same-type + crow reference brains in one native contact world.

Exercises strict identity/mapping handshakes and independent asynchronous
progress. This does not validate contact calibration, trained tasks or durable
output commit. Each engine owns one shared world and each body one real Runner.
"""
import argparse
from contextlib import ExitStack
import json
from pathlib import Path
import subprocess
import sys

from embodiment_lab import ROOT, ReferenceRunner


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--target', choices=['webots', 'unreal'], required=True)
    p.add_argument('--output', type=Path, required=True)
    args = p.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    report = dict(schema_version=1, status='fail', target=args.target, shared_contact_world=True,
        mode='AER1-reference-untrained', learned_task_status='not_run', governed_effects=False,
        scope='distinct same-type identities/mappings and asynchronous real neural exchange; physical calibration independent')
    try:
        with ExitStack() as stack:
            labs = [stack.enter_context(ReferenceRunner(profile, instance, output / instance))
                for instance, profile in [('ugot_mecanum_0', 'ugot_mecanum'),
                    ('ugot_mecanum_1', 'ugot_mecanum'), ('crow_corvus_corone_0', 'crow_corvus_corone')]]
            bindings = {lab.contract['brain_id']: lab.contract for lab in labs}
            manifest = output / 'bindings.json'
            manifest.write_text(json.dumps(bindings, indent=2) + '\n')
            command = [sys.executable, 'scripts/run_webots_embodiment.py'] if args.target == 'webots' else [
                sys.executable, 'scripts/run_native_embodiment.py', '--target', 'unreal']
            command += ['--robots', 'ugot_mecanum=2,crow_corvus_corone=1', '--control', 'aarnn',
                '--headless', '--no-build', '--steps', '1500', '--reference-bindings', str(manifest),
                '--output', str(output / 'physics')]
            if args.target == 'webots': command += ['--realtime']
            report['command'] = command
            with (output / 'engine.log').open('w') as log:
                subprocess.run(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=150)
            path = output / 'physics' / ('result.json' if args.target == 'webots' else 'engine-result.json')
            sample = json.loads(path.read_text())
            worlds = list(sample['worlds'].values()) if args.target == 'webots' else sample['instances']
            report['instances']=worlds
            report['instance_checks']={}
            if len(worlds) != len(labs): raise ValueError('missing mixed-fleet body')
            for world in worlds:
                binding = bindings[world['instance_id']]
                reason=None
                if world['brain_id'] != binding['brain_id'] or world['model_digest'] != binding['model_digest'] or world['io_digest'] != binding['io_digest']:
                    reason='body/brain/model/IO identity cross-wire'
                if not world['transport'] or world['transport'].get('fault') or world['transport']['exchanges'] < 10 or world['applied'] < 1 or world['active_steps'] < 1:
                    reason='reference channel fault: '+str((world['transport'] or {}).get('fault')) if (world['transport'] or {}).get('fault') else 'insufficient independent neural progression/effort'
                if world.get('npc_steps', 0): reason='NPC effort during explicit neural control'
                report['instance_checks'][world['instance_id']]=dict(status='fail' if reason else 'pass',reason=reason)
            if any(c['status']=='fail' for c in report['instance_checks'].values()):
                raise ValueError('mixed-fleet instance failed; inspect instance_checks and retained native transports')
            report.update(status='pass', instances=worlds, evidence=str(path.relative_to(ROOT)),
                brain_count=len(bindings), source_producers=len(worlds), physics_owners=1,
                worker_count='one reference Runner process per brain; runtime internal threads independent')
    except Exception as error:
        report['reason'] = str(error)
    (output / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(dict(status=report['status'], evidence=str(output))))
    return 0 if report['status'] == 'pass' else 1


if __name__ == '__main__':
    raise SystemExit(main())
