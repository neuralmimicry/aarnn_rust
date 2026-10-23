#!/usr/bin/env python3
"""Read-only installed-toolchain inventory; presence never proves execution.

Optional SSH hosts must be explicitly selected through NM_EMBODIMENT_QA_SSH.
No engine, service, workload or package is installed, started or stopped.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess

PROBE = r'''
import json, os, platform, shutil, subprocess
from pathlib import Path
home=Path.home()
roots=[home/'Developer',Path('/opt'),Path('/usr/local'),home/'.local/share/ov/pkg',home/'isaacsim',home/'Unity/Hub/Editor']
candidates=[]
checked=[]
for root in roots:
    checked.append(str(root))
    if not root.is_dir():continue
    paths=[root]
    try:paths+=list(root.iterdir())
    except PermissionError:continue
    for path in paths:
        if not path.is_dir():continue
        for relative in ('python.sh','isaac-sim.sh','Editor/Unity','Unity'):
            item=path/relative
            if item.is_file():candidates.append(str(item))
configured={key:os.environ.get(key) for key in ('NM_ISAACSIM_PYTHON','NM_UNITY_EDITOR','UE_ENGINE','AARNN_MUJOCO_DIR','WEBOTS_HOME')}
gpu=shutil.which('nvidia-smi')
try:
    gpu_info=subprocess.check_output([gpu,'--query-gpu=name,memory.total,driver_version','--format=csv,noheader,nounits'],text=True,timeout=10).strip() if gpu else None
except (OSError,subprocess.SubprocessError):gpu_info=None
print(json.dumps(dict(system=platform.system(),architecture=platform.machine(),checked_roots=checked,
    configured=configured,runtime_candidates=sorted(set(candidates)),gpu=gpu_info,
    known_tools={name:shutil.which(name) for name in ('webots','Unity','isaacsim','cargo')},
    scope='configured launch paths and standard installation roots only; no engine execution or exhaustive filesystem absence claim')))
'''


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);args=p.parse_args()
    hosts=[host for host in os.environ.get('NM_EMBODIMENT_QA_SSH','').split(',') if host]
    if len(hosts)>8 or any(not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.@:-]*',h) for h in hosts):p.error('at most eight explicit SSH host names')
    def inspect(host):
        command=['ssh','-o','BatchMode=yes','-o','ConnectTimeout=10',host,'python3 -'] if host else ['python3','-']
        try:
            result=subprocess.run(command,input=PROBE,text=True,capture_output=True,timeout=30,check=True)
            return dict(host=host or 'local',status='observed',command=command,observation=json.loads(result.stdout))
        except (OSError,ValueError,subprocess.SubprocessError) as error:
            return dict(host=host or 'local',status='unavailable',command=command,reason=str(error))
    with ThreadPoolExecutor(max_workers=4) as pool:observations=list(pool.map(inspect,[None]+hosts))
    report=dict(schema_version=1,generated_utc=datetime.now(timezone.utc).isoformat(),observations=observations,
        scope='toolchain discovery only; no native profile/target acceptance promoted',
        reproduction='NM_EMBODIMENT_QA_SSH='+','.join(hosts)+' cargo xtask qa run --suite lec-crow-capabilities')
    args.output.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
    return 0 if all(o['status']=='observed' for o in observations) else 1


if __name__=='__main__':raise SystemExit(main())
