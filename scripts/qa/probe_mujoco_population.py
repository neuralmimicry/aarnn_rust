#!/usr/bin/env python3
"""Actual mixed preserved brains plus an unbound NPC, with native viewers."""
import argparse
import json
import os
from pathlib import Path
import socket
from run_embodiments import run
from probe_mujoco_managed import ROOT,digest,port


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);p.add_argument('--startup-timeout',type=int,default=600);args=p.parse_args();out=args.output.resolve();out.mkdir(parents=True,exist_ok=True)
    if not 1<=args.startup_timeout<=3600:p.error('--startup-timeout must be 1..3600 seconds')
    originals=[ROOT/'network_hexapod.json',ROOT/'network_celegans.json',ROOT/'webots_world/worlds/multi_neuroworld.wbt']
    before={str(p.relative_to(ROOT)):digest(p) for p in originals}
    base=None
    for _ in range(20):
        candidate=port();held=[]
        try:
            for n in range(3):
                s=socket.socket();held.append(s);s.bind(('127.0.0.1',candidate+n))
            base=candidate;break
        except OSError:pass
        finally:
            for s in held:s.close()
    if base is None:raise RuntimeError('three private ports unavailable')
    command=['bash','scripts/run_sim.sh','--sim','mujoco','--robots','hexapod=2,celegans=1,ugot_mecanum=1','--no-build','--nodes','3','--tcp-base-port',str(base),'--tcp-ready-timeout',str(args.startup_timeout),'--steps','1500','--realtime','--output',str(out/'worlds.json'),'--screenshot',str(out/'capture.ppm')]
    env=dict(os.environ,NM_DISABLE_OPENCL='1',RAYON_NUM_THREADS='2')
    code=run(command,out/'run.log',max(600,args.startup_timeout+300),env)
    assert code==0,f'population exit={code}; see {out}'
    rows=json.loads((out/'worlds.json').read_text())['worlds'];assert len(rows)==4
    brains=set()
    for r in rows:
        assert r['steps']==1500 and r['status']=='pass',r
        if r['profile']=='ugot_mecanum':
            assert r['brain_id'] is None and r['npc_steps']==1500 and r['transport'] is None
        else:
            assert r['brain_id']==r['instance_id'] and r['npc_steps']==0
            assert r['transport']['ready'] and r['transport']['fault'] is None
            assert r['transport']['exchanges']>=2 and r['applied']>0 and r['active_steps']>0
            brains.add(r['brain_id'])
    assert brains=={'hexapod_0','hexapod_1','celegans_0'}
    assert (out/'capture.ppm').stat().st_size>100000
    assert before=={str(p.relative_to(ROOT)):digest(p) for p in originals}
    for n in range(3):
        with socket.socket() as s:assert s.connect_ex(('127.0.0.1',base+n))!=0,'private bridge leaked'
    result=dict(status='pass',scope='three distinct preserved neural bindings plus one NPC; per-world interactive step budgets, actual native rendering; no learned-task claim',command=command,preserved_sources=before,worlds=[{k:r[k] for k in ('profile','instance_id','brain_id','steps','npc_steps','active_steps','applied','model_digest','io_digest')} for r in rows])
    (out/'population.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))


if __name__=='__main__':main()
