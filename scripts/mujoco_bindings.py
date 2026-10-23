#!/usr/bin/env python3
"""Pin named local simulation bindings without modifying the neural snapshot.

Called once per managed body by the common cluster launcher. A generation here
is the private compatibility session generation, not a production writer term.
"""
import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def prepare(profile, brain, network, host, port, output, config=None):
    if host not in ('127.0.0.1', '::1'):
        raise ValueError('Managed simulation reference transport requires a loopback literal')
    record = json.loads((ROOT / 'sim/content/embodiments/generated' / (profile + '.json')).read_text())
    io = record['io']
    snapshot = json.loads(Path(network).read_text())
    labels = snapshot['connectome_labels']
    sensory = [s['name'] for s in io['sensors']]
    outputs = io['output_names']
    # Existing neural labels use MDL01 etc; the physical prefix identifies the
    # channel in the alignment. Compare ordered meaning, not just dimensions.
    neural_sensory = io.get('neural_sensory_names', sensory)
    neural_outputs = io.get('neural_output_names', [n.split('_', 3)[-1] for n in outputs] if profile=='celegans' else outputs)
    if (labels['sensory_nodes'] != neural_sensory or labels['output_nodes'] != neural_outputs
            or snapshot['net']['num_sensory_neurons'] != len(sensory) or snapshot['net']['num_output_neurons'] != len(outputs)):
        raise ValueError(f'{profile}: snapshot named sensory/output projection mismatch; snapshot will not be resized')
    if record['controller_transfer'].startswith('blocked'):
        raise ValueError('Model transfer remains blocked')
    if config:
        cfg=json.loads(Path(config).read_text());cfg=cfg.get('net',cfg)
        if cfg.get('num_sensory_neurons')!=len(sensory) or cfg.get('num_output_neurons')!=len(outputs):
            raise ValueError(f'{profile}: configuration disagrees with pinned snapshot/IO; no runtime resize permitted')
    binding = dict(brain_id=brain, brain_generation=1, model_digest=record['digest'], io_digest=io['digest'], host=host, port=port)
    contract = {k: binding[k] for k in ('brain_id', 'brain_generation', 'model_digest', 'io_digest')}
    contract.update(s_names=sensory, o_names=outputs, sensory_base=io['sensory_base'], output_base=io['output_base'])
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    (output / (brain + '.contract.json')).write_text(json.dumps(contract, indent=2) + '\n')
    mapping = output / 'mujoco-bindings.json'
    bindings = json.loads(mapping.read_text()) if mapping.exists() else {}
    bindings[brain] = binding
    mapping.write_text(json.dumps(bindings, indent=2) + '\n')
    (output / (brain + '.source.json')).write_text(json.dumps(dict(network=str(Path(network).resolve()), sha256=hashlib.sha256(Path(network).read_bytes()).hexdigest(), scope='preserved snapshot; no mutation'), indent=2) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('profile', 'brain', 'network', 'host', 'output'):
        parser.add_argument('--' + name, required=True)
    parser.add_argument('--port', type=int, required=True)
    parser.add_argument('--network-map', default='')
    parser.add_argument('--config')
    parser.add_argument('--config-map',default='')
    args = parser.parse_args()
    for entry in args.network_map.split(','):
        if entry and entry.split('=', 1)[0] == args.brain:
            args.network = entry.split('=', 1)[1]
    for entry in args.config_map.split(','):
        if entry and entry.split('=',1)[0]==args.brain:args.config=entry.split('=',1)[1]
    prepare(args.profile, args.brain, args.network, args.host, args.port, args.output,args.config)
