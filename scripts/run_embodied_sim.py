#!/usr/bin/env python3
"""Launch native embodiments with one physics owner per instance.

Reference AER is explicit; this launcher never attaches physical devices or
starts a live remote brain. Isaac must run under its separate Python launcher.
"""
import argparse
from contextlib import ExitStack
import json
import os
from pathlib import Path
import signal
import sys
import time
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
sys.path.insert(0,str(ROOT/'scripts'))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--sim',choices=['mujoco','isaacsim'],default='mujoco')
    parser.add_argument('--robots',default='galbot_g1_golf=1')
    parser.add_argument('--headless',action='store_true')
    parser.add_argument('--steps',type=int,default=1000)
    parser.add_argument('--realtime',action='store_true')
    parser.add_argument('--output',type=Path)
    parser.add_argument('--reference-bindings',type=Path,help='Explicit per-instance strict AER1 manifests and loopback endpoints')
    args=parser.parse_args()
    if args.steps<1 or args.steps>10000000: parser.error('steps must be 1..10000000')
    if args.sim=='isaacsim':
        parser.error('Isaac native runner is not yet validated: generated USD is available; see sim/isaacsim/README.md. Run the supported Kit integration lane before enabling actuation.')
    try:
        from sim.mujoco.runtime import World
    except ImportError as error:
        parser.error(f'MuJoCo environment unavailable: {error}; use NM_MUJOCO_PYTHON with the pinned venv')
    from robot_profiles import iter_brain_instances
    from robot_aer import AsyncReferenceClient
    entries=list(iter_brain_instances(args.robots))
    if len(entries)>32: parser.error('at most 32 bodies per process')
    worlds=[World(profile,instance) for instance,profile in entries]
    configured=json.loads(args.reference_bindings.read_text()) if args.reference_bindings else {}
    stop=False
    def halt(*_):
        nonlocal stop
        stop=True
        for world in worlds: world.disarm()
    signal.signal(signal.SIGINT,halt);signal.signal(signal.SIGTERM,halt)
    with ExitStack() as stack:
        for world in worlds:
            if world.instance_id in configured:
                config=configured[world.instance_id]
                world.brain_id=config['brain_id']
                world.client=AsyncReferenceClient(world.record,world.binding(),config,config['host'],config['port'])
                stack.callback(world.client.close)
        viewer=None
        if not args.headless:
            if len(worlds)!=1: parser.error('passive interactive viewer currently requires one world; use headless for independent worlds')
            import mujoco.viewer
            viewer=stack.enter_context(mujoco.viewer.launch_passive(worlds[0].model,worlds[0].data))
        print(json.dumps(dict(status='ready',physics_owner='mujoco',mode='AER1-reference' if configured else 'silent-mechanics',
                              instances=[w.instance_id for w in worlds],worlds=len(worlds),shared_contact_world=False)),flush=True)
        deadline=time.monotonic()
        for _ in range(args.steps):
            if stop or (viewer and not viewer.is_running()): break
            for world in worlds: world.step()
            if viewer: viewer.sync()
            if args.realtime:
                deadline+=.002;time.sleep(max(0,deadline-time.monotonic()))
        for world in worlds: world.disarm()
        result=[dict(pose=w.poses(),snapshot=w.snapshot(),transport=dict(exchanges=w.client.exchanges,gaps=w.client.gaps,fault=w.client.fault) if w.client else None) for w in worlds]
        if args.output:
            args.output.parent.mkdir(parents=True,exist_ok=True)
            # Session report may be replaced; published checkpoints use separate immutable APIs.
            args.output.write_text(json.dumps(result,indent=2)+'\n')
        print(json.dumps(dict(status='stopped',steps=[w.step_index for w in worlds])),flush=True)
        return 1 if any(w.client and w.client.fault for w in worlds) else 0

if __name__=='__main__':raise SystemExit(main())
