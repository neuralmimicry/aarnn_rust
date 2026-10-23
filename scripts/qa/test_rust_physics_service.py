#!/usr/bin/env python3
"""Native Rust HTTP authority: real dynamics, contact scene, epochs and denial."""
import json
import os
from pathlib import Path
import select
import subprocess
import tempfile
import unittest
import urllib.error
import urllib.request
ROOT = Path(__file__).resolve().parents[2]

class NativeService(unittest.TestCase):
    def test_actual_native_authority_and_lifecycle(self):
        with tempfile.TemporaryDirectory(prefix='aarnn-physics-test-') as directory:
            token_file = Path(directory) / 'token'
            process = subprocess.Popen([str(ROOT/'target/debug/aarnn-physics-service'), '--profile', 'ugot_mecanum', '--control','silent', '--token-file', str(token_file), '--port', '0'], cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            try:
                self.assertTrue(select.select([process.stdout], [], [], 20)[0], 'readiness deadline')
                ready = json.loads(process.stdout.readline())
                self.assertEqual(ready['status'], 'ready')
                token = token_file.read_text().strip()
                self.assertEqual(token_file.stat().st_mode & 0o777, 0o600)
                def request(path, body=None, auth=True, origin=None):
                    headers = {'Authorization': 'Bearer '+token} if auth else {}
                    if origin: headers['Origin'] = origin
                    data = None if body is None else json.dumps(body).encode()
                    req = urllib.request.Request(ready['url'].rstrip('/')+path, data=data, headers=headers | ({'Content-Type':'application/json'} if data else {}))
                    with urllib.request.urlopen(req, timeout=3) as response: return json.load(response)
                def denied(path, code, **kwargs):
                    with self.assertRaises(urllib.error.HTTPError) as failure: request(path, **kwargs)
                    self.assertEqual(failure.exception.code, code)
                    failure.exception.close()
                denied('/state', 403, auth=False)
                denied('/state', 403, origin='http://untrusted.invalid')
                model = request('/model')
                state = request('/state')
                def command(action, epoch=0, **kwargs):
                    return request('/command', {'action': action, 'reset_epoch':epoch, 'model_digest':model['digest'], **kwargs})
                command('pause')
                cp = command('checkpoint')['checkpoint']
                self.assertEqual(cp['model_digest'],model['digest'])
                command('step')
                self.assertEqual(request('/state')['step'],cp['step']+1)
                before = command('measure')['values']
                command('obstacle',slot=0,position=[1.3,0,.5])
                after = command('measure')['values']
                self.assertNotEqual(before,after,'mirror must affect real ray measurements')
                # Replace/remove mirrors atomically; a rejected batch publishes
                # none of its proposed positions, and stale writers are rejected.
                config_epoch=request('/state')['world_config_epoch']
                denied('/command',409,body={'action':'collision_set','model_digest':model['digest'],'reset_epoch':0,'world_config_epoch':config_epoch,'voxels':[[2,0,.5],[0,0,0]]})
                self.assertEqual(request('/state')['obstacles']['0'],[1.3,0,.5])
                command('collision_set',world_config_epoch=config_epoch,voxels=[[1.3,0,.5],[2,1,.5]])
                denied('/command',409,body={'action':'collision_set','model_digest':model['digest'],'reset_epoch':0,'world_config_epoch':config_epoch,'voxels':[]})
                config_epoch=request('/state')['world_config_epoch']
                command('collision_set',world_config_epoch=config_epoch,voxels=[[1.3,0,.5]])
                self.assertEqual(len(request('/state')['obstacles']),1)
                saved = command('checkpoint')['checkpoint']
                command('reset')
                denied('/command',409,body={'action':'resume','model_digest':model['digest'],'reset_epoch':0})
                state = request('/state')
                self.assertEqual(state['reset_epoch'],1)
                self.assertTrue(state['actuation_inhibited'])
                command('restore',epoch=1,checkpoint=saved)
                state = request('/state')
                self.assertEqual(state['step'],saved['step'])
                self.assertEqual(state['reset_epoch'],2)
                self.assertEqual(state['obstacles']['0'],[1.3,0,.5])
                self.assertTrue(state['disarmed'] and state['actuation_inhibited'])
                denied('/command',409,body={'action':'arm','model_digest':model['digest'],'reset_epoch':2})
                request('/stop',{})
                self.assertIsNone(process.poll(),'viewer detach does not own the world')
                node=subprocess.run(['node','--experimental-vm-modules','scripts/qa/test_minecraft_physics.cjs'],cwd=ROOT,
                    env=dict(os.environ,NM_TEST_PHYSICS_URL=ready['url'].rstrip('/'),NM_TEST_PHYSICS_TOKEN=token),
                    stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,timeout=20)
                self.assertEqual(node.returncode,0,node.stdout)
                self.assertIn('"status":"pass"',node.stdout)
                print(node.stdout.strip())
                # Java adapter logic is tested against the same real owner when
                # its catalogue lane has built the classes. The native Rust lane
                # records explicit not-run when the Java build is absent.
                classes=ROOT/'sim/minecraft/build/classes/java/main'
                gson=list((Path.home()/'.gradle/caches/modules-2/files-2.1/com.google.code.gson/gson/2.10.1').glob('*/*.jar'))
                if (classes/'org/neuralmimicry/minecraft/PhysicsSession.class').is_file() and gson:
                    config={k:state[k] for k in ['world_id','profile_id','model_digest','io_digest']}
                    config.update(schema_version=1,endpoint=ready['url'].rstrip('/'),token_environment='AARNN_PHYSICS_TEST_TOKEN',origin=[0,64,0])
                    config_file=Path(directory)/'java-physics.json';config_file.write_text(json.dumps(config))
                    java_home=Path('/usr/lib/jvm/java-25-openjdk-amd64');classpath=str(classes)+':'+str(gson[0])
                    compiled=subprocess.run([str(java_home/'bin/javac'),'--release','21','-cp',classpath,'-d',directory,str(ROOT/'scripts/qa/PhysicsSessionProbe.java')],capture_output=True,text=True,timeout=20)
                    self.assertEqual(compiled.returncode,0,compiled.stderr)
                    java=subprocess.run([str(java_home/'bin/java'),'-cp',directory+':'+classpath,'org.neuralmimicry.minecraft.PhysicsSessionProbe',str(config_file)],env=dict(os.environ,AARNN_PHYSICS_TEST_TOKEN=token),capture_output=True,text=True,timeout=20)
                    self.assertEqual(java.returncode,0,java.stderr)
                    self.assertIn('"status":"pass"',java.stdout)
                    print(java.stdout.strip())
                else: print('Java native-service client: not-run; build the Minecraft adapter first')


            finally:
                process.terminate()
                stdout, stderr = process.communicate(timeout=10)
                self.assertEqual(process.returncode,0,stderr)
            self.assertFalse(list((ROOT/'sim/content/embodiments/generated').glob('.physics-mirror-*.xml')))

if __name__ == '__main__': unittest.main()
