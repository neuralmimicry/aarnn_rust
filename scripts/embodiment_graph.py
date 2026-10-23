#!/usr/bin/env python3
"""Engine-neutral rigid-cluster export for native Unity/Unreal factories.

The authored canonical MJCF/record stays authoritative. This generated graph
preserves fixed-link mass/inertia, named joints, collision/visual geometry and
transmission coefficients, rather than inventing engine-specific robot aliases.
"""
import argparse
from collections import defaultdict
import json
import math
from pathlib import Path
import sys
import numpy as np
import mujoco
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'scripts'))
from embodiment import NEW,LEGACY,OUT,digest
from embodiment_geometry import primitive
DEST=ROOT/'sim/content/embodiments/graphs'

def matrix(q):
    out=np.empty(9);mujoco.mju_quat2Mat(out,np.array(q,dtype=float));return out.reshape(3,3)
def quaternion(rotation):
    out=np.empty(4);mujoco.mju_mat2Quat(out,np.asarray(rotation,dtype=float).ravel());return out.tolist()
def multiply(a,b):
    out=np.empty(4);mujoco.mju_mulQuat(out,np.array(a,dtype=float),np.array(b,dtype=float));return out.tolist()
def fixed_clusters(record):
    joints=defaultdict(list);leaders={};clusters=defaultdict(list);composite={}
    for j in record['joints']:joints[j['body']].append(j)
    for bid,b in enumerate(record['bodies'],1):
        leader=bid if not b['parent'] or joints[bid] else leaders[b['parent']]
        leaders[bid]=leader;clusters[leader].append(bid)
    for leader,members in clusters.items():
        base=record['bodies'][leader-1];basis=matrix(base['world_quaternion_wxyz']);origin=np.array(base['world_position']);parts=[]
        for bid in members:
            b=record['bodies'][bid-1];rotation=basis.T@matrix(b['world_quaternion_wxyz']);position=basis.T@(np.array(b['world_position'])-origin)+rotation@b['com_m'];ir=rotation@matrix(b['inertia_orientation_wxyz'])
            parts.append((b['mass_kg'],position,ir@np.diag(b['diagonal_inertia_kgm2'])@ir.T))
        mass=sum(m for m,_,_ in parts);com=sum((m*p for m,p,_ in parts),np.zeros(3))/mass if mass else np.zeros(3)
        inertia=sum((i+m*(np.dot(p-com,p-com)*np.eye(3)-np.outer(p-com,p-com)) for m,p,i in parts),np.zeros((3,3)))
        composite[leader]=(mass,com,inertia)
    if not math.isclose(sum(x[0] for x in composite.values()),record['total_mass_kg'],rel_tol=1e-12,abs_tol=1e-12):raise ValueError('fixed cluster mass conservation failed')
    return leaders,clusters,composite

def relative(record,bid,parent):
    b=record['bodies'][bid-1];p=record['bodies'][parent-1];r=matrix(p['world_quaternion_wxyz']).T
    return r@(np.array(b['world_position'])-p['world_position']),quaternion(r@matrix(b['world_quaternion_wxyz']))
def joint_maps(record,model):
    motors={a['joint']:i for i,a in enumerate(record['actuators'])};constraints=[]
    for c in record['constraints']:
        if c['tag']!='joint':raise ValueError('constraint type requires explicit implementation')
        p=list(map(float,c.get('polycoef','0 1 0 0 0').split()))
        if len(p)!=5 or any(p[2:]) or p[1]==0:raise ValueError('nonlinear equality needs an explicit adapter')
        j1,j2=[mujoco.mj_name2id(model,mujoco.mjtObj.mjOBJ_JOINT,c[key]) for key in ('joint1','joint2')]
        d1,d2=[model.jnt_dofadr[j] for j in (j1,j2)];effective=min(model.dof_M0[d1],model.dof_M0[d2]/p[1]**2)
        constraints.append(dict(joint1=c['joint1'],joint2=c['joint2'],offset=p[0],ratio=p[1],stiffness=.05*effective/.002**2,damping=.2*effective/.002,scope='target compliant approximation; calibration pending'))
    coupled={c[k] for c in constraints for k in ('joint1','joint2')};targets=[]
    for j in record['joints']:
        if j['type']==0:continue
        jid=mujoco.mj_name2id(model,mujoco.mjtObj.mjOBJ_JOINT,j['name']);aid=motors.get(j['name'])
        if aid is not None:
            force=model.actuator_forcerange[aid].tolist() if model.actuator_forcelimited[aid] else [-1e6,1e6]
            gain=float(model.actuator_gainprm[aid,0]);bias=model.actuator_biasprm[aid,:3].tolist();gear=float(model.actuator_gear[aid,0])
        else:gain=0;bias=[0,0,0];gear=1;force=[0,0]
        targets.append(dict(joint=j['name'],device=j['name'],sensor=j['name']+'_position',body=j['body'],kind='linear' if j['type']==2 else 'rotational',actuator=aid,gain=gain,bias=bias,gear=gear,force_range=force,damping=float(model.dof_damping[model.jnt_dofadr[jid]]),driven=aid is not None or j['name'] in coupled))
    return targets,constraints

def compile_graph(profile):
    record=json.loads((OUT/f'{profile}.json').read_text());model=mujoco.MjModel.from_xml_path(str(OUT/f'{profile}.xml'))
    leaders,clusters,composite=fixed_clusters(record);targets,couplings=joint_maps(record,model);free={j['body'] for j in record['joints'] if j['type']==0};bodies=[];geoms=[];joints=[];meshes=[]
    for leader,members in clusters.items():
        b=record['bodies'][leader-1];mass,com,inertia=composite[leader]
        if mass<=0:continue # Empty source frames retain their owner map below.
        principal,axes=np.linalg.eigh(inertia)
        if not np.all(principal>0):raise ValueError('invalid principal inertia')
        for i in range(3):
            if axes[np.argmax(np.abs(axes[:,i])),i]<0:axes[:,i]*=-1
        if np.linalg.det(axes)<0:axes[:,-1]*=-1
        bodies.append(dict(id=leader,name=b['name'],source_members=members,world_position=b['world_position'],world_quaternion_wxyz=b['world_quaternion_wxyz'],mass_kg=mass,com_m=com.tolist(),inertia_kgm2=inertia.tolist(),principal_inertia_kgm2=principal.tolist(),principal_axes_wxyz=quaternion(axes),anchored=not b['parent'] and leader not in free))
    for index,g in enumerate(record['geoms']):
        if not g['body']:continue
        owner=leaders[g['body']]
        if g['body']==owner:p=[0,0,0];q=[1,0,0,0]
        else:p,q=relative(record,g['body'],owner)
        local=np.asarray(p)+matrix(q)@g['position']
        geoms.append(dict(g,id=index,owner=owner,position=local.tolist(),quaternion_wxyz=multiply(q,g['quaternion_wxyz'])))
    for j in record['joints']:
        if j['type']==0:continue
        b=record['bodies'][j['body']-1];parent=leaders[b['parent']];p,q=relative(record,j['body'],parent);rotation=matrix(q)
        axis=np.asarray(j['axis'],dtype=float);axis/=np.linalg.norm(axis);other=np.array([0,0,1]) if abs(axis[2])<.9 else np.array([0,1,0]);second=np.cross(other,axis);second/=np.linalg.norm(second);basis=np.column_stack([axis,second,np.cross(axis,second)])
        joints.append(dict(j,parent=parent,child=leaders[j['body']],parent_position=(p+rotation@j['position']).tolist(),child_position=j['position'],parent_quaternion_wxyz=quaternion(rotation@basis),child_quaternion_wxyz=quaternion(basis)))
    for mid in sorted({g['mesh_id'] for g in geoms if g['type']==7}):
        va,vn,fa,fn=[int(a[mid]) for a in (model.mesh_vertadr,model.mesh_vertnum,model.mesh_faceadr,model.mesh_facenum)]
        meshes.append(dict(id=mid,vertices_xyz=model.mesh_vert[va:va+vn].astype(float).ravel().tolist(),triangles_ijk=model.mesh_face[fa:fa+fn].astype(int).ravel().tolist()))
    world_geoms=[]
    for index,g in enumerate(record['geoms']):
        if g['body']:continue
        item=dict(g,id=index,owner=0)
        if item['type']==0:
            extent=record.get('scene',{}).get('half_extent_m',10)*1.2
            item.update(type=6,half_size_m=[extent,extent,.025],position=[g['position'][0],g['position'][1],g['position'][2]-.025])
        world_geoms.append(item)
    analytic={}
    for g in geoms+world_geoms:
        if g['type']==7:g['render_mesh']=g['mesh_id'];continue
        key=(g['type'],*g['half_size_m'])
        if key not in analytic:
            mid=-(len(analytic)+1);analytic[key]=mid
            vertices,triangles=primitive(g['type'],g['half_size_m'])
            meshes.append(dict(id=mid,vertices_xyz=[v for row in vertices for v in row],triangles_ijk=[v for row in triangles for v in row]))
        g['render_mesh']=analytic[key]
    cameras=[]
    for cid in range(model.ncam):
        bid=int(model.cam_bodyid[cid]);owner=leaders[bid]
        p,q=([0,0,0],[1,0,0,0]) if bid==owner else relative(record,bid,owner)
        cameras.append(dict(name=mujoco.mj_id2name(model,mujoco.mjtObj.mjOBJ_CAMERA,cid),owner=owner,position=(np.asarray(p)+matrix(q)@model.cam_pos[cid]).tolist(),quaternion_wxyz=multiply(q,model.cam_quat[cid]),fovy_deg=float(model.cam_fovy[cid])))
    graph=dict(schema_version=1,compiler='embodiment-rigid-graph-v2',digest_encoding='embodiment-binary-v1',profile_id=profile,model_digest=record['digest'],io_digest=record['io']['digest'],fidelity=record['fidelity'],variant_id=record['variant_id'],controller_transfer=record['controller_transfer'],coordinates='right-handed X-forward Y-left Z-up; SI',dt_us=2000,collision_tessellation='convex-primitives-v1:16-segments-8-rings; per-mesh convex hull',bodies=bodies,geoms=geoms,joints=joints,targets=targets,constraints=couplings,cameras=cameras,meshes=meshes,actuators=record['actuators'],source_body_owners=[dict(id=i+1,owner=leaders[i+1],name=b['name']) for i,b in enumerate(record['bodies'])],io=record['io'],calibration='native solver, collision/mesh convex approximation and compliant equality coupling require target calibration; no learned-task assertion')
    # A nonzero conaffinity alone does not imply mutual self-collision. Keep
    # the canonical masks when reducing shapes to native rigid clusters.
    masks=defaultdict(list)
    for gid in range(model.ngeom):
        bid=int(model.geom_bodyid[gid])
        if bid:masks[leaders[bid]].append((int(model.geom_contype[gid]),int(model.geom_conaffinity[gid])))
    exclusions=[]
    for i,a in enumerate(bodies):
        for b in bodies[i+1:]:
            x,y=a['id'],b['id']
            if not any((at&bc) or (bt&ac) for at,ac in masks[x] for bt,bc in masks[y]):
                exclusions.append(dict(body1=x,body2=y,reason='canonical source collision masks'))
    graph['world_geoms']=world_geoms
    graph['standard_projection']=not record['controller_transfer'].startswith('blocked') and (profile=='crow_corvus_corone' or all(s['kind'] in ('angle','angular_velocity','body_gyro','range_ray') for s in record['io']['sensors']))
    graph['scene']=record.get('scene')
    graph['collision_exclusions']=exclusions
    graph['collision_filter_scope']='source masks aggregated per rigid cluster; joint-neighbour exclusions native; different instances retain external contact'
    graph['digest']=digest(graph);return graph

def products():return {DEST/f'{p}.json':json.dumps(compile_graph(p),indent=2,allow_nan=False)+'\n' for p in NEW+LEGACY}
def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--check',action='store_true');args=parser.parse_args();DEST.mkdir(parents=True,exist_ok=True)
    for path,text in products().items():
        if args.check:
            if not path.exists() or path.read_text()!=text:raise SystemExit('stale rigid graph '+str(path))
        else:path.write_text(text)
    print('Rigid graphs verified' if args.check else 'Rigid graphs generated; engine execution remains separate')
if __name__=='__main__':main()
