#!/usr/bin/env python3
"""Probe each Webots robot's motors in a separate, sequential Webots process.

This uses a generated, disposable world and synthetic commands. It does not
assert neural provenance or biological locomotion.
"""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[2]
CASES = {
    'celegans': ('CelegansRobot', 'C_ELEGANS_01'),
    'drosophila-banc': ('DrosophilaBancRobot', 'DROS_BANC_01'),
    'drosophila-fafb': ('DrosophilaFafbRobot', 'DROS_FAFB_01'),
    'hexapod': ('HexapodRobot', 'HEXAPOD_01'),
    'nao': (None, 'NAO_01'),
    'zebrafish': ('ZebrafishRobot', 'ZEBRAFISH_01'),
}

ROBOT_CONTROLLER = '''from controller import Robot, Motor
import json, math, os
from pathlib import Path

robot = Robot()
step_ms = int(robot.getBasicTimeStep())
motors = [robot.getDeviceByIndex(i) for i in range(robot.getNumberOfDevices())]
motors = [motor for motor in motors if isinstance(motor, Motor)]

def channel_only(name):
    return name.startswith(('celegans_o_', 'dros_o_'))

real = []
channel_count = 0
for motor in motors:
    name = motor.getName()
    if channel_only(name):
        channel_count += 1
        continue
    lo, hi = motor.getMinPosition(), motor.getMaxPosition()
    speed = motor.getMaxVelocity()
    if not all(map(math.isfinite, (lo, hi, speed))) or lo >= hi or speed <= 0:
        continue
    sensor = motor.getPositionSensor()
    if sensor:
        sensor.enable(step_ms)
    real.append((motor, sensor, lo, hi, speed))

robot.step(step_ms)
initial = [sensor.getValue() if sensor else None for _, sensor, _, _, _ in real]
accepted = 0
for motor, _, lo, hi, _ in real:
    target = (lo + hi) * 0.5 + (hi - lo) * 0.1
    motor.setPosition(target)
    if abs(motor.getTargetPosition() - target) < 1e-6:
        accepted += 1

for _ in range(20):
    if robot.step(step_ms) == -1:
        break

feedback = [abs(sensor.getValue() - before)
            for (_, sensor, _, _, _), before in zip(real, initial) if sensor]
result = {
    'robot': robot.getName(),
    'motors': len(motors),
    'channel_only': channel_count,
    'physical_motors': len(real),
    'commands_accepted': accepted,
    'position_sensors': len(feedback),
    'joints_changed': sum(delta > 1e-4 for delta in feedback),
    'max_position_change': max(feedback, default=0.0),
    'max_motor_speed': max((speed for _, _, _, _, speed in real), default=0.0),
}
path = Path(os.environ['NM_PROBE_DIR']) / (robot.getName() + '.json')
path.write_text(json.dumps(result, indent=2))
print('[actuator-probe] ' + json.dumps(result), flush=True)
'''

FINISH_CONTROLLER = '''from controller import Supervisor
from pathlib import Path
import json, math, os

supervisor = Supervisor()
root = Path(os.environ['NM_PROBE_DIR'])
expected = os.environ['NM_PROBE_ROBOT']

def spine_positions(robot_name):
    children = supervisor.getRoot().getField('children')
    for i in range(children.getCount()):
        robot = children.getMFNode(i)
        name = robot.getField('name')
        if not name or name.getSFString() != robot_name:
            continue
        field = robot.getBaseNodeField('children')
        if not field:
            return []
        joint = next((field.getMFNode(j) for j in range(field.getCount())
                      if field.getMFNode(j).getBaseTypeName() == 'HingeJoint'), None)
        positions = []
        while joint:
            end = joint.getField('endPoint').getSFNode()
            if not end:
                break
            positions.append(end.getPosition())
            nested = end.getField('children')
            joint = next((nested.getMFNode(j) for j in range(nested.getCount())
                          if nested.getMFNode(j).getBaseTypeName() == 'HingeJoint'), None)
        return positions
    return []

initial = spine_positions(expected) if expected in ('C_ELEGANS_01', 'ZEBRAFISH_01') else []
for _ in range(120):
    if supervisor.step(int(supervisor.getBasicTimeStep())) == -1:
        break
    if (root / (expected + '.json')).exists():
        feedback = {}
        if initial:
            after = spine_positions(expected)
            delta = [math.dist(start, end) for start, end in zip(initial, after)]
            feedback[expected] = {'segments': len(delta),
                                  'segments_changed': sum(value > 1e-4 for value in delta),
                                  'max_displacement_m': max(delta, default=0.0)}
        (root / 'posture_feedback.json').write_text(json.dumps(feedback, indent=2))
        supervisor.simulationQuit(0)
        break
else:
    supervisor.simulationQuit(1)
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--kind', choices=CASES, help='Probe one robot profile only')
    parser.add_argument('--webots', default='webots')
    args = parser.parse_args()
    base = ROOT / 'target/qa/webots-actuator-sensitivity'
    base.mkdir(parents=True, exist_ok=True)
    output = Path(tempfile.mkdtemp(prefix='run-', dir=base))
    reports = {}
    posture = {}
    for kind, (proto, robot_name) in CASES.items():
        if args.kind and kind != args.kind:
            continue
        case_dir = output / kind
        case_dir.mkdir()
        with tempfile.TemporaryDirectory(prefix='nm-actuator-webots-', dir=base) as folder:
            project = Path(folder)
            world_dir = project / 'worlds'
            world_dir.mkdir()
            for name, source in [('probe_actuators', ROBOT_CONTROLLER),
                                 ('probe_finish', FINISH_CONTROLLER)]:
                controller_dir = project / 'controllers' / name
                controller_dir.mkdir(parents=True)
                (controller_dir / (name + '.py')).write_text(source)
            world = world_dir / 'probe.wbt'
            command = [sys.executable, str(ROOT / 'scripts/build_webots_multi_world.py'),
                       '--world', str(world), f'--{kind}-brains', kind + '_probe']
            if proto:
                command += [f'--{kind}-proto', str(ROOT / f'webots_world/protos/{proto}.proto')]
            subprocess.run(command, cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
            content = world.read_text().replace('controller "nao_nn_controller_uds"',
                                                 'controller "probe_actuators"')
            content = content.replace('controller "nm_world_recorder"',
                                      'controller "probe_finish"')
            assert content.count('controller "probe_actuators"') == 1, kind
            world.write_text(content)
            env = dict(os.environ, NM_PROBE_DIR=str(case_dir), NM_PROBE_ROBOT=robot_name)
            with (case_dir / 'webots.log').open('w') as log:
                subprocess.run([args.webots, '--batch', '--mode=fast', '--no-rendering',
                                '--stdout', '--stderr', str(world)], cwd=ROOT, env=env,
                               stdout=log, stderr=subprocess.STDOUT, timeout=90,
                               check=True)
        reports[robot_name] = json.loads((case_dir / (robot_name + '.json')).read_text())
        posture.update(json.loads((case_dir / 'posture_feedback.json').read_text()))
        print(f'[webots-actuator-probe] completed {kind}', flush=True)
    for name, report in reports.items():
        assert report['physical_motors'] > 0, name
        assert report['commands_accepted'] == report['physical_motors'], name
    for name in reports.keys() & {'C_ELEGANS_01', 'ZEBRAFISH_01'}:
        assert posture[name]['segments'] >= 4, name
        assert posture[name]['segments_changed'] > 0, name
    (output / 'report.json').write_text(json.dumps({'robots': reports, 'posture_feedback': posture}, indent=2))
    print(json.dumps({'result_dir': str(output), 'robots': reports,
                      'posture_feedback': posture}, indent=2))


if __name__ == '__main__':
    main()
