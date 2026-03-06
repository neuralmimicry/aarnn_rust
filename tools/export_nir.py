#!/usr/bin/env python3
"""
Export a Neuromorphic Intermediate Representation (NIR) JSON from a
aarnn_rust Runner snapshot (network.json produced by export_network_json).

Usage:
  python3 tools/export_nir.py --in-network network.json --out-nir model.nir.json

NIR schema v0.1 (subset):
{
  "meta": { "nir_version": "0.1", "producer": "aarnn_rust", "created_at": ISO8601 },
  "config": { ... subset of NetworkConfig ... },
  "weights": { "w_in": M, "w_hh_fwd": [M...], "w_hh_bwd": [M...], "w_out": M },
  "topology": { "layers": [ [ {x,y,z,layer}, ... ], ... ] },   (optional)
  "aarnn": { "use_delays": bool, "use_atten": bool, "velocity": f32, "atten_k": f32 }, (optional)
  "morphology": { ... }  (optional, not emitted by this exporter)
}
Matrix M: { rows, cols, data } with row-major flat data.
"""
import argparse
import json
from datetime import datetime, timezone


def load_snapshot(path):
    with open(path, 'r') as f:
        return json.load(f)


def main():
    argument_parser = argparse.ArgumentParser()
    argument_parser.add_argument('--in-network', dest='input_network_path', required=True)
    argument_parser.add_argument('--out-nir', dest='output_nir_path', required=True)
    args = argument_parser.parse_args()

    snapshot = load_snapshot(args.input_network_path)
    network_config = snapshot.get('net', {})
    nir_output = {
        'meta': {
            'nir_version': '0.1',
            'producer': 'aarnn_rust',
            'created_at': datetime.now(timezone.utc).isoformat(),
        },
        'config': {
            'n_sensory': network_config.get('n_sensory'),
            'num_hidden_layers': network_config.get('num_hidden_layers'),
            'num_hidden_per_layer_initial': network_config.get('num_hidden_per_layer_initial'),
            'n_output': network_config.get('n_output'),
            'lif_dt': 1.0,  # the demo uses dt in LIFParams; not included in snapshot; keep hint
            'learning': 'unknown',
        },
        'weights': {
            'w_in': snapshot.get('w_in'),
            'w_hh_fwd': snapshot.get('w_hh_fwd', []),
            'w_hh_bwd': snapshot.get('w_hh_bwd', []),
            'w_out': snapshot.get('w_out'),
        }
    }
    # Optional topology
    topology = snapshot.get('topo') or snapshot.get('topology')
    if topology:
        nir_output['topology'] = topology
    # Optional AARNN flags
    aarnn_config = {
        'use_delays': bool(network_config.get('use_aarnn_delays', False)),
        'use_atten': bool(network_config.get('use_aarnn_attenuation', False)),
        'velocity': float(network_config.get('aarnn_velocity', 10.0)),
        'atten_k': float(network_config.get('dend_atten_k', 0.0)),
    }
    nir_output['aarnn'] = aarnn_config

    with open(args.output_nir_path, 'w') as f:
        json.dump(nir_output, f, indent=2)
    print(f"Wrote NIR: {args.output_nir_path}")


if __name__ == '__main__':
    main()
