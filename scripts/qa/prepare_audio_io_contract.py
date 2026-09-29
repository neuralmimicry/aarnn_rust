#!/usr/bin/env python3
"""Prepare a run-local sensory I/O contract for an interactive I/O example.

The workstation may create a bounded local audio provider while a managed
snapshot still has no sensory inputs. This helper adds the requested width only
when neither the config contract nor the network snapshot already declares a
positive width. It never edits either source file.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


def load_json(path: Path) -> dict[str, Any] | None:
    """Load one JSON object when its optional path exists."""
    if not path.is_file():
        return None
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"expected a JSON object in {path}")
    return value


def network_config(document: dict[str, Any] | None) -> dict[str, Any]:
    """Return a detached network config from either a config or snapshot DTO."""
    if document is None:
        return {}
    nested = document.get("net")
    source = nested if isinstance(nested, dict) else document
    return dict(source)


def prepare_audio_io_contract(
    config_document: dict[str, Any] | None,
    network_document: dict[str, Any] | None,
    requested_width: int,
) -> tuple[dict[str, Any], bool]:
    """Build a config contract, seeding sensory width only when both are zero."""
    snapshot = network_config(network_document)
    config = (
        network_config(config_document)
        if config_document is not None
        else dict(snapshot)
    )
    configured_width = int(config.get("num_sensory_neurons", 0) or 0)
    snapshot_width = int(snapshot.get("num_sensory_neurons", 0) or 0)
    if configured_width <= 0 and snapshot_width <= 0:
        config["num_sensory_neurons"] = requested_width
        return config, True
    return config, False


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--network", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--sensory-neurons", type=int, required=True)
    args = parser.parse_args()
    if not 1 <= args.sensory_neurons <= 65_536:
        parser.error("--sensory-neurons must be in the range 1..65536")

    contract, seeded = prepare_audio_io_contract(
        load_json(args.config), load_json(args.network), args.sensory_neurons
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(contract, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    if seeded:
        print(f"Seeded zero-width network with {args.sensory_neurons} sensory inputs")
    else:
        print(
            "Preserved existing sensory width "
            f"{contract.get('num_sensory_neurons', 0)} from the configured network"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
