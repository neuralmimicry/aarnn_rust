#!/usr/bin/env python3
"""SIM-NAO-INTERACTION-001: contracts, real Rust output and optional real browser."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import urllib.request

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'scripts'))
from run_nao_social import stop_process
from nao_social import CONTRACT


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--browser',action='store_true');args=parser.parse_args()
    base=ROOT/'target/qa/nao-social';base.mkdir(parents=True,exist_ok=True)
    output=Path(tempfile.mkdtemp(prefix='run-',dir=base))
    result=dict(scenario='SIM-NAO-INTERACTION-001',started_utc=datetime.now(timezone.utc).isoformat(),
        git_revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
        git_dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)),
        scenario_digest=hashlib.sha256((ROOT/'qa/scenarios/SIM-NAO-INTERACTION-001.toml').read_bytes()).hexdigest(),
        contract_digest=hashlib.sha256((ROOT/'sim/nao/interaction.json').read_bytes()).hexdigest(),
        seed=0,real_neural=[],browser='pending' if args.browser else 'not requested',biology='engineered LIF reflexes; no trained language or learning improvement claim')
    process=None;started=time.monotonic()
    try:
        with (output/'unit.log').open('w') as log:subprocess.run([sys.executable,ROOT/'scripts/qa/test_nao_social.py'],stdout=log,stderr=subprocess.STDOUT,check=True,timeout=60)
        with (output/'launcher.log').open('w') as log:
            process=subprocess.Popen([sys.executable,ROOT/'scripts/run_nao_social.py','--no-engine','--run-dir',output/'runtime','--http-port','0','--body-port','0'],cwd=ROOT,stdout=log,stderr=subprocess.STDOUT)
        deadline=time.monotonic()+180
        while not (output/'runtime/session.json').is_file():
            if process.poll() is not None or time.monotonic()>deadline:raise RuntimeError('NAO launcher readiness failed')
            time.sleep(.1)
        settings=json.loads((output/'runtime/session.json').read_text())
        result['model_sha256']=settings['model_sha256']
        def call(path,data,credential=None):
            request=urllib.request.Request(settings['url']+path,data=json.dumps(data).encode(),headers={'Content-Type':'application/json','Authorization':'Bearer '+(credential or settings['adapter_token'])})
            with urllib.request.urlopen(request,timeout=15) as response:return json.load(response)
        owner=call('/api/body/open',{})['body_session']
        def frame():return call('/api/body',dict(body_session=owner,input_values=[0]*250))
        frame();token=call('/api/join',{},settings['join_token'])['player_token']
        cases=[(None,'inquiry'),('hello','greeting'),('your name','identity'),('help','help'),('thanks','thanks'),('goodbye','goodbye'),('yes','affirmation'),('no','negation'),('elephant','unrecognised')]
        for seq,(text,expected) in enumerate(cases):
            data=call('/api/encounter',dict(kind='player',sequence=0,capture_ns=0),token) if text is None else call('/api/turn',dict(text=text,sequence=seq,capture_ns=seq+1,modality='typed'),token)
            for _ in range(64):
                frame();reply=call('/api/poll',dict(id=data['id']),token)
                if reply['state'] not in ('queued','active'):break
            assert reply['state']=='replied',reply['state']
            assert reply['reply']['act']==expected,(expected,reply['reply']['act'])
            result['real_neural'].append(dict(expected=expected,**reply))
            for _ in range(16):frame()
        call('/api/body/close',dict(body_session=owner))
        if args.browser:
            env=dict(os.environ,NM_NAO_QA_SESSION=str(output/'runtime/session.json'),NM_NAO_QA_OUTPUT=str(output))
            with (output/'browser.log').open('w') as log:subprocess.run(['node',ROOT/'scripts/qa/test_nao_social_browser.cjs'],cwd=ROOT,env=env,stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
            result['browser']='pass'
        result['status']='pass'
    except Exception as error:
        result.update(status='fail',error=str(error))
    finally:
        if process:stop_process(process)
        result['wall_seconds']=time.monotonic()-started
        (output/'result.json').write_text(json.dumps(result,indent=2)+'\n')
        print(f"{result['status']}: {output}")
    return 0 if result['status']=='pass' else 1

if __name__=='__main__':raise SystemExit(main())
