#!/usr/bin/env python3
"""Native NPC encounter through installed BDS, companion, social proxy and Rust."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'scripts'))
from run_nao_social import stop_process
from probe_minecraft_bedrock import Server


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--trace',action='store_true');args=parser.parse_args()
    base=ROOT/'target/qa/nao-social';base.mkdir(parents=True,exist_ok=True)
    out=Path(tempfile.mkdtemp(prefix='bedrock-',dir=base));launcher=server=None
    started=time.monotonic()
    result=dict(status='fail',engine='Bedrock Dedicated Server',trace=args.trace,
        started_utc=datetime.now(timezone.utc).isoformat(),
        scope='exported world, native villager encounter, companion JAR, proxy, real Rust inquiry and native name-tag bubble; client rendering not tested')
    try:
        with (out/'launcher.log').open('w') as log:
            launcher=subprocess.Popen([sys.executable,ROOT/'scripts/run_nao_social.py','--sim','minecraft',
                '--minecraft-edition','bedrock','--no-engine','--run-dir',out/'runtime',
                '--http-port','0','--body-port','0'],stdout=log,stderr=subprocess.STDOUT)
        deadline=time.monotonic()+180
        while 'Isolated Minecraft installation:' not in (out/'launcher.log').read_text():
            if launcher.poll() is not None or time.monotonic()>deadline:raise RuntimeError('Isolated installation readiness failed')
            time.sleep(.2)
        settings=json.loads((out/'runtime/session.json').read_text())
        result['model_sha256']=settings['model_sha256']
        result['observer_sha256']=hashlib.sha256((ROOT/'scripts/qa/nao_bedrock_probe.js').read_bytes()).hexdigest()
        with (ROOT/'sim/minecraft/build/bedrock/AARNN-Bedrock-Sensory-Lab.mcworld').open('rb') as stream:
            result['world_sha256']=hashlib.file_digest(stream,'sha256').hexdigest()
        install=out/'runtime/bedrock-server';scripts=install/'behavior_packs/AARNN_Lab_BP/scripts'
        shutil.copy2(ROOT/'scripts/qa/nao_bedrock_probe.js',scripts/'social-qa.js')
        with (scripts/'main.js').open('a') as stream:stream.write("\nimport './social-qa.js';\n")
        if args.trace:
            # Trace only status/error from this scripted NPC fixture, never credentials
            # or a player's message. Keep instrumentation in the isolated QA copy.
            social=scripts/'nao-chat.js'
            code=social.read_text()
            code=code.replace("const response=await io.socialRequest(path,data);",
                "console.warn('AARNN_SOCIAL_QA_REQUEST '+path);const response=await io.socialRequest(path,data);"
                "const qa=JSON.parse(response.body);console.warn('AARNN_SOCIAL_QA_RESPONSE '+JSON.stringify({path,status:response.status,state:qa.state,error:qa.error}));")
            code=code.replace("} catch(error) {stop(player.id);", "} catch(error) {console.warn('AARNN_SOCIAL_QA_ERROR '+String(error));stop(player.id);")
            code=code.replace("} catch { /* No loaded NAO", "} catch(error) { console.warn('AARNN_SOCIAL_QA_SCAN '+String(error)); /* No loaded NAO")
            social.write_text(code)
        # Normal server startup also exercises the eagerly negotiated companion
        # connection remaining idle before the first native body frame.
        server=Server(install,out/'native.log')
        result['port_allocation']=server.allocation
        version=server.until(lambda line:'Version: ' in line,180)
        result['version']=version.split('Version: ',1)[1].strip()
        server.until(lambda line:'AARNN_BEDROCK_READY ' in line,180)
        server.send('scriptevent aarnn:world build')
        server.until(lambda line:'world already exists' in line,90)
        server.send('scriptevent aarnn:connect nao')
        server.send('scriptevent aarnnqa:encounter spawn')
        identity=server.until(lambda line:'AARNN_NPC_ID ' in line,30)
        if identity.split('AARNN_NPC_ID ',1)[1].strip()!='minecraft:villager_v2':raise RuntimeError('Native villager identity corrupted')
        line=server.until(lambda line:'AARNN_SOCIAL_NATIVE ' in line,120)
        observation=json.loads(line.split('AARNN_SOCIAL_NATIVE ',1)[1])
        if 'What can you teach me' not in observation['bubble']:raise RuntimeError('Expected neural inquiry')
        result.update(status='pass',native_npc_identity=True,observation=observation)
    except Exception as error:result['error']=str(error)
    finally:
        if server:
            try:server.close()
            except Exception as error:result.update(status='fail',shutdown_error=str(error))
        if launcher:stop_process(launcher)
        result['wall_seconds']=time.monotonic()-started
        (out/'result.json').write_text(json.dumps(result,indent=2)+'\n')
        print(result['status']+': '+str(out))
    return 0 if result['status']=='pass' else 1


if __name__=='__main__':raise SystemExit(main())
