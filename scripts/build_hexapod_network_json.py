#!/usr/bin/env python3
"""
Build a Webots Hexapod-focused Runner snapshot JSON and matching NetworkConfig JSON.

This builder intentionally reuses the same I/O semantics used by the Drosophila
pipeline for vision channels:
  - camera event channels are expanded as <camera>.on/off.rXXcYY
  - an .io_alignment.json sidecar is emitted beside the config

The resulting snapshot is Hexapod-biased (18 motor outputs) while carrying
compact, AARNN-ready morphology/growth settings aligned with existing
biomimicry defaults in the Rust runtime.
"""

from __future__ import annotations

import argparse
import json
import math
import random
import re
from pathlib import Path
from typing import Dict, Iterable, List, Sequence, Tuple


NAME_RE = re.compile(r'\bname\s+"([^"]+)"')
TYPE_RE = re.compile(r'\btype\s+"([^"]+)"')
HEX_ACTUATOR_RE = re.compile(r"^hex_o_(?P<idx>\d{3})_(?P<leg>[a-z]{2})_(?P<joint>coxa|femur|tibia)$")
DROSOPHILA_REFERENCE_DATASETS = ["BANC v626", "FAFB v783"]
HEX_SENSORS_REGEX = r"^hex_s_[0-9]{2}_.*$"
HEX_ACTUATORS_REGEX = r"^hex_o_[0-9]{3}_.*$"


def iter_node_blocks(proto_text: str, node_type: str) -> Iterable[str]:
    pattern = re.compile(rf"\b{re.escape(node_type)}\b\s*\{{")
    for match in pattern.finditer(proto_text):
        start = match.end() - 1
        depth = 0
        i = start
        n = len(proto_text)
        while i < n:
            ch = proto_text[i]
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    yield proto_text[start : i + 1]
                    break
            i += 1


def first_name(block: str) -> str | None:
    m = NAME_RE.search(block)
    if not m:
        return None
    name = m.group(1).strip()
    return name or None


def dedupe_keep_order(values: Sequence[str]) -> List[str]:
    seen = set()
    out: List[str] = []
    for value in values:
        if value in seen:
            continue
        seen.add(value)
        out.append(value)
    return out


def index_digits(n: int) -> int:
    max_index = max(0, n - 1)
    digits = 1
    while max_index >= 10:
        max_index //= 10
        digits += 1
    return max(2, digits)


def camera_event_channels(camera_name: str, rows: int, cols: int) -> List[str]:
    row_digits = index_digits(rows)
    col_digits = index_digits(cols)
    out: List[str] = []
    for r in range(rows):
        for c in range(cols):
            out.append(f"{camera_name}.on.r{r:0{row_digits}d}c{c:0{col_digits}d}")
            out.append(f"{camera_name}.off.r{r:0{row_digits}d}c{c:0{col_digits}d}")
    return out


def sort_sensor_channels_for_mapper(channels: Sequence[str]) -> List[str]:
    camera_re = re.compile(r"^(?P<base>.+)\.(?P<polarity>on|off)\.r(?P<row>\d+)c(?P<col>\d+)$")
    axis_order = {
        "x": 0,
        "y": 1,
        "z": 2,
        "mean": 3,
        "center": 4,
        "mean_gray": 3,
        "center_gray": 4,
    }

    def key(channel: str) -> Tuple[str, int, int, int, int, int, str]:
        cam = camera_re.match(channel)
        if cam:
            base = cam.group("base")
            row = int(cam.group("row"))
            col = int(cam.group("col"))
            polarity = 0 if cam.group("polarity") == "on" else 1
            return (base, 0, 0, row, col, polarity, channel)
        if "." in channel:
            base, axis = channel.rsplit(".", 1)
            if axis in axis_order:
                return (base, 0, 1, axis_order[axis], 0, 0, channel)
        return (channel, 1, 0, 0, 0, 0, channel)

    return sorted(dedupe_keep_order(channels), key=key)


def parse_hexapod_channels(
    proto_path: Path,
    camera_retina_width: int,
    camera_retina_height: int,
) -> Tuple[List[str], List[str], Dict[str, int]]:
    text = proto_path.read_text(encoding="utf-8")

    sensor_devices: List[Tuple[str, List[str]]] = []
    actuator_devices: List[str] = []
    camera_channel_counts: Dict[str, int] = {}

    for block in iter_node_blocks(text, "Accelerometer"):
        name = first_name(block) or "accelerometer"
        sensor_devices.append((name, [f"{name}.x", f"{name}.y", f"{name}.z"]))

    for block in iter_node_blocks(text, "Camera"):
        name = first_name(block)
        if name:
            channels = camera_event_channels(name, camera_retina_height, camera_retina_width)
            sensor_devices.append((name, channels))
            camera_channel_counts[name] = len(channels)

    for block in iter_node_blocks(text, "Gyro"):
        name = first_name(block) or "gyro"
        sensor_devices.append((name, [f"{name}.x", f"{name}.y", f"{name}.z"]))

    for block in iter_node_blocks(text, "DistanceSensor"):
        name = first_name(block)
        if name:
            sensor_devices.append((name, [name]))

    for block in iter_node_blocks(text, "LightSensor"):
        name = first_name(block)
        if name:
            sensor_devices.append((name, [name]))

    for block in iter_node_blocks(text, "TouchSensor"):
        name = first_name(block)
        if not name:
            continue
        t_match = TYPE_RE.search(block)
        t = (t_match.group(1).strip().lower() if t_match else "")
        if "force-3d" in t or "force3d" in t:
            sensor_devices.append((name, [f"{name}.x", f"{name}.y", f"{name}.z"]))
        else:
            sensor_devices.append((name, [name]))

    for block in iter_node_blocks(text, "PositionSensor"):
        name = first_name(block)
        if name:
            sensor_devices.append((name, [name]))

    for block in iter_node_blocks(text, "RotationalMotor"):
        name = first_name(block)
        if name:
            actuator_devices.append(name)

    for block in iter_node_blocks(text, "LinearMotor"):
        name = first_name(block)
        if name:
            actuator_devices.append(name)

    sensor_channels: List[str] = []
    for _, channels in sorted(sensor_devices, key=lambda item: item[0]):
        sensor_channels.extend(channels)
    sensor_channels = sort_sensor_channels_for_mapper(sensor_channels)
    actuator_devices = sorted(dedupe_keep_order(actuator_devices))

    return sensor_channels, actuator_devices, camera_channel_counts


def synthetic_names(prefix: str, count: int, width: int) -> List[str]:
    return [f"{prefix}_{i:0{width}d}" for i in range(count)]


def resize_channels(channels: Sequence[str], target: int, prefix: str, width: int) -> Tuple[List[str], str]:
    channels = dedupe_keep_order(channels)
    if target <= 0:
        return [], "empty_target"
    if len(channels) == target:
        return list(channels), "exact"
    if len(channels) > target:
        return list(channels[:target]), "trimmed"
    out = list(channels)
    i = 0
    while len(out) < target:
        out.append(f"{prefix}_{i:0{width}d}")
        i += 1
    return out, "padded"


def infer_sensor_role(name: str) -> str:
    low = name.lower()
    if ".on.r" in low or ".off.r" in low or "camera" in low:
        return "vision"
    if "ultrasonic" in low or "distance" in low:
        return "range"
    if "_foot" in low:
        return "contact"
    if "_coxa" in low or "_femur" in low or "_tibia" in low:
        return "proprioception"
    if "accel" in low or "gyro" in low:
        return "vestibular"
    return "general"


def infer_output_role(name: str) -> str:
    m = HEX_ACTUATOR_RE.match(name.lower())
    if not m:
        return "motor"
    return f"leg_{m.group('leg')}_{m.group('joint')}"


def group_from_sensor(name: str) -> str:
    low = name.lower()
    if "_lf_" in low:
        return "left_front"
    if "_lm_" in low:
        return "left_middle"
    if "_lr_" in low:
        return "left_rear"
    if "_rf_" in low:
        return "right_front"
    if "_rm_" in low:
        return "right_middle"
    if "_rr_" in low:
        return "right_rear"
    if "camera" in low:
        return "head_vision"
    if "ultrasonic" in low or "distance" in low:
        return "head_range"
    if "accel" in low or "gyro" in low:
        return "body_state"
    return "body_misc"


def group_from_output(name: str) -> str:
    m = HEX_ACTUATOR_RE.match(name.lower())
    if not m:
        return "motor_misc"
    return f"{m.group('leg')}_{m.group('joint')}"


def aarnn_laminar_io_layers(hidden_layer_count: int) -> Tuple[int, int]:
    hidden_layer_count = max(1, hidden_layer_count)
    return 0, hidden_layer_count - 1


def empty_matrix(rows: int, cols: int, fill: float = 0.0) -> List[List[float]]:
    return [[fill for _ in range(cols)] for _ in range(rows)]


def empty_matrix_u32(rows: int, cols: int, fill: int = 0) -> List[List[int]]:
    return [[fill for _ in range(cols)] for _ in range(rows)]


def flatten_row_major(mat: List[List[float]]) -> List[float]:
    out: List[float] = []
    for row in mat:
        out.extend(float(v) for v in row)
    return out


def flatten_row_major_u32(mat: List[List[int]]) -> List[int]:
    out: List[int] = []
    for row in mat:
        out.extend(int(v) for v in row)
    return out


def sample_weight(rng: random.Random, base: float) -> float:
    sign = -1.0 if rng.random() < 0.18 else 1.0
    jitter = base * (0.25 + 0.75 * rng.random())
    return sign * jitter


def sparse_connect(
    rows: int,
    cols: int,
    prob: float,
    rng: random.Random,
    *,
    min_per_col: int,
    min_per_row: int,
    base_weight: float,
    allow_self: bool = True,
) -> Tuple[List[List[float]], List[List[int]]]:
    mat = empty_matrix(rows, cols, 0.0)
    presence = empty_matrix_u32(rows, cols, 0)
    prob = max(0.0, min(1.0, prob))

    for r in range(rows):
        for c in range(cols):
            if not allow_self and rows == cols and r == c:
                continue
            if rng.random() < prob:
                mat[r][c] = sample_weight(rng, base_weight)
                presence[r][c] = 1

    min_per_col = max(0, min(min_per_col, rows))
    for c in range(cols):
        connected = [r for r in range(rows) if presence[r][c]]
        needed = min_per_col - len(connected)
        if needed <= 0:
            continue
        choices = [r for r in range(rows) if allow_self or rows != cols or r != c]
        rng.shuffle(choices)
        for r in choices[:needed]:
            if presence[r][c]:
                continue
            mat[r][c] = sample_weight(rng, base_weight)
            presence[r][c] = 1

    min_per_row = max(0, min(min_per_row, cols))
    for r in range(rows):
        connected = [c for c in range(cols) if presence[r][c]]
        needed = min_per_row - len(connected)
        if needed <= 0:
            continue
        choices = [c for c in range(cols) if allow_self or rows != cols or c != r]
        rng.shuffle(choices)
        for c in choices[:needed]:
            if presence[r][c]:
                continue
            mat[r][c] = sample_weight(rng, base_weight)
            presence[r][c] = 1

    return mat, presence


def build_hexapod_regions() -> List[dict]:
    return [
        {
            "name": "supraesophageal_ganglion",
            "shape": {"shape": "ellipsoid", "center": [0.0, 8.0, 6.0], "radii": [14.0, 10.0, 8.0]},
            "center": [0.0, 8.0, 6.0],
            "radii": [14.0, 10.0, 8.0],
            "type_distribution": [["Sensory", 0.30], ["Interneuron", 0.45], ["Motor", 0.15], ["Neuromodulatory", 0.10]],
        },
        {
            "name": "subesophageal_ganglion",
            "shape": {"shape": "ellipsoid", "center": [0.0, -6.0, 2.0], "radii": [12.0, 8.0, 6.0]},
            "center": [0.0, -6.0, 2.0],
            "radii": [12.0, 8.0, 6.0],
            "type_distribution": [["Interneuron", 0.46], ["Motor", 0.34], ["Sensory", 0.12], ["Neuromodulatory", 0.08]],
        },
        {
            "name": "thoracic_pattern_network",
            "shape": {"shape": "tube", "line_from": [-18.0, -28.0, -2.0], "line_to": [18.0, -28.0, -2.0], "radius": 5.5},
            "center": [0.0, -28.0, -2.0],
            "radii": [18.0, 5.5, 5.5],
            "type_distribution": [["Interneuron", 0.42], ["Motor", 0.35], ["command_neuron", 0.15], ["Neuromodulatory", 0.08]],
        },
        {
            "name": "left_front_leg_ganglion",
            "shape": {"shape": "ellipsoid", "center": [20.0, 18.0, -4.0], "radii": [7.0, 6.0, 5.0]},
            "center": [20.0, 18.0, -4.0],
            "radii": [7.0, 6.0, 5.0],
            "type_distribution": [["Sensory", 0.22], ["Interneuron", 0.38], ["Motor", 0.32], ["Neuromodulatory", 0.08]],
        },
        {
            "name": "left_mid_leg_ganglion",
            "shape": {"shape": "ellipsoid", "center": [24.0, 0.0, -4.0], "radii": [7.0, 6.0, 5.0]},
            "center": [24.0, 0.0, -4.0],
            "radii": [7.0, 6.0, 5.0],
            "type_distribution": [["Sensory", 0.22], ["Interneuron", 0.38], ["Motor", 0.32], ["Neuromodulatory", 0.08]],
        },
        {
            "name": "left_rear_leg_ganglion",
            "shape": {"shape": "ellipsoid", "center": [20.0, -18.0, -4.0], "radii": [7.0, 6.0, 5.0]},
            "center": [20.0, -18.0, -4.0],
            "radii": [7.0, 6.0, 5.0],
            "type_distribution": [["Sensory", 0.22], ["Interneuron", 0.38], ["Motor", 0.32], ["Neuromodulatory", 0.08]],
        },
        {
            "name": "right_front_leg_ganglion",
            "shape": {"shape": "ellipsoid", "center": [-20.0, 18.0, -4.0], "radii": [7.0, 6.0, 5.0]},
            "center": [-20.0, 18.0, -4.0],
            "radii": [7.0, 6.0, 5.0],
            "type_distribution": [["Sensory", 0.22], ["Interneuron", 0.38], ["Motor", 0.32], ["Neuromodulatory", 0.08]],
        },
        {
            "name": "right_mid_leg_ganglion",
            "shape": {"shape": "ellipsoid", "center": [-24.0, 0.0, -4.0], "radii": [7.0, 6.0, 5.0]},
            "center": [-24.0, 0.0, -4.0],
            "radii": [7.0, 6.0, 5.0],
            "type_distribution": [["Sensory", 0.22], ["Interneuron", 0.38], ["Motor", 0.32], ["Neuromodulatory", 0.08]],
        },
        {
            "name": "right_rear_leg_ganglion",
            "shape": {"shape": "ellipsoid", "center": [-20.0, -18.0, -4.0], "radii": [7.0, 6.0, 5.0]},
            "center": [-20.0, -18.0, -4.0],
            "radii": [7.0, 6.0, 5.0],
            "type_distribution": [["Sensory", 0.22], ["Interneuron", 0.38], ["Motor", 0.32], ["Neuromodulatory", 0.08]],
        },
    ]


def region_centers_normalized() -> Dict[str, Tuple[float, float, float]]:
    return {
        "supraesophageal_ganglion": (0.0, 0.62, 0.32),
        "subesophageal_ganglion": (0.0, 0.36, 0.14),
        "thoracic_pattern_network": (0.0, -0.16, -0.04),
        "left_front_leg_ganglion": (0.56, 0.42, -0.12),
        "left_mid_leg_ganglion": (0.62, 0.06, -0.12),
        "left_rear_leg_ganglion": (0.56, -0.32, -0.12),
        "right_front_leg_ganglion": (-0.56, 0.42, -0.12),
        "right_mid_leg_ganglion": (-0.62, 0.06, -0.12),
        "right_rear_leg_ganglion": (-0.56, -0.32, -0.12),
    }


def region_for_sensor(name: str) -> str:
    low = name.lower()
    if "camera" in low or "ultrasonic" in low or "distance" in low:
        return "supraesophageal_ganglion"
    if "_lf_" in low:
        return "left_front_leg_ganglion"
    if "_lm_" in low:
        return "left_mid_leg_ganglion"
    if "_lr_" in low:
        return "left_rear_leg_ganglion"
    if "_rf_" in low:
        return "right_front_leg_ganglion"
    if "_rm_" in low:
        return "right_mid_leg_ganglion"
    if "_rr_" in low:
        return "right_rear_leg_ganglion"
    if "accel" in low or "gyro" in low:
        return "thoracic_pattern_network"
    return "subesophageal_ganglion"


def region_for_output(name: str) -> str:
    low = name.lower()
    if "_lf_" in low:
        return "left_front_leg_ganglion"
    if "_lm_" in low:
        return "left_mid_leg_ganglion"
    if "_lr_" in low:
        return "left_rear_leg_ganglion"
    if "_rf_" in low:
        return "right_front_leg_ganglion"
    if "_rm_" in low:
        return "right_mid_leg_ganglion"
    if "_rr_" in low:
        return "right_rear_leg_ganglion"
    return "thoracic_pattern_network"


def sample_node(
    rng: random.Random,
    center: Tuple[float, float, float],
    spread: float,
    layer: int,
    region_name: str,
    type_name: str,
) -> dict:
    cx, cy, cz = center
    return {
        "x": float(max(-1.0, min(1.0, cx + rng.uniform(-spread, spread)))),
        "y": float(max(-1.0, min(1.0, cy + rng.uniform(-spread, spread)))),
        "z": float(max(-1.0, min(1.0, cz + rng.uniform(-spread, spread)))),
        "layer": int(layer),
        "region_name": region_name,
        "type_name": type_name,
    }


def build_topology(
    sensory_names: Sequence[str],
    output_names: Sequence[str],
    hidden_layers: int,
    hidden_per_layer: int,
    rng: random.Random,
) -> Tuple[dict, Dict[str, int]]:
    centers = region_centers_normalized()
    hidden_region_cycle = [
        "supraesophageal_ganglion",
        "subesophageal_ganglion",
        "thoracic_pattern_network",
        "left_front_leg_ganglion",
        "left_mid_leg_ganglion",
        "left_rear_leg_ganglion",
        "right_front_leg_ganglion",
        "right_mid_leg_ganglion",
        "right_rear_leg_ganglion",
    ]
    hidden_type_cycle = ["Interneuron", "Interneuron", "Motor", "Sensory", "Neuromodulatory"]

    topo_layers: List[List[dict]] = []
    region_counts: Dict[str, int] = {}

    for l in range(hidden_layers):
        layer_nodes: List[dict] = []
        for i in range(hidden_per_layer):
            region_name = hidden_region_cycle[(i + l) % len(hidden_region_cycle)]
            type_name = hidden_type_cycle[(i + 2 * l) % len(hidden_type_cycle)]
            node = sample_node(
                rng,
                centers[region_name],
                spread=0.12 if region_name == "thoracic_pattern_network" else 0.09,
                layer=l,
                region_name=region_name,
                type_name=type_name,
            )
            layer_nodes.append(node)
            region_counts[region_name] = region_counts.get(region_name, 0) + 1
        topo_layers.append(layer_nodes)

    sensory_nodes: List[dict] = []
    for name in sensory_names:
        region_name = region_for_sensor(name)
        sensory_nodes.append(
            sample_node(
                rng,
                centers[region_name],
                spread=0.08,
                layer=0,
                region_name=region_name,
                type_name="Sensory",
            )
        )

    output_layer = max(0, hidden_layers - 1)
    output_nodes: List[dict] = []
    for name in output_names:
        region_name = region_for_output(name)
        output_nodes.append(
            sample_node(
                rng,
                centers[region_name],
                spread=0.07,
                layer=output_layer,
                region_name=region_name,
                type_name="Motor",
            )
        )

    topo = {
        "layers": topo_layers,
        "sensory_nodes": sensory_nodes,
        "output_nodes": output_nodes,
        "early_cells": [],
    }
    return topo, region_counts


def build_io_alignment_map(
    sensory_nodes: Sequence[str],
    output_nodes: Sequence[str],
    *,
    camera_retina_width: int,
    camera_retina_height: int,
) -> dict:
    return {
        "profile": "hexapod",
        "sensor_regex": HEX_SENSORS_REGEX,
        "actuator_regex": HEX_ACTUATORS_REGEX,
        "sensory_channels": [
            {
                "channel_index": i,
                "connectome_node_id": sensory_nodes[i],
                "device_port": sensory_nodes[i],
            }
            for i in range(len(sensory_nodes))
        ],
        "output_channels": [
            {
                "channel_index": i,
                "connectome_node_id": output_nodes[i],
                "actuator_name": output_nodes[i],
            }
            for i in range(len(output_nodes))
        ],
        "meta": {
            "camera_event_encoder": {
                "retina_width": int(camera_retina_width),
                "retina_height": int(camera_retina_height),
                "channels_per_camera": int(2 * camera_retina_width * camera_retina_height),
            },
            "generator": "build_hexapod_network_json.py",
            "drosophila_reference_datasets": DROSOPHILA_REFERENCE_DATASETS,
        },
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate Hexapod network/config artifacts for Webots runtime.")
    parser.add_argument("--template", default="network.json", help="Template snapshot/config JSON path")
    parser.add_argument("--output", default="network_hexapod.json", help="Output snapshot path")
    parser.add_argument(
        "--config-output",
        default="webots_world/configs/config_hexapod_webots.json",
        help="Output NetworkConfig JSON path",
    )
    parser.add_argument(
        "--hexapod-proto",
        default="webots_world/protos/HexapodRobot.proto",
        help="Path to HexapodRobot.proto for sensor/actuator extraction",
    )
    parser.add_argument(
        "--io-map-output",
        default="",
        help="Optional output path for io alignment map (defaults to <config>.io_alignment.json)",
    )
    parser.add_argument("--camera-retina-width", type=int, default=1, help="Retina columns per hexapod camera")
    parser.add_argument("--camera-retina-height", type=int, default=1, help="Retina rows per hexapod camera")
    parser.add_argument("--expected-sensory", type=int, default=None, help="Expected sensory channel count")
    parser.add_argument("--expected-output", type=int, default=18, help="Expected output channel count")
    parser.add_argument("--hidden-layers", type=int, default=6, help="Hidden layer count")
    parser.add_argument("--hidden-per-layer", type=int, default=96, help="Hidden width per layer")
    parser.add_argument("--aarnn-depth", type=int, default=4, help="Desired AARNN depth")
    parser.add_argument("--growth-headroom", type=float, default=1.8, help="Neuron budget multiplier")
    parser.add_argument("--seed", type=int, default=1337, help="RNG seed")
    return parser.parse_args()


def main() -> None:
    args = parse_args()

    if args.camera_retina_width <= 0 or args.camera_retina_height <= 0:
        raise SystemExit("camera-retina-width/camera-retina-height must be positive")
    if args.expected_output <= 0:
        raise SystemExit("expected-output must be > 0")
    if args.hidden_layers <= 0 or args.hidden_per_layer <= 0:
        raise SystemExit("hidden-layers and hidden-per-layer must be > 0")
    if args.aarnn_depth <= 0:
        raise SystemExit("aarnn-depth must be > 0")
    if args.growth_headroom < 1.0:
        raise SystemExit("growth-headroom must be >= 1.0")

    template_path = Path(args.template)
    output_path = Path(args.output)
    config_output_path = Path(args.config_output)
    proto_path = Path(args.hexapod_proto)
    io_map_path = (
        Path(args.io_map_output)
        if args.io_map_output
        else config_output_path.with_name(f"{config_output_path.stem}.io_alignment.json")
    )

    if not template_path.exists():
        raise SystemExit(f"Template JSON not found: {template_path}")
    if not proto_path.exists():
        raise SystemExit(f"Hexapod proto not found: {proto_path}")

    template_raw = json.loads(template_path.read_text(encoding="utf-8"))
    net = dict(template_raw.get("net") or {})

    parsed_sensory, parsed_output, camera_channel_counts = parse_hexapod_channels(
        proto_path,
        camera_retina_width=args.camera_retina_width,
        camera_retina_height=args.camera_retina_height,
    )

    expected_sensory = args.expected_sensory if args.expected_sensory is not None else len(parsed_sensory)
    sensory_names, sensory_adjustment = resize_channels(parsed_sensory, expected_sensory, "hex_s_pad", 2)
    output_names, output_adjustment = resize_channels(parsed_output, args.expected_output, "hex_o_pad", 3)

    sensory_count = len(sensory_names)
    output_count = len(output_names)
    hidden_layers = int(args.hidden_layers)
    hidden_per_layer = int(args.hidden_per_layer)
    hidden_total = hidden_layers * hidden_per_layer
    requested_depth = int(args.aarnn_depth)
    aarnn_depth = max(1, min(requested_depth, 5))
    sensory_target_layer, output_source_layer = aarnn_laminar_io_layers(hidden_layers)

    rng = random.Random(args.seed)

    w_in, p_in = sparse_connect(
        hidden_per_layer,
        sensory_count,
        prob=0.24,
        rng=rng,
        min_per_col=4,
        min_per_row=2,
        base_weight=0.52,
        allow_self=True,
    )

    w_fwd: List[List[List[float]]] = []
    p_fwd: List[List[List[int]]] = []
    w_bwd: List[List[List[float]]] = []
    p_bwd: List[List[List[int]]] = []
    for _ in range(hidden_layers - 1):
        wf, pf = sparse_connect(
            hidden_per_layer,
            hidden_per_layer,
            prob=0.14,
            rng=rng,
            min_per_col=3,
            min_per_row=3,
            base_weight=0.44,
            allow_self=False,
        )
        wb, pb = sparse_connect(
            hidden_per_layer,
            hidden_per_layer,
            prob=0.08,
            rng=rng,
            min_per_col=2,
            min_per_row=2,
            base_weight=0.32,
            allow_self=False,
        )
        w_fwd.append(wf)
        p_fwd.append(pf)
        w_bwd.append(wb)
        p_bwd.append(pb)

    w_rec: List[List[List[float]]] = []
    p_rec: List[List[List[int]]] = []
    for _ in range(hidden_layers):
        wr, pr = sparse_connect(
            hidden_per_layer,
            hidden_per_layer,
            prob=0.06,
            rng=rng,
            min_per_col=2,
            min_per_row=2,
            base_weight=0.28,
            allow_self=False,
        )
        w_rec.append(wr)
        p_rec.append(pr)

    w_out, p_out = sparse_connect(
        output_count,
        hidden_per_layer,
        prob=0.26,
        rng=rng,
        min_per_col=1,
        min_per_row=4,
        base_weight=0.58,
        allow_self=True,
    )

    net["num_sensory_neurons"] = sensory_count
    net["num_hidden_layers"] = hidden_layers
    net["num_hidden_per_layer_initial"] = hidden_per_layer
    net["num_output_neurons"] = output_count
    net["sensory_target_layer"] = sensory_target_layer
    net["output_source_layer"] = output_source_layer

    net["spike_io"] = {
        "input_domain": "hybrid",
        "output_domain": "hybrid",
        "profile": "hexapod",
        "input_strategy": "profile_default",
        "output_strategy": "profile_default",
    }

    net["p_in"] = 0.24
    net["p_hidden"] = 0.14
    net["p_out"] = 0.26
    net["growth_enabled"] = True
    net["morpho_growth_enabled"] = True
    net["use_morphology"] = True
    net["use_aarnn_delays"] = True
    net["aarnn_layer_depth"] = aarnn_depth
    net["max_layers"] = max(hidden_layers + 2, 6)

    net["clumping_design"] = "Hexapod"
    net["brain_regions"] = build_hexapod_regions()
    net["neuron_types"] = [
        {"name": "Pyramidal", "bio_params": {"izh_preset": "RS", "synaptic_gain": 1.0}},
        {"name": "Sensory", "bio_params": {"izh_preset": "RS", "synaptic_gain": 0.95}},
        {"name": "Interneuron", "bio_params": {"izh_preset": "FS", "synaptic_gain": 0.92}},
        {"name": "Motor", "bio_params": {"izh_preset": "CH", "synaptic_gain": 1.08}},
        {"name": "Neuromodulatory", "bio_params": {"izh_preset": "IB", "synaptic_gain": 0.88}},
    ]

    # Hexapod biomimicry defaults aligned with the runtime profile, while
    # retaining Drosophila-style event-vision and IO-alignment conventions.
    net["development_growth_interval_ms"] = 7.0
    net["development_pruning_interval_ms"] = 34.0
    net["development_io_formation_interval_ms"] = 320.0
    net["development_stage_mode"] = "auto"
    net["development_stage"] = "axon_pathfinding"
    net["development_stage_dendrite_start_ms"] = 4000.0
    net["development_stage_synaptogenesis_start_ms"] = 14000.0
    net["development_stage_refinement_start_ms"] = 60000.0
    net["development_stage_myelination_start_ms"] = 130000.0

    net["spawn_radius"] = 0.055
    net["new_edge_prob"] = 0.032
    net["proximity_degree_cap"] = 4
    net["aarnn_velocity"] = 7.4
    net["axon_velocity"] = 9.2
    net["dend_velocity"] = 4.0
    net["p_release_default"] = 0.70
    net["bouton_latency_ms"] = 0.35
    net["bouton_jitter_ms"] = 0.06

    net["aarnn_dale_strictness"] = 0.86
    net["aarnn_inhibitory_fraction"] = 0.33
    net["aarnn_gap_junction_strength"] = 0.04
    net["aarnn_gap_junction_radius"] = 0.24
    net["aarnn_gap_junction_inhibitory_only"] = False
    net["aarnn_nmda_voltage_sensitivity"] = 0.028
    net["aarnn_distance_attenuation_per_unit"] = 0.22
    net["aarnn_release_prob_heterogeneity"] = 0.11
    net["volume_transmission_enabled"] = True
    net["volume_transmission_radius"] = 0.22
    net["volume_transmission_strength"] = 0.07
    net["aarnn_triplet_ltp_gain"] = 0.16
    net["aarnn_triplet_ltd_gain"] = 0.10
    net["aarnn_synaptic_scaling_strength"] = 0.028
    net["aarnn_synaptic_scaling_target"] = 0.92

    net["aarnn_myelination_enabled"] = False
    net["aarnn_myelination_rate"] = 0.0
    net["aarnn_demyelination_rate"] = 0.0
    net["aarnn_myelin_min_conduction_gain"] = 1.0
    net["aarnn_myelin_max_conduction_gain"] = 1.0
    net["aarnn_myelin_initial"] = 0.0

    net["perceptual_loop_enabled"] = True
    net["world_model_enabled"] = False
    net["sleep_enabled"] = True
    net["sleep_cycle_ms"] = 90000.0
    net["sleep_duration_ms"] = 750.0
    net["theta_rhythm_enabled"] = True
    net["theta_rhythm_hz"] = 6.0
    net["theta_rhythm_duty"] = 0.28
    net["theta_rhythm_phase_jitter"] = 0.03
    net["thalamic_gating_enabled"] = False

    net["aarnn_import_topology_rewire_enabled"] = True
    net["aarnn_import_topology_rewire_keep_fraction"] = 0.80
    net["aarnn_import_topology_rewire_region_bias"] = 0.26

    initial_total_neurons = sensory_count + output_count + hidden_total
    requested_budget = int(math.ceil(initial_total_neurons * float(args.growth_headroom)))
    net["max_total_neurons"] = max(initial_total_neurons, requested_budget, int(net.get("max_total_neurons", 0) or 0))

    sensor_roles = {name: infer_sensor_role(name) for name in sensory_names}
    output_roles = {name: infer_output_role(name) for name in output_names}
    sensory_groups = {name: group_from_sensor(name) for name in sensory_names}
    output_groups = {name: group_from_output(name) for name in output_names}

    hidden_nodes: List[str] = []
    hidden_layer_sizes: List[int] = []
    for l in range(hidden_layers):
        hidden_layer_sizes.append(hidden_per_layer)
        hidden_nodes.extend(f"hex_h{l:02d}_{i:04d}" for i in range(hidden_per_layer))

    topo, topo_region_counts = build_topology(
        sensory_names=sensory_names,
        output_names=output_names,
        hidden_layers=hidden_layers,
        hidden_per_layer=hidden_per_layer,
        rng=rng,
    )

    label_mode = "proto_exact" if sensory_adjustment == "exact" and output_adjustment == "exact" else "proto_resized"
    snapshot = {
        "net": net,
        "t": 0,
        "t_ms": 0.0,
        "rng_seed": int(args.seed),
        "topo": topo,
        "w_in": {"rows": hidden_per_layer, "cols": sensory_count, "data": flatten_row_major(w_in)},
        "w_hh_fwd": [
            {"rows": hidden_per_layer, "cols": hidden_per_layer, "data": flatten_row_major(m)}
            for m in w_fwd
        ],
        "w_hh_bwd": [
            {"rows": hidden_per_layer, "cols": hidden_per_layer, "data": flatten_row_major(m)}
            for m in w_bwd
        ],
        "w_hh_rec": [
            {"rows": hidden_per_layer, "cols": hidden_per_layer, "data": flatten_row_major(m)}
            for m in w_rec
        ],
        "w_out": {"rows": output_count, "cols": hidden_per_layer, "data": flatten_row_major(w_out)},
        "p_in": {"rows": hidden_per_layer, "cols": sensory_count, "data": flatten_row_major_u32(p_in)},
        "p_fwd": [
            {"rows": hidden_per_layer, "cols": hidden_per_layer, "data": flatten_row_major_u32(m)}
            for m in p_fwd
        ],
        "p_bwd": [
            {"rows": hidden_per_layer, "cols": hidden_per_layer, "data": flatten_row_major_u32(m)}
            for m in p_bwd
        ],
        "p_rec": [
            {"rows": hidden_per_layer, "cols": hidden_per_layer, "data": flatten_row_major_u32(m)}
            for m in p_rec
        ],
        "p_out": {"rows": output_count, "cols": hidden_per_layer, "data": flatten_row_major_u32(p_out)},
        "layer_range": None,
        "connectome_labels": {
            "dataset": "Hexapod sensorimotor scaffold (drosophila-aligned IO)",
            "species": "freenove_big_hexapod",
            "source_file": str(proto_path),
            "source_files": {"proto": str(proto_path)},
            "sensory_nodes": sensory_names,
            "hidden_nodes": hidden_nodes,
            "hidden_layer_sizes": hidden_layer_sizes,
            "laminar_mapping": {
                "hidden_layer_count": hidden_layers,
                "sensory_target_layer": sensory_target_layer,
                "output_source_layer": output_source_layer,
            },
            "output_nodes": output_names,
            "sensor_role_map": sensor_roles,
            "output_role_map": output_roles,
            "sensory_groups": sensory_groups,
            "output_groups": output_groups,
            "label_mode": label_mode,
            "label_adjustments": {"sensory": sensory_adjustment, "output": output_adjustment},
            "expected_counts": {"sensory": expected_sensory, "output": args.expected_output},
            "parsed_counts": {"sensory": len(parsed_sensory), "output": len(parsed_output)},
            "camera_event_encoder": {
                "retina_width": int(args.camera_retina_width),
                "retina_height": int(args.camera_retina_height),
                "channels_per_camera": int(2 * args.camera_retina_width * args.camera_retina_height),
                "camera_channel_counts": camera_channel_counts,
            },
            "generator": "build_hexapod_network_json.py",
            "growth_headroom": float(args.growth_headroom),
            "aarnn_depth_requested": requested_depth,
            "aarnn_depth_applied": aarnn_depth,
            "max_total_neurons": int(net["max_total_neurons"]),
            "topology_projection": {
                "mode": "hexapod_region_biased",
                "topo_region_counts": topo_region_counts,
                "uses_role_guided_regioning": True,
            },
            "drosophila_alignment": {
                "reference_datasets": DROSOPHILA_REFERENCE_DATASETS,
                "reused_elements": [
                    "event_camera_channel_naming",
                    "io_alignment_schema",
                    "compact_sensory_baseline_target",
                ],
                "base_sensory_reference_count": 34,
            },
            "bio_profile": {
                "profile": "hexapod",
                "growth3d": True,
                "morphology": True,
                "aarnn_layer_depth": int(net["aarnn_layer_depth"]),
                "sleep": bool(net["sleep_enabled"]),
                "theta": bool(net["theta_rhythm_enabled"]),
                "rewire_on_import": bool(net["aarnn_import_topology_rewire_enabled"]),
                "rewire_keep_fraction": float(net["aarnn_import_topology_rewire_keep_fraction"]),
            },
        },
    }

    io_map = build_io_alignment_map(
        sensory_names,
        output_names,
        camera_retina_width=args.camera_retina_width,
        camera_retina_height=args.camera_retina_height,
    )

    output_path.parent.mkdir(parents=True, exist_ok=True)
    config_output_path.parent.mkdir(parents=True, exist_ok=True)
    io_map_path.parent.mkdir(parents=True, exist_ok=True)

    output_path.write_text(json.dumps(snapshot, indent=2) + "\n", encoding="utf-8")
    config_output_path.write_text(json.dumps(net, indent=2) + "\n", encoding="utf-8")
    io_map_path.write_text(json.dumps(io_map, indent=2) + "\n", encoding="utf-8")

    def count_nonzero_u32(mat: List[List[int]]) -> int:
        return sum(1 for row in mat for v in row if int(v) != 0)

    in_edges = count_nonzero_u32(p_in)
    rec_edges = sum(count_nonzero_u32(m) for m in p_rec)
    fwd_edges = sum(count_nonzero_u32(m) for m in p_fwd)
    bwd_edges = sum(count_nonzero_u32(m) for m in p_bwd)
    out_edges = count_nonzero_u32(p_out)
    print(
        f"Wrote {output_path}, {config_output_path}, {io_map_path} | "
        f"S={sensory_count} H={hidden_layers}x{hidden_per_layer} O={output_count} "
        f"depth={aarnn_depth} headroom={float(args.growth_headroom):.2f} "
        f"(edges in/fwd/bwd/rec/out={in_edges}/{fwd_edges}/{bwd_edges}/{rec_edges}/{out_edges}) "
        f"labels={label_mode}"
    )


if __name__ == "__main__":
    main()
