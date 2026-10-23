#!/usr/bin/env python3
"""Bounded real Runner/AER reference exchange; never a trained-task claim."""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
ROOT=Path(__file__).resolve().parents[2]
sys.path[:0]=[str(ROOT),str(ROOT/'scripts')]
from robot_aer import AsyncReferenceClient
from sim.mujoco.runtime import World
from embodiment_lab import ReferenceRunner


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--profile',default='galbot_g1_golf');parser.add_argument('--rust',action='store_true');parser.add_argument('--webots',action='store_true');parser.add_argument('--unreal',action='store_true');parser.add_argument('--out',type=Path,required=True);args=parser.parse_args()
    args.out.mkdir(parents=True,exist_ok=True)
    world=World(args.profile)
    record=world.record;io=record['io']
    lab=ReferenceRunner(args.profile,world.instance_id,args.out)
    contract=lab.contract;manifest=lab.manifest;command=lab.command;port=contract['port']
    result=dict(schema_version=1,profile=args.profile,brain_id=world.brain_id,model_digest=record['digest'],io_digest=io['digest'],
                command=command,mode='AER1-reference-untrained',governed_effects=False,learned_task_status='not_run',
                mechanics_diagnostic_controller=False,samples=[])
    client=None
    try:
        lab.__enter__()
        if args.rust or args.webots or args.unreal:
            sdk=Path(os.environ.get('AARNN_MUJOCO_DIR',str(Path.home()/'Developer/mujoco-3.17.0')))
            native_command=[str(ROOT/'target/debug/aarnn-mujoco'),str(ROOT/'sim/content/embodiments/generated'/f'{args.profile}.xml'),'--steps','1000','--control','aarnn','--headless','--realtime','--reference-binding',str(manifest),'--output',str(args.out/'native.json')]
            if args.webots or args.unreal:
                mapping=args.out/'bindings.json';mapping.write_text(json.dumps({world.instance_id:contract},indent=2)+'\n')
                native_command=[sys.executable,'scripts/run_webots_embodiment.py','--robots',args.profile+'=1','--steps','1000','--control','aarnn','--headless','--realtime','--no-build','--reference-bindings',str(mapping),'--output',str(args.out/'physics')]
                if args.unreal:native_command=[sys.executable,'scripts/run_native_embodiment.py','--target','unreal','--robots',args.profile+'=1','--steps','1000','--control','aarnn','--headless','--no-build','--reference-bindings',str(mapping),'--output',str(args.out/'physics')]
            env=dict(os.environ,LD_LIBRARY_PATH=str(sdk/'lib')+(':'+os.environ['LD_LIBRARY_PATH'] if os.environ.get('LD_LIBRARY_PATH') else ''))
            native=subprocess.run(native_command,cwd=ROOT,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,timeout=150)
            (args.out/'native.log').write_text(native.stdout)
            if native.returncode:raise RuntimeError('native Rust loop failed; see native.log')
            measured=json.loads((args.out/'physics/result.json').read_text())['worlds'][world.instance_id] if args.webots else json.loads((args.out/'physics/engine-result.json').read_text())['instances'][0] if args.unreal else json.loads((args.out/'native.json').read_text())['worlds'][0]
            if args.unreal:measured=json.loads((args.out/'physics/engine-result.json').read_text())['instances'][0]
            if measured['transport'].get('fault'):raise RuntimeError('reference transport fault; inspect native evidence')
            if measured['transport']['exchanges']<10 or measured['applied']<1:raise RuntimeError('insufficient native reference exchanges/applications')
            if measured.get('active_steps',0)<1:raise RuntimeError('no measured nonzero native neural effort; packets alone do not prove actuation')
            result['active_steps']=measured['active_steps']
            result.update(status='pass',implementation='native-chaos-shared-rust' if args.unreal else 'native-webots-rust' if args.webots else 'native-mujoco-rust',scope='real native physics/AER reference loop; no learned task/production commitment claim',exchanges=measured['transport']['exchanges'],spikes=measured['transport']['output_spikes'],applied=measured['applied'],expired=measured['expired'],native_command=native_command)
            latencies=sorted(measured['transport'].get('latency_us',[]))
            if latencies:result['round_trip_latency_us']={name:latencies[min(len(latencies)-1,int((len(latencies)-1)*q))] for name,q in [('p50',.5),('p95',.95),('p99',.99)]}
            result['network_initialisation']='fresh untrained Runner network; initial connectivity uses runtime RNG and is not a fixed learning seed'
            (args.out/'result.json').write_text(json.dumps(result,indent=2)+'\n')
            return 0
        client=AsyncReferenceClient(record,world.binding(),contract,'127.0.0.1',port)
        if not client.ready.wait(10) or client.fault:raise RuntimeError(client.fault or 'handshake timeout')
        # Physics runs independently; one bounded source capture is submitted
        # per cycle here to retain every sample in this diagnostic evidence.
        latencies=[];outputs=0;applied=0;expired=0
        for sequence in range(48):
            for _ in range(5):world.step()
            values=world.sensors();source_step=world.step_index
            if not client.submit(source_step,source_step*2000,values):raise RuntimeError('unexpected admission pressure')
            deadline=time.monotonic()+5;start=time.monotonic();reply=None
            while time.monotonic()<deadline:
                reply=client.receive()
                if reply:break
                if client.fault:raise RuntimeError(client.fault)
                # Continued physics while waiting; no blocking network callback.
                world.step();time.sleep(.002)
            if not reply:raise RuntimeError('AER reply deadline exceeded')
            latencies.append(time.monotonic()-start)
            output_status='applied'
            try:world.accept_reference(reply);applied+=1
            except ValueError as error:
                if str(error)!='late/future reference output':raise
                world.disarm();expired+=1;output_status='expired'
            outputs+=sum(reply['spikes'])
            result['samples'].append(dict(source_step=source_step,source_timestamp_us=source_step*2000,
                                          sensor_min=min(values),sensor_max=max(values),spikes=sum(reply['spikes']),output_status=output_status,
                                          output_timestamp_us=reply['output_timestamp_us'],root_position=world.data.qpos[:3].tolist()))
        if applied==0:raise RuntimeError('no reference output met the pinned 100ms source-step budget')
        result.update(status='pass',applied=applied,expired=expired,scope='real reference Runner packets and local output decoding only',
                      exchanges=client.exchanges,spikes=outputs,latency_s=sorted(latencies),gaps=client.gaps,
                      task_success=None,admission_commit_trace='unavailable in AER1 reference; production gate remains open')
    except Exception as error:result.update(status='fail',reason=str(error))
    finally:
        world.disarm()
        if client:client.close()
        lab.close();(args.out/'result.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({k:v for k,v in result.items() if k!='samples'},indent=2))
    return 0 if result['status']=='pass' else 1

if __name__=='__main__':raise SystemExit(main())
