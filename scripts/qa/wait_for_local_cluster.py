#!/usr/bin/env python3
"""Wait for a single-brain local cluster to appear in the dashboard status API.

This checks cluster membership and hosted-brain identity, not production
consensus, durable ownership, or cross-tenant authorisation. It is intended
for bounded local launchers and their smoke tests.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import sys
import time
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.parse import urlencode, urlsplit
from urllib.request import urlopen


class HierarchyContractError(RuntimeError):
    """The status response contradicts the launcher's declared hierarchy."""


def _normalise_address(value: Any) -> str:
    """Compare advertised addresses without an optional URL scheme or slash."""

    address = str(value or "").strip()
    if not address:
        return ""
    parsed = urlsplit(address if "://" in address else f"//{address}")
    return parsed.netloc.rstrip("/") if parsed.netloc else address.rstrip("/")


def inspect_status(
    payload: dict[str, Any], brain_id: str, workers: dict[str, str]
) -> tuple[bool, str]:
    """Return readiness and a useful transient status explanation.

    A worker is ready only after the orchestrator reports both its stable
    ``NodeId`` and the intended hosted ``BrainId``. Network IDs matching a
    worker ID indicate the exact identity mix-up this launcher guards against.
    """

    networks = payload.get("networks")
    nodes = payload.get("nodes")
    if not isinstance(networks, list) or not isinstance(nodes, list):
        return False, "status response has no network or node lists"

    network_ids = {
        str(network.get("network_id", "")).strip()
        for network in networks
        if isinstance(network, dict)
    }
    phantom_brains = sorted(set(workers) & network_ids)
    if phantom_brains:
        raise HierarchyContractError(
            "worker node IDs were registered as separate brain IDs: "
            + ", ".join(phantom_brains)
        )

    unexpected_networks = sorted(network_ids - {brain_id})
    if unexpected_networks:
        raise HierarchyContractError(
            f"expected only brain {brain_id!r}; status also reports: "
            + ", ".join(unexpected_networks)
        )
    if brain_id not in network_ids:
        return False, f"waiting for brain {brain_id!r} to register"

    node_by_id = {
        str(node.get("node_id", "")).strip(): node
        for node in nodes
        if isinstance(node, dict)
    }
    waiting: list[str] = []
    for node_id, expected_address in workers.items():
        node = node_by_id.get(node_id)
        if node is None:
            waiting.append(f"{node_id} membership")
            continue
        if _normalise_address(node.get("address")) != _normalise_address(expected_address):
            waiting.append(f"{node_id} advertised address")
            continue
        active_networks = node.get("active_networks")
        if not isinstance(active_networks, list) or brain_id not in active_networks:
            waiting.append(f"{node_id} hosted brain {brain_id}")

    if waiting:
        return False, "waiting for " + ", ".join(waiting)

    return True, (
        f"brain {brain_id} hosted by "
        + ", ".join(f"{node_id}@{_normalise_address(address)}" for node_id, address in workers.items())
    )


def fetch_status(base_url: str, orchestrator: str) -> dict[str, Any]:
    """Fetch one bounded dashboard projection from the selected orchestrator."""

    query = urlencode({"addr": orchestrator})
    with urlopen(f"{base_url.rstrip('/')}/api/status?{query}", timeout=3) as response:  # nosec B310 - local launcher endpoint
        payload = json.loads(response.read())
    if not isinstance(payload, dict):
        raise ValueError("/api/status did not return a JSON object")
    return payload


def _parse_worker(raw: str) -> tuple[str, str]:
    node_id, separator, address = raw.partition("=")
    node_id, address = node_id.strip(), address.strip()
    if not separator or not node_id or not address:
        raise argparse.ArgumentTypeError("workers must be supplied as NODE_ID=HOST:PORT")
    return node_id, address


def _parse_process(raw: str) -> tuple[str, int]:
    node_id, separator, pid_raw = raw.partition("=")
    try:
        pid = int(pid_raw)
    except ValueError as error:
        raise argparse.ArgumentTypeError("processes must be supplied as NODE_ID=PID") from error
    if not separator or not node_id.strip() or pid <= 0:
        raise argparse.ArgumentTypeError("processes must be supplied as NODE_ID=positive-PID")
    return node_id.strip(), pid


def _process_is_running(node_id: str, pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        raise RuntimeError(f"worker {node_id} process {pid} exited before joining")
    except PermissionError:
        return True
    return True


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", required=True, help="Local web dashboard URL")
    parser.add_argument("--orchestrator", required=True, help="Orchestrator gRPC address")
    parser.add_argument("--brain-id", required=True, help="The one brain this local cluster hosts")
    parser.add_argument("--worker", action="append", type=_parse_worker, default=[], metavar="NODE=HOST:PORT")
    parser.add_argument("--pid", action="append", type=_parse_process, default=[], metavar="NODE=PID")
    parser.add_argument("--timeout", type=float, default=45.0)
    parser.add_argument("--poll-interval", type=float, default=0.5)
    args = parser.parse_args()

    if not args.worker:
        parser.error("at least one --worker is required")
    if not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error("--timeout must be a finite positive number")
    if not math.isfinite(args.poll_interval) or args.poll_interval <= 0:
        parser.error("--poll-interval must be a finite positive number")
    workers = dict(args.worker)
    if len(workers) != len(args.worker):
        parser.error("worker node IDs must be unique")
    processes = dict(args.pid)
    if len(processes) != len(args.pid) or set(processes) - set(workers):
        parser.error("each --pid must name one unique declared worker")

    deadline = time.monotonic() + args.timeout
    last_problem = "no status response yet"
    while time.monotonic() < deadline:
        try:
            for node_id, pid in processes.items():
                _process_is_running(node_id, pid)
            ready, last_problem = inspect_status(
                fetch_status(args.base_url, args.orchestrator), args.brain_id, workers
            )
            if ready:
                print(last_problem, flush=True)
                return 0
        except HierarchyContractError as error:
            raise SystemExit(str(error)) from error
        except (HTTPError, URLError, TimeoutError, OSError, ValueError, json.JSONDecodeError) as error:
            last_problem = str(error)
        except RuntimeError as error:
            raise SystemExit(str(error)) from error
        time.sleep(args.poll_interval)

    raise SystemExit(f"local cluster did not become ready within {args.timeout:g}s: {last_problem}")


if __name__ == "__main__":
    sys.exit(main())
