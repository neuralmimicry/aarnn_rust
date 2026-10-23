#!/usr/bin/env python3
"""Install additive task inputs. Preview by default; never replace different files."""
from __future__ import annotations
import argparse
import json
from pathlib import Path
import shutil
import sys
from pack_common import PACK_ROOT, pack_files, safe_path, sha256

def installation_plan(source, repo):
    source, repo = Path(source).resolve(), Path(repo).expanduser().resolve()
    if not (repo / 'Cargo.toml').is_file() or not (repo / '.git').exists():
        raise ValueError('Expected an existing Git/Cargo repository root (--repo).')
    destination = safe_path(repo, '.agent/robot-expansion')
    plan = []
    conflicts = []
    for src in pack_files(source):
        rel = src.relative_to(source).as_posix()
        dst = safe_path(destination, rel)
        if dst.exists():
            if not dst.is_file() or sha256(dst) != sha256(src):
                conflicts.append(rel)
            else:
                plan.append((src, dst, 'identical'))
        else:
            plan.append((src, dst, 'create'))
    if conflicts:
        raise ValueError('Refusing to replace different existing files: ' + ', '.join(conflicts[:12]))
    return destination, plan

def install(source, repo, apply=False):
    destination, plan = installation_plan(source, repo)
    created = 0
    if apply:
        for src, dst, action in plan:
            if action == 'identical':
                continue
            dst.parent.mkdir(parents=True, exist_ok=True)
            # Exclusive create protects files that appeared after the preview.
            with src.open('rb') as inp, dst.open('xb') as out:
                shutil.copyfileobj(inp, out)
            shutil.copystat(src, dst)
            created += 1
    return dict(mode='installed' if apply else 'preview', destination=str(destination),
                files_to_create=sum(a == 'create' for _, _, a in plan),
                identical_files=sum(a == 'identical' for _, _, a in plan), created=created,
                root_agents_modified=False,
                next_step='Paste START_HERE.md into the RustRover Codex interface.')

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', required=True)
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    try:
        print(json.dumps(install(PACK_ROOT, args.repo, args.apply), indent=2))
        return 0
    except (OSError, ValueError) as error:
        print(str(error), file=sys.stderr)
        return 1

if __name__ == '__main__':
    raise SystemExit(main())
