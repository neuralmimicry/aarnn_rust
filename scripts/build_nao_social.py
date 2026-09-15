#!/usr/bin/env python3
"""Append a versioned, engineered social circuit to the canonical NAO snapshot.

The source snapshot and all its existing weights/labels are preserved. This is
an explicit new reference model, not an in-place checkpoint migration or LLM.
"""
from __future__ import annotations
import argparse
import copy
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CONTRACT = ROOT / 'sim/nao/interaction.json'


def extend_matrix(matrix, rows, cols):
    old_rows, old_cols = matrix['rows'], matrix['cols']
    if rows < old_rows or cols < old_cols or len(matrix['data']) != old_rows * old_cols:
        raise ValueError('Invalid source matrix; resizing down is forbidden')
    result = dict(rows=rows, cols=cols, data=[0] * (rows * cols))
    for r in range(old_rows):
        result['data'][r*cols:r*cols+old_cols] = matrix['data'][r*old_cols:(r+1)*old_cols]
    return result


def build(source: dict, source_digest: str) -> dict:
    c = json.loads(CONTRACT.read_text())
    net = source['net']
    if (net['num_sensory_neurons'], net['num_output_neurons']) != (250, 40):
        raise ValueError('Source must be the canonical 250/40 NAO model')
    if source.get('runtime_state') or source.get('t', 0) or source.get('t_ms', 0):
        raise ValueError('Use an initial model, not a live checkpoint')
    result = copy.deepcopy(source)
    net = result['net']
    layers = len(source['topo']['layers'])
    sizes = [len(layer) for layer in source['topo']['layers']]
    if len(set(sizes)) != 1 or not layers:
        raise ValueError('Source must have uniform hidden layer sizes')
    h = sizes[0]
    # nn_tcp_server uses the repository's LIF reference Runner. Its supported
    # feed-forward sensory entry is H0; the source's cortical H1 mapping belongs
    # to the AARNN morphology profile. Record this explicit new-model choice.
    original_in_layer = net['sensory_target_layer']
    in_layer = 0
    out_layer = net['output_source_layer']
    if not 0 <= in_layer <= out_layer < layers:
        raise ValueError('Social relay needs forward-connected input/output layers')
    count = len(c['acts'])
    for key in ('w_in', 'p_in'):
        result[key] = extend_matrix(source[key], h+count, c['sensory'])
    for key in ('w_out', 'p_out'):
        result[key] = extend_matrix(source[key], c['output'], h+count)
    for key in ('w_hh_fwd', 'p_fwd', 'w_hh_bwd', 'p_bwd', 'w_hh_rec', 'p_rec'):
        result[key] = [extend_matrix(m, h+count, h+count) for m in source[key]]
    def edge(weight, presence, row, col):
        weight['data'][row*weight['cols']+col] = 1.5
        presence['data'][row*presence['cols']+col] = 1
    for i in range(count):
        edge(result['w_in'], result['p_in'], h+i, 250+i if i<8 else 279)
        for layer in range(in_layer, out_layer):
            edge(result['w_hh_fwd'][layer], result['p_fwd'][layer], h+i, h+i)
        edge(result['w_out'], result['p_out'], 40+i, h+i)
    # The social profile freezes structural growth/import rewiring so a saved
    # channel binding cannot change underneath a live player session. Dynamics
    # and STDP remain the existing Rust reference Runner's implementation.
    overrides = dict(num_sensory_neurons=c['sensory'], num_output_neurons=c['output'],
                     num_hidden_per_layer_initial=h+count, growth_enabled=False,
                     morpho_growth_enabled=False, aarnn_import_topology_rewire_enabled=False,
                     use_aarnn_delays=False, sensory_target_layer=in_layer)
    net.update(overrides)
    # Retain the recognised NAO spike strategy; social schema is separately named.
    net['max_total_neurons'] = max(net['max_total_neurons'], c['sensory']+c['output']+(h+count)*layers)
    def node(index, layer, role):
        return dict(x=-.32+index*.08, y=.22+layer*.02, z=.32, layer=layer,
                    region_name='social_reference', type_name=role)
    result['topo']['sensory_nodes'] += [node(i%8, 0, 'Sensory') for i in range(32)]
    result['topo']['output_nodes'] += [node(i, 0, 'Motor') for i in range(count)]
    for layer in range(layers):
        result['topo']['layers'][layer] += [node(i, layer, 'Pyramidal') for i in range(count)]
    labels = result['connectome_labels']
    labels['sensory_nodes'] += c['sensory_extension']
    labels['output_nodes'] += ['social.say.'+a['id'] for a in c['acts']]
    # Canonical hidden labels are a flat layer-major array.
    original_hidden = source['connectome_labels']['hidden_nodes']
    if len(original_hidden) != h*layers:
        raise ValueError('Hidden labels do not match source matrices')
    labels['hidden_nodes'] = [name for layer in range(layers) for name in
        (original_hidden[layer*h:(layer+1)*h] + [f'social.relay.{layer}.{a["id"]}' for a in c['acts']])]
    labels['hidden_layer_sizes'] = [h+count]*layers
    labels['sensor_role_map'].update({name:'social_reference' for name in c['sensory_extension']})
    labels['output_role_map'].update({'social.say.'+a['id']:'social_reference' for a in c['acts']})
    labels['sensory_groups']['social_reference'] = list(range(250, c['sensory']))
    labels['output_groups']['social_reference'] = list(range(40, c['output']))
    labels['expected_counts'] = dict(sensory=c['sensory'], output=c['output'])
    labels['social_interface'] = dict(schema=c['schema'], profile=c['profile'],
        source_sha256=source_digest, contract_sha256=hashlib.sha256(CONTRACT.read_bytes()).hexdigest(),
        generator='scripts/build_nao_social.py', model_note=c['model_note'],
        config_overrides=overrides, original_sensory_target_layer=original_in_layer, relay_weight=1.5,
        biological_validation='Engineered lexical reflexes; schematic anatomy, no language training or biological adequacy claim',
        byte_channels='One UTF-8 byte per reference frame, LSB first; lexical features are versioned preprocessing',
        parameter_provenance='1.5 dimensionless relay weight against the existing LIF threshold 1; validate with the Rust Runner')
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--source', type=Path, default=ROOT/'network_nao.json')
    p.add_argument('--output', type=Path, default=ROOT/'target/nao-social/network_nao_social.json')
    args = p.parse_args()
    if args.source.resolve() == args.output.resolve():
        p.error('Never overwrite the source snapshot')
    raw = args.source.read_bytes()
    model = build(json.loads(raw), hashlib.sha256(raw).hexdigest())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(model, separators=(',', ':'))+'\n')
    args.output.with_suffix('.config.json').write_text(json.dumps(model['net'], indent=2)+'\n')
    print(f'Generated {args.output}: 282 inputs / 49 outputs; original 250/40 weights preserved')


if __name__ == '__main__':
    main()
