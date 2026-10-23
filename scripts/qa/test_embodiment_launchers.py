#!/usr/bin/env python3
"""Real native smoke + argument/failure checks for root/convenience launchers."""
import json
import os
import select
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
import urllib.request
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'scripts'))
from embodiment import NEW,LEGACY
class Launchers(unittest.TestCase):
    def run_script(self,script,*args,timeout=90,env=None):
        return subprocess.run(['bash',str(ROOT/script),*map(str,args)],cwd=ROOT,env=env or os.environ,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=timeout)
    def test_relevant_shell_syntax_and_help(self):
        files=['run_sim.sh','run_webot.sh','scripts/embodiment_dispatch.sh','scripts/run_multi_robot_webots.sh']+[f'scripts/run_{p}_sim.sh' for p in ['mujoco','isaacsim','unity','unreal','webgl','minecraft']]+['scripts/run_sim.sh']
        for file in files:
            r=subprocess.run(['bash','-n',str(ROOT/file)],capture_output=True,text=True);self.assertEqual(r.returncode,0,r.stderr)
        r=self.run_script('run_sim.sh','--help');self.assertEqual(r.returncode,0,r.stdout)
        for profile in NEW+LEGACY:self.assertIn(profile,r.stdout)
        self.assertIn('physics-service',r.stdout)
    def test_root_mujoco_and_convenience_preserve_all_profiles(self):
        with tempfile.TemporaryDirectory(prefix='aarnn-launch-test-') as d:
            output=Path(d)/'all.json'
            r=self.run_script('run_sim.sh','--sim','mujoco','--robots',','.join(p+'=1' for p in NEW+LEGACY),'--control','npc','--no-build','--headless','--steps','8','--output',output)
            self.assertEqual(r.returncode,0,r.stdout);rows=json.loads(output.read_text())['worlds'];self.assertEqual(len(rows),28);self.assertEqual({r['profile'] for r in rows},set(NEW+LEGACY))
            output=Path(d)/'one.json'
            r=self.run_script('scripts/run_mujoco_sim.sh','--robots','crow_corvus_corone=1','--no-build','--headless','--steps','8','--output',output)
            self.assertEqual(r.returncode,0,r.stdout);self.assertEqual(json.loads(output.read_text())['worlds'][0]['steps'],8)
    def test_both_webots_entrypoints_use_native_controller(self):
        with tempfile.TemporaryDirectory(prefix='aarnn-launch-test-') as d:
            for i,script in enumerate(['run_webot.sh','scripts/run_multi_robot_webots.sh']):
                output=Path(d)/str(i)
                r=self.run_script(script,'--no-build','--robots','ugot_mecanum=1','--headless','--steps','8','--output',output)
                self.assertEqual(r.returncode,0,r.stdout);report=json.loads((output/'result.json').read_text());self.assertEqual(report['worlds']['ugot_mecanum_0']['steps'],8)
    def test_unavailable_engines_and_unsupported_cluster_maps_fail(self):
        with tempfile.TemporaryDirectory(prefix='aarnn-launch-test-') as d:
            env=dict(os.environ,NM_UNITY_EDITOR='/unavailable/unity',NM_ISAACSIM_PYTHON='')
            r=self.run_script('scripts/run_unity_sim.sh','--robots','ugot_mecanum=1','--no-build','--headless','--steps','8','--output',Path(d)/'unity',env=env)
            self.assertEqual(r.returncode,2,r.stdout);self.assertIn('Unity Editor unavailable',r.stdout)
            r=self.run_script('scripts/run_isaacsim_sim.sh','--robots','crow_corvus_corone=1','--no-build','--headless',env=env)
            self.assertEqual(r.returncode,2,r.stdout);self.assertIn('Isaac Sim unavailable',r.stdout)
            r=self.run_script('run_webot.sh','--robots','ugot_mecanum=1','--config-map','ugot_mecanum_0=not-a-config','--headless','--no-build')
            self.assertEqual(r.returncode,2,r.stdout);self.assertIn('explicit --reference-bindings',r.stdout)
    def test_webgl_and_minecraft_wrappers_launch_real_authority(self):
        with tempfile.TemporaryDirectory(prefix='aarnn-launch-service-') as d:
            for index,script in enumerate(('scripts/run_webgl_sim.sh','scripts/run_minecraft_sim.sh')):
                token_file=Path(d)/str(index)
                process=subprocess.Popen(['bash',str(ROOT/script),'--robots','crow_corvus_corone=1','--physics-service','--control','silent','--no-build','--web-port','0','--token-file',str(token_file)],cwd=ROOT,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
                try:
                    self.assertTrue(select.select([process.stdout],[],[],20)[0],'native service readiness deadline')
                    ready=json.loads(process.stdout.readline());self.assertEqual(ready['status'],'ready')
                    headers={'Authorization':'Bearer '+token_file.read_text().strip()}
                    with urllib.request.urlopen(urllib.request.Request(ready['url'].rstrip('/')+'/state',headers=headers),timeout=3) as response:
                        state=json.load(response)
                    self.assertEqual(state['profile_id'],'crow_corvus_corone')
                    with urllib.request.urlopen(urllib.request.Request(ready['url'].rstrip('/')+'/stop',data=b'{}',headers=headers|{'Content-Type':'application/json'}),timeout=3) as response:
                        self.assertEqual(response.status,202)
                    deadline=time.monotonic()+3
                    while time.monotonic()<deadline:
                        with urllib.request.urlopen(urllib.request.Request(ready['url'].rstrip('/')+'/state',headers=headers),timeout=3) as response:
                            stopped=json.load(response)
                        if stopped['disarmed'] and stopped['actuation_inhibited']:break
                        time.sleep(.02)
                    self.assertTrue(stopped['disarmed'] and stopped['actuation_inhibited'])
                    self.assertIsNone(process.poll())
                finally:
                    process.terminate();_,errors=process.communicate(timeout=10)
                    self.assertEqual(process.returncode,0,errors)
if __name__=='__main__':unittest.main()
