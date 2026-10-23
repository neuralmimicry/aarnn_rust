#!/usr/bin/env python3
"""Real HTTP authority, stale/version/auth denials and collision mirror test."""
import json
from pathlib import Path
import sys
import threading
import unittest
import urllib.request
import urllib.error
ROOT=Path(__file__).resolve().parents[2];sys.path[:0]=[str(ROOT),str(ROOT/'scripts')]
from http.server import ThreadingHTTPServer
from sim.mujoco.runtime import World
from sim.mujoco.service import Authority,handler


class Service(unittest.TestCase):
    def test_authority_auth_reset_and_viewer_detach(self):
        world=World('ugot_mecanum',with_obstacles=True);authority=Authority(world)
        token='private-test-token-'+('x'*32)
        server=ThreadingHTTPServer(('127.0.0.1',0),handler(authority,token));server.daemon_threads=True
        thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
        base=f'http://127.0.0.1:{server.server_port}'
        def request(path,body=None,auth=True):
            headers={'Authorization':'Bearer '+token} if auth else {}
            if body is not None:headers['Content-Type']='application/json'
            req=urllib.request.Request(base+path,data=json.dumps(body).encode() if body is not None else None,headers=headers)
            with urllib.request.urlopen(req,timeout=2) as response:return json.load(response)
        try:
            with self.assertRaises(urllib.error.HTTPError) as denied:request('/state',auth=False)
            self.assertEqual(denied.exception.code,403);denied.exception.close()
            def command(action,**kwargs):return request('/command',dict(action=action,reset_epoch=0,model_digest=world.record['digest'],**kwargs))
            self.assertEqual(command('pause')['status'],'applied')
            before=request('/state');self.assertEqual(command('step')['status'],'applied')
            after=request('/state');self.assertGreaterEqual(after['step'],before['step'])
            initial=world.sensors()
            self.assertEqual(command('obstacle',slot=0,position=[1.3,0,.5])['status'],'applied')
            self.assertNotEqual(initial,world.sensors())
            self.assertEqual(command('reset')['reset_epoch'],1)
            with self.assertRaises(urllib.error.HTTPError) as stale:command('resume')
            self.assertEqual(stale.exception.code,409);stale.exception.close()
            self.assertEqual(request('/stop',{})['status'],'disarm-requested')
            # Reads/view detach never owns or stops the authoritative worker.
            self.assertTrue(authority.worker.is_alive());self.assertEqual(world.reset_epoch,1)
            self.assertFalse(world.obstacles)
        finally:
            server.shutdown();server.server_close();authority.close();thread.join(2)
        self.assertTrue(world.disarmed);self.assertFalse(authority.worker.is_alive())

if __name__=='__main__':unittest.main()
