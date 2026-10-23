"""Small dependency-free helpers for this instruction/source pack."""
from __future__ import annotations
import hashlib
import json
from pathlib import Path, PurePosixPath

PACK_ROOT = Path(__file__).resolve().parents[1]

def load_json(relative, root=PACK_ROOT):
    return json.loads(safe_path(root, relative).read_text(encoding='utf-8'))

def safe_path(root, relative):
    rel = PurePosixPath(relative)
    if not relative or rel.is_absolute() or '..' in rel.parts or '\\' in relative:
        raise ValueError(f'Unsafe pack path: {relative!r}')
    base = Path(root).resolve()
    candidate = base.joinpath(*rel.parts)
    if not candidate.resolve().is_relative_to(base):
        raise ValueError(f'Path escapes root: {relative!r}')
    current = base
    for part in rel.parts:
        current = current / part
        if current.is_symlink():
            raise ValueError(f'Symlink is not permitted in pack paths: {relative!r}')
    return candidate

def sha256(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()

def pack_files(root=PACK_ROOT):
    root = Path(root).resolve()
    return sorted(p for p in root.rglob('*') if p.is_file() and
                  '__pycache__' not in p.parts and not p.name.endswith('.pyc'))
