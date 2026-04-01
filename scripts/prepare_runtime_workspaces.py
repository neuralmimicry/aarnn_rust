#!/usr/bin/env python3
import argparse
import json
import os
import tempfile
from pathlib import Path


def sanitize_segment(raw: str, fallback: str) -> str:
    cleaned = "".join(
        ch if ch.isascii() and (ch.isalnum() or ch in "-_.") else "-"
        for ch in raw.strip()
    )
    normalized = cleaned.strip("-.").lower()
    return normalized or fallback


def now_ms() -> int:
    return int(__import__("time").time() * 1000)


def atomic_write(path: Path, payload: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp_name = tempfile.mkstemp(prefix=path.name + ".", dir=str(path.parent))
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as fh:
            fh.write(payload)
        os.replace(tmp_name, path)
    finally:
        try:
            os.unlink(tmp_name)
        except FileNotFoundError:
            pass


def load_json(path: Path) -> object:
    return json.loads(path.read_text(encoding="utf-8"))


def resolve_seed_payload(spec: dict, latest_path: Path, resume_existing: bool) -> tuple[str, str]:
    if resume_existing and latest_path.exists():
        return latest_path.read_text(encoding="utf-8"), "latest"

    snapshot_path = str(spec.get("snapshot_path") or "").strip()
    if snapshot_path:
        path = Path(snapshot_path)
        return path.read_text(encoding="utf-8"), str(path)

    config_path = str(spec.get("config_path") or "").strip()
    if config_path:
        path = Path(config_path)
        return path.read_text(encoding="utf-8"), str(path)

    raise SystemExit(
        f"workspace spec for brain '{spec.get('brain_id', '')}' requires snapshot_path, config_path, or an existing latest snapshot"
    )


def manifest_engine_from_payload(payload: str, neuron_model: str, learning_rule: str) -> dict:
    data = json.loads(payload)
    if isinstance(data, dict) and isinstance(data.get("net"), dict):
        net = data["net"]
    elif isinstance(data, dict):
        net = data
    else:
        raise SystemExit("runtime workspace payload must decode to a JSON object")
    return {
        "lif": {},
        "stdp": {},
        "net": net,
        "neuron_model": neuron_model,
        "learning_rule": learning_rule,
    }


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Prepare persistent runtime workspaces for Webots/cluster launch."
    )
    parser.add_argument("--root", default="data/runtime", help="Runtime root directory.")
    parser.add_argument("--user", required=True, help="Runtime workspace user namespace.")
    parser.add_argument(
        "--autosave-steps", type=int, default=10, help="Autosave cadence stored in manifests."
    )
    parser.add_argument(
        "--resume-existing",
        type=int,
        default=1,
        help="Reuse existing latest.snapshot.json when present (1/0).",
    )
    parser.add_argument(
        "--spec-json",
        required=True,
        help="JSON array of workspace specs with brain_id/config_path/snapshot_path.",
    )
    args = parser.parse_args()

    try:
        specs = json.loads(args.spec_json)
    except json.JSONDecodeError as exc:
        raise SystemExit(f"invalid --spec-json payload: {exc}") from exc
    if not isinstance(specs, list):
        raise SystemExit("--spec-json must decode to a JSON array")

    root_dir = Path(args.root).expanduser().resolve()
    user_id = sanitize_segment(args.user, "anonymous")
    resume_existing = args.resume_existing not in (0, False)
    autosave_steps = max(1, int(args.autosave_steps))

    bindings: dict[str, dict] = {}
    for spec in specs:
        if not isinstance(spec, dict):
            raise SystemExit("workspace spec entries must be JSON objects")
        brain_id = str(spec.get("brain_id") or "").strip()
        if not brain_id:
            raise SystemExit("workspace spec is missing brain_id")
        workspace_id = sanitize_segment(
            str(spec.get("workspace_id") or brain_id), "workspace"
        )
        workspace_name = str(spec.get("name") or workspace_id).strip() or workspace_id
        neuron_model = str(spec.get("neuron_model") or "aarnn").strip() or "aarnn"
        learning_rule = str(spec.get("learning_rule") or "aarnn").strip() or "aarnn"

        workspace_dir = (
            root_dir / "users" / user_id / "workspaces" / workspace_id
        )
        manifest_path = workspace_dir / "manifest.json"
        baseline_path = workspace_dir / "baseline.snapshot.json"
        latest_path = workspace_dir / "latest.snapshot.json"

        seed_payload, seed_source = resolve_seed_payload(spec, latest_path, resume_existing)
        engine = manifest_engine_from_payload(seed_payload, neuron_model, learning_rule)

        existing_manifest = None
        if manifest_path.exists():
            try:
                existing_manifest = load_json(manifest_path)
            except Exception:
                existing_manifest = None

        created_at_ms = (
            int(existing_manifest.get("created_at_ms", 0))
            if isinstance(existing_manifest, dict)
            else 0
        ) or now_ms()
        last_saved_at_ms = (
            int(existing_manifest.get("last_saved_at_ms", 0))
            if isinstance(existing_manifest, dict)
            else 0
        ) or (
            int(latest_path.stat().st_mtime * 1000) if latest_path.exists() else created_at_ms
        )
        manifest = {
            "version": 1,
            "user_id": user_id,
            "workspace_id": workspace_id,
            "name": workspace_name,
            "created_at_ms": created_at_ms,
            "updated_at_ms": max(last_saved_at_ms, now_ms()),
            "last_saved_at_ms": last_saved_at_ms,
            "desired_running": False,
            "autosave_steps": autosave_steps,
            "engine": engine,
        }

        workspace_dir.mkdir(parents=True, exist_ok=True)
        if not baseline_path.exists() or not resume_existing:
            atomic_write(baseline_path, seed_payload)
        if not latest_path.exists() or not resume_existing:
            atomic_write(latest_path, seed_payload)
            manifest["last_saved_at_ms"] = now_ms()
            manifest["updated_at_ms"] = manifest["last_saved_at_ms"]
        atomic_write(manifest_path, json.dumps(manifest, indent=2) + "\n")

        bindings[brain_id] = {
            "workspace_id": workspace_id,
            "workspace_dir": str(workspace_dir),
            "manifest_path": str(manifest_path),
            "baseline_snapshot_path": str(baseline_path),
            "latest_snapshot_path": str(latest_path),
            "autosave_steps": autosave_steps,
            "seed_source": seed_source,
        }

    print(json.dumps(bindings, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
