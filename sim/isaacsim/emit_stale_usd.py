#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Re-emit USD for embodiments whose checked-in .usda predates their record.

Runs the unchanged repository generator (scripts/embodiment_usd.py emit) under
the pinned offline USD venv and writes ONLY to a scratch directory, default
target/isaacsim-usd; sim/content/embodiments/generated/ is never modified.
The Isaac runner rejects a USD whose aarnnModelDigest differs from the record;
pass ``--usd-dir target/isaacsim-usd`` to use these re-emissions.

  target/robot-usd-venv/bin/python sim/isaacsim/emit_stale_usd.py [--all] [--out DIR]
"""
import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
GENERATED = ROOT / 'sim/content/embodiments/generated'
sys.path.insert(0, str(ROOT / 'scripts'))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--out', type=Path, default=ROOT / 'target/isaacsim-usd')
    ap.add_argument('--all', action='store_true', help='re-emit every profile, not only stale ones')
    a = ap.parse_args()
    import embodiment_usd
    registry = json.loads((GENERATED / 'registry.generated.json').read_text())
    a.out.mkdir(parents=True, exist_ok=True)
    emitted, skipped = [], []
    for profile in registry['profiles']:
        record = json.loads((GENERATED / (profile['id'] + '.json')).read_text())
        usd = (GENERATED / profile['model']).with_suffix('.usda')
        found = re.search(r'aarnnModelDigest = "([0-9a-f]+)"', usd.read_text()) if usd.exists() else None
        stale = not found or found.group(1) != record['digest']
        if not (stale or a.all):
            continue
        if profile['id'] == 'galbot_g1_golf':
            skipped.append(profile['id'])   # references its immutable official USD by relative path
            continue
        target = a.out / usd.name
        target.unlink(missing_ok=True)
        embodiment_usd.emit(record, target)
        emitted.append(profile['id'])
    print(json.dumps(dict(out=str(a.out), emitted=emitted, skipped=skipped)))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
