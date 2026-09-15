#!/usr/bin/env python3
"""Load shared habitats and six robot PROTOs in Webots without connecting a brain.

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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--webots', default='webots')
    parser.add_argument('--timeout', type=int, default=90)
    args = parser.parse_args()
    base = ROOT / 'target/qa/simulator-content'
    base.mkdir(parents=True, exist_ok=True)
    output = Path(os.environ['NM_SIM_CONTENT_RESULT_DIR']) if 'NM_SIM_CONTENT_RESULT_DIR' in os.environ else Path(tempfile.mkdtemp(prefix='webots-', dir=base))
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='nm-content-webots-') as name:
        project = Path(name)
        (project / 'worlds').mkdir()
        controller = project / 'controllers/probe'
        controller.mkdir(parents=True)
        world = project / 'worlds/probe.wbt'
        report = project / 'report.json'
        command = [sys.executable, str(ROOT / 'scripts/build_webots_multi_world.py'), '--world', str(world)]
        for kind, proto in [('celegans', 'CelegansRobot'), ('drosophila-banc', 'DrosophilaBancRobot'),
                            ('drosophila-fafb', 'DrosophilaFafbRobot'), ('hexapod', 'HexapodRobot'),
                            ('zebrafish', 'ZebrafishRobot')]:
            command += [f'--{kind}-brains', kind + '_probe', f'--{kind}-proto', str(ROOT / f'webots_world/protos/{proto}.proto')]
        command += ['--nao-brains', 'nao_probe']
        subprocess.run(command, cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
        text = world.read_text().replace('controller "nao_nn_controller_uds"', 'controller "<none>"')
        text = text.replace('controller "nm_world_recorder"', 'controller "<none>"')
        world.write_text(text + '\nRobot { supervisor TRUE controller "probe" }\n')
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
        with (output / 'webots-load.log').open('w') as log:
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
        data = compile_catalog()
        assert result['habitat_objects'] == sum(len(h['objects']) for h in data['habitats']), result
        assert len(result['robots']) == 6, result
        log = (output / 'webots-load.log').read_text()
        assert 'ERROR:' not in log and 'should be unique' not in log, log
        result.update(digest=data['digest'], scope='scene construction only; inspect log for physical mass-ratio warnings')
        (output / 'webots.json').write_text(json.dumps(result, indent=2))
        print(json.dumps(result))


if __name__ == '__main__':
    main()
