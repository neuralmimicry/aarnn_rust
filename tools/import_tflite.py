#!/usr/bin/env python3
"""
Import a TFLite MLP (as exported by tools/export_tflite.py) and emit a
neuromorphic_demo Runner snapshot JSON ("network.json" shape) to the
given path.

Usage:
  python3 tools/import_tflite.py --in model.tflite --out-network network.json

Notes:
- Supports float and common quantized (int8/uint8) models.
- Robust name matching across TF2 scopes (e.g., StatefulPartitionedCall/sequential/...).
- Heuristic fallback mapping when names don’t match.
- FlatBuffer fallback path available (requires tflite-support OR tflite + flatbuffers).
- Env vars:
  - NMD_TFLITE_DEBUG=1 → print discovered kernels and final mapping to stderr
  - NMD_TFLITE_FB=1    → force FlatBuffer fallback path
"""
import argparse
import json
import os
import sys
import numpy as np


def create_matrix_dict(rows, cols, data):
    return {"rows": int(rows), "cols": int(cols), "data": [float(x) for x in data]}


def convert_to_snapshot(weights_in, weights_hidden_fwd, weights_out):
    network_snapshot = {
        "net": {
            "n_sensory":  int(weights_in.shape[1]),
            "num_hidden_layers": int(len(weights_hidden_fwd) + 1),
            "num_hidden_per_layer_initial": int(weights_in.shape[0]),
            "n_output": int(weights_out.shape[0]),
            # Defaults for the rest; UI build ignores many of these on import
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
        "w_in": create_matrix_dict(*weights_in.shape, weights_in.flatten().tolist()),
        "w_hh_fwd": [create_matrix_dict(*wf.shape, wf.flatten().tolist()) for wf in weights_hidden_fwd],
        "w_hh_bwd": [create_matrix_dict(wf.shape[1], 0, []) for wf in weights_hidden_fwd],
        "w_hh_rec": [],
        "w_out": create_matrix_dict(*weights_out.shape, weights_out.flatten().tolist()),
    }
    return network_snapshot


def align_chain(weights_in, weights_hidden_fwd, weights_out):
    """Ensure consecutive matrices have compatible input sizes by truncating or zero-padding."""
    def fix_in(mat, expected_in):
        out, inp = mat.shape
        if inp == expected_in:
            return mat
        if inp > expected_in:
            return mat[:, :expected_in]
        pad = np.zeros((out, expected_in - inp), dtype=np.float32)
        return np.concatenate([mat, pad], axis=1)

    wi = weights_in.astype(np.float32, copy=False)
    prev_out = wi.shape[0]
    w_fwd = []
    for mat in weights_hidden_fwd:
        m = fix_in(mat.astype(np.float32, copy=False), prev_out)
        prev_out = m.shape[0]
        w_fwd.append(m)
    w_out = fix_in(weights_out.astype(np.float32, copy=False), prev_out)
    return wi, w_fwd, w_out


def load_tflite_interpreter(model_path):
    """Return a TFLite Interpreter instance from TF or tflite_runtime."""
    # Try TensorFlow first
    try:
        import tensorflow as tf  # noqa: F401
        try:
            interpreter = tf.lite.Interpreter(model_path=model_path)
            return interpreter, "tensorflow"
        except Exception:
            pass
    except Exception:
        pass
    # Try tflite-runtime
    try:
        from tflite_runtime.interpreter import Interpreter  # type: ignore
        interpreter = Interpreter(model_path=model_path)
        return interpreter, "tflite-runtime"
    except Exception:
        raise RuntimeError(
            "Neither TensorFlow nor tflite-runtime is available. Install one of them: "
            "pip install tensorflow  (or) pip install tflite-runtime"
        )


def load_tflite_schema():
    """Return the TFLite FlatBuffer schema module."""
    if sys.version_info < (3, 12):
        try:
            from tflite_support import schema_py_generated as tflite_schema  # preferred
            return tflite_schema
        except Exception:
            pass
    try:
        import tflite as tflite_schema  # alternative schema module
        return tflite_schema
    except Exception as e:
        msg = (
            "FlatBuffer fallback requires a TFLite schema module. Install either:\n"
            "  pip install tflite-support flatbuffers   (preferred for Python < 3.12)\n"
            "or:\n"
            "  pip install tflite flatbuffers"
        )
        if sys.version_info >= (3, 12):
            msg += "\nNote: tflite-support is not compatible with Python 3.12+."
        raise RuntimeError(msg) from e


def is_2d_weight_kernel(tensor_detail, tensor_data):
    """Accept float (fp16/fp32) and quantized (int8/uint8) tensors that can be treated as 2D."""
    try:
        shape = tensor_data.shape
    except Exception:
        return False
    if not isinstance(tensor_data, np.ndarray):
        return False
    
    # Squeeze out dimensions of size 1
    effective_shape = [s for s in shape if s > 1]
    if len(effective_shape) == 2:
        # Effectively 2D
        pass
    elif len(shape) == 2 and shape[0] > 0 and shape[1] > 0:
        # Already 2D, even if one dim is 1
        pass
    else:
        return False
    return tensor_data.dtype in (np.float32, np.float16, np.int8, np.uint8)


def _get_quantization_params(tensor_detail):
    """Extract per-tensor or per-axis quantization parameters.

    Returns dict with keys:
      kind: 'per_tensor'|'per_axis'|'none'
      scale(s), zero_point(s), axis
    """
    qp = tensor_detail.get('quantization_parameters', {}) or {}
    scales = qp.get('scales', []) or []
    zeros = qp.get('zero_points', []) or []
    axis = qp.get('quantized_dimension', 0) or 0
    if isinstance(scales, np.ndarray):
        scales = scales.tolist()
    if isinstance(zeros, np.ndarray):
        zeros = zeros.tolist()
    if len(scales) == 1 and len(zeros) == 1:
        return {'kind': 'per_tensor', 'scale': float(scales[0]), 'zero_point': int(zeros[0])}
    if len(scales) > 1 and len(scales) == len(zeros):
        return {'kind': 'per_axis', 'scales': [float(x) for x in scales], 'zero_points': [int(x) for x in zeros], 'axis': int(axis)}
    # Try legacy per-tensor field 'quantization' (scale, zero_point)
    q = tensor_detail.get('quantization', None)
    if isinstance(q, tuple) and len(q) == 2 and q[0] not in (None, 0):
        return {'kind': 'per_tensor', 'scale': float(q[0]), 'zero_point': int(q[1] or 0)}
    return {'kind': 'none'}


def dequantize_weight_tensor(array_data, tensor_detail):
    """Return array_data as float32, dequantizing if needed (supports per-tensor and per-axis)."""
    if array_data.dtype in (np.float32, np.float16):
        return array_data.astype(np.float32, copy=False)
    if array_data.dtype not in (np.int8, np.uint8):
        # Unexpected dtype; best effort cast
        return array_data.astype(np.float32, copy=False)
    q = _get_quantization_params(tensor_detail)
    x = array_data.astype(np.float32)
    if q['kind'] == 'per_tensor':
        scale = q.get('scale', 0.0)
        zp = q.get('zero_point', 0)
        if scale == 0.0:
            return x  # avoid div by zero; fallback
        return scale * (x - float(zp))
    if q['kind'] == 'per_axis':
        scales = np.array(q['scales'], dtype=np.float32)
        zeros = np.array(q['zero_points'], dtype=np.float32)
        axis = int(q['axis'])
        # Broadcast over axis
        if axis < 0 or axis >= x.ndim:
            axis = 0
        shape = [1] * x.ndim
        shape[axis] = x.shape[axis]
        scale_b = scales.reshape(shape)
        zp_b = zeros.reshape(shape)
        return scale_b * (x - zp_b)
    # No quant info; best effort
    return x


def discover_all_2d_kernels(tensor_details, get_tensor_func):
    """Return list of (name, dtype, shape, dequantized_float32_array, detail)."""
    discovered_kernels = []
    for detail in tensor_details:
        try:
            weights = get_tensor_func(detail['index'])
        except Exception:
            continue
        if not is_2d_weight_kernel(detail, weights):
            continue
        weights_f32 = dequantize_weight_tensor(weights, detail)
        # Squeeze extra dimensions if it's not already 2D
        if weights_f32.ndim > 2:
            eff_shape = [s for s in weights_f32.shape if s > 1]
            if len(eff_shape) == 2:
                weights_f32 = weights_f32.reshape(eff_shape)
        discovered_kernels.append((detail.get('name', ''), str(weights.dtype), list(weights_f32.shape), weights_f32, detail))
    return discovered_kernels


def pick_name_match(name, keys):
    name = name or ""
    for k in keys:
        if k in name:
            return True
    return False


def find_kernel_by_name_keys(tensor_details, get_tensor_func, key_variants):
    """Find a 2D kernel (float or quantized), dequantize to float32.
    Return ndarray(float32) or None.
    """
    candidates = []
    for detail in tensor_details:
        name = detail.get('name', '')
        if not pick_name_match(name, key_variants):
            continue
        try:
            weights = get_tensor_func(detail['index'])
        except Exception:
            continue
        if not is_2d_weight_kernel(detail, weights):
            continue
        weights_f32 = dequantize_weight_tensor(weights, detail)
        # Squeeze extra dimensions if it's not already 2D
        if weights_f32.ndim > 2:
            eff_shape = [s for s in weights_f32.shape if s > 1]
            if len(eff_shape) == 2:
                weights_f32 = weights_f32.reshape(eff_shape)
        candidates.append((name, weights_f32))
    if not candidates:
        return None
    # Prefer the largest parameter tensor (kernel over bias/constants)
    candidates.sort(key=lambda x: (x[1].size, x[1].shape[0], x[1].shape[1]), reverse=True)
    return candidates[0][1]


def import_via_interpreter(path, debug_mode=False):
    interpreter, _ = load_tflite_interpreter(path)
    interpreter.allocate_tensors()
    tensor_details = interpreter.get_tensor_details()
    get_tensor_func = interpreter.get_tensor

    def generate_keys_for(tag):
        return [
            tag,
            f"sequential/{tag}",
            f"model/{tag}",
            f"StatefulPartitionedCall/sequential/{tag}",
            f"StatefulPartitionedCall/{tag}",
        ]

    kernel_in = find_kernel_by_name_keys(tensor_details, get_tensor_func, generate_keys_for('dense_in'))
    kernel_out = find_kernel_by_name_keys(tensor_details, get_tensor_func, generate_keys_for('dense_out'))
    hidden_fwd = []
    layer_idx = 0
    while True:
        ki = find_kernel_by_name_keys(tensor_details, get_tensor_func, generate_keys_for(f'dense_fwd_{layer_idx}'))
        if ki is None:
            break
        hidden_fwd.append(ki)
        layer_idx += 1

    kernels_2d = discover_all_2d_kernels(tensor_details, get_tensor_func)
    if debug_mode:
        sys.stderr.write("[TFLite] 2D kernels (name, dtype, shape):\n")
        for name, dtype, shape, _weights, _detail in kernels_2d:
            qp = _get_quantization_params(_detail)
            if qp['kind'] == 'per_tensor':
                qsum = f"per_tensor scale={qp['scale']:.6g} zp={qp['zero_point']}"
            elif qp['kind'] == 'per_axis':
                qsum = f"per_axis axis={qp['axis']} n={len(qp['scales'])}"
            else:
                qsum = "none"
            sys.stderr.write(f"  - {name}: dtype={dtype} shape={shape} quant={qsum}\n")

    if kernel_in is None or kernel_out is None:
        if len(kernels_2d) >= 2:
            ordered_weights = sorted([w for (_name, _dtype, _shape, w, _detail) in kernels_2d], key=lambda a: (a.shape[0], a.shape[1]))
            if kernel_in is None and len(ordered_weights) >= 1:
                kernel_in = ordered_weights[0]
            if kernel_out is None and len(ordered_weights) >= 2:
                kernel_out = ordered_weights[-1]
            if not hidden_fwd and len(ordered_weights) > 2:
                hidden_fwd = ordered_weights[1:-1]

    if kernel_in is None or kernel_out is None:
        raise RuntimeError("Interpreter path could not find required Dense kernels.")

    # Keras/TFLite Dense kernel layout is (in, out) → transpose back to (out, in)
    weights_in = np.array(kernel_in, dtype=np.float32).T.copy()
    weights_hidden_fwd = [np.array(w, dtype=np.float32).T.copy() for w in hidden_fwd]
    weights_out = np.array(kernel_out, dtype=np.float32).T.copy()
    return weights_in, weights_hidden_fwd, weights_out


def import_via_flatbuffer(path, debug_mode=False, allow_fallback=False):
    """Parse FlatBuffer model and extract FULLY_CONNECTED weights.
    Tries tflite-support schema first, then tflite. Requires flatbuffers.
    """
    tflite_schema = load_tflite_schema()

    with open(path, 'rb') as f:
        buffer_data = f.read()
    model = tflite_schema.Model.GetRootAsModel(buffer_data, 0)
    subgraph = model.Subgraphs(0)

    # Map opcode index → builtin code
    opcode_map = {}
    for i in range(model.OperatorCodesLength()):
        op_code = model.OperatorCodes(i)
        opcode_map[i] = op_code.BuiltinCode()

    # Collect FULLY_CONNECTED (9), BATCH_MATMUL (126), or MATMUL (131) operators 
    # in execution order.
    ops = []
    for oi in range(subgraph.OperatorsLength()):
        op = subgraph.Operators(oi)
        opcode_idx = op.OpcodeIndex()
        code = opcode_map.get(opcode_idx, -1)
        if code in (9, 126, 131):
            ops.append(op)

    # Optional debug: list available builtin codes
    if debug_mode:
        try:
            codes = []
            for oi in range(subgraph.OperatorsLength()):
                op = subgraph.Operators(oi)
                opcode_idx = op.OpcodeIndex()
                code = opcode_map.get(opcode_idx, -1)
                codes.append(code)
            sys.stderr.write(f"[TFLite][FB] Builtin op codes in subgraph: {codes}\n")
        except Exception:
            pass

    # If no FULLY_CONNECTED ops are present (e.g., SELECT_TF_OPS models),
    # optionally fall back to scanning all 2D constant tensors from buffers and infer layers.
    if not ops:
        if not allow_fallback:
            raise RuntimeError(
                "FlatBuffer path found no FULLY_CONNECTED/MATMUL ops. "
                "This model is likely not a simple MLP. "
                "Set NMD_TFLITE_ALLOW_FALLBACK=1 to force the 2D-constant fallback."
            )
        # Helper: collect all rank-2 tensors backed by non-empty buffers.
        def scan_2d_constant_kernels(schema_model, sub):
            matrices = []
            for ti in range(sub.TensorsLength()):
                tensor = sub.Tensors(ti)
                # Skip tensors without buffers/data
                bidx = tensor.Buffer()
                if bidx <= 0:
                    continue
                tb = schema_model.Buffers(bidx)
                data_attr = getattr(tb, 'DataAsNumpy', None)
                buf = None
                if callable(data_attr):
                    buf = data_attr()
                else:
                    try:
                        buf = tb.DataAsNumpy()
                    except Exception:
                        buf = None
                if buf is None:
                    continue
                # Identify rank-2 or squeezable rank-N tensors
                shape_len = tensor.ShapeLength()
                shape = [tensor.Shape(i) for i in range(shape_len)]
                eff_shape = [s for s in shape if s > 1]
                if len(eff_shape) == 2:
                    shape = eff_shape
                elif len(shape) == 2 and shape[0] > 0 and shape[1] > 0:
                    pass
                else:
                    continue
                # Decode dtype
                ttype = tensor.Type()
                if ttype == 0:       # FLOAT32
                    arr = np.frombuffer(buf, dtype=np.float32)
                elif ttype == 1:     # FLOAT16
                    arr = np.frombuffer(buf, dtype=np.float16).astype(np.float32)
                elif ttype == 9:     # INT8
                    arr = np.frombuffer(buf, dtype=np.int8)
                elif ttype == 3:     # UINT8
                    arr = np.frombuffer(buf, dtype=np.uint8)
                else:
                    continue
                if np.prod(shape, dtype=np.int64) != arr.size:
                    continue
                arr = arr.reshape(shape)
                # Dequantize if needed
                if arr.dtype in (np.int8, np.uint8):
                    qp = tensor.Quantization()
                    if qp is not None and qp.ScaleLength() > 0:
                        if qp.ScaleLength() == 1:
                            scale = qp.Scale(0)
                            zp = qp.ZeroPoint(0) if qp.ZeroPointLength() > 0 else 0
                            arr = scale * (arr.astype(np.float32) - float(zp))
                        else:
                            try:
                                axis = qp.QuantizedDimension()
                            except Exception:
                                axis = 0
                            scales = np.array([qp.Scale(i) for i in range(qp.ScaleLength())], dtype=np.float32)
                            zps = np.array([qp.ZeroPoint(i) if i < qp.ZeroPointLength() else 0 for i in range(qp.ScaleLength())], dtype=np.float32)
                            bshape = [1] * arr.ndim
                            bshape[axis] = arr.shape[axis]
                            arr = scales.reshape(bshape) * (arr.astype(np.float32) - zps.reshape(bshape))
                    else:
                        arr = arr.astype(np.float32)
                else:
                    arr = arr.astype(np.float32)
                matrices.append((ti, arr))
            return matrices

        mats = scan_2d_constant_kernels(model, subgraph)
        if debug_mode:
            sys.stderr.write(f"[TFLite][FB] Found {len(mats)} constant 2D buffers.\n")
        if len(mats) < 2:
            raise RuntimeError("FlatBuffer fallback: no suitable 2D constant tensors found.")
        # Heuristic ordering: sort by (rows, cols); pick first as W_in, last as W_out
        mats_sorted = sorted([m for (_ti, m) in mats], key=lambda a: (a.shape[0], a.shape[1]))
        W_in_fb = mats_sorted[0]
        W_out_fb = mats_sorted[-1]
        W_fwd_fb = mats_sorted[1:-1]
        def orient_matrix(mat):
            r, c = mat.shape
            return mat if r >= c else mat.T.copy()
        w_in = orient_matrix(W_in_fb)
        w_fwd = [orient_matrix(m) for m in W_fwd_fb]
        w_out = orient_matrix(W_out_fb)
        return w_in, w_fwd, w_out

    # Helper to access tensor buffer as numpy array of uint8 (robust to flatbuffers versions)
    def get_tensor_buffer_as_array(schema_model, tensor):
        bidx = tensor.Buffer()
        tb = schema_model.Buffers(bidx)
        data_attr = getattr(tb, 'DataAsNumpy', None)
        def to_u8_array(data):
            if data is None:
                return np.array([], dtype=np.uint8)
            try:
                return np.frombuffer(data, dtype=np.uint8)
            except TypeError:
                try:
                    return np.asarray(data, dtype=np.uint8)
                except Exception:
                    if isinstance(data, int):
                        return np.array([data], dtype=np.uint8)
                    return np.array([], dtype=np.uint8)
        if callable(data_attr):
            data = data_attr()
            return to_u8_array(data)
        try:
            data = tb.DataAsNumpy()
            return to_u8_array(data)
        except Exception:
            return np.array([], dtype=np.uint8)

    # Extract weight tensors from each FC op (inputs[1])
    weight_matrices = []
    for op in ops:
        inputs = [op.Inputs(i) for i in range(op.InputsLength())]
        if len(inputs) < 2:
            continue
        weight_tensor_idx = inputs[1]
        tensor = subgraph.Tensors(weight_tensor_idx)
        ttype = tensor.Type()
        shape = [tensor.Shape(i) for i in range(tensor.ShapeLength())]
        raw_buffer = get_tensor_buffer_as_array(model, tensor)
        # DType mapping using numeric enum values
        if ttype == 0:       # FLOAT32
            arr = raw_buffer.view(np.float32)
        elif ttype == 1:     # FLOAT16
            arr = raw_buffer.view(np.float16).astype(np.float32)
        elif ttype == 9:     # INT8
            arr = raw_buffer.view(np.int8)
        elif ttype == 3:     # UINT8
            arr = raw_buffer.view(np.uint8)
        else:
            # unknown/unhandled dtype; skip
            continue
        # reshape and squeeze
        if np.prod(shape, dtype=np.int64) != arr.size:
            continue
        arr = arr.reshape(shape)
        if arr.ndim > 2:
            eff_shape = [s for s in shape if s > 1]
            if len(eff_shape) == 2:
                arr = arr.reshape(eff_shape)

        # Dequantize if needed using QuantizationParameters
        if arr.dtype in (np.int8, np.uint8):
            qp = tensor.Quantization()
            if qp is not None and qp.ScaleLength() > 0:
                if qp.ScaleLength() == 1:
                    scale = qp.Scale(0)
                    zp = qp.ZeroPoint(0) if qp.ZeroPointLength() > 0 else 0
                    arr = scale * (arr.astype(np.float32) - float(zp))
                else:
                    try:
                        axis = qp.QuantizedDimension()
                    except Exception:
                        axis = 0
                    scales = np.array([qp.Scale(i) for i in range(qp.ScaleLength())], dtype=np.float32)
                    zps = np.array([qp.ZeroPoint(i) if i < qp.ZeroPointLength() else 0 for i in range(qp.ScaleLength())], dtype=np.float32)
                    bshape = [1] * arr.ndim
                    bshape[axis] = arr.shape[axis]
                    arr = scales.reshape(bshape) * (arr.astype(np.float32) - zps.reshape(bshape))
            else:
                arr = arr.astype(np.float32)
        else:
            arr = arr.astype(np.float32)

        weight_matrices.append(arr)

    if len(weight_matrices) < 2:
        raise RuntimeError("FlatBuffer fallback: insufficient FC layers to map.")

    # Heuristic mapping: first FC → W_in, last → W_out, middles → forward
    W_in_fb = weight_matrices[0]
    W_out_fb = weight_matrices[-1]
    W_fwd_fb = weight_matrices[1:-1]

    # Orient to (out, in): FC weights are commonly (out, in); if not, transpose
    def orient_matrix(mat):
        r, c = mat.shape
        return mat if r >= c else mat.T.copy()

    w_in = orient_matrix(W_in_fb)
    w_fwd = [orient_matrix(m) for m in W_fwd_fb]
    w_out = orient_matrix(W_out_fb)
    return w_in, w_fwd, w_out


def import_via_flatbuffer_cnn(path, debug_mode=False, max_layers=16):
    """Parse FlatBuffer model and extract CONV/DEPTHWISE/FULLY_CONNECTED ops,
    collapsing conv kernels to per-channel dense matrices (avg over spatial dims).
    """
    tflite_schema = load_tflite_schema()

    with open(path, 'rb') as f:
        buffer_data = f.read()
    model = tflite_schema.Model.GetRootAsModel(buffer_data, 0)
    subgraph = model.Subgraphs(0)

    opcode_map = {}
    for i in range(model.OperatorCodesLength()):
        op_code = model.OperatorCodes(i)
        opcode_map[i] = op_code.BuiltinCode()

    def get_tensor_buffer_as_array(schema_model, tensor):
        bidx = tensor.Buffer()
        tb = schema_model.Buffers(bidx)
        data_attr = getattr(tb, 'DataAsNumpy', None)
        def to_u8_array(data):
            if data is None:
                return np.array([], dtype=np.uint8)
            try:
                return np.frombuffer(data, dtype=np.uint8)
            except TypeError:
                try:
                    return np.asarray(data, dtype=np.uint8)
                except Exception:
                    if isinstance(data, int):
                        return np.array([data], dtype=np.uint8)
                    return np.array([], dtype=np.uint8)
        if callable(data_attr):
            data = data_attr()
            return to_u8_array(data)
        try:
            data = tb.DataAsNumpy()
            return to_u8_array(data)
        except Exception:
            return np.array([], dtype=np.uint8)

    def tensor_to_array(tensor):
        ttype = tensor.Type()
        shape = [tensor.Shape(i) for i in range(tensor.ShapeLength())]
        raw_buffer = get_tensor_buffer_as_array(model, tensor)
        raw_buffer = np.asarray(raw_buffer, dtype=np.uint8)
        if raw_buffer.ndim == 0:
            raw_buffer = raw_buffer.reshape(1)
        if ttype == 0:
            if raw_buffer.size % 4 != 0:
                return None
            arr = raw_buffer.view(np.float32)
        elif ttype == 1:
            if raw_buffer.size % 2 != 0:
                return None
            arr = raw_buffer.view(np.float16).astype(np.float32)
        elif ttype == 9:
            arr = raw_buffer.view(np.int8)
        elif ttype == 3:
            arr = raw_buffer.view(np.uint8)
        else:
            return None
        if np.prod(shape, dtype=np.int64) != arr.size:
            return None
        arr = arr.reshape(shape)
        if arr.dtype in (np.int8, np.uint8):
            qp = tensor.Quantization()
            if qp is not None and qp.ScaleLength() > 0:
                if qp.ScaleLength() == 1:
                    scale = qp.Scale(0)
                    zp = qp.ZeroPoint(0) if qp.ZeroPointLength() > 0 else 0
                    arr = scale * (arr.astype(np.float32) - float(zp))
                else:
                    try:
                        axis = qp.QuantizedDimension()
                    except Exception:
                        axis = 0
                    scales = np.array([qp.Scale(i) for i in range(qp.ScaleLength())], dtype=np.float32)
                    zps = np.array([qp.ZeroPoint(i) if i < qp.ZeroPointLength() else 0 for i in range(qp.ScaleLength())], dtype=np.float32)
                    bshape = [1] * arr.ndim
                    bshape[axis] = arr.shape[axis]
                    arr = scales.reshape(bshape) * (arr.astype(np.float32) - zps.reshape(bshape))
            else:
                arr = arr.astype(np.float32)
        else:
            arr = arr.astype(np.float32)
        return arr

    def conv_kernel_to_dense(arr):
        # CONV_2D: [H, W, C_in, C_out] -> [C_out, C_in]
        if arr.ndim != 4:
            return None
        h, w, c_in, c_out = arr.shape
        avg = arr.reshape(h * w, c_in, c_out).mean(axis=0)
        return avg.T.copy()

    def depthwise_kernel_to_dense(arr):
        # DEPTHWISE: [H, W, C_in, M] -> [C_out, C_in] where C_out=C_in*M
        if arr.ndim != 4:
            return None
        h, w, c_in, mult = arr.shape
        avg = arr.reshape(h * w, c_in, mult).mean(axis=0)
        out = np.zeros((c_in * mult, c_in), dtype=np.float32)
        for ci in range(c_in):
            for m in range(mult):
                out[ci * mult + m, ci] = avg[ci, m]
        return out

    matrices = []
    for oi in range(subgraph.OperatorsLength()):
        op = subgraph.Operators(oi)
        opcode_idx = op.OpcodeIndex()
        code = opcode_map.get(opcode_idx, -1)
        inputs = [op.Inputs(i) for i in range(op.InputsLength())]
        if len(inputs) < 2:
            continue
        weight_tensor_idx = inputs[1]
        tensor = subgraph.Tensors(weight_tensor_idx)
        arr = tensor_to_array(tensor)
        if arr is None:
            continue
        if code == 3:  # CONV_2D
            dense = conv_kernel_to_dense(arr)
        elif code == 4:  # DEPTHWISE_CONV_2D
            dense = depthwise_kernel_to_dense(arr)
        elif code in (9, 126, 131):  # FULLY_CONNECTED/MATMUL
            dense = arr
            if dense.ndim > 2:
                eff_shape = [s for s in dense.shape if s > 1]
                if len(eff_shape) == 2:
                    dense = dense.reshape(eff_shape)
            if dense.ndim != 2:
                dense = None
        else:
            dense = None
        if dense is None:
            continue
        if dense.shape[0] < dense.shape[1]:
            dense = dense.T.copy()
        matrices.append(dense)

    if len(matrices) < 2:
        raise RuntimeError("CNN import path: insufficient conv/FC layers to map.")

    hidden = matrices[1:-1]
    if max_layers is not None and max_layers > 0 and len(hidden) > max_layers:
        idxs = np.linspace(0, len(hidden) - 1, max_layers)
        idxs = [int(round(i)) for i in idxs]
        idxs = sorted(set(idxs))
        if len(idxs) < max_layers:
            for i in range(len(hidden)):
                if i not in idxs:
                    idxs.append(i)
                if len(idxs) >= max_layers:
                    break
        idxs = sorted(idxs[:max_layers])
        if debug_mode:
            sys.stderr.write(f"[TFLite][CNN] Downsampling hidden layers {len(hidden)} -> {len(idxs)}\n")
        hidden = [hidden[i] for i in idxs]
    matrices = [matrices[0]] + hidden + [matrices[-1]]

    w_in = matrices[0]
    w_out = matrices[-1]
    w_fwd = matrices[1:-1]
    if debug_mode:
        sys.stderr.write("[TFLite][CNN] Collapsed layer shapes:\n")
        sys.stderr.write(f"  W_in: {list(w_in.shape)}\n")
        for i, m in enumerate(w_fwd):
            sys.stderr.write(f"  W_fwd[{i}]: {list(m.shape)}\n")
        sys.stderr.write(f"  W_out: {list(w_out.shape)}\n")
    return w_in, w_fwd, w_out


def main():
    argument_parser = argparse.ArgumentParser()
    argument_parser.add_argument('--in', dest='input_tflite_path', required=True)
    argument_parser.add_argument('--out-network', dest='output_network_path', required=True)
    argument_parser.add_argument('--mode', dest='mode', choices=['mlp', 'cnn'], default='mlp')
    argument_parser.add_argument('--max-layers', dest='max_layers', type=int, default=16)
    argument_parser.add_argument('--max-params', dest='max_params', type=int, default=2_000_000)
    args = argument_parser.parse_args()

    debug_mode = os.environ.get('NMD_TFLITE_DEBUG', '') == '1'
    force_flatbuffer = os.environ.get('NMD_TFLITE_FB', '') == '1'

    def summarize_layer_shapes(prefix, weights_in, weights_hidden_fwd, weights_out):
        try:
            sys.stderr.write(prefix + "\n")
            sys.stderr.write(f"  W_in:  {list(weights_in.shape)}\n")
            for i, wf in enumerate(weights_hidden_fwd):
                sys.stderr.write(f"  W_fwd[{i}]: {list(wf.shape)}\n")
            sys.stderr.write(f"  W_out: {list(weights_out.shape)}\n")
        except Exception:
            pass

    def validate_limits(weights_in, weights_hidden_fwd, weights_out):
        allow_large = os.environ.get('NMD_TFLITE_ALLOW_LARGE', '') in ('1', 'true', 'TRUE')
        max_layers = args.max_layers
        max_params = args.max_params
        hidden_layers = len(weights_hidden_fwd) + 1
        total_params = int(weights_in.size + weights_out.size + sum(w.size for w in weights_hidden_fwd))
        if allow_large:
            return
        if hidden_layers > max_layers:
            raise RuntimeError(
                f"Refusing import: {hidden_layers} hidden layers > {max_layers}. "
                "Set NMD_TFLITE_ALLOW_LARGE=1 to override."
            )
        if total_params > max_params:
            raise RuntimeError(
                f"Refusing import: {total_params} params > {max_params}. "
                "Set NMD_TFLITE_ALLOW_LARGE=1 to override."
            )

    # Choose path: FlatBuffer first unless interpreter is forced.
    allow_fallback = os.environ.get('NMD_TFLITE_ALLOW_FALLBACK', '') == '1'
    try:
        use_interpreter = os.environ.get('NMD_TFLITE_INTERP', '') == '1'
        if args.mode == 'cnn':
            w_in, w_fwd, w_out = import_via_flatbuffer_cnn(
                args.input_tflite_path,
                debug_mode=debug_mode,
                max_layers=args.max_layers,
            )
        else:
            if force_flatbuffer or not use_interpreter:
                w_in, w_fwd, w_out = import_via_flatbuffer(
                    args.input_tflite_path,
                    debug_mode=debug_mode,
                    allow_fallback=allow_fallback,
                )
            else:
                w_in, w_fwd, w_out = import_via_interpreter(args.input_tflite_path, debug_mode=debug_mode)
    except Exception as e_primary:
        if debug_mode:
            sys.stderr.write(f"[TFLite] Primary import path failed: {e_primary}\n")
        if not allow_fallback:
            raise
        # Try the other path as fallback.
        if args.mode == 'cnn':
            w_in, w_fwd, w_out = import_via_flatbuffer_cnn(
                args.input_tflite_path,
                debug_mode=debug_mode,
                max_layers=args.max_layers,
            )
        else:
            if force_flatbuffer or not os.environ.get('NMD_TFLITE_INTERP', '') == '1':
                w_in, w_fwd, w_out = import_via_interpreter(args.input_tflite_path, debug_mode=debug_mode)
            else:
                w_in, w_fwd, w_out = import_via_flatbuffer(
                    args.input_tflite_path,
                    debug_mode=debug_mode,
                    allow_fallback=allow_fallback,
                )

    # Basic shape validation
    if w_in.shape[1] <= 0 or w_out.shape[0] <= 0:
        raise RuntimeError(f"Unusable shapes: W_in {w_in.shape}, W_out {w_out.shape}")

    w_in, w_fwd, w_out = align_chain(w_in, w_fwd, w_out)

    validate_limits(w_in, w_fwd, w_out)

    if debug_mode:
        summarize_layer_shapes("[TFLite] Final mapped layer shapes:", w_in, w_fwd, w_out)

    network_snapshot = convert_to_snapshot(w_in, w_fwd, w_out)
    with open(args.output_network_path, 'w') as f:
        json.dump(network_snapshot, f, indent=2)
    print(f"Wrote network snapshot: {args.output_network_path}")


if __name__ == '__main__':
    main()
