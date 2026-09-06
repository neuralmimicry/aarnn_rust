#!/usr/bin/env python3
"""Validate the deployment contract required by the stable AARNN profile.

The SwarmHPC Ansible role lives in a sibling checkout, so the repository's
normal Rust tests cannot import its Jinja task graph without creating a second
deployment implementation.  This bounded QA adapter checks the canonical role
in place and optionally runs its read-only playbook syntax check.  It checks
identity wiring only; it never renders secrets, applies a playbook, changes a
cluster, or claims that mTLS credentials have been provisioned.
"""

from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import sys


def require(text: str, marker: str, description: str) -> None:
    if marker not in text:
        raise RuntimeError(f"stable Ansible profile is missing {description}: {marker}")


def validate(role: Path, repo_root: Path) -> None:
    defaults = (role / "defaults" / "main.yml").read_text(encoding="utf-8")
    tasks = (role / "tasks" / "main.yml").read_text(encoding="utf-8")
    template = (role / "templates" / "aarnn-stack.yaml.j2").read_text(encoding="utf-8")
    entrypoint = (repo_root / "scripts" / "container_entrypoint.sh").read_text(encoding="utf-8")

    require(
        defaults,
        'continuum_tenant_aarnn_engine_node_identity_source: "kubernetes-node-name"',
        "the host-bound worker identity default",
    )
    require(
        defaults,
        "continuum_tenant_aarnn_orchestrator_node_id:",
        "the explicit orchestrator identity setting",
    )
    require(tasks, "engine_mode | lower == 'daemonset'", "the stable daemonset guard")
    require(
        tasks,
        "engine_node_identity_source | lower == 'kubernetes-node-name'",
        "the stable identity-source guard",
    )
    require(
        tasks,
        "stable_node_identity_enable | bool",
        "the node-local credential-provider guard",
    )
    require(template, "--node-id {{ continuum_tenant_aarnn_orchestrator_node_id", "the orchestrator node ID")
    if template.count('--node-id "${AARNN_NODE_ID_PREFIX}-${AARNN_ENGINE_NODE_NAME}"') != 2:
        raise RuntimeError("both deployment and daemonset engine templates must bind node IDs")
    if template.count("fieldPath: spec.nodeName") != 2:
        raise RuntimeError("both engine workload variants must use the host name")
    if template.count('export NM_CAUSAL_NODE_TOKEN=') != 3:
        raise RuntimeError("orchestrator and both engine workload variants must load node credentials")
    if template.count("stable-node-identity") != 6:
        raise RuntimeError("orchestrator and both engine workload variants must mount node identity")
    require(
        entrypoint,
        'default_args+=(--node-id "${AARNN_NODE_ID}")',
        "the container wrapper's explicit node ID forwarding",
    )


def syntax_check(ansible_dir: Path, inventory: Path) -> None:
    command = [
        "ansible-playbook",
        "-i",
        str(inventory),
        "continuum_tenant_aarnn_site.yml",
        "--syntax-check",
    ]
    result = subprocess.run(
        command,
        cwd=ansible_dir,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        detail = (result.stdout + result.stderr).strip()
        raise RuntimeError(f"Ansible syntax check failed:\n{detail}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--ansible-dir",
        type=Path,
        default=Path("/home/pbisaacs/Developer/swarmhpc/swarmhpc/ansible"),
    )
    parser.add_argument("--skip-syntax-check", action="store_true")
    args = parser.parse_args()

    ansible_dir = args.ansible_dir.resolve()
    role = ansible_dir / "roles" / "continuum_tenant_aarnn"
    if not role.is_dir():
        raise RuntimeError(f"AARNN Ansible role does not exist: {role}")
    validate(role, Path(__file__).resolve().parents[2])
    if not args.skip_syntax_check:
        syntax_check(ansible_dir, ansible_dir / "inventory" / "hosts.ini")
    print(f"validated stable AARNN deployment identity contract: {role}")
    if not args.skip_syntax_check:
        print("validated Ansible playbook syntax (read-only)")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1)
