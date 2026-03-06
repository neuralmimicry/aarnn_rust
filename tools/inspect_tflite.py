#!/usr/bin/env python3
"""
Inspect a TFLite model and print basic information useful for debugging imports.

Usage:
  python3 tools/inspect_tflite.py --in model.tflite

Outputs to stdout/stderr:
- Interpreter path (if available): list of 2D tensors (name, dtype, shape) and quantization summary.
- FlatBuffer path (if available): builtin op codes in subgraph and a count of constant rank-2 buffers.
"""
import argparse
import os
import sys


def load_tflite_interpreter(path):
    try:
        import tensorflow as tf  # noqa: F401
        try:
            interpreter = tf.lite.Interpreter(model_path=path)
            return interpreter, "tensorflow"
        except Exception:
            pass
    except Exception:
        pass
    try:
        from tflite_runtime.interpreter import Interpreter  # type: ignore
        interpreter = Interpreter(model_path=path)
        return interpreter, "tflite-runtime"
    except Exception:
        return None, None


def _get_quantization_summary(tensor_detail):
    """Return a concise quantization summary string for a tensor detail.

    Handles numpy arrays (including empty) without ambiguous truth value errors.
    """
    qp = tensor_detail.get('quantization_parameters', {}) or {}
    scales = qp.get('scales', [])
    zeros = qp.get('zero_points', [])
    axis = qp.get('quantized_dimension', 0) or 0

    def _convert_to_list(data):
        try:
            # numpy arrays expose .tolist(); Python lists pass through
            return data.tolist() if hasattr(data, 'tolist') else (list(data) if hasattr(data, '__iter__') and not isinstance(data, (str, bytes)) else [])
        except Exception:
            return []

    scales_list = _convert_to_list(scales)
    zeros_list = _convert_to_list(zeros)

    if len(scales_list) == 1 and len(zeros_list) == 1:
        return f"per_tensor scale={scales_list[0]} zp={zeros_list[0]}"
    if len(scales_list) > 1 and len(scales_list) == len(zeros_list):
        return f"per_axis axis={axis} n={len(scales_list)}"

    # Legacy per-tensor tuple (scale, zero_point)
    q = tensor_detail.get('quantization', None)
    if isinstance(q, tuple) and len(q) == 2 and q[0] not in (None, 0):
        return f"per_tensor scale={q[0]} zp={q[1] or 0}"
    return "none"


def inspect_via_interpreter(path):
    interpreter, runtime_name = load_tflite_interpreter(path)
    if interpreter is None:
        print("[Inspect][Interp] No interpreter available (tensorflow or tflite-runtime)")
        return
    interpreter.allocate_tensors()
    tensor_details = interpreter.get_tensor_details()
    get_tensor_func = interpreter.get_tensor
    print(f"[Inspect][Interp] runtime={runtime_name} tensors={len(tensor_details)}")
    count_2d = 0
    for detail in tensor_details:
        name = detail.get('name', '')
        try:
            weights = get_tensor_func(detail['index'])
        except Exception:
            continue
        shape = getattr(weights, 'shape', None)
        if shape is None or len(shape) != 2:
            continue
        count_2d += 1
        print(f"  2D: name='{name}' dtype={getattr(weights,'dtype',type(weights))} shape={list(shape)} quant={_get_quantization_summary(detail)}")
    print(f"[Inspect][Interp] total 2D tensors: {count_2d}")


def inspect_via_flatbuffer(path):
    try:
        from tflite_support import schema_py_generated as tflite_schema  # preferred
    except Exception:
        try:
            import tflite as tflite_schema  # fallback
        except Exception as e:
            print(f"[Inspect][FB] No schema available: {e}")
            return
    import numpy as np  # noqa: F401 (import needed for DataAsNumpy compatibility)
    with open(path, 'rb') as f:
        buffer_data = f.read()
    model = tflite_schema.Model.GetRootAsModel(buffer_data, 0)
    subgraph = model.Subgraphs(0)
    # opcode map
    opcode_map = {}
    for i in range(model.OperatorCodesLength()):
        op_code = model.OperatorCodes(i)
        opcode_map[i] = op_code.BuiltinCode()
    # list builtin codes
    builtin_codes = []
    for oi in range(subgraph.OperatorsLength()):
        op = subgraph.Operators(oi)
        opcode_idx = op.OpcodeIndex()
        code = opcode_map.get(opcode_idx, -1)
        builtin_codes.append(code)
    print(f"[Inspect][FB] Builtin op codes in subgraph: {builtin_codes}")
    # scan constant 2D buffers
    def get_constant_2d_buffer_count():
        count = 0
        for ti in range(subgraph.TensorsLength()):
            tensor = subgraph.Tensors(ti)
            bidx = tensor.Buffer()
            if bidx <= 0:
                continue
            tb = model.Buffers(bidx)
            data_attr = getattr(tb, 'DataAsNumpy', None)
            buffer_values = None
            if callable(data_attr):
                buffer_values = data_attr()
            else:
                try:
                    buffer_values = tb.DataAsNumpy()
                except Exception:
                    buffer_values = None
            if buffer_values is None:
                continue
            if tensor.ShapeLength() == 2:
                count += 1
        return count
    print(f"[Inspect][FB] Constant rank-2 buffers: {get_constant_2d_buffer_count()}")


def main():
    argument_parser = argparse.ArgumentParser()
    argument_parser.add_argument('--in', dest='input_tflite_path', required=True)
    args = argument_parser.parse_args()
    file_path = args.input_tflite_path
    if not os.path.exists(file_path):
        print(f"File not found: {file_path}")
        sys.exit(2)
    print(f"Inspecting: {file_path}")
    inspect_via_interpreter(file_path)
    inspect_via_flatbuffer(file_path)


if __name__ == '__main__':
    main()
