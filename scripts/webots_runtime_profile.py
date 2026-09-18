"""Shared Cargo and management profile selection for Python Webots launchers."""

from __future__ import annotations

import os
from pathlib import Path

from local_management_env import prepare as prepare_local_management_env


DEFAULT_PROFILE = "engine_runtime,ui,robot_io,cuda"
MANAGEMENT_ENV_NAMES = (
    "NM_GRPC_TLS_CERT",
    "NM_GRPC_TLS_KEY",
    "NM_GRPC_TLS_CA",
    "NM_GRPC_TLS_DOMAIN",
    "NM_MANAGEMENT_BEARER_TOKEN",
    "NM_MANAGEMENT_PRINCIPAL",
    "NM_MANAGEMENT_PRINCIPALS",
    "NM_MANAGEMENT_STATE_PATH",
)


def runtime_profile(all_features: bool = False) -> str:
    requested = os.environ.get("NM_WEBOTS_RUNTIME_FEATURES", DEFAULT_PROFILE)
    if all_features or requested.lower() in {"all", "all-features"}:
        return "all-features"
    return requested or DEFAULT_PROFILE


def cargo_feature_args(profile: str) -> list[str]:
    if profile == "all-features":
        return ["--all-features"]
    return ["--no-default-features", "--features", profile]


def prepare_management_environment(profile: str, runtime_root: Path) -> dict[str, str]:
    if profile == "all-features":
        return prepare_local_management_env(runtime_root)
    for name in MANAGEMENT_ENV_NAMES:
        os.environ.pop(name, None)
    return {}
