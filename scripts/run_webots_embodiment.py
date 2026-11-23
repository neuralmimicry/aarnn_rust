#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Private native Webots embodiment experiment; existing six-profile route stays intact."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'scripts'))
from embodiment import NEW,LEGACY,OUT
from embodiment_webots import DEST,emit,world
from robot_profiles import iter_brain_instances

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--robots',default='ugot_mecanum=1');p.add_argument('--steps',type=int,default=1000);p.add_argument('--headless',action='store_true');p.add_argument('--realtime',action='store_true');p.add_argument('--reference-bindings',type=Path);p.add_argument('--output',type=Path,required=True);p.add_argument('--reset-at',type=int);p.add_argument('--no-build',action='store_true');p.add_argument('--diagnostic-joint');p.add_argument('--diagnostic-effort',type=float,default=.01);p.add_argument('--diagnostic-steps',type=int,default=20);p.add_argument('--control',choices=['auto','npc','silent','aarnn'],default='auto');p.add_argument('--drop-height',type=float,default=0);p.add_argument('--lockstep',action='store_true',help='wait for each reference reply before the next 2 ms step (MuJoCo --lockstep)');p.add_argument('--arm-file',type=Path,help='managed startup gate: no physics before this file exists');p.add_argument('--trace',type=Path,help='new directory for INSTANCE.jsonl traces (MuJoCo trace fields)');p.add_argument('--trace-every',type=int,default=25);args=p.parse_args()
    entries=list(iter_brain_instances(args.robots))
    if not entries or len(entries)>16 or any(profile not in NEW+LEGACY for _,profile in entries):p.error('native expansion route requires 1..16 physical profiles; preserve legacy preview launchers')
    if not 1<=args.steps<=10000000:p.error('bounded steps required')
    bindings=json.loads(args.reference_bindings.read_text()) if args.reference_bindings else {}
    if any(key not in {i for i,_ in entries} for key in bindings):p.error('binding for absent instance')
    brains=[b['brain_id'] for b in bindings.values()];endpoints=[(b['host'],b['port']) for b in bindings.values()]
    if len(set(brains))!=len(brains) or len(set(endpoints))!=len(endpoints):p.error('distinct bodies require distinct brain and endpoint identities')
    if bindings and args.diagnostic_joint:p.error('mechanical diagnostics cannot run in neural mode')
    if args.lockstep and args.realtime:p.error('--lockstep replaces --realtime pacing')
    if args.trace and args.trace.exists() and any(args.trace.iterdir()):p.error('choose an empty trace directory')
    if not args.no_build:subprocess.run(['cargo','build','--locked','-p','aarnn-webots','--features','native'],cwd=ROOT,check=True)
    args.output=args.output.resolve()
    if args.output.exists() and any(args.output.iterdir()):p.error('choose an empty output directory; prior evidence is immutable')
    args.output.mkdir(parents=True,exist_ok=True)
    project=Path(tempfile.mkdtemp(prefix='native-webots-',dir=args.output));(project/'worlds').mkdir();controller=project/'controllers/aarnn_webots';controller.mkdir(parents=True)
    # The executable is built once in the workspace. This is a controller
    # packaging link in ignored run output, never a copied Rust implementation.
    (controller/'aarnn_webots').symlink_to(Path(os.environ.get('NM_WEBOTS_CONTROLLER_BIN',ROOT/'target/debug/aarnn-webots')).resolve())
    robots=[];scene_cursor=0.;previous_extent=0.
    for index,(instance,profile) in enumerate(entries):
        # The device/target map is emitted with this run's world from the
        # current generated model, so the two can never drift apart (the
        # checked-in DEST map is a static export for inspection).
        config=dict(model=str(OUT/f'{profile}.xml'),target=str(project/(instance+'.webots.json')),instance_id=instance,output=str(args.output/f'{instance}.json'),steps=args.steps,binding=bindings.get(instance),control='silent' if args.diagnostic_joint else args.control,reset_at=args.reset_at,realtime=args.realtime,lockstep=args.lockstep,arm_file=str(args.arm_file.resolve()) if args.arm_file else None,trace=str(args.trace.resolve()) if args.trace else None,trace_every=args.trace_every,quit_when_done=len(entries)==1,diagnostic=dict(joint=args.diagnostic_joint,effort=args.diagnostic_effort,steps=args.diagnostic_steps) if args.diagnostic_joint else None)
        config_path=project/(instance+'.json');config_path.write_text(json.dumps(config,indent=2)+'\n')
        extent=json.loads((OUT/f'{profile}.json').read_text()).get('scene',{}).get('half_extent_m',1.5)
        if index:scene_cursor+=previous_extent+extent+.5
        previous_extent=extent
        source,meta=emit(profile,instance,offset=(scene_cursor,0,args.drop_height),args=[str(config_path)])
        (project/(instance+'.webots.json')).write_text(json.dumps(meta,indent=2)+'\n')
        robots.append(source)
    scene=project/'worlds/embodiments.wbt';scene.write_text(world('\n'.join(robots)))
    command=['webots','--batch','--mode=fast','--stdout','--stderr']+(['--no-rendering','--minimize'] if args.headless else [])+[str(scene)]
    env=dict(os.environ);sdk=env.get('WEBOTS_HOME','/usr/local/webots');env['LD_LIBRARY_PATH']=str(Path(sdk)/'lib/controller')+(':'+env['LD_LIBRARY_PATH'] if env.get('LD_LIBRARY_PATH') else '')
    with (args.output/'webots.log').open('w') as log:
        process=subprocess.Popen(command,cwd=ROOT,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
        cancelled=False
        def halt(*_):
            nonlocal cancelled
            cancelled=True
        signal.signal(signal.SIGINT,halt);signal.signal(signal.SIGTERM,halt)
        try:
            # Lock-step and managed arming are bounded by the brain, not real time.
            deadline=time.monotonic()+(max(600,args.steps*.002*40) if args.lockstep or args.arm_file else max(90,args.steps*.002*3))
            while time.monotonic()<deadline and process.poll() is None and not cancelled:
                if all((args.output/f'{instance}.json').exists() for instance,_ in entries):break
                text=(args.output/'webots.log').read_text(errors='replace')
                if 'Failed to load due to' in text or 'controller exited with status:' in text:break
                time.sleep(.05)
        finally:
            if process.poll() is None:os.killpg(process.pid,signal.SIGTERM)
            try:process.wait(timeout=10)
            except subprocess.TimeoutExpired:os.killpg(process.pid,signal.SIGKILL);process.wait()
    reports={instance:json.loads((args.output/f'{instance}.json').read_text()) if (args.output/f'{instance}.json').exists() else dict(status='fail',reason='controller report missing; inspect webots.log') for instance,_ in entries}
    result=dict(schema_version=1,status='pass' if all(r['status']=='pass' for r in reports.values()) else 'fail',command=command,scene_sha256=hashlib.sha256(scene.read_bytes()).hexdigest(),worlds=reports,scope='native Webots mechanics/optional reference AER only; task and parity gates independent')
    (args.output/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(status=result['status'],evidence=str(args.output))))
    return 0 if result['status']=='pass' else 1
if __name__=='__main__':raise SystemExit(main())
