#!/usr/bin/env python3
"""Launch native Unity/Chaos bodies using one shared graph and Rust AER ABI.

An explicitly isolated AER1 reference experiment. Does not start or manage a
brain. Unity requires a separately installed supported Editor; no mock is used.
"""
import argparse
import json
import math
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'scripts'))
from robot_profiles import iter_brain_instances
from embodiment import NEW,LEGACY,OUT

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--target',choices=['unity','unreal'],required=True);p.add_argument('--robots',default='ugot_mecanum=1');p.add_argument('--steps',type=int,default=100);p.add_argument('--headless',action='store_true');p.add_argument('--reference-bindings',type=Path);p.add_argument('--output',type=Path,required=True);p.add_argument('--no-build',action='store_true');p.add_argument('--capture',action='store_true');p.add_argument('--drop-height',type=float,default=0);p.add_argument('--control',choices=['auto','npc','silent','aarnn'],default='auto');p.add_argument('--reset-at',type=int,default=-1);p.add_argument('--diagnostic-joint');p.add_argument('--diagnostic-effort',type=float,default=.01);p.add_argument('--diagnostic-steps',type=int,default=20);a=p.parse_args()
    entries=list(iter_brain_instances(a.robots));bindings=json.loads(a.reference_bindings.read_text()) if a.reference_bindings else {}
    if not 1<=len(entries)<=16 or any(profile not in NEW+LEGACY for _,profile in entries):p.error('1..16 explicit profile instances required')
    if not 1<=a.steps<=10000000 or not math.isfinite(a.drop_height) or not 0<=a.drop_height<=10:p.error('bounded steps/drop height required')
    if a.reset_at!=-1 and (a.target!='unreal' or not 1<=a.reset_at<a.steps):p.error('native reset requires Unreal and a boundary in 1..steps-1; Unity lifecycle acceptance remains unavailable')
    if set(bindings)-{i for i,_ in entries}:p.error('binding for absent instance')
    if len({b['brain_id'] for b in bindings.values()})!=len(bindings) or len({(b['host'],b['port']) for b in bindings.values()})!=len(bindings):p.error('distinct brains and endpoints required')
    if bindings and a.diagnostic_joint:p.error('diagnostics cannot run in neural mode')
    if not math.isfinite(a.diagnostic_effort) or abs(a.diagnostic_effort)>2 or not 0<=a.diagnostic_steps<=100:p.error('bounded diagnostic effort required')
    a.output=a.output.resolve()
    if a.output.exists() and any(a.output.iterdir()):p.error('choose an empty evidence directory')
    a.output.mkdir(parents=True,exist_ok=True)
    engine=Path(os.environ.get('UE_ENGINE',str(Path.home()/'Developer/Engine')))
    editor=os.environ.get('NM_UNITY_EDITOR')
    if a.target=='unity' and (not editor or not Path(editor).is_file()):
        (a.output/'capability.json').write_text(json.dumps(dict(status='blocked',reason='Set NM_UNITY_EDITOR to an installed Unity 2022.3 LTS+ Editor; factory source is not executed acceptance'))+'\n')
        print('Unity Editor unavailable: set NM_UNITY_EDITOR',file=sys.stderr);return 2
    if a.target=='unreal' and not (engine/'Binaries/Linux/UnrealEditor').is_file():p.error('set UE_ENGINE to installed engine directory')
    def run(command,log):
        with (a.output/log).open('w') as f:subprocess.run(command,cwd=ROOT,stdout=f,stderr=subprocess.STDOUT,check=True)
    if not a.no_build:
        run(['python3','scripts/generate_embodiment_ffi.py','--check'],'binding-check.log')
        run(['cargo','build','--locked','-p','aarnn-embodiment-ffi'],'rust-build.log')
        if a.target=='unreal':run([str(engine/'Build/BatchFiles/Linux/Build.sh'),'NeuralMimicrySimEditor','Linux','Development','-Project='+str(ROOT/'sim/unreal/NeuralMimicrySim.uproject'),'-WaitMutex'],'engine-build.log')
    instances=[];scene_cursor=0.;previous_extent=0.
    for index,(instance,profile) in enumerate(entries):
        graph=ROOT/'sim/content/embodiments/graphs'/f'{profile}.json'
        if not graph.is_file():p.error('generate rigid graphs with the pinned offline MuJoCo converter first')
        value=json.loads(graph.read_text())
        if a.diagnostic_joint and a.diagnostic_joint not in {j['name'] for j in value['joints']}:p.error('diagnostic joint absent from '+profile)
        extent=(value.get('scene') or {}).get('half_extent_m',1.5)
        if index:scene_cursor+=previous_extent+extent+.5
        previous_extent=extent
        instances.append(dict(instance_id=instance,model=str(OUT/f'{profile}.xml'),graph=str(graph),binding_json=json.dumps(bindings[instance]) if instance in bindings else '',offset_m=[scene_cursor,0,a.drop_height]))
    result=a.output/'engine-result.json'
    manifest=dict(capture=str(a.output/'capture.png') if a.capture else '',control='silent' if a.diagnostic_joint else a.control,schema_version=1,dt_us=2000,steps=a.steps,reset_at=a.reset_at,output=str(result),rust_library=str(ROOT/'target/debug/libaarnn_embodiment_ffi.so'),diagnostic_joint=a.diagnostic_joint or '',diagnostic_effort=a.diagnostic_effort,diagnostic_steps=a.diagnostic_steps,instances=instances)
    scene=a.output/'scene.json';scene.write_text(json.dumps(manifest,indent=2)+'\n')
    env=dict(os.environ,NM_EMBODIMENT_SCENE=str(scene))
    if a.target=='unreal':
        command=[str(engine/'Binaries/Linux/UnrealEditor'),str(ROOT/'sim/unreal/NeuralMimicrySim.uproject'),'/Engine/Maps/Entry?game=/Script/NmAerBridge.NmSimGameMode','-game','-unattended','-nosound','-NoSplash','-NoVerifyGC','-log','-UseFixedTimeStep','-FPS=500','-NoLiveCoding','-NoUBA','-ini:EditorSettings:[/Script/UnrealEd.CrashReportsPrivacySettings]:bSendUnattendedBugReports=False','-ini:EditorSettings:[/Script/UnrealEd.AnalyticsPrivacySettings]:bSendUsageData=False']+(['-RenderOffscreen','-ResX=1280','-ResY=960','-ForceRes'] if a.capture else ['-nullrhi'] if a.headless else [])
    else:
        # Package links in ignored output; one canonical Rust library and one
        # canonical Assets tree, never a fork of the robot implementation.
        project=a.output/'unity-project';(project/'Assets').mkdir(parents=True);(project/'Packages').mkdir();(project/'ProjectSettings').mkdir()
        (project/'Assets/NeuralMimicry').symlink_to(ROOT/'sim/unity/Assets/NeuralMimicry',target_is_directory=True)
        plugin=project/'Assets/Plugins/x86_64';plugin.mkdir(parents=True);(plugin/'libaarnn_embodiment_ffi.so').symlink_to(ROOT/'target/debug/libaarnn_embodiment_ffi.so')
        (project/'Packages/manifest.json').write_text(json.dumps({'dependencies':{'com.unity.modules.physics':'1.0.0','com.unity.modules.imgui':'1.0.0'}}))
        command=[editor]+(['-batchmode'] if a.headless else [])+['-projectPath',str(project),'-executeMethod','NeuralMimicry.EmbodimentEditor.Run','-logFile',str(a.output/'unity-editor.log')]+(['-nographics'] if a.headless else [])
    with (a.output/'engine.log').open('w') as log:
        process=subprocess.Popen(command,cwd=ROOT,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
        try:
            code=process.wait(timeout=max(120,a.steps*.01))
        except (subprocess.TimeoutExpired,KeyboardInterrupt):
            os.killpg(process.pid,signal.SIGTERM)
            try:process.wait(timeout=10)
            except subprocess.TimeoutExpired:os.killpg(process.pid,signal.SIGKILL);process.wait()
            code=124
    report=json.loads(result.read_text()) if result.exists() else None
    status='pass' if code==0 and report and report.get('steps')==a.steps and (not a.capture or (a.output/'capture.png').is_file()) else 'fail'
    summary=dict(schema_version=1,status=status,target=a.target,command=command,exit_code=code,report=report,scope='native source construction and bounded mechanics/reference exchange only; target calibration, lifecycle and trained tasks remain open')
    (a.output/'result.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps(dict(status=status,evidence=str(a.output))))
    return 0 if status=='pass' else 1
if __name__=='__main__':raise SystemExit(main())
