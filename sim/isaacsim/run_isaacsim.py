#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Isaac Sim (Kit + PhysX) physics owner for one AARNN embodiment.

Mirrors the native MuJoCo owner (adapters/mujoco/src/main.rs, world.rs): the
generated USD articulation steps at 2 ms; sensors are the MuJoCo owner's
projections (sim/isaacsim/senses.py); captures, replies, motor arbitration and
the graded/pulse-density decoders are the shared Rust ABI
(libaarnn_embodiment_ffi via sim/isaacsim/embodiment_ffi.py). Arm-file gating,
the initial capture, lock-step pacing and the trace schema follow main.rs.

Run under the separate Isaac environment only (NM_ISAACSIM_PYTHON), e.g.

  target/isaacsim-venv/bin/python sim/isaacsim/run_isaacsim.py --robots hexapod=1 \
      --headless --steps 15000 --lockstep --reference-bindings MAP.json \
      --arm-file FILE --output run.json --trace DIR

Isaac never shares an interpreter with the MuJoCo venv. No physical
peripheral, production lease or effect authority is involved.
"""
import argparse
import json
import math
import os
import signal
import sys
import time
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
import embodiment_ffi as ffi  # noqa: E402
import senses as sensing  # noqa: E402

GRAVITY = np.array([0.0, 0.0, -9.81])
DT = 0.002
TRACE_LIMIT = 200_000


def parse_args(argv=None):
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument('--robots', required=True, help='PROFILE=1 (one body per Isaac process)')
    p.add_argument('--assets', type=Path, default=ROOT / 'sim/content/embodiments/generated')
    p.add_argument('--usd-dir', type=Path, help='Load PROFILE.usda from here instead of --assets (e.g. a '
                   'scripts/embodiment_usd.py re-emission of a record whose checked-in USD is stale)')
    p.add_argument('--control', choices=['auto', 'npc', 'silent', 'aarnn'], default='auto')
    p.add_argument('--steps', type=int)
    p.add_argument('--headless', action='store_true')
    p.add_argument('--realtime', action='store_true')
    p.add_argument('--lockstep', action='store_true')
    p.add_argument('--reference-binding', type=Path)
    p.add_argument('--reference-bindings', type=Path)
    p.add_argument('--arm-file', type=Path)
    p.add_argument('--output', type=Path)
    p.add_argument('--trace', type=Path)
    p.add_argument('--trace-every', type=int, default=25)
    p.add_argument('--json', action='store_true')
    p.add_argument('--device', default='cpu', help='PhysX pipeline: cpu (default, deterministic) or cuda')
    p.add_argument('--physx-threads', type=int, default=0,
                   help='PhysX CPU worker threads; 0 (default) steps on the calling thread, fastest for one small '
                        'articulation on a loaded host')
    p.add_argument('--physics-owner', choices=['isaacsim', 'mujoco-twin'], default='isaacsim',
                   help='mujoco-twin: diagnostic only; same Python body/ABI path on MuJoCo dynamics, to separate '
                        'port errors from PhysX/MuJoCo physics differences')
    a = p.parse_args(argv)
    if a.steps is None:
        a.steps = 1000
    if not 1 <= a.steps <= 10_000_000:
        p.error('step budget')
    if not 1 <= a.trace_every <= 100_000:
        p.error('trace interval outside 1..=100000 steps')
    if a.lockstep and a.realtime:
        p.error('--lockstep replaces --realtime pacing')
    if not a.headless:
        p.error('Isaac runner is headless-only here; pass --headless --steps N')
    return a


def resolve_entries(spec, assets):
    registry = json.loads((assets / 'registry.generated.json').read_text())
    if registry['schema_version'] != 1:
        raise SystemExit('registry schema')
    entries = []
    for token in [t for t in spec.replace(';', ',').split(',') if t.strip()]:
        name, count = token.split('=', 1)
        normalized = name.strip().lower().replace('-', '_').replace(' ', '_')
        profile = next((q for q in registry['profiles'] if q['id'] == normalized or normalized in q['aliases']), None)
        if profile is None:
            raise SystemExit(f'unknown profile {name}')
        for index in range(int(count)):
            entries.append((f"{profile['id']}_{index}", profile))
    if len(entries) != 1:
        raise SystemExit('Isaac runner owns exactly one body per process (independent worlds); '
                         'launch one process per body')
    return entries


def quat_wxyz_to_matrix(q):
    w, x, y, z = q
    return np.array([[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
                     [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
                     [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]])


def speed_allows(target, q, velocity, limit):
    """Port of aarnn_embodiment::motor::servo_speed_allows."""
    return all(math.isfinite(v) for v in (target, q, velocity, limit)) and limit > 0 and (
        abs(velocity) <= limit or velocity * (target - q) <= 0)


class MjcfParameters:
    """Mechanical parameters the generated USD omits, read from the compiled MJCF."""

    def __init__(self, geometry, record):
        m = geometry.m
        self.geometry = geometry
        self.joints = {}
        for j in record['joints']:
            if j['type'] == 0:
                continue
            q, v, jid = geometry.joint(j['name'])
            self.joints[j['name']] = dict(qadr=q, vadr=v, ref=float(m.qpos0[q]), damping=float(m.dof_damping[v]),
                                          armature=float(m.dof_armature[v]),
                                          frictionloss=float(m.dof_frictionloss[v]),
                                          range=[float(x) for x in m.jnt_range[jid]], limited=bool(m.jnt_limited[jid]))
        self.actuators = []
        import mujoco
        for a in record['actuators']:
            i = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_ACTUATOR, a['name'])
            if i < 0:
                raise KeyError('actuator ' + a['name'])
            gain, bias = m.actuator_gainprm[i], m.actuator_biasprm[i]
            gaintype, biastype = int(m.actuator_gaintype[i]), int(m.actuator_biastype[i])
            if int(m.actuator_dyntype[i]) != 0 or int(m.actuator_trntype[i]) != 0:
                raise NotImplementedError(f"{a['name']}: only joint-transmission actuators without dynamics map to PhysX")
            if gaintype == 0 and biastype == 1 and abs(gain[0] + bias[1]) < 1e-12:
                kind = 'position'      # implicit PhysX drive: kp (target - q) - kv qdot
            elif gaintype == 0 and biastype == 0:
                kind = 'motor'         # explicit joint effort gain x ctrl
            else:
                raise NotImplementedError(f"{a['name']}: only MJCF position servos and motors map to PhysX")
            self.actuators.append(dict(name=a['name'], joint=a['joint'], kind=kind, gain=float(gain[0]),
                                       kp=float(gain[0]) if kind == 'position' else 0.0,
                                       kv=float(-bias[2]) if kind == 'position' else 0.0,
                                       gear=float(m.actuator_gear[i][0]),
                                       forcerange=[float(x) for x in m.actuator_forcerange[i]],
                                       forcelimited=bool(m.actuator_forcelimited[i])))
        if m.neq or m.ntendon:
            raise NotImplementedError('MJCF equality constraints/tendons are not emitted by scripts/embodiment_usd.py; '
                                      'coupled joints would move freely in PhysX')
        free = [j for j in record['joints'] if j['type'] == 0]
        if len(free) > 1:
            raise NotImplementedError('more than one free body; the generated USD has one articulation root')
        if any(j['type'] not in (0, 3) for j in record['joints']):
            raise NotImplementedError('only free and hinge joints are mapped (prismatic drives unvalidated)')
        self.free_qadr = geometry.joint(free[0]['name'])[0] if free else None
        self.free_vadr = geometry.joint(free[0]['name'])[1] if free else None
        self.free_body = int(free[0]['body']) if free else None
        self.friction = float(np.max(m.geom_friction[:, 0]))
        self.timestep = float(m.opt.timestep)


class OwnerSemantics:
    """Sensor-time semantics shared by every physics owner in this runner.

    The native MuJoCo owner reads qpos/qvel after mj_step but body frames,
    COM velocities and contacts left by that step's forward pass (the state
    one 2 ms step earlier); accelerations span the step. Owners supply
    ``frame``/``prev_frame`` (see IsaacWorld.read), ``proper_acc`` and
    ``contacts`` and inherit the identical projection inputs.
    """

    def state(self):
        prev = self.prev_frame
        self.geometry.sync(prev['qpos'])
        contact, force, points = self.contacts
        st = sensing.State(qpos=self.frame['qpos'], qvel=self.frame['qvel'], body_pos=self.geometry.d.xpos.copy(),
                           body_rot=self.geometry.d.xmat.reshape(-1, 3, 3).copy(), com_vel=prev['com_vel'],
                           proper_acc=self.proper_acc, contact=contact, contact_force=force, contact_points=points)
        ids = self.body_ids
        self.pose_error = max(self.pose_error, float(np.abs(st.body_pos[ids] - prev['pos'][ids]).max()))
        return st

    def active_effort(self):
        return bool(self.drive_active.any())

    def _empty_contacts(self):
        return np.zeros(self.nbody, dtype=bool), np.zeros((self.nbody, 3)), {}


class IsaacWorld(OwnerSemantics):
    """One body on one PhysX stage. All USD edits go to the session layer."""

    def __init__(self, record, usd, xml, device):
        import omni.usd
        from pxr import Gf, PhysxSchema, Sdf, Tf, Usd, UsdGeom, UsdPhysics, UsdShade
        self.record = record
        self.geometry = sensing.Geometry(xml)
        self.params = MjcfParameters(self.geometry, record)
        if abs(self.params.timestep - DT) > 1e-12:
            raise ValueError('MJCF timestep differs from the 2 ms AER source clock')
        context = omni.usd.get_context()
        if not context.open_stage(str(usd)):
            raise RuntimeError(f'cannot open {usd}')
        stage = context.get_stage()
        self.stage = stage
        root = stage.GetDefaultPrim()
        if root.GetCustomDataByKey('aarnnModelDigest') != record['digest'] or \
                root.GetCustomDataByKey('aarnnIODigest') != record['io']['digest']:
            raise ValueError(f'{usd.name}: USD digest differs from the pinned model record (stale conversion)')
        stage.SetEditTarget(Usd.EditTarget(stage.GetSessionLayer()))
        self.overrides = []
        valid = Tf.MakeValidIdentifier
        # Physics scene: 2 ms, MJCF gravity, CPU PhysX unless explicitly cuda.
        scene = UsdPhysics.Scene.Define(stage, '/physicsScene')
        scene.CreateGravityDirectionAttr(Gf.Vec3f(0, 0, -1))
        scene.CreateGravityMagnitudeAttr(9.81)
        physx_scene = PhysxSchema.PhysxSceneAPI.Apply(scene.GetPrim())
        physx_scene.CreateTimeStepsPerSecondAttr(int(round(1 / DT)))
        physx_scene.CreateEnableGPUDynamicsAttr(device != 'cpu')
        physx_scene.CreateBroadphaseTypeAttr('MBP' if device == 'cpu' else 'GPU')
        physx_scene.CreateSolverTypeAttr('TGS')
        physx_scene.CreateEnableStabilizationAttr(False)
        self.overrides.append('physics scene: 500 Hz TGS, gravity -9.81 z, ' + device)
        # MJCF sliding friction. MuJoCo takes the max of a pair, or the
        # higher-priority geom's value; PhysX combines by mode precedence, so a
        # priority geom gets 'min'/'max' according to which side it must win.
        m = self.geometry.m
        friction = [float(x) for x in m.geom_friction[:, 0]]
        priority = [int(x) for x in m.geom_priority]
        materials = {}

        def material(value, mode):
            key = (round(value, 9), mode)
            if key not in materials:
                mat = UsdShade.Material.Define(stage, f'/PhysicsMaterials/mjcf_{len(materials)}')
                api = UsdPhysics.MaterialAPI.Apply(mat.GetPrim())
                api.CreateStaticFrictionAttr(value)
                api.CreateDynamicFrictionAttr(value)
                api.CreateRestitutionAttr(0.0)
                PhysxSchema.PhysxMaterialAPI.Apply(mat.GetPrim()).CreateFrictionCombineModeAttr(mode)
                materials[key] = mat
            return materials[key]
        if len(set(friction)) == 1:
            for path in ('/Robot', '/Habitat'):
                UsdShade.MaterialBindingAPI.Apply(stage.GetPrimAtPath(path)).Bind(
                    material(friction[0], 'max'), UsdShade.Tokens.weakerThanDescendants, 'physics')
            self.overrides.append(f'friction material {friction[0]} (USD has none; PhysX default 0.5)')
        else:
            body_paths = {0: '/Habitat'}
            body_paths.update({i + 1: f"/Robot/{valid(b['name'])}" for i, b in enumerate(record['bodies'])})
            low = min(friction)
            for gid, g in enumerate(record['geoms']):
                if not g['collision']:
                    continue
                prim = stage.GetPrimAtPath(f"{body_paths[g['body']]}/{valid(g['name'] or 'geom_' + str(gid))}")
                if not prim.IsValid():
                    raise KeyError('collider for geom ' + str(g['name']))
                mode = ('min' if friction[gid] <= low else 'max') if priority[gid] > 0 else 'max'
                UsdShade.MaterialBindingAPI.Apply(prim).Bind(material(friction[gid], mode),
                                                            UsdShade.Tokens.weakerThanDescendants, 'physics')
            self.overrides.append('per-geom MJCF friction materials ' +
                                  ', '.join(f'{k[0]}:{k[1]}' for k in materials) + ' (priority geoms win)')
        # MJCF contype/conaffinity decide self-collision (MuJoCo, like PhysX,
        # never collides a body with its jointed parent). Enable PhysX
        # self-collision only if some cross-body robot pair may touch, and
        # filter body pairs whose masks never allow it.
        geoms = [(int(m.geom_bodyid[g]), int(m.geom_contype[g]), int(m.geom_conaffinity[g]))
                 for g, rec in enumerate(record['geoms']) if rec['collision'] and m.geom_bodyid[g] > 0]
        bodies = sorted({b for b, _, _ in geoms})
        allowed = {(a, b): False for i, a in enumerate(bodies) for b in bodies[i + 1:]
                   if m.body_parentid[a] != b and m.body_parentid[b] != a}
        for b1, t1, a1 in geoms:
            for b2, t2, a2 in geoms:
                if b1 < b2 and (b1, b2) in allowed and (t1 & a2 or t2 & a1):
                    allowed[(b1, b2)] = True
        self_collide = any(allowed.values())
        PhysxSchema.PhysxArticulationAPI.Apply(root).CreateEnabledSelfCollisionsAttr(self_collide)
        if self_collide:
            def body_path(b):
                return f"/Robot/{valid(record['bodies'][b - 1]['name'])}"
            for (b1, b2), ok in allowed.items():
                if not ok:
                    UsdPhysics.FilteredPairsAPI.Apply(stage.GetPrimAtPath(body_path(b1))).CreateFilteredPairsRel() \
                        .AddTarget(body_path(b2))
            self.overrides.append('articulation self-collision enabled per MJCF contype/conaffinity '
                                  f'({sum(allowed.values())} body pairs)')
        else:
            self.overrides.append('articulation self-collision disabled (MJCF contype/conaffinity)')
        # Body names and MuJoCo ids.
        self.body_names = [b['name'] for b in record['bodies']]
        self.link_paths = {n: f'/Robot/{valid(n)}' for n in self.body_names}
        for name, path in self.link_paths.items():
            prim = stage.GetPrimAtPath(path)
            if prim.HasAPI(UsdPhysics.RigidBodyAPI):
                rb = PhysxSchema.PhysxRigidBodyAPI.Apply(prim)
                rb.CreateAngularDampingAttr(0.0)
                rb.CreateLinearDampingAttr(0.0)
                rb.CreateSleepThresholdAttr(0.0)
                rb.CreateStabilizationThresholdAttr(0.0)
                report = PhysxSchema.PhysxContactReportAPI.Apply(prim)
                report.CreateThresholdAttr(0.0)
        self.overrides.append('PhysX link damping/sleep disabled (MuJoCo has none); contact reports on links')
        # Joint limits are authored in MJCF qpos coordinates; the PhysX joint
        # coordinate is zero at the authored (qpos0 = ref) pose.
        self.dof_paths = {}
        for name, j in self.params.joints.items():
            path = f"/Robot/{valid('joint_' + name)}"
            prim = stage.GetPrimAtPath(path)
            if not prim.IsValid():
                raise KeyError(path)
            self.dof_paths[name] = path
            joint = UsdPhysics.RevoluteJoint(prim)
            if j['limited'] and joint:
                lo, hi = (math.degrees(x - j['ref']) for x in j['range'])
                joint.GetLowerLimitAttr().Set(lo)
                joint.GetUpperLimitAttr().Set(hi)
            PhysxSchema.PhysxJointAPI.Apply(prim).CreateArmatureAttr(j['armature'])
        if any(abs(j['ref']) > 0 for j in self.params.joints.values()):
            self.overrides.append('joint limits shifted by MJCF ref (generator emits qpos-coordinate limits)')
        self.overrides.append('joint armature from MJCF; passive joint damping folded into the drive')

    # ---------------------------------------------------------------------------------------
    def start(self):
        import omni.physics.core as physics
        import omni.physics.tensors as tensors
        from omni.physx import get_physx_simulation_interface
        from pxr import UsdUtils
        # Kit 110 / Isaac Sim 6.x order (SimulationManager.initialize_physics),
        # without its warm-up steps: the first simulate() is source step 0 -> 1.
        stage_update = physics.get_physics_stage_update_interface()
        stage_update.force_load_physics_from_usd()
        stage_update.start_simulation()
        self.sim = get_physx_simulation_interface()
        stage_id = UsdUtils.StageCache.Get().GetId(self.stage).ToLongInt()
        self.view = tensors.create_simulation_view('numpy', stage_id=stage_id)
        self.view.set_subspace_roots('/')
        self.art = self.view.create_articulation_view('/Robot')
        if self.art.count != 1:
            raise RuntimeError('articulation view did not resolve /Robot')
        meta = self.art.shared_metatype
        links = list(meta.link_names)
        dofs = list(meta.dof_names)
        from pxr import Tf
        # Record bodies -> MuJoCo body ids (arrays below are in MuJoCo order).
        self.body_ids = np.array([self.geometry.body(n) for n in self.body_names])
        self.link_index = np.array([links.index(Tf.MakeValidIdentifier(n)) for n in self.body_names])
        self.dof_index = {name: dofs.index(path.rsplit('/', 1)[1]) for name, path in self.dof_paths.items()}
        self.ndof = len(dofs)
        self.nlinks = len(links)
        m = self.geometry.m
        self.nbody = m.nbody
        self.mass = np.array(m.body_mass, dtype=float)
        self.com_local = np.array(m.body_ipos, dtype=float)
        # Drive parameters (tensor API is SI: N m/rad, N m s/rad).
        self.act = []
        for a in self.params.actuators:
            j = self.params.joints[a['joint']]
            self.act.append(dict(a, dof=self.dof_index[a['joint']], ref=j['ref'], passive=j['damping'],
                                 qadr=j['qadr'], vadr=j['vadr'],
                                 limit=next(x for x in self.record['actuators'] if x['name'] == a['name']).get(
                                     'velocity_limit')))
        self.has_motor = any(a['kind'] == 'motor' for a in self.act)
        self.passive = np.zeros((1, self.ndof))
        for name, j in self.params.joints.items():
            self.passive[0, self.dof_index[name]] = j['damping']
        self.all = np.arange(1, dtype=np.int32)
        arm = np.zeros((1, self.ndof))
        for name, j in self.params.joints.items():
            arm[0, self.dof_index[name]] = j['armature']
        if hasattr(self.art, 'set_dof_armatures'):
            self.art.set_dof_armatures(arm, self.all)
        self.disarm_drives()
        self.time = 0.0
        self.contacts = self._empty_contacts()
        self.frame = self.read()
        self.prev_frame = self.frame
        self.proper_acc = np.zeros((self.nbody, 3))
        self.pose_error = 0.0
        return self

    def disarm_drives(self):
        zeros = np.zeros((1, self.ndof))
        self.art.set_dof_stiffnesses(zeros, self.all)
        self.art.set_dof_dampings(self.passive, self.all)
        self.art.set_dof_max_forces(np.full((1, self.ndof), 1e9), self.all)
        self.art.set_dof_position_targets(zeros, self.all)
        self.art.set_dof_velocity_targets(zeros, self.all)
        if getattr(self, 'has_motor', False):
            self.art.set_dof_actuation_forces(zeros, self.all)
        self.drive_active = np.zeros(len(self.act), dtype=bool)

    def read(self):
        """Owner state in MuJoCo layout (qpos/qvel) plus link frames/COM velocities."""
        tf = self.art.get_link_transforms().reshape(-1, 7)[self.link_index]
        vel = self.art.get_link_velocities().reshape(-1, 6)[self.link_index]
        q = self.art.get_dof_positions().reshape(-1)
        qd = self.art.get_dof_velocities().reshape(-1)
        nq, nv = self.geometry.m.nq, self.geometry.m.nv
        qpos, qvel = np.zeros(nq), np.zeros(nv)
        for name, j in self.params.joints.items():
            qpos[j['qadr']] = q[self.dof_index[name]] + j['ref']
            qvel[j['vadr']] = qd[self.dof_index[name]]
        pos = np.zeros((self.nbody, 3))
        rot = np.tile(np.eye(3), (self.nbody, 1, 1))
        com_vel = np.zeros((self.nbody, 6))
        for k, b in enumerate(self.body_ids):
            pos[b] = tf[k, :3]
            x, y, z, w = tf[k, 3:]
            rot[b] = quat_wxyz_to_matrix((w, x, y, z))
            # PhysX link velocity rows are [linear at the COM, angular].
            com_vel[b, :3] = vel[k, 3:]
            com_vel[b, 3:] = vel[k, :3]
        if self.params.free_qadr is not None:
            b = self.params.free_body
            k = int(np.flatnonzero(self.body_ids == b)[0])
            x, y, z, w = tf[k, 3:]
            qpos[self.params.free_qadr:self.params.free_qadr + 7] = [*tf[k, :3], w, x, y, z]
            qvel[self.params.free_vadr:self.params.free_vadr + 3] = com_vel[b, 3:] - np.cross(
                com_vel[b, :3], rot[b] @ self.com_local[b])
            qvel[self.params.free_vadr + 3:self.params.free_vadr + 6] = rot[b].T @ com_vel[b, :3]
        return dict(qpos=qpos, qvel=qvel, pos=pos, rot=rot, com_vel=com_vel, q=q, qd=qd)

    def apply_controls(self, controls, mask, disarmed, stiffness=None):
        """MuJoCo position servo -> PhysX drive, gain gated each step as in world.rs.

        ``stiffness`` optionally scales kp and kv per actuator (zebrafish trunk).
        """
        if disarmed:
            if self.drive_active.any():
                self.disarm_drives()
            return
        stiff = np.zeros((1, self.ndof))
        damp = self.passive.copy()
        maxf = np.full((1, self.ndof), 1e9)
        target = np.zeros((1, self.ndof))
        effort = np.zeros((1, self.ndof))
        q, qd = self.frame['qpos'], self.frame['qvel']
        for i, a in enumerate(self.act):
            goal = float(controls[i])
            limit = a['limit']
            active = bool(mask[i]) and (limit is None or speed_allows(goal, float(q[a['qadr']]), float(qd[a['vadr']]),
                                                                       float(limit)))
            self.drive_active[i] = active
            d = a['dof']
            if a['kind'] == 'motor':
                if active:
                    force = a['gain'] * goal
                    if a['forcelimited']:
                        force = min(max(force, a['forcerange'][0]), a['forcerange'][1])
                    effort[0, d] = force * a['gear']
                continue
            target[0, d] = goal - a['ref']
            if active:
                scale = 1.0 if stiffness is None else float(stiffness[i])
                stiff[0, d] = a['kp'] * a['gear'] ** 2 * scale
                damp[0, d] += a['kv'] * a['gear'] ** 2 * scale
                if a['forcelimited']:
                    maxf[0, d] = max(abs(a['forcerange'][0]), abs(a['forcerange'][1])) * abs(a['gear'])
        self.art.set_dof_stiffnesses(stiff, self.all)
        self.art.set_dof_dampings(damp, self.all)
        self.art.set_dof_max_forces(maxf, self.all)
        self.art.set_dof_position_targets(target, self.all)
        if self.has_motor:
            self.art.set_dof_actuation_forces(effort, self.all)

    def apply_external(self, forces):
        """Sum (body, force, point, torque) into one wrench per link about its COM."""
        if not forces:
            return
        f = np.zeros((self.nlinks, 3))
        t = np.zeros((self.nlinks, 3))
        p = np.zeros((self.nlinks, 3))
        row = {int(b): int(i) for b, i in zip(self.body_ids, self.link_index)}
        prev = self.prev_frame
        for body, force, point, torque in forces:
            k = row[body]
            centre = prev['pos'][body] + prev['rot'][body] @ self.com_local[body]
            f[k] += force
            t[k] += np.cross(np.asarray(point) - centre, force)
            if torque is not None:
                t[k] += torque
        for body, k in row.items():
            p[k] = self.frame['pos'][body] + self.frame['rot'][body] @ self.com_local[body]
        self.art.apply_forces_and_torques_at_position(f, t, p, self.all, True)

    def step(self):
        self.prev_frame = self.frame
        self.sim.simulate(DT, self.time)
        self.sim.fetch_results()
        self.time += DT
        self.frame = self.read()
        dv = self.frame['com_vel'][:, 3:] - self.prev_frame['com_vel'][:, 3:]
        self.proper_acc = dv / DT - GRAVITY
        self.proper_acc[0] = 0.0
        self.contacts = self._contact_report()

    def _contact_report(self):
        from pxr import PhysicsSchemaTools
        contact, force, points = self._empty_contacts()
        headers, data = self.sim.get_contact_report()
        lookup = getattr(self, '_paths', None)
        if lookup is None:
            lookup = self._paths = {self.link_paths[n]: int(b) for n, b in zip(self.body_names, self.body_ids)}
        lost = getattr(self, '_lost', None)
        if lost is None:
            from omni.physx.bindings._physx import ContactEventType
            lost = self._lost = ContactEventType.CONTACT_LOST
        for h in headers:
            if h.type == lost:
                continue
            a0 = str(PhysicsSchemaTools.intToSdfPath(h.actor0))
            a1 = str(PhysicsSchemaTools.intToSdfPath(h.actor1))
            c0 = str(PhysicsSchemaTools.intToSdfPath(h.collider0))
            c1 = str(PhysicsSchemaTools.intToSdfPath(h.collider1))
            rows = data[h.contact_data_offset:h.contact_data_offset + h.num_contact_data]
            touching = [r for r in rows if r.separation <= 0.0 or
                        (r.impulse[0] ** 2 + r.impulse[1] ** 2 + r.impulse[2] ** 2) > 0.0]
            if not touching:
                continue
            impulse = np.sum([[r.impulse[0], r.impulse[1], r.impulse[2]] for r in touching], axis=0)
            for actor, other, sign in ((a0, c1, 1.0), (a1, c0, -1.0)):
                body = lookup.get(actor)
                if body is None:
                    continue
                contact[body] = True
                force[body] += sign * impulse / DT
                points.setdefault(body, []).extend(
                    (np.array([r.position[0], r.position[1], r.position[2]]), other.rsplit('/', 1)[-1]) for r in touching)
        return contact, force, points


class Body:
    """Port of main.rs Session + world.rs World::advance around the Rust ABI."""

    def __init__(self, instance, profile, record, assets, binding, args):
        self.instance, self.profile, self.record = instance, profile, record
        self.xml = assets / profile['model']
        self.usd = (args.usd_dir / self.xml.name).with_suffix('.usda') if args.usd_dir else self.xml.with_suffix('.usda')
        if args.physics_owner == 'mujoco-twin':
            from mujoco_twin import MujocoTwinWorld
            self.world = MujocoTwinWorld(record, self.xml, MjcfParameters)
        else:
            self.world = IsaacWorld(record, self.usd, self.xml, args.device)
        self.senses = sensing.Senses(record, self.world.geometry)
        self.session = ffi.Session(self.xml, binding, control=args.control)
        self.brain_id = binding['brain_id'] if binding else None
        self.npc_enabled = args.control == 'npc' or (args.control == 'auto')
        self.n_s = len(record['io']['sensors'])
        self.n_a = len(record['actuators'])
        self.step_index = 0
        self.submitted = 0
        self.initial_capture = False
        self.last_sensors = []
        self.active_steps = 0
        self.start_time = time.monotonic()
        self.trace = None
        self.args = args
        self.scene = record.get('scene') or {}
        self.tail_joints = np.array([a['joint'].startswith('tail_joint_') for a in record['actuators']]) \
            if record['profile_id'] == 'zebrafish' else None

    def open_trace(self, directory, every):
        directory.mkdir(parents=True, exist_ok=True)
        path = directory / f'{self.instance}.jsonl'
        handle = path.open('x')
        segments = [[n, int(b)] for n, b in zip(self.world.body_names, self.world.body_ids) if n.startswith('segment_')]
        header = {
            'schema': 'aarnn-mujoco-trace-v2', 'physics_owner': self.args.physics_owner,
            'muscle_activation_layout': 'row 01 (head) .. 24 (tail) x [MDL, MDR, MVL, MVR]',
            'instance_id': self.instance, 'profile': self.record['profile_id'], 'brain_id': self.brain_id,
            'model_digest': self.record['digest'], 'io_digest': self.record['io']['digest'],
            'body_count': self.world.nbody, 'segment_body_ids': segments,
            'actuated_joints': [a['joint'] for a in self.record['actuators']],
            'sensor_names': [s['name'] for s in self.record['io']['sensors']],
            'output_names': self.record['io']['output_names'], 'trace_every_steps': every,
            'scope': 'read-only physics/IO observation; no production commit or effect authority'}
        handle.write(json.dumps(header) + '\n')
        self.trace = dict(path=path, handle=handle, records=0, skipped=0, every=every)

    def status(self):
        return self.session.status()

    def ready(self):
        """main.rs Session::ready: arm gate, client readiness, initial capture."""
        if self.args.arm_file and not self.args.arm_file.is_file():
            return False
        if not self.session.bound:
            return True
        transport = self.status()['transport'] or {}
        if transport.get('exchanges', 0) > 0:
            return True
        if transport.get('fault'):
            raise RuntimeError('reference transport failed: ' + transport['fault'])
        if not transport.get('ready'):
            return False
        if not self.initial_capture:
            values = self.senses.measure(self.world.state(), self.step_index)
            if self.session.submit(self.step_index, values):
                self.submitted += 1
            self.initial_capture = True
        return False

    def await_reply(self, stop):
        if not self.session.bound:
            return True
        data_ms = int(os.environ.get('NM_AER_REFERENCE_DATA_TIMEOUT_MS', '0') or 0)
        data_ms = min(max(data_ms, 1000), 600000) if data_ms else 0
        deadline = time.monotonic() + max(data_ms + 5000, 30000) / 1000
        while True:
            transport = self.status()['transport'] or {}
            if transport.get('fault'):
                raise RuntimeError('reference transport failed: ' + transport['fault'])
            if transport.get('exchanges', 0) >= self.submitted:
                return True
            if stop():
                return False
            if time.monotonic() >= deadline:
                raise RuntimeError('lock-step reply deadline exceeded')
            time.sleep(0.00025)

    def external_forces(self):
        """world.rs passive environment forces, from the last forward pass's frames.

        Returns (mujoco_body, world_force, world_point, world_torque_or_None).
        C. elegans resistive-force substrate + plate tap; zebrafish
        resistive-ellipsoid-v1 water and Drosophila wing panels through the
        shared Rust force code (nm_reference_hydro / nm_panel_forces).
        """
        forces = []
        frame = self.world.prev_frame
        w = self.world
        profile = self.record['profile_id']

        def com(b):
            return frame['pos'][b] + frame['rot'][b] @ w.com_local[b]
        if profile == 'celegans':
            rft = self.scene.get('resistive_force')
            taps = self.scene.get('taps')
            tap = bool(taps) and taps['period_s'] > 0 and w.time >= taps['first_s'] and \
                (w.time - taps['first_s']) % taps['period_s'] < taps['duration_s']
            for b in map(int, w.body_ids):
                r = frame['rot'][b]
                v = frame['com_vel'][b, 3:]
                force = np.zeros(3)
                if rft:
                    t = np.array([r[0][0], r[1][0]])
                    n = math.hypot(*t)
                    if n > 1e-9:
                        t /= n
                        vt = v[0] * t[0] + v[1] * t[1]
                        vn = v[:2] - vt * t
                        force[:2] = -rft['tangential_N_s_per_m'] * vt * t - rft['normal_N_s_per_m'] * vn
                if tap:
                    force[2] += w.mass[b] * taps['acceleration_mps2']
                if force.any():
                    forces.append((b, force, com(b), None))
        hydro = self.scene.get('hydrodynamics')
        if hydro:
            rows, owners = [], []
            offset = np.asarray(hydro.get('buoyancy_offset_m', [0.0, 0.0, 0.0]), dtype=float)
            for b in range(1, w.nbody):
                centre = com(b) + frame['rot'][b] @ offset
                rows.append([0.0, w.mass[b], centre[2]] + [0.0] * 18)
                owners.append((b, centre))
            for e in hydro['bodies']:
                b = self.world.geometry.body(e['body'])
                r = frame['rot'][b]
                centre = frame['pos'][b] + r @ np.asarray(e['offset_m'], dtype=float)
                omega = frame['com_vel'][b, :3]
                linear = frame['com_vel'][b, 3:] + np.cross(omega, centre - com(b))
                rows.append([1.0, w.mass[b], centre[2], *e['half_size_m'], *r.reshape(-1), *linear, *omega])
                owners.append((b, centre))
            for (b, centre), wrench in zip(owners, self.session.hydro(rows)):
                forces.append((b, wrench[:3], centre, wrench[3:]))
        elif profile == 'zebrafish':
            raise NotImplementedError('zebrafish without scene.hydrodynamics (legacy water_force) is not ported')
        panels = []
        if profile.startswith('drosophila_') and self.record.get('flight'):
            # Fly mechanics v3: the resonant-flight-v1 motor (aarnn_embodiment::
            # flight) and the blades' scene.aerodynamics panels are not ported
            # to Isaac (no FFI or joint-torque path yet). The old v1 hinge
            # panels no longer describe this body, so none are applied: the
            # wings are passive and the fly can walk but not fly here.
            if not getattr(self, '_flight_unported_noted', False):
                print('isaac: drosophila resonant-flight-v1 is not ported; wings are passive', file=sys.stderr)
                self._flight_unported_noted = True
        elif profile.startswith('drosophila_'):
            panels = [(side + '_wing', [0.0, 0.0, 0.0], 0.0025) for side in ('left', 'right')]
        elif profile == 'crow_corvus_corone':
            raise NotImplementedError('crow sensory projection/energy model is not ported to Isaac')
        if panels:
            rows, points = [], []
            for name, local, area in panels:
                b = self.world.geometry.body(name)
                r = frame['rot'][b]
                point = frame['pos'][b] + r @ np.asarray(local, dtype=float)
                omega = frame['com_vel'][b, :3]
                relative = frame['com_vel'][b, 3:] + np.cross(omega, point - com(b))
                rows.append([*relative, *r.reshape(-1), area])
                points.append((b, point))
            for (b, point), force in zip(points, ffi.panel_forces(rows)):
                forces.append((b, force, point, None))
        return forces

    def advance(self):
        """world.rs World::advance."""
        transport = (self.status()['transport'] or {}) if self.session.bound else {}
        neural_ready = bool(transport.get('ready')) and not transport.get('fault')
        observation = self.last_sensors if len(self.last_sensors) == self.n_s else [0.0] * self.n_s
        if self.npc_enabled and not neural_ready:
            observation = self.senses.measure(self.world.state(), self.step_index)
        code, controls = self.session.apply(self.step_index, observation, self.n_a + 2)
        mask = self.session.motor_mask(self.n_a)
        stiffness = None
        if self.tail_joints is not None:
            process = self.session.status().get('process_rates') or [0.0, 0.0]
            stiffness = np.where(self.tail_joints, 1.0 + 4.0 * float(process[0]), 1.0)
        self.world.apply_controls(controls, mask, disarmed=(code == 0), stiffness=stiffness)
        self.world.apply_external(self.external_forces())
        self.world.step()
        if code == 1 and self.world.active_effort():
            self.active_steps += 1
        self.step_index += 1
        if not np.all(np.isfinite(self.world.frame['qpos'])):
            raise RuntimeError('nonfinite physical state')
        if neural_ready or (self.session.bound and (self.status()['transport'] or {}).get('ready')):
            values = self.senses.measure(self.world.state(), self.step_index)
            self.last_sensors = values
            if self.session.submit(self.step_index, values):
                self.submitted += 1
        if self.step_index % 2500 == 0:
            print(f"Isaac progress {self.instance}: {self.world.time:.2f}s, "
                  f"{transport.get('exchanges', 0)} neural exchanges, {self.active_steps} active steps",
                  file=sys.stderr, flush=True)
        if self.trace and self.step_index % self.trace['every'] == 0:
            if self.trace['records'] < TRACE_LIMIT:
                self.write_trace()
            else:
                self.trace['skipped'] += 1

    def write_trace(self):
        status = self.status()
        st = self.world.state()
        bodies = st.body_pos[1:].tolist()
        transport = status.get('transport') or {}
        record = {
            'step': self.step_index, 'reset_epoch': status['reset_epoch'], 'time_s': self.world.time,
            'body_xyz': bodies,
            'joint_q': [float(st.qpos[a['qadr']]) for a in self.world.act],
            'motor_rates': status.get('motor_rates'), 'disarmed': status['disarmed'],
            'npc_control': status['controller_mode'] == 'programmed-NPC',
            'sensors': self.last_sensors,
            'sensor_source': 'submitted' if self.session.bound else ('unsampled' if not self.last_sensors else
                                                                     'passive-observation'),
            'applied_spikes': status.get('applied_spikes'), 'applied': status['applied'],
            'expired': status['expired'], 'exchanges': transport.get('exchanges') if self.session.bound else None,
            'output_code': status.get('output_code'), 'qpos': st.qpos.tolist(),
            'muscle_activation': status.get('muscle_activation')}
        self.trace['handle'].write(json.dumps(record) + '\n')
        self.trace['records'] += 1

    def finish(self, error=None):
        trace = None
        if self.trace:
            flush_error = None
            try:
                self.trace['handle'].close()
            except OSError as e:
                flush_error = str(e)
            trace = dict(path=str(self.trace['path']), records=self.trace['records'],
                         skipped_over_limit=self.trace['skipped'], flush_error=flush_error)
        status = self.status()
        transport = status.get('transport')
        self.session.close()
        failed = error is not None or bool(transport and transport.get('fault')) or bool(trace and trace['flush_error'])
        m = self.world.geometry.m
        return {
            'status': 'fail' if failed else 'pass', 'error': error,
            'scope': 'native physics and optional reference packets; no learned task/production commitment claim',
            'controller_mode': status['controller_mode'], 'npc_policy': status['npc_policy'],
            'npc_steps': status['npc_steps'], 'physics': physics_label(),
            'instance_id': self.instance, 'profile': self.record['profile_id'], 'brain_id': self.brain_id,
            'model_digest': self.record['digest'], 'io_digest': self.record['io']['digest'],
            'usd': str(self.usd), 'nq': m.nq, 'nv': m.nv, 'nu': m.nu, 'steps': self.step_index,
            'reset_epoch': status['reset_epoch'], 'time_s': self.world.time, 'qpos': self.world.frame['qpos'].tolist(),
            'energy_J': None, 'elapsed_s': time.monotonic() - self.start_time, 'applied': status['applied'],
            'applied_spikes': status.get('applied_spikes'), 'active_steps': self.active_steps,
            'expired': status['expired'], 'transport': transport, 'trace': trace,
            'stage_overrides': self.world.overrides,
            'conversion_check': dict(max_link_vs_mjcf_kinematics_error_m=self.world.pose_error)}


_PHYSICS = None


def physics_label():
    return _PHYSICS or 'Isaac Sim / PhysX'


def main(argv=None):
    global _PHYSICS
    args = parse_args(argv)
    entries = resolve_entries(args.robots, args.assets)
    bindings = json.loads(args.reference_bindings.read_text()) if args.reference_bindings else {}
    if args.reference_binding:
        if bindings:
            raise SystemExit('single binding requires one body')
        bindings[entries[0][0]] = json.loads(args.reference_binding.read_text())
    if set(bindings) - {i for i, _ in entries}:
        raise SystemExit('binding for absent body')
    if args.physics_owner == 'isaacsim' and \
            os.environ.get('OMNI_KIT_ACCEPT_EULA', '').strip().lower() not in ('yes', 'y', '1'):
        raise SystemExit('Isaac Sim requires accepting the NVIDIA Omniverse EULA: export OMNI_KIT_ACCEPT_EULA=YES')
    if args.physics_owner == 'isaacsim':
        from isaacsim import SimulationApp
        app = SimulationApp({'headless': True,
                             'extra_args': [f'--/persistent/physics/numThreads={max(args.physx_threads, 0)}']})
        import carb
        settings = carb.settings.get_settings()
        # State is read through omni.physics.tensors; skip per-step USD writes.
        for key in ('/physics/updateToUsd', '/physics/updateVelocitiesToUsd',
                    '/physics/updateParticlesToUsd', '/physics/updateResidualsToUsd'):
            settings.set_bool(key, False)
    else:
        app = None
    stop = {'flag': False}

    def halt(*_):
        stop['flag'] = True
    signal.signal(signal.SIGINT, halt)
    signal.signal(signal.SIGTERM, halt)
    reports, code = [], 0
    try:
        if app is not None:
            import omni.kit.app
            from importlib.metadata import version
            _PHYSICS = f"Isaac Sim {version('isaacsim')} (Kit {omni.kit.app.get_app().get_build_version()}) / PhysX"
        else:
            import mujoco
            _PHYSICS = f'MuJoCo {mujoco.__version__} python twin (diagnostic, not Isaac)'
        instance, profile = entries[0]
        record = json.loads((args.assets / profile['model']).with_suffix('.json').read_text())
        binding = bindings.get(instance)
        if args.control in ('npc', 'silent') and binding:
            raise SystemExit('NPC/silent require an unbound body')
        if args.control == 'aarnn' and not binding:
            raise SystemExit('aarnn requires an explicit appropriate binding')
        body = Body(instance, profile, record, args.assets, binding, args)
        body.world.start()
        if args.trace:
            body.open_trace(args.trace, args.trace_every)
        print(json.dumps(dict(status='ready', pid=os.getpid(), instance_id=instance, profile=record['profile_id'],
                              brain_id=body.brain_id, physics=physics_label(),
                              mode='AER1-reference' if binding else ('programmed-NPC' if body.npc_enabled
                                                                     else 'silent-mechanics'))),
              file=sys.stderr, flush=True)
        error = None
        try:
            iteration = 0
            deadline = time.monotonic()
            while iteration < args.steps and not stop['flag']:
                if not body.ready():
                    time.sleep(0.01)
                    deadline = time.monotonic()
                    continue
                if args.lockstep and not body.await_reply(lambda: stop['flag']):
                    break
                body.advance()
                iteration += 1
                if args.realtime:
                    deadline += DT
                    time.sleep(max(0.0, deadline - time.monotonic()))
        except Exception as e:  # report, then fail the run
            error = f'{type(e).__name__}: {e}'
            print('Isaac runner error: ' + error, file=sys.stderr, flush=True)
        reports.append(body.finish(error))
    except BaseException as e:   # start-up failure: report it before Kit shuts down
        import traceback
        traceback.print_exc()
        sys.stderr.flush()
        reports.append(dict(status='fail', error=f'{type(e).__name__}: {e}', instance_id=entries[0][0], steps=0,
                            time_s=0.0, active_steps=0, controller_mode='not-started', transport=None))
    finally:
        result = dict(schema_version=1, independent_worlds=True, worlds=reports)
        if args.output:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(result, indent=2) + '\n')
        for row in reports:
            t = row.get('transport') or {}
            print(f"Isaac {row['instance_id']}: {row['steps']} steps, {row['time_s']:.3f}s simulated, "
                  f"{t.get('exchanges', 0)} neural exchanges, {t.get('output_spikes', 0)} output spikes, "
                  f"{row['active_steps']} active steps ({row['controller_mode']})", flush=True)
        if args.json:
            print(json.dumps(result))
        if args.output:
            print(f'Evidence: {args.output}', flush=True)
        code = 1 if not reports or any(r['status'] != 'pass' for r in reports) else 0
        sys.stdout.flush()
        sys.stderr.flush()
        if app is not None:
            # Kit shutdown can stall after a headless physics session; evidence
            # is already written, so bound it and keep the exit status.
            import threading
            threading.Timer(60.0, lambda: os._exit(code)).start()
            app.close(exit_code=code)
    return code


if __name__ == '__main__':
    raise SystemExit(main())
