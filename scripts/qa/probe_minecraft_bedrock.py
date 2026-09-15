#!/usr/bin/env python3
"""Native BDS acceptance in a fresh isolated lab; never opens an installed world."""
import hashlib
import json
import os
from pathlib import Path
import queue
import re
import secrets
import shutil
import socket
import subprocess
import sys
import threading
import time
import urllib.request

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'scripts'))
from bedrock import server_directory, PACKS, detect
from bedrock_ports import launch_directory
from minecraft import java
from robot_profiles import PROFILES
from probe_minecraft_neural import port, ready, stop
from bedrock_metadata import enable_script_experiments


class Server:
    def __init__(self,directory,log):
        self.log=log.open('w');self.events=queue.Queue(maxsize=1024)
        self.runtime=launch_directory(dict(directory=str(directory),world='AARNN'))
        runtime,reservation=self.runtime.__enter__()
        self.allocation=json.loads((Path(runtime['directory'])/'aarnn-launch.json').read_text())
        reservation.close()
        self.process=subprocess.Popen([str(directory/'bedrock_server')],cwd=runtime['directory'],
            env=dict(os.environ,LD_LIBRARY_PATH=str(directory)),stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,bufsize=1)
        def read():
            for line in self.process.stdout:
                self.log.write(line);self.log.flush();self.events.put(line)
            self.events.put(None)
        self.reader=threading.Thread(target=read,daemon=True);self.reader.start()

    def send(self,line):
        self.process.stdin.write(line+'\n');self.process.stdin.flush()

    def until(self,predicate,timeout=90):
        deadline=time.monotonic()+timeout
        while time.monotonic()<deadline:
            try:line=self.events.get(timeout=.2)
            except queue.Empty:continue
            if line is None:raise RuntimeError('BDS exited before required evidence')
            if ' ERROR]' in line or 'AARNN world: ' in line:
                raise RuntimeError('Native content/runtime error: '+line.strip())
            if 'AARNN_BEDROCK_QA ' in line:
                value=json.loads(line.split('AARNN_BEDROCK_QA ',1)[1])
                if value['kind']=='failure':raise RuntimeError(value['reason'])
            if predicate(line):return line
        raise TimeoutError('Native BDS evidence deadline exceeded')

    def verify(self,stage):
        self.send('scriptevent aarnnqa:verify '+stage)
        line=self.until(lambda s:'AARNN_BEDROCK_QA ' in s and '"kind":"world"' in s)
        return json.loads(line.split('AARNN_BEDROCK_QA ',1)[1])

    def close(self):
        clean=False
        try:
            if self.process.poll() is None:self.send('stop')
            self.process.wait(timeout=30);clean=self.process.returncode==0
        except (OSError,subprocess.TimeoutExpired):stop(self.process)
        finally:
            self.reader.join(timeout=2);self.log.close();self.runtime.__exit__(None,None,None)
        if not clean:raise RuntimeError('Native BDS did not exit cleanly')


def prepare(source,directory,token):
    directory.mkdir()
    for name in ('behavior_packs','resource_packs','data','definitions','treatments','world_templates','minecraftpe'):
        if (source/name).is_dir():shutil.copytree(source/name,directory/name)
    for name in ('bedrock_server','profanity_filter.wlist','packetlimitconfig.json'):
        shutil.copy2(source/name,directory/name)
    (directory/'server.properties').write_text(
        'server-name=AARNN isolated QA\nlevel-name=AARNN\nlevel-type=FLAT\nlevel-seed=481516\n'
        'gamemode=creative\nallow-cheats=true\nonline-mode=true\nallow-list=true\n'
        'enable-lan-visibility=false\ntransport=nethernet\nserver-ip=127.0.0.1\n'
        f'server-port={port()}\nmax-players=1\nview-distance=5\ntick-distance=4\n'
        'content-log-file-enabled=true\ncontent-log-console-output-enabled=true\n')
    for name in ('allowlist.json','permissions.json'):(directory/name).write_text('[]')
    # Let BDS create complete native metadata before activating experimental
    # packs. A handwritten subset can silently misidentify vanilla entities.
    # The managed launcher mounts existing world directories into its run copy.
    world=directory/'worlds/AARNN';world.mkdir(parents=True)
    bootstrap=Server(directory,directory.parent/'bootstrap.log')
    try:bootstrap.until(lambda line:'Server started.' in line,180)
    finally:bootstrap.close()
    level=world/'level.dat'
    level.write_bytes(enable_script_experiments(level.read_bytes()))
    for folder,target in (('server','behavior_packs/AARNN_Lab_BP'),('resource','resource_packs/AARNN_Lab_RP')):
        if (directory/target).exists():raise RuntimeError('Source server already contains AARNN packs; use a stock server installation for QA')
        shutil.copytree(PACKS/folder,directory/target)
    for name in ('world_behavior_packs.json','world_resource_packs.json'):shutil.copy2(PACKS/name,world/name)
    metadata=json.loads((PACKS/'bedrock-content.json').read_text())
    config=directory/'config'/metadata['pack_ids']['script'];shutil.copytree(PACKS/'server-config',config)
    (config/'variables.json').write_text(json.dumps(dict(AARNN_ALLOW_SANDBOX_INFERENCE=True,AARNN_CONTENT_DIGEST=metadata['content_digest'])))
    with (config/'secrets.json').open('x') as stream:
        os.chmod(stream.name,0o600);json.dump({'AARNN_AUTHORIZATION':'Bearer '+token},stream)
    errors=detect(directory)['errors']
    if errors:
        (config/'secrets.json').unlink()
        raise RuntimeError('Native preflight failed: '+'; '.join(errors))
    scripts=directory/'behavior_packs/AARNN_Lab_BP/scripts'
    shutil.copy2(ROOT/'scripts/qa/bedrock_native_probe.js',scripts/'native-qa.js')
    with (scripts/'main.js').open('a') as stream:stream.write("\nimport './native-qa.js';\n")
    return config/'secrets.json'


def main():
    output=Path(os.environ['NM_MINECRAFT_RESULT_DIR'])
    source=server_directory();java_path=java().get('path')
    executable=ROOT/'target/release/examples/nn_tcp_server'
    if source is None or not (source/'bedrock_server').is_file():raise RuntimeError('Native Linux BDS is required')
    if not java_path or not executable.is_file():raise RuntimeError('Java 21 and built Rust nn_tcp_server are required')
    # The released BDS connector deliberately has one fixed loopback destination.
    with socket.socket() as check:
        check.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1)
        check.bind(('127.0.0.1',62620))
    token=secrets.token_urlsafe(32);directory=output/'server'
    secret=prepare(source,directory,token);server=None;udp_collisions=[]
    collision=socket.socket();collision.bind(('127.0.0.1',0));collision.listen()
    settings=directory/'server.properties'
    settings.write_text(re.sub(r'server-port=\d+',f'server-port={collision.getsockname()[1]}',settings.read_text()))
    report=dict(status='fail',source=str(source),native_engine='Bedrock Dedicated Server',profiles=[])
    try:
        server=Server(directory,output/'native.log')
        report['port_allocation']=server.allocation
        if server.allocation['endpoints'][0]['port']==collision.getsockname()[1]:raise RuntimeError('Native occupied TCP port was not reassigned')
        version=server.until(lambda s:'Version: ' in s)
        report['version']=version.split('Version: ',1)[1].strip()
        server.until(lambda s:'AARNN_BEDROCK_READY ' in s and 'inference=true' in s)
        server.send('scriptevent aarnn:world build')
        server.until(lambda s:'AARNN_BEDROCK_WORLD ' in s,180)
        report['world']=server.verify('built')
        content=json.loads((ROOT/'sim/content/compiled.generated.json').read_text())
        for profile in content['profiles']:
            name=profile['id'];p=PROFILES[name];model=ROOT/p.network_rel
            with model.open('rb') as stream:model_sha=hashlib.file_digest(stream,'sha256').hexdigest()
            with model.open() as stream:prefix=stream.read(65536)
            match=re.match(r'\s*\{\s*"net"\s*:\s*',prefix)
            if not match:raise RuntimeError('Expected canonical snapshot prefix')
            net,_=json.JSONDecoder().raw_decode(prefix,match.end())
            if (net['num_sensory_neurons'],net['num_output_neurons'])!=(p.sensory,p.output):raise RuntimeError('Snapshot dimensions differ')
            number=port();env=dict(os.environ,AARNN_MINECRAFT_TOKEN=token,RAYON_NUM_THREADS='4',NM_REALTIME_IPC='1')
            with (output/(name+'-rust.log')).open('w') as rust_log,(output/(name+'-bridge.log')).open('w') as bridge_log:
                rust=subprocess.Popen([str(executable),'--tcp',f'127.0.0.1:{number}','--sensory',str(p.sensory),'--output',str(p.output),'--config',str(ROOT/p.config_rel),'--network',str(model)],cwd=output,env=env,stdout=rust_log,stderr=subprocess.STDOUT)
                bridge=None
                try:
                    ready(rust,number,180)
                    bridge=subprocess.Popen([java_path,'-jar',str(ROOT/'sim/minecraft/build/libs/aarnn-minecraft-0.1.0-bridge.jar'),'--port','62620','--base-port',str(number),'--profiles',name],cwd=output,env=env,stdout=bridge_log,stderr=subprocess.STDOUT)
                    ready(bridge,62620,75)
                    request=urllib.request.Request('http://127.0.0.1:62620/api/aarnn/health',headers={'Authorization':'Bearer '+token})
                    with urllib.request.urlopen(request,timeout=5) as response:
                        if response.status!=200:raise RuntimeError('Bridge readiness failed')
                    server.send('scriptevent aarnn:connect '+name)
                    replies=[]
                    for _ in range(4):
                        line=server.until(lambda s:'AARNN_BEDROCK_QA ' in s and '"kind":"response"' in s and '"profile":"'+name+'"' in s,75)
                        replies.append(json.loads(line.split('AARNN_BEDROCK_QA ',1)[1])['reply'])
                    server.send('scriptevent aarnn:disconnect '+name)
                    server.send('scriptevent aarnn:stop')
                    server.until(lambda s:'AARNN all robots stopped' in s)
                    server.verify('stopped-'+name)
                    report['profiles'].append(dict(profile=name,status='pass',frames=len(replies),sensory=p.sensory,output=p.output,snapshot_sha256=model_sha,output_spikes=sum(len(r['output_spike_indices']) for r in replies),response_sha256=hashlib.sha256(json.dumps(replies,sort_keys=True).encode()).hexdigest()))
                    (output/(name+'-native-responses.json')).write_text(json.dumps(replies,indent=2)+'\n')
                    print(name+': native BDS / companion / real Rust snapshot passed',flush=True)
                finally:
                    if bridge is not None:stop(bridge)
                    stop(rust)
            if any(s in (output/(name+'-rust.log')).read_text().lower() for s in ('failed parsing snapshot','startup snapshot import failed','continuing with defaults','failed reading snapshot')):raise RuntimeError('Rust snapshot fallback detected')
        server.close();server=None
        # Reload the same world with traditional RakNet, deliberately occupying
        # both requested UDP ports. The production allocator must move them.
        overrides={'transport':'raknet','enable-lan-visibility':'true'}
        for family,host,key in [(socket.AF_INET,'0.0.0.0','server-port'),(socket.AF_INET6,'::','server-portv6')]:
            if family==socket.AF_INET6 and not socket.has_ipv6:continue
            busy=socket.socket(family,socket.SOCK_DGRAM)
            if family==socket.AF_INET6:busy.setsockopt(socket.IPPROTO_IPV6,socket.IPV6_V6ONLY,1)
            busy.bind((host,0));udp_collisions.append(busy);overrides[key]=str(busy.getsockname()[1])
        lines=[line for line in settings.read_text().splitlines() if line.split('=',1)[0] not in overrides]
        settings.write_text('\n'.join(lines+[key+'='+value for key,value in overrides.items()])+'\n')
        server=Server(directory,output/'reload.log')
        report['reload_port_allocation']=server.allocation
        if any(p['port']==p['requested'] for p in server.allocation['endpoints']):raise RuntimeError('Native occupied RakNet UDP ports were not reassigned')
        server.until(lambda s:'AARNN_BEDROCK_READY ' in s and 'inference=true' in s)
        server.send('scriptevent aarnn:world build')
        server.until(lambda s:'AARNN world already exists' in s)
        report['reload']=server.verify('reloaded')
        if [p['entity_id'] for p in report['world']['profiles']]!=[p['entity_id'] for p in report['reload']['profiles']]:raise RuntimeError('Bodies duplicated/replaced on reload')
        server.close();server=None
        report.update(status='pass',clean_exit=True,content_digest=content['digest'])
    finally:
        try:
            if server is not None:server.close()
        finally:
            collision.close();secret.unlink(missing_ok=True)
            for busy in udp_collisions:busy.close()
            (output/'bedrock-native.json').write_text(json.dumps(report,indent=2)+'\n')
    return 0


if __name__=='__main__':sys.exit(main())
