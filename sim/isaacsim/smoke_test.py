#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Headless Kit + PhysX start-up evidence for this machine's Isaac Sim install.

  OMNI_KIT_ACCEPT_EULA=YES target/isaacsim-venv/bin/python sim/isaacsim/smoke_test.py [--output smoke.json]

Boots SimulationApp headless, records Kit/Isaac/GPU identity, drops a 0.1 m
cube on a static slab for 1 s at the 2 ms AER clock (CPU PhysX) and checks it
comes to rest on the surface. No robot, brain or network is involved.
"""
import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--output', type=Path)
    a = ap.parse_args()
    if os.environ.get('OMNI_KIT_ACCEPT_EULA', '').strip().lower() not in ('yes', 'y', '1'):
        raise SystemExit('export OMNI_KIT_ACCEPT_EULA=YES to accept the NVIDIA Omniverse EULA')
    t0 = time.monotonic()
    from isaacsim import SimulationApp
    # Same PhysX settings as run_isaacsim.py: step on the calling thread, no USD writeback.
    app = SimulationApp({'headless': True, 'extra_args': ['--/persistent/physics/numThreads=0']})
    import carb
    for key in ('/physics/updateToUsd', '/physics/updateVelocitiesToUsd'):
        carb.settings.get_settings().set_bool(key, False)
    boot_s = time.monotonic() - t0
    result = dict(status='fail', boot_s=boot_s)
    try:
        import numpy as np
        import omni.kit.app
        import omni.usd
        from pxr import Gf, PhysxSchema, UsdGeom, UsdPhysics
        result['kit_version'] = omni.kit.app.get_app().get_build_version()
        try:
            from importlib.metadata import version
            result['isaacsim_version'] = version('isaacsim')
        except Exception as e:  # pragma: no cover - evidence only
            result['isaacsim_version'] = repr(e)
        try:
            result['gpu'] = subprocess.run(['nvidia-smi', '--query-gpu=name,driver_version,memory.total',
                                            '--format=csv,noheader'], capture_output=True, text=True).stdout.strip()
        except OSError:
            result['gpu'] = None
        omni.usd.get_context().new_stage()
        stage = omni.usd.get_context().get_stage()
        UsdGeom.SetStageUpAxis(stage, UsdGeom.Tokens.z)
        UsdGeom.SetStageMetersPerUnit(stage, 1.0)
        scene = UsdPhysics.Scene.Define(stage, '/physicsScene')
        scene.CreateGravityDirectionAttr(Gf.Vec3f(0, 0, -1))
        scene.CreateGravityMagnitudeAttr(9.81)
        physx = PhysxSchema.PhysxSceneAPI.Apply(scene.GetPrim())
        physx.CreateTimeStepsPerSecondAttr(500)
        physx.CreateEnableGPUDynamicsAttr(False)
        physx.CreateBroadphaseTypeAttr('MBP')
        ground = UsdGeom.Cube.Define(stage, '/ground')
        ground.CreateSizeAttr(2.0)
        ground.AddTranslateOp().Set(Gf.Vec3d(0, 0, -0.05))
        ground.AddScaleOp().Set(Gf.Vec3f(2, 2, 0.05))
        UsdPhysics.CollisionAPI.Apply(ground.GetPrim())
        box = UsdGeom.Cube.Define(stage, '/box')
        box.CreateSizeAttr(0.1)
        box.AddTranslateOp().Set(Gf.Vec3d(0, 0, 0.5))
        UsdPhysics.CollisionAPI.Apply(box.GetPrim())
        UsdPhysics.RigidBodyAPI.Apply(box.GetPrim())
        UsdPhysics.MassAPI.Apply(box.GetPrim()).CreateMassAttr(1.0)
        import omni.physics.core as physics
        import omni.physics.tensors as tensors
        from omni.physx import get_physx_simulation_interface
        from pxr import UsdUtils
        # Kit 110 / Isaac Sim 6.x start-up order (as SimulationManager.initialize_physics).
        physics.get_physics_stage_update_interface().force_load_physics_from_usd()
        physics.get_physics_stage_update_interface().start_simulation()
        sim = get_physx_simulation_interface()
        stage_id = UsdUtils.StageCache.Get().GetId(stage).ToLongInt()
        result['physx_attached_stage'] = sim.get_attached_stage() if hasattr(sim, 'get_attached_stage') else None
        try:
            view = tensors.create_simulation_view('numpy', stage_id=stage_id)
            result['view_without_warmup'] = True
        except Exception:
            sim.simulate(0.002, 0.0)
            sim.fetch_results()
            view = tensors.create_simulation_view('numpy', stage_id=stage_id)
            result['view_without_warmup'] = False
        bodies = view.create_rigid_body_view('/box')
        t = 0.0
        started = time.monotonic()
        heights = []
        for _ in range(500):
            sim.simulate(0.002, t)
            sim.fetch_results()
            t += 0.002
            heights.append(float(bodies.get_transforms().reshape(-1)[2]))
        result['steps'] = 500
        result['step_wall_ms'] = (time.monotonic() - started) / 500 * 1000
        result['box_final_z_m'] = heights[-1]
        result['box_min_z_m'] = min(heights)
        result['status'] = 'pass' if abs(heights[-1] - 0.05) < 0.005 else 'fail'
    except Exception as e:
        result['error'] = f'{type(e).__name__}: {e}'
    print(json.dumps(result, indent=2), flush=True)
    if a.output:
        a.output.parent.mkdir(parents=True, exist_ok=True)
        a.output.write_text(json.dumps(result, indent=2) + '\n')
    code = 0 if result['status'] == 'pass' else 1
    sys.stdout.flush()
    import threading
    threading.Timer(60.0, lambda: os._exit(code)).start()
    app.close(exit_code=code)
    return code


if __name__ == '__main__':
    raise SystemExit(main())
