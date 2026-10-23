#!/usr/bin/env python3
"""Restore missing pinned assets; check hashes; never run downloaded code."""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import urllib.parse
import urllib.request
from pack_common import PACK_ROOT, load_json, safe_path, sha256

ALLOWED_HOSTS = {'raw.githubusercontent.com', 'files.pythonhosted.org'}
MAX_FILE_BYTES = 32 * 1024 * 1024

def fetch_one(asset, root=PACK_ROOT):
    dst = safe_path(root, asset['path'])
    if dst.exists():
        if not dst.is_file() or sha256(dst) != asset['sha256']:
            raise ValueError('Existing asset differs; move it aside explicitly before restoring: ' + asset['path'])
        return 'verified_existing'
    parsed = urllib.parse.urlparse(asset['url'])
    if parsed.scheme != 'https' or parsed.hostname not in ALLOWED_HOSTS:
        raise ValueError('Asset URL is outside the pinned source hosts.')
    expected = asset['size_bytes']
    if not 0 < expected <= MAX_FILE_BYTES:
        raise ValueError('Pinned asset size is invalid.')
    dst.parent.mkdir(parents=True, exist_ok=True)
    request = urllib.request.Request(asset['url'], headers={'User-Agent':'AARNN-asset-integrity/1.0'})
    temporary = None
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            actual_url = urllib.parse.urlparse(response.url)
            if actual_url.scheme != 'https' or actual_url.hostname not in ALLOWED_HOSTS:
                raise ValueError('Unexpected asset redirect.')
            with tempfile.NamedTemporaryFile(prefix='.asset-', dir=dst.parent, delete=False) as stream:
                temporary = Path(stream.name)
                digest = hashlib.sha256()
                count = 0
                while True:
                    block = response.read(256 * 1024)
                    if not block:
                        break
                    count += len(block)
                    if count > expected:
                        raise ValueError('Asset is larger than pinned size.')
                    digest.update(block)
                    stream.write(block)
        if count != expected or digest.hexdigest() != asset['sha256']:
            raise ValueError('Asset size or SHA-256 mismatch: ' + asset['path'])
        # Exclusive destination copy: do not overwrite a newly appeared file.
        with temporary.open('rb') as inp, dst.open('xb') as out:
            import shutil
            shutil.copyfileobj(inp, out)
        return 'restored'
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--group', required=True, choices=['galbot','ugot','ugot-sdk','all'])
    args = parser.parse_args()
    assets = [a for a in load_json('manifests/assets.lock.json')['assets'] if args.group == 'all' or a['group'] == args.group]
    counts = {'verified_existing':0, 'restored':0, 'failed':0}
    for asset in assets:
        try:
            counts[fetch_one(asset)] += 1
        except (OSError, ValueError) as error:
            counts['failed'] += 1
            print(asset['path'] + ': ' + str(error), file=sys.stderr)
    print(json.dumps(counts, indent=2))
    return int(counts['failed'] != 0)

if __name__ == '__main__':
    raise SystemExit(main())
