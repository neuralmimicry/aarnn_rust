"""Prepare isolated Minecraft instances for a NAO social run; never alter installed worlds."""
import json
import os
from pathlib import Path
import shutil
import zipfile
from urllib.parse import urlsplit
from nao_social import ROOT


def extract_world(archive, destination, java=False):
    total=0
    with zipfile.ZipFile(archive) as bundle:
        for info in bundle.infolist():
            path=Path(info.filename)
            if path.is_absolute() or '..' in path.parts:
                raise ValueError('Unsafe world archive path')
            if java:
                if not path.parts or path.parts[0]!='AARNN-Sensory-Lab':
                    raise ValueError('Unexpected Java world root')
                path=Path(*path.parts[1:])
            else:
                # Activate the current connected packs at installation scope;
                # never install the exported offline duplicates alongside them.
                if path.parts and path.parts[0] in ('behavior_packs','resource_packs'):
                    continue
            total+=info.file_size
            if total>512*1024*1024:
                raise ValueError('World exceeds installation bound')
            target=destination/path
            if info.is_dir():
                target.mkdir(parents=True,exist_ok=True)
            else:
                target.parent.mkdir(parents=True,exist_ok=True)
                with bundle.open(info) as source,target.open('xb') as output:
                    shutil.copyfileobj(source,output)


def prepare_minecraft(run, settings, edition, bedrock_source=None):
    content=json.loads((ROOT/'sim/content/compiled.generated.json').read_text())
    directory=run/('bedrock-server' if edition=='bedrock' else 'minecraft-java')
    directory.mkdir(mode=0o700)
    if edition=='java':
        (directory/'mods').mkdir();(directory/'config').mkdir()
        for source in (ROOT/'sim/minecraft/build/libs/aarnn-minecraft-0.1.0.jar',ROOT/'dist/minecraft/fabric-api-0.107.0+1.21.1.jar'):
            if not source.is_file():raise ValueError('Build/package Minecraft prerequisites first')
            shutil.copy2(source,directory/'mods'/source.name)
        extract_world(ROOT/'sim/minecraft/build/distributions/AARNN-Sensory-Lab.zip',directory/'saves/AARNN-Sensory-Lab',True)
        (directory/'config/aarnn.json').write_text(json.dumps(dict(schemaVersion=1,allowLegacySandboxInference=True,
            endpoint=f'http://127.0.0.1:{settings["minecraft_port"]}/api/aer/infer',tokenEnvironment='AARNN_MINECRAFT_TOKEN',
            contentDigest=content['digest'],bindings={'nao':dict(networkId='nao',nodeId=None,sensory=250,output=40,firstStep=0)}),indent=2)+'\n')
        return directory
    from bedrock import server_directory,PACKS,detect
    source=server_directory(bedrock_source)
    if source is None or not (source/'bedrock_server').is_file():
        raise ValueError('Bedrock server unavailable; install it or select --minecraft-edition java')
    for name in ('behavior_packs','resource_packs','data','definitions','treatments','world_templates','minecraftpe'):
        if (source/name).is_dir():shutil.copytree(source/name,directory/name)
    for name in ('bedrock_server','profanity_filter.wlist','packetlimitconfig.json'):
        shutil.copy2(source/name,directory/name)
    (directory/'server.properties').write_text(
        'server-name=AARNN NAO social lab\nlevel-name=AARNN\nlevel-type=FLAT\n'
        'gamemode=creative\nallow-cheats=true\nonline-mode=true\nallow-list=false\n'
        'enable-lan-visibility=false\ntransport=raknet\nserver-ip=127.0.0.1\n'
        'server-port=19132\nserver-portv6=19133\nmax-players=16\nview-distance=8\ntick-distance=4\n'
        'content-log-file-enabled=true\ncontent-log-console-output-enabled=true\n')
    for name in ('allowlist.json','permissions.json'):(directory/name).write_text('[]')
    for folder,target in (('server','behavior_packs/AARNN_Lab_BP'),('resource','resource_packs/AARNN_Lab_RP')):
        if (directory/target).exists():raise ValueError('Source BDS already contains lab packs; use a stock installation')
        shutil.copytree(PACKS/folder,directory/target)
    extract_world(PACKS/'AARNN-Bedrock-Sensory-Lab.mcworld',directory/'worlds/AARNN')
    for name in ('world_behavior_packs.json','world_resource_packs.json'):
        shutil.copy2(PACKS/name,directory/'worlds/AARNN'/name)
    ids=json.loads((PACKS/'bedrock-content.json').read_text())['pack_ids']
    config=directory/'config'/ids['script'];shutil.copytree(PACKS/'server-config',config)
    (config/'variables.json').write_text(json.dumps(dict(AARNN_ALLOW_SANDBOX_INFERENCE=True,AARNN_CONTENT_DIGEST=content['digest'],
        AARNN_ALLOW_NAO_CHAT=True,AARNN_NAO_PORT=urlsplit(settings['url']).port,AARNN_INFERENCE_PORT=settings['minecraft_port']),indent=2)+'\n')
    with (config/'secrets.json').open('x') as stream:
        os.chmod(stream.name,0o600)
        json.dump(dict(AARNN_AUTHORIZATION='Bearer '+settings['minecraft_token'],AARNN_NAO_AUTHORIZATION='Bearer '+settings['adapter_token']),stream)
    errors=detect(directory)['errors']
    if errors:raise ValueError('Isolated Bedrock preflight: '+'; '.join(errors))
    return directory
