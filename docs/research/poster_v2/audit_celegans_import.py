#!/usr/bin/env python3
"""Independent audit of the C. elegans import used on the A0 poster (v2).

Re-parses the legacy encoded wiring in ``celegans.py`` (signed integer weights,
including negative/inhibitory entries) and compares every pair against the
generated snapshot ``network_celegans.json`` written by
``scripts/build_celegans_network_json.py``.

Usage:  python3 -I docs/research/poster_v2/audit_celegans_import.py [repo_root]
Writes: docs/research/poster_v2/celegans_audit.json
"""
import json
import re
import sys
from collections import defaultdict
from pathlib import Path

root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[3]
src = (root / "celegans.py").read_text(encoding="utf-8")
snap = json.loads((root / "network_celegans.json").read_text(encoding="utf-8"))

DEF_RE = re.compile(r"^def\s+([A-Za-z0-9_]+)\s*\(")
EDGE_RE = re.compile(r"postsynaptic\['([^']+)'\]\s*\+=\s*(-?\s*[0-9]+)")

pairs = defaultdict(int)  # (pre, post) -> signed summed weight
current = None
for line in src.splitlines():
    s = line.strip()
    if s.startswith("#"):
        continue
    m = DEF_RE.match(line)
    if m:
        current = m.group(1)
        continue
    if current is None:
        continue
    for post, w in EDGE_RE.findall(line):
        pairs[(current, post)] += int(w.replace(" ", ""))

labels = snap["connectome_labels"]
hidden = labels["hidden_nodes"]
muscles = labels["output_nodes"]
h_idx = {n: i for i, n in enumerate(hidden)}
m_idx = {n: i for i, n in enumerate(muscles)}


def dense(mat):
    rows, cols, data = mat["rows"], mat["cols"], mat["data"]
    return [data[r * cols:(r + 1) * cols] for r in range(rows)]


rec = dense(snap["w_hh_rec"][0])  # rows = post, cols = pre (verified below)
out = dense(snap["w_out"])         # rows = muscle, cols = pre neuron

res = {
    "source_pairs_signed": 0, "source_pairs_positive": 0, "source_pairs_negative": 0,
    "nn_pairs_source": 0, "nm_pairs_source_positive": 0, "nm_pairs_source_negative": 0,
    "nn_negative_pairs_source": 0,
    "positive_pairs_matched": 0, "positive_pairs_mismatched": 0,
    "negative_pairs_present_in_snapshot": 0, "negative_pairs_dropped": 0,
    "negative_weight_sum_dropped": 0, "negative_presynaptic_cells": [],
    "unknown_targets": [],
}
points = {"nn": [], "nm_kept": [], "nm_dropped": []}
neg_pre = set()
for (pre, post), w in sorted(pairs.items()):
    if w == 0:
        continue
    res["source_pairs_signed"] += 1
    if pre not in h_idx:
        res["unknown_targets"].append([pre, post])
        continue
    j = h_idx[pre]
    if post in h_idx:
        i = h_idx[post]
        res["nn_pairs_source"] += 1
        got = rec[i][j]
        if w < 0:
            res["nn_negative_pairs_source"] += 1
    elif post in m_idx:
        i = m_idx[post]
        got = out[i][j]
        if w > 0:
            res["nm_pairs_source_positive"] += 1
        else:
            res["nm_pairs_source_negative"] += 1
    else:
        res["unknown_targets"].append([pre, post])
        continue
    if w > 0:
        res["source_pairs_positive"] += 1
        if abs(got - w) < 1e-6:
            res["positive_pairs_matched"] += 1
        else:
            res["positive_pairs_mismatched"] += 1
        (points["nn"] if post in h_idx else points["nm_kept"]).append([j, (h_idx.get(post, None) if post in h_idx else len(hidden) + m_idx[post])])
    else:
        res["source_pairs_negative"] += 1
        if abs(got - w) < 1e-6:
            res["negative_pairs_present_in_snapshot"] += 1
        else:
            res["negative_pairs_dropped"] += 1
            res["negative_weight_sum_dropped"] += w
            neg_pre.add(pre)
            points["nm_dropped" if post in m_idx else "nn"].append([j, (len(hidden) + m_idx[post]) if post in m_idx else h_idx[post]])

# Any snapshot entries not explained by the source?
extra = 0
for i, row in enumerate(rec):
    for j, v in enumerate(row):
        if v and pairs.get((hidden[j], hidden[i]), 0) != v:
            extra += 1
for i, row in enumerate(out):
    for j, v in enumerate(row):
        if v and pairs.get((hidden[j], muscles[i]), 0) != v:
            extra += 1
res["snapshot_entries_not_in_source"] = extra
res["snapshot_nonzero_rec"] = sum(1 for row in rec for v in row if v)
res["snapshot_nonzero_out"] = sum(1 for row in out for v in row if v)
res["snapshot_negative_entries"] = sum(1 for row in rec + out for v in row if v < 0)
res["negative_presynaptic_cells"] = sorted(neg_pre)
res["hidden_labels"] = len(hidden)
res["muscle_readouts"] = len(muscles)
res["sensory_channels"] = len(labels["sensory_nodes"])
res["isolated_hidden_nodes_added"] = labels.get("isolated_hidden_nodes_added")
res["snapshot_edge_count_field"] = labels.get("edge_count")
res["snapshot_total_weight_field"] = labels.get("total_weight")
res["points"] = points

outp = Path(__file__).with_name("celegans_audit.json")
outp.write_text(json.dumps(res, indent=1), encoding="utf-8")
summary = {k: v for k, v in res.items() if k != "points"}
summary["negative_presynaptic_cells"] = len(neg_pre)
print(json.dumps(summary, indent=1))
