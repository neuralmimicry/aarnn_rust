#!/usr/bin/env python3
"""
Compute biomimicry calibration metrics for AARNN snapshot JSON files.

Metrics:
  - Connection and physical length distributions
  - Hidden-network triad motifs (transitive / cycle / reciprocal)
  - E/I composition by region
  - Optional activity metrics from spike logs (rate, ISI CV, band power)
"""

from __future__ import annotations

import argparse
import csv
import json
import math
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Iterable, List, Sequence, Tuple


EPS = 1.0e-12


@dataclass(frozen=True)
class TopoNode:
    x: float
    y: float
    z: float
    region_name: str
    type_name: str


def q(values: Sequence[float], p: float) -> float:
    if not values:
        return 0.0
    if len(values) == 1:
        return float(values[0])
    pos = max(0.0, min(1.0, p)) * float(len(values) - 1)
    lo = int(math.floor(pos))
    hi = int(math.ceil(pos))
    if lo == hi:
        return float(values[lo])
    frac = pos - float(lo)
    return float(values[lo] * (1.0 - frac) + values[hi] * frac)


def summarize(values: Sequence[float]) -> Dict[str, float | int]:
    if not values:
        return {"count": 0}
    s = sorted(float(v) for v in values)
    n = len(s)
    mean = sum(s) / float(n)
    var = sum((v - mean) ** 2 for v in s) / float(n)
    return {
        "count": n,
        "min": s[0],
        "p10": q(s, 0.10),
        "p50": q(s, 0.50),
        "p90": q(s, 0.90),
        "p99": q(s, 0.99),
        "max": s[-1],
        "mean": mean,
        "std": math.sqrt(max(0.0, var)),
    }


def load_snapshot(path: Path) -> dict[str, Any]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise SystemExit(f"Snapshot must be a JSON object: {path}")
    return payload


def to_topo_nodes(raw_nodes: Iterable[dict[str, Any]]) -> List[TopoNode]:
    out: List[TopoNode] = []
    for node in raw_nodes:
        out.append(
            TopoNode(
                x=float(node.get("x", 0.0) or 0.0),
                y=float(node.get("y", 0.0) or 0.0),
                z=float(node.get("z", 0.0) or 0.0),
                region_name=str(node.get("region_name", "") or ""),
                type_name=str(node.get("type_name", "") or ""),
            )
        )
    return out


def matrix_meta(mat: dict[str, Any]) -> tuple[int, int, List[float]]:
    rows = int(mat.get("rows", 0) or 0)
    cols = int(mat.get("cols", 0) or 0)
    data = [float(v) for v in (mat.get("data") or [])]
    if rows < 0 or cols < 0:
        raise SystemExit("Matrix rows/cols must be non-negative")
    if rows * cols > len(data):
        # tolerate partial data vectors by padding with zeros
        data.extend([0.0] * (rows * cols - len(data)))
    return rows, cols, data


def presence_meta(mat: dict[str, Any] | None) -> tuple[int, int, List[int]] | None:
    if not isinstance(mat, dict):
        return None
    rows = int(mat.get("rows", 0) or 0)
    cols = int(mat.get("cols", 0) or 0)
    data = [int(v) for v in (mat.get("data") or [])]
    if rows * cols > len(data):
        data.extend([0] * (rows * cols - len(data)))
    return rows, cols, data


def dist3(a: TopoNode, b: TopoNode) -> float:
    dx = a.x - b.x
    dy = a.y - b.y
    dz = a.z - b.z
    return math.sqrt(dx * dx + dy * dy + dz * dz)


def matrix_nonzero_edges(
    mat: dict[str, Any],
    presence: dict[str, Any] | None,
    pre_nodes: Sequence[TopoNode],
    post_nodes: Sequence[TopoNode],
) -> tuple[List[float], int]:
    rows, cols, data = matrix_meta(mat)
    pmeta = presence_meta(presence)
    row_lim = min(rows, len(post_nodes))
    col_lim = min(cols, len(pre_nodes))
    lengths: List[float] = []
    count = 0
    for r in range(row_lim):
        base = r * cols
        for c in range(col_lim):
            w = data[base + c]
            if abs(w) <= EPS:
                continue
            if pmeta is not None:
                p_rows, p_cols, p_data = pmeta
                if r < p_rows and c < p_cols and p_data[r * p_cols + c] <= 0:
                    continue
            count += 1
            lengths.append(dist3(pre_nodes[c], post_nodes[r]))
    return lengths, count


def flatten_hidden_nodes(hidden_layers: Sequence[Sequence[TopoNode]]) -> tuple[List[TopoNode], Dict[tuple[int, int], int]]:
    flat: List[TopoNode] = []
    idx: Dict[tuple[int, int], int] = {}
    for l, layer in enumerate(hidden_layers):
        for i, node in enumerate(layer):
            idx[(l, i)] = len(flat)
            flat.append(node)
    return flat, idx


def hidden_adjacency(snapshot: dict[str, Any], hidden_layers: Sequence[Sequence[TopoNode]]) -> tuple[Dict[int, set[int]], int]:
    adjacency: Dict[int, set[int]] = defaultdict(set)
    edge_count = 0
    _, index = flatten_hidden_nodes(hidden_layers)

    def add_edge(pre_layer: int, pre_idx: int, post_layer: int, post_idx: int) -> None:
        nonlocal edge_count
        p = index.get((pre_layer, pre_idx))
        qid = index.get((post_layer, post_idx))
        if p is None or qid is None or p == qid:
            return
        if qid not in adjacency[p]:
            adjacency[p].add(qid)
            edge_count += 1

    # recurrent
    rec = snapshot.get("w_hh_rec") or []
    for l, mat in enumerate(rec):
        rows, cols, data = matrix_meta(mat)
        row_lim = min(rows, len(hidden_layers[l]) if l < len(hidden_layers) else 0)
        col_lim = min(cols, len(hidden_layers[l]) if l < len(hidden_layers) else 0)
        for r in range(row_lim):
            base = r * cols
            for c in range(col_lim):
                if abs(data[base + c]) > EPS:
                    add_edge(l, c, l, r)

    # forward
    fwd = snapshot.get("w_hh_fwd") or []
    for l, mat in enumerate(fwd):
        rows, cols, data = matrix_meta(mat)
        row_lim = min(rows, len(hidden_layers[l + 1]) if l + 1 < len(hidden_layers) else 0)
        col_lim = min(cols, len(hidden_layers[l]) if l < len(hidden_layers) else 0)
        for r in range(row_lim):
            base = r * cols
            for c in range(col_lim):
                if abs(data[base + c]) > EPS:
                    add_edge(l, c, l + 1, r)

    # backward
    bwd = snapshot.get("w_hh_bwd") or []
    for l, mat in enumerate(bwd):
        rows, cols, data = matrix_meta(mat)
        row_lim = min(rows, len(hidden_layers[l]) if l < len(hidden_layers) else 0)
        col_lim = min(cols, len(hidden_layers[l + 1]) if l + 1 < len(hidden_layers) else 0)
        for r in range(row_lim):
            base = r * cols
            for c in range(col_lim):
                if abs(data[base + c]) > EPS:
                    add_edge(l + 1, c, l, r)

    return adjacency, edge_count


def select_motif_nodes(adjacency: Dict[int, set[int]], max_nodes: int) -> List[int]:
    nodes = sorted(set(adjacency.keys()) | {v for out in adjacency.values() for v in out})
    if len(nodes) <= max_nodes:
        return nodes

    in_deg = defaultdict(int)
    for src, outs in adjacency.items():
        for dst in outs:
            in_deg[dst] += 1
        in_deg[src] += 0
    nodes.sort(key=lambda n: (-(len(adjacency.get(n, set())) + in_deg[n]), n))
    return sorted(nodes[:max_nodes])


def motif_counts(adjacency: Dict[int, set[int]], max_nodes: int) -> Dict[str, int]:
    nodes = select_motif_nodes(adjacency, max_nodes)
    node_set = set(nodes)

    reciprocal = 0
    for i in nodes:
        out_i = adjacency.get(i, set())
        for j in out_i:
            if j in node_set and j > i and i in adjacency.get(j, set()):
                reciprocal += 1

    transitive = set()
    cycles = set()
    for i in nodes:
        out_i = adjacency.get(i, set()) & node_set
        for j in out_i:
            if j == i:
                continue
            out_j = adjacency.get(j, set()) & node_set
            for k in out_j:
                if k == i or k == j:
                    continue
                if k in out_i:
                    transitive.add(tuple(sorted((i, j, k))))
                if i in adjacency.get(k, set()):
                    cycles.add(tuple(sorted((i, j, k))))

    return {
        "sampled_nodes": len(nodes),
        "reciprocal_pairs": reciprocal,
        "transitive_triads": len(transitive),
        "cycle_triads": len(cycles),
    }


def type_is_inhibitory(type_name: str) -> bool:
    t = (type_name or "").strip().lower()
    if not t:
        return False
    return any(
        token in t
        for token in (
            "inhib",
            "interneuron",
            "gaba",
            "pv",
            "som",
            "dd",
            "vd",
        )
    )


def ei_by_region(hidden_layers: Sequence[Sequence[TopoNode]], fallback_fraction: float) -> Dict[str, Any]:
    stats: Dict[str, Dict[str, int]] = defaultdict(lambda: {"excitatory": 0, "inhibitory": 0, "unknown": 0})
    total_exc = 0
    total_inh = 0
    total_unk = 0

    for layer in hidden_layers:
        for node in layer:
            region = node.region_name or "unknown_region"
            if not node.type_name:
                stats[region]["unknown"] += 1
                total_unk += 1
            elif type_is_inhibitory(node.type_name):
                stats[region]["inhibitory"] += 1
                total_inh += 1
            else:
                stats[region]["excitatory"] += 1
                total_exc += 1

    measured_total = total_exc + total_inh
    if measured_total > 0:
        inhib_fraction = total_inh / float(measured_total)
    else:
        inhib_fraction = fallback_fraction

    return {
        "overall": {
            "excitatory": total_exc,
            "inhibitory": total_inh,
            "unknown": total_unk,
            "inhibitory_fraction": inhib_fraction,
        },
        "by_region": dict(sorted(stats.items(), key=lambda kv: kv[0])),
    }


def parse_spikes(path: Path) -> List[Tuple[float, str]]:
    ext = path.suffix.lower()
    spikes: List[Tuple[float, str]] = []

    def parse_event(evt: dict[str, Any]) -> Tuple[float, str] | None:
        t = None
        for key in ("time_ms", "t_ms", "time", "t"):
            if key in evt:
                try:
                    t = float(evt[key])
                except Exception:
                    t = None
                break
        node = None
        for key in ("node_id", "node", "neuron", "id"):
            if key in evt:
                node = str(evt[key])
                break
        if t is None or node is None:
            return None
        return (t, node)

    if ext == ".csv":
        with path.open("r", encoding="utf-8", newline="") as f:
            reader = csv.DictReader(f)
            for row in reader:
                event = parse_event(row)
                if event is not None:
                    spikes.append(event)
    else:
        payload = json.loads(path.read_text(encoding="utf-8"))
        if isinstance(payload, dict):
            payload = payload.get("spikes", [])
        if not isinstance(payload, list):
            raise SystemExit(f"Unsupported spike payload shape: {path}")
        for item in payload:
            if not isinstance(item, dict):
                continue
            event = parse_event(item)
            if event is not None:
                spikes.append(event)

    spikes.sort(key=lambda item: item[0])
    return spikes


def band_power(series: Sequence[float], fs_hz: float, f_lo: float, f_hi: float, step_hz: float = 1.0) -> float:
    if len(series) < 8 or fs_hz <= 0.0:
        return 0.0
    nyq = fs_hz * 0.5
    lo = max(0.0, min(f_lo, nyq))
    hi = max(lo, min(f_hi, nyq))
    if hi <= lo + 1.0e-6:
        return 0.0
    freqs = []
    f = max(step_hz, lo)
    while f <= hi + 1.0e-9:
        freqs.append(f)
        f += step_hz
    if not freqs:
        return 0.0

    n = len(series)
    total = 0.0
    for freq in freqs:
        re = 0.0
        im = 0.0
        for i, x in enumerate(series):
            ang = 2.0 * math.pi * freq * float(i) / fs_hz
            re += x * math.cos(ang)
            im -= x * math.sin(ang)
        total += (re * re + im * im) / float(n * n)
    return total / float(len(freqs))


def activity_metrics(spikes: List[Tuple[float, str]], bin_ms: float) -> Dict[str, Any]:
    if not spikes:
        return {"available": False, "reason": "no_spikes"}

    t_min = spikes[0][0]
    t_max = spikes[-1][0]
    duration_ms = max(1.0, t_max - t_min)
    duration_s = duration_ms / 1000.0

    per_node: Dict[str, List[float]] = defaultdict(list)
    for t, node in spikes:
        per_node[node].append(t)

    rates = [len(ts) / duration_s for ts in per_node.values()]
    isi_cv_values: List[float] = []
    for ts in per_node.values():
        if len(ts) < 3:
            continue
        isi = [ts[i + 1] - ts[i] for i in range(len(ts) - 1)]
        m = sum(isi) / float(len(isi))
        if m <= EPS:
            continue
        var = sum((x - m) ** 2 for x in isi) / float(len(isi))
        isi_cv_values.append(math.sqrt(max(0.0, var)) / m)

    bin_ms = max(1.0, bin_ms)
    n_bins = int(math.floor(duration_ms / bin_ms)) + 1
    pop = [0.0] * n_bins
    for t, _ in spikes:
        idx = int((t - t_min) / bin_ms)
        idx = max(0, min(n_bins - 1, idx))
        pop[idx] += 1.0
    fs_hz = 1000.0 / bin_ms

    bands = {
        "delta_0_5_4": (0.5, 4.0),
        "theta_4_8": (4.0, 8.0),
        "alpha_8_12": (8.0, 12.0),
        "beta_12_30": (12.0, 30.0),
        "gamma_30_80": (30.0, 80.0),
    }
    power = {name: band_power(pop, fs_hz, lo, hi) for name, (lo, hi) in bands.items()}

    return {
        "available": True,
        "duration_ms": duration_ms,
        "total_spikes": len(spikes),
        "active_nodes": len(per_node),
        "firing_rate_hz": summarize(rates),
        "isi_cv": summarize(isi_cv_values),
        "population_band_power": power,
    }


def compute_metrics(snapshot: dict[str, Any], max_triad_nodes: int, spikes: List[Tuple[float, str]] | None, spike_bin_ms: float) -> Dict[str, Any]:
    net = snapshot.get("net") or {}
    topo = snapshot.get("topo") or {}
    hidden_layers_raw = topo.get("layers") or []
    sensory_nodes = to_topo_nodes(topo.get("sensory_nodes") or [])
    output_nodes = to_topo_nodes(topo.get("output_nodes") or [])
    hidden_layers = [to_topo_nodes(layer or []) for layer in hidden_layers_raw]

    sensory_target = int(net.get("sensory_target_layer", 0) or 0)
    output_source = int(net.get("output_source_layer", max(0, len(hidden_layers) - 1)) or 0)
    sensory_target = max(0, min(sensory_target, max(0, len(hidden_layers) - 1)))
    output_source = max(0, min(output_source, max(0, len(hidden_layers) - 1)))

    connection_lengths: Dict[str, List[float]] = {}
    edge_counts: Dict[str, int] = {}

    if hidden_layers:
        lengths, count = matrix_nonzero_edges(
            snapshot.get("w_in") or {"rows": 0, "cols": 0, "data": []},
            snapshot.get("p_in"),
            sensory_nodes,
            hidden_layers[sensory_target],
        )
        connection_lengths["sensory_to_hidden"] = lengths
        edge_counts["sensory_to_hidden"] = count

        total_lengths: List[float] = list(lengths)
        total_edges = count

        for l, mat in enumerate(snapshot.get("w_hh_fwd") or []):
            if l + 1 >= len(hidden_layers):
                continue
            lengths, count = matrix_nonzero_edges(mat, None, hidden_layers[l], hidden_layers[l + 1])
            connection_lengths[f"hidden_fwd_l{l}"] = lengths
            edge_counts[f"hidden_fwd_l{l}"] = count
            total_lengths.extend(lengths)
            total_edges += count

        for l, mat in enumerate(snapshot.get("w_hh_bwd") or []):
            if l + 1 >= len(hidden_layers):
                continue
            lengths, count = matrix_nonzero_edges(mat, None, hidden_layers[l + 1], hidden_layers[l])
            connection_lengths[f"hidden_bwd_l{l}"] = lengths
            edge_counts[f"hidden_bwd_l{l}"] = count
            total_lengths.extend(lengths)
            total_edges += count

        for l, mat in enumerate(snapshot.get("w_hh_rec") or []):
            if l >= len(hidden_layers):
                continue
            lengths, count = matrix_nonzero_edges(mat, None, hidden_layers[l], hidden_layers[l])
            connection_lengths[f"hidden_rec_l{l}"] = lengths
            edge_counts[f"hidden_rec_l{l}"] = count
            total_lengths.extend(lengths)
            total_edges += count

        lengths, count = matrix_nonzero_edges(
            snapshot.get("w_out") or {"rows": 0, "cols": 0, "data": []},
            snapshot.get("p_out"),
            hidden_layers[output_source],
            output_nodes,
        )
        connection_lengths["hidden_to_output"] = lengths
        edge_counts["hidden_to_output"] = count
        total_lengths.extend(lengths)
        total_edges += count
    else:
        total_lengths = []
        total_edges = 0

    hidden_adj, hidden_edge_count = hidden_adjacency(snapshot, hidden_layers)
    motif = motif_counts(hidden_adj, max_triad_nodes)
    motif["hidden_directed_edge_count"] = hidden_edge_count

    ei = ei_by_region(hidden_layers, float(net.get("aarnn_inhibitory_fraction", 0.2) or 0.2))

    by_class = {name: summarize(vals) for name, vals in connection_lengths.items()}
    activity = (
        activity_metrics(spikes, spike_bin_ms)
        if spikes is not None
        else {"available": False, "reason": "spike_log_not_provided"}
    )

    return {
        "network": {
            "sensory": int(net.get("num_sensory_neurons", 0) or 0),
            "hidden_layers": int(net.get("num_hidden_layers", 0) or 0),
            "output": int(net.get("num_output_neurons", 0) or 0),
            "aarnn_layer_depth": int(net.get("aarnn_layer_depth", 0) or 0),
        },
        "connectivity": {
            "edge_count_total": total_edges,
            "edge_count_by_class": dict(sorted(edge_counts.items(), key=lambda kv: kv[0])),
            "connection_length_total": summarize(total_lengths),
            "connection_length_by_class": dict(sorted(by_class.items(), key=lambda kv: kv[0])),
        },
        "motifs": motif,
        "ei_balance": ei,
        "activity": activity,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Compute AARNN biomimicry calibration metrics.")
    parser.add_argument("--snapshot", required=True, help="Path to network snapshot JSON")
    parser.add_argument(
        "--spikes",
        default=None,
        help="Optional spike log path (CSV/JSON with time_ms,node_id fields)",
    )
    parser.add_argument(
        "--max-triad-nodes",
        type=int,
        default=384,
        help="Maximum hidden nodes used for motif counting (default: 384)",
    )
    parser.add_argument(
        "--spike-bin-ms",
        type=float,
        default=5.0,
        help="Bin width in ms for population-band metrics when --spikes is provided",
    )
    parser.add_argument("--output", default=None, help="Optional output JSON path")
    parser.add_argument("--pretty", action="store_true", help="Pretty-print JSON output")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    snapshot_path = Path(args.snapshot)
    if not snapshot_path.exists():
        raise SystemExit(f"Missing snapshot file: {snapshot_path}")

    spikes = None
    if args.spikes:
        spikes_path = Path(args.spikes)
        if not spikes_path.exists():
            raise SystemExit(f"Missing spikes file: {spikes_path}")
        spikes = parse_spikes(spikes_path)

    snapshot = load_snapshot(snapshot_path)
    metrics = compute_metrics(
        snapshot=snapshot,
        max_triad_nodes=max(8, int(args.max_triad_nodes)),
        spikes=spikes,
        spike_bin_ms=float(args.spike_bin_ms),
    )
    metrics["snapshot_path"] = str(snapshot_path)
    if args.spikes:
        metrics["spikes_path"] = str(Path(args.spikes))

    encoded = json.dumps(metrics, indent=2 if args.pretty else None)
    if args.output:
        out_path = Path(args.output)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(encoded + ("\n" if args.pretty else ""), encoding="utf-8")
    else:
        print(encoded)


if __name__ == "__main__":
    main()

