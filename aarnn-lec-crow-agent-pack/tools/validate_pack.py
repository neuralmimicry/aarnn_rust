#!/usr/bin/env python3
"""Validate instructions, coverage, pinned source bytes and proposed crow IO."""
from __future__ import annotations
import json
import sys
from pack_common import PACK_ROOT, load_json, safe_path, sha256

def validate(root=PACK_ROOT):
    data=load_json('manifests/robots.json',root)
    new=data['robots']
    old=load_json('manifests/existing-profiles.json',root)['profiles']
    ids=[r['id'] for r in new+old]
    if len(ids)!=28 or len(set(ids))!=28:
        raise ValueError('Expected 22 new plus six distinct existing profiles')
    products={r['commercial_product_id'] for r in new if r['lec_listing']}
    if len(products)!=15 or sum(r['id'].startswith('ugot_') for r in new)!=7:
        raise ValueError('Incorrect commercial-family or UGOT-form coverage')
    for rid in ids:
        if not safe_path(root,f'robots/{rid}/AGENTS.md').is_file():
            raise ValueError('Missing per-profile instructions: '+rid)
    required=['AGENTS.md','START_HERE.md','README.md','EXECPLAN.md','research/inventory.md','research/repository-map.md','spec/requirements.md','spec/aer-contract.md','spec/physics-and-engines.md','spec/crow-biology.md','spec/acceptance.md','agents/45-mujoco.md','agents/46-isaacsim.md']
    for rel in required:
        if not safe_path(root,rel).is_file():
            raise ValueError('Missing task input: '+rel)
    crow=load_json('spec/crow-io.proposed.json',root)
    if len(crow['controlled_joints'])!=31 or len(crow['sensor_names'])!=492 or len(crow['output_names'])!=64:
        raise ValueError('Crow projection dimensions do not match')
    if len(set(crow['sensor_names']))!=492 or len(set(crow['output_names']))!=64:
        raise ValueError('Duplicate crow channel names')
    if crow['sensory_base']+492 > crow['output_base']:
        raise ValueError('Crow address windows overlap')
    rows=load_json('verification/acceptance-matrix.json',root)['rows']
    targets={'webots','unity','unreal','webgl','minecraft_java','minecraft_bedrock','mujoco','isaacsim'}
    pairs={(r['robot_id'],r['target']) for r in rows}
    if len(rows)!=224 or pairs!={(rid,target) for rid in ids for target in targets}:
        raise ValueError('Incomplete/duplicate target matrix')
    if any(r['status']!='not_run' for r in rows):
        raise ValueError('This prepared pack must not claim simulator execution')
    sources={s['id'] for s in load_json('research/sources.json',root)['sources']}
    for robot in new:
        if not set(robot['evidence']) <= sources:
            raise ValueError('Unknown robot evidence source')
    assets=load_json('manifests/assets.lock.json',root)['assets']
    for asset in assets:
        path=safe_path(root,asset['path'])
        if not path.is_file() or path.stat().st_size!=asset['size_bytes'] or sha256(path)!=asset['sha256']:
            raise ValueError('Pinned asset mismatch: '+asset['path'])
    for licence in ['assets/galbot-g1-golf/LICENSE','assets/ugot-mecanum/LICENSE']:
        if not safe_path(root,licence).is_file():
            raise ValueError('Missing source licence')
    import zipfile
    with zipfile.ZipFile(safe_path(root,'assets/ugot-sdk/ugot-0.2.13-py3-none-any.whl')) as wheel:
        if not any(n.endswith('.dist-info/LICENSE') for n in wheel.namelist()):
            raise ValueError('SDK licence not present in wheel')
    return dict(status='pass',new_profiles=len(new),existing_profiles=len(old),total_profiles=len(ids),targets=len(targets),acceptance_rows=len(rows),verified_asset_files=len(assets),scope='Pack integrity and instruction coverage only; no simulator or AARNN execution.')

if __name__=='__main__':
    try:
        print(json.dumps(validate(),indent=2))
    except (ValueError,OSError,KeyError) as error:
        print(str(error),file=sys.stderr)
        raise SystemExit(1)
