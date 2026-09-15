"""Behavioural admission/isolation tests; synthetic output here is not neural proof."""
import copy
import hashlib
import http.client
import json
from pathlib import Path
import socket
import struct
import sys
import threading
import unittest

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'scripts'))
from nao_social import Hub, CONTRACT, SocialError, text_sample, lexical_features
from nao_social_server import BoundedHttp, handler, read_frame, frame_values
from build_nao_social import build

class HubTests(unittest.TestCase):
    def setUp(self):
        self.now=1.;self.h=Hub(lambda:self.now);self.h.attach('body');self.p=self.h.register('java:one')
    def sample(self,seq=0,text='hello'):
        return dict(sequence=seq,capture_ns=seq+1,text=text,modality='typed')
    def frames(self,n,outputs=()):
        for _ in range(n):
            turn,features=self.h.before_frame('body');self.h.after_frame('body',turn,self.h.last_step+1,list(outputs))
    def test_duplicate_conflict_and_isolation(self):
        data=self.sample();t=self.h.submit(self.p,data);self.assertEqual(t,self.h.submit(self.p,data))
        with self.assertRaises(SocialError):self.h.submit(self.p,self.sample(text='different'))
        other=self.h.register('java:two')
        with self.assertRaises(SocialError):self.h.get(other,t['id'])
        self.frames(16,[40]);self.assertEqual(self.h.get(self.p,t['id'])['reply']['act'],'greeting')
        self.h.stop(self.p);self.h.submit(other,self.sample());self.frames(128,[40])
        self.assertIsNone(self.h.turns[self.h.players[other]['turn']].reply)
    def test_cancel_during_step_and_disconnect(self):
        t=self.h.submit(self.p,self.sample());key,_=self.h.before_frame('body');self.h.stop(self.p)
        self.h.after_frame('body',key,0,[40]);self.assertIsNone(self.h.get(self.p,t['id'])['reply'])
        self.h.detach('body');self.h.attach('new')
        with self.assertRaises(SocialError):self.h.before_frame('body')
        with self.assertRaises(SocialError):self.h.get(self.p,t['id'])
    def test_silence_does_not_speak(self):
        t=self.h.submit(self.p,self.sample());self.frames(CONTRACT['turn_frames'])
        self.assertEqual(self.h.get(self.p,t['id'])['state'],'no unambiguous neural reply')
    def test_encounter_and_capabilities(self):
        t=self.h.encounter(self.p,dict(kind='player',sequence=0,capture_ns=1));self.assertEqual(t['modality'],'scene_encounter')
        self.assertEqual(self.h.encounter(self.p,dict(sequence=0,capture_ns=1))['state'],'observed')
        _,features=self.h.before_frame('body');self.assertEqual(features[29],1);self.assertFalse(any(features[:8]))
        self.frames(16,[48]);self.assertEqual(self.h.get(self.p,t['id'])['reply']['act'],'inquiry')
        for kind in ('celegans','drosophila_banc','drosophila_fafb','hexapod','zebrafish'):
            p=self.h.register('java:'+kind);self.assertEqual(self.h.encounter(p,dict(kind=kind,sequence=0,capture_ns=1))['state'],'observed')
        with self.assertRaises(SocialError):self.h.encounter(self.p,dict(position=[1,1,1]))
    def test_bounds_and_leave(self):
        for i in range(7):self.h.submit(self.h.register('java:q'+str(i)),self.sample())
        self.h.submit(self.p,self.sample())
        with self.assertRaises(SocialError):self.h.submit(self.h.register('java:overflow'),self.sample())
        self.h.leave(self.p);self.assertNotIn(self.p,self.h.players)
        for i in range(32):p=self.h.register();self.h.leave(p)
        for bad in ('', 'a\n', '\ud800', '💧'*65):
            with self.assertRaises(SocialError):text_sample(bad)
        self.assertEqual(len(text_sample('💧'*64)[1]),256)
        self.assertEqual(lexical_features('HELLO'),lexical_features('hello'))
    def test_stall_and_sequence(self):
        t=self.h.submit(self.p,self.sample());self.now+=11
        self.assertIn('stalled',self.h.get(self.p,t['id'])['state'])
        with self.assertRaises(SocialError):self.h.submit(self.p,self.sample(1))
    def test_allocation_bound(self):
        a,b=socket.socketpair()
        try:
            a.sendall(struct.pack('<I',65537))
            with self.assertRaises(SocialError):read_frame(b)
        finally:a.close();b.close()
    def test_native_amplitude_codec(self):
        frame=b'AER1'+struct.pack('<Q',1000)+bytes([0,128,32,255,1])
        values=frame_values(frame)
        self.assertEqual(values[0],1)
        self.assertEqual(sum(values),1)
        with self.assertRaises(SocialError):frame_values(frame[:-2]+bytes([128,2]))
    def test_http_identity_origin_and_body_separation(self):
        class Body:
            hub=self.h
        server=BoundedHttp(('127.0.0.1',0),handler(Body(),'adapter-key','invite-key'))
        thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
        def post(path,token,data,origin=None):
            c=http.client.HTTPConnection('127.0.0.1',server.server_port,timeout=3)
            try:
                headers={'Content-Type':'application/json','Authorization':'Bearer '+token}
                if origin:headers['Origin']=origin
                c.request('POST',path,json.dumps(data),headers);r=c.getresponse();return r.status,json.loads(r.read())
            finally:c.close()
        try:
            code,data=post('/api/join','invite-key',{});self.assertEqual(code,200);token=data['player_token']
            self.assertEqual(post('/api/body/open',token,{})[0],409)
            self.assertEqual(post('/api/turn',token,self.sample(), 'https://attacker.invalid')[0],409)
            self.assertEqual(post('/api/turn','invite-key',self.sample())[0],409)
            self.assertEqual(post('/api/turn','adapter-key',dict(self.sample(),player='fabricated-browser'))[0],409)
            self.assertEqual(post('/api/turn',token,self.sample())[0],200)
            self.assertEqual(post('/api/leave',token,{})[0],200)
            self.assertEqual(post('/api/turn',token,self.sample(1))[0],409)
        finally:server.shutdown();server.server_close();thread.join()

class ModelTests(unittest.TestCase):
    def test_source_preservation(self):
        raw=(ROOT/'network_nao.json').read_bytes();source=json.loads(raw);before=copy.deepcopy(source)
        result=build(source,hashlib.sha256(raw).hexdigest());self.assertEqual(source,before)
        def equal_matrix(a,b):
            for row in range(a['rows']):self.assertEqual(a['data'][row*a['cols']:(row+1)*a['cols']],b['data'][row*b['cols']:row*b['cols']+a['cols']])
        for name in ('w_in','p_in','w_out','p_out'):equal_matrix(source[name],result[name])
        for name in ('w_hh_fwd','p_fwd','w_hh_bwd','p_bwd','w_hh_rec','p_rec'):
            for a,b in zip(source[name],result[name]):equal_matrix(a,b)
        for name in ('sensory_nodes','output_nodes'):
            self.assertEqual(source['connectome_labels'][name],result['connectome_labels'][name][:len(source['connectome_labels'][name])])
        self.assertEqual(result['net']['sensory_target_layer'],0)
        self.assertEqual(result['net']['num_output_neurons'],49)
        self.assertEqual(len(result['topo']['sensory_nodes']),282)
        self.assertFalse(result['net']['use_aarnn_delays'])

if __name__=='__main__':unittest.main()
