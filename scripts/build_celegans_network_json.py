#!/usr/bin/env python3
"""
Build a Runner-compatible network snapshot JSON from celegans.py connectome code.

The generated file follows the `Snapshot` schema used by `Runner::import_network_json`:
  - net
  - w_in / w_hh_fwd / w_hh_bwd / w_hh_rec / w_out
  - p_in / p_fwd / p_bwd / p_rec / p_out

Mapping strategy:
  - Presynaptic neuron functions (uppercase names) -> hidden neurons (single hidden layer)
  - Postsynaptic targets that are never presynaptic functions -> output neurons (muscles)
  - Webots Celegans sensory channels (24) -> label-aware projections into known
    C. elegans sensory/interneuronal hidden nodes (not index-only remapping).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
from collections import Counter
from pathlib import Path
from typing import Dict, List, Tuple


DEF_RE = re.compile(r"^def\s+([A-Za-z0-9_]+)\s*\(")
EDGE_RE = re.compile(r"postsynaptic\['([^']+)'\]\s*\+=\s*([0-9]+)")

TARGET_CELEGANS_NEURONS = 302
# celegans.py encodes 300 synaptically connected neurons; CANL/CANR are part of
# the canonical 302-neuron soma set but are typically omitted in synapse-only
# reconstructions due sparse/absent synaptic entries.
CANONICAL_ISOLATED_NEURONS: List[str] = ["CANL", "CANR"]

# Webots controller sensor channel order for Celegans (must match DeviceMapper order).
CELEGANS_SENSOR_CHANNELS: List[str] = [
    "celegans_s_00_vibration_accel.x",
    "celegans_s_00_vibration_accel.y",
    "celegans_s_00_vibration_accel.z",
    "celegans_s_03_vibration_gyro.x",
    "celegans_s_03_vibration_gyro.y",
    "celegans_s_03_vibration_gyro.z",
    "celegans_s_06_touch_front",
    "celegans_s_07_touch_rear",
    "celegans_s_08_light_left",
    "celegans_s_09_light_right",
    "celegans_s_10_heat_left",
    "celegans_s_11_heat_right",
    "celegans_s_12_taste_front_center",
    "celegans_s_13_taste_front_left",
    "celegans_s_14_taste_front_right",
    "celegans_s_15_chem_left",
    "celegans_s_16_chem_right",
    "celegans_s_17_chem_rear_left",
    "celegans_s_18_chem_rear_right",
    "celegans_s_19_flow_front",
    "celegans_s_20_flow_rear",
    "celegans_s_21_far_front_left",
    "celegans_s_22_far_front_right",
    "celegans_s_23_far_rear",
]

# Label-aware sensory projection from Webots channels into known C. elegans
# sensory/integration neurons present in the connectome hidden set.
CELEGANS_SENSOR_TO_NEURONS: Dict[str, List[Tuple[str, float]]] = {
    # IMU / vibration equivalents
    "celegans_s_00_vibration_accel.x": [
        ("IL1DL", 0.9), ("OLQDL", 0.8), ("CEPDL", 0.8), ("ALML", 1.0), ("FLPL", 0.9), ("DVA", 1.0),
    ],
    "celegans_s_00_vibration_accel.y": [
        ("IL1L", 0.9), ("OLQVL", 0.8), ("CEPVL", 0.8), ("ALML", 0.9), ("PVDL", 0.9), ("DVA", 1.0),
    ],
    "celegans_s_00_vibration_accel.z": [
        ("IL1R", 0.9), ("OLQVR", 0.8), ("CEPVR", 0.8), ("ALMR", 0.9), ("PVDR", 0.9), ("DVA", 1.0),
    ],
    "celegans_s_03_vibration_gyro.x": [
        ("IL1DR", 0.8), ("OLQDR", 0.8), ("CEPDR", 0.8), ("ALMR", 0.9), ("FLPR", 0.9), ("DVA", 1.0),
    ],
    "celegans_s_03_vibration_gyro.y": [
        ("IL1VL", 0.8), ("CEPVL", 0.7), ("PVDL", 0.9), ("AQR", 0.7), ("DVA", 1.0),
    ],
    "celegans_s_03_vibration_gyro.z": [
        ("IL1VR", 0.8), ("CEPVR", 0.7), ("PVDR", 0.9), ("PQR", 0.7), ("DVA", 1.0),
    ],
    # Touch / mechanosensation
    "celegans_s_06_touch_front": [
        ("ALML", 1.3), ("ALMR", 1.3), ("AVM", 1.4), ("FLPL", 1.0), ("FLPR", 1.0), ("IL1L", 0.8), ("IL1R", 0.8),
    ],
    "celegans_s_07_touch_rear": [
        ("PLML", 1.3), ("PLMR", 1.3), ("PVM", 1.2), ("PVDL", 1.0), ("PVDR", 1.0), ("DVA", 0.9),
    ],
    # Light / heat equivalents
    "celegans_s_08_light_left": [
        ("ASJL", 1.0), ("AWCL", 0.9), ("AWBL", 0.8), ("ASIL", 0.8),
    ],
    "celegans_s_09_light_right": [
        ("ASJR", 1.0), ("AWCR", 0.9), ("AWBR", 0.8), ("ASIR", 0.8),
    ],
    "celegans_s_10_heat_left": [
        ("AFDL", 1.4), ("ASIL", 0.8), ("ASEL", 0.7),
    ],
    "celegans_s_11_heat_right": [
        ("AFDR", 1.4), ("ASIR", 0.8), ("ASER", 0.7),
    ],
    # Taste / chemical channels
    "celegans_s_12_taste_front_center": [
        ("ASEL", 1.1), ("ASER", 1.1), ("ASGL", 0.8), ("ASGR", 0.8), ("ADFL", 0.8), ("ADFR", 0.8),
    ],
    "celegans_s_13_taste_front_left": [
        ("ASEL", 1.2), ("AWAL", 1.0), ("ASKL", 0.9), ("ASGL", 0.9), ("ADLL", 0.8),
    ],
    "celegans_s_14_taste_front_right": [
        ("ASER", 1.2), ("AWAR", 1.0), ("ASKR", 0.9), ("ASGR", 0.9), ("ADLR", 0.8),
    ],
    "celegans_s_15_chem_left": [
        ("ASHL", 1.1), ("ADLL", 1.0), ("AWCL", 0.9), ("ASEL", 0.9), ("ASKL", 0.8),
    ],
    "celegans_s_16_chem_right": [
        ("ASHR", 1.1), ("ADLR", 1.0), ("AWCR", 0.9), ("ASER", 0.9), ("ASKR", 0.8),
    ],
    "celegans_s_17_chem_rear_left": [
        ("PHAL", 1.0), ("PHBL", 1.0), ("PVDL", 0.8), ("PLML", 0.9),
    ],
    "celegans_s_18_chem_rear_right": [
        ("PHAR", 1.0), ("PHBR", 1.0), ("PVDR", 0.8), ("PLMR", 0.9),
    ],
    # Flow / far-range channels (gas/flow equivalents)
    "celegans_s_19_flow_front": [
        ("URXL", 1.0), ("URXR", 1.0), ("AQR", 0.9), ("BAGL", 0.8), ("BAGR", 0.8),
    ],
    "celegans_s_20_flow_rear": [
        ("PQR", 1.0), ("PHAL", 0.8), ("PHAR", 0.8), ("BAGL", 0.7), ("BAGR", 0.7),
    ],
    "celegans_s_21_far_front_left": [
        ("URXL", 0.9), ("AWCL", 0.8), ("ASJL", 0.8), ("AQR", 0.7),
    ],
    "celegans_s_22_far_front_right": [
        ("URXR", 0.9), ("AWCR", 0.8), ("ASJR", 0.8), ("AQR", 0.7),
    ],
    "celegans_s_23_far_rear": [
        ("PQR", 1.0), ("PHBL", 0.8), ("PHBR", 0.8), ("BAGL", 0.7), ("BAGR", 0.7),
    ],
}

# Normalized region anchors used for explicit per-neuron topology (`topo`) export.
# `net["brain_regions"]` remains in canonical worm-space units from config.rs.
CELEGANS_TOPO_REGIONS: Dict[str, dict] = {
    "head_ganglia": {"shape": "ellipsoid", "center": (0.0, 0.86, 0.0), "radii": (0.34, 0.18, 0.26)},
    "nerve_ring": {"shape": "torus", "center": (0.0, 0.73, 0.0), "R": 0.24, "r": 0.06},
    "ventral_nerve_cord": {
        "shape": "tube",
        "line_from": (0.0, 0.58, -0.18),
        "line_to": (0.0, -0.95, -0.18),
        "radius": 0.050,
    },
    "dorsal_nerve_cord": {
        "shape": "tube",
        "line_from": (0.0, 0.58, 0.18),
        "line_to": (0.0, -0.95, 0.18),
        "radius": 0.045,
    },
    "tail_ganglia": {"shape": "ellipsoid", "center": (0.0, -0.98, 0.0), "radii": (0.22, 0.12, 0.18)},
}

SENSORY_PREFIXES: Tuple[str, ...] = (
    "ADF",
    "ADL",
    "AFD",
    "ALM",
    "ASE",
    "ASG",
    "ASH",
    "ASI",
    "ASJ",
    "ASK",
    "AWA",
    "AWB",
    "AWC",
    "BAG",
    "CEP",
    "FLP",
    "IL",
    "OLQ",
    "PHA",
    "PHB",
    "PHC",
    "PLM",
    "PQR",
    "PVD",
    "PVM",
    "UR",
    "AQR",
    "AVM",
)
MOTOR_PREFIXES: Tuple[str, ...] = (
    "AS",
    "DA",
    "DB",
    "DD",
    "RMD",
    "RME",
    "RMF",
    "RMH",
    "SAB",
    "SIA",
    "SIB",
    "SMB",
    "SMD",
    "VA",
    "VB",
    "VC",
    "VD",
    "DVA",
    "DVB",
    "DVC",
    "AVL",
)
NEUROMOD_PREFIXES: Tuple[str, ...] = ("ADE", "AIM", "NSM", "PDE", "RIM", "RIS")
TAIL_PREFIXES: Tuple[str, ...] = (
    "PH",
    "PLM",
    "PLN",
    "PQR",
    "PVQ",
    "PVT",
    "PVW",
    "LUA",
    "PDA",
    "PDB",
    "PDE",
)
CORD_PREFIXES: Tuple[str, ...] = (
    "AS",
    "DA",
    "DB",
    "DD",
    "DVA",
    "DVB",
    "DVC",
    "VA",
    "VB",
    "VC",
    "VD",
    "PVC",
    "PVP",
    "PVR",
    "PVN",
)


def stable_u01(node_key: str, salt: str) -> float:
    digest = hashlib.blake2b(f"{node_key}|{salt}".encode("utf-8"), digest_size=8).digest()
    return int.from_bytes(digest, "big") / float((1 << 64) - 1)


def stable_gauss(node_key: str, salt: str) -> float:
    u1 = max(1e-12, stable_u01(node_key, f"{salt}:u1"))
    u2 = stable_u01(node_key, f"{salt}:u2")
    return math.sqrt(-2.0 * math.log(u1)) * math.cos(2.0 * math.pi * u2)


def clamp(v: float, lo: float, hi: float) -> float:
    return max(lo, min(hi, v))


def pick_one(node_key: str, salt: str, values: List[float]) -> float:
    if not values:
        return 0.0
    idx = int(stable_u01(node_key, salt) * len(values)) % len(values)
    return values[idx]


def sample_clustered_ellipsoid(
    center: Tuple[float, float, float],
    radii: Tuple[float, float, float],
    cluster_center_norm: Tuple[float, float, float],
    cluster_spread_norm: Tuple[float, float, float],
    node_key: str,
    salt: str,
) -> Tuple[float, float, float]:
    nx = cluster_center_norm[0] + stable_gauss(node_key, f"{salt}:x") * cluster_spread_norm[0]
    ny = cluster_center_norm[1] + stable_gauss(node_key, f"{salt}:y") * cluster_spread_norm[1]
    nz = cluster_center_norm[2] + stable_gauss(node_key, f"{salt}:z") * cluster_spread_norm[2]
    norm_sq = nx * nx + ny * ny + nz * nz
    if norm_sq > 1.0:
        scale = 0.98 / math.sqrt(norm_sq)
        nx *= scale
        ny *= scale
        nz *= scale
    return (
        center[0] + radii[0] * nx,
        center[1] + radii[1] * ny,
        center[2] + radii[2] * nz,
    )


def sample_point_in_ellipsoid(
    center: Tuple[float, float, float],
    radii: Tuple[float, float, float],
    node_key: str,
    salt: str,
) -> Tuple[float, float, float]:
    u = stable_u01(node_key, f"{salt}:u")
    v = stable_u01(node_key, f"{salt}:v")
    w = stable_u01(node_key, f"{salt}:w")
    theta = 2.0 * math.pi * u
    cos_phi = max(-1.0, min(1.0, 2.0 * v - 1.0))
    phi = math.acos(cos_phi)
    rr = w ** (1.0 / 3.0)
    sin_phi = math.sin(phi)
    dx = rr * sin_phi * math.cos(theta)
    dy = rr * sin_phi * math.sin(theta)
    dz = rr * math.cos(phi)
    return (
        center[0] + radii[0] * dx,
        center[1] + radii[1] * dy,
        center[2] + radii[2] * dz,
    )


def sample_point_in_torus(
    center: Tuple[float, float, float],
    major_radius: float,
    minor_radius: float,
    node_key: str,
    salt: str,
    theta_override: float | None = None,
) -> Tuple[float, float, float]:
    theta = theta_override if theta_override is not None else (2.0 * math.pi * stable_u01(node_key, f"{salt}:t"))
    phi = 2.0 * math.pi * stable_u01(node_key, f"{salt}:p")
    rr = math.sqrt(stable_u01(node_key, f"{salt}:r")) * minor_radius
    ring = major_radius + rr * math.cos(phi)
    x = center[0] + ring * math.cos(theta)
    y = center[1] + rr * math.sin(phi)
    z = center[2] + ring * math.sin(theta)
    return (x, y, z)


def sample_point_in_tube(
    line_from: Tuple[float, float, float],
    line_to: Tuple[float, float, float],
    radius: float,
    node_key: str,
    salt: str,
    t_override: float | None = None,
) -> Tuple[float, float, float]:
    t = t_override if t_override is not None else stable_u01(node_key, f"{salt}:t")
    angle = 2.0 * math.pi * stable_u01(node_key, f"{salt}:a")
    rr = math.sqrt(stable_u01(node_key, f"{salt}:r")) * radius
    px = line_from[0] + (line_to[0] - line_from[0]) * t
    py = line_from[1] + (line_to[1] - line_from[1]) * t
    pz = line_from[2] + (line_to[2] - line_from[2]) * t
    return (px + rr * math.cos(angle), py, pz + rr * math.sin(angle))


def neuron_prefix(name: str) -> str:
    m = re.match(r"([A-Z]+)", name.upper())
    return m.group(1) if m else name.upper()


def parse_terminal_index(name: str) -> int | None:
    m = re.search(r"(\d+)$", name)
    if not m:
        return None
    try:
        return int(m.group(1))
    except ValueError:
        return None


def infer_side(name: str) -> str:
    n = name.upper()
    if n.endswith("L"):
        return "left"
    if n.endswith("R"):
        return "right"
    if "LEFT" in n:
        return "left"
    if "RIGHT" in n:
        return "right"
    return "midline"


def infer_dorsoventral(name: str) -> str:
    n = name.upper()
    p = neuron_prefix(n)
    if n.startswith("MD") or p in {"DA", "DB", "DD", "DVB", "DVC"}:
        return "dorsal"
    if n.startswith("MV") or p in {"VA", "VB", "VC", "VD", "AS"}:
        return "ventral"
    return "none"


def infer_hidden_type(name: str) -> str:
    if name.startswith(NEUROMOD_PREFIXES):
        return "Neuromodulatory"
    if name.startswith(MOTOR_PREFIXES):
        return "Motor"
    if name.startswith(SENSORY_PREFIXES):
        return "Sensory"
    return "Interneuron"


def infer_hidden_region(name: str, type_name: str) -> str:
    if name.startswith(TAIL_PREFIXES):
        return "tail_ganglia"
    if name.startswith(CORD_PREFIXES):
        return "dorsal_nerve_cord" if infer_dorsoventral(name) == "dorsal" else "ventral_nerve_cord"
    if type_name == "Motor":
        return "dorsal_nerve_cord" if infer_dorsoventral(name) == "dorsal" else "ventral_nerve_cord"
    if type_name == "Sensory":
        if name.startswith(("PHA", "PHB", "PHC", "PLM", "PQR")):
            return "tail_ganglia"
        return "head_ganglia"
    return "nerve_ring"


def infer_hidden_ap_fraction(name: str, region_name: str) -> float:
    jitter = stable_u01(name, "ap")
    if region_name == "head_ganglia":
        return 0.06 + 0.12 * jitter
    if region_name == "nerve_ring":
        return 0.13 + 0.05 * jitter
    if region_name == "tail_ganglia":
        return 0.86 + 0.12 * jitter

    idx = parse_terminal_index(name)
    p = neuron_prefix(name)
    if idx is not None:
        if p == "AS":
            max_idx = 11
        elif p in {"VA", "VB", "VC", "VD", "DA", "DB", "DD"}:
            max_idx = 12
        else:
            max_idx = max(12, idx)
        frac = (max(1, idx) - 1) / float(max(1, max_idx - 1))
    else:
        frac = jitter
    return 0.20 + 0.74 * frac


def infer_sensor_region(channel: str) -> str:
    c = channel.lower()
    if any(k in c for k in ("touch_rear", "chem_rear", "far_rear", "flow_rear")):
        return "tail_ganglia"
    if "vibration" in c:
        return "nerve_ring"
    return "head_ganglia"


def infer_sensor_side(channel: str) -> str:
    c = channel.lower()
    if "left" in c:
        return "left"
    if "right" in c:
        return "right"
    if "rear" in c:
        return "midline"
    return "midline"


def infer_sensor_ap_fraction(channel: str) -> float:
    c = channel.lower()
    jitter = stable_u01(channel, "sensor_ap")
    if "rear" in c:
        return 0.84 + 0.12 * jitter
    if "vibration" in c:
        return 0.12 + 0.08 * jitter
    if "flow_front" in c:
        return 0.17 + 0.05 * jitter
    return 0.07 + 0.10 * jitter


def infer_output_region(name: str) -> str:
    if name.startswith("MD"):
        return "dorsal_nerve_cord"
    return "ventral_nerve_cord"


def infer_output_side(name: str) -> str:
    if name.startswith(("MDL", "MVL")):
        return "left"
    if name.startswith(("MDR", "MVR")):
        return "right"
    return "midline"


def infer_output_ap_fraction(name: str) -> float:
    if name == "MVULVA":
        return 0.56
    idx = parse_terminal_index(name)
    if idx is None:
        return 0.55
    bounded = max(1, min(24, idx))
    return 0.22 + 0.73 * ((bounded - 1) / 23.0)


def sample_celegans_position(
    node_key: str,
    region_name: str,
    side: str,
    ap_fraction: float,
) -> Tuple[float, float, float]:
    region = CELEGANS_TOPO_REGIONS[region_name]
    side_push = -1.0 if side == "left" else 1.0 if side == "right" else 0.0

    if region["shape"] == "ellipsoid":
        if region_name == "head_ganglia":
            if side == "left":
                centers = [(-0.55, 0.20, 0.34), (-0.40, -0.18, 0.06), (-0.34, 0.02, -0.30), (-0.22, 0.24, -0.02)]
            elif side == "right":
                centers = [(0.55, 0.20, 0.34), (0.40, -0.18, 0.06), (0.34, 0.02, -0.30), (0.22, 0.24, -0.02)]
            else:
                centers = [(0.0, 0.18, 0.25), (0.0, -0.12, -0.16), (0.0, 0.02, 0.02)]
            c = centers[int(stable_u01(node_key, "head_cluster") * len(centers)) % len(centers)]
            spread = (0.16, 0.14, 0.14)
            x, y, z = sample_clustered_ellipsoid(region["center"], region["radii"], c, spread, node_key, "head")
        elif region_name == "tail_ganglia":
            if side == "left":
                centers = [(-0.42, 0.14, 0.24), (-0.28, -0.20, 0.02), (-0.20, -0.02, -0.24)]
            elif side == "right":
                centers = [(0.42, 0.14, 0.24), (0.28, -0.20, 0.02), (0.20, -0.02, -0.24)]
            else:
                centers = [(0.0, 0.14, 0.20), (0.0, -0.16, -0.08)]
            c = centers[int(stable_u01(node_key, "tail_cluster") * len(centers)) % len(centers)]
            spread = (0.18, 0.18, 0.18)
            x, y, z = sample_clustered_ellipsoid(region["center"], region["radii"], c, spread, node_key, "tail")
        else:
            x, y, z = sample_point_in_ellipsoid(region["center"], region["radii"], node_key, region_name)
        y += (0.5 - ap_fraction) * 0.06
    elif region["shape"] == "torus":
        if side == "left":
            theta_anchors = [math.pi - 0.55, math.pi - 0.10, math.pi + 0.35]
        elif side == "right":
            theta_anchors = [-0.35, 0.10, 0.55]
        else:
            theta_anchors = [0.5 * math.pi - 0.25, 0.5 * math.pi + 0.25, -0.5 * math.pi]
        theta_base = pick_one(node_key, "ring_theta_anchor", theta_anchors)
        theta = theta_base + stable_gauss(node_key, "ring_theta_jitter") * 0.14
        phi_base = pick_one(node_key, "ring_phi_anchor", [-0.7, -0.2, 0.35, 0.8])
        phi = phi_base + stable_gauss(node_key, "ring_phi_jitter") * 0.16
        rr = math.sqrt(stable_u01(node_key, "ring_rr")) * region["r"]
        ring = region["R"] + rr * math.cos(phi)
        x = region["center"][0] + ring * math.cos(theta)
        y = region["center"][1] + rr * math.sin(phi)
        z = region["center"][2] + ring * math.sin(theta)
        y += (0.5 - ap_fraction) * 0.03
    else:
        base_t = clamp((ap_fraction - 0.20) / 0.74, 0.0, 1.0)
        segment_anchors = [0.06, 0.14, 0.23, 0.32, 0.41, 0.50, 0.61, 0.72, 0.84, 0.94]
        nearest = sorted(segment_anchors, key=lambda s: abs(s - base_t))[:3]
        seg = pick_one(node_key, "tube_segment", nearest)
        t = clamp(seg + stable_gauss(node_key, "tube_t_jitter") * 0.020 + (base_t - seg) * 0.45, 0.0, 1.0)
        x, y, z = sample_point_in_tube(region["line_from"], region["line_to"], region["radius"], node_key, region_name, t_override=t)
        # Segmental dorso-ventral fasciculation.
        z += stable_gauss(node_key, "tube_dv") * 0.010

    x += 0.07 * side_push

    return (
        max(-1.35, min(1.35, x)),
        max(-1.35, min(1.35, y)),
        max(-1.35, min(1.35, z)),
    )


def build_celegans_topology(hidden_nodes: List[str], output_nodes: List[str]) -> Tuple[dict, Dict[str, int]]:
    region_counts: Counter[str] = Counter()

    hidden_layer: List[dict] = []
    for node_id in hidden_nodes:
        type_name = infer_hidden_type(node_id)
        region_name = infer_hidden_region(node_id, type_name)
        side = infer_side(node_id)
        ap_fraction = infer_hidden_ap_fraction(node_id, region_name)
        x, y, z = sample_celegans_position(node_id, region_name, side, ap_fraction)
        hidden_layer.append(
            {
                "x": x,
                "y": y,
                "z": z,
                "layer": 0,
                "region_name": region_name,
                "type_name": type_name,
            }
        )
        region_counts[f"hidden:{region_name}"] += 1

    sensory_out: List[dict] = []
    for channel in CELEGANS_SENSOR_CHANNELS:
        region_name = infer_sensor_region(channel)
        side = infer_sensor_side(channel)
        ap_fraction = infer_sensor_ap_fraction(channel)
        x, y, z = sample_celegans_position(channel, region_name, side, ap_fraction)
        sensory_out.append(
            {
                "x": x,
                "y": y,
                "z": z,
                "layer": 0,
                "region_name": region_name,
                "type_name": "Sensory",
            }
        )
        region_counts[f"sensory:{region_name}"] += 1

    output_out: List[dict] = []
    for node_id in output_nodes:
        region_name = infer_output_region(node_id)
        side = infer_output_side(node_id)
        ap_fraction = infer_output_ap_fraction(node_id)
        x, y, z = sample_celegans_position(node_id, region_name, side, ap_fraction)
        output_out.append(
            {
                "x": x,
                "y": y,
                "z": z,
                "layer": 0,
                "region_name": region_name,
                "type_name": "Motor",
            }
        )
        region_counts[f"output:{region_name}"] += 1

    topo = {"layers": [hidden_layer], "sensory_nodes": sensory_out, "output_nodes": output_out}
    return topo, {k: int(v) for k, v in sorted(region_counts.items())}


def is_connectome_neuron_fn(name: str) -> bool:
    # Keep original neuron naming convention (uppercase + digits), reject helpers.
    return name.upper() == name and not re.search(r"[a-z_]", name)


def parse_connectome_edges(celegans_path: Path) -> Tuple[List[str], List[str], Counter, List[str]]:
    lines = celegans_path.read_text(encoding="utf-8").splitlines()

    defs: List[Tuple[str, int]] = []
    for idx, line in enumerate(lines):
        m = DEF_RE.match(line)
        if m:
            defs.append((m.group(1), idx))

    defs = sorted(defs, key=lambda x: x[1])
    blocks: Dict[str, List[str]] = {}
    for i, (name, start) in enumerate(defs):
        end = defs[i + 1][1] if i + 1 < len(defs) else len(lines)
        blocks[name] = lines[start:end]

    hidden_nodes = sorted([name for name, _ in defs if is_connectome_neuron_fn(name)])
    added_hidden_nodes: List[str] = []
    for neuron in CANONICAL_ISOLATED_NEURONS:
        if neuron not in hidden_nodes:
            hidden_nodes.append(neuron)
            added_hidden_nodes.append(neuron)
    hidden_nodes = sorted(hidden_nodes)
    if len(hidden_nodes) != TARGET_CELEGANS_NEURONS:
        raise SystemExit(
            f"Expected {TARGET_CELEGANS_NEURONS} hidden neurons after canonical completion; got {len(hidden_nodes)}."
        )

    edges: Counter = Counter()
    post_nodes = set()
    for src in hidden_nodes:
        for line in blocks.get(src, []):
            m = EDGE_RE.search(line)
            if not m:
                continue
            dst = m.group(1)
            w = int(m.group(2))
            edges[(src, dst)] += w
            post_nodes.add(dst)

    output_nodes = sorted(post_nodes.difference(hidden_nodes))
    return hidden_nodes, output_nodes, edges, added_hidden_nodes


def build_nematode_regions() -> List[dict]:
    # Mirrors apply_nematode_worm_design() in src/config.rs.
    return [
        {
            "name": "head_ganglia",
            "shape": {
                "shape": "ellipsoid",
                "center": [0.0, 7.0, 0.0],
                "radii": [6.0, 8.0, 6.0],
            },
            "center": [0.0, 7.0, 0.0],
            "radii": [6.0, 8.0, 6.0],
            "type_distribution": [
                ["Sensory", 0.55],
                ["Interneuron", 0.30],
                ["Neuromodulatory", 0.10],
                ["Motor", 0.05],
            ],
        },
        {
            "name": "nerve_ring",
            "shape": {
                "shape": "torus",
                "center": [0.0, 10.0, 0.0],
                "R": 6.0,
                "r": 1.5,
                "plane": "x-z",
            },
            "center": [0.0, 10.0, 0.0],
            "radii": [7.5, 1.5, 7.5],
            "type_distribution": [
                ["Sensory", 0.15],
                ["Interneuron", 0.75],
                ["Neuromodulatory", 0.10],
            ],
        },
        {
            "name": "ventral_nerve_cord",
            "shape": {
                "shape": "tube",
                "line_from": [0.0, 15.0, -6.0],
                "line_to": [0.0, 95.0, -6.0],
                "radius": 1.2,
            },
            "center": [0.0, 55.0, -6.0],
            "radii": [1.2, 40.0, 1.2],
            "type_distribution": [
                ["Sensory", 0.10],
                ["Interneuron", 0.25],
                ["Motor", 0.60],
                ["Neuromodulatory", 0.05],
            ],
        },
        {
            "name": "dorsal_nerve_cord",
            "shape": {
                "shape": "tube",
                "line_from": [0.0, 15.0, 6.0],
                "line_to": [0.0, 95.0, 6.0],
                "radius": 1.0,
            },
            "center": [0.0, 55.0, 6.0],
            "radii": [1.0, 40.0, 1.0],
            "type_distribution": [
                ["Motor", 0.95],
                ["Interneuron", 0.05],
            ],
        },
        {
            "name": "tail_ganglia",
            "shape": {
                "shape": "ellipsoid",
                "center": [0.0, 95.0, 0.0],
                "radii": [5.0, 6.0, 5.0],
            },
            "center": [0.0, 95.0, 0.0],
            "radii": [5.0, 6.0, 5.0],
            "type_distribution": [
                ["Sensory", 0.60],
                ["Interneuron", 0.20],
                ["Motor", 0.15],
                ["Neuromodulatory", 0.05],
            ],
        },
    ]


def make_matrix(rows: int, cols: int) -> List[List[float]]:
    return [[0.0 for _ in range(cols)] for _ in range(rows)]


def flatten_row_major(mat: List[List[float]]) -> List[float]:
    out: List[float] = []
    for row in mat:
        out.extend(row)
    return out


def flatten_row_major_u32(mat: List[List[int]]) -> List[int]:
    out: List[int] = []
    for row in mat:
        out.extend(row)
    return out


def aarnn_laminar_io_layers(hidden_layer_count: int) -> Tuple[int, int]:
    """Map hidden-layer count to canonical AARNN laminar IO layers."""
    if hidden_layer_count <= 0:
        return 0, 0
    sensory_target_layer = 1 if hidden_layer_count > 1 else 0  # L4
    if hidden_layer_count > 4:
        output_source_layer = 4  # L5
    else:
        output_source_layer = hidden_layer_count - 1
    return sensory_target_layer, output_source_layer


def build_sensory_projection(
    hidden_nodes: List[str],
) -> Tuple[List[List[float]], List[List[int]], Dict[str, List[str]]]:
    hidden_index = {name: i for i, name in enumerate(hidden_nodes)}
    h_count = len(hidden_nodes)
    s_count = len(CELEGANS_SENSOR_CHANNELS)
    w_in = make_matrix(h_count, s_count)
    p_in = [[0 for _ in range(s_count)] for _ in range(h_count)]
    used_targets: Dict[str, List[str]] = {}

    for s_idx, s_name in enumerate(CELEGANS_SENSOR_CHANNELS):
        specs = CELEGANS_SENSOR_TO_NEURONS.get(s_name, [])
        picked: List[str] = []
        for n_name, gain in specs:
            h_idx = hidden_index.get(n_name)
            if h_idx is None:
                continue
            w_in[h_idx][s_idx] += float(gain)
            p_in[h_idx][s_idx] = max(p_in[h_idx][s_idx], 1)
            picked.append(n_name)

        # Safety fallback: if a channel lost all named targets, connect to a
        # compact sensory integration set rather than dropping the channel.
        if not picked:
            for fallback_name in ("ASEL", "ASER", "AIAL", "AIAR"):
                h_idx = hidden_index.get(fallback_name)
                if h_idx is None:
                    continue
                w_in[h_idx][s_idx] += 0.75
                p_in[h_idx][s_idx] = max(p_in[h_idx][s_idx], 1)
                picked.append(fallback_name)

        used_targets[s_name] = picked

    return w_in, p_in, used_targets


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate C. elegans connectome network JSON.")
    parser.add_argument(
        "--connectome",
        default="celegans.py",
        help="Path to celegans.py connectome source (default: celegans.py)",
    )
    parser.add_argument(
        "--template",
        default="network.json",
        help="Template snapshot JSON used for baseline net defaults (default: network.json)",
    )
    parser.add_argument(
        "--output",
        default="network_celegans.json",
        help="Output snapshot JSON path (default: network_celegans.json)",
    )
    args = parser.parse_args()

    connectome_path = Path(args.connectome)
    template_path = Path(args.template)
    output_path = Path(args.output)

    if not connectome_path.exists():
        raise SystemExit(f"Missing connectome source: {connectome_path}")
    if not template_path.exists():
        raise SystemExit(f"Missing template JSON: {template_path}")

    hidden_nodes, output_nodes, edges, added_hidden_nodes = parse_connectome_edges(connectome_path)
    if not hidden_nodes:
        raise SystemExit("No connectome neuron functions detected.")

    hidden_index = {name: i for i, name in enumerate(hidden_nodes)}
    output_index = {name: i for i, name in enumerate(output_nodes)}

    hidden_count = len(hidden_nodes)
    output_count = len(output_nodes)
    sensory_count = len(CELEGANS_SENSOR_CHANNELS)

    if output_count != 96:
        raise SystemExit(f"Expected 96 muscle outputs in connectome projection, got {output_count}.")

    w_in, p_in, sensory_target_map = build_sensory_projection(hidden_nodes)
    topo, topo_region_counts = build_celegans_topology(hidden_nodes, output_nodes)
    w_rec = make_matrix(hidden_count, hidden_count)
    p_rec = [[0 for _ in range(hidden_count)] for _ in range(hidden_count)]
    w_out = make_matrix(output_count, hidden_count)
    p_out = [[0 for _ in range(hidden_count)] for _ in range(output_count)]

    for (src, dst), weight in edges.items():
        s_idx = hidden_index[src]
        if dst in hidden_index:
            d_idx = hidden_index[dst]
            w_rec[d_idx][s_idx] += float(weight)
            p_rec[d_idx][s_idx] += int(weight)
        elif dst in output_index:
            d_idx = output_index[dst]
            w_out[d_idx][s_idx] += float(weight)
            p_out[d_idx][s_idx] += int(weight)
        # else: ignore unknown targets (none expected)

    template = json.loads(template_path.read_text(encoding="utf-8"))
    net = dict(template.get("net", {}))

    # Ensure a fully specified, loadable net config with low-latency defaults for
    # embodied Webots control.
    net["num_sensory_neurons"] = sensory_count
    net["num_hidden_layers"] = 1
    net["num_hidden_per_layer_initial"] = hidden_count
    net["num_output_neurons"] = output_count
    sensory_target_layer, output_source_layer = aarnn_laminar_io_layers(net["num_hidden_layers"])
    net["sensory_target_layer"] = sensory_target_layer
    net["output_source_layer"] = output_source_layer
    # C. elegans biomimicry profile: fixed-layer connectome with active morphology,
    # local electrotonic effects, and restrained developmental dynamics.
    net["growth_enabled"] = True
    net["morpho_growth_enabled"] = True
    net["use_morphology"] = True
    net["use_aarnn_delays"] = True
    net["aarnn_layer_depth"] = max(3, int(net.get("aarnn_layer_depth", 0) or 0))
    net["max_layers"] = 1
    net["layer_split_threshold"] = 4096
    net["spawn_radius"] = 0.045
    net["new_edge_prob"] = 0.025
    net["proximity_degree_cap"] = 3
    net["sleep_enabled"] = True
    net["sleep_cycle_ms"] = 180000.0
    net["sleep_duration_ms"] = 1200.0
    net["world_model_enabled"] = False
    net["theta_rhythm_enabled"] = False
    net["thalamic_gating_enabled"] = False
    net["aarnn_velocity"] = 5.5
    net["axon_velocity"] = 6.8
    net["dend_velocity"] = 3.6
    net["p_release_default"] = 0.72
    net["bouton_latency_ms"] = 0.4
    net["bouton_jitter_ms"] = 0.05
    net["aarnn_dale_strictness"] = 0.90
    net["aarnn_inhibitory_fraction"] = 0.36
    net["aarnn_gap_junction_strength"] = 0.06
    net["aarnn_gap_junction_radius"] = 0.28
    net["aarnn_gap_junction_inhibitory_only"] = False
    net["aarnn_nmda_voltage_sensitivity"] = 0.02
    net["aarnn_distance_attenuation_per_unit"] = 0.26
    net["aarnn_release_prob_heterogeneity"] = 0.12
    net["volume_transmission_enabled"] = True
    net["volume_transmission_radius"] = 0.18
    net["volume_transmission_strength"] = 0.08
    net["aarnn_triplet_ltp_gain"] = 0.12
    net["aarnn_triplet_ltd_gain"] = 0.08
    net["aarnn_synaptic_scaling_strength"] = 0.03
    net["aarnn_synaptic_scaling_target"] = 0.85
    net["aarnn_myelination_enabled"] = False
    net["aarnn_myelination_rate"] = 0.0
    net["aarnn_demyelination_rate"] = 0.0
    net["aarnn_myelin_min_conduction_gain"] = 1.0
    net["aarnn_myelin_max_conduction_gain"] = 1.0
    net["aarnn_myelin_initial"] = 0.0
    net["aarnn_import_topology_rewire_enabled"] = True
    net["aarnn_import_topology_rewire_keep_fraction"] = 0.74
    net["aarnn_import_topology_rewire_region_bias"] = 0.30
    net["clumping_design"] = "NematodeWorm"
    # Keep connectome size unconstrained so imported outputs are not capped out by preset limits.
    net["max_total_neurons"] = max(hidden_count + output_count, int(net.get("max_total_neurons", 0) or 0))
    net["brain_regions"] = build_nematode_regions()
    net["neuron_types"] = [
        {"name": "Sensory", "bio_params": {"izh_preset": "RS", "synaptic_gain": 1.0}},
        {"name": "Interneuron", "bio_params": {"izh_preset": "FS", "synaptic_gain": 1.0}},
        {"name": "Motor", "bio_params": {"izh_preset": "RS", "synaptic_gain": 1.2}},
        {"name": "Neuromodulatory", "bio_params": {"izh_preset": "RS", "synaptic_gain": 1.1}},
    ]

    snapshot = {
        "net": net,
        "topo": topo,
        "w_in": {"rows": hidden_count, "cols": sensory_count, "data": flatten_row_major(w_in)},
        "w_hh_fwd": [],
        "w_hh_bwd": [],
        "w_hh_rec": [
            {"rows": hidden_count, "cols": hidden_count, "data": flatten_row_major(w_rec)}
        ],
        "w_out": {"rows": output_count, "cols": hidden_count, "data": flatten_row_major(w_out)},
        "p_in": {"rows": hidden_count, "cols": sensory_count, "data": flatten_row_major_u32(p_in)},
        "p_fwd": [],
        "p_bwd": [],
        "p_rec": [
            {"rows": hidden_count, "cols": hidden_count, "data": flatten_row_major_u32(p_rec)}
        ],
        "p_out": {"rows": output_count, "cols": hidden_count, "data": flatten_row_major_u32(p_out)},
        "layer_range": None,
        # Extra metadata is ignored by loader but useful for post-load interpretation.
        "connectome_labels": {
            "sensory_nodes": CELEGANS_SENSOR_CHANNELS,
            "hidden_nodes": hidden_nodes,
            "output_nodes": output_nodes,
            "sensory_projection": sensory_target_map,
            "laminar_mapping": {
                "hidden_layer_count": int(net.get("num_hidden_layers", 1)),
                "sensory_target_layer": sensory_target_layer,
                "output_source_layer": output_source_layer,
                "sensory_layer_name": "L4" if sensory_target_layer == 1 else "fallback",
                "output_layer_name": "L5" if output_source_layer == 4 else "fallback",
            },
            "source_file": str(connectome_path),
            "edge_count": len(edges),
            "total_weight": int(sum(edges.values())),
            "isolated_hidden_nodes_added": added_hidden_nodes,
            "topology_projection": {
                "mode": "biomimetic_nematode_heuristic",
                "topo_region_counts": topo_region_counts,
                "uses_name_based_regioning": True,
            },
            "bio_profile": {
                "species": "caenorhabditis_elegans",
                "dataset": "openworm_celegans_connectome",
                "growth3d": bool(net.get("growth_enabled", False)),
                "morphology": bool(net.get("use_morphology", False)),
                "morphological_growth": bool(net.get("morpho_growth_enabled", False)),
                "aarnn_layer_depth": int(net.get("aarnn_layer_depth", 0) or 0),
                "rewire_on_import": bool(net.get("aarnn_import_topology_rewire_enabled", False)),
                "rewire_keep_fraction": float(net.get("aarnn_import_topology_rewire_keep_fraction", 1.0) or 1.0),
            },
        },
    }

    output_path.write_text(json.dumps(snapshot, indent=2), encoding="utf-8")

    print(
        f"Wrote {output_path} | sensory={sensory_count} hidden={hidden_count} output={output_count} "
        f"edges={len(edges)} total_weight={sum(edges.values())} "
        f"isolated_added={len(added_hidden_nodes)}"
    )


if __name__ == "__main__":
    main()
