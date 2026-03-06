#!/usr/bin/env python3
"""
Import an ONNX MLP and emit a aarnn_rust Runner snapshot JSON
("network.json" shape) to the given path.

Usage:
  python3 tools/import_onnx.py --in model.onnx --out-network network.json

Behavior:
- First tries the app's naming convention: W_in, W_fwd_l{idx}, W_out.
- If not found, falls back to discovering Gemm/MatMul nodes in graph order
  and grabbing their weight initializers.
- As a last resort, scans all 2D float initializers and heuristically maps
  the first/last to input/output and the rest as forward layers.

Env:
- NMD_ONNX_DEBUG=1 prints discovered initializers and chosen mapping.
"""
import argparse
import json
import os
import onnx
import numpy as np
from onnx import numpy_helper


def create_matrix_dict(rows, cols, data):
    return {"rows": int(rows), "cols": int(cols), "data": [float(x) for x in data]}


def convert_to_snapshot(weights_map):
    # Minimal snapshot with default config; Runner will adopt sizes from matrices.
    w_in = weights_map['w_in']              # (H0 x S)
    w_fwd = weights_map['w_fwd']            # list of (H_{l+1} x H_l)
    w_out = weights_map['w_out']            # (O x H_last)
    network_snapshot = {
        "net": {
            "n_sensory":  int(w_in.shape[1]),
            "num_hidden_layers": int(len(w_fwd) + 1),
            "num_hidden_per_layer_initial": int(w_in.shape[0]),
            "n_output": int(w_out.shape[0]),
            # Other fields default; UI Runner ignores most on import
            "p_in": 0.15, "p_hidden": 0.10, "p_out": 0.15,
            "growth_enabled": False,
            "max_layers": 6,
            "saturation_threshold": 0.5,
            "saturation_window_ms": 200.0,
            "growth_cooldown_ms": 500.0,
            "spawn_radius": 0.1,
            "migrate_in_prob": 0.5,
            "migrate_out_prob": 0.5,
            "new_edge_prob": 0.05,
            "layer_split_threshold": 32,
            "global_growth_cooldown_ms": 150.0,
            "proximity_degree_cap": 6,
            "use_morphology": False,
            "aarnn_velocity": 10.0,
            "p_release_default": 1.0,
            "dend_atten_k": 0.0,
            "use_aarnn_delays": False,
            "use_aarnn_attenuation": True,
            "enforce_unique_geometry": True,
            "min_node_sep": 0.02,
            "min_segment_sep": 0.01,
            "synapse_offset": 0.0125,
            "max_place_tries": 16,
            "relax_iters": 2,
            "relax_step": 0.004,
            "seg_eps": 0.0015,
            "max_reroute_tries": 6,
            "use_mid_bends": True,
        },
        "w_in": create_matrix_dict(*w_in.shape, w_in.flatten().tolist()),
        "w_hh_fwd": [create_matrix_dict(*w.shape, w.flatten().tolist()) for w in w_fwd],
        "w_hh_bwd": [create_matrix_dict(w.shape[1], 0, []) for w in w_fwd],  # placeholders
        "w_out": create_matrix_dict(*w_out.shape, w_out.flatten().tolist()),
    }
    return network_snapshot


def _get_numpy_from_initializer_map(initializers, name):
    tensor = initializers.get(name)
    if tensor is None:
        return None
    return np.array(numpy_helper.to_array(tensor))


def _orient_to_output_input(weights: np.ndarray) -> np.ndarray:
    """Return weights as (output_features, input_features). If rank!=2, raises; if rows<cols keep, else transpose."""
    if weights.ndim != 2:
        raise ValueError("weight is not rank-2")
    rows, cols = weights.shape
    return weights if rows <= cols else weights.T.copy()


def _discover_layers_by_nodes(model) -> list:
    """Return list of (name, weight ndarray) for Gemm/MatMul nodes in graph order.
    Only includes nodes whose weight initializer is present and rank-2.
    Orientation is normalized to (output_features, input_features).
    """
    name_to_initializer = {init.name: init for init in model.graph.initializer}
    discovered_layers = []
    for node in model.graph.node:
        if node.op_type not in ("Gemm", "MatMul"):
            continue
        # For Gemm/MatMul, weight is typically input[1]
        if len(node.input) < 2:
            continue
        weight_name = node.input[1]
        initializer = name_to_initializer.get(weight_name)
        if initializer is None:
            continue
        array_data = numpy_helper.to_array(initializer)
        if not isinstance(array_data, np.ndarray) or array_data.ndim != 2:
            continue
        weights = _orient_to_output_input(np.array(array_data))
        discovered_layers.append((weight_name, weights))
    return discovered_layers


def _discover_layers_by_initializers(model) -> list:
    """Fallback: collect all rank-2 float initializers as (name, oriented_weight)."""
    discovered_layers = []
    for init in model.graph.initializer:
        array_data = numpy_helper.to_array(init)
        if not isinstance(array_data, np.ndarray) or array_data.ndim != 2:
            continue
        if array_data.dtype.kind not in ('f', 'i', 'u'):
            continue
        # Cast to float32 for safety
        weights = _orient_to_output_input(np.array(array_data, dtype=np.float32))
        discovered_layers.append((init.name, weights))
    # Heuristic order: sort by (rows, cols); assume first is input, last is output
    discovered_layers.sort(key=lambda x: (x[1].shape[0], x[1].shape[1]))
    return discovered_layers


def main():
    argument_parser = argparse.ArgumentParser()
    argument_parser.add_argument('--in', dest='input_onnx_path', required=True)
    argument_parser.add_argument('--out-network', dest='output_network_path', required=True)
    args = argument_parser.parse_args()

    debug_mode = os.environ.get('NMD_ONNX_DEBUG', '') == '1'

    onn_model = onnx.load(args.input_onnx_path)
    initializers = {init.name: init for init in onn_model.graph.initializer}

    # 1) Preferred: exact names from our exporter
    w_in = _get_numpy_from_initializer_map(initializers, 'W_in')
    w_fwd = []
    layer_idx = 0
    while True:
        name = f'W_fwd_l{layer_idx}'
        wf = _get_numpy_from_initializer_map(initializers, name)
        if wf is None:
            break
        w_fwd.append(wf)
        layer_idx += 1
    w_out = _get_numpy_from_initializer_map(initializers, 'W_out')

    if w_in is not None and w_out is not None:
        # transpose back from (input,output) to (output,input)
        w_in = _orient_to_output_input(w_in)
        w_fwd = [_orient_to_output_input(w) for w in w_fwd]
        w_out = _orient_to_output_input(w_out)
        network_snapshot = convert_to_snapshot({'w_in': w_in, 'w_fwd': w_fwd, 'w_out': w_out})
        with open(args.output_network_path, 'w') as f:
            json.dump(network_snapshot, f, indent=2)
        print(f"Wrote network snapshot: {args.output_network_path}")
        return

    # 2) Fallback: walk Gemm/MatMul nodes in graph order
    node_layers = _discover_layers_by_nodes(onn_model)
    if debug_mode:
        print("[ONNX][DiscoverByNodes]", [(name, weights.shape) for (name, weights) in node_layers])
    if node_layers:
        all_weights = [weights for (_name, weights) in node_layers]
        if len(all_weights) == 1:
            # Single layer (input->output)
            w_in = all_weights[0]
            w_fwd = []
            w_out = all_weights[0]
        else:
            w_in = all_weights[0]
            w_fwd = all_weights[1:-1]
            w_out = all_weights[-1]
        network_snapshot = convert_to_snapshot({'w_in': w_in, 'w_fwd': w_fwd, 'w_out': w_out})
        with open(args.output_network_path, 'w') as f:
            json.dump(network_snapshot, f, indent=2)
        print(f"Wrote network snapshot: {args.output_network_path}")
        return

    # 3) Last resort: scan all 2D initializers and heuristically map
    init_layers = _discover_layers_by_initializers(onn_model)
    if debug_mode:
        print("[ONNX][DiscoverByInitializers]", [(name, weights.shape) for (name, weights) in init_layers])
    if len(init_layers) >= 2:
        mats = [weights for (_name, weights) in init_layers]
        w_in = mats[0]
        w_out = mats[-1]
        w_fwd = mats[1:-1]
        network_snapshot = convert_to_snapshot({'w_in': w_in, 'w_fwd': w_fwd, 'w_out': w_out})
        with open(args.output_network_path, 'w') as f:
            json.dump(network_snapshot, f, indent=2)
        print(f"Wrote network snapshot: {args.output_network_path}")
        return

    raise RuntimeError("Could not locate Dense/Gemm weights in ONNX file. Tips: use the app's Export ONNX…, or set NMD_ONNX_DEBUG=1 and retry to see discovered initializers/nodes.")


if __name__ == '__main__':
    main()
