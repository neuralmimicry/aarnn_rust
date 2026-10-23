#!/usr/bin/env python3
"""Hash the reviewable task diff, excluding immutable supplied input packs."""
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess

ROOT=Path(__file__).resolve().parents[2]
OUTPUT=ROOT/'qa/fixtures/lec-crow/changes.current.json'


def main():
    names=subprocess.check_output(['git','ls-files','--modified','--others','--exclude-standard','-z'],cwd=ROOT).decode().split('\0')
    files=[]
    for name in sorted(set(names)-{''}):
        if name.startswith(('.agent/robot-expansion/','aarnn-lec-crow-agent-pack/')) or name in ('webots_world/worlds/multi_neuroworld.wbt','webots_world/worlds/.multi_neuroworld.wbproj','MUJOCO_LOG.TXT'):continue
        path=ROOT/name
        if path==OUTPUT or not path.is_file():continue
        files.append(dict(path=name,bytes=path.stat().st_size,mode=oct(path.stat().st_mode&0o777),sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
    report=dict(schema_version=1,generated_utc=datetime.now(timezone.utc).isoformat(),
        git_revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
        branch=subprocess.check_output(['git','branch','--show-current'],cwd=ROOT,text=True).strip(),
        scope='current task diff; original input packs, unrelated user Webots world/project edits and MuJoCo runtime log excluded; manifest excludes itself',files=files)
    OUTPUT.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(dict(files=len(files),bytes=sum(f['bytes'] for f in files),manifest=str(OUTPUT))))


if __name__=='__main__':main()
