#!/usr/bin/env python3
"""Bounded native body/Rust/chat round trip; GUI interaction is separate evidence."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request

ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT/'scripts'))
from run_nao_social import stop_process


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--engine',choices=['webots','unreal'],required=True);args=parser.parse_args()
    base=ROOT/'target/qa/nao-social';base.mkdir(parents=True,exist_ok=True)
    out=Path(tempfile.mkdtemp(prefix=args.engine+'-',dir=base));launcher=engine=None;world=None
    result=dict(engine=args.engine,status='fail',scope='native sensor/body proxy/Rust/chat HTTP; no microphone or native-window click proof')
    try:
        with (out/'launcher.log').open('w') as log:
            launcher=subprocess.Popen([sys.executable,ROOT/'scripts/run_nao_social.py','--no-engine','--sim',args.engine,'--run-dir',out/'runtime','--http-port','0','--body-port','0'],stdout=log,stderr=subprocess.STDOUT)
        deadline=time.monotonic()+180
        while not (out/'runtime/session.json').exists():
            if launcher.poll() is not None or time.monotonic()>deadline:raise RuntimeError('Launcher readiness failed')
            time.sleep(.1)
        s=json.loads((out/'runtime/session.json').read_text())
        env=dict(os.environ,NM_NAO_SOCKET=s['uds'],NM_NAO_SESSION_FILE=str(out/'runtime/session.json'),
            NM_NAO_CHAT_URL=s['url'],NM_NAO_JOIN_TOKEN=s['join_token'],NM_IPC_LOCKSTEP_MAX_WAIT_MS='5000',
            NM_CAMERA_RETINA_WIDTH='8',NM_CAMERA_RETINA_HEIGHT='6',NM_UE_ROBOTS='nao=1',NM_AARNN_HOST='127.0.0.1',NM_AARNN_BASE_PORT=str(s['body_port']))
        if args.engine=='webots':
            world=ROOT/'webots_world/worlds'/('.nao_probe_'+out.name+'.wbt')
            world.write_text((ROOT/'webots_world/worlds/neuroworld.wbt').read_text())
            command=[shutil.which('webots') or 'webots','--batch','--mode=fast','--no-rendering','--stdout','--stderr',world]
        else:
            executable=Path(os.environ.get('UE_ENGINE',str(Path.home()/'Developer/Engine')))/'Binaries/Linux/UnrealEditor'
            command=[executable,ROOT/'sim/unreal/NeuralMimicrySim.uproject','/Engine/Maps/Entry?game=/Script/NmAerBridge.NmSimGameMode',
                     '-game','-RenderOffscreen','-unattended','-nosound','-windowed','-ResX=1280','-ResY=900']
        with (out/'engine.log').open('w') as log:engine=subprocess.Popen(command,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
        def status():
            with urllib.request.urlopen(s['url']+'/api/status',timeout=5) as r:return json.load(r)
        def call(path,data,token):
            q=urllib.request.Request(s['url']+path,data=json.dumps(data).encode(),headers={'Content-Type':'application/json','Authorization':'Bearer '+token})
            try:
                with urllib.request.urlopen(q,timeout=5) as r:return json.load(r)
            except urllib.error.HTTPError as error:
                raise RuntimeError(path+': '+json.load(error).get('error','interaction rejected')) from None
        deadline=time.monotonic()+180
        while True:
            state=status()
            if state['available'] and state['output_step_index']>=0:break
            if engine.poll() is not None or time.monotonic()>deadline:raise RuntimeError('Native body did not become ready')
            time.sleep(.2)
        token=call('/api/join',{},s['join_token'])['player_token']
        turn=call('/api/turn',dict(text='hello',modality='typed',sequence=1,capture_ns=1),token)
        deadline=time.monotonic()+45
        while turn['state'] in ('queued','active'):
            if time.monotonic()>deadline:raise RuntimeError('Native neural reply deadline exceeded')
            time.sleep(.1);turn=call('/api/poll',dict(id=turn['id']),token)
        if turn['state']!='replied' or turn['reply']['act']!='greeting':raise RuntimeError('Native body did not sustain a neural greeting')
        result.update(status='pass',turn=turn,model_sha256=s['model_sha256'])
    except Exception as error:result['error']=str(error)
    finally:
        if engine:stop_process(engine)
        if launcher:stop_process(launcher)
        if world:
            world.unlink(missing_ok=True)
            world.with_name('.'+world.stem+'.wbproj').unlink(missing_ok=True)
        (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(result['status']+': '+str(out))
    return 0 if result['status']=='pass' else 1

if __name__=='__main__':raise SystemExit(main())
