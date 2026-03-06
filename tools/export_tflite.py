#!/usr/bin/env python3
"""
Convert aarnn_rust JSON snapshot (Runner::export_network_json)
to a TFLite MLP with Dense(linear) layers.

Usage:
  python3 tools/export_tflite.py --in network.json --out model.tflite

Requires: tensorflow (or tf-nightly) installed in Python environment.
"""
import argparse
import json
import numpy as np


def get_numpy_array_from_matrix(matrix_dict):
    array_data = np.zeros((matrix_dict['rows'], matrix_dict['cols']), dtype=np.float32)
    flattened_data = np.array(matrix_dict['data'], dtype=np.float32)
    if flattened_data.size:
        array_data.flat[:min(array_data.size, flattened_data.size)] = flattened_data.flat[:min(array_data.size, flattened_data.size)]
    return array_data


def build_keras_sequential_model(weights_in, weights_hidden_fwd, weights_out):
    import tensorflow as tf
    from tensorflow.keras import layers, models

    num_sensory_neurons = weights_in.shape[1]
    inputs = layers.Input(shape=(num_sensory_neurons,), name='input')
    x = inputs
    # Sensory to first hidden layer
    num_hidden_0_neurons = weights_in.shape[0]
    dense_in_layer = layers.Dense(num_hidden_0_neurons, activation=None, use_bias=False, name='dense_in')
    x = dense_in_layer(x)
    # Hidden forward layers
    hidden_layers = []
    for layer_idx, weight_matrix in enumerate(weights_hidden_fwd):
        dense_hidden_layer = layers.Dense(weight_matrix.shape[0], activation=None, use_bias=False, name=f'dense_fwd_{layer_idx}')
        x = dense_hidden_layer(x)
        hidden_layers.append(dense_hidden_layer)
    # Last hidden to output layer
    num_output_neurons = weights_out.shape[0]
    dense_out_layer = layers.Dense(num_output_neurons, activation=None, use_bias=False, name='dense_out')
    outputs = dense_out_layer(x)
    keras_model = models.Model(inputs, outputs)

    # Set weights (Keras Dense expects (input_dim, output_dim))
    dense_in_layer.set_weights([weights_in.T])
    for layer_idx, weight_matrix in enumerate(weights_hidden_fwd):
        keras_model.get_layer(f'dense_fwd_{layer_idx}').set_weights([weight_matrix.T])
    dense_out_layer.set_weights([weights_out.T])
    return keras_model


def main():
    argument_parser = argparse.ArgumentParser()
    argument_parser.add_argument('--in', dest='input_network_path', required=True)
    argument_parser.add_argument('--out', dest='output_tflite_path', required=True)
    args = argument_parser.parse_args()

    snapshot = json.load(open(args.input_network_path, 'r'))
    weights_in = get_numpy_array_from_matrix(snapshot['w_in'])
    weights_hidden_fwd = [get_numpy_array_from_matrix(m) for m in snapshot['w_hh_fwd']]
    weights_out = get_numpy_array_from_matrix(snapshot['w_out'])

    keras_model = build_keras_sequential_model(weights_in, weights_hidden_fwd, weights_out)
    # Convert to TFLite
    import tensorflow as tf
    tflite_converter = tf.lite.TFLiteConverter.from_keras_model(keras_model)
    tflite_model_data = tflite_converter.convert()
    with open(args.output_tflite_path, 'wb') as f:
        f.write(tflite_model_data)
    print(f"Wrote TFLite: {args.output_tflite_path}")


if __name__ == '__main__':
    main()
