#!/usr/bin/env python3
"""
Build a Runner-compatible network snapshot JSON from the Seung Lab zebrafish
connectome (data/zebrafish/04152019.csv).

Dataset: ~2 M synaptic site records from larval Danio rerio whole-brain EM.
  psd_segid, BBOX_b/e x/y/z, postsyn_sz, postsyn_wt, postsyn_x/y/z,
  presyn_sz, presyn_wt, presyn_x/y/z, size, postsyn_segid, presyn_segid,
  centroid_x/y/z

Strategy (two-pass streaming — avoids loading the full 411 MB into RAM):
  Pass 1: accumulate per-neuron mean positions and in/out degrees.
  Select nodes (sensory, output, hidden) from degree + spatial region.
  Pass 2: build weight matrices from connectome edges in selected set.

Output JSON follows the Snapshot schema used by Runner::import_network_json:
  net, w_in / w_hh_fwd / w_hh_bwd / w_hh_rec / w_out,
  p_in / p_fwd / p_bwd / p_rec / p_out, topo, connectome_labels.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import sys
from collections import Counter, defaultdict
from pathlib import Path
from typing import Dict, List, Optional, Tuple

import pandas as pd


# ---------------------------------------------------------------------------
# Zebrafish Webots sensor channel names (32 channels)
# Must match DeviceMapper order in the Webots controller.
# ---------------------------------------------------------------------------
ZEBRAFISH_SENSOR_CHANNELS: List[str] = [
    # Lateral line (neuromast mechanoreceptors) — 16
    "zebrafish_s_00_lateralline_l0",
    "zebrafish_s_01_lateralline_l1",
    "zebrafish_s_02_lateralline_l2",
    "zebrafish_s_03_lateralline_l3",
    "zebrafish_s_04_lateralline_l4",
    "zebrafish_s_05_lateralline_l5",
    "zebrafish_s_06_lateralline_l6",
    "zebrafish_s_07_lateralline_l7",
    "zebrafish_s_08_lateralline_r0",
    "zebrafish_s_09_lateralline_r1",
    "zebrafish_s_10_lateralline_r2",
    "zebrafish_s_11_lateralline_r3",
    "zebrafish_s_12_lateralline_r4",
    "zebrafish_s_13_lateralline_r5",
    "zebrafish_s_14_lateralline_r6",
    "zebrafish_s_15_lateralline_r7",
    # Eyes — 4 (left/right × intensity/gradient)
    "zebrafish_s_16_eye_left_lum",
    "zebrafish_s_17_eye_left_grad",
    "zebrafish_s_18_eye_right_lum",
    "zebrafish_s_19_eye_right_grad",
    # Olfactory bulb inputs — 4 directional
    "zebrafish_s_20_olfactory_l",
    "zebrafish_s_21_olfactory_r",
    "zebrafish_s_22_olfactory_front",
    "zebrafish_s_23_olfactory_rear",
    # Flow / pressure — 4
    "zebrafish_s_24_flow_anterior",
    "zebrafish_s_25_flow_posterior",
    "zebrafish_s_26_pressure_depth",
    "zebrafish_s_27_pressure_pitch",
    # Inertial (accelerometer + gyro) — 4
    # .x/.y axis-suffix: DeviceMapper extracts one axis → 1 channel per device
    "zebrafish_s_28_accel.x",
    "zebrafish_s_29_accel.y",
    "zebrafish_s_30_gyro.x",
    "zebrafish_s_31_gyro.y",
]

# Webots motor output channel names (32 channels)
ZEBRAFISH_OUTPUT_CHANNELS: List[str] = [
    # Tail segments L/R (8 segments × 2) = 16
    "zebrafish_o_00_tail_l0",
    "zebrafish_o_01_tail_r0",
    "zebrafish_o_02_tail_l1",
    "zebrafish_o_03_tail_r1",
    "zebrafish_o_04_tail_l2",
    "zebrafish_o_05_tail_r2",
    "zebrafish_o_06_tail_l3",
    "zebrafish_o_07_tail_r3",
    "zebrafish_o_08_tail_l4",
    "zebrafish_o_09_tail_r4",
    "zebrafish_o_10_tail_l5",
    "zebrafish_o_11_tail_r5",
    "zebrafish_o_12_tail_l6",
    "zebrafish_o_13_tail_r6",
    "zebrafish_o_14_tail_l7",
    "zebrafish_o_15_tail_r7",
    # Pectoral fins (2 fins × 2 DOF) = 4
    "zebrafish_o_16_pec_fin_l_pitch",
    "zebrafish_o_17_pec_fin_l_roll",
    "zebrafish_o_18_pec_fin_r_pitch",
    "zebrafish_o_19_pec_fin_r_roll",
    # Dorsal fin = 2
    "zebrafish_o_20_dorsal_fin_l",
    "zebrafish_o_21_dorsal_fin_r",
    # Caudal fin = 2
    "zebrafish_o_22_caudal_fin_l",
    "zebrafish_o_23_caudal_fin_r",
    # Pelvic fins = 4
    "zebrafish_o_24_pelvic_fin_l",
    "zebrafish_o_25_pelvic_fin_r",
    "zebrafish_o_26_pelvic_fin_l2",
    "zebrafish_o_27_pelvic_fin_r2",
    # Jaw / operculum / trunk = 4
    "zebrafish_o_28_jaw",
    "zebrafish_o_29_operculum_l",
    "zebrafish_o_30_operculum_r",
    "zebrafish_o_31_trunk_stiffness",
]

NUM_SENSORY = len(ZEBRAFISH_SENSOR_CHANNELS)   # 32
NUM_OUTPUT  = len(ZEBRAFISH_OUTPUT_CHANNELS)   # 32

# ---------------------------------------------------------------------------
# Brain region spatial partition (normalised [0,1] coordinates).
# Assumes the EM volume is oriented A→P along the Z axis and D→V along Y.
# Derived from typical larval zebrafish neuroanatomy (Bhatt et al. 2007;
# Mueller & Wullimann 2005). Adjust ZF_AP_AXIS / ZF_DV_AXIS if the dataset
# has a different orientation.
# ---------------------------------------------------------------------------
ZF_AP_AXIS = "z"   # anterior→posterior
ZF_DV_AXIS = "y"   # dorsal→ventral  (0 = dorsal, 1 = ventral)

# Each tuple: (ap_min, ap_max, dv_min, dv_max, region_name)
REGION_SLICES: List[Tuple[float, float, float, float, str]] = [
    (0.00, 0.12, 0.00, 1.00, "olfactory_bulb"),
    (0.12, 0.25, 0.00, 0.50, "telencephalon"),
    (0.12, 0.25, 0.50, 1.00, "diencephalon"),
    (0.25, 0.45, 0.00, 0.55, "tectum_l"),
    (0.25, 0.45, 0.55, 1.00, "pretectum"),
    (0.45, 0.62, 0.00, 0.50, "tectum_r"),
    (0.45, 0.62, 0.50, 1.00, "cerebellum"),
    (0.62, 0.82, 0.00, 1.00, "hindbrain"),
    (0.82, 1.00, 0.00, 1.00, "spinal_cord"),
]

SENSORY_REGIONS  = {"olfactory_bulb", "tectum_l", "tectum_r", "pretectum"}
OUTPUT_REGIONS   = {"spinal_cord", "hindbrain"}

# Biological affinity: which Webots sensor channels connect to which region.
# Used to build the synthetic w_in sensory-projection matrix.
SENSORY_REGION_AFFINITY: Dict[str, List[int]] = {
    # Olfactory bulb → olfactory receptor channels 20-23
    "olfactory_bulb":  [20, 21, 22, 23],
    # Optic tectum → eye channels 16-19, plus ambient light from flow/pressure
    "tectum_l":        [16, 17, 26, 27],
    "tectum_r":        [18, 19, 26, 27],
    # Pretectum → visual + depth cues
    "pretectum":       [16, 17, 18, 19, 26, 27],
    # Diencephalon (hypothalamus/epithalamus) → pressure + olfactory integration
    "diencephalon":    [22, 23, 26, 27],
    # Telencephalon → no direct sensory (higher association)
    "telencephalon":   [],
    # Cerebellum → inertial sensors (proprioception / vestibular)
    "cerebellum":      [28, 29, 30, 31],
    # Hindbrain → lateral line (medial LL nucleus) + inertial + flow
    "hindbrain":       [0, 1, 2, 3, 4, 5, 6, 7,
                        8, 9, 10, 11, 12, 13, 14, 15,
                        24, 25, 28, 29, 30, 31],
    # Spinal cord → chemical / tail-tip sensors
    "spinal_cord":     [20, 21, 24, 25],
}

NEURON_TYPE_BY_REGION = {
    "olfactory_bulb":  "Sensory",
    "tectum_l":        "Sensory",
    "tectum_r":        "Sensory",
    "pretectum":       "Sensory",
    "telencephalon":   "Interneuron",
    "diencephalon":    "Interneuron",
    "cerebellum":      "Interneuron",
    "hindbrain":       "Motor",
    "spinal_cord":     "Motor",
}

# 3-D region geometry (used in topo export), scaled in worm-space units.
ZEBRAFISH_TOPO_REGIONS: List[dict] = [
    {
        "name": "olfactory_bulb",
        "shape": {"shape": "ellipsoid", "center": [0.0, 0.0, 22.0], "radii": [6.0, 5.0, 4.0]},
        "center": [0.0, 0.0, 22.0], "radii": [6.0, 5.0, 4.0],
        "type_distribution": [["Sensory", 0.85], ["Interneuron", 0.15]],
    },
    {
        "name": "telencephalon",
        "shape": {"shape": "ellipsoid", "center": [0.0, 3.0, 14.0], "radii": [8.0, 5.0, 5.0]},
        "center": [0.0, 3.0, 14.0], "radii": [8.0, 5.0, 5.0],
        "type_distribution": [["Interneuron", 1.0]],
    },
    {
        "name": "diencephalon",
        "shape": {"shape": "ellipsoid", "center": [0.0, -4.0, 14.0], "radii": [7.0, 4.0, 5.0]},
        "center": [0.0, -4.0, 14.0], "radii": [7.0, 4.0, 5.0],
        "type_distribution": [["Interneuron", 0.7], ["Sensory", 0.3]],
    },
    {
        "name": "tectum_l",
        "shape": {"shape": "ellipsoid", "center": [-9.0, 4.0, 4.0], "radii": [8.0, 6.0, 8.0]},
        "center": [-9.0, 4.0, 4.0], "radii": [8.0, 6.0, 8.0],
        "type_distribution": [["Sensory", 0.7], ["Interneuron", 0.3]],
    },
    {
        "name": "tectum_r",
        "shape": {"shape": "ellipsoid", "center": [9.0, 4.0, 4.0], "radii": [8.0, 6.0, 8.0]},
        "center": [9.0, 4.0, 4.0], "radii": [8.0, 6.0, 8.0],
        "type_distribution": [["Sensory", 0.7], ["Interneuron", 0.3]],
    },
    {
        "name": "pretectum",
        "shape": {"shape": "ellipsoid", "center": [0.0, 2.0, 0.0], "radii": [5.0, 4.0, 4.0]},
        "center": [0.0, 2.0, 0.0], "radii": [5.0, 4.0, 4.0],
        "type_distribution": [["Sensory", 0.5], ["Interneuron", 0.5]],
    },
    {
        "name": "cerebellum",
        "shape": {"shape": "ellipsoid", "center": [0.0, 5.0, -5.0], "radii": [7.0, 4.0, 5.0]},
        "center": [0.0, 5.0, -5.0], "radii": [7.0, 4.0, 5.0],
        "type_distribution": [["Interneuron", 1.0]],
    },
    {
        "name": "hindbrain",
        "shape": {
            "shape": "tube",
            "line_from": [-6.0, -2.0, -10.0],
            "line_to":   [ 6.0, -2.0, -20.0],
            "radius": 4.0,
        },
        "center": [0.0, -2.0, -15.0], "radii": [6.0, 4.0, 5.0],
        "type_distribution": [["Motor", 0.55], ["Interneuron", 0.35], ["Sensory", 0.10]],
    },
    {
        "name": "spinal_cord",
        "shape": {
            "shape": "tube",
            "line_from": [-2.0, -2.0, -22.0],
            "line_to":   [ 2.0, -2.0, -45.0],
            "radius": 2.0,
        },
        "center": [0.0, -2.0, -34.0], "radii": [2.0, 2.0, 12.0],
        "type_distribution": [["Motor", 0.80], ["Interneuron", 0.20]],
    },
]


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def stable_u01(key: str, salt: str) -> float:
    digest = hashlib.blake2b(f"{key}|{salt}".encode(), digest_size=8).digest()
    return int.from_bytes(digest, "big") / float((1 << 64) - 1)


def assign_region(ap: float, dv: float) -> str:
    for ap_min, ap_max, dv_min, dv_max, name in REGION_SLICES:
        if ap_min <= ap < ap_max and dv_min <= dv < dv_max:
            return name
    return "hindbrain"


def make_matrix(rows: int, cols: int) -> List[List[float]]:
    return [[0.0] * cols for _ in range(rows)]


def make_int_matrix(rows: int, cols: int) -> List[List[int]]:
    return [[0] * cols for _ in range(rows)]


def flatten(mat: List[List[float]]) -> List[float]:
    out: List[float] = []
    for row in mat:
        out.extend(row)
    return out


def flatten_int(mat: List[List[int]]) -> List[int]:
    out: List[int] = []
    for row in mat:
        out.extend(row)
    return out


# ---------------------------------------------------------------------------
# Pass 1: accumulate positions and degrees
# ---------------------------------------------------------------------------

def pass1_positions_degrees(
    csv_path: Path,
    chunk_size: int,
    verbose: bool,
) -> Tuple[Dict[int, List[float]], Counter, Counter]:
    """
    Stream the CSV once.  Returns:
      neuron_centroid  – {seg_id: [mean_x, mean_y, mean_z]}
      out_degree       – {seg_id: total synapses as pre}
      in_degree        – {seg_id: total synapses as post}
    """
    COLS = ["presyn_segid", "postsyn_segid",
            "presyn_x", "presyn_y", "presyn_z",
            "postsyn_x", "postsyn_y", "postsyn_z"]

    pos_sum:   Dict[int, List[float]] = defaultdict(lambda: [0.0, 0.0, 0.0, 0])
    out_deg:   Counter = Counter()
    in_deg:    Counter = Counter()
    n_rows = 0

    for chunk in pd.read_csv(csv_path, usecols=COLS, chunksize=chunk_size,
                             low_memory=False):
        valid = chunk.dropna(subset=["presyn_segid", "postsyn_segid"])
        if valid.empty:
            continue
        valid = valid.copy()
        valid["presyn_segid"]  = valid["presyn_segid"].astype(int)
        valid["postsyn_segid"] = valid["postsyn_segid"].astype(int)

        # Accumulate positions
        pre_grp = valid.groupby("presyn_segid")[["presyn_x", "presyn_y", "presyn_z"]].sum()
        for nid, row in pre_grp.iterrows():
            pos_sum[nid][0] += row["presyn_x"]
            pos_sum[nid][1] += row["presyn_y"]
            pos_sum[nid][2] += row["presyn_z"]
            pos_sum[nid][3] += valid["presyn_segid"].eq(nid).sum()

        post_grp = valid.groupby("postsyn_segid")[["postsyn_x", "postsyn_y", "postsyn_z"]].sum()
        for nid, row in post_grp.iterrows():
            pos_sum[nid][0] += row["postsyn_x"]
            pos_sum[nid][1] += row["postsyn_y"]
            pos_sum[nid][2] += row["postsyn_z"]
            pos_sum[nid][3] += valid["postsyn_segid"].eq(nid).sum()

        # Degree counts
        out_deg.update(valid["presyn_segid"].value_counts().to_dict())
        in_deg.update(valid["postsyn_segid"].value_counts().to_dict())

        n_rows += len(valid)
        if verbose:
            print(f"\r  Pass 1: {n_rows:,} rows  {len(pos_sum):,} neurons", end="", flush=True)

    if verbose:
        print(f"\r  Pass 1 done: {n_rows:,} rows  {len(pos_sum):,} neurons")

    centroid = {}
    for nid, acc in pos_sum.items():
        cnt = acc[3] or 1
        centroid[nid] = [acc[0] / cnt, acc[1] / cnt, acc[2] / cnt]

    return centroid, out_deg, in_deg


# ---------------------------------------------------------------------------
# Pass 2: build weight matrices for selected nodes
# ---------------------------------------------------------------------------

def pass2_build_matrices(
    csv_path: Path,
    chunk_size: int,
    hidden_nodes: List[int],
    output_nodes: List[int],
    verbose: bool,
) -> Tuple[
    List[List[float]], List[List[int]],   # w_hh_rec, p_hh_rec
    List[List[float]], List[List[int]],   # w_out,    p_out
]:
    hidden_idx = {nid: i for i, nid in enumerate(hidden_nodes)}
    output_idx = {nid: i for i, nid in enumerate(output_nodes)}
    all_selected = set(hidden_idx) | set(output_idx)

    h = len(hidden_nodes)
    o = len(output_nodes)

    w_rec = make_matrix(h, h)
    p_rec = make_int_matrix(h, h)
    w_out = make_matrix(o, h)
    p_out = make_int_matrix(o, h)

    COLS = ["presyn_segid", "postsyn_segid", "presyn_wt", "postsyn_wt"]
    n_rows = 0
    n_edges = 0

    for chunk in pd.read_csv(csv_path, usecols=COLS, chunksize=chunk_size,
                             low_memory=False):
        valid = chunk.dropna(subset=["presyn_segid", "postsyn_segid"])
        if valid.empty:
            continue
        valid = valid.copy()
        valid["presyn_segid"]  = valid["presyn_segid"].astype(int)
        valid["postsyn_segid"] = valid["postsyn_segid"].astype(int)

        # Filter to rows where at least the pre is in selected set
        mask_pre = valid["presyn_segid"].isin(all_selected)
        filtered = valid[mask_pre]

        for row in filtered.itertuples(index=False):
            pre  = int(row.presyn_segid)
            post = int(row.postsyn_segid)
            if post not in all_selected:
                continue
            # Combined weight: average of pre and post-synaptic weights
            pw = row.presyn_wt if not math.isnan(row.presyn_wt) else 0.5
            qw = row.postsyn_wt if not math.isnan(row.postsyn_wt) else 0.5
            w  = (pw + qw) / 2.0

            if pre in hidden_idx:
                pre_i = hidden_idx[pre]
                if post in hidden_idx:
                    post_i = hidden_idx[post]
                    w_rec[post_i][pre_i] += w
                    p_rec[post_i][pre_i] += 1
                    n_edges += 1
                elif post in output_idx:
                    post_i = output_idx[post]
                    w_out[post_i][pre_i] += w
                    p_out[post_i][pre_i] += 1
                    n_edges += 1

        n_rows += len(valid)
        if verbose:
            print(f"\r  Pass 2: {n_rows:,} rows  {n_edges:,} edges captured", end="", flush=True)

    if verbose:
        print(f"\r  Pass 2 done: {n_rows:,} rows  {n_edges:,} edges")

    return w_rec, p_rec, w_out, p_out


# ---------------------------------------------------------------------------
# Node selection
# ---------------------------------------------------------------------------

def normalise_coords(
    centroid: Dict[int, List[float]]
) -> Dict[int, Tuple[float, float, float]]:
    xs = [v[0] for v in centroid.values()]
    ys = [v[1] for v in centroid.values()]
    zs = [v[2] for v in centroid.values()]
    x0, x1 = min(xs), max(xs)
    y0, y1 = min(ys), max(ys)
    z0, z1 = min(zs), max(zs)

    def norm(v):
        return (
            (v[0] - x0) / (x1 - x0 + 1e-9),
            (v[1] - y0) / (y1 - y0 + 1e-9),
            (v[2] - z0) / (z1 - z0 + 1e-9),
        )
    return {nid: norm(v) for nid, v in centroid.items()}


def select_nodes(
    norm_coords: Dict[int, Tuple[float, float, float]],
    out_deg: Counter,
    in_deg: Counter,
    max_hidden: int,
    verbose: bool,
) -> Tuple[List[int], List[int], Dict[int, str]]:
    """
    Returns (hidden_nodes, output_nodes, region_map).

    - output_nodes: exactly NUM_OUTPUT neurons from OUTPUT_REGIONS, ranked by
      in-degree (they act as motor-command neurons driving Webots actuators).
    - hidden_nodes: up to max_hidden neurons from all other regions, ranked by
      total degree, stratified by region.
    """
    # Map each neuron to a brain region
    region_map: Dict[int, str] = {}
    for nid, (nx, ny, nz) in norm_coords.items():
        # ZF_AP_AXIS=z, ZF_DV_AXIS=y
        region_map[nid] = assign_region(nz, ny)

    # --- output nodes (spinal cord + hindbrain, top in-degree) ---
    output_candidates = [
        nid for nid in in_deg if region_map.get(nid) in OUTPUT_REGIONS
    ]
    output_candidates.sort(key=lambda n: in_deg[n], reverse=True)
    output_nodes = output_candidates[:NUM_OUTPUT]
    output_set   = set(output_nodes)

    # --- hidden nodes (everything not in output set) ---
    all_others = [nid for nid in norm_coords if nid not in output_set]
    # Sort by total degree descending (most-connected neurons first)
    all_others.sort(key=lambda n: out_deg[n] + in_deg[n], reverse=True)

    if len(all_others) <= max_hidden:
        hidden_nodes = all_others
    else:
        # Stratified subsample across regions for balanced coverage
        by_region: Dict[str, List[int]] = defaultdict(list)
        for nid in all_others:
            by_region[region_map[nid]].append(nid)
        region_counts = Counter(region_map[n] for n in all_others)
        total = len(all_others)
        hidden_nodes = []
        for region, members in by_region.items():
            quota = max(1, round(max_hidden * region_counts[region] / total))
            hidden_nodes.extend(members[:quota])
        hidden_nodes = hidden_nodes[:max_hidden]

    if verbose:
        reg_summary = Counter(region_map[n] for n in hidden_nodes)
        print(f"  Hidden {len(hidden_nodes)} | Output {len(output_nodes)}")
        for r, c in sorted(reg_summary.items()):
            print(f"    {r}: {c}")

    return hidden_nodes, output_nodes, region_map


# ---------------------------------------------------------------------------
# Sensory projection (w_in, p_in)  — synthetic, no connectome edges used
# ---------------------------------------------------------------------------

def build_sensory_projection(
    hidden_nodes: List[int],
    region_map: Dict[int, str],
    used_targets: Optional[Dict[str, List[int]]] = None,
) -> Tuple[List[List[float]], List[List[int]], Dict[str, List[int]]]:
    """
    Build w_in [hidden × sensory] and p_in [hidden × sensory].

    Neurons in sensory-afferent regions receive non-zero weights from the
    biologically appropriate sensor channels (SENSORY_REGION_AFFINITY).
    Neurons with no affinity get a sparse fallback projection so no channel
    is entirely dark.
    """
    h = len(hidden_nodes)
    s = NUM_SENSORY
    w_in = make_matrix(h, s)
    p_in = make_int_matrix(h, s)
    targets: Dict[str, List[int]] = {ch: [] for ch in ZEBRAFISH_SENSOR_CHANNELS}
    channel_covered = [False] * s

    for h_idx, nid in enumerate(hidden_nodes):
        region = region_map.get(nid, "hindbrain")
        channels = SENSORY_REGION_AFFINITY.get(region, [])
        if not channels:
            continue
        for ch_idx in channels:
            if ch_idx >= s:
                continue
            gain = 0.85 + 0.30 * stable_u01(str(nid), f"sensor_{ch_idx}")
            w_in[h_idx][ch_idx] += gain
            p_in[h_idx][ch_idx] = max(p_in[h_idx][ch_idx], 1)
            targets[ZEBRAFISH_SENSOR_CHANNELS[ch_idx]].append(nid)
            channel_covered[ch_idx] = True

    # Safety: ensure every channel has at least one connected hidden neuron
    fallback_indices = [i for i, nid in enumerate(hidden_nodes)
                        if region_map.get(nid) == "hindbrain"][:4]
    if not fallback_indices:
        fallback_indices = list(range(min(4, h)))
    for ch_idx, covered in enumerate(channel_covered):
        if not covered:
            for h_idx in fallback_indices:
                w_in[h_idx][ch_idx] += 0.60
                p_in[h_idx][ch_idx] = max(p_in[h_idx][ch_idx], 1)
                targets[ZEBRAFISH_SENSOR_CHANNELS[ch_idx]].append(hidden_nodes[h_idx])

    if used_targets is not None:
        used_targets.update(targets)

    return w_in, p_in, targets


# ---------------------------------------------------------------------------
# Topology (topo) export
# ---------------------------------------------------------------------------

def _sample_in_region(region_name: str, nid: int, norm: Tuple[float, float, float]) -> Tuple[float, float, float]:
    """Place a neuron in topo space using its actual EM coords (scaled to [-1,1])."""
    # Use scaled EM coordinates so real spatial structure is preserved in the viewer.
    nx, ny, nz = norm
    return (2.0 * nx - 1.0, 2.0 * ny - 1.0, 2.0 * nz - 1.0)


def build_zebrafish_topology(
    hidden_nodes: List[int],
    output_nodes: List[int],
    norm_coords: Dict[int, Tuple[float, float, float]],
    region_map: Dict[int, str],
) -> Tuple[dict, Dict[str, int]]:
    region_counts: Counter = Counter()

    hidden_layer = []
    for nid in hidden_nodes:
        region  = region_map.get(nid, "hindbrain")
        n_type  = NEURON_TYPE_BY_REGION.get(region, "Interneuron")
        norm    = norm_coords.get(nid, (0.5, 0.5, 0.5))
        x, y, z = _sample_in_region(region, nid, norm)
        hidden_layer.append({"x": x, "y": y, "z": z,
                              "layer": 0,
                              "region_name": region,
                              "type_name": n_type})
        region_counts[f"hidden:{region}"] += 1

    sensory_out = []
    for idx, ch in enumerate(ZEBRAFISH_SENSOR_CHANNELS):
        # Infer topology position from channel semantics
        frac_ap = 0.05 + (idx / NUM_SENSORY) * 0.20   # sensory inputs near anterior
        frac_dv = 0.5 + 0.4 * stable_u01(ch, "sens_dv")
        frac_lr = 0.5 + 0.45 * stable_u01(ch, "sens_lr") * (1 if "left" in ch or "_l" in ch else -1 if "right" in ch or "_r" in ch else 0)
        x = 2.0 * frac_lr - 1.0
        y = 2.0 * frac_dv - 1.0
        z = 2.0 * frac_ap - 1.0
        region = "hindbrain" if "lateralline" in ch else \
                 "tectum_l"  if "_left" in ch or "_l_" in ch or ch.endswith("_l0") or ch.endswith("_l1") else \
                 "tectum_r"  if "_right" in ch or "_r_" in ch or ch.endswith("_r0") else \
                 "olfactory_bulb" if "olfactory" in ch else \
                 "cerebellum"
        sensory_out.append({"x": x, "y": y, "z": z,
                             "layer": 0,
                             "region_name": region,
                             "type_name": "Sensory"})
        region_counts[f"sensory:{region}"] += 1

    output_out = []
    for idx, nid in enumerate(output_nodes):
        region  = region_map.get(nid, "spinal_cord")
        norm    = norm_coords.get(nid, (0.5, 0.5, 0.9))
        x, y, z = _sample_in_region(region, nid, norm)
        output_out.append({"x": x, "y": y, "z": z,
                            "layer": 0,
                            "region_name": region,
                            "type_name": "Motor"})
        region_counts[f"output:{region}"] += 1

    topo = {"layers": [hidden_layer],
            "sensory_nodes": sensory_out,
            "output_nodes":  output_out}
    return topo, {k: int(v) for k, v in sorted(region_counts.items())}


# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------

def main() -> None:
    parser = argparse.ArgumentParser(
        description="Generate zebrafish connectome network JSON for AARNN.")
    parser.add_argument("--data",
        default="data/zebrafish/04152019.csv",
        help="Path to 04152019.csv (default: data/zebrafish/04152019.csv)")
    parser.add_argument("--template",
        default="network.json",
        help="Baseline snapshot JSON (default: network.json)")
    parser.add_argument("--output",
        default="network_zebrafish.json",
        help="Output snapshot JSON (default: network_zebrafish.json)")
    parser.add_argument("--max-hidden", type=int, default=2000,
        help="Max hidden neurons to select (default: 2000)")
    parser.add_argument("--chunk-size", type=int, default=200_000,
        help="Pandas chunk size for streaming CSV (default: 200000)")
    parser.add_argument("--quiet", action="store_true",
        help="Suppress progress output")
    args = parser.parse_args()

    verbose = not args.quiet
    csv_path = Path(args.data)
    template_path = Path(args.template)
    output_path = Path(args.output)

    if not csv_path.exists():
        raise SystemExit(f"Zebrafish data CSV not found: {csv_path}")
    if not template_path.exists():
        raise SystemExit(f"Template JSON not found: {template_path}")

    mb = csv_path.stat().st_size // 1_000_000
    if verbose:
        print(f"Zebrafish connectome: {csv_path} ({mb} MB)")

    # ------------------------------------------------------------------
    # Pass 1
    # ------------------------------------------------------------------
    if verbose:
        print("Pass 1: scanning neuron positions and degrees …")
    centroid, out_deg, in_deg = pass1_positions_degrees(
        csv_path, args.chunk_size, verbose)

    if verbose:
        print(f"  Unique neurons: {len(centroid):,}")

    # ------------------------------------------------------------------
    # Node selection
    # ------------------------------------------------------------------
    if verbose:
        print("Normalising coordinates and selecting nodes …")
    norm_coords = normalise_coords(centroid)
    hidden_nodes, output_nodes, region_map = select_nodes(
        norm_coords, out_deg, in_deg, args.max_hidden, verbose)

    # Pad / trim to exact expected counts
    output_nodes = output_nodes[:NUM_OUTPUT]
    if len(output_nodes) < NUM_OUTPUT:
        print(f"Warning: only {len(output_nodes)} output neurons found; "
              f"expected {NUM_OUTPUT}.  Padding with hindbrain neurons.",
              file=sys.stderr)
        hindbrain_extra = [n for n in hidden_nodes
                           if region_map.get(n) in OUTPUT_REGIONS]
        needed = NUM_OUTPUT - len(output_nodes)
        output_nodes = output_nodes + hindbrain_extra[:needed]
        hidden_nodes = [n for n in hidden_nodes if n not in set(output_nodes)]

    h = len(hidden_nodes)
    o = len(output_nodes)

    if verbose:
        print(f"Final: hidden={h}  output={o}  sensory_channels={NUM_SENSORY}")

    # ------------------------------------------------------------------
    # Pass 2 — build w_hh_rec and w_out
    # ------------------------------------------------------------------
    if verbose:
        print("Pass 2: building weight matrices from connectome edges …")
    w_rec, p_rec, w_out, p_out = pass2_build_matrices(
        csv_path, args.chunk_size, hidden_nodes, output_nodes, verbose)

    # ------------------------------------------------------------------
    # Sensory projection (w_in, p_in) — synthetic
    # ------------------------------------------------------------------
    if verbose:
        print("Building sensory projection …")
    sensory_targets: Dict[str, List[int]] = {}
    w_in, p_in, sensory_targets = build_sensory_projection(
        hidden_nodes, region_map, sensory_targets)

    # ------------------------------------------------------------------
    # Topology
    # ------------------------------------------------------------------
    if verbose:
        print("Building topology …")
    topo, topo_region_counts = build_zebrafish_topology(
        hidden_nodes, output_nodes, norm_coords, region_map)

    # ------------------------------------------------------------------
    # Assemble snapshot
    # ------------------------------------------------------------------
    template = json.loads(template_path.read_text(encoding="utf-8"))
    net = dict(template.get("net", {}))

    net["num_sensory_neurons"]         = NUM_SENSORY
    net["num_hidden_layers"]           = 1
    net["num_hidden_per_layer_initial"] = h
    net["num_output_neurons"]          = o
    net["sensory_target_layer"]        = 0
    net["output_source_layer"]         = 0

    # Zebrafish vertebrate biomimicry profile: myelinated axons, theta rhythm,
    # active cerebellum-like spike-timing modulation.
    net["growth_enabled"]              = True
    net["morpho_growth_enabled"]       = True
    net["use_morphology"]              = True
    net["use_aarnn_delays"]            = True
    net["aarnn_layer_depth"]           = max(4, int(net.get("aarnn_layer_depth", 0) or 0))
    net["max_layers"]                  = 1
    net["layer_split_threshold"]       = 8192
    net["spawn_radius"]                = 0.038
    net["new_edge_prob"]               = 0.018
    net["proximity_degree_cap"]        = 4
    net["sleep_enabled"]               = True
    net["sleep_cycle_ms"]              = 120000.0    # ~2 min circadian cycle (larval)
    net["sleep_duration_ms"]           = 800.0
    net["world_model_enabled"]         = True
    net["theta_rhythm_enabled"]        = True
    net["thalamic_gating_enabled"]     = True
    net["aarnn_velocity"]              = 8.2
    net["axon_velocity"]               = 12.5        # faster, myelinated axons
    net["dend_velocity"]               = 4.8
    net["p_release_default"]           = 0.68
    net["bouton_latency_ms"]           = 0.3
    net["bouton_jitter_ms"]            = 0.04
    net["aarnn_dale_strictness"]       = 0.88
    net["aarnn_inhibitory_fraction"]   = 0.30        # ~30% inhibitory (vertebrate cortex)
    net["aarnn_gap_junction_strength"] = 0.04
    net["aarnn_gap_junction_radius"]   = 0.22
    net["aarnn_gap_junction_inhibitory_only"] = False
    net["aarnn_nmda_voltage_sensitivity"]      = 0.025
    net["aarnn_distance_attenuation_per_unit"] = 0.20
    net["aarnn_release_prob_heterogeneity"]    = 0.14
    net["volume_transmission_enabled"] = True
    net["volume_transmission_radius"]  = 0.15
    net["volume_transmission_strength"]= 0.06
    net["aarnn_triplet_ltp_gain"]      = 0.14
    net["aarnn_triplet_ltd_gain"]      = 0.09
    net["aarnn_synaptic_scaling_strength"] = 0.04
    net["aarnn_synaptic_scaling_target"]   = 0.80
    # Zebrafish axons are myelinated — faster propagation with lower jitter.
    net["aarnn_myelination_enabled"]       = True
    net["aarnn_myelination_rate"]          = 0.0004
    net["aarnn_demyelination_rate"]        = 0.00005
    net["aarnn_myelin_min_conduction_gain"]= 1.0
    net["aarnn_myelin_max_conduction_gain"]= 3.5
    net["aarnn_myelin_initial"]            = 0.40
    net["aarnn_import_topology_rewire_enabled"]          = True
    net["aarnn_import_topology_rewire_keep_fraction"]    = 0.78
    net["aarnn_import_topology_rewire_region_bias"]      = 0.35
    net["clumping_design"]               = "ZebraFish"
    net["max_total_neurons"]             = max(h + o, int(net.get("max_total_neurons", 0) or 0))
    net["brain_regions"]                 = ZEBRAFISH_TOPO_REGIONS
    net["neuron_types"] = [
        {"name": "Sensory",        "bio_params": {"izh_preset": "RS",  "synaptic_gain": 1.0}},
        {"name": "Interneuron",    "bio_params": {"izh_preset": "FS",  "synaptic_gain": 1.0}},
        {"name": "Motor",          "bio_params": {"izh_preset": "RS",  "synaptic_gain": 1.2}},
        {"name": "Neuromodulatory","bio_params": {"izh_preset": "RS",  "synaptic_gain": 1.1}},
    ]

    snapshot = {
        "net":      net,
        "topo":     topo,
        "w_in":     {"rows": h, "cols": NUM_SENSORY, "data": flatten(w_in)},
        "w_hh_fwd": [],
        "w_hh_bwd": [],
        "w_hh_rec": [{"rows": h, "cols": h, "data": flatten(w_rec)}],
        "w_out":    {"rows": o, "cols": h,  "data": flatten(w_out)},
        "p_in":     {"rows": h, "cols": NUM_SENSORY, "data": flatten_int(p_in)},
        "p_fwd":    [],
        "p_bwd":    [],
        "p_rec":    [{"rows": h, "cols": h, "data": flatten_int(p_rec)}],
        "p_out":    {"rows": o, "cols": h,  "data": flatten_int(p_out)},
        "layer_range": None,
        "connectome_labels": {
            "sensory_nodes":  ZEBRAFISH_SENSOR_CHANNELS,
            "hidden_nodes":   [str(n) for n in hidden_nodes],
            "output_nodes":   ZEBRAFISH_OUTPUT_CHANNELS,
            "sensory_projection": {ch: [str(n) for n in nids]
                                   for ch, nids in sensory_targets.items()},
            "laminar_mapping": {
                "hidden_layer_count":    1,
                "sensory_target_layer":  0,
                "output_source_layer":   0,
            },
            "selection": {
                "max_hidden":  args.max_hidden,
                "max_output":  NUM_OUTPUT,
                "max_sensory": NUM_SENSORY,
            },
            "source_file":   str(csv_path),
            "edge_count":    int(sum(p_rec[i][j]
                                    for i in range(h) for j in range(h))),
            "topology_projection": {
                "mode":                       "zebrafish_em_coordinate_partition",
                "topo_region_counts":         topo_region_counts,
                "ap_axis":                    ZF_AP_AXIS,
                "dv_axis":                    ZF_DV_AXIS,
                "uses_name_based_regioning":  False,
            },
            "bio_profile": {
                "species":              "danio_rerio",
                "dataset":             "seunglab_zebrafish_04152019",
                "growth3d":            bool(net.get("growth_enabled")),
                "morphology":          bool(net.get("use_morphology")),
                "morphological_growth":bool(net.get("morpho_growth_enabled")),
                "aarnn_layer_depth":   int(net.get("aarnn_layer_depth", 0) or 0),
                "rewire_on_import":    bool(net.get("aarnn_import_topology_rewire_enabled")),
                "rewire_keep_fraction":float(net.get("aarnn_import_topology_rewire_keep_fraction", 1.0) or 1.0),
            },
        },
    }

    output_path.write_text(json.dumps(snapshot, indent=2), encoding="utf-8")
    total_w = sum(
        sum(w_rec[i][j] for j in range(h)) for i in range(h)
    ) + sum(sum(w_out[i][j] for j in range(h)) for i in range(o))
    print(
        f"Wrote {output_path}  "
        f"| sensory={NUM_SENSORY} hidden={h} output={o} "
        f"| total_weight={total_w:.1f}"
    )


if __name__ == "__main__":
    main()
