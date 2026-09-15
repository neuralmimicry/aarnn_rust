#!/usr/bin/env python3
"""Export only a clean, native-verified lab with current offline packs and no QA code."""
import argparse
import hashlib
import json
from pathlib import Path
import zipfile

ROOT=Path(__file__).resolve().parents[1]
PACKS=ROOT/'sim/minecraft/build/bedrock'


def digest(path):
    with path.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()


def package(evidence):
    evidence=evidence.resolve()
    if not evidence.is_relative_to(ROOT/'target/qa/minecraft'):raise ValueError('Use a repository native QA result directory')
    report=json.loads((evidence/'bedrock-native.json').read_text())
    content=json.loads((PACKS/'bedrock-content.json').read_text())
    if report.get('status')!='pass' or not report.get('clean_exit') or report.get('content_digest')!=content['content_digest']:
        raise ValueError('Native clean-exit acceptance absent or stale')
    if sorted(p['profile'] for p in report['profiles'] if p['status']=='pass')!=sorted(content['profiles']):
        raise ValueError('All six native Rust profiles must pass')
    if not all(report.get(stage,{}).get('native_npc_identity') for stage in ('world','reload')):
        raise ValueError('Native vanilla NPC identity must pass before and after reload')
    server=evidence/'server';world=server/'worlds/AARNN'
    # Prove the actual tested pack is the released source plus the isolated
    # observer import. Neither observer nor private server config enters the ZIP.
    tested=[]
    for folder,target in [('server','behavior_packs/AARNN_Lab_BP'),('resource','resource_packs/AARNN_Lab_RP')]:
        for source in (PACKS/folder).rglob('*'):
            if not source.is_file():continue
            actual=(server/target/source.relative_to(PACKS/folder)).read_bytes()
            expected=source.read_bytes()
            if folder=='server' and source.name=='main.js':expected+=b"\nimport './native-qa.js';\n"
            if actual!=expected:raise ValueError('Native pack acceptance is stale: '+str(source.relative_to(PACKS)))
            tested.append(source)
    files={'level.dat':world/'level.dat'}
    for name in ('levelname.txt','chunks.dat','importedchunks.dat'):
        if (world/name).is_file():files[name]=world/name
    for source in (world/'db').iterdir():
        if source.is_file() and source.name not in ('LOCK','LOG','LOG.old'):files['db/'+source.name]=source
    if 'db/CURRENT' not in files:raise ValueError('Saved Bedrock database missing')
    for name in ('world_behavior_packs.json','world_resource_packs.json'):files[name]=PACKS/name
    for folder,target in [('offline','behavior_packs/AARNN_Lab_BP'),('resource','resource_packs/AARNN_Lab_RP')]:
        for source in (PACKS/folder).rglob('*'):
            if source.is_file():files[target+'/'+str(source.relative_to(PACKS/folder))]=source
    destination=PACKS/'AARNN-Bedrock-Sensory-Lab.mcworld'
    with zipfile.ZipFile(destination,'w',zipfile.ZIP_DEFLATED) as archive:
        for name,source in sorted(files.items()):
            entry=zipfile.ZipInfo(name,(2026,1,1,0,0,0));entry.compress_type=zipfile.ZIP_DEFLATED
            archive.writestr(entry,source.read_bytes())
    receipt=dict(status='pass',content_digest=content['content_digest'],native_version=report['version'],
        world_sha256=digest(destination),evidence=str(evidence.relative_to(ROOT)),
        evidence_sha256=digest(evidence/'bedrock-native.json'),
        sources={str(p.relative_to(ROOT)):digest(p) for p in list(files.values())+tested})
    (PACKS/'bedrock-world-acceptance.json').write_text(json.dumps(receipt,indent=2)+'\n')
    print('Certified native Bedrock world:',destination)


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--evidence',type=Path,required=True)
    package(parser.parse_args().evidence)
