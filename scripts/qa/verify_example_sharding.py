#!/usr/bin/env python3
"""Bounded local verification for the compatibility example sharder.

The helper accepts the legacy layer projection for compatibility, but prefers
the active hierarchical area/layer/sub-shard telemetry when it is present.
It checks both the orchestrator projection and the destination worker's
returned snapshot after a worker failure.
"""

from __future__ import annotations

import argparse
import json
import os
import signal
import sys
import time
from pathlib import Path
from typing import Any, Callable
from urllib.parse import urlencode
from urllib.request import urlopen


POLL_INTERVAL_S = 0.05


def snapshot_total(snapshot: dict[str, Any]) -> int:
    """Return the biological neuron count represented by a Runner snapshot."""

    net = snapshot.get("net", {})
    hidden = [
        int(matrix["rows"])
        for matrix in snapshot.get("w_hh_rec", [])
        if isinstance(matrix, dict) and "rows" in matrix
    ]
    if not hidden:
        hidden = [
            int(net.get("num_hidden_per_layer_initial", 0))
        ] * int(net.get("num_hidden_layers", 0))
    output = int(net.get("num_output_neurons", 0))
    if isinstance(snapshot.get("w_out"), dict):
        output = int(snapshot["w_out"].get("rows", output))
    sensory = int(net.get("num_sensory_neurons", 0))
    return sensory + sum(hidden) + output


def get_json(url: str) -> dict[str, Any]:
    with urlopen(url, timeout=3) as response:  # nosec B310 - local URL is supplied by launcher
        payload = response.read()
    value = json.loads(payload)
    if not isinstance(value, dict):
        raise RuntimeError(f"expected an object from {url}")
    return value


def status(base_url: str, orchestrator: str) -> dict[str, Any]:
    query = urlencode({"addr": orchestrator})
    return get_json(f"{base_url}/api/status?{query}")


def cluster_summary(payload: dict[str, Any], network_id: str) -> dict[str, Any]:
    network = next(
        (item for item in payload.get("networks", []) if item.get("network_id") == network_id),
        None,
    )
    if network is None:
        raise RuntimeError(f"network {network_id!r} is not present in /api/status")

    distribution = network.get("distribution", [])
    hierarchical = network.get("hierarchical_shards", [])
    active = [entry for entry in distribution if entry.get("layers")]
    owners_by_layer: dict[int, str] = {}
    hierarchical_active_nodes: set[str] = set()
    for area in hierarchical if isinstance(hierarchical, list) else []:
        for sub_shard in area.get("sub_shards", []):
            node_id = str(sub_shard.get("active_node", "")).strip()
            if not node_id:
                continue
            hierarchical_active_nodes.add(node_id)
            layer = int(sub_shard.get("layer", -1))
            if layer < 0:
                continue
            if layer in owners_by_layer and owners_by_layer[layer] != node_id:
                raise RuntimeError(f"layer {layer} has multiple active hierarchical owners")
            owners_by_layer[layer] = node_id
    for entry in active:
        for layer in entry.get("layers", []):
            layer = int(layer)
            if layer in owners_by_layer and owners_by_layer[layer] != entry["node_id"]:
                raise RuntimeError(f"layer {layer} has multiple active owners")
            owners_by_layer[layer] = entry["node_id"]

    expected_layers = int(network.get("num_layers", 0))
    return {
        "total": int(network.get("total_neurons", 0)),
        "expected_layers": expected_layers,
        "owners_by_layer": owners_by_layer,
        "active_nodes": sorted(hierarchical_active_nodes or {entry["node_id"] for entry in active}),
        "distribution": distribution,
    }


def wait_for(
    timeout_s: float,
    description: str,
    predicate: Callable[[], Any],
) -> Any:
    deadline = time.monotonic() + timeout_s
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            result = predicate()
            if result:
                return result
        except Exception as error:  # startup and reconnect are expected to be transient
            last_error = error
        time.sleep(POLL_INTERVAL_S)
    detail = f"; last error: {last_error}" if last_error else ""
    raise RuntimeError(f"timed out waiting for {description}{detail}")


def make_fixture(source: Path, destination: Path, headroom: int) -> int:
    with source.open(encoding="utf-8") as stream:
        snapshot = json.load(stream)
    if not isinstance(snapshot, dict) or not isinstance(snapshot.get("net"), dict):
        raise RuntimeError(f"{source} is not a Runner snapshot")

    initial = snapshot_total(snapshot)
    net = snapshot["net"]
    net.update(
        {
            "growth_enabled": True,
            "max_total_neurons": initial + max(headroom, 2),
            "development_growth_interval_ms": 1.0,
            "global_growth_cooldown_ms": 0.0,
            "spontaneous_neuron_interval_ms": 0.0,
            "growth_cooldown_ms": 0.0,
            # Keep this probe within the current layer-assignment contract.
            # A layer split is a separate topology transaction and is not part
            # of this relocation/growth regression.
            "layer_split_threshold": 1_000_000,
        }
    )
    destination.parent.mkdir(parents=True, exist_ok=True)
    with destination.open("w", encoding="utf-8") as stream:
        json.dump(snapshot, stream, separators=(",", ":"))
    return initial


def fetch_snapshot_total(base_url: str, orchestrator: str, network_id: str, node_id: str) -> int:
    query = urlencode(
        {"network_id": network_id, "addr": orchestrator, "node_id": node_id}
    )
    response = get_json(f"{base_url}/api/snapshot?{query}")
    snapshot_json = response.get("snapshot_json")
    if not isinstance(snapshot_json, str):
        raise RuntimeError("snapshot response did not contain snapshot_json")
    return snapshot_total(json.loads(snapshot_json))


def verify(args: argparse.Namespace) -> None:
    def current() -> dict[str, Any]:
        return cluster_summary(status(args.base_url, args.orchestrator), args.network_id)

    sharded = wait_for(
        args.timeout,
        "a complete multi-node shard assignment",
        lambda: (
            summary
            if (summary := current())["expected_layers"] > 0
            and len(summary["active_nodes"]) >= 2
            and len(summary["owners_by_layer"]) == summary["expected_layers"]
            else None
        ),
    )
    baseline = sharded["total"]
    print(
        f"[verify] sharded {args.network_id}: {baseline} neurons across "
        f"{sharded['active_nodes']} owners",
        flush=True,
    )

    source_layer, source_node = sorted(sharded["owners_by_layer"].items())[0]
    survivor = next(node for node in sharded["active_nodes"] if node != source_node)
    survivor_before = wait_for(
        args.timeout,
        "a readable survivor snapshot before relocation",
        lambda: (
            total
            if (total := fetch_snapshot_total(
                args.base_url, args.orchestrator, args.network_id, survivor
            )) > 0
            else None
        ),
    )
    pid = args.node1_pid if source_node.startswith("node_1") else args.node2_pid
    if pid <= 0:
        raise RuntimeError(f"no launcher PID is available for active owner {source_node}")
    try:
        os.kill(pid, signal.SIGTERM)
    except ProcessLookupError as error:
        raise RuntimeError(f"active owner {source_node} exited before relocation") from error
    print(
        f"[verify] stopped {source_node} (layer {source_layer}); survivor "
        f"snapshot was {survivor_before} "
        f"to force relocation",
        flush=True,
    )

    wait_for(
        args.timeout,
        f"relocation of all active layers to {survivor}",
        lambda: (
            summary
            if (summary := current())["active_nodes"] == [survivor]
            and len(summary["owners_by_layer"]) == summary["expected_layers"]
            else None
        ),
    )
    after_relocation = wait_for(
        args.timeout,
        "a relocated snapshot that retains the survivor topology and has growth headroom",
        lambda: (
            total
            if (total := fetch_snapshot_total(
                args.base_url, args.orchestrator, args.network_id, survivor
            )) > survivor_before
            else None
        ),
    )
    continued = wait_for(
        args.timeout,
        "continued growth on the relocated shard",
        lambda: (
            total
            if (total := fetch_snapshot_total(
                args.base_url, args.orchestrator, args.network_id, survivor
            )) > after_relocation
            else None
        ),
    )
    print(
        f"[verify] relocated to {survivor}: snapshot retained {after_relocation} "
        f"and grew after relocation to {continued}",
        flush=True,
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--make-fixture", nargs=2, metavar=("SOURCE", "DESTINATION"))
    parser.add_argument("--headroom", type=int, default=256)
    parser.add_argument("--base-url", default="http://127.0.0.1:8080")
    parser.add_argument("--orchestrator", default="127.0.0.1:50051")
    parser.add_argument("--network-id", default="cluster_master")
    parser.add_argument("--node1-pid", type=int, default=0)
    parser.add_argument("--node2-pid", type=int, default=0)
    parser.add_argument("--timeout", type=float, default=50.0)
    args = parser.parse_args()

    try:
        if args.make_fixture:
            initial = make_fixture(
                Path(args.make_fixture[0]), Path(args.make_fixture[1]), args.headroom
            )
            print(initial)
            return 0
        verify(args)
        return 0
    except Exception as error:
        print(f"[verify] FAILED: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
