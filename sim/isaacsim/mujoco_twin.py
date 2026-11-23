# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Diagnostic MuJoCo-python owner with the Isaac runner's exact body/ABI path.

``run_isaacsim.py --physics-owner mujoco-twin`` swaps PhysX for MuJoCo while
keeping the Python sensory port, ABI calls, drive gating and trace writer.
Agreement with the native Rust MuJoCo owner validates that path; remaining
Isaac-vs-MuJoCo differences are then physics (contact, solver, conversion).
Actuation follows adapters/mujoco/src/native.rs (gain/bias rows zeroed when
inactive). Not an Isaac result and never a replacement for the Rust owner.
"""
import numpy as np

import senses as sensing
from run_isaacsim import DT, OwnerSemantics, speed_allows


class MujocoTwinWorld(OwnerSemantics):
    def __init__(self, record, xml, parameters):
        import mujoco
        self.mj = mujoco
        self.record = record
        self.geometry = sensing.Geometry(xml)
        self.params = parameters(self.geometry, record)
        self.m = mujoco.MjModel.from_xml_path(str(xml))
        self.d = mujoco.MjData(self.m)
        # ``d`` stays exactly as the Rust owner leaves it after mj_step (frames,
        # contacts and Jacobians of the step's start); ``probe`` re-poses a copy
        # to read the new state's frames without touching the dynamics.
        self.probe = mujoco.MjData(self.m)
        self.overrides = [f'MuJoCo {mujoco.__version__} python dynamics (diagnostic twin, not Isaac)']
        self.body_names = [b['name'] for b in record['bodies']]

    def start(self):
        mj, m = self.mj, self.m
        self.gains = m.actuator_gainprm.copy()
        self.biases = m.actuator_biasprm.copy()
        self.body_ids = np.array([self.geometry.body(n) for n in self.body_names])
        self.nbody = m.nbody
        self.mass = np.array(m.body_mass, dtype=float)
        self.com_local = np.array(m.body_ipos, dtype=float)
        self.act = []
        for a, rec in zip(self.params.actuators, self.record['actuators']):
            j = self.params.joints[a['joint']]
            index = mj.mj_name2id(m, mj.mjtObj.mjOBJ_ACTUATOR, a['name'])
            self.act.append(dict(a, index=index, qadr=j['qadr'], vadr=j['vadr'], limit=rec.get('velocity_limit')))
        self.drive_active = np.zeros(len(self.act), dtype=bool)
        self.disarm()
        mj.mj_forward(m, self.d)
        self.time = 0.0
        self.contacts = self._empty_contacts()
        self.frame = self.read()
        self.prev_frame = self.frame
        self.proper_acc = np.zeros((self.nbody, 3))
        self.pose_error = 0.0
        return self

    def disarm(self):
        self.d.ctrl[:] = 0
        self.m.actuator_gainprm[:] = 0
        self.m.actuator_biasprm[:] = 0
        self.drive_active[:] = False

    def read(self):
        mj, m, d = self.mj, self.m, self.probe
        d.qpos[:] = self.d.qpos
        d.qvel[:] = self.d.qvel
        mj.mj_forward(m, d)
        com_vel = np.zeros((m.nbody, 6))
        buffer = np.zeros(6)
        for b in range(1, m.nbody):
            mj.mj_objectVelocity(m, d, mj.mjtObj.mjOBJ_BODY, b, buffer, 0)
            com_vel[b] = buffer
        return dict(qpos=d.qpos.copy(), qvel=d.qvel.copy(), pos=d.xpos.copy(),
                    rot=d.xmat.reshape(-1, 3, 3).copy(), com_vel=com_vel)

    def apply_controls(self, controls, mask, disarmed, stiffness=None):
        if disarmed:
            self.disarm()
            return
        q, qd = self.frame['qpos'], self.frame['qvel']
        for i, a in enumerate(self.act):
            goal = float(controls[i])
            limit = a['limit']
            active = bool(mask[i]) and (limit is None or speed_allows(goal, float(q[a['qadr']]), float(qd[a['vadr']]),
                                                                       float(limit)))
            self.drive_active[i] = active
            k = a['index']
            self.d.ctrl[k] = goal
            scale = 1.0 if stiffness is None else float(stiffness[i])
            self.m.actuator_gainprm[k] = self.gains[k] * scale if active else 0
            self.m.actuator_biasprm[k] = self.biases[k] * scale if active else 0

    def apply_external(self, forces):
        self.d.qfrc_applied[:] = 0
        for body, force, point, torque in forces:
            self.mj.mj_applyFT(self.m, self.d, np.asarray(force, float),
                               np.zeros(3) if torque is None else np.asarray(torque, float),
                               np.asarray(point, float), int(body), self.d.qfrc_applied)

    def step(self):
        mj, m, d = self.mj, self.m, self.d
        self.prev_frame = self.frame
        mj.mj_step(m, d)
        self.time += DT
        # Contacts of this step (detected at its start), as the Rust owner reads them.
        contact, force, points = self._empty_contacts()
        for i in range(d.ncon):
            c = d.contact[i]
            for g, other in ((c.geom1, c.geom2), (c.geom2, c.geom1)):
                if g >= 0:
                    b = int(m.geom_bodyid[g])
                    contact[b] = True
                    points.setdefault(b, []).append((c.pos.copy(), self.geometry.geom_name(other)))
        self.contacts = contact, force, points
        self.frame = self.read()
        # Rust owner: mj_rnePostConstraint + mj_objectAcceleration on the
        # post-step data (COM, world axes, gravity-referenced).
        mj.mj_rnePostConstraint(m, d)
        buffer = np.zeros(6)
        self.proper_acc = np.zeros((m.nbody, 3))
        for b in range(1, m.nbody):
            mj.mj_objectAcceleration(m, d, mj.mjtObj.mjOBJ_BODY, b, buffer, 0)
            self.proper_acc[b] = buffer[3:]
