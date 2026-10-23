#!/usr/bin/env python3
"""Conservation and joint-frame checks against native MuJoCo source models."""
import json
from pathlib import Path
import sys
import unittest
import numpy as np
import mujoco
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'scripts'))
from embodiment import NEW,LEGACY,OUT
from embodiment_graph import compile_graph,matrix

def compound(parts):
    mass=sum(m for m,_,_ in parts);com=sum((m*p for m,p,_ in parts),np.zeros(3))/mass
    inertia=sum((i+m*(np.dot(p-com,p-com)*np.eye(3)-np.outer(p-com,p-com)) for m,p,i in parts),np.zeros((3,3)))
    return mass,com,inertia
class Graph(unittest.TestCase):
    def test_fixed_clusters_preserve_global_mass_com_and_inertia(self):
        for profile in NEW+LEGACY:
            with self.subTest(profile=profile):
                graph=compile_graph(profile);native=mujoco.MjModel.from_xml_path(str(OUT/f'{profile}.xml'));data=mujoco.MjData(native);mujoco.mj_forward(native,data)
                source=[]
                for b in range(1,native.nbody):
                    r=data.ximat[b].reshape(3,3)
                    source.append((native.body_mass[b],data.xipos[b],r@np.diag(native.body_inertia[b])@r.T))
                reduced=[]
                for b in graph['bodies']:
                    r=matrix(b['world_quaternion_wxyz']);p=np.array(b['world_position'])+r@b['com_m']
                    reduced.append((b['mass_kg'],p,r@np.array(b['inertia_kgm2'])@r.T))
                a,b=compound(source),compound(reduced)
                for i in range(3):np.testing.assert_allclose(a[i],b[i],rtol=1e-10,atol=1e-12,err_msg=profile)
    def test_joint_frames_meet_at_native_pivot_with_named_axis(self):
        for profile in NEW+LEGACY:
            graph=compile_graph(profile);bodies={b['id']:b for b in graph['bodies']}
            for j in graph['joints']:
                with self.subTest(profile=profile,joint=j['name']):
                    p,c=bodies[j['parent']],bodies[j['child']]
                    pr,cr=matrix(p['world_quaternion_wxyz']),matrix(c['world_quaternion_wxyz'])
                    np.testing.assert_allclose(np.array(p['world_position'])+pr@j['parent_position'],np.array(c['world_position'])+cr@j['child_position'],atol=1e-12)
                    np.testing.assert_allclose(pr@matrix(j['parent_quaternion_wxyz']),cr@matrix(j['child_quaternion_wxyz']),atol=1e-12)
                    np.testing.assert_allclose(matrix(j['child_quaternion_wxyz'])[:,0],j['axis'],atol=1e-12)
    def test_reflected_engine_bases_preserve_work_and_moment(self):
        # Polar forces, axial torques and inertia use different transforms.
        for basis,length in [(np.diag([1,-1,1]),100.),(np.array([[0,-1,0],[0,0,1],[1,0,0]]),1.)]:
            det=np.linalg.det(basis);p=np.array([.3,-.1,.6]);f=np.array([2.,7.,-3.]);w=np.array([.4,-.7,.2]);moment=np.cross(p,f)
            np.testing.assert_allclose(np.cross(length*basis@p,length*basis@f),length**2*det*basis@moment,atol=1e-12)
            self.assertAlmostEqual(np.dot(length*basis@f,length*basis@p)/length**2,np.dot(f,p))
            self.assertAlmostEqual(np.dot(length**2*det*basis@moment,det*basis@w)/length**2,np.dot(moment,w))
            r=matrix([.5,.5,.5,.5]);mapped=basis@r@basis.T
            np.testing.assert_allclose(mapped@(basis@p),basis@(r@p),atol=1e-12)
    def test_cluster_collision_filters_preserve_source_masks(self):
        for profile in NEW+LEGACY:
            with self.subTest(profile=profile):
                graph=compile_graph(profile);native=mujoco.MjModel.from_xml_path(str(OUT/f'{profile}.xml'))
                owner={x['id']:x['owner'] for x in graph['source_body_owners']}
                geoms={b['id']:[i for i in range(native.ngeom) if int(native.geom_bodyid[i]) and owner[int(native.geom_bodyid[i])]==b['id']] for b in graph['bodies']}
                actual={(x['body1'],x['body2']) for x in graph['collision_exclusions']}
                expected=set()
                ids=list(geoms)
                for i,a in enumerate(ids):
                    for b in ids[i+1:]:
                        allowed=any((int(native.geom_contype[x])&int(native.geom_conaffinity[y])) or (int(native.geom_contype[y])&int(native.geom_conaffinity[x])) for x in geoms[a] for y in geoms[b])
                        if not allowed:expected.add((a,b))
                self.assertEqual(actual,expected)
        # The official Golf collision shapes accept the world but do not
        # mutually collide. This regression was exposed by real Chaos energy.
        golf=compile_graph('galbot_g1_golf')
        self.assertGreater(len(golf['collision_exclusions']),100)
if __name__=='__main__':unittest.main()
