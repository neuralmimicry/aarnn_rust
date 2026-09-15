#!/usr/bin/env python3
"""Native Bedrock Dedicated Server capability checks and a bounded managed launcher."""
import argparse
import json
import os
from pathlib import Path
import queue
import re
import shutil
import signal
import subprocess
import sys
import threading
import time

ROOT=Path(__file__).resolve().parents[1]
PACKS=ROOT/'sim/minecraft/build/bedrock'


def read_json(path,limit=1024*1024):
    if path.stat().st_size>limit:raise ValueError('metadata too large')
    return json.loads(path.read_text())


def server_directory(explicit=None):
    if explicit:return Path(explicit).expanduser()
    if os.environ.get('NM_BEDROCK_DIR'):return Path(os.environ['NM_BEDROCK_DIR']).expanduser()
    executable=os.environ.get('NM_BEDROCK_SERVER') or shutil.which('bedrock_server') or shutil.which('bedrock_server.exe')
    if executable:return Path(executable).expanduser().resolve().parent
    for candidate in (Path.home()/'bedrock-server',Path.home()/'minecraft-bedrock',Path.home()/'Games/bedrock-server',Path('/opt/minecraft-bedrock')):
        if (candidate/'bedrock_server').exists() or (candidate/'bedrock_server.exe').exists():return candidate
    # Official ZIPs are commonly unpacked into versioned Developer directories.
    # Inspect one directory level only; an observation never starts the server.
    candidates=[p for p in (Path.home()/'Developer').glob('bedrock-server-*')
                if re.fullmatch(r'bedrock-server-\d+(?:\.\d+){1,4}',p.name)]
    for candidate in sorted(candidates,key=lambda p:tuple(map(int,p.name.removeprefix('bedrock-server-').split('.'))),reverse=True):
        if (candidate/'bedrock_server').is_file() or (candidate/'bedrock_server.exe').is_file():return candidate
    return None


def detect(explicit=None):
    directory=server_directory(explicit);errors=[]
    result=dict(edition='bedrock',directory=str(directory) if directory else None,server=None,
        api_version='2.9.0',network_api='1.0.0-beta',runtime_probe_required=True,
        native_runtime_verified=False,errors=errors)
    if directory is None:
        errors.append('Bedrock Dedicated Server not found; set NM_BEDROCK_DIR or NM_BEDROCK_SERVER')
        result['status']='unavailable';return result
    binary=Path(os.environ['NM_BEDROCK_SERVER']).expanduser() if os.environ.get('NM_BEDROCK_SERVER') else directory/('bedrock_server.exe' if os.name=='nt' else 'bedrock_server')
    if binary.is_file() and os.access(binary,os.X_OK):result['server']=str(binary.resolve())
    else:errors.append('Bedrock server executable missing or not executable')
    try:
        metadata=read_json(PACKS/'bedrock-content.json');ids=metadata['pack_ids']
        expected=read_json(ROOT/'sim/content/compiled.generated.json',8*1024*1024)['digest']
        if metadata['content_digest']!=expected:raise ValueError('Generated Bedrock content is stale')
        properties={}
        for line in (directory/'server.properties').read_text().splitlines():
            if '=' in line and not line.startswith('#'):
                key,value=line.split('=',1);properties[key.strip()]=value.strip()
        level=properties.get('level-name','Bedrock level')
        if not level or Path(level).name!=level or level in ('.','..'):raise ValueError('Invalid level-name')
        result['world']=level
        from bedrock_ports import check_world_available
        check_world_available(directory,level)
        for kind,pack_id in (('behavior',ids['behaviour']),('resource',ids['resource'])):
            active=read_json(directory/'worlds'/level/('world_'+kind+'_packs.json'))
            if not isinstance(active,list) or not any(isinstance(p,dict) and p.get('pack_id')==pack_id and p.get('version')==[0,1,0] for p in active):raise ValueError('AARNN '+kind+' pack is not activated for this world')
            copies=[]
            for base in (directory/(kind+'_packs'),directory/'worlds'/level/(kind+'_packs')):
                for manifest_path in base.glob('*/manifest.json'):
                    # Stock Bedrock packs may use JSONC. Only our UUID-bearing
                    # manifests are strict generated JSON; do not reject vanilla.
                    if manifest_path.stat().st_size>1024*1024:raise ValueError('Pack manifest exceeds bound')
                    if pack_id not in manifest_path.read_text():continue
                    candidate=read_json(manifest_path)
                    if isinstance(candidate,dict) and candidate.get('header',{}).get('uuid')==pack_id:copies.append(manifest_path)
            if len(copies)!=1:raise ValueError('Expected one AARNN '+kind+' pack; move duplicate exported/installed copies aside')
            folder='AARNN_Lab_BP' if kind=='behavior' else 'AARNN_Lab_RP'
            manifest=read_json(directory/(kind+'_packs')/folder/'manifest.json')
            if manifest['header']['uuid']!=pack_id or expected not in manifest['header']['description']:raise ValueError('Installed '+kind+' pack is stale')
            source=PACKS/('server' if kind=='behavior' else 'resource')
            for p in source.rglob('*'):
                if p.is_file():
                    installed=directory/(kind+'_packs')/folder/p.relative_to(source)
                    if not installed.is_file() or installed.read_bytes()!=p.read_bytes():raise ValueError('Installed Bedrock pack differs: '+str(p.relative_to(source)))
        settings=directory/'config'/ids['script']
        permissions=read_json(settings/'permissions.json')
        modules=permissions.get('allowed_modules',[]) if isinstance(permissions,dict) else []
        if not isinstance(modules,list) or not all(n in modules for n in ('@minecraft/server','@minecraft/server-net','@minecraft/server-admin')):raise ValueError('Grant the three required modules to the AARNN script UUID')
        variables=read_json(settings/'variables.json')
        if not isinstance(variables,dict) or variables.get('AARNN_CONTENT_DIGEST')!=expected:raise ValueError('BDS variables have stale content digest')
        result['inference_configured']=variables.get('AARNN_ALLOW_SANDBOX_INFERENCE') is True
        # Secret contents are neither loaded nor displayed by detection. Runtime
        # resolves a SecretString and reports availability after module negotiation.
        result['secret_file_present']=(settings/'secrets.json').is_file()
        if not result['inference_configured'] or not result['secret_file_present']:raise ValueError('Configure the scoped BDS inference variable and private secrets.json; see the Bedrock guide')
        if properties.get('allow-cheats','false').lower()!='true':raise ValueError('Operator lab commands require allow-cheats=true in this dedicated lab')
        result['content_digest']=expected
    except (OSError,ValueError,KeyError,TypeError,AttributeError,RuntimeError) as error:
        errors.append(str(error))
    result['status']='unavailable' if errors else 'ready_for_runtime_probe'
    return result


def launch(report,timeout=180):
    from bedrock_ports import launch_directory
    if report['errors']:
        print(json.dumps(report,indent=2));return 3
    try:
        for attempt in range(3):
            with launch_directory(report) as (runtime,reservation):
                # BDS cannot inherit bound sockets. Keep reservations until the
                # spawn boundary and retry only a proved pre-readiness bind race.
                reservation.close()
                code=_launch(runtime,timeout)
            if code!=75:return code
            print('Bedrock port was taken during startup; selecting free ports again.',flush=True)
        print('Bedrock could not acquire listeners after three bounded attempts.',file=sys.stderr)
    except (OSError,ValueError,RuntimeError) as error:
        print('Bedrock launch unavailable: '+str(error),file=sys.stderr)
    return 3


def _launch(report,timeout=180):
    if report['errors']:
        print(json.dumps(report,indent=2));return 3
    events=queue.Queue(maxsize=256);stop=threading.Event();ready=False
    env=dict(os.environ)
    if sys.platform.startswith('linux'):
        env['LD_LIBRARY_PATH']=report['directory']+(':'+env['LD_LIBRARY_PATH'] if env.get('LD_LIBRARY_PATH') else '')
    try:
        process=subprocess.Popen([report['server']],cwd=report['directory'],env=env,stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,bufsize=1)
    except OSError as error:
        print('Bedrock could not start: '+str(error),file=sys.stderr);return 3
    def read():
        while line:=process.stdout.readline(65537):
            events.put(line[:65536])
        events.put(None)
    def input_lines():
        for line in sys.stdin:
            if stop.is_set():return
            if len(line)<=4096:
                try:process.stdin.write(line);process.stdin.flush()
                except (OSError,ValueError):return
    threading.Thread(target=read,daemon=True).start()
    previous={sig:signal.signal(sig,lambda *_:stop.set()) for sig in (signal.SIGINT,signal.SIGTERM)}
    deadline=time.monotonic()+timeout
    try:
        while not stop.is_set():
            if not ready and time.monotonic()>deadline:
                print('Bedrock script readiness failed: check Beta APIs, pack/API versions and scoped permissions.',file=sys.stderr);return 3
            try:line=events.get(timeout=.1)
            except queue.Empty:
                if process.poll() is not None:return 3 if not ready else process.returncode
                continue
            if line is None:
                try:code=process.wait(timeout=5)
                except subprocess.TimeoutExpired:return 3
                return 3 if not ready else code
            print(line,end='',flush=True)
            if not ready and any(marker in line.lower() for marker in ('address already in use','failed to bind','port is already in use','port already in use')):
                return 75
            if ' ERROR]' in line and ('[Scripting]' in line or 'AARNN' in line):
                print('Bedrock AARNN content or script failed; stopping this managed server.',file=sys.stderr)
                return 3
            if 'AARNN_BEDROCK_READY '+report['content_digest']+' inference=true' in line:
                if not ready:threading.Thread(target=input_lines,daemon=True).start()
                ready=True;print('Native Bedrock script modules, catalogue and inference configuration are ready.',flush=True)
        return 0 if ready else 3
    finally:
        stop.set()
        if process.poll() is None:
            try:process.stdin.write('stop\n');process.stdin.flush();process.wait(timeout=15)
            except (OSError,ValueError,subprocess.TimeoutExpired):
                process.terminate()
                try:process.wait(timeout=5)
                except subprocess.TimeoutExpired:process.kill();process.wait()
        for sig,handler in previous.items():signal.signal(sig,handler)


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('command',choices=['doctor','launch'])
    parser.add_argument('--server-dir',type=Path);args=parser.parse_args();report=detect(args.server_dir)
    if args.command=='launch':return launch(report)
    print(json.dumps(report,indent=2));return 3 if report['errors'] else 0


if __name__=='__main__':sys.exit(main())
