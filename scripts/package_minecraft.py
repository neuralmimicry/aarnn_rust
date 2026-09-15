#!/usr/bin/env python3
"""Stage the built Minecraft adapter, official Fabric prerequisites and saved world.

Downloads are bounded and pinned by SHA-256. No installation or account files are changed.
"""
import hashlib
import json
from pathlib import Path
import shutil
import tempfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
PROJECT = ROOT / 'sim/minecraft'


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    dist = ROOT / 'dist/minecraft'
    dist.mkdir(parents=True, exist_ok=True)
    files = [PROJECT / 'build/libs/aarnn-minecraft-1.21.1-0.1.0.jar',
             PROJECT / 'build/libs/aarnn-minecraft-1.21.1-0.1.0-bridge.jar',
             PROJECT / 'build/distributions/AARNN-Sensory-Lab.zip',
             PROJECT / 'README.md', PROJECT / 'NOTICE.md', PROJECT / 'LICENSE',
             PROJECT / 'dependencies.lock.json']
    for path in files:
        if not path.is_file():
            raise RuntimeError(f'Missing build artefact: {path.relative_to(ROOT)}')
    expected = json.loads((ROOT/'sim/content/compiled.generated.json').read_text())['digest']
    accepted = json.loads((PROJECT/'build/distributions/acceptance.json').read_text())
    certificate = PROJECT/'build/gametest/clean-exit.json'
    if accepted['content_digest'] != expected or accepted['world_sha256'] != digest(files[2]) or accepted['certificate_sha256'] != digest(certificate):
        raise RuntimeError('Saved-world package acceptance is stale; rerun the world QA lane')
    for name, sha in json.loads(certificate.read_text())['sha256'].items():
        if digest(PROJECT/name) != sha:
            raise RuntimeError(f'Native world acceptance is stale: {name}')
    for path in files[:2]:
        with zipfile.ZipFile(path) as archive:
            if json.loads(archive.read('aarnn/content.generated.json'))['digest'] != expected:
                raise RuntimeError(f'Stale JAR content: {path.name}')
            if any('gametest' in p.lower() or 'visualtest' in p.lower() or 'TestStorage' in p or 'WorldPack' in p for p in archive.namelist()):
                raise RuntimeError('Development fixtures must not enter distributable JARs')
    with zipfile.ZipFile(files[2]) as world:
        if any(not p.startswith('AARNN-Sensory-Lab/') or '..' in Path(p).parts for p in world.namelist()):
            raise RuntimeError('Invalid saved-world ZIP paths')
        if expected not in world.read('AARNN-Sensory-Lab/AARNN-WORLD.txt').decode():
            raise RuntimeError('Stale saved-world content')
    from build_minecraft_bedrock import generate, OUT as bedrock_output
    # Recompile into an isolated directory and compare actual archive payloads;
    # matching catalogue metadata alone cannot certify an old script or model.
    with tempfile.TemporaryDirectory(prefix='aarnn-bedrock-package-') as temporary:
        fresh=Path(temporary);generate(fresh)
        for variant in ('offline','server'):
            package=bedrock_output/('AARNN-Bedrock-'+variant+'.mcaddon')
            expected_files={target+'/'+str(p.relative_to(fresh/folder)):p.read_bytes()
                for folder,target in ((variant,'AARNN_Lab_BP'),('resource','AARNN_Lab_RP'))
                for p in (fresh/folder).rglob('*') if p.is_file()}
            with zipfile.ZipFile(package) as archive:
                if set(archive.namelist())!=set(expected_files) or any(archive.read(name)!=value for name,value in expected_files.items()):
                    raise RuntimeError('Stale Bedrock pack; rerun the Bedrock QA lane')
            files.append(package)
        ids=json.loads((fresh/'bedrock-content.json').read_text())['pack_ids']
        overlay={target+'/'+str(p.relative_to(fresh/folder)):p.read_bytes()
            for folder,target in (('server','behavior_packs/AARNN_Lab_BP'),('resource','resource_packs/AARNN_Lab_RP'),('server-config','config/'+ids['script']))
            for p in (fresh/folder).rglob('*') if p.is_file()}
        overlay.update({'worlds/AARNN-Sensory-Lab/'+name:(fresh/name).read_bytes() for name in ('world_behavior_packs.json','world_resource_packs.json')})
        with zipfile.ZipFile(bedrock_output/'AARNN-Bedrock-server-overlay.zip') as archive:
            if set(archive.namelist())!=set(overlay) or any(archive.read(name)!=value for name,value in overlay.items()):
                raise RuntimeError('Stale Bedrock server overlay')
    files += [bedrock_output/'AARNN-Bedrock-server-overlay.zip',bedrock_output/'bedrock-content.json',PROJECT/'VALIDATION.md']
    native_world=bedrock_output/'AARNN-Bedrock-Sensory-Lab.mcworld'
    receipt=bedrock_output/'bedrock-world-acceptance.json'
    if not native_world.is_file() or not receipt.is_file():
        raise RuntimeError('Native Bedrock world acceptance missing; run the simulator-minecraft-bedrock-native lane')
    if native_world.exists() or receipt.exists():
        accepted=json.loads(receipt.read_text())
        if accepted['status']!='pass' or accepted['content_digest']!=expected or accepted['world_sha256']!=digest(native_world):
            raise RuntimeError('Native Bedrock world receipt is stale')
        if accepted['evidence_sha256']!=digest(ROOT/accepted['evidence']/'bedrock-native.json'):
            raise RuntimeError('Native Bedrock evidence changed')
        for name,sha in accepted['sources'].items():
            if digest(ROOT/name)!=sha:raise RuntimeError('Native Bedrock world inputs changed; revalidate/repackage')
        with zipfile.ZipFile(native_world) as archive:
            if any('..' in Path(p).parts or Path(p).is_absolute() or any(marker in p for marker in ('native-qa','social-qa','secrets.json')) for p in archive.namelist()):
                raise RuntimeError('Invalid native Bedrock world contents')
        files += [native_world,receipt]
    with zipfile.ZipFile(files[0]) as mod:
        metadata = json.loads(mod.read('fabric.mod.json'))
        if metadata['id'] != 'aarnn' or metadata['version'] != '0.1.0':
            raise RuntimeError('Unexpected mod metadata')
        if any('gametest' in p.lower() or 'visualtest' in p.lower() or 'IoTrace' in p for p in mod.namelist()):
            raise RuntimeError('Development fixtures must not enter the mod')
    for path in files:
        shutil.copy2(path, dist / path.name)
    shutil.copy2(PROJECT/'bedrock/README.md',dist/'BEDROCK.md')
    # The standalone bundle has a flat documentation layout.
    (dist/'README.md').write_text((dist/'README.md').read_text().replace('bedrock/README.md','BEDROCK.md').replace('../content/README.md','CONTENT.md').replace('../nao/README.md','NAO.md'))
    (dist/'BEDROCK.md').write_text((dist/'BEDROCK.md').read_text().replace('../VALIDATION.md','VALIDATION.md').replace('../../nao/README.md','NAO.md'))
    (dist/'VALIDATION.md').write_text((dist/'VALIDATION.md').read_text().replace('../nao/README.md','NAO.md'))
    shutil.copy2(ROOT/'sim/content/README.md',dist/'CONTENT.md')
    (dist/'CONTENT.md').write_text((dist/'CONTENT.md').read_text()
        .replace('[the simulator guide](../README.md)','`sim/README.md` in the repository')
        .replace('[the living execution plan](../../docs/execplans/simulator-content-parity.md)',
                 '`docs/execplans/simulator-content-parity.md` in the repository')
        .replace('../minecraft/README.md','README.md'))
    shutil.copy2(ROOT/'sim/content/communication.json',dist/'COMMUNICATION.json')
    (dist/'NAO.md').write_text((ROOT/'sim/nao/README.md').read_text().replace('../content/communication.json','COMMUNICATION.json').replace('../minecraft/README.md','README.md').replace('../minecraft/VALIDATION.md','VALIDATION.md'))
    shutil.copytree(PROJECT / 'licenses', dist / 'licenses', dirs_exist_ok=True)
    for item in json.loads((PROJECT / 'dependencies.lock.json').read_text())['downloads']:
        target = dist / item['file']
        if not target.exists() or digest(target) != item['sha256']:
            temporary = target.with_suffix('.download')
            try:
                with urllib.request.urlopen(item['url'], timeout=45) as response, temporary.open('wb') as out:
                    total = 0
                    while block := response.read(65536):
                        total += len(block)
                        if total > 8 * 1024 * 1024:
                            raise RuntimeError('Prerequisite exceeds download bound')
                        out.write(block)
                if digest(temporary) != item['sha256']:
                    raise RuntimeError('Prerequisite SHA-256 mismatch')
                temporary.replace(target)
            finally:
                temporary.unlink(missing_ok=True)
    manifest = '\n'.join(f'{digest(p)}  {p.relative_to(dist)}' for p in sorted(dist.rglob('*'))
                         if p.is_file() and p.name != 'SHA256SUMS') + '\n'
    (dist / 'SHA256SUMS').write_text(manifest)
    print(f'Minecraft bundle: {dist}')


if __name__ == '__main__':
    main()
