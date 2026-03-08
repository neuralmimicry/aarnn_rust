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
  - No sensory neurons (num_sensory_neurons = 0)
"""

from __future__ import annotations

import argparse
import json
import re
from collections import Counter
from pathlib import Path
from typing import Dict, List, Tuple


DEF_RE = re.compile(r"^def\s+([A-Za-z0-9_]+)\s*\(")
EDGE_RE = re.compile(r"postsynaptic\['([^']+)'\]\s*\+=\s*([0-9]+)")


def is_connectome_neuron_fn(name: str) -> bool:
    # Keep original neuron naming convention (uppercase + digits), reject helpers.
    return name.upper() == name and not re.search(r"[a-z_]", name)


def parse_connectome_edges(celegans_path: Path) -> Tuple[List[str], List[str], Counter]:
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
    return hidden_nodes, output_nodes, edges


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

    hidden_nodes, output_nodes, edges = parse_connectome_edges(connectome_path)
    if not hidden_nodes:
        raise SystemExit("No connectome neuron functions detected.")

    hidden_index = {name: i for i, name in enumerate(hidden_nodes)}
    output_index = {name: i for i, name in enumerate(output_nodes)}

    hidden_count = len(hidden_nodes)
    output_count = len(output_nodes)

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

    # Ensure a fully specified, loadable net config while preserving current defaults.
    net["num_sensory_neurons"] = 0
    net["num_hidden_layers"] = 1
    net["num_hidden_per_layer_initial"] = hidden_count
    net["num_output_neurons"] = output_count
    net["sensory_target_layer"] = 0
    net["output_source_layer"] = 0
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
        "w_in": {"rows": hidden_count, "cols": 0, "data": []},
        "w_hh_fwd": [],
        "w_hh_bwd": [],
        "w_hh_rec": [
            {"rows": hidden_count, "cols": hidden_count, "data": flatten_row_major(w_rec)}
        ],
        "w_out": {"rows": output_count, "cols": hidden_count, "data": flatten_row_major(w_out)},
        "p_in": {"rows": hidden_count, "cols": 0, "data": []},
        "p_fwd": [],
        "p_bwd": [],
        "p_rec": [
            {"rows": hidden_count, "cols": hidden_count, "data": flatten_row_major_u32(p_rec)}
        ],
        "p_out": {"rows": output_count, "cols": hidden_count, "data": flatten_row_major_u32(p_out)},
        "layer_range": None,
        # Extra metadata is ignored by loader but useful for post-load interpretation.
        "connectome_labels": {
            "hidden_nodes": hidden_nodes,
            "output_nodes": output_nodes,
            "source_file": str(connectome_path),
            "edge_count": len(edges),
            "total_weight": int(sum(edges.values())),
        },
    }

    output_path.write_text(json.dumps(snapshot, indent=2), encoding="utf-8")

    print(
        f"Wrote {output_path} | hidden={hidden_count} output={output_count} "
        f"edges={len(edges)} total_weight={sum(edges.values())}"
    )


if __name__ == "__main__":
    main()
