#!/usr/bin/env python3
"""Regenerate visual assets only; never rewrite networks, configs or editor state."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

import sim_content as content
import build_webots_celegans_assets as worm
import build_webots_drosophila_assets as fly
import build_webots_zebrafish_assets as fish
import build_webots_multi_world as multi


def generated_assets():
    root = content.ROOT
    data = content.compile_catalog()
    yield from content.outputs(data)
    # Rebase scratch world references to the canonical worlds/protos layout.
    with tempfile.TemporaryDirectory() as folder:
        scratch = Path(folder) / 'asset'
        profile = next(p for p in data['profiles'] if p['id'] == 'celegans')
        labels = [name.split('_', 3)[-1] for name in profile['output_names']]
        worm.generate_proto(labels, scratch, '../../network_celegans.json', '../configs/config_celegans_webots.json')
        yield root / 'webots_world/protos/CelegansRobot.proto', scratch.read_text()
        for name, kind in [('DrosophilaRobot', 'drosophila'), ('DrosophilaBancRobot', 'drosophila_banc'), ('DrosophilaFafbRobot', 'drosophila_fafb')]:
            alignment = json.loads((root / f'webots_world/configs/config_{kind}_webots.io_alignment.json').read_text())
            labels = [v['connectome_node_id'] for v in alignment['output_channels']]
            fly.generate_proto(labels, scratch, f'../../network_{kind}.json', f'../configs/config_{kind}_webots.json',
                               proto_name=name, brain_id='default', default_robot_name=kind + '_robot',
                               include_compound_eyes=True, eye_camera_width=32, eye_camera_height=24)
            yield root / f'webots_world/protos/{name}.proto', scratch.read_text()
        yield root / 'webots_world/protos/ZebrafishRobot.proto', fish.build_proto()
        worm.generate_world(scratch)
        yield root / 'webots_world/worlds/celegans_neuroworld.wbt', scratch.read_text()
        yield root / 'webots_world/worlds/zebrafish_neuroworld.wbt', fish.build_world()
        yield root / 'webots_world/worlds/neuroworld.wbt', content.reference_world('nao', 'Nao', multi.NAO_EXTERNPROTO)
        yield root / 'webots_world/worlds/hexapod_neuroworld.wbt', content.reference_world('hexapod', 'HexapodRobot', '../protos/HexapodRobot.proto', [
            'NM_BRAINS=default', f'NM_SENSORS_default={multi.HEXAPOD_SENSORS_REGEX}', f'NM_ACTUATORS_default={multi.HEXAPOD_ACTUATORS_REGEX}'])
        # Rebase only generated references, never touch tracked .wbproj settings.
        fly.generate_world(scratch, root / 'webots_world/protos/DrosophilaBancRobot.proto',
                           root / 'webots_world/protos/DrosophilaFafbRobot.proto',
                           'DrosophilaBancRobot', 'DrosophilaFafbRobot', 'default', 'fafb', single_instance=True)
        text = scratch.read_text()
        for name in ['DrosophilaBancRobot', 'DrosophilaFafbRobot']:
            ref = os.path.relpath(root / f'webots_world/protos/{name}.proto', scratch.parent)
            text = text.replace(ref, f'../protos/{name}.proto')
        yield root / 'webots_world/worlds/drosophila_neuroworld.wbt', text
        for filename, brains in [('multi_neuroworld.wbt', 'celegans_01'), ('multi_neuroworld_test.wbt', 'celegans_01,celegans_02')]:
            subprocess.run([sys.executable, str(root / 'scripts/build_webots_multi_world.py'), '--world', str(scratch),
                            '--celegans-proto', str(root / 'webots_world/protos/CelegansRobot.proto'),
                            '--celegans-brains', brains], check=True, stdout=subprocess.DEVNULL)
            ref = os.path.relpath(root / 'webots_world/protos/CelegansRobot.proto', scratch.parent)
            yield root / 'webots_world/worlds' / filename, scratch.read_text().replace(ref, '../protos/CelegansRobot.proto')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    stale = []
    for path, expected in generated_assets():
        if args.check:
            if not path.exists() or path.read_text() != expected:
                stale.append(str(path.relative_to(content.ROOT)))
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(expected)
    if stale:
        raise SystemExit('Stale simulation assets: ' + ', '.join(stale))
    print('All simulator content and Webots robot/world assets ' + ('verified' if args.check else 'regenerated'))


if __name__ == '__main__':
    main()
