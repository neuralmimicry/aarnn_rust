#!/usr/bin/env python3
"""Shared robot profile and robot-spec parsing helpers for launch scripts."""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, Iterable, List, Tuple


LEGACY_TYPES: Tuple[str, ...] = (
    "celegans",
    "drosophila_banc",
    "drosophila_fafb",
    "hexapod",
    "nao",
    "zebrafish",
)


@dataclass(frozen=True)
class RobotProfile:
    canonical: str
    sensory: int
    output: int
    network_rel: str
    config_rel: str
    aliases: Tuple[str, ...]


PROFILES: Dict[str, RobotProfile] = {
    "celegans": RobotProfile(
        canonical="celegans",
        sensory=24,
        output=96,
        network_rel="network_celegans.json",
        config_rel="webots_world/configs/config_celegans_webots.json",
        aliases=("celegans", "worm", "worms", "c_elegans"),
    ),
    "drosophila_banc": RobotProfile(
        canonical="drosophila_banc",
        sensory=418,
        output=48,
        network_rel="network_drosophila_banc.json",
        config_rel="webots_world/configs/config_drosophila_banc_webots.json",
        aliases=(
            "drosophila",
            "fly",
            "flies",
            "fruitfly",
            "fruitflies",
            "drosophila_banc",
            "banc_drosophila",
            "drosophila_banc_v626",
            "banc",
        ),
    ),
    "drosophila_fafb": RobotProfile(
        canonical="drosophila_fafb",
        sensory=418,
        output=48,
        network_rel="network_drosophila_fafb.json",
        config_rel="webots_world/configs/config_drosophila_fafb_webots.json",
        aliases=("drosophila_fafb", "fafb_drosophila", "drosophila_fafb_v783", "fafb"),
    ),
    "hexapod": RobotProfile(
        canonical="hexapod",
        sensory=34,
        output=18,
        network_rel="network_hexapod.json",
        config_rel="webots_world/configs/config_hexapod_webots.json",
        aliases=("hexapod", "hex", "hexapods", "freenove_hexapod", "big_hexapod", "freenove", "six_legged"),
    ),
    "nao": RobotProfile(
        canonical="nao",
        sensory=250,
        output=40,
        network_rel="network_nao.json",
        config_rel="webots_world/configs/config_nao_webots.json",
        aliases=("nao", "naos"),
    ),
    "zebrafish": RobotProfile(
        canonical="zebrafish",
        sensory=32,
        output=32,
        network_rel="network_zebrafish.json",
        config_rel="webots_world/configs/config_zebrafish_webots.json",
        aliases=("zebrafish", "zebrafishes", "danio", "danio_rerio", "fish", "zfish", "zf"),
    ),
}


# Additive native embodiment registry; the six legacy profiles and their
# trained network paths stay byte-for-byte unchanged. New recipes are untrained.
_EMBODIMENTS = Path(__file__).resolve().parents[1] / "sim/content/embodiments/generated"
_registry = _EMBODIMENTS / "registry.generated.json"
if _registry.is_file():
    for _entry in json.loads(_registry.read_text())["profiles"]:
        _id = _entry["id"]
        if _id in PROFILES:
            continue
        PROFILES[_id] = RobotProfile(_id, _entry["sensory"], _entry["output"], "",
            f"sim/content/embodiments/generated/{_entry['config']}", tuple(_entry["aliases"]))
CANONICAL_TYPES = tuple(PROFILES)


ALIASES: Dict[str, str] = {
    alias: profile.canonical for profile in PROFILES.values() for alias in profile.aliases
}


def normalize_key(raw: str) -> str:
    return re.sub(r"[^a-z0-9]+", "_", raw.strip().lower()).strip("_")


def canonicalize_type(raw: str) -> str:
    key = normalize_key(raw)
    if not key:
        raise ValueError("robot key is empty")
    if key in ALIASES:
        return ALIASES[key]

    if "fafb" in key:
        return "drosophila_fafb"
    if "banc" in key or "drosophila" in key or "fly" in key:
        return "drosophila_banc"
    if "hexapod" in key or "freenove" in key:
        return "hexapod"
    if "zebra" in key or "danio" in key:
        return "zebrafish"
    if key in {"nao", "naos"}:
        return "nao"
    if key in {"worm", "worms", "celegans", "c_elegans"}:
        return "celegans"

    raise ValueError(f"unknown robot type '{raw}'")


def parse_robot_spec(spec: str) -> List[Tuple[str, int]]:
    entries: List[Tuple[str, int]] = []
    for token in re.split(r"[;,]", spec):
        token = token.strip()
        if not token:
            continue
        if "=" not in token:
            raise ValueError(f"invalid robot token '{token}' (expected key=value)")
        key_raw, value_raw = token.split("=", 1)
        canonical = canonicalize_type(key_raw)
        try:
            count = int(value_raw.strip())
        except Exception as exc:  # pragma: no cover - straightforward parse failure
            raise ValueError(f"invalid count '{value_raw}' for robot '{key_raw}'") from exc
        if count < 0:
            raise ValueError(f"count must be >= 0 for robot '{key_raw}'")
        if count > 0:
            entries.append((canonical, count))
    if not entries:
        raise ValueError("robot spec resolves to zero robots")
    return entries


def iter_brain_instances(spec: str) -> Iterable[Tuple[str, str]]:
    type_index: Dict[str, int] = defaultdict(int)
    for canonical, count in parse_robot_spec(spec):
        for _ in range(count):
            idx = type_index[canonical]
            type_index[canonical] = idx + 1
            yield f"{canonical}_{idx}", canonical


def counts_for_spec(spec: str) -> Dict[str, int]:
    out = {k: 0 for k in CANONICAL_TYPES}
    for canonical, count in parse_robot_spec(spec):
        out[canonical] += count
    return out


def cmd_brains(args: argparse.Namespace) -> int:
    try:
        for brain_id, robot_type in iter_brain_instances(args.spec):
            print(f"{brain_id} {robot_type}")
    except ValueError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    return 0


def cmd_counts_sh(args: argparse.Namespace) -> int:
    try:
        counts = counts_for_spec(args.spec)
    except ValueError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    for canonical in CANONICAL_TYPES:
        print(f"COUNT_{canonical.upper()}={counts[canonical]}")
    return 0


def _resolve_path(root_dir: str, rel_path: str) -> str:
    return str((Path(root_dir) / rel_path).resolve())


def cmd_profile_field(args: argparse.Namespace) -> int:
    canonical = canonicalize_type(args.robot_type)
    profile = PROFILES[canonical]
    field = args.field
    if field == "canonical":
        print(profile.canonical)
        return 0
    if field == "sensory":
        print(profile.sensory)
        return 0
    if field == "output":
        print(profile.output)
        return 0
    if field == "network":
        print(_resolve_path(args.root_dir, profile.network_rel) if profile.network_rel else "")
        return 0
    if field == "config":
        print(_resolve_path(args.root_dir, profile.config_rel))
        return 0
    print(f"unknown field '{field}'", file=sys.stderr)
    return 2


def cmd_validate_target(args: argparse.Namespace) -> int:
    legacy = {"celegans", "drosophila_banc", "drosophila_fafb", "hexapod", "nao", "zebrafish"}
    for profile, _ in parse_robot_spec(args.spec):
        if profile not in legacy and args.target not in {"mujoco", "isaacsim", "webots", "unity", "unreal"}:
            print(f"ERROR: {profile}: {args.target} native factory/client acceptance is not implemented; use the native MuJoCo adapter or its explicit physics-service viewer.", file=sys.stderr)
            return 2
    return 0


def cmd_has_new(args: argparse.Namespace) -> int:
    try:
        return 0 if any(p not in LEGACY_TYPES for p,_ in parse_robot_spec(args.spec)) else 1
    except ValueError as error:
        print(f"ERROR: {error}",file=sys.stderr)
        return 2


def cmd_managed_brains(args: argparse.Namespace) -> int:
    """Select brains independently; other bodies remain unbound NPCs."""
    selected={entry.split('=',1)[0] for entry in args.network_map.split(',') if '=' in entry}
    rows=[(brain,profile) for brain,profile in iter_brain_instances(args.spec) if profile in LEGACY_TYPES or brain in selected]
    for brain,profile in rows:print(brain,profile)
    return 0 if rows else 1


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="cmd", required=True)
    p_managed=sub.add_parser('managed-brains',help='Print preserved or explicitly selected per-instance neural bindings')
    p_managed.add_argument('spec');p_managed.add_argument('--network-map',default='');p_managed.set_defaults(func=cmd_managed_brains)
    p_new = sub.add_parser("has-new", help="Exit 0 for a population using an expansion profile; 1 for legacy only")
    p_new.add_argument("spec")
    p_new.set_defaults(func=cmd_has_new)

    p_target = sub.add_parser("validate-target", help="Reject unavailable embodiment factories without aliasing")
    p_target.add_argument("target")
    p_target.add_argument("spec")
    p_target.set_defaults(func=cmd_validate_target)

    p_brains = sub.add_parser("brains", help="Emit 'brain_id robot_type' lines for a robot spec")
    p_brains.add_argument("spec", help="Robot spec, e.g. 'celegans=1,hexapod=2'")
    p_brains.set_defaults(func=cmd_brains)

    p_counts = sub.add_parser("counts-sh", help="Emit shell COUNT_* assignments for a robot spec")
    p_counts.add_argument("spec", help="Robot spec, e.g. 'celegans=1,hexapod=2'")
    p_counts.set_defaults(func=cmd_counts_sh)

    p_field = sub.add_parser("profile-field", help="Emit one profile field for a robot type")
    p_field.add_argument("robot_type", help="Canonical type or alias")
    p_field.add_argument("field", choices=("canonical", "sensory", "output", "network", "config"))
    p_field.add_argument("--root-dir", default=str(Path.cwd()), help="Repo root used for network/config fields")
    p_field.set_defaults(func=cmd_profile_field)

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    return int(args.func(args))


if __name__ == "__main__":
    raise SystemExit(main())
