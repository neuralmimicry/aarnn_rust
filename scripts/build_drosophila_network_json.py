#!/usr/bin/env python3
"""
Build a Runner-compatible network snapshot JSON from Drosophila BANC connectome CSVs.

The raw BANC graph is very large (~100k+ neurons), while the current snapshot schema
expects dense matrices. To keep imports practical, this script builds a structured
projection with configurable caps:
  - sensory nodes (input layer)
  - hidden nodes (single recurrent hidden layer)
  - motor nodes (output layer)

Input files expected (default paths):
  - data/drosophila/BANC v626/neurons.csv.gz
  - data/drosophila/BANC v626/connections_princeton.csv.gz

Output matches Runner::import_network_json schema:
  - net
  - w_in / w_hh_fwd / w_hh_bwd / w_hh_rec / w_out
  - p_in / p_fwd / p_bwd / p_rec / p_out
"""

from __future__ import annotations

import argparse
import csv
import gzip
import json
import math
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, Iterable, Iterator, List, TextIO


SENSORY_HINTS = (
    "olfactory",
    "gustatory",
    "visual",
    "auditory",
    "proprioception",
    "tactile",
    "vibro",
    "nociception",
    "sensory",
)

MOTOR_HINTS = (
    "motor",
    "wing_steering",
    "leg_motor",
    "efferent",
)


@dataclass(frozen=True)
class NodeMeta:
    root_id: str
    super_class: str
    class_name: str
    flow: str
    function: str
    community_labels: str
    body_part: str
    primary_cell_type: str

    @property
    def full_text(self) -> str:
        return " ".join(
            (
                self.super_class,
                self.class_name,
                self.flow,
                self.function,
                self.community_labels,
                self.body_part,
                self.primary_cell_type,
            )
        )


def open_csv_maybe_gzip(path: Path) -> TextIO:
    if path.suffix == ".gz":
        return gzip.open(path, "rt", encoding="utf-8", newline="")
    return path.open("r", encoding="utf-8", newline="")


def parse_syn_count(raw: str) -> int:
    raw = (raw or "").strip()
    if not raw:
        return 0
    try:
        return int(float(raw))
    except ValueError:
        return 0


def has_any_hint(text: str, hints: Iterable[str]) -> bool:
    return any(h in text for h in hints)


def is_sensory(meta: NodeMeta | None) -> bool:
    if meta is None:
        return False
    if "sensory" in meta.super_class:
        return True
    if "afferent" in meta.flow:
        return True
    if "sensory neuron" in meta.community_labels:
        return True
    return has_any_hint(meta.full_text, SENSORY_HINTS)


def is_motor(meta: NodeMeta | None) -> bool:
    if meta is None:
        return False
    if "motor" in meta.super_class:
        return True
    if "efferent" in meta.flow:
        return True
    if "motor" in meta.class_name:
        return True
    return has_any_hint(meta.full_text, MOTOR_HINTS)


def load_neuron_metadata(neurons_path: Path) -> Dict[str, NodeMeta]:
    meta: Dict[str, NodeMeta] = {}
    with open_csv_maybe_gzip(neurons_path) as f:
        reader = csv.DictReader(f)
        required = {"Root ID", "Super Class", "Class", "Flow", "Function"}
        missing = [name for name in required if name not in reader.fieldnames]
        if missing:
            raise SystemExit(f"neurons CSV missing required columns: {missing}")
        for row in reader:
            root_id = (row.get("Root ID") or "").strip()
            if not root_id:
                continue
            meta[root_id] = NodeMeta(
                root_id=root_id,
                super_class=(row.get("Super Class") or "").strip().lower(),
                class_name=(row.get("Class") or "").strip().lower(),
                flow=(row.get("Flow") or "").strip().lower(),
                function=(row.get("Function") or "").strip().lower(),
                community_labels=(row.get("Community labels") or "").strip().lower(),
                body_part=(row.get("Body Part") or "").strip().lower(),
                primary_cell_type=(row.get("Primary Cell Type") or "").strip().lower(),
            )
    return meta


def connection_rows(connections_path: Path) -> Iterator[tuple[str, str, int]]:
    with open_csv_maybe_gzip(connections_path) as f:
        reader = csv.DictReader(f)
        required = {"pre_root_id", "post_root_id", "syn_count"}
        missing = [name for name in required if name not in reader.fieldnames]
        if missing:
            raise SystemExit(f"connections CSV missing required columns: {missing}")
        for row in reader:
            pre = (row.get("pre_root_id") or "").strip()
            post = (row.get("post_root_id") or "").strip()
            if not pre or not post:
                continue
            syn = parse_syn_count(row.get("syn_count") or "")
            if syn <= 0:
                continue
            yield pre, post, syn


def select_top(
    candidates: Iterable[str],
    limit: int,
    primary_score: Dict[str, float],
    secondary_score: Dict[str, float],
) -> List[str]:
    cand_list = list(candidates)
    cand_list.sort(
        key=lambda node: (
            -float(primary_score.get(node, 0.0)),
            -float(secondary_score.get(node, 0.0)),
            node,
        )
    )
    return cand_list[:limit]


def build_fruitfly_regions() -> List[dict]:
    # Mirrors the fruit fly clumping intent in config, with explicit named regions
    # so topology-aware tooling has stable anatomical anchors.
    return [
        {
            "name": "optic_lobes",
            "shape": {"shape": "ellipsoid", "center": [0.0, 18.0, 0.0], "radii": [18.0, 14.0, 24.0]},
            "center": [0.0, 18.0, 0.0],
            "radii": [18.0, 14.0, 24.0],
            "type_distribution": [
                ["Sensory", 0.55],
                ["Interneuron", 0.35],
                ["Neuromodulatory", 0.10],
            ],
        },
        {
            "name": "central_brain",
            "shape": {"shape": "ellipsoid", "center": [0.0, 0.0, 0.0], "radii": [22.0, 18.0, 20.0]},
            "center": [0.0, 0.0, 0.0],
            "radii": [22.0, 18.0, 20.0],
            "type_distribution": [
                ["Interneuron", 0.70],
                ["Sensory", 0.15],
                ["Motor", 0.10],
                ["Neuromodulatory", 0.05],
            ],
        },
        {
            "name": "gnathal_ganglion",
            "shape": {"shape": "ellipsoid", "center": [0.0, -20.0, 0.0], "radii": [16.0, 10.0, 14.0]},
            "center": [0.0, -20.0, 0.0],
            "radii": [16.0, 10.0, 14.0],
            "type_distribution": [
                ["Interneuron", 0.45],
                ["Motor", 0.35],
                ["Sensory", 0.15],
                ["Neuromodulatory", 0.05],
            ],
        },
        {
            "name": "ventral_nerve_cord",
            "shape": {"shape": "tube", "line_from": [0.0, -26.0, -6.0], "line_to": [0.0, -95.0, -6.0], "radius": 6.0},
            "center": [0.0, -60.0, -6.0],
            "radii": [6.0, 34.5, 6.0],
            "type_distribution": [
                ["Motor", 0.55],
                ["Interneuron", 0.35],
                ["Sensory", 0.10],
            ],
        },
    ]


def build_projection_snapshot(
    neurons_path: Path,
    connections_path: Path,
    template_path: Path,
    max_sensory: int,
    max_hidden: int,
    max_output: int,
    min_syn_count: int,
    weight_transform: str,
) -> dict:
    metadata = load_neuron_metadata(neurons_path)

    print("Pass 1/3: scanning connectome degrees...")
    in_syn: Counter[str] = Counter()
    out_syn: Counter[str] = Counter()
    in_edges: Counter[str] = Counter()
    out_edges: Counter[str] = Counter()
    rows = 0
    syn_total = 0
    for pre, post, syn in connection_rows(connections_path):
        rows += 1
        syn_total += syn
        out_syn[pre] += syn
        in_syn[post] += syn
        out_edges[pre] += 1
        in_edges[post] += 1
    graph_nodes = set(in_syn) | set(out_syn)

    if not graph_nodes:
        raise SystemExit("No usable edges found in connectome.")

    sensory_candidates = [n for n in graph_nodes if is_sensory(metadata.get(n)) and out_syn[n] > 0]
    sensory_primary = {n: float(out_syn[n]) + 0.25 * float(in_syn[n]) for n in sensory_candidates}
    sensory_secondary = {n: float(out_edges[n]) for n in sensory_candidates}
    sensory_nodes = select_top(sensory_candidates, max_sensory, sensory_primary, sensory_secondary)

    # Backfill sensory nodes if the metadata-classified set is small.
    if len(sensory_nodes) < max_sensory:
        remaining = max_sensory - len(sensory_nodes)
        sensory_set = set(sensory_nodes)
        fallback = [n for n in graph_nodes if n not in sensory_set and out_syn[n] > 0]
        fallback_primary = {n: float(out_syn[n]) for n in fallback}
        fallback_secondary = {n: float(out_edges[n]) for n in fallback}
        sensory_nodes.extend(select_top(fallback, remaining, fallback_primary, fallback_secondary))

    sensory_set = set(sensory_nodes)

    output_candidates = [
        n for n in graph_nodes if n not in sensory_set and is_motor(metadata.get(n)) and in_syn[n] > 0
    ]
    output_primary = {n: float(in_syn[n]) + 0.20 * float(out_syn[n]) for n in output_candidates}
    output_secondary = {n: float(in_edges[n]) for n in output_candidates}
    output_nodes = select_top(output_candidates, max_output, output_primary, output_secondary)

    # Backfill output nodes if motor labels are sparse.
    if len(output_nodes) < max_output:
        remaining = max_output - len(output_nodes)
        output_set = set(output_nodes)
        fallback = [n for n in graph_nodes if n not in sensory_set and n not in output_set and in_syn[n] > 0]
        fallback_primary = {n: float(in_syn[n]) for n in fallback}
        fallback_secondary = {n: float(in_edges[n]) for n in fallback}
        output_nodes.extend(select_top(fallback, remaining, fallback_primary, fallback_secondary))

    output_set = set(output_nodes)

    print("Pass 2/3: measuring sensory->hidden and hidden->motor support...")
    support_from_sensory: Counter[str] = Counter()
    support_to_output: Counter[str] = Counter()
    for pre, post, syn in connection_rows(connections_path):
        if pre in sensory_set and post not in sensory_set and post not in output_set:
            support_from_sensory[post] += syn
        if post in output_set and pre not in sensory_set and pre not in output_set:
            support_to_output[pre] += syn

    hidden_candidates = [n for n in graph_nodes if n not in sensory_set and n not in output_set]
    hidden_primary = {
        n: (
            float(in_syn[n]) + float(out_syn[n]) + 2.0 * float(support_from_sensory[n]) + 2.0 * float(support_to_output[n])
        )
        for n in hidden_candidates
    }
    hidden_secondary = {n: float(out_edges[n] + in_edges[n]) for n in hidden_candidates}
    hidden_nodes = select_top(hidden_candidates, max_hidden, hidden_primary, hidden_secondary)
    hidden_set = set(hidden_nodes)

    if not hidden_nodes:
        raise SystemExit("Hidden projection selection failed (0 nodes selected).")
    if not sensory_nodes:
        raise SystemExit("Sensory projection selection failed (0 nodes selected).")
    if not output_nodes:
        raise SystemExit("Output projection selection failed (0 nodes selected).")

    sensory_idx = {node: i for i, node in enumerate(sensory_nodes)}
    hidden_idx = {node: i for i, node in enumerate(hidden_nodes)}
    output_idx = {node: i for i, node in enumerate(output_nodes)}

    s_count = len(sensory_nodes)
    h_count = len(hidden_nodes)
    o_count = len(output_nodes)

    # Dense matrices (flat row-major) for Runner snapshot.
    w_in = [0.0] * (h_count * s_count)
    p_in = [0] * (h_count * s_count)
    w_rec = [0.0] * (h_count * h_count)
    p_rec = [0] * (h_count * h_count)
    w_out = [0.0] * (o_count * h_count)
    p_out = [0] * (o_count * h_count)

    def transform_weight(syn: int) -> float:
        if weight_transform == "raw":
            return float(syn)
        if weight_transform == "sqrt":
            return math.sqrt(float(syn))
        if weight_transform == "log1p":
            return math.log1p(float(syn))
        raise ValueError(f"unsupported weight transform: {weight_transform}")

    retained_edges = 0
    retained_syn = 0
    w_in_edges = 0
    w_rec_edges = 0
    w_out_edges = 0

    print("Pass 3/3: building projected dense matrices...")
    for pre, post, syn in connection_rows(connections_path):
        if syn < min_syn_count:
            continue
        if pre in sensory_idx and post in hidden_idx:
            row = hidden_idx[post]
            col = sensory_idx[pre]
            flat = row * s_count + col
            w_in[flat] += transform_weight(syn)
            p_in[flat] += syn
            retained_edges += 1
            retained_syn += syn
            w_in_edges += 1
        elif pre in hidden_idx and post in hidden_idx:
            row = hidden_idx[post]
            col = hidden_idx[pre]
            flat = row * h_count + col
            w_rec[flat] += transform_weight(syn)
            p_rec[flat] += syn
            retained_edges += 1
            retained_syn += syn
            w_rec_edges += 1
        elif pre in hidden_idx and post in output_idx:
            row = output_idx[post]
            col = hidden_idx[pre]
            flat = row * h_count + col
            w_out[flat] += transform_weight(syn)
            p_out[flat] += syn
            retained_edges += 1
            retained_syn += syn
            w_out_edges += 1

    template = json.loads(template_path.read_text(encoding="utf-8"))
    net = dict(template.get("net", {}))

    net["num_sensory_neurons"] = s_count
    net["num_hidden_layers"] = 1
    net["num_hidden_per_layer_initial"] = h_count
    net["num_output_neurons"] = o_count
    net["sensory_target_layer"] = 0
    net["output_source_layer"] = 0
    net["clumping_design"] = "FruitFly"
    net["max_total_neurons"] = max(h_count + o_count + s_count, int(net.get("max_total_neurons", 0) or 0))
    net["growth_enabled"] = False
    net["morpho_growth_enabled"] = False
    net["sleep_enabled"] = False
    net["brain_regions"] = build_fruitfly_regions()
    net["neuron_types"] = [
        {"name": "Sensory", "bio_params": {"izh_preset": "RS", "synaptic_gain": 1.0}},
        {"name": "Interneuron", "bio_params": {"izh_preset": "FS", "synaptic_gain": 1.0}},
        {"name": "Motor", "bio_params": {"izh_preset": "RS", "synaptic_gain": 1.2}},
        {"name": "Neuromodulatory", "bio_params": {"izh_preset": "RS", "synaptic_gain": 1.1}},
    ]

    snapshot = {
        "net": net,
        "w_in": {"rows": h_count, "cols": s_count, "data": w_in},
        "w_hh_fwd": [],
        "w_hh_bwd": [],
        "w_hh_rec": [{"rows": h_count, "cols": h_count, "data": w_rec}],
        "w_out": {"rows": o_count, "cols": h_count, "data": w_out},
        "p_in": {"rows": h_count, "cols": s_count, "data": p_in},
        "p_fwd": [],
        "p_bwd": [],
        "p_rec": [{"rows": h_count, "cols": h_count, "data": p_rec}],
        "p_out": {"rows": o_count, "cols": h_count, "data": p_out},
        "layer_range": None,
        "connectome_labels": {
            "species": "drosophila_melanogaster",
            "dataset": "BANC v626",
            "source_files": {"neurons": str(neurons_path), "connections": str(connections_path)},
            "selection": {
                "max_sensory": max_sensory,
                "max_hidden": max_hidden,
                "max_output": max_output,
                "min_syn_count": min_syn_count,
                "weight_transform": weight_transform,
            },
            "sensory_nodes": sensory_nodes,
            "hidden_nodes": hidden_nodes,
            "output_nodes": output_nodes,
            "global_stats": {
                "rows": rows,
                "syn_total": syn_total,
                "graph_nodes": len(graph_nodes),
                "sensory_candidates": len(sensory_candidates),
                "output_candidates": len(output_candidates),
                "hidden_candidates": len(hidden_candidates),
            },
            "projection_stats": {
                "retained_edges": retained_edges,
                "retained_syn": retained_syn,
                "w_in_edges": w_in_edges,
                "w_rec_edges": w_rec_edges,
                "w_out_edges": w_out_edges,
                "sensory_count": s_count,
                "hidden_count": h_count,
                "output_count": o_count,
            },
        },
    }
    return snapshot


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate Drosophila connectome snapshot JSON for AARNN Runner."
    )
    parser.add_argument(
        "--neurons",
        default="data/drosophila/BANC v626/neurons.csv.gz",
        help="Path to neurons.csv(.gz)",
    )
    parser.add_argument(
        "--connections",
        default="data/drosophila/BANC v626/connections_princeton.csv.gz",
        help="Path to connections_princeton.csv(.gz)",
    )
    parser.add_argument(
        "--template",
        default="network.json",
        help="Template snapshot JSON used for baseline net defaults (default: network.json)",
    )
    parser.add_argument(
        "--output",
        default="network_drosophila.json",
        help="Output snapshot JSON path",
    )
    parser.add_argument(
        "--max-sensory",
        type=int,
        default=34,
        help="Maximum sensory nodes to project into the input layer",
    )
    parser.add_argument(
        "--max-hidden",
        type=int,
        default=2048,
        help="Maximum hidden nodes to keep in the recurrent layer",
    )
    parser.add_argument(
        "--max-output",
        type=int,
        default=48,
        help="Maximum motor/output nodes to project into output layer",
    )
    parser.add_argument(
        "--min-syn-count",
        type=int,
        default=1,
        help="Ignore raw edges with syn_count below this threshold",
    )
    parser.add_argument(
        "--weight-transform",
        choices=("raw", "sqrt", "log1p"),
        default="sqrt",
        help="Transform applied to syn_count for weight matrices",
    )
    parser.add_argument(
        "--pretty",
        action="store_true",
        help="Write pretty-printed JSON (larger files, easier inspection)",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    neurons_path = Path(args.neurons)
    connections_path = Path(args.connections)
    template_path = Path(args.template)
    output_path = Path(args.output)

    if not neurons_path.exists():
        raise SystemExit(f"Missing neurons file: {neurons_path}")
    if not connections_path.exists():
        raise SystemExit(f"Missing connections file: {connections_path}")
    if not template_path.exists():
        raise SystemExit(f"Missing template file: {template_path}")
    if args.max_sensory <= 0 or args.max_hidden <= 0 or args.max_output <= 0:
        raise SystemExit("max-sensory/max-hidden/max-output must all be > 0")
    if args.min_syn_count <= 0:
        raise SystemExit("min-syn-count must be > 0")

    snapshot = build_projection_snapshot(
        neurons_path=neurons_path,
        connections_path=connections_path,
        template_path=template_path,
        max_sensory=args.max_sensory,
        max_hidden=args.max_hidden,
        max_output=args.max_output,
        min_syn_count=args.min_syn_count,
        weight_transform=args.weight_transform,
    )

    output_path.parent.mkdir(parents=True, exist_ok=True)
    if args.pretty:
        encoded = json.dumps(snapshot, indent=2)
    else:
        encoded = json.dumps(snapshot, separators=(",", ":"))
    output_path.write_text(encoded, encoding="utf-8")

    labels = snapshot["connectome_labels"]["projection_stats"]
    print(
        f"Wrote {output_path} | S={labels['sensory_count']} "
        f"H={labels['hidden_count']} O={labels['output_count']} "
        f"edges={labels['retained_edges']} syn={labels['retained_syn']}"
    )


if __name__ == "__main__":
    main()
