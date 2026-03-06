#!/usr/bin/env python3
"""
Inspect an ONNX model and print information useful for mapping to a linear MLP:
- Unique op types in graph order
- Count and names/shapes/dtypes of initializers
- Rank‑2 initializers (candidates for Dense weights)
- Gemm/MatMul nodes and their weight inputs (if present)

Usage:
  python3 tools/inspect_onnx.py --in model.onnx
"""
import argparse
import onnx
import numpy as np
from onnx import numpy_helper


def main():
    argument_parser = argparse.ArgumentParser()
    argument_parser.add_argument('--in', dest='input_onnx_path', required=True)
    args = argument_parser.parse_args()

    onn_model = onnx.load(args.input_onnx_path)

    # Op types in order and unique set
    op_types = [node.op_type for node in onn_model.graph.node]
    unique_op_types = []
    seen_op_types = set()
    for op_type in op_types:
        if op_type not in seen_op_types:
            unique_op_types.append(op_type)
            seen_op_types.add(op_type)
    print(f"[ONNX][Ops] total_nodes={len(op_types)} unique_types={unique_op_types}")

    # Initializers summary
    initializers = list(onn_model.graph.initializer)
    print(f"[ONNX][Inits] total={len(initializers)}")
    rank2_initializers = []
    for initializer in initializers[:50]:  # cap print
        array_data = numpy_helper.to_array(initializer)
        shape = getattr(array_data, 'shape', ())
        dtype = getattr(array_data, 'dtype', type(array_data))
        print(f"  init: name='{initializer.name}' shape={list(shape)} dtype={dtype}")
        if isinstance(array_data, np.ndarray) and array_data.ndim == 2:
            rank2_initializers.append((initializer.name, shape))
    if len(initializers) > 50:
        print(f"  ... ({len(initializers) - 50} more)")
    print(f"[ONNX][Inits] rank2_count={len(rank2_initializers)} rank2_names_shapes={rank2_initializers}")

    # Gemm/MatMul nodes with candidate weight inputs
    matrix_multiply_nodes = []
    for node in onn_model.graph.node:
        if node.op_type in ("Gemm", "MatMul") and len(node.input) >= 2:
            matrix_multiply_nodes.append((node.name or '(anon)', node.op_type, node.input[0], node.input[1]))
    print(f"[ONNX][Gemm/MatMul] count={len(matrix_multiply_nodes)}")
    for node_name, op_kind, input_name, weight_name in matrix_multiply_nodes[:50]:
        print(f"  {op_kind}: name='{node_name}' X='{input_name}' W='{weight_name}'")

    print("[ONNX][Hint] The importer expects rank‑2 weights (Dense). If none are present, this model may not be a plain MLP; consider exporting an MLP or extracting a linear head.")


if __name__ == '__main__':
    main()
