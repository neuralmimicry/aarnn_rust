#!/usr/bin/env python3
"""Validate live hierarchical placement telemetry from the local example."""

from __future__ import annotations

import argparse
import json
import time
from typing import Any
from urllib.parse import urlencode
from urllib.request import urlopen


def status(base_url: str, orchestrator: str) -> dict[str, Any]:
    query = urlencode({"addr": orchestrator})
    with urlopen(f"{base_url}/api/status?{query}", timeout=3) as response:  # nosec B310
        payload = json.loads(response.read())
    if not isinstance(payload, dict):
        raise RuntimeError("/api/status did not return an object")
    return payload


def verify(payload: dict[str, Any], require_multi_host: bool) -> dict[str, Any] | None:
    for network in payload.get("networks", []):
        records = [
            record
            for record in network.get("hierarchical_shards", [])
            if isinstance(record, dict) and record.get("sub_shards")
        ]
        if not records:
            continue
        active_records = [
            record
            for record in records
            if str(record.get("role", "active")).lower() != "backup"
        ]
        backup_records = [
            record
            for record in records
            if str(record.get("role", "active")).lower() == "backup"
        ]
        areas = {int(record.get("area_id", 0)) for record in active_records}
        labels = {str(record.get("area_label", "")) for record in active_records}
        sub_shards = [
            sub
            for record in active_records
            for sub in record.get("sub_shards", [])
            if isinstance(sub, dict)
        ]
        backup_sub_shards = [
            sub
            for record in backup_records
            for sub in record.get("sub_shards", [])
            if isinstance(sub, dict)
        ]
        hosts = {
            str(sub.get("active_node", ""))
            for sub in sub_shards
            if sub.get("active_node")
        }
        backup_hosts = {
            str(sub.get("active_node", ""))
            for sub in backup_sub_shards
            if sub.get("active_node")
        }
        if not areas or 0 in areas:
            raise RuntimeError("hierarchical telemetry contains no stable non-zero area IDs")
        if not labels or "" in labels:
            raise RuntimeError("hierarchical telemetry contains an unlabeled area")
        if not sub_shards:
            raise RuntimeError("hierarchical telemetry contains no sub-shards")
        if not backup_records:
            raise RuntimeError("hierarchical telemetry contains no backup area shards")
        if not backup_sub_shards:
            raise RuntimeError("hierarchical telemetry contains no backup sub-shards")

        active_layers = {
            int(sub.get("layer"))
            for sub in sub_shards
            if isinstance(sub.get("layer"), int)
        }
        backup_layers = {
            int(sub.get("layer"))
            for sub in backup_sub_shards
            if isinstance(sub.get("layer"), int)
        }
        if active_layers and not active_layers.issubset(backup_layers):
            missing = sorted(active_layers - backup_layers)
            raise RuntimeError(
                f"hierarchical backup telemetry does not cover active layers: {missing}"
            )

        active_hosts_by_layer: dict[int, set[str]] = {}
        backup_hosts_by_layer: dict[int, set[str]] = {}
        for sub in sub_shards:
            layer = sub.get("layer")
            host = str(sub.get("active_node", "")).strip()
            if isinstance(layer, int) and host:
                active_hosts_by_layer.setdefault(layer, set()).add(host)
        for sub in backup_sub_shards:
            layer = sub.get("layer")
            host = str(sub.get("active_node", "")).strip()
            if isinstance(layer, int) and host:
                backup_hosts_by_layer.setdefault(layer, set()).add(host)

        if len(hosts | backup_hosts) > 1:
            missing_anti_affinity = {
                layer
                for layer, active_layer_hosts in active_hosts_by_layer.items()
                if not backup_hosts_by_layer.get(layer, set()) - active_layer_hosts
            }
            if missing_anti_affinity:
                raise RuntimeError(
                    "hierarchical backups share every active host for layers: "
                    f"{sorted(missing_anti_affinity)}"
                )
        if require_multi_host and len(hosts) < 2:
            raise RuntimeError(f"hierarchical telemetry is not distributed across hosts: {sorted(hosts)}")
        return {
            "network_id": network.get("network_id"),
            "areas": len(active_records),
            "area_labels": sorted(labels),
            "sub_shards": len(sub_shards),
            "hosts": sorted(hosts),
            "backup_areas": len(backup_records),
            "backup_sub_shards": len(backup_sub_shards),
            "backup_hosts": sorted(backup_hosts),
        }
    return None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--orchestrator", required=True)
    parser.add_argument("--timeout", type=float, default=50.0)
    parser.add_argument("--allow-single-host", action="store_true")
    args = parser.parse_args()
    deadline = time.monotonic() + args.timeout
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            result = verify(
                status(args.base_url, args.orchestrator),
                require_multi_host=not args.allow_single_host,
            )
            if result:
                print(
                    "[verify] hierarchical placement: "
                    f"{result['network_id']} · {result['areas']} areas · "
                    f"{result['sub_shards']} sub-shards · hosts {result['hosts']} · "
                    f"{result['backup_areas']} backup areas · "
                    f"{result['backup_sub_shards']} backup sub-shards · "
                    f"backup hosts {result['backup_hosts']}",
                    flush=True,
                )
                return 0
        except Exception as error:  # startup and worker registration are transient
            last_error = error
        time.sleep(0.2)
    detail = f"; last error: {last_error}" if last_error else ""
    raise SystemExit(f"timed out waiting for live hierarchical placement telemetry{detail}")


if __name__ == "__main__":
    main()
