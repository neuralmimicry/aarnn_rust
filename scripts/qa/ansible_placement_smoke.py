#!/usr/bin/env python3
"""Run a read-only Ansible inventory probe through the local placement planner.

This harness is deliberately an adapter, not a second scheduler. Ansible
collects bounded host facts over SSH; Rust remains the sole owner of placement
validation, scoring, canonical serialisation and digest generation. A host is
included in the compute set only when it is named explicitly with
``--grant-compute``. Reachability, discovery or a GPU report never grants
authority.

The probe does not install packages, change services, write remote files,
restart hosts or call an orchestrator mutation API. It is suitable for
repeated hardware QA against the SwarmHPC inventory.
"""

from __future__ import annotations

import argparse
import json
import math
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
from typing import Any


FACT_COMMAND = r'''set +e
cpu=$(getconf _NPROCESSORS_ONLN 2>/dev/null || printf '0')
mem=$(awk '/^MemTotal:/{print $2*1024}' /proc/meminfo)
mem_available=$(awk '/^MemAvailable:/{print $2*1024}' /proc/meminfo)
load_milli=$(awk '{printf "%d", $1*1000}' /proc/loadavg)
root_available=$(df -PB1 / | awk 'NR==2 {print $4}')
temp_raw=$(for path in /sys/class/thermal/thermal_zone*/temp; do
  [ -r "$path" ] && cat "$path"
done | awk '{if ($1 > max) max=$1} END {print max+0}')
if [ "$temp_raw" -gt 0 ] 2>/dev/null; then thermal_milli=$((temp_raw / 100)); else thermal_milli=1000; fi
speed_mbps=$(for path in /sys/class/net/*/speed; do
  [ -r "$path" ] && cat "$path" 2>/dev/null
done | awk '$1 > max {max=$1} END {print (max ? max : 0)}')
if [ "$speed_mbps" -le 0 ] 2>/dev/null; then
  interface=$(ip route get 1.1.1.1 2>/dev/null | awk '{for (i=1; i<=NF; i++) if ($i == "dev") {print $(i+1); exit}}')
  if command -v iw >/dev/null 2>&1 && [ -n "$interface" ]; then
    speed_mbps=$(iw dev "$interface" link 2>/dev/null | awk '/tx bitrate:/ {print int($3); exit}')
  fi
fi
if [ "$speed_mbps" -le 0 ] 2>/dev/null && command -v ethtool >/dev/null 2>&1 && [ -n "$interface" ]; then
  speed_mbps=$(ethtool "$interface" 2>/dev/null | awk -F': ' '/Speed:/ {gsub("Mb/s", "", $2); print int($2); exit}')
fi
gpu=$(nvidia-smi --query-gpu=name,memory.total,driver_version --format=csv,noheader,nounits 2>/dev/null | tr '\n' ';')
printf 'host=%s\n' "$(hostname)"
printf 'architecture=%s\n' "$(uname -m)"
printf 'capacity_units=%s\n' "$((cpu * 1000))"
printf 'memory_bytes=%s\n' "$mem"
printf 'memory_available_bytes=%s\n' "$mem_available"
printf 'load_milli=%s\n' "$load_milli"
printf 'root_available_bytes=%s\n' "$root_available"
printf 'thermal_pressure_milli=%s\n' "$thermal_milli"
printf 'network_speed_mbps=%s\n' "$speed_mbps"
printf 'gpu=%s\n' "$gpu"
'''

SUCCESS_LINE = re.compile(r"^(?P<host>\S+) \| (?:SUCCESS|CHANGED) \|.*\(stdout\) (?P<stdout>.*)$")


def run(command: list[str], *, cwd: Path, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    """Run one bounded local command and retain stderr for diagnosis."""

    return subprocess.run(
        command,
        cwd=cwd,
        env=env,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def probe(ansible_dir: Path, inventory: Path, selected: str, timeout: int) -> tuple[dict[str, dict[str, Any]], list[str]]:
    """Collect facts through Ansible's SSH transport and return exclusions."""

    env = os.environ.copy()
    env.setdefault("ANSIBLE_CONFIG", str(ansible_dir / "ansible.cfg"))
    result = run(
        [
            "ansible",
            "-i",
            str(inventory),
            selected,
            "-m",
            "shell",
            "-a",
            FACT_COMMAND,
            "-o",
            "--forks",
            "16",
            "--timeout",
            str(timeout),
        ],
        cwd=ansible_dir,
        env=env,
    )
    facts: dict[str, dict[str, Any]] = {}
    for line in result.stdout.splitlines():
        match = SUCCESS_LINE.match(line)
        if not match:
            continue
        values: dict[str, Any] = {}
        escaped = match.group("stdout").encode("utf-8").decode("unicode_escape")
        for item in escaped.splitlines():
            key, separator, value = item.partition("=")
            if separator:
                values[key] = value
        if values.get("host"):
            facts[match.group("host")] = values

    requested = [host.strip() for host in selected.split(",") if host.strip()]
    excluded = sorted(set(requested) - set(facts))
    if result.returncode != 0 and not facts:
        raise RuntimeError(
            "Ansible could not collect facts from any selected host:\n"
            + (result.stdout + result.stderr).strip()
        )
    return facts, excluded


def integer(values: dict[str, Any], key: str, minimum: int = 0) -> int:
    value = int(str(values.get(key, "0")).strip() or "0")
    if value < minimum:
        raise ValueError(f"{key} must be at least {minimum}, got {value}")
    return value


def build_request(
    facts: dict[str, dict[str, Any]],
    granted: set[str],
    shard_count: int,
    maximum_thermal_pressure: int,
    minimum_warm_replicas: int,
    allow_single_host_degraded_durability: bool,
    consolidate_to: str | None,
) -> dict[str, Any]:
    resources: list[dict[str, Any]] = []
    for node_id in sorted(facts):
        values = facts[node_id]
        total_memory = integer(values, "memory_bytes", 1)
        available_memory = min(integer(values, "memory_available_bytes"), total_memory)
        capacity = integer(values, "capacity_units", 1)
        storage = integer(values, "root_available_bytes", 1)
        network_mbps = integer(values, "network_speed_mbps")
        network_bps = network_mbps * 125_000
        resources.append(
            {
                "node_id": node_id,
                "device_id": f"{node_id}-cpu",
                "healthy": True,
                "enrolled": node_id in granted,
                "compute_authorised": node_id in granted,
                # Host-level anti-affinity only; this does not claim separate
                # racks, power feeds or other independent failure domains.
                "failure_domain": f"host:{node_id}",
                "numerical_profiles": ["reference-cpu-v1"],
                "capacity_units": capacity,
                "reserved_capacity_units": math.ceil(capacity * 0.15),
                "memory_bytes": total_memory,
                "reserved_memory_bytes": total_memory - available_memory + math.ceil(total_memory * 0.10),
                "storage_bytes": storage,
                "reserved_storage_bytes": math.ceil(storage * 0.10),
                "network_bytes_per_second": network_bps,
                "reserved_network_bytes_per_second": math.ceil(network_bps * 0.20),
                "cpu_pressure_milli": min(
                    1000,
                    integer(values, "load_milli") // max(1, integer(values, "capacity_units") // 1000),
                ),
                "memory_pressure_milli": min(1000, ((total_memory - available_memory) * 1000) // total_memory),
                "network_pressure_milli": 100,
                "thermal_pressure_milli": min(1000, integer(values, "thermal_pressure_milli")),
            }
        )

    demands = [
        {
            "shard_id": shard,
            "load_units": 1000,
            "memory_bytes": 256 * 1024 * 1024,
            "checkpoint_bytes": 512 * 1024 * 1024,
            "network_bytes_per_second": 1_000_000,
            "zero_delay_component": None,
            "required_numerical_profile": "reference-cpu-v1",
            "preferred_node": None,
        }
        for shard in range(1, shard_count + 1)
    ]
    intent: dict[str, Any] = {"Automatic": None}
    if consolidate_to:
        intent = {"Consolidate": {"target_node": consolidate_to}}
    return {
        "brain_id": 42,
        "topology_generation": 1,
        "partition_generation": 1,
        "lease_term": 1,
        "fencing_token": 1,
        "effective_tag": {"tick": 0, "microstep": 0},
        "demands": demands,
        "resources": resources,
        "constraints": {
            "minimum_headroom_milli": 150,
            "maximum_cpu_pressure_milli": 850,
            "maximum_memory_pressure_milli": 850,
            "maximum_network_pressure_milli": 850,
            "maximum_thermal_pressure_milli": maximum_thermal_pressure,
            "minimum_warm_replicas": minimum_warm_replicas,
            "require_distinct_warm_failure_domain": True,
            "allow_single_host_degraded_durability": allow_single_host_degraded_durability,
        },
        "intent": intent,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ansible-dir", type=Path, required=True)
    parser.add_argument("--inventory", type=Path, required=True)
    parser.add_argument(
        "--hosts",
        default="localhost,qc00,qc01,qc02,qc03,qc04,qc05,sm00,sm01",
        help="comma-separated inventory hosts to probe",
    )
    parser.add_argument(
        "--grant-compute",
        required=True,
        help="comma-separated hosts with an explicit enrolled compute grant",
    )
    parser.add_argument("--shards", type=int, default=8)
    parser.add_argument("--maximum-thermal-pressure", type=int, default=800)
    parser.add_argument("--minimum-warm-replicas", type=int, default=1)
    parser.add_argument("--allow-single-host-degraded-durability", action="store_true")
    parser.add_argument("--consolidate-to", help="co-locate all requested shards on this node")
    parser.add_argument("--timeout", type=int, default=8)
    parser.add_argument("--keep-request", type=Path)
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parents[2]
    granted = {item.strip() for item in args.grant_compute.split(",") if item.strip()}
    if not granted:
        parser.error("--grant-compute must name at least one explicitly authorised host")
    if not 1 <= args.shards <= 256:
        parser.error("--shards must be between 1 and 256")
    if not 0 <= args.maximum_thermal_pressure <= 1000:
        parser.error("--maximum-thermal-pressure must be between 0 and 1000")
    if not 0 <= args.minimum_warm_replicas <= 32:
        parser.error("--minimum-warm-replicas must be between 0 and 32")
    if args.consolidate_to and args.consolidate_to not in granted:
        parser.error("--consolidate-to must also appear in --grant-compute")

    facts, excluded = probe(args.ansible_dir, args.inventory, args.hosts, args.timeout)
    request = build_request(
        facts,
        granted,
        args.shards,
        args.maximum_thermal_pressure,
        args.minimum_warm_replicas,
        args.allow_single_host_degraded_durability,
        args.consolidate_to,
    )
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as handle:
        json.dump(request, handle, indent=2, sort_keys=False)
        handle.write("\n")
        request_path = Path(handle.name)
    try:
        if args.keep_request:
            args.keep_request.parent.mkdir(parents=True, exist_ok=True)
            args.keep_request.write_text(request_path.read_text())
        result = run(
            [
                "cargo",
                "run",
                "--locked",
                "--quiet",
                "--bin",
                "aarnn_rust",
                "--",
                "--placement-request-local-json",
                str(request_path),
            ],
            cwd=repo_root,
        )
        if result.returncode != 0:
            sys.stderr.write(result.stderr)
            sys.stderr.write(result.stdout)
            return result.returncode
        output = json.loads(result.stdout)
        plan = output["plan"]
        active_nodes = sorted({placement["active_node"] for placement in plan["placements"]})
        enrolled_nodes = sorted(node for node in active_nodes if node in granted)
        if not active_nodes:
            raise RuntimeError("planner returned no active placements")
        print(
            json.dumps(
                {
                    "status": "passed",
                    "reachable_hosts": sorted(facts),
                    "excluded_hosts": excluded,
                    "explicit_compute_grants": sorted(granted),
                    "active_nodes": active_nodes,
                    "active_enrolled_nodes": enrolled_nodes,
                    "placement_digest": plan["digest"],
                    "command_digest": output["command_digest"],
                    "degraded_durability": plan["decision"]["degraded_durability"],
                    "applied": output["applied"],
                },
                indent=2,
            )
        )
        return 0
    finally:
        request_path.unlink(missing_ok=True)


if __name__ == "__main__":
    raise SystemExit(main())
