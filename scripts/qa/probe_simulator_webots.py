#!/usr/bin/env python3
"""Load each robot's habitat in a separate Webots process without a brain.

Requires an installed Webots R2025a and its supported display/OpenGL environment.
This is a scene-construction probe, not a dynamics or biological acceptance test.
"""
import argparse
import json
import os
import signal
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
from sim_content import compile_catalog

CASES = {
    'celegans': ('CelegansRobot', 'C_ELEGANS_01'),
    'drosophila-banc': ('DrosophilaBancRobot', 'DROS_BANC_01'),
    'drosophila-fafb': ('DrosophilaFafbRobot', 'DROS_FAFB_01'),
    'hexapod': ('HexapodRobot', 'HEXAPOD_01'),
    'nao': (None, 'NAO_01'),
    'zebrafish': ('ZebrafishRobot', 'ZEBRAFISH_01'),
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--webots', default='webots')
    parser.add_argument('--timeout', type=int, default=90)
    parser.add_argument('--kind', choices=CASES, help='Load one robot profile only')
    args = parser.parse_args()
    base = ROOT / 'target/qa/simulator-content'
    base.mkdir(parents=True, exist_ok=True)
    output = Path(os.environ['NM_SIM_CONTENT_RESULT_DIR']) if 'NM_SIM_CONTENT_RESULT_DIR' in os.environ else Path(tempfile.mkdtemp(prefix='webots-', dir=base))
    output.mkdir(parents=True, exist_ok=True)
    catalog = compile_catalog()
    profiles = {profile['id']: profile for profile in catalog['profiles']}
    habitats = {habitat['id']: habitat for habitat in catalog['habitats']}
    reports = {}
    for kind, (proto, robot_name) in CASES.items():
        if args.kind and kind != args.kind:
            continue
        case_dir = output / kind
        case_dir.mkdir()
        with tempfile.TemporaryDirectory(prefix='nm-content-webots-') as name:
            project = Path(name)
            (project / 'worlds').mkdir()
            controller = project / 'controllers/probe'
            controller.mkdir(parents=True)
            world = project / 'worlds/probe.wbt'
            report = case_dir / 'scene.json'
            command = [sys.executable, str(ROOT / 'scripts/build_webots_multi_world.py'),
                       '--world', str(world), f'--{kind}-brains', kind + '_probe']
            if proto:
                command += [f'--{kind}-proto', str(ROOT / f'webots_world/protos/{proto}.proto')]
            subprocess.run(command, cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
            content = world.read_text().replace('controller "nao_nn_controller_uds"', 'controller "<none>"')
            content = content.replace('controller "nm_world_recorder"', 'controller "probe"')
            assert content.count('controller "<none>"') == 1, kind
            assert content.count('controller "probe"') == 1, kind
            world.write_text(content)
            (controller / 'probe.py').write_text(f'''from controller import Supervisor
from pathlib import Path
import json
import os
import signal
s = Supervisor()
children = s.getRoot().getField("children")
names = []
for i in range(children.getCount()):
    node = children.getMFNode(i)
    name = node.getField("name")
    if name: names.append(name.getSFString())
report = {{"nodes": children.getCount(), "habitat_objects": sum(n.startswith("nm_") for n in names),
          "robots": [n for n in names if n.startswith(("C_ELEGANS", "DROS_", "HEXAPOD", "NAO_", "ZEBRAFISH"))]}}
Path({str(report)!r}).write_text(json.dumps(report))
s.simulationQuit(0)
''')
            with (case_dir / 'webots-load.log').open('w') as log:
                command = [args.webots, '--batch', '--mode=fast', '--no-rendering', '--stdout', '--stderr', str(world)]
                process = subprocess.Popen(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
                try:
                    code = process.wait(timeout=args.timeout)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGTERM)
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        os.killpg(process.pid, signal.SIGKILL)
                        process.wait()
                    raise
                if code:
                    raise subprocess.CalledProcessError(code, command)
            # The file is written before simulationQuit: Webots may close before
            # forwarding the last controller stdout line, so exit status alone is insufficient.
            result = json.loads(report.read_text())
            profile_id = kind.replace('-', '_')
            expected_habitat = habitats[profiles[profile_id]['habitat']]
            assert result['habitat_objects'] == len(expected_habitat['objects']), (kind, result)
            assert result['robots'] == [robot_name], (kind, result)
            log = (case_dir / 'webots-load.log').read_text()
            assert 'ERROR:' not in log and 'should be unique' not in log, (kind, log)
            reports[kind] = result
            print(f'[webots-scene-probe] completed {kind}', flush=True)
    result = dict(digest=catalog['digest'], robots=reports,
                  scope='sequential one-robot scene construction; inspect logs for physical mass-ratio warnings')
    (output / 'webots.json').write_text(json.dumps(result, indent=2))
    print(json.dumps(result))


if __name__ == '__main__':
    main()
