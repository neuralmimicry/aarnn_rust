#!/usr/bin/env python3
"""
ONNX Exporter for Neuromorphic Demo

This script converts a Neuromorphic Demo network snapshot (JSON) into a
standardized ONNX model.

The resulting ONNX model represents the spiking neural network as a
Multi-Layer Perceptron (MLP) using Gemm (General Matrix Multiplication) nodes.
Since SNNs and MLPs have different dynamics, this export focuses on preserving
the synaptic weight structure. Activations are set to linear (no-op) to
exactly mirror the weights.

This is useful for:
1. Visualizing the weight structure using standard neural network tools (e.g., Netron).
2. Using the learned weights in traditional deep learning frameworks.
3. Performing weight analysis and pruning using external tools.

Workflow:
1. Load the network snapshot JSON.
2. Convert matrix data into NumPy arrays.
3. Build an ONNX computational graph using `onnx.helper`.
4. Create Gemm nodes for each layer interface (Input->H0, H->H, H_last->Output).
5. Validate and save the ONNX model.

Usage:
  python3 tools/export_onnx.py --in network.json --out model.onnx
"""
import argparse
import json
import numpy as np
import onnx
from onnx import helper, TensorProto


def load_network_snapshot(path):
    """Loads a network snapshot JSON file."""
    with open(path, 'r') as f:
        return json.load(f)


def get_numpy_array_from_matrix(matrix_dict):
    """Converts a row/col/data dictionary into a 2D NumPy array."""
    array_data = np.zeros((matrix_dict['rows'], matrix_dict['cols']), dtype=np.float32)
    flattened_data = np.array(matrix_dict['data'], dtype=np.float32)
    if flattened_data.size > 0:
        array_data.flat[:min(array_data.size, flattened_data.size)] = flattened_data.flat[:min(array_data.size, flattened_data.size)]
    return array_data


def main():
    argument_parser = argparse.ArgumentParser()
    argument_parser.add_argument('--in', dest='input_network_path', required=True)
    argument_parser.add_argument('--out', dest='output_onnx_path', required=True)
    args = argument_parser.parse_args()

    snapshot = load_network_snapshot(args.input_network_path)
    w_in = get_numpy_array_from_matrix(snapshot['w_in'])  # H0 x S
    w_f_list = [get_numpy_array_from_matrix(m) for m in snapshot['w_hh_fwd']]  # list of (H_{l+1} x H_l)
    w_out = get_numpy_array_from_matrix(snapshot['w_out'])  # O x H_last

    num_sensory_neurons = w_in.shape[1]
    num_hidden_0_neurons = w_in.shape[0]
    num_output_neurons = w_out.shape[0]

    # Build graph: input X (num_sensory_neurons,), layers L0..L{L}, output Y (num_output_neurons,)
    nodes = []
    inputs = [helper.make_tensor_value_info('X', TensorProto.FLOAT, [num_sensory_neurons])]
    outputs = [helper.make_tensor_value_info('Y', TensorProto.FLOAT, [num_output_neurons])]
    initializers = []

    previous_output_name = 'X'
    # Sensory to first hidden layer
    weight_name_in = 'W_in'
    bias_name_in = 'B_in'
    initializers.append(helper.make_tensor(weight_name_in, TensorProto.FLOAT, list(w_in.shape[::-1]), w_in.T.flatten().tolist()))
    bias_in = np.zeros((num_hidden_0_neurons,), dtype=np.float32)
    initializers.append(helper.make_tensor(bias_name_in, TensorProto.FLOAT, [num_hidden_0_neurons], bias_in.tolist()))
    layer_0_output_name = 'L0'
    nodes.append(helper.make_node('Gemm', [previous_output_name, weight_name_in, bias_name_in], [layer_0_output_name], alpha=1.0, beta=1.0, transB=0))
    previous_output_name = layer_0_output_name

    # Hidden forward layers
    for layer_idx, weight_matrix in enumerate(w_f_list):
        num_current_hidden_neurons = weight_matrix.shape[0]
        weight_name = f'W_fwd_l{layer_idx}'
        bias_name = f'B_fwd_l{layer_idx}'
        initializers.append(helper.make_tensor(weight_name, TensorProto.FLOAT, list(weight_matrix.shape[::-1]), weight_matrix.T.flatten().tolist()))
        bias_values = np.zeros((num_current_hidden_neurons,), dtype=np.float32)
        initializers.append(helper.make_tensor(bias_name, TensorProto.FLOAT, [num_current_hidden_neurons], bias_values.tolist()))
        current_layer_output_name = f'L{layer_idx+1}'
        nodes.append(helper.make_node('Gemm', [previous_output_name, weight_name, bias_name], [current_layer_output_name], alpha=1.0, beta=1.0, transB=0))
        previous_output_name = current_layer_output_name

    # Last hidden to output layer
    weight_name_out = 'W_out'
    bias_name_out = 'B_out'
    initializers.append(helper.make_tensor(weight_name_out, TensorProto.FLOAT, list(w_out.shape[::-1]), w_out.T.flatten().tolist()))
    bias_out = np.zeros((num_output_neurons,), dtype=np.float32)
    initializers.append(helper.make_tensor(bias_name_out, TensorProto.FLOAT, [num_output_neurons], bias_out.tolist()))
    nodes.append(helper.make_node('Gemm', [previous_output_name, weight_name_out, bias_name_out], ['Y'], alpha=1.0, beta=1.0, transB=0))

    graph = helper.make_graph(nodes, 'neuromorphic_mlp', inputs, outputs, initializer=initializers)
    onn_model = helper.make_model(graph, producer_name='neuromorphic_demo')
    onnx.checker.check_model(onn_model)
    onnx.save(onn_model, args.output_onnx_path)
    print(f"Wrote ONNX: {args.output_onnx_path}")


if __name__ == '__main__':
    main()
