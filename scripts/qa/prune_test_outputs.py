#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Prune robot test outputs under target/: keep the two most recent tests of
each robot and only each robot's most recent .mp4; also delete run logs under
logs/ older than an hour (``--log-minutes``).

A *test* is the smallest directory that records one run:
* a run directory (holds ``run.json`` or a ``trace/`` folder); its robot(s)
  come from the trace file names (``<profile>_<n>.jsonl``) or ``run.json``;
* a study directory (holds ``REPORT.md`` or ``study.json``, e.g. a C. elegans
  depth study): one test, whatever runs it contains;
* each child of a known engine series folder (``minecraft``, ``nao-social``,
  ``hexapod-startup``): grouped by the series.
Tests are grouped by the set of robots they ran; the two newest (by the
latest file modification inside) are kept. Directories touched within
``--recent-minutes`` are always kept (runs in progress). Loose logs, JSON
evidence and build products are never touched. Afterwards each robot keeps
only its newest .mp4.

  python3 scripts/qa/prune_test_outputs.py            # dry run
  python3 scripts/qa/prune_test_outputs.py --apply
"""
import argparse
import json
import re
import shutil
import time
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ROOTS = ["target/qa", "target/hexapod-fresh-check", "target/nao-social"]
SERIES = {"minecraft": "series:minecraft", "nao-social": "series:nao-social", "hexapod-startup": "series:hexapod-startup"}
# showcase: the public robot+network videos and the lock-step runs they are
# rendered from (target/qa/showcase, final videos in target/showcase-videos).
SKIP_DIRS = {"isaacsim-venv", "robot-mujoco-venv", "robot-usd-venv", "showcase"}


def newest_mtime(path: Path) -> float:
    latest = path.stat().st_mtime
    for p in path.rglob("*"):
        try:
            latest = max(latest, p.stat().st_mtime)
        except OSError:
            pass
    return latest


def size(path: Path) -> int:
    total = 0
    for p in path.rglob("*"):
        try:
            if p.is_file() and not p.is_symlink():
                total += p.stat().st_size
        except OSError:
            pass
    return total


def robots_of(path: Path) -> frozenset:
    names = set()
    for t in path.rglob("*.jsonl"):
        if t.parent.name == "trace":
            m = re.match(r"(.+)_\d+\.jsonl$", t.name)
            if m:
                names.add(m.group(1))
    if not names:
        for r in path.rglob("run.json"):
            try:
                data = json.loads(r.read_text())
                for w in data.get("worlds", []):
                    pid = w.get("profile_id") or w.get("profile") or str(w.get("instance_id", "")).rsplit("_", 1)[0]
                    if pid:
                        names.add(pid)
            except (OSError, ValueError, AttributeError):
                pass
    return frozenset(names)


def is_study(path: Path) -> bool:
    return (path / "REPORT.md").exists() or (path / "study.json").exists()


def is_run(path: Path) -> bool:
    return (path / "run.json").exists() or (path / "trace").is_dir()


def collect(base: Path, tests: list, series: str = None) -> None:
    for child in sorted(p for p in base.iterdir() if p.is_dir() and p.name not in SKIP_DIRS):
        if series:
            tests.append((child, series))
        elif child.name in SERIES:
            collect(child, tests, SERIES[child.name])
        elif is_study(child) or is_run(child):
            tests.append((child, None))
        else:
            collect(child, tests)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--apply", action="store_true")
    ap.add_argument("--keep", type=int, default=2)
    ap.add_argument("--recent-minutes", type=float, default=60.0)
    ap.add_argument("--log-minutes", type=float, default=60.0,
                    help="also delete run logs under logs/ untouched for this long (0 = keep logs)")
    args = ap.parse_args()
    now = time.time()
    tests = []
    for r in ROOTS:
        base = ROOT / r
        if not base.exists():
            continue
        if is_run(base) or is_study(base):
            tests.append((base, None))
        else:
            collect(base, tests, SERIES.get(base.name))
    groups = defaultdict(list)
    for path, series in tests:
        key = series or ",".join(sorted(robots_of(path))) or "unknown:" + path.parent.name
        groups[key].append((newest_mtime(path), path))
    delete, freed = [], 0
    for key, items in sorted(groups.items()):
        items.sort(reverse=True)
        keep = items[:args.keep]
        for mtime, path in items[args.keep:]:
            if now - mtime < args.recent_minutes * 60:
                keep.append((mtime, path))
                continue
            delete.append(path)
        print(f"{key:40s} tests {len(items):3d} keep {[str(p.relative_to(ROOT)) for _, p in keep[:args.keep]]}")
    for path in delete:
        freed += size(path)
    print(f"\ntests to delete: {len(delete)}, {freed / 1e9:.2f} GB")
    if args.apply:
        for path in delete:
            shutil.rmtree(path, ignore_errors=True)
    # Videos: newest per robot among what remains.
    videos = defaultdict(list)
    for r in ROOTS:
        base = ROOT / r
        if not base.exists():
            continue
        for v in base.rglob("*.mp4"):
            if any(d in v.parts for d in SKIP_DIRS) or (args.apply is False and any(v.is_relative_to(p) for p in delete)):
                continue
            if not v.exists():
                continue
            test = next((p for p, _ in tests if v.is_relative_to(p)), v.parent)
            robots = robots_of(test) or frozenset([v.stem.split("_")[0]])
            for robot in robots:
                videos[robot].append((v.stat().st_mtime, v))
    vdel, vfreed = set(), 0
    for robot, items in sorted(videos.items()):
        items.sort(reverse=True)
        print(f"video {robot:28s} {len(items):3d} keep {items[0][1].relative_to(ROOT)}")
        keep_paths = {items[0][1]}
        for _, v in items[1:]:
            if v not in keep_paths:
                vdel.add(v)
    # a multi-robot video kept for one robot is not deleted for another
    kept = {sorted(items, reverse=True)[0][1] for items in videos.values()}
    vdel -= kept
    for v in vdel:
        vfreed += v.stat().st_size
    print(f"videos to delete: {len(vdel)}, {vfreed / 1e9:.2f} GB")
    # Run logs (git-ignored logs/: sim_cluster_*, nm-*.log, webgl_sim_*):
    # everything untouched for --log-minutes goes; live runs keep theirs.
    ldel, lfreed = [], 0
    logs = ROOT / "logs"
    if args.log_minutes > 0 and logs.is_dir():
        for p in logs.iterdir():
            try:
                if now - newest_mtime(p) >= args.log_minutes * 60:
                    ldel.append(p)
                    lfreed += size(p) if p.is_dir() else p.stat().st_size
            except OSError:
                pass
    print(f"logs to delete: {len(ldel)}, {lfreed / 1e9:.2f} GB")
    if args.apply:
        for v in vdel:
            v.unlink(missing_ok=True)
        for p in ldel:
            if p.is_dir():
                shutil.rmtree(p, ignore_errors=True)
            else:
                p.unlink(missing_ok=True)
        print("applied")


if __name__ == "__main__":
    main()
