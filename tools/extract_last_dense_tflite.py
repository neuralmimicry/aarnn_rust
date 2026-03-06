#!/usr/bin/env python3
"""
Extract the last Dense-like head from a TFLite model and emit a minimal
aarnn_rust Runner snapshot JSON (network.json) that preserves only
the final linear mapping as W_out. The hidden/input are synthesized to allow
import into the app (identity W_in; no forward layers).

Usage:
  python3 tools/extract_last_dense_tflite.py --in model.tflite --out-network network.json

Heuristics:
  - Uses interpreter (tensorflow or tflite-runtime) to list all rank-2 tensors.
  - Picks the candidate with the smallest number of rows as the last head (O x H)
    (ties broken by smallest parameter count).
  - Transposes from (in, out) → (out, in) if needed by checking which orientation
    yields rows <= cols (typical for heads). If ambiguous, ensures result is (out, in).
  - Produces a snapshot with n_hidden_layers = 1, H = H_last, S = H_last, identity W_in (H x H),
    empty w_hh_fwd/bwd, and extracted W_out (O x H).
"""
import argparse
import json
import numpy as np


def create_matrix_dict(rows, cols, data):
    return {"rows": int(rows), "cols": int(cols), "data": [float(x) for x in data]}


def convert_to_snapshot_from_weights_out(weights_out):
    num_output_neurons, num_hidden_neurons = weights_out.shape
    # Identity W_in (num_hidden_neurons x num_hidden_neurons) so S=H
    weights_in = np.eye(num_hidden_neurons, dtype=np.float32)
    network_snapshot = {
        "net": {
            "n_sensory": int(num_hidden_neurons),
            "num_hidden_layers": 1,
            "num_hidden_per_layer_initial": int(num_hidden_neurons),
            "n_output": int(num_output_neurons),
            # defaults
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
        "w_in": create_matrix_dict(num_hidden_neurons, num_hidden_neurons, weights_in.flatten().tolist()),
        "w_hh_fwd": [],
        "w_hh_bwd": [],
        "w_out": create_matrix_dict(num_output_neurons, num_hidden_neurons, weights_out.flatten().tolist()),
    }
    return network_snapshot


def load_tflite_interpreter(model_path):
    try:
        import tensorflow as tf  # noqa: F401
        try:
            interpreter = tf.lite.Interpreter(model_path=model_path)
            return interpreter, "tensorflow"
        except Exception:
            pass
    except Exception:
        pass
    try:
        from tflite_runtime.interpreter import Interpreter  # type: ignore
        interpreter = Interpreter(model_path=model_path)
        return interpreter, "tflite-runtime"
    except Exception:
        raise RuntimeError(
            "Neither TensorFlow nor tflite-runtime is available. Install one:\n"
            "  pip install tensorflow   or   pip install tflite-runtime"
        )


def find_last_dense_layer_weights(model_path):
    interpreter, _ = load_tflite_interpreter(model_path)
    interpreter.allocate_tensors()
    tensor_details = interpreter.get_tensor_details()
    get_tensor_func = interpreter.get_tensor

    candidate_layers = []  # list of (rows, cols, ndarray)
    for detail in tensor_details:
        try:
            weights = get_tensor_func(detail['index'])
        except Exception:
            continue
        if not isinstance(weights, np.ndarray):
            continue
        if weights.ndim != 2:
            continue
        rows, cols = weights.shape
        candidate_layers.append((rows, cols, weights))

    if not candidate_layers:
        raise RuntimeError("No rank-2 tensors found in TFLite model; cannot extract last Dense.")

    # Pick the head as the one with smallest rows; tie-break by smallest params
    candidate_layers.sort(key=lambda t: (t[0], t[0] * t[1]))
    rows, cols, weights = candidate_layers[0]

    # Heuristic orientation: prefer (output_features, input_features) = (rows, cols) with rows <= cols
    # If rows > cols, transpose.
    if rows > cols:
        weights = weights.T.copy()
        rows, cols = weights.shape

    # Ensure result is float32
    if weights.dtype != np.float32:
        weights = weights.astype(np.float32)
    return weights  # shape (num_output_neurons, num_hidden_neurons)


def main():
    argument_parser = argparse.ArgumentParser()
    argument_parser.add_argument('--in', dest='input_tflite_path', required=True)
    argument_parser.add_argument('--out-network', dest='output_network_path', required=True)
    args = argument_parser.parse_args()

    weights_out = find_last_dense_layer_weights(args.input_tflite_path)
    network_snapshot = convert_to_snapshot_from_weights_out(weights_out)
    with open(args.output_network_path, 'w') as f:
        json.dump(network_snapshot, f, indent=2)
    print(f"Wrote network snapshot (last Dense head): {args.output_network_path}")


if __name__ == '__main__':
    main()
