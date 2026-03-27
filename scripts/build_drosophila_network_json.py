#!/usr/bin/env python3
"""
Build a Runner-compatible network snapshot JSON from Drosophila BANC connectome CSVs.

The raw BANC graph is very large (~100k+ neurons), while the current snapshot schema
expects dense matrices. To keep imports practical, this script builds a structured
projection with configurable caps:
  - sensory nodes (input layer)
  - hidden nodes (multi-layer recurrent core, configurable layer width)
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
import hashlib
import json
import math
import re
from collections import Counter
from dataclasses import dataclass, replace
from pathlib import Path
from typing import Callable, Dict, Iterable, Iterator, List, TextIO, Tuple


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

NEUROMOD_HINTS = (
    "dopamin",
    "octopamin",
    "seroton",
    "tyramin",
    "modulator",
    "neuromod",
    "peptidergic",
)

INHIBITORY_HINTS = (
    "gaba",
    "inhib",
    "interneuron",
    "local",
)

# BANC "Top in/out region" tokens grouped into coarse anatomical domains.
OPTIC_REGION_TOKENS = {
    "ME",
    "LO",
    "LOP",
    "PLP",
    "PVLP",
    "AVLP",
    "AOTU",
    "AME",
    "LA",
}
GNG_REGION_TOKENS = {"GNG"}
VNC_REGION_TOKENS = {
    "ANM",
    "AMNP",
    "HTCT",
    "WTCT",
    "LTCT",
    "NTCT",
    "INTTCT",
    "T1_MVAC",
    "T2_MVAC",
    "T3_MVAC",
    "T1_PRONM",
    "T2_MESONM",
    "T3_METANM",
}
VNC_REGION_PREFIXES = ("T1_", "T2_", "T3_")
TOP_REGION_SPLIT_RE = re.compile(r"[^A-Z0-9_]+")

# Approximate region anchors in normalized topology coordinates.
REGION_SIDE_SHAPES: Dict[str, Dict[str, Tuple[Tuple[float, float, float], Tuple[float, float, float]]]] = {
    "optic_lobes": {
        "left": ((-0.82, 0.14, 0.02), (0.30, 0.24, 0.34)),
        "right": ((0.82, 0.14, 0.02), (0.30, 0.24, 0.34)),
        "midline": ((0.0, 0.14, 0.02), (0.50, 0.20, 0.28)),
    },
    "central_brain": {
        "left": ((-0.22, 0.02, 0.05), (0.30, 0.26, 0.28)),
        "right": ((0.22, 0.02, 0.05), (0.30, 0.26, 0.28)),
        "midline": ((0.0, 0.02, 0.05), (0.24, 0.22, 0.24)),
    },
    "gnathal_ganglion": {
        "left": ((-0.22, -0.30, -0.01), (0.17, 0.14, 0.16)),
        "right": ((0.22, -0.30, -0.01), (0.17, 0.14, 0.16)),
        "midline": ((0.0, -0.30, -0.01), (0.14, 0.12, 0.14)),
    },
}

# VNC is better approximated as a longitudinal tube than an ellipsoid.
VNC_TUBE_BY_SIDE: Dict[str, Tuple[Tuple[float, float, float], Tuple[float, float, float], float]] = {
    "left": ((-0.10, -0.32, -0.08), (-0.14, -1.14, -0.14), 0.08),
    "right": ((0.10, -0.32, -0.08), (0.14, -1.14, -0.14), 0.08),
    "midline": ((0.0, -0.34, -0.08), (0.0, -1.16, -0.14), 0.06),
}


@dataclass(frozen=True)
class NodeMeta:
    root_id: str
    top_region: str
    super_class: str
    class_name: str
    sub_class: str
    flow: str
    function: str
    community_labels: str
    body_part: str
    soma_side: str
    nerve: str
    primary_cell_type: str
    soma_x: float | None
    soma_y: float | None
    soma_z: float | None

    @property
    def full_text(self) -> str:
        return " ".join(
            (
                self.top_region,
                self.super_class,
                self.class_name,
                self.sub_class,
                self.flow,
                self.function,
                self.community_labels,
                self.body_part,
                self.soma_side,
                self.nerve,
                self.primary_cell_type,
            )
        )


NODE_META_FIELDS = (
    "top_region",
    "super_class",
    "class_name",
    "sub_class",
    "flow",
    "function",
    "community_labels",
    "body_part",
    "soma_side",
    "nerve",
    "primary_cell_type",
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


def normalize_text(raw: str | None) -> str:
    return (raw or "").strip().lower()


def normalize_upper(raw: str | None) -> str:
    return (raw or "").strip().upper()


def normalize_side(raw: str | None) -> str:
    side = normalize_text(raw)
    if side in {"l", "left", "lhs"}:
        return "left"
    if side in {"r", "right", "rhs"}:
        return "right"
    if side in {"m", "mid", "midline", "center", "centre"}:
        return "midline"
    return side


def parse_float(raw: str | None) -> float | None:
    value = (raw or "").strip()
    if not value:
        return None
    try:
        return float(value)
    except ValueError:
        return None


def parse_position_triplet(raw: str | None) -> tuple[float, float, float] | None:
    text = (raw or "").strip()
    if not text:
        return None
    nums = re.findall(r"-?\d+(?:\.\d+)?", text)
    if len(nums) < 3:
        return None
    try:
        return float(nums[0]), float(nums[1]), float(nums[2])
    except ValueError:
        return None


def csv_headers(path: Path) -> list[str]:
    with open_csv_maybe_gzip(path) as f:
        reader = csv.reader(f)
        try:
            return next(reader)
        except StopIteration:
            return []


def load_csv_by_root_id(path: Path, id_columns: tuple[str, ...]) -> Dict[str, dict[str, str]]:
    if not path.exists():
        return {}
    out: Dict[str, dict[str, str]] = {}
    with open_csv_maybe_gzip(path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            root_id = ""
            for key in id_columns:
                root_id = (row.get(key) or "").strip()
                if root_id:
                    break
            if not root_id:
                continue
            out[root_id] = row
    return out


def infer_function_from_text(text: str) -> str:
    t = text.lower()
    if "sensory" in t or "afferent" in t:
        return "sensory"
    if "motor" in t or "efferent" in t:
        return "motor"
    if has_any_hint(t, NEUROMOD_HINTS):
        return "neuromodulatory"
    return ""


def deterministic_side_from_id(node_id: str) -> str:
    digest = hashlib.blake2b(node_id.encode("utf-8"), digest_size=1).digest()
    return "left" if (digest[0] % 2 == 0) else "right"


def load_banc_neuron_metadata(neurons_path: Path) -> Dict[str, NodeMeta]:
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
                top_region=normalize_upper(row.get("Top in/out region")),
                super_class=normalize_text(row.get("Super Class")),
                class_name=normalize_text(row.get("Class")),
                sub_class=normalize_text(row.get("Sub Class")),
                flow=normalize_text(row.get("Flow")),
                function=normalize_text(row.get("Function")),
                community_labels=normalize_text(row.get("Community labels")),
                body_part=normalize_text(row.get("Body Part")),
                soma_side=normalize_side(row.get("Soma side")),
                nerve=normalize_text(row.get("Nerve")),
                primary_cell_type=normalize_text(row.get("Primary Cell Type")),
                soma_x=None,
                soma_y=None,
                soma_z=None,
            )
    return meta


def load_fafb_neuron_metadata(fafb_dir: Path, neurons_path: Path) -> Dict[str, NodeMeta]:
    neuron_rows = load_csv_by_root_id(neurons_path, ("root_id", "Root ID"))
    coordinate_rows = load_csv_by_root_id(fafb_dir / "coordinates.csv.gz", ("root_id", "Root ID"))
    classification_rows = load_csv_by_root_id(fafb_dir / "classification.csv.gz", ("root_id", "Root ID"))
    names_rows = load_csv_by_root_id(fafb_dir / "names.csv.gz", ("root_id", "Root ID"))
    types_rows = load_csv_by_root_id(
        fafb_dir / "consolidated_cell_types.csv.gz",
        ("root_id", "Root ID"),
    )
    labels_rows = load_csv_by_root_id(fafb_dir / "processed_labels.csv.gz", ("root_id", "Root ID"))
    connectivity_tag_rows = load_csv_by_root_id(fafb_dir / "connectivity_tags.csv.gz", ("root_id", "Root ID"))
    column_rows = load_csv_by_root_id(fafb_dir / "column_assignment.csv.gz", ("root_id", "Root ID"))

    root_ids = set(neuron_rows) | set(classification_rows) | set(names_rows) | set(types_rows)
    if not root_ids:
        raise SystemExit(f"No FAFB metadata rows found under: {fafb_dir}")

    out: Dict[str, NodeMeta] = {}
    for root_id in root_ids:
        nrow = neuron_rows.get(root_id, {})
        coord_row = coordinate_rows.get(root_id, {})
        crow = classification_rows.get(root_id, {})
        name_row = names_rows.get(root_id, {})
        trow = types_rows.get(root_id, {})
        lrow = labels_rows.get(root_id, {})
        grow = connectivity_tag_rows.get(root_id, {})
        colrow = column_rows.get(root_id, {})

        class_name = normalize_text(crow.get("class"))
        super_class = normalize_text(crow.get("super_class"))
        sub_class = normalize_text(crow.get("sub_class")) or normalize_text(trow.get("primary_type"))
        flow = normalize_text(crow.get("flow"))
        name = normalize_text(name_row.get("name"))
        group = normalize_text(nrow.get("group")) or normalize_text(name_row.get("group"))
        processed_labels = normalize_text(lrow.get("processed_labels"))
        connectivity_tag = normalize_text(grow.get("connectivity_tag"))
        community_labels = " ".join(part for part in (processed_labels, connectivity_tag) if part)
        primary_cell_type = normalize_text(trow.get("primary_type")) or normalize_text(nrow.get("group"))
        additional_type = normalize_text(trow.get("additional_type(s)"))
        if additional_type:
            primary_cell_type = (primary_cell_type + " " + additional_type).strip()

        top_region = normalize_upper(" ".join(part for part in (name, group, class_name, super_class) if part))
        body_part = normalize_text(colrow.get("type"))
        if not body_part:
            if "optic" in super_class or "visual" in class_name:
                body_part = "optic_lobe"
            elif "motor" in class_name:
                body_part = "thorax"

        function = infer_function_from_text(
            " ".join(part for part in (class_name, sub_class, super_class, flow, community_labels, primary_cell_type) if part)
        )
        nerve = normalize_text(crow.get("nerve"))
        soma_side = normalize_side(crow.get("side") or colrow.get("hemisphere"))
        if not soma_side:
            soma_side = deterministic_side_from_id(root_id)

        coord = parse_position_triplet(coord_row.get("position"))
        if coord is not None:
            soma_x, soma_y, soma_z = coord
        else:
            soma_x = parse_float(colrow.get("x"))
            soma_y = parse_float(colrow.get("y"))
            # `column_assignment` lacks explicit z in some exports; use p/q as weak depth fallback.
            soma_z = parse_float(colrow.get("q"))
            if soma_z is None:
                soma_z = parse_float(colrow.get("p"))

        out[root_id] = NodeMeta(
            root_id=root_id,
            top_region=top_region,
            super_class=super_class,
            class_name=class_name,
            sub_class=sub_class,
            flow=flow,
            function=function,
            community_labels=community_labels,
            body_part=body_part,
            soma_side=soma_side,
            nerve=nerve,
            primary_cell_type=primary_cell_type,
            soma_x=soma_x,
            soma_y=soma_y,
            soma_z=soma_z,
        )
    return out


def detect_neuron_metadata_format(neurons_path: Path) -> str:
    headers = set(csv_headers(neurons_path))
    if "Root ID" in headers:
        return "banc"
    if "root_id" in headers:
        return "fafb"
    return "unknown"


def dataset_label_from_neurons_path(neurons_path: Path) -> str:
    path_str = str(neurons_path).lower()
    if "fafb" in path_str:
        return "FAFB v783"
    if "banc" in path_str:
        return "BANC v626"
    return "drosophila_unknown"


def mode_nonempty(values: Iterable[str]) -> str:
    counts = Counter(v for v in values if v)
    if not counts:
        return ""
    return counts.most_common(1)[0][0]


def field_mode_by_group(meta: Dict[str, NodeMeta], field: str, group_field: str) -> Dict[str, str]:
    grouped: Dict[str, Counter[str]] = {}
    for node in meta.values():
        group = getattr(node, group_field)
        value = getattr(node, field)
        if not group or not value:
            continue
        grouped.setdefault(group, Counter())[value] += 1
    return {group: counter.most_common(1)[0][0] for group, counter in grouped.items()}


def apply_metadata_fallback(primary: Dict[str, NodeMeta], fallback: Dict[str, NodeMeta]) -> Dict[str, NodeMeta]:
    if not primary or not fallback:
        return primary

    def coord_stats_by_group(
        meta_map: Dict[str, NodeMeta],
        group_field: str,
    ) -> Dict[str, tuple[tuple[float, float, float], tuple[float, float, float]]]:
        accum: Dict[str, List[float]] = {}
        for node in meta_map.values():
            coord = coord_triplet(node)
            if coord is None:
                continue
            group = getattr(node, group_field)
            if not group:
                continue
            slot = accum.setdefault(group, [0.0] * 7)
            slot[0] += 1.0
            slot[1] += coord[0]
            slot[2] += coord[1]
            slot[3] += coord[2]
            slot[4] += coord[0] * coord[0]
            slot[5] += coord[1] * coord[1]
            slot[6] += coord[2] * coord[2]

        out: Dict[str, tuple[tuple[float, float, float], tuple[float, float, float]]] = {}
        for group, slot in accum.items():
            count = max(1.0, slot[0])
            mx, my, mz = slot[1] / count, slot[2] / count, slot[3] / count
            vx = max(0.0, slot[4] / count - mx * mx)
            vy = max(0.0, slot[5] / count - my * my)
            vz = max(0.0, slot[6] / count - mz * mz)
            out[group] = ((mx, my, mz), (math.sqrt(vx), math.sqrt(vy), math.sqrt(vz)))
        return out

    def coord_stats_by_pair(
        meta_map: Dict[str, NodeMeta],
        field_a: str,
        field_b: str,
    ) -> Dict[tuple[str, str], tuple[tuple[float, float, float], tuple[float, float, float]]]:
        accum: Dict[tuple[str, str], List[float]] = {}
        for node in meta_map.values():
            coord = coord_triplet(node)
            if coord is None:
                continue
            a_val = getattr(node, field_a)
            b_val = getattr(node, field_b)
            if field_b == "soma_side":
                b_val = normalize_side(b_val)
            if not a_val or not b_val:
                continue
            key = (a_val, b_val)
            slot = accum.setdefault(key, [0.0] * 7)
            slot[0] += 1.0
            slot[1] += coord[0]
            slot[2] += coord[1]
            slot[3] += coord[2]
            slot[4] += coord[0] * coord[0]
            slot[5] += coord[1] * coord[1]
            slot[6] += coord[2] * coord[2]

        out: Dict[tuple[str, str], tuple[tuple[float, float, float], tuple[float, float, float]]] = {}
        for key, slot in accum.items():
            count = max(1.0, slot[0])
            mx, my, mz = slot[1] / count, slot[2] / count, slot[3] / count
            vx = max(0.0, slot[4] / count - mx * mx)
            vy = max(0.0, slot[5] / count - my * my)
            vz = max(0.0, slot[6] / count - mz * mz)
            out[key] = ((mx, my, mz), (math.sqrt(vx), math.sqrt(vy), math.sqrt(vz)))
        return out

    def coord_stats_global(meta_map: Dict[str, NodeMeta]) -> tuple[tuple[float, float, float], tuple[float, float, float]] | None:
        accum: List[float] = [0.0] * 7
        for node in meta_map.values():
            coord = coord_triplet(node)
            if coord is None:
                continue
            accum[0] += 1.0
            accum[1] += coord[0]
            accum[2] += coord[1]
            accum[3] += coord[2]
            accum[4] += coord[0] * coord[0]
            accum[5] += coord[1] * coord[1]
            accum[6] += coord[2] * coord[2]
        if accum[0] <= 0.0:
            return None
        count = accum[0]
        mx, my, mz = accum[1] / count, accum[2] / count, accum[3] / count
        vx = max(0.0, accum[4] / count - mx * mx)
        vy = max(0.0, accum[5] / count - my * my)
        vz = max(0.0, accum[6] / count - mz * mz)
        return ((mx, my, mz), (math.sqrt(vx), math.sqrt(vy), math.sqrt(vz)))

    def stable_u01_local(node_key: str, salt: str) -> float:
        digest = hashlib.blake2b(f"{node_key}|{salt}".encode("utf-8"), digest_size=8).digest()
        return int.from_bytes(digest, "big") / float((1 << 64) - 1)

    def stable_gauss_local(node_key: str, salt: str) -> float:
        u1 = max(1e-12, stable_u01_local(node_key, f"{salt}:u1"))
        u2 = stable_u01_local(node_key, f"{salt}:u2")
        return math.sqrt(-2.0 * math.log(u1)) * math.cos(2.0 * math.pi * u2)

    global_modes = {field: mode_nonempty(getattr(m, field) for m in fallback.values()) for field in NODE_META_FIELDS}
    modes_by_super = {field: field_mode_by_group(fallback, field, "super_class") for field in NODE_META_FIELDS}
    modes_by_class = {field: field_mode_by_group(fallback, field, "class_name") for field in NODE_META_FIELDS}
    coord_by_primary_type = coord_stats_by_group(fallback, "primary_cell_type")
    coord_by_subclass = coord_stats_by_group(fallback, "sub_class")
    coord_by_class = coord_stats_by_group(fallback, "class_name")
    coord_by_super = coord_stats_by_group(fallback, "super_class")
    coord_by_flow_side = coord_stats_by_pair(fallback, "flow", "soma_side")
    coord_by_flow = coord_stats_by_group(fallback, "flow")
    coord_by_side = coord_stats_by_group(fallback, "soma_side")
    coord_global = coord_stats_global(fallback)

    merged: Dict[str, NodeMeta] = {}
    for root_id, node in primary.items():
        updates: Dict[str, object] = {}
        for field in NODE_META_FIELDS:
            if getattr(node, field):
                continue
            value = ""
            if node.super_class:
                value = modes_by_super[field].get(node.super_class, "")
            if not value and node.class_name:
                value = modes_by_class[field].get(node.class_name, "")
            if not value:
                value = global_modes.get(field, "")
            if field == "top_region" and value:
                value = value.upper()
            if field == "soma_side" and value:
                value = normalize_side(value)
            if value:
                updates[field] = value

        if not node.soma_side and "soma_side" not in updates:
            updates["soma_side"] = deterministic_side_from_id(root_id)

        fallback_node = fallback.get(root_id)
        if node.soma_x is None and fallback_node is not None and fallback_node.soma_x is not None:
            updates["soma_x"] = fallback_node.soma_x
        if node.soma_y is None and fallback_node is not None and fallback_node.soma_y is not None:
            updates["soma_y"] = fallback_node.soma_y
        if node.soma_z is None and fallback_node is not None and fallback_node.soma_z is not None:
            updates["soma_z"] = fallback_node.soma_z

        # If root IDs don't overlap across datasets, reuse coordinate priors from matched metadata groups.
        if node.soma_x is None or node.soma_y is None or node.soma_z is None:
            coord_stats = None
            node_side = normalize_side(node.soma_side or str(updates.get("soma_side", "")))
            if node.primary_cell_type:
                coord_stats = coord_by_primary_type.get(node.primary_cell_type)
            if coord_stats is None and node.sub_class:
                coord_stats = coord_by_subclass.get(node.sub_class)
            if coord_stats is None and node.class_name:
                coord_stats = coord_by_class.get(node.class_name)
            if coord_stats is None and node.super_class:
                coord_stats = coord_by_super.get(node.super_class)
            if coord_stats is None and node.flow and node_side:
                coord_stats = coord_by_flow_side.get((node.flow, node_side))
            if coord_stats is None and node.flow:
                coord_stats = coord_by_flow.get(node.flow)
            if coord_stats is None and node_side:
                coord_stats = coord_by_side.get(node_side)
            if coord_stats is None:
                coord_stats = coord_global
            if coord_stats is not None:
                mean_xyz, sigma_xyz = coord_stats
                jitter_scale = 0.35
                est_x = mean_xyz[0] + stable_gauss_local(root_id, "coord_x") * sigma_xyz[0] * jitter_scale
                est_y = mean_xyz[1] + stable_gauss_local(root_id, "coord_y") * sigma_xyz[1] * jitter_scale
                est_z = mean_xyz[2] + stable_gauss_local(root_id, "coord_z") * sigma_xyz[2] * jitter_scale
                if node.soma_x is None and "soma_x" not in updates:
                    updates["soma_x"] = est_x
                if node.soma_y is None and "soma_y" not in updates:
                    updates["soma_y"] = est_y
                if node.soma_z is None and "soma_z" not in updates:
                    updates["soma_z"] = est_z

        if updates:
            merged[root_id] = replace(node, **updates)
        else:
            merged[root_id] = node
    return merged


def load_neuron_metadata(neurons_path: Path) -> Dict[str, NodeMeta]:
    fmt = detect_neuron_metadata_format(neurons_path)
    if fmt == "banc":
        return load_banc_neuron_metadata(neurons_path)
    if fmt == "fafb":
        return load_fafb_neuron_metadata(neurons_path.parent, neurons_path)
    raise SystemExit(f"Unable to infer neurons metadata format from headers in: {neurons_path}")


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


def enforce_role_alignment(
    selected: List[str],
    ranked_candidates: List[str],
    limit: int,
    metadata: Dict[str, NodeMeta],
    predicate: Callable[[NodeMeta | None], bool],
) -> tuple[List[str], List[str]]:
    aligned: List[str] = []
    seen: set[str] = set()
    replaced: List[str] = []

    for node in selected:
        if node in seen:
            continue
        if predicate(metadata.get(node)):
            aligned.append(node)
            seen.add(node)
        else:
            replaced.append(node)

    if len(aligned) < limit:
        for node in ranked_candidates:
            if node in seen:
                continue
            if not predicate(metadata.get(node)):
                continue
            aligned.append(node)
            seen.add(node)
            if len(aligned) >= limit:
                break

    return aligned[:limit], replaced


def remap_index(index: int, src_size: int, dst_size: int) -> int:
    if dst_size <= 1:
        return 0
    if src_size <= 1:
        return 0
    ratio = float(index) / float(src_size - 1)
    mapped = int(round(ratio * float(dst_size - 1)))
    return max(0, min(dst_size - 1, mapped))


def aarnn_laminar_io_layers(hidden_layer_count: int) -> tuple[int, int]:
    """Map hidden-layer count to canonical AARNN laminar IO layers."""
    if hidden_layer_count <= 0:
        return 0, 0
    sensory_target_layer = 1 if hidden_layer_count > 1 else 0  # L4
    if hidden_layer_count > 4:
        output_source_layer = 4  # L5
    else:
        output_source_layer = hidden_layer_count - 1
    return sensory_target_layer, output_source_layer


def default_hidden_layers_for_clumping(clumping_design: str) -> int | None:
    """Return canonical laminar hidden-layer count for a clumping style."""
    return {
        "HumanBrain": 6,
        "FruitFly": 10,
        "FruitFlyLarva": 10,
        "ZebraFish": 6,
        "NematodeWorm": 1,
    }.get(clumping_design)


def split_hidden_nodes_into_layers(
    hidden_nodes: List[str],
    hidden_layer_width: int,
    target_layer_count: int | None,
) -> List[List[str]]:
    """
    Split ordered hidden nodes into layers.

    If `target_layer_count` is provided, produce exactly that many non-empty layers
    (bounded by the number of hidden nodes). Otherwise, fall back to width chunking.
    """
    if not hidden_nodes:
        return []
    if target_layer_count is not None and target_layer_count > 0:
        layer_count = max(1, min(int(target_layer_count), len(hidden_nodes)))
        base = len(hidden_nodes) // layer_count
        remainder = len(hidden_nodes) % layer_count
        layers: List[List[str]] = []
        start = 0
        for li in range(layer_count):
            size = base + (1 if li < remainder else 0)
            end = start + size
            layers.append(hidden_nodes[start:end])
            start = end
        return layers
    width = max(1, int(hidden_layer_width))
    return [hidden_nodes[start : start + width] for start in range(0, len(hidden_nodes), width)]


def stable_u01(node_key: str, salt: str) -> float:
    digest = hashlib.blake2b(f"{node_key}|{salt}".encode("utf-8"), digest_size=8).digest()
    value = int.from_bytes(digest, "big")
    return value / float((1 << 64) - 1)


def stable_gauss(node_key: str, salt: str) -> float:
    u1 = max(1e-12, stable_u01(node_key, f"{salt}:u1"))
    u2 = stable_u01(node_key, f"{salt}:u2")
    return math.sqrt(-2.0 * math.log(u1)) * math.cos(2.0 * math.pi * u2)


def clamp(v: float, lo: float, hi: float) -> float:
    return max(lo, min(hi, v))


def pick_weighted_cluster(
    node_key: str,
    salt: str,
    clusters: List[Tuple[Tuple[float, float, float], Tuple[float, float, float], float]],
) -> Tuple[Tuple[float, float, float], Tuple[float, float, float]]:
    if not clusters:
        return (0.0, 0.0, 0.0), (0.25, 0.25, 0.25)
    total = sum(max(0.0, c[2]) for c in clusters)
    if total <= 0.0:
        return clusters[0][0], clusters[0][1]
    t = stable_u01(node_key, f"{salt}:cluster") * total
    acc = 0.0
    for center_n, spread_n, weight in clusters:
        acc += max(0.0, weight)
        if t <= acc:
            return center_n, spread_n
    return clusters[-1][0], clusters[-1][1]


def sample_clustered_ellipsoid(
    center: tuple[float, float, float],
    radii: tuple[float, float, float],
    node_key: str,
    salt: str,
    clusters: List[Tuple[Tuple[float, float, float], Tuple[float, float, float], float]],
) -> tuple[float, float, float]:
    (cx, cy, cz), (sx, sy, sz) = pick_weighted_cluster(node_key, salt, clusters)
    nx = cx + stable_gauss(node_key, f"{salt}:x") * sx
    ny = cy + stable_gauss(node_key, f"{salt}:y") * sy
    nz = cz + stable_gauss(node_key, f"{salt}:z") * sz
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
    center: tuple[float, float, float],
    radii: tuple[float, float, float],
    node_key: str,
    salt: str,
) -> tuple[float, float, float]:
    u = stable_u01(node_key, f"{salt}:u")
    v = stable_u01(node_key, f"{salt}:v")
    w = stable_u01(node_key, f"{salt}:w")
    theta = 2.0 * math.pi * u
    cos_phi = max(-1.0, min(1.0, 2.0 * v - 1.0))
    phi = math.acos(cos_phi)
    r = w ** (1.0 / 3.0)
    sin_phi = math.sin(phi)
    dx = r * sin_phi * math.cos(theta)
    dy = r * sin_phi * math.sin(theta)
    dz = r * math.cos(phi)
    return (
        center[0] + radii[0] * dx,
        center[1] + radii[1] * dy,
        center[2] + radii[2] * dz,
    )


def sample_point_in_tube(
    line_from: tuple[float, float, float],
    line_to: tuple[float, float, float],
    radius: float,
    node_key: str,
    salt: str,
) -> tuple[float, float, float]:
    t = stable_u01(node_key, f"{salt}:t")
    angle = 2.0 * math.pi * stable_u01(node_key, f"{salt}:a")
    rr = math.sqrt(stable_u01(node_key, f"{salt}:r")) * radius
    px = line_from[0] + (line_to[0] - line_from[0]) * t
    py = line_from[1] + (line_to[1] - line_from[1]) * t
    pz = line_from[2] + (line_to[2] - line_from[2]) * t
    # Radial jitter around tube axis; axis is mostly Y so perturb X/Z.
    return (px + rr * math.cos(angle), py, pz + rr * math.sin(angle))


def infer_side(meta: NodeMeta | None, node_id: str) -> str:
    if meta is not None and meta.soma_side in {"left", "right", "midline"}:
        return meta.soma_side
    return "left" if stable_u01(node_id, "side") < 0.5 else "right"


def top_region_tokens(meta: NodeMeta | None) -> List[str]:
    if meta is None or not meta.top_region:
        return []
    return [tok for tok in TOP_REGION_SPLIT_RE.split(meta.top_region) if tok]


def infer_region_name(meta: NodeMeta | None, node_id: str, role: str) -> str:
    # role in {"sensory", "hidden", "output"}.
    if meta is None:
        return "central_brain"

    tokens = top_region_tokens(meta)
    token_set = set(tokens)
    token_has_prefix = any(tok.startswith(VNC_REGION_PREFIXES) for tok in tokens)
    text = meta.full_text

    # Body-part cues are strongest for sensory and output classes.
    if role in {"sensory", "output"} and meta.body_part:
        body = meta.body_part
        if any(k in body for k in ("retina", "interommatidial")):
            return "optic_lobes"
        if any(k in body for k in ("antenna", "maxillary", "labellum", "pharynx", "palp")):
            return "gnathal_ganglion"
        if any(k in body for k in ("leg", "wing", "haltere", "abdomen", "thorax", "neck")):
            return "ventral_nerve_cord"

    if "optic_lobe" in meta.super_class or "visual" in text:
        return "optic_lobes"
    if "ventral_nerve_cord" in meta.super_class:
        return "ventral_nerve_cord"
    if "motor" in meta.super_class and role == "output":
        return "ventral_nerve_cord"

    if token_set & OPTIC_REGION_TOKENS:
        return "optic_lobes"
    if token_set & GNG_REGION_TOKENS:
        return "gnathal_ganglion"
    if (token_set & VNC_REGION_TOKENS) or token_has_prefix:
        return "ventral_nerve_cord"

    # Fallback by role keeps sensory near anterior neuropils and outputs near VNC.
    if role == "sensory":
        return "gnathal_ganglion"
    if role == "output":
        return "ventral_nerve_cord"
    return "central_brain"


def lateralized_region_name(base_region: str, side: str) -> str:
    side_key = side if side in {"left", "right"} else "midline"
    return f"{base_region}_{side_key}"


def infer_type_name(meta: NodeMeta | None, node_id: str, role: str) -> str:
    if role == "sensory":
        return "Sensory"
    if role == "output":
        return "Motor"
    if meta is None:
        # Keep default hidden blend close to profile target (~30% inhibitory).
        return "Interneuron" if stable_u01(node_id, "hidden:type_balance") < 0.30 else "Sensory"
    if has_any_hint(meta.full_text, NEUROMOD_HINTS):
        return "Neuromodulatory"
    text = meta.full_text
    if has_any_hint(text, INHIBITORY_HINTS):
        # Let strong inhibitory annotations dominate unless hash says otherwise.
        if stable_u01(node_id, "hidden:inhibitory_hint") < 0.90:
            return "Interneuron"

    balance_u = stable_u01(node_id, "hidden:type_balance")
    if balance_u < 0.30:
        return "Interneuron"

    if is_motor(meta) and balance_u > 0.70:
        return "Motor"
    if is_sensory(meta) and balance_u > 0.55:
        return "Sensory"

    # Default excitatory-like hidden phenotype for unlabeled central cells.
    return "Sensory" if stable_u01(node_id, "hidden:excitatory_role") < 0.60 else "Motor"


def coord_triplet(meta: NodeMeta | None) -> tuple[float, float, float] | None:
    if meta is None:
        return None
    if meta.soma_x is None or meta.soma_y is None or meta.soma_z is None:
        return None
    return float(meta.soma_x), float(meta.soma_y), float(meta.soma_z)


def quantile_sorted(values: List[float], q: float) -> float:
    if not values:
        return 0.0
    if len(values) == 1:
        return values[0]
    q_clamped = max(0.0, min(1.0, q))
    pos = q_clamped * float(len(values) - 1)
    lo = int(math.floor(pos))
    hi = int(math.ceil(pos))
    if lo == hi:
        return values[lo]
    frac = pos - float(lo)
    return values[lo] * (1.0 - frac) + values[hi] * frac


def build_coord_normalizer(
    metadata: Dict[str, NodeMeta],
    node_ids: List[str],
) -> Tuple[Callable[[tuple[float, float, float]], tuple[float, float, float]] | None, int]:
    coords: List[tuple[float, float, float]] = []
    for node_id in node_ids:
        c = coord_triplet(metadata.get(node_id))
        if c is not None:
            coords.append(c)
    if len(coords) < 24:
        return None, len(coords)

    xs = sorted(c[0] for c in coords)
    ys = sorted(c[1] for c in coords)
    zs = sorted(c[2] for c in coords)

    x_lo, x_hi = quantile_sorted(xs, 0.01), quantile_sorted(xs, 0.99)
    y_lo, y_hi = quantile_sorted(ys, 0.01), quantile_sorted(ys, 0.99)
    z_lo, z_hi = quantile_sorted(zs, 0.01), quantile_sorted(zs, 0.99)

    x_mid, y_mid, z_mid = (x_lo + x_hi) * 0.5, (y_lo + y_hi) * 0.5, (z_lo + z_hi) * 0.5
    x_span = max(1e-9, (x_hi - x_lo) * 0.5)
    y_span = max(1e-9, (y_hi - y_lo) * 0.5)
    z_span = max(1e-9, (z_hi - z_lo) * 0.5)

    def normalize(raw: tuple[float, float, float]) -> tuple[float, float, float]:
        x = (raw[0] - x_mid) / x_span
        y = (raw[1] - y_mid) / y_span
        z = (raw[2] - z_mid) / z_span
        return (
            max(-1.35, min(1.35, x)),
            max(-1.35, min(1.35, y)),
            max(-1.35, min(1.35, z)),
        )

    return normalize, len(coords)


def sample_region_position_synthetic(
    node_id: str,
    region_name: str,
    side: str,
    role: str,
    layer_frac: float,
) -> tuple[float, float, float]:
    side_key = side if side in {"left", "right"} else "midline"
    if region_name == "ventral_nerve_cord":
        line_from, line_to, radius = VNC_TUBE_BY_SIDE.get(side_key, VNC_TUBE_BY_SIDE["midline"])
        if role == "sensory":
            anchors = [0.08, 0.18, 0.30, 0.44]
        elif role == "output":
            anchors = [0.52, 0.66, 0.80, 0.93]
        else:
            anchors = [0.10, 0.22, 0.34, 0.47, 0.61, 0.74, 0.88]
        idx = int(stable_u01(node_id, "vnc_seg") * len(anchors)) % len(anchors)
        t = clamp(anchors[idx] + stable_gauss(node_id, "vnc_t") * 0.028, 0.0, 1.0)
        angle_anchors = [0.2, 1.6, 3.2, 4.8]
        angle = angle_anchors[int(stable_u01(node_id, "vnc_ang_anchor") * len(angle_anchors)) % len(angle_anchors)]
        angle += stable_gauss(node_id, "vnc_ang") * 0.24
        rr = clamp(0.40 + 0.60 * stable_u01(node_id, "vnc_rr"), 0.0, 1.0) * radius
        px = line_from[0] + (line_to[0] - line_from[0]) * t
        py = line_from[1] + (line_to[1] - line_from[1]) * t
        pz = line_from[2] + (line_to[2] - line_from[2]) * t
        x = px + rr * math.cos(angle)
        y = py
        z = pz + rr * math.sin(angle)
    else:
        center, radii = REGION_SIDE_SHAPES[region_name][side_key]
        if region_name == "optic_lobes":
            clusters = [
                ((-0.56, 0.18, 0.34), (0.16, 0.12, 0.12), 0.38),
                ((-0.20, -0.03, 0.06), (0.18, 0.15, 0.14), 0.30),
                ((0.16, -0.17, -0.24), (0.15, 0.14, 0.12), 0.22),
                ((0.45, 0.12, -0.34), (0.12, 0.10, 0.10), 0.10),
            ]
        elif region_name == "central_brain":
            clusters = [
                ((-0.36, 0.08, 0.12), (0.18, 0.16, 0.14), 0.27),
                ((0.00, 0.18, 0.24), (0.16, 0.13, 0.12), 0.24),
                ((0.34, -0.12, 0.02), (0.17, 0.15, 0.14), 0.25),
                ((0.00, -0.20, -0.20), (0.16, 0.13, 0.12), 0.24),
            ]
        else:
            clusters = [
                ((-0.34, 0.12, 0.04), (0.20, 0.16, 0.14), 0.34),
                ((0.20, -0.02, 0.12), (0.20, 0.16, 0.14), 0.32),
                ((0.02, -0.26, -0.16), (0.20, 0.16, 0.14), 0.34),
            ]
        x, y, z = sample_clustered_ellipsoid(center, radii, node_id, role, clusters)

    # Keep role-specific structure: sensory a bit peripheral, outputs a bit caudal.
    side_push = -1.0 if side_key == "left" else 1.0 if side_key == "right" else 0.0
    if role == "sensory":
        x += 0.06 * side_push
        y += 0.03
    elif role == "output":
        y -= 0.03
    elif role == "hidden":
        # Preserve a weak depth cue from hidden layering without forcing a strip.
        z += (0.5 - layer_frac) * 0.10

    return (
        max(-1.35, min(1.35, x)),
        max(-1.35, min(1.35, y)),
        max(-1.35, min(1.35, z)),
    )


def sample_region_position(
    node_id: str,
    region_name: str,
    side: str,
    role: str,
    layer_frac: float,
    meta: NodeMeta | None = None,
    coord_normalizer: Callable[[tuple[float, float, float]], tuple[float, float, float]] | None = None,
) -> tuple[float, float, float]:
    sx, sy, sz = sample_region_position_synthetic(node_id, region_name, side, role, layer_frac)
    if coord_normalizer is None:
        return sx, sy, sz
    raw = coord_triplet(meta)
    if raw is None:
        return sx, sy, sz
    cx, cy, cz = coord_normalizer(raw)
    # Keep source coordinates as primary geometry, with mild anatomical regularization.
    blend = 0.90 if role == "hidden" else 0.84
    x = blend * cx + (1.0 - blend) * sx
    y = blend * cy + (1.0 - blend) * sy
    z = blend * cz + (1.0 - blend) * sz
    return (
        max(-1.35, min(1.35, x)),
        max(-1.35, min(1.35, y)),
        max(-1.35, min(1.35, z)),
    )


def build_topology(
    metadata: Dict[str, NodeMeta],
    sensory_nodes: List[str],
    hidden_layers: List[List[str]],
    output_nodes: List[str],
) -> tuple[dict, Dict[str, int]]:
    layer_count = max(1, len(hidden_layers))
    all_nodes = [n for layer in hidden_layers for n in layer]
    all_nodes.extend(sensory_nodes)
    all_nodes.extend(output_nodes)
    coord_normalizer, coord_nodes = build_coord_normalizer(metadata, all_nodes)
    topo_layers: List[List[dict]] = []
    region_counts: Counter[str] = Counter()

    for li, layer in enumerate(hidden_layers):
        layer_frac = li / float(max(1, layer_count - 1))
        out_layer: List[dict] = []
        for node_id in layer:
            meta = metadata.get(node_id)
            side = infer_side(meta, node_id)
            region_base = infer_region_name(meta, node_id, role="hidden")
            region_name = lateralized_region_name(region_base, side)
            type_name = infer_type_name(meta, node_id, role="hidden")
            x, y, z = sample_region_position(
                node_id,
                region_base,
                side,
                "hidden",
                layer_frac,
                meta=meta,
                coord_normalizer=coord_normalizer,
            )
            out_layer.append(
                {
                    "x": x,
                    "y": y,
                    "z": z,
                    "layer": li,
                    "region_name": region_name,
                    "type_name": type_name,
                }
            )
            region_counts[f"hidden:{region_name}"] += 1
        topo_layers.append(out_layer)

    sensory_out: List[dict] = []
    for node_id in sensory_nodes:
        meta = metadata.get(node_id)
        side = infer_side(meta, node_id)
        region_base = infer_region_name(meta, node_id, role="sensory")
        region_name = lateralized_region_name(region_base, side)
        type_name = infer_type_name(meta, node_id, role="sensory")
        x, y, z = sample_region_position(
            node_id,
            region_base,
            side,
            "sensory",
            layer_frac=0.0,
            meta=meta,
            coord_normalizer=coord_normalizer,
        )
        sensory_out.append(
            {
                "x": x,
                "y": y,
                "z": z,
                "layer": 0,
                "region_name": region_name,
                "type_name": type_name,
            }
        )
        region_counts[f"sensory:{region_name}"] += 1

    output_out: List[dict] = []
    for node_id in output_nodes:
        meta = metadata.get(node_id)
        side = infer_side(meta, node_id)
        region_base = infer_region_name(meta, node_id, role="output")
        region_name = lateralized_region_name(region_base, side)
        type_name = infer_type_name(meta, node_id, role="output")
        x, y, z = sample_region_position(
            node_id,
            region_base,
            side,
            "output",
            layer_frac=1.0,
            meta=meta,
            coord_normalizer=coord_normalizer,
        )
        output_out.append(
            {
                "x": x,
                "y": y,
                "z": z,
                "layer": 0,
                "region_name": region_name,
                "type_name": type_name,
            }
        )
        region_counts[f"output:{region_name}"] += 1

    topo = {"layers": topo_layers, "sensory_nodes": sensory_out, "output_nodes": output_out}
    if coord_nodes > 0:
        topo["source_coordinate_nodes"] = coord_nodes
        topo["source_coordinate_mode"] = "soma_coordinates_with_heuristic_fallback"
    return topo, {k: int(v) for k, v in sorted(region_counts.items())}


def build_fruitfly_regions() -> List[dict]:
    # Bilateralized region list so clumping/timing keep left-right fly anatomy
    # separated instead of collapsing both hemispheres to a single anchor.
    regions: List[dict] = []

    def add_ellipsoid_region(
        name: str,
        center: List[float],
        radii: List[float],
        type_distribution: List[List[float | str]],
    ) -> None:
        regions.append(
            {
                "name": name,
                "shape": {"shape": "ellipsoid", "center": center, "radii": radii},
                "center": center,
                "radii": radii,
                "type_distribution": type_distribution,
            }
        )

    optic_types: List[List[float | str]] = [["Sensory", 0.55], ["Interneuron", 0.35], ["Neuromodulatory", 0.10]]
    add_ellipsoid_region("optic_lobes_left", [-28.0, 18.0, 0.0], [14.0, 14.0, 16.0], optic_types)
    add_ellipsoid_region("optic_lobes_right", [28.0, 18.0, 0.0], [14.0, 14.0, 16.0], optic_types)
    add_ellipsoid_region("optic_lobes_midline", [0.0, 18.0, 0.0], [10.0, 10.0, 12.0], optic_types)

    central_types: List[List[float | str]] = [
        ["Interneuron", 0.70],
        ["Sensory", 0.15],
        ["Motor", 0.10],
        ["Neuromodulatory", 0.05],
    ]
    add_ellipsoid_region("central_brain_left", [-10.0, 0.0, 2.0], [12.0, 14.0, 12.0], central_types)
    add_ellipsoid_region("central_brain_right", [10.0, 0.0, 2.0], [12.0, 14.0, 12.0], central_types)
    add_ellipsoid_region("central_brain_midline", [0.0, 0.0, 2.0], [10.0, 12.0, 10.0], central_types)

    gng_types: List[List[float | str]] = [
        ["Interneuron", 0.45],
        ["Motor", 0.35],
        ["Sensory", 0.15],
        ["Neuromodulatory", 0.05],
    ]
    add_ellipsoid_region("gnathal_ganglion_left", [-10.0, -20.0, 0.0], [9.0, 9.0, 9.0], gng_types)
    add_ellipsoid_region("gnathal_ganglion_right", [10.0, -20.0, 0.0], [9.0, 9.0, 9.0], gng_types)
    add_ellipsoid_region("gnathal_ganglion_midline", [0.0, -20.0, 0.0], [8.0, 8.0, 8.0], gng_types)

    vnc_types: List[List[float | str]] = [["Motor", 0.55], ["Interneuron", 0.35], ["Sensory", 0.10]]
    regions.append(
        {
            "name": "ventral_nerve_cord_left",
            "shape": {"shape": "tube", "line_from": [-3.0, -26.0, -6.0], "line_to": [-5.0, -95.0, -8.0], "radius": 5.5},
            "center": [-4.0, -60.5, -7.0],
            "radii": [5.5, 34.5, 5.5],
            "type_distribution": vnc_types,
        }
    )
    regions.append(
        {
            "name": "ventral_nerve_cord_right",
            "shape": {"shape": "tube", "line_from": [3.0, -26.0, -6.0], "line_to": [5.0, -95.0, -8.0], "radius": 5.5},
            "center": [4.0, -60.5, -7.0],
            "radii": [5.5, 34.5, 5.5],
            "type_distribution": vnc_types,
        }
    )
    regions.append(
        {
            "name": "ventral_nerve_cord_midline",
            "shape": {"shape": "tube", "line_from": [0.0, -26.0, -6.0], "line_to": [0.0, -95.0, -6.0], "radius": 4.0},
            "center": [0.0, -60.5, -6.0],
            "radii": [4.0, 34.5, 4.0],
            "type_distribution": vnc_types,
        }
    )
    return regions


def build_projection_snapshot(
    neurons_path: Path,
    connections_path: Path,
    template_path: Path,
    max_sensory: int,
    max_hidden: int,
    max_output: int,
    min_syn_count: int,
    weight_transform: str,
    hidden_layer_width: int,
    long_range_policy: str,
    metadata: Dict[str, NodeMeta] | None = None,
    dataset_label: str | None = None,
    source_files: Dict[str, str] | None = None,
) -> dict:
    if metadata is None:
        metadata = load_neuron_metadata(neurons_path)
    if dataset_label is None:
        dataset_label = dataset_label_from_neurons_path(neurons_path)

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

    sensory_ranked = select_top(sensory_candidates, len(sensory_candidates), sensory_primary, sensory_secondary)
    sensory_nodes, sensory_replaced = enforce_role_alignment(
        selected=sensory_nodes,
        ranked_candidates=sensory_ranked,
        limit=max_sensory,
        metadata=metadata,
        predicate=is_sensory,
    )
    if sensory_replaced:
        print(f"Role alignment: replaced {len(sensory_replaced)} non-sensory input nodes with sensory-classified nodes.")
    if len(sensory_nodes) < max_sensory:
        raise SystemExit(
            "Unable to build a fully sensory-aligned input layer. "
            f"Requested {max_sensory}, found {len(sensory_nodes)} sensory-classified nodes."
        )

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

    output_ranked = select_top(output_candidates, len(output_candidates), output_primary, output_secondary)
    output_nodes, output_replaced = enforce_role_alignment(
        selected=output_nodes,
        ranked_candidates=output_ranked,
        limit=max_output,
        metadata=metadata,
        predicate=is_motor,
    )
    if output_replaced:
        print(f"Role alignment: replaced {len(output_replaced)} non-motor output nodes with motor-classified nodes.")
    if len(output_nodes) < max_output:
        raise SystemExit(
            "Unable to build a fully motor-aligned output layer. "
            f"Requested {max_output}, found {len(output_nodes)} motor-classified nodes."
        )

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
    region_candidates: Dict[str, List[str]] = {}
    for node in hidden_candidates:
        region_name = infer_region_name(metadata.get(node), node, role="hidden")
        region_candidates.setdefault(region_name, []).append(node)

    hidden_nodes: List[str] = []
    hidden_seen: set[str] = set()
    if region_candidates:
        # Balance region coverage using sqrt weighting so large domains don't dominate
        # the extracted subset while preserving degree-based prioritization within region.
        region_weights = {region: math.sqrt(len(nodes)) for region, nodes in region_candidates.items()}
        total_weight = sum(region_weights.values()) or 1.0
        region_order = sorted(region_candidates.keys())
        base_quota: Dict[str, int] = {}
        frac_quota: Dict[str, float] = {}
        for region in region_order:
            target = (max_hidden * region_weights[region]) / total_weight
            q = int(math.floor(target))
            base_quota[region] = min(q, len(region_candidates[region]))
            frac_quota[region] = target - float(q)

        selected = sum(base_quota.values())
        remaining_slots = max(0, max_hidden - selected)
        if remaining_slots > 0:
            add_order = sorted(
                region_order,
                key=lambda r: (-frac_quota[r], -region_weights[r], r),
            )
            idx = 0
            guard = 0
            max_guard = max_hidden * 8 + 1024
            while remaining_slots > 0 and guard < max_guard:
                guard += 1
                region = add_order[idx % len(add_order)]
                idx += 1
                if base_quota[region] >= len(region_candidates[region]):
                    continue
                base_quota[region] += 1
                remaining_slots -= 1

        for region in region_order:
            picks = select_top(
                region_candidates[region],
                base_quota[region],
                hidden_primary,
                hidden_secondary,
            )
            for node in picks:
                if node not in hidden_seen:
                    hidden_seen.add(node)
                    hidden_nodes.append(node)

    if len(hidden_nodes) < max_hidden:
        for node in select_top(hidden_candidates, max_hidden, hidden_primary, hidden_secondary):
            if node in hidden_seen:
                continue
            hidden_seen.add(node)
            hidden_nodes.append(node)
            if len(hidden_nodes) >= max_hidden:
                break

    hidden_region_counts = Counter(
        infer_region_name(metadata.get(node), node, role="hidden") for node in hidden_nodes
    )

    if not hidden_nodes:
        raise SystemExit("Hidden projection selection failed (0 nodes selected).")
    if not sensory_nodes:
        raise SystemExit("Sensory projection selection failed (0 nodes selected).")
    if not output_nodes:
        raise SystemExit("Output projection selection failed (0 nodes selected).")

    hidden_layer_width = max(1, int(hidden_layer_width))
    clumping_design = "FruitFly"
    target_layer_count = default_hidden_layers_for_clumping(clumping_design)

    # Order selected hidden neurons by sensory->output "depth" so splitting into layers
    # keeps more edges within same/adjacent layers.
    def depth_score(node: str) -> float:
        sensory_drive = float(support_from_sensory[node])
        motor_drive = float(support_to_output[node])
        denom = sensory_drive + motor_drive
        if denom <= 0.0:
            return 0.5
        return sensory_drive / denom

    hidden_nodes.sort(
        key=lambda node: (
            -depth_score(node),
            -float(hidden_primary.get(node, 0.0)),
            -float(hidden_secondary.get(node, 0.0)),
            node,
        )
    )

    hidden_layers = split_hidden_nodes_into_layers(
        hidden_nodes=hidden_nodes,
        hidden_layer_width=hidden_layer_width,
        target_layer_count=target_layer_count,
    )
    layer_sizes = [len(layer) for layer in hidden_layers]
    layer_count = len(layer_sizes)
    if layer_count == 0:
        raise SystemExit("Hidden projection layering failed (0 layers).")
    sensory_target_layer, output_source_layer = aarnn_laminar_io_layers(layer_count)

    hidden_pos: Dict[str, tuple[int, int]] = {}
    for li, layer in enumerate(hidden_layers):
        for idx, node in enumerate(layer):
            hidden_pos[node] = (li, idx)

    sensory_idx = {node: i for i, node in enumerate(sensory_nodes)}
    output_idx = {node: i for i, node in enumerate(output_nodes)}

    s_count = len(sensory_nodes)
    h_count = len(hidden_nodes)
    o_count = len(output_nodes)

    # Dense matrices (flat row-major) for Runner snapshot, split across hidden layers.
    in_rows = layer_sizes[sensory_target_layer]
    out_cols = layer_sizes[output_source_layer]
    w_in = [0.0] * (in_rows * s_count)
    p_in = [0] * (in_rows * s_count)
    w_fwd = [
        [0.0] * (layer_sizes[l + 1] * layer_sizes[l]) for l in range(layer_count - 1)
    ]
    p_fwd = [[0] * (layer_sizes[l + 1] * layer_sizes[l]) for l in range(layer_count - 1)]
    w_bwd = [[0.0] * (layer_sizes[l] * layer_sizes[l + 1]) for l in range(layer_count - 1)]
    p_bwd = [[0] * (layer_sizes[l] * layer_sizes[l + 1]) for l in range(layer_count - 1)]
    w_rec = [[0.0] * (n * n) for n in layer_sizes]
    p_rec = [[0] * (n * n) for n in layer_sizes]
    w_out = [0.0] * (o_count * out_cols)
    p_out = [0] * (o_count * out_cols)

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
    w_fwd_edges = 0
    w_bwd_edges = 0
    w_rec_edges = 0
    w_out_edges = 0
    folded_sensory_edges = 0
    folded_forward_edges = 0
    folded_backward_edges = 0
    folded_output_edges = 0
    dropped_long_range_edges = 0

    print("Pass 3/3: building projected dense matrices...")
    for pre, post, syn in connection_rows(connections_path):
        if syn < min_syn_count:
            continue
        weight = transform_weight(syn)

        if pre in sensory_idx and post in hidden_pos:
            sensory_col = sensory_idx[pre]
            post_layer, post_row = hidden_pos[post]
            if post_layer != sensory_target_layer:
                if long_range_policy == "fold":
                    post_row = remap_index(
                        post_row, layer_sizes[post_layer], layer_sizes[sensory_target_layer]
                    )
                    folded_sensory_edges += 1
                else:
                    dropped_long_range_edges += 1
                    continue
            flat = post_row * s_count + sensory_col
            w_in[flat] += weight
            p_in[flat] += syn
            retained_edges += 1
            retained_syn += syn
            w_in_edges += 1
            continue

        if pre in hidden_pos and post in hidden_pos:
            pre_layer, pre_idx = hidden_pos[pre]
            post_layer, post_idx = hidden_pos[post]

            if post_layer == pre_layer:
                n = layer_sizes[pre_layer]
                flat = post_idx * n + pre_idx
                w_rec[pre_layer][flat] += weight
                p_rec[pre_layer][flat] += syn
                retained_edges += 1
                retained_syn += syn
                w_rec_edges += 1
                continue

            if post_layer == pre_layer + 1:
                cols = layer_sizes[pre_layer]
                flat = post_idx * cols + pre_idx
                w_fwd[pre_layer][flat] += weight
                p_fwd[pre_layer][flat] += syn
                retained_edges += 1
                retained_syn += syn
                w_fwd_edges += 1
                continue

            if pre_layer == post_layer + 1:
                cols = layer_sizes[pre_layer]
                flat = post_idx * cols + pre_idx
                w_bwd[post_layer][flat] += weight
                p_bwd[post_layer][flat] += syn
                retained_edges += 1
                retained_syn += syn
                w_bwd_edges += 1
                continue

            if long_range_policy != "fold":
                dropped_long_range_edges += 1
                continue

            if post_layer > pre_layer:
                # Fold distant forward edge into nearest forward hop.
                hop = pre_layer
                mapped_post = remap_index(post_idx, layer_sizes[post_layer], layer_sizes[hop + 1])
                cols = layer_sizes[hop]
                flat = mapped_post * cols + pre_idx
                w_fwd[hop][flat] += weight
                p_fwd[hop][flat] += syn
                folded_forward_edges += 1
                retained_edges += 1
                retained_syn += syn
                w_fwd_edges += 1
            else:
                # Fold distant backward edge into nearest backward hop.
                hop = post_layer
                mapped_pre = remap_index(pre_idx, layer_sizes[pre_layer], layer_sizes[hop + 1])
                cols = layer_sizes[hop + 1]
                flat = post_idx * cols + mapped_pre
                w_bwd[hop][flat] += weight
                p_bwd[hop][flat] += syn
                folded_backward_edges += 1
                retained_edges += 1
                retained_syn += syn
                w_bwd_edges += 1
            continue

        if pre in hidden_pos and post in output_idx:
            pre_layer, pre_idx = hidden_pos[pre]
            out_row = output_idx[post]
            if pre_layer != output_source_layer:
                if long_range_policy == "fold":
                    pre_idx = remap_index(pre_idx, layer_sizes[pre_layer], layer_sizes[output_source_layer])
                    folded_output_edges += 1
                else:
                    dropped_long_range_edges += 1
                    continue
            flat = out_row * out_cols + pre_idx
            w_out[flat] += weight
            p_out[flat] += syn
            retained_edges += 1
            retained_syn += syn
            w_out_edges += 1
            continue

    template = json.loads(template_path.read_text(encoding="utf-8"))
    net = dict(template.get("net", {}))

    net["num_sensory_neurons"] = s_count
    net["num_hidden_layers"] = layer_count
    net["num_hidden_per_layer_initial"] = layer_sizes[0]
    net["num_output_neurons"] = o_count
    net["sensory_target_layer"] = sensory_target_layer
    net["output_source_layer"] = output_source_layer
    net["clumping_design"] = clumping_design
    net["max_layers"] = max(layer_count, int(net.get("max_layers", 0) or 0))
    net["max_total_neurons"] = max(
        h_count + o_count + s_count, int(net.get("max_total_neurons", 0) or 0)
    )
    # Adult Drosophila biomimicry profile.
    net["growth_enabled"] = True
    net["morpho_growth_enabled"] = True
    net["use_morphology"] = True
    net["use_aarnn_delays"] = True
    net["aarnn_layer_depth"] = max(4, int(net.get("aarnn_layer_depth", 0) or 0))
    net["max_layers"] = max(layer_count + 2, int(net.get("max_layers", 0) or 0))
    net["spawn_radius"] = 0.065
    net["new_edge_prob"] = 0.04
    net["proximity_degree_cap"] = 5
    net["sleep_enabled"] = True
    net["sleep_cycle_ms"] = 120000.0
    net["sleep_duration_ms"] = 900.0
    net["theta_rhythm_enabled"] = True
    net["theta_rhythm_hz"] = 8.0
    net["theta_rhythm_duty"] = 0.24
    net["theta_rhythm_phase_jitter"] = 0.04
    net["thalamic_gating_enabled"] = False
    net["aarnn_velocity"] = 8.5
    net["axon_velocity"] = 12.0
    net["dend_velocity"] = 4.6
    net["p_release_default"] = 0.68
    net["bouton_latency_ms"] = 0.45
    net["bouton_jitter_ms"] = 0.08
    net["aarnn_dale_strictness"] = 0.82
    net["aarnn_inhibitory_fraction"] = 0.30
    net["aarnn_gap_junction_strength"] = 0.03
    net["aarnn_gap_junction_radius"] = 0.22
    net["aarnn_gap_junction_inhibitory_only"] = False
    net["aarnn_nmda_voltage_sensitivity"] = 0.03
    net["aarnn_distance_attenuation_per_unit"] = 0.20
    net["aarnn_release_prob_heterogeneity"] = 0.10
    net["volume_transmission_enabled"] = True
    net["volume_transmission_radius"] = 0.28
    net["volume_transmission_strength"] = 0.09
    net["aarnn_triplet_ltp_gain"] = 0.18
    net["aarnn_triplet_ltd_gain"] = 0.11
    net["aarnn_synaptic_scaling_strength"] = 0.025
    net["aarnn_synaptic_scaling_target"] = 1.0
    net["aarnn_myelination_enabled"] = False
    net["aarnn_myelination_rate"] = 0.0
    net["aarnn_demyelination_rate"] = 0.0
    net["aarnn_myelin_min_conduction_gain"] = 1.0
    net["aarnn_myelin_max_conduction_gain"] = 1.0
    net["aarnn_myelin_initial"] = 0.0
    net["aarnn_import_topology_rewire_enabled"] = True
    net["aarnn_import_topology_rewire_keep_fraction"] = 0.78
    net["aarnn_import_topology_rewire_region_bias"] = 0.24
    net["brain_regions"] = build_fruitfly_regions()
    net["neuron_types"] = [
        {"name": "Sensory", "bio_params": {"izh_preset": "RS", "synaptic_gain": 1.0}},
        {"name": "Interneuron", "bio_params": {"izh_preset": "FS", "synaptic_gain": 1.0}},
        {"name": "Motor", "bio_params": {"izh_preset": "RS", "synaptic_gain": 1.2}},
        {"name": "Neuromodulatory", "bio_params": {"izh_preset": "RS", "synaptic_gain": 1.1}},
    ]

    total_dense_params = len(w_in) + len(w_out)
    total_dense_params += sum(len(m) for m in w_fwd)
    total_dense_params += sum(len(m) for m in w_bwd)
    total_dense_params += sum(len(m) for m in w_rec)
    topo, topo_region_counts = build_topology(
        metadata=metadata,
        sensory_nodes=sensory_nodes,
        hidden_layers=hidden_layers,
        output_nodes=output_nodes,
    )

    snapshot = {
        "net": net,
        "topo": topo,
        "w_in": {"rows": in_rows, "cols": s_count, "data": w_in},
        "w_hh_fwd": [
            {"rows": layer_sizes[l + 1], "cols": layer_sizes[l], "data": m}
            for l, m in enumerate(w_fwd)
        ],
        "w_hh_bwd": [
            {"rows": layer_sizes[l], "cols": layer_sizes[l + 1], "data": m}
            for l, m in enumerate(w_bwd)
        ],
        "w_hh_rec": [
            {"rows": layer_sizes[l], "cols": layer_sizes[l], "data": m}
            for l, m in enumerate(w_rec)
        ],
        "w_out": {"rows": o_count, "cols": out_cols, "data": w_out},
        "p_in": {"rows": in_rows, "cols": s_count, "data": p_in},
        "p_fwd": [
            {"rows": layer_sizes[l + 1], "cols": layer_sizes[l], "data": m}
            for l, m in enumerate(p_fwd)
        ],
        "p_bwd": [
            {"rows": layer_sizes[l], "cols": layer_sizes[l + 1], "data": m}
            for l, m in enumerate(p_bwd)
        ],
        "p_rec": [
            {"rows": layer_sizes[l], "cols": layer_sizes[l], "data": m}
            for l, m in enumerate(p_rec)
        ],
        "p_out": {"rows": o_count, "cols": out_cols, "data": p_out},
        "layer_range": None,
        "connectome_labels": {
            "species": "drosophila_melanogaster",
            "dataset": dataset_label,
            "source_files": source_files
            or {"neurons": str(neurons_path), "connections": str(connections_path)},
            "selection": {
                "max_sensory": max_sensory,
                "max_hidden": max_hidden,
                "max_output": max_output,
                "min_syn_count": min_syn_count,
                "weight_transform": weight_transform,
                "hidden_layer_width": hidden_layer_width,
                "long_range_policy": long_range_policy,
                "sensory_replacements_for_alignment": len(sensory_replaced),
                "output_replacements_for_alignment": len(output_replaced),
            },
            "sensory_nodes": sensory_nodes,
            "hidden_nodes": hidden_nodes,
            "output_nodes": output_nodes,
            "hidden_layer_sizes": layer_sizes,
            "laminar_mapping": {
                "hidden_layer_count": layer_count,
                "sensory_target_layer": sensory_target_layer,
                "output_source_layer": output_source_layer,
                "sensory_layer_name": "L4" if sensory_target_layer == 1 else "fallback",
                "output_layer_name": "L5" if output_source_layer == 4 else "fallback",
            },
            "hidden_region_counts": {k: int(v) for k, v in sorted(hidden_region_counts.items())},
            "topology_projection": {
                "mode": "region_inferred_balanced",
                "topo_region_counts": topo_region_counts,
                "uses_soma_side": True,
                "uses_top_region_tokens": True,
            },
            "bio_profile": {
                "species": "drosophila_melanogaster",
                "dataset": dataset_label,
                "growth3d": bool(net.get("growth_enabled", False)),
                "morphology": bool(net.get("use_morphology", False)),
                "morphological_growth": bool(net.get("morpho_growth_enabled", False)),
                "aarnn_layer_depth": int(net.get("aarnn_layer_depth", 0) or 0),
                "rewire_on_import": bool(net.get("aarnn_import_topology_rewire_enabled", False)),
                "rewire_keep_fraction": float(net.get("aarnn_import_topology_rewire_keep_fraction", 1.0) or 1.0),
            },
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
                "w_hh_fwd_edges": w_fwd_edges,
                "w_hh_bwd_edges": w_bwd_edges,
                "w_hh_rec_edges": w_rec_edges,
                "w_out_edges": w_out_edges,
                "folded_sensory_edges": folded_sensory_edges,
                "folded_forward_edges": folded_forward_edges,
                "folded_backward_edges": folded_backward_edges,
                "folded_output_edges": folded_output_edges,
                "dropped_long_range_edges": dropped_long_range_edges,
                "sensory_count": s_count,
                "hidden_count": h_count,
                "hidden_layer_count": layer_count,
                "output_count": o_count,
                "dense_param_count": total_dense_params,
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
        "--dual",
        action="store_true",
        help="Build both BANC and FAFB snapshots in one run",
    )
    parser.add_argument(
        "--banc-dir",
        default="data/drosophila/BANC v626",
        help="BANC dataset directory (dual mode)",
    )
    parser.add_argument(
        "--fafb-dir",
        default="data/drosophila/FAFB v783",
        help="FAFB dataset directory (dual mode)",
    )
    parser.add_argument(
        "--output-banc",
        default="network_drosophila_banc.json",
        help="Output snapshot JSON path for BANC (dual mode)",
    )
    parser.add_argument(
        "--output-fafb",
        default="network_drosophila_fafb.json",
        help="Output snapshot JSON path for FAFB (dual mode)",
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
        default=20000,
        help="Maximum hidden nodes to keep across all hidden layers",
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
        "--hidden-layer-width",
        type=int,
        default=512,
        help="Fallback max neurons per hidden layer chunk when clumping style has no fixed layer count",
    )
    parser.add_argument(
        "--long-range-policy",
        choices=("fold", "drop"),
        default="fold",
        help="How to handle non-adjacent hidden edges in multilayer projection",
    )
    parser.add_argument(
        "--pretty",
        action="store_true",
        help="Write pretty-printed JSON (larger files, easier inspection)",
    )
    return parser.parse_args()


def write_snapshot(path: Path, snapshot: dict, pretty: bool) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if pretty:
        encoded = json.dumps(snapshot, indent=2)
    else:
        encoded = json.dumps(snapshot, separators=(",", ":"))
    path.write_text(encoded, encoding="utf-8")


def print_snapshot_summary(path: Path, snapshot: dict) -> None:
    labels = snapshot["connectome_labels"]["projection_stats"]
    layer_sizes = snapshot["connectome_labels"].get("hidden_layer_sizes", [])
    dataset = snapshot["connectome_labels"].get("dataset", "unknown")
    print(
        f"Wrote {path} ({dataset}) | S={labels['sensory_count']} "
        f"H={labels['hidden_count']} ({labels['hidden_layer_count']} layers) "
        f"O={labels['output_count']} "
        f"edges={labels['retained_edges']} syn={labels['retained_syn']} "
        f"layer_sizes={layer_sizes}"
    )


def ensure_positive_projection_args(args: argparse.Namespace) -> None:
    if args.max_sensory <= 0 or args.max_hidden <= 0 or args.max_output <= 0:
        raise SystemExit("max-sensory/max-hidden/max-output must all be > 0")
    if args.min_syn_count <= 0:
        raise SystemExit("min-syn-count must be > 0")
    if args.hidden_layer_width <= 0:
        raise SystemExit("hidden-layer-width must be > 0")


def find_companion_neurons_path(neurons_path: Path) -> Path | None:
    dataset_dir = neurons_path.parent
    parent = dataset_dir.parent
    name = dataset_dir.name.lower()
    if "banc" in name:
        candidate = parent / "FAFB v783" / "neurons.csv.gz"
        return candidate if candidate.exists() else None
    if "fafb" in name:
        candidate = parent / "BANC v626" / "neurons.csv.gz"
        return candidate if candidate.exists() else None
    return None


def main() -> None:
    args = parse_args()
    template_path = Path(args.template)
    if not template_path.exists():
        raise SystemExit(f"Missing template file: {template_path}")
    ensure_positive_projection_args(args)

    if args.dual:
        banc_dir = Path(args.banc_dir)
        fafb_dir = Path(args.fafb_dir)
        banc_neurons = banc_dir / "neurons.csv.gz"
        banc_connections = banc_dir / "connections_princeton.csv.gz"
        fafb_neurons = fafb_dir / "neurons.csv.gz"
        fafb_connections = fafb_dir / "connections_princeton.csv.gz"

        for p in (banc_neurons, banc_connections, fafb_neurons, fafb_connections):
            if not p.exists():
                raise SystemExit(f"Missing dataset file: {p}")

        metadata_banc = load_banc_neuron_metadata(banc_neurons)
        metadata_fafb = load_fafb_neuron_metadata(fafb_dir, fafb_neurons)

        # Fill sparse fields in each model from the alternate model's metadata priors.
        metadata_banc = apply_metadata_fallback(metadata_banc, metadata_fafb)
        metadata_fafb = apply_metadata_fallback(metadata_fafb, metadata_banc)

        snapshot_banc = build_projection_snapshot(
            neurons_path=banc_neurons,
            connections_path=banc_connections,
            template_path=template_path,
            max_sensory=args.max_sensory,
            max_hidden=args.max_hidden,
            max_output=args.max_output,
            min_syn_count=args.min_syn_count,
            weight_transform=args.weight_transform,
            hidden_layer_width=args.hidden_layer_width,
            long_range_policy=args.long_range_policy,
            metadata=metadata_banc,
            dataset_label="BANC v626",
            source_files={
                "neurons": str(banc_neurons),
                "connections": str(banc_connections),
                "metadata_fallback": "FAFB v783",
            },
        )
        snapshot_fafb = build_projection_snapshot(
            neurons_path=fafb_neurons,
            connections_path=fafb_connections,
            template_path=template_path,
            max_sensory=args.max_sensory,
            max_hidden=args.max_hidden,
            max_output=args.max_output,
            min_syn_count=args.min_syn_count,
            weight_transform=args.weight_transform,
            hidden_layer_width=args.hidden_layer_width,
            long_range_policy=args.long_range_policy,
            metadata=metadata_fafb,
            dataset_label="FAFB v783",
            source_files={
                "neurons": str(fafb_neurons),
                "connections": str(fafb_connections),
                "classification": str(fafb_dir / "classification.csv.gz"),
                "names": str(fafb_dir / "names.csv.gz"),
                "cell_types": str(fafb_dir / "consolidated_cell_types.csv.gz"),
                "metadata_fallback": "BANC v626",
            },
        )

        output_banc = Path(args.output_banc)
        output_fafb = Path(args.output_fafb)
        write_snapshot(output_banc, snapshot_banc, args.pretty)
        write_snapshot(output_fafb, snapshot_fafb, args.pretty)
        print_snapshot_summary(output_banc, snapshot_banc)
        print_snapshot_summary(output_fafb, snapshot_fafb)
        return

    neurons_path = Path(args.neurons)
    connections_path = Path(args.connections)
    output_path = Path(args.output)

    if not neurons_path.exists():
        raise SystemExit(f"Missing neurons file: {neurons_path}")
    if not connections_path.exists():
        raise SystemExit(f"Missing connections file: {connections_path}")

    metadata_primary = load_neuron_metadata(neurons_path)
    fallback_meta_source = None
    companion_neurons = find_companion_neurons_path(neurons_path)
    if companion_neurons is not None and companion_neurons != neurons_path:
        try:
            metadata_companion = load_neuron_metadata(companion_neurons)
            metadata_primary = apply_metadata_fallback(metadata_primary, metadata_companion)
            fallback_meta_source = str(companion_neurons)
        except Exception:
            # Keep single-dataset metadata path resilient when companion files are partial.
            fallback_meta_source = None

    snapshot = build_projection_snapshot(
        neurons_path=neurons_path,
        connections_path=connections_path,
        template_path=template_path,
        max_sensory=args.max_sensory,
        max_hidden=args.max_hidden,
        max_output=args.max_output,
        min_syn_count=args.min_syn_count,
        weight_transform=args.weight_transform,
        hidden_layer_width=args.hidden_layer_width,
        long_range_policy=args.long_range_policy,
        metadata=metadata_primary,
        dataset_label=dataset_label_from_neurons_path(neurons_path),
        source_files={
            "neurons": str(neurons_path),
            "connections": str(connections_path),
            "metadata_fallback_neurons": fallback_meta_source,
        },
    )

    write_snapshot(output_path, snapshot, args.pretty)
    print_snapshot_summary(output_path, snapshot)


if __name__ == "__main__":
    main()
