#!/usr/bin/env python3
"""Exercise every Webots robot's real controller with bounded synthetic neural output.

The temporary UDS responders provide an output frame after each sensory frame.
This validates the controller/bridge/motor-command path without claiming that
the synthetic spikes came from a committed brain or caused locomotion.
"""

import argparse
import json
import os
from pathlib import Path
import re
import socket
import struct
import subprocess
import sys
import tempfile
import threading


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
from sim_content import compile_catalog

KINDS = {
    'celegans': ('CelegansRobot', 96),
    'drosophila-banc': ('DrosophilaBancRobot', 48),
    'drosophila-fafb': ('DrosophilaFafbRobot', 48),
    'hexapod': ('HexapodRobot', 18),
    'nao': (None, 40),
    'zebrafish': ('ZebrafishRobot', 32),
}
ROBOT_KINDS = {
    'C_ELEGANS_01': 'celegans',
    'DROS_BANC_01': 'drosophila-banc',
    'DROS_FAFB_01': 'drosophila-fafb',
    'HEXAPOD_01': 'hexapod',
    'NAO_01': 'nao',
    'ZEBRAFISH_01': 'zebrafish',
}
SENSORY_COUNTS = {
    'celegans': 24,
    'drosophila-banc': 418,
    'drosophila-fafb': 418,
    'hexapod': 34,
    'nao': 250,
    'zebrafish': 32,
}

FINISH_CONTROLLER = '''from controller import Supervisor
import json, math, os
from pathlib import Path

s = Supervisor()

def spine_positions(robot_name):
    children = s.getRoot().getField('children')
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

def robot_height(robot_name):
    children = s.getRoot().getField('children')
    for i in range(children.getCount()):
        robot = children.getMFNode(i)
        name = robot.getField('name')
        if name and name.getSFString() == robot_name:
            return robot.getPosition()[2]
    return None

def robot_upright(robot_name):
    children = s.getRoot().getField('children')
    for i in range(children.getCount()):
        robot = children.getMFNode(i)
        name = robot.getField('name')
        if name and name.getSFString() == robot_name:
            orientation = robot.getOrientation()
            # Local +Y is the fly's upward axis before its world placement.
            return orientation[7]
    return None

name = os.environ['NM_PROBE_ROBOT']
kind = os.environ['NM_PROBE_KIND']
scale = int(os.environ['NM_PROBE_STEP_SCALE'])
has_spine = kind in ('celegans', 'zebrafish')
is_fly = kind.startswith('drosophila-')
before = {}
after = {}
flight = {}
aquatic = {}
for step in range(90 * scale):
    if s.step(int(s.getBasicTimeStep())) == -1:
        break
    if step == 7 * scale:
        if has_spine:
            before = spine_positions(name)
        if is_fly:
            flight['baseline_m'] = {name: robot_height(name)}
            flight['upright_at_baseline'] = {name: robot_upright(name)}
        if kind == 'zebrafish':
            aquatic['baseline_m'] = robot_height(name)
    if step == 55 * scale:
        if has_spine:
            after = spine_positions(name)
        if is_fly:
            flight['driven_m'] = {name: robot_height(name)}
            flight['upright_at_drive'] = {name: robot_upright(name)}
        if kind == 'zebrafish':
            aquatic['driven_m'] = robot_height(name)
    if step == 88 * scale and is_fly:
        flight['recovered_m'] = {name: robot_height(name)}
    if step == 88 * scale and kind == 'zebrafish':
        aquatic['recovered_m'] = robot_height(name)

report = {}
if has_spine:
    initial_root = before[0]
    driven_root = after[0]
    delta = [math.dist(tuple(a - b for a, b in zip(start, initial_root)),
                       tuple(a - b for a, b in zip(end, driven_root)))
             for start, end in zip(before, after)]
    report[name] = {'segments': len(delta),
                    'segments_changed': sum(value > 1e-4 for value in delta),
                    'max_relative_displacement_m': max(delta, default=0.0)}
Path(os.environ['NM_PROBE_DIR'], 'posture_feedback.json').write_text(json.dumps({'segments': report, 'flight': flight, 'aquatic': aquatic}))
s.simulationQuit(0)
'''


def frame(kind, count, sequence, worm_ventral):
    output = [0.5] * count
    scale = 4 if kind.startswith('drosophila-') else 1
    if 8 * scale <= sequence < 58 * scale:
        stimulated = 0.65 if sequence < 32 * scale else 1.0
        if kind == 'celegans':
            for i in worm_ventral:
                output[i] = stimulated
        elif kind in ('drosophila-banc', 'drosophila-fafb'):
            for i in range(count // 2):
                output[i] = stimulated
        else:
            for i in range(0, count, 2):
                output[i] = stimulated
    return struct.pack('<' + 'f' * count, *output)


def serve(sock, kind, count, worm_ventral, stop, tally, sensory):
    sock.settimeout(0.2)
    sequence = 0
    while not stop.is_set():
        try:
            packet, peer = sock.recvfrom(1 << 20)
        except socket.timeout:
            continue
        if packet.startswith(b'{'):
            try:
                handshake = json.loads(packet)
                sensory[kind]['handshake_width'] = handshake['sensory']
                sensory[kind]['handshake_names'] = handshake.get('s_names', [])
            except (ValueError, KeyError):
                sensory[kind]['handshake_width'] = -1
            continue
        expected_bytes = 4 * (1 + SENSORY_COUNTS[kind])
        if len(packet) != expected_bytes:
            sensory[kind]['bad_frames'] += 1
            continue
        values = struct.unpack_from('<' + 'f' * SENSORY_COUNTS[kind], packet, 4)
        capture = sensory[kind]
        capture['frames'] += 1
        for index, value in enumerate(values):
            if not (0.0 <= value <= 1.0):
                capture['bad_values'] += 1
                continue
            capture['minimum'][index] = min(capture['minimum'][index], value)
            capture['maximum'][index] = max(capture['maximum'][index], value)
        try:
            sock.sendto(frame(kind, count, sequence, worm_ventral), peer)
        except OSError:
            continue
        sequence += 1
        tally[kind] = sequence


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--kind', choices=KINDS, help='Run one robot profile only')
    args = parser.parse_args()
    cases = {kind: value for kind, value in KINDS.items()
             if args.kind is None or kind == args.kind}
    base = ROOT / 'target/qa/webots-neural-actuators'
    base.mkdir(parents=True, exist_ok=True)
    output = Path(tempfile.mkdtemp(prefix='run-', dir=base))
    profile = next(p for p in compile_catalog()['profiles'] if p['id'] == 'celegans')
    worm_ventral = [i for i, name in enumerate(profile['output_names'])
                    if re.search(r'_(?:MVL|MVR)\d{2}$', name)]
    assert worm_ventral

    tally = {}
    sensory = {}
    feedback = {'segments': {}, 'flight': {}, 'aquatic': {}}
    logs = {}
    # A separate Webots process is essential: the whole fleet mixes gram-scale
    # insects and fish with kilogram-scale robots and can exhaust desktop RAM.
    for kind, (proto, count) in cases.items():
        robot_name = next(name for name, profile in ROBOT_KINDS.items() if profile == kind)
        case_dir = output / kind
        case_dir.mkdir()
        with tempfile.TemporaryDirectory(prefix='nm-neural-webots-', dir=base) as project_str, \
             tempfile.TemporaryDirectory(prefix='nm-neural-sockets-') as sockets_str:
            project = Path(project_str)
            world_dir = project / 'worlds'
            world_dir.mkdir()
            controller_dir = project / 'controllers/probe_finish'
            controller_dir.mkdir(parents=True)
            (controller_dir / 'probe_finish.py').write_text(FINISH_CONTROLLER)
            (project / 'controllers/nao_nn_controller_uds').symlink_to(
                ROOT / 'webots_world/controllers/nao_nn_controller_uds',
                target_is_directory=True,
            )
            world = world_dir / 'probe.wbt'
            brain_id = kind + '_probe'
            command = [sys.executable, str(ROOT / 'scripts/build_webots_multi_world.py'),
                       '--world', str(world), f'--{kind}-brains', brain_id]
            if proto:
                command += [f'--{kind}-proto',
                            str(ROOT / f'webots_world/protos/{proto}.proto')]
            subprocess.run(command, cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
            content = world.read_text().replace('controller "nm_world_recorder"',
                                                 'controller "probe_finish"')
            assert re.findall(r'"NM_BRAINS=([^"]+)"', content) == [brain_id]
            path = Path(sockets_str) / (brain_id + '.nn')
            sock = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
            sock.bind(str(path))
            content = content.replace(
                f'"NM_BRAINS={brain_id}"',
                f'"NM_BRAINS={brain_id}"\n    "NM_NAO_SOCKET={path}"', 1)
            world.write_text(content)
            sensory[kind] = {
                'handshake_width': None, 'handshake_names': [], 'frames': 0, 'bad_frames': 0,
                'bad_values': 0, 'minimum': [1.0] * SENSORY_COUNTS[kind],
                'maximum': [0.0] * SENSORY_COUNTS[kind],
            }
            stop = threading.Event()
            thread = threading.Thread(target=serve,
                                      args=(sock, kind, count, worm_ventral, stop, tally, sensory),
                                      daemon=True)
            thread.start()
            env = dict(os.environ, NM_WEBOTS_MOTOR_DEBUG_INTERVAL='10',
                       NM_PROBE_DIR=str(case_dir), NM_PROBE_KIND=kind,
                       NM_PROBE_ROBOT=robot_name,
                       NM_PROBE_STEP_SCALE='4' if kind.startswith('drosophila-') else '1',
                       NM_IPC_DISABLE_AER='1',
                       NM_IPC_HANDSHAKE_INCLUDE_NAMES='1',
                       NM_IPC_HANDSHAKE_MAX_BYTES='65536',
                       NM_CAMERA_RETINA_WIDTH='8', NM_CAMERA_RETINA_HEIGHT='6',
                       NM_DROS_CAMERA_RETINA_WIDTH='12', NM_DROS_CAMERA_RETINA_HEIGHT='8',
                       NM_CAMERA_RETINA_WIDTH_HEX_S_26_HEAD_CAMERA='1',
                       NM_CAMERA_RETINA_HEIGHT_HEX_S_26_HEAD_CAMERA='1',
                       NM_CAMERA_RETINA_WIDTH_ZEBRAFISH_S_16_EYE_LEFT='1',
                       NM_CAMERA_RETINA_HEIGHT_ZEBRAFISH_S_16_EYE_LEFT='1',
                       NM_CAMERA_RETINA_WIDTH_ZEBRAFISH_S_18_EYE_RIGHT='1',
                       NM_CAMERA_RETINA_HEIGHT_ZEBRAFISH_S_18_EYE_RIGHT='1',
                       NM_CELEGANS_BRIDGE_DEBUG_INTERVAL='10',
                       NM_DROS_BRIDGE_DEBUG_INTERVAL='10',
                       NM_DROS_FLIGHT_DEBUG='1',
                       NM_HEXAPOD_BRIDGE_DEBUG_INTERVAL='10',
                       NM_NAO_BRIDGE_DEBUG_INTERVAL='10',
                       NM_CELEGANS_TWITCH_FALLBACK='0',
                       NM_DROS_TWITCH_FALLBACK='0',
                       NM_HEXAPOD_TWITCH_FALLBACK='0',
                       NM_NAO_TWITCH_FALLBACK='0')
            try:
                with (case_dir / 'webots.log').open('w') as log_file:
                    subprocess.run(['webots', '--batch', '--mode=fast', '--no-rendering',
                                    '--stdout', '--stderr', str(world)], cwd=ROOT,
                                   env=env, stdout=log_file, stderr=subprocess.STDOUT,
                                   timeout=90, check=True)
            finally:
                stop.set()
                thread.join(timeout=1)
                sock.close()
            case_feedback = json.loads((case_dir / 'posture_feedback.json').read_text())
            feedback['segments'].update(case_feedback['segments'])
            for phase, values in case_feedback['flight'].items():
                feedback['flight'].setdefault(phase, {}).update(values)
            if case_feedback['aquatic']:
                feedback['aquatic'][robot_name] = case_feedback['aquatic']
            logs[kind] = (case_dir / 'webots.log').read_text()
            print(f'[neural-actuator-probe] completed {kind}', flush=True)

    results = {}
    for robot, kind in ROBOT_KINDS.items():
        if kind not in cases:
            continue
        log = logs[kind]
        rows = re.findall(
            rf'\[DeviceMapper\] motor diag robot={robot} active=(\d+)/(\d+) '
            r'max_target_fraction=([\d.e+-]+) target_mismatches=(\d+)', log)
        assert rows, (robot, output / kind)
        assert any(int(active) > 0 and float(fraction) > 0.01 and
                   int(mismatches) == 0
                   for active, _, fraction, mismatches in rows), (robot, rows)
        fractions = [float(row[2]) for row in rows]
        scale = 4 if kind.startswith('drosophila-') else 1
        assert len(fractions) >= 8 * scale - 1, (robot, fractions)
        moderate_fraction = max(fractions[:3 * scale])
        strong_fraction = max(fractions[3 * scale:5 * scale])
        recovered_fraction = fractions[-1]
        assert moderate_fraction > 0.02, (robot, fractions)
        assert strong_fraction > moderate_fraction, (robot, fractions)
        assert recovered_fraction < moderate_fraction, (robot, fractions)
        assert all(int(row[3]) == 0 for row in rows), (robot, rows)
        results[robot] = {
            'frames_replied': tally.get(kind, 0),
            'samples': len(rows),
            'max_active_motors': max(int(row[0]) for row in rows),
            'physical_motors': int(rows[0][1]),
            'moderate_target_fraction': moderate_fraction,
            'strong_target_fraction': strong_fraction,
            'recovered_target_fraction': recovered_fraction,
        }
        if kind.startswith('drosophila-'):
            # Each isolated process has one fly. Group its diagnostic rows by
            # the known drive schedule rather than depending on line adjacency.
            leg_rows = [(float(drive), float(leg)) for drive, leg in re.findall(
                r'Dros bridge diag dros_drive_abs\[mean,max\]=\[[^,]+,([\d.e+-]+)\]'
                r'[^\n]*legs\[min,max\]=\[[^,]+,([\d.e+-]+)\]', log)]
            moderate_legs = [leg for drive, leg in leg_rows if 0.1 <= drive < 0.2]
            strong_legs = [leg for drive, leg in leg_rows if drive >= 0.3]
            recovered_legs = [leg for drive, leg in leg_rows if drive == 0.0]
            assert moderate_legs and strong_legs and recovered_legs, (robot, leg_rows)
            assert max(moderate_legs) > 0.52, (robot, moderate_legs)
            assert max(strong_legs) > max(moderate_legs), (robot, leg_rows)
            assert min(recovered_legs) < max(moderate_legs), (robot, leg_rows)
            results[robot]['moderate_leg_command'] = max(moderate_legs)
            results[robot]['strong_leg_command'] = max(strong_legs)
    assert all(count >= 20 for count in tally.values()), tally
    sensory_report = {}
    profiles = {profile['id']: profile for profile in compile_catalog()['profiles']}
    nao_model_names = json.loads((ROOT / 'network_nao.json').read_text())['connectome_labels']['sensory_nodes']
    for kind, capture in sensory.items():
        assert capture['handshake_width'] == SENSORY_COUNTS[kind], (kind, capture['handshake_width'])
        assert capture['frames'] >= 20 and capture['bad_frames'] == 0, (kind, capture)
        assert capture['bad_values'] == 0, (kind, capture['bad_values'])
        profile_id = kind.replace('-', '_')
        expected_names = nao_model_names if kind == 'nao' else profiles[profile_id]['sensor_names']
        received_names = capture['handshake_names']
        if expected_names:
            assert received_names == expected_names, (kind, [
                (index, expected, received_names[index] if index < len(received_names) else None)
                for index, expected in enumerate(expected_names)
                if index >= len(received_names) or expected != received_names[index]
            ][:12])
        varied = sum(high - low > 0.03 for low, high in zip(capture['minimum'], capture['maximum']))
        sensory_report[kind] = {
            'handshake_width': capture['handshake_width'],
            'frames': capture['frames'],
            'channels_with_range_gt_0_03': varied,
            'channels_seen_above_0_6': sum(high > 0.6 for high in capture['maximum']),
            'channels_always_above_0_99': sum(low > 0.99 for low in capture['minimum']),
            'always_high_indices': [index for index, low in enumerate(capture['minimum']) if low > 0.99],
            'varied_indices': [index for index, (low, high) in enumerate(zip(capture['minimum'], capture['maximum'])) if high - low > 0.03],
        }
        assert varied > 0, (kind, sensory_report[kind])
    posture = feedback['segments']
    for robot in posture:
        assert posture[robot]['segments'] >= 4, (robot, posture)
        assert posture[robot]['segments_changed'] > 0, (robot, posture)
    flight = feedback['flight']
    for robot in (name for name, kind in ROBOT_KINDS.items()
                  if kind in cases and kind.startswith('drosophila-')):
        assert all(flight[phase][robot] is not None for phase in
                   ('baseline_m', 'driven_m', 'recovered_m')), (robot, flight)
        assert flight['driven_m'][robot] > flight['baseline_m'][robot] + 0.05, (robot, flight)
        assert flight['recovered_m'][robot] > -0.01, (robot, flight)
        assert flight['recovered_m'][robot] < flight['driven_m'][robot] - 0.05, (robot, flight)
        assert flight['upright_at_drive'][robot] > 0.5, (robot, flight)
    if 'zebrafish' in cases:
        fish = feedback['aquatic']['ZEBRAFISH_01']
        assert all(fish[phase] is not None for phase in ('baseline_m', 'driven_m', 'recovered_m'))
        assert all(0.06 <= fish[phase] <= 0.24 for phase in fish), fish
    (output / 'report.json').write_text(json.dumps(
        {'robots': results, 'frames': tally, 'sensory': sensory_report,
         'posture_feedback': posture, 'flight_height': flight,
         'aquatic_height': feedback['aquatic']}, indent=2))
    print(json.dumps({'result_dir': str(output), 'robots': results,
                      'frames': tally,
                      'sensory': {kind: {key: value for key, value in report.items()
                                         if key not in ('varied_indices', 'always_high_indices')}
                                  for kind, report in sensory_report.items()},
                      'posture_feedback': posture, 'flight_height': flight,
                      'aquatic_height': feedback['aquatic']}, indent=2))


if __name__ == '__main__':
    main()
