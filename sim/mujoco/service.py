"""Private loopback reference physics service for functional viewers.

One worker owns all physics and sensing. Viewers submit bounded environmental
interactions, never AER sensory frames or body poses. Production deployment must
use the existing authenticated gateway; this endpoint cannot control hardware.
"""
from __future__ import annotations
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import hmac
import json
import os
from pathlib import Path
import queue
import secrets
import signal
import sys
import threading
import time
ROOT=Path(__file__).resolve().parents[2]
sys.path[:0]=[str(ROOT),str(ROOT/'scripts')]
from sim.mujoco.runtime import World


class Authority:
    def __init__(self,world):
        self.world=world;self.commands=queue.Queue(32);self.stop=threading.Event();self.emergency=threading.Event()
        self.snapshot={};self.lock=threading.Lock();self.fault=None;self.sequence=0
        self.worker=threading.Thread(target=self.run,name='physics-owner',daemon=True)
        self.worker.start()

    def run(self):
        try:
            deadline=time.monotonic()
            while not self.stop.is_set():
                if self.emergency.is_set():self.world.actuation_inhibited=True;self.world.disarm();self.emergency.clear()
                for _ in range(8):
                    try:command,ack=self.commands.get_nowait()
                    except queue.Empty:break
                    try:
                        if command['reset_epoch']!=self.world.reset_epoch:raise ValueError('stale world epoch')
                        action=command['action']
                        if action=='pause':self.world.paused=True
                        elif action=='resume':self.world.paused=False
                        elif action=='step':
                            if not self.world.paused:raise ValueError('single step requires pause')
                            self.world.paused=False;self.world.step();self.world.paused=True
                        elif action=='reset':self.world.reset();self.world.actuation_inhibited=True
                        elif action=='arm':
                            if not self.world.client:raise ValueError('explicit reference binding required before arming')
                            self.world.actuation_inhibited=False
                        elif action=='obstacle':self.world.place_obstacle(command['slot'],command['position'])
                        else:raise ValueError('unsupported action')
                        ack.put_nowait(dict(status='applied',reset_epoch=self.world.reset_epoch))
                    except (ValueError,KeyError,IndexError,TypeError) as error:ack.put_nowait(dict(status='rejected',reason=str(error)))
                self.world.step();self.sequence+=1
                snapshot=dict(self.world.poses(),publication=self.sequence,viewer_mode='functional-physics-service',
                              authority='mujoco-reference',quality='reference-uncommitted-neural-effects' if self.world.client else 'silent-mechanics',
                              world_config_epoch=self.world.world_config_epoch,paused=self.world.paused,
                              queue_depth=self.commands.qsize(),obstacles=self.world.obstacles)
                with self.lock:self.snapshot=snapshot
                deadline+=.002;self.stop.wait(max(0,deadline-time.monotonic()))
        except Exception as error:self.fault=str(error);self.world.disarm()

    def close(self):
        self.emergency.set();self.stop.set();self.worker.join(timeout=5)
        if self.worker.is_alive():raise RuntimeError('physics shutdown deadline')
        self.world.disarm()
        if self.world.client:self.world.client.close()


def handler(authority,token):
    class Handler(BaseHTTPRequestHandler):
        protocol_version='HTTP/1.1'
        def log_message(self,*_):pass # Never log tokens or payloads.
        def send_json(self,status,value):
            data=json.dumps(value,allow_nan=False).encode();self.send_response(status)
            self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(data)))
            self.send_header('Cache-Control','no-store');self.end_headers();self.wfile.write(data)
        def authorised(self):
            # No cross-origin credentials/CORS. The bundled viewer shares origin.
            if self.headers.get('Origin') not in (None,f'http://{self.headers.get("Host")}'):return False
            return hmac.compare_digest(self.headers.get('Authorization',''),'Bearer '+token)
        def do_GET(self):
            if self.path in ('/','/viewer.js','/webgl-world.js'):
                path=ROOT/'web_ui'/('physics-view.html' if self.path=='/' else 'webgl-world.js' if self.path=='/webgl-world.js' else 'physics-view.js')
                data=path.read_bytes();self.send_response(200)
                self.send_header('Content-Type','text/html' if self.path=='/' else 'text/javascript')
                self.send_header('Content-Length',str(len(data)));self.send_header('Cache-Control','no-store');self.end_headers();self.wfile.write(data);return
            if not self.authorised():self.send_json(403,dict(error='private simulation token required'));return
            if self.path=='/state':
                if authority.fault:self.send_json(503,dict(error=authority.fault));return
                with authority.lock:snapshot=authority.snapshot
                self.send_json(200,snapshot)
            elif self.path=='/model':self.send_json(200,authority.world.record)
            else:self.send_json(404,dict(error='unknown route'))
        def do_POST(self):
            if not self.authorised():self.send_json(403,dict(error='private simulation token required'));return
            if self.path not in ('/command','/stop'):self.send_json(404,dict(error='unknown route'));return
            if self.path=='/stop':authority.emergency.set();self.send_json(202,dict(status='disarm-requested'));return
            try:
                size=int(self.headers.get('Content-Length','0'))
                if not 1<=size<=4096:raise ValueError('bounded command required')
                command=json.loads(self.rfile.read(size))
                if command.get('model_digest')!=authority.world.record['digest']:raise ValueError('model mismatch')
                ack=queue.Queue(1);authority.commands.put_nowait((command,ack))
                result=ack.get(timeout=1)
                self.send_json(200 if result['status']=='applied' else 409,result)
            except (ValueError,TypeError,KeyError,json.JSONDecodeError) as error:self.send_json(400,dict(error=str(error)))
            except queue.Full:self.send_json(429,dict(error='command budget exhausted'))
            except queue.Empty:self.send_json(503,dict(error='command outcome unknown; read state before retry'))
    return Handler


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--profile',default='crow_corvus_corone');p.add_argument('--port',type=int,default=8092)
    p.add_argument('--token-file',type=Path,required=True);p.add_argument('--reference-bindings',type=Path);args=p.parse_args()
    args.token_file.parent.mkdir(parents=True,exist_ok=True)
    if args.token_file.exists():token=args.token_file.read_text().strip()
    else:
        token=secrets.token_urlsafe(32)
        fd=os.open(args.token_file,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
        with os.fdopen(fd,'w') as f:f.write(token+'\n')
    if len(token)<32:raise ValueError('private token must have at least 32 characters')
    world=World(args.profile,with_obstacles=True)
    if args.reference_bindings:
        from robot_aer import AsyncReferenceClient
        config=json.loads(args.reference_bindings.read_text())[world.instance_id]
        world.brain_id=config['brain_id'];world.client=AsyncReferenceClient(world.record,world.binding(),config,config['host'],config['port'])
    authority=Authority(world)
    server=ThreadingHTTPServer(('127.0.0.1',args.port),handler(authority,token));server.daemon_threads=True
    print(json.dumps(dict(status='ready',url=f'http://127.0.0.1:{server.server_port}/',token_file=str(args.token_file),world_id=world.instance_id)),flush=True)
    def stop(*_):
        authority.emergency.set()
        threading.Thread(target=server.shutdown,daemon=True).start()
    signal.signal(signal.SIGINT,stop);signal.signal(signal.SIGTERM,stop)
    try:server.serve_forever(poll_interval=.1)
    finally:server.server_close();authority.close()

if __name__=='__main__':main()
