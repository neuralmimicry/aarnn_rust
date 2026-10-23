#!/usr/bin/env python3
"""Export an installed Webots body to URDF without connecting a neural brain.

The export is an intermediate import artefact, not a licence grant. Store under
ignored target output; redistribution is separately reviewed per source model.
"""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
ROOT=Path(__file__).resolve().parents[2]
CASES={'celegans':'CelegansRobot','drosophila_banc':'DrosophilaBancRobot','drosophila_fafb':'DrosophilaFafbRobot','hexapod':'HexapodRobot','nao':None,'zebrafish':'ZebrafishRobot'}


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--profile',choices=CASES,required=True);parser.add_argument('--out',type=Path,required=True);args=parser.parse_args()
    args.out=args.out.resolve();args.out.mkdir(parents=True,exist_ok=True)
    profile=args.profile;kind=profile.replace('_','-');proto=CASES[profile]
    report=args.out/'body.urdf'
    with tempfile.TemporaryDirectory(prefix='aarnn-export-') as name:
        project=Path(name);(project/'worlds').mkdir();controller=project/'controllers/export_body';controller.mkdir(parents=True)
        world=project/'worlds/import.wbt'
        command=[sys.executable,str(ROOT/'scripts/build_webots_multi_world.py'),'--world',str(world),'--'+kind+'-brains',profile+'_export']
        if proto:command+=['--'+kind+'-proto',str(ROOT/f'webots_world/protos/{proto}.proto')]
        subprocess.run(command,check=True,cwd=ROOT,stdout=subprocess.DEVNULL)
        content=world.read_text().replace('controller "nao_nn_controller_uds"','controller "export_body"').replace('controller "nm_world_recorder"','controller "<none>"')
        world.write_text(content)
        (controller/'export_body.py').write_text(f'''from controller import Robot
from pathlib import Path
import json
r=Robot()
r.step(int(r.getBasicTimeStep()))
Path({str(report)!r}).write_text(r.getUrdf())
Path({str(args.out/'devices.json')!r}).write_text(json.dumps([r.getDeviceByIndex(i).getName() for i in range(r.getNumberOfDevices())]))
while r.step(int(r.getBasicTimeStep())) != -1:pass
''')
        with (args.out/'webots.log').open('w') as log:
            process=subprocess.Popen(['webots','--batch','--mode=fast','--no-rendering','--stdout','--stderr',str(world)],cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
            try:
                deadline=time.monotonic()+90
                while not report.exists() and process.poll() is None and time.monotonic()<deadline:time.sleep(.05)
                if not report.exists() or not report.read_text().strip():raise RuntimeError('Webots URDF export failed; inspect '+str(args.out/'webots.log'))
            finally:
                if process.poll() is None:os.killpg(process.pid,signal.SIGTERM)
                try:process.wait(timeout=8)
                except subprocess.TimeoutExpired:os.killpg(process.pid,signal.SIGKILL);process.wait()
    print(report)

if __name__=='__main__':main()
