#!/usr/bin/env python3
"""Physics falsification and protocol tests, not learned-policy acceptance."""
import copy
import json
import math
from pathlib import Path
import struct
import sys
import unittest
ROOT=Path(__file__).resolve().parents[2]
sys.path[:0]=[str(ROOT),str(ROOT/'scripts')]
import numpy as np
import mujoco
from embodiment import FIRST, products
from robot_aer import Binding, RateEncoder, decode
from sim.mujoco.runtime import World
from sim.mujoco.aerodynamics import panel_force


class Contract(unittest.TestCase):
    def test_generated_freshness(self):
        for path,value in products().items():self.assertEqual(path.read_text(),value,str(path))

    def test_native_import_and_passive_coordinates(self):
        for profile,drives in [('galbot_g1_golf',27),('ugot_mecanum',4),('crow_corvus_corone',31)]:
            w=World(profile);self.assertEqual(w.model.nu,drives)
            self.assertTrue(np.all(w.model.body_mass[1:]>=0))
            self.assertGreater(w.model.nv,w.model.nu)
            self.assertEqual(len(w.sensors()),len(w.record['io']['sensors']))
        self.assertAlmostEqual(sum(World('crow_corvus_corone').model.body_mass),.510)
        self.assertEqual(World('ugot_mecanum').model.njnt-1-4,36)

    def test_free_fall_and_mass_impulse(self):
        for mass_factor in (1,2):
            w=World('crow_corvus_corone')
            w.data.qpos[2]=3;w.model.body_mass[1:]*=mass_factor;w.model.body_inertia[1:]*=mass_factor
            mujoco.mj_setConst(w.model,w.data);w.data.qpos[2]=3;mujoco.mj_forward(w.model,w.data)
            start=w.data.qpos[2]
            for _ in range(20):w.step()
            self.assertLess(w.data.qpos[2],start-.004)
            self.assertTrue(np.all(w.data.ctrl==0))
        # Known force gives inverse-mass acceleration with gravity disabled.
        accelerations=[]
        for factor in (1,2):
            w=World('crow_corvus_corone');w.model.opt.gravity[:]=0
            w.model.body_mass[1:]*=factor;w.model.body_inertia[1:]*=factor;w.model.dof_armature[:]*=factor
            mujoco.mj_setConst(w.model,w.data);w.data.qpos[2]=3
            w.data.xfrc_applied[1,0]=1
            mujoco.mj_forward(w.model,w.data);accelerations.append(w.data.qacc[0])
        self.assertAlmostEqual(accelerations[0]/accelerations[1],2,places=5)

    def test_aero_has_no_free_power_or_spike_lift(self):
        for velocity in ([8,0,0],[8,0,-2],[0,0,-3],[-3,2,1]):
            force=panel_force(velocity,np.eye(3),.045)
            self.assertLessEqual(np.dot(force,velocity),1e-10)
        self.assertTrue(np.all(panel_force([0,0,0],np.eye(3),.045)==0))
        self.assertGreater(panel_force([8,0,-2],np.eye(3),.045)[2],0)
        self.assertLess(panel_force([8,0,2],np.eye(3),.045)[2],0)

    def test_reset_isolation_and_snapshot(self):
        a,b=World('ugot_mecanum','a','brain-a'),World('ugot_mecanum','b','brain-b')
        a.data.qvel[0]=1;a.step();self.assertEqual(b.step_index,0)
        before=b.data.qpos.copy();saved=a.snapshot();a.reset()
        np.testing.assert_equal(before,b.data.qpos)
        self.assertEqual(a.reset_epoch,1)
        a.restore(saved);self.assertTrue(a.disarmed);np.testing.assert_allclose(a.data.qpos,saved['physics'][1:1+a.model.nq])
        with self.assertRaises(ValueError):b.restore(saved)

    def test_output_epoch_identity_duplicate_and_silence(self):
        w=World('ugot_mecanum','one','brain-one')
        reply=dict(brain_id='brain-one',binding_generation=1,reset_epoch=0,step=0,spikes=[1,0]*4)
        w.accept_reference(reply);w.step();self.assertTrue(np.any(w.data.ctrl!=0))
        with self.assertRaises(ValueError):w.accept_reference(reply)
        wrong=dict(reply,step=1,brain_id='brain-two')
        with self.assertRaises(ValueError):w.accept_reference(wrong)
        w.reset()
        with self.assertRaises(ValueError):w.accept_reference(reply)
        for _ in range(60):w.step()
        self.assertTrue(w.disarmed);self.assertTrue(np.all(w.model.actuator_gainprm==0))

    def test_aer_goldens_empty_and_malformed(self):
        encoder=RateEncoder(2)
        self.assertEqual(encoder.encode([0,0],123,4096),b'AER1'+struct.pack('<Q',123))
        payload=encoder.encode([1,0],124,4096)
        self.assertEqual(payload.hex(),'414552317c0000000000000000802001')
        self.assertEqual(decode(payload,4096,2),(124,[1,0]))
        for invalid in (b'',b'AER1',payload[:-1],b'AER1'+struct.pack('<Q',0)+b'\x80'*10):
            with self.assertRaises(ValueError):decode(invalid,4096,2)
        with self.assertRaises(ValueError):decode(payload,16384,2)

if __name__=='__main__':unittest.main()
