#!/usr/bin/env python3
"""
NIR Importer for AARNN

This script converts a Neuromorphic Intermediate Representation (NIR) JSON file
into a network snapshot JSON compatible with the AARNN Runner.

NIR is a standardized format for exchanging spiking neural network models.
This importer allows models trained in other frameworks (and exported to NIR)
to be loaded into this simulator for visualization or real-time execution.

Workflow:
1. Load NIR JSON.
2. Extract network configuration (layer sizes, AARNN parameters).
3. Extract synaptic weight matrices (Input, Forward, Backward, Output).
4. Optionally extract spatial topology (neuron positions).
5. Map these into the 'network.json' format used by `Runner::import_network_json`.

Usage:
  python3 tools/import_nir.py --in-nir model.nir.json --out-network network.json [--strict]
"""
import argparse
import json


def create_matrix_dict(rows, cols, data):
    """Helper to package flat data into a row/col dictionary structure."""
    return {"rows": int(rows), "cols": int(cols), "data": [float(x) for x in data]}


def main():
    argument_parser = argparse.ArgumentParser()
    argument_parser.add_argument('--in-nir', dest='input_nir_path', required=True)
    argument_parser.add_argument('--out-network', dest='output_network_path', required=True)
    argument_parser.add_argument('--strict', action='store_true')
    args = argument_parser.parse_args()

    nir_data = json.load(open(args.input_nir_path, 'r'))
    config = nir_data.get('config', {})
    weights_data = nir_data.get('weights', {})

    # Extract matrices; tolerate either nested matrix objects or already in snapshot shape
    def get_matrix(obj, key):
        matrix = obj.get(key)
        if matrix is None:
            return None
        # Expect rows/cols/data
        if all(k in matrix for k in ('rows','cols','data')):
            return matrix
        # Else treat as numpy-like (unlikely in JSON). Not supported.
        raise RuntimeError(f"NIR weights.{key} missing rows/cols/data")

    w_in = get_matrix(weights_data, 'w_in')
    w_fwd = weights_data.get('w_hh_fwd', [])
    w_bwd = weights_data.get('w_hh_bwd', [])
    w_out = get_matrix(weights_data, 'w_out')

    # Minimal snapshot expected by Runner::import_network_json
    network_snapshot = {
        'net': {
            'n_sensory': int(config.get('n_sensory', w_in['cols'] if w_in else 0)),
            'num_hidden_layers': int(config.get('num_hidden_layers', len(w_fwd) + 1 if w_in else 0)),
            'num_hidden_per_layer_initial': int(config.get('num_hidden_per_layer_initial', w_in['rows'] if w_in else 0)),
            'n_output': int(config.get('n_output', w_out['rows'] if w_out else 0)),
            # Pass through AARNN flags if present
            'aarnn_velocity': float(nir_data.get('aarnn',{}).get('velocity', 10.0)),
            'dend_atten_k': float(nir_data.get('aarnn',{}).get('atten_k', 0.0)),
            'use_aarnn_delays': bool(nir_data.get('aarnn',{}).get('use_delays', False)),
            'use_aarnn_attenuation': bool(nir_data.get('aarnn',{}).get('use_atten', True)),
            # Keep defaults for the rest; UI build ignores many of these on import
            'p_in': 0.15, 'p_hidden': 0.10, 'p_out': 0.15,
            'growth_enabled': False,
            'max_layers': 6,
            'saturation_threshold': 0.5,
            'saturation_window_ms': 200.0,
            'growth_cooldown_ms': 500.0,
            'spawn_radius': 0.1,
            'migrate_in_prob': 0.5,
            'migrate_out_prob': 0.5,
            'new_edge_prob': 0.05,
            'layer_split_threshold': 32,
            'global_growth_cooldown_ms': 150.0,
            'proximity_degree_cap': 6,
            'use_morphology': False,
            'enforce_unique_geometry': True,
            'min_node_sep': 0.02,
            'min_segment_sep': 0.01,
            'synapse_offset': 0.0125,
            'max_place_tries': 16,
            'relax_iters': 2,
            'relax_step': 0.004,
            'seg_eps': 0.0015,
            'max_reroute_tries': 6,
            'use_mid_bends': True,
        },
        'w_in': w_in,
        'w_hh_fwd': w_fwd,
        'w_hh_bwd': w_bwd,
        'w_out': w_out,
    }

    # Optional topology pass-through
    topology = nir_data.get('topology') or nir_data.get('topo')
    if topology is not None:
        network_snapshot['topo'] = topology

    if args.strict:
        # Basic shape checks
        if w_in is None or w_out is None:
            raise RuntimeError('NIR missing required weights w_in or w_out')
        num_hidden_layers = len(w_fwd) + 1
        if network_snapshot['net']['num_hidden_layers'] != num_hidden_layers:
            raise RuntimeError('Hidden layer count mismatch in NIR config vs weights')

    with open(args.output_network_path, 'w') as f:
        json.dump(network_snapshot, f, indent=2)
    print(f"Wrote network snapshot: {args.output_network_path}")


if __name__ == '__main__':
    main()
