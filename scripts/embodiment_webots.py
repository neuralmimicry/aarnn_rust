#!/usr/bin/env python3
"""Export canonical mechanics into native Webots worlds (not task acceptance).

Webots/ODE compliance, contact and joint integration need separate calibration.
Equality joints use a named, dissipative two-joint compliant coupling in the
Rust controller; coupled motor commands are never mistaken for a constraint.
"""
import argparse
from collections import defaultdict
import hashlib
import json
import math
from pathlib import Path
import sys
import numpy as np
import mujoco
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'scripts'))
from embodiment import NEW, LEGACY, OUT, digest
from embodiment_graph import fixed_clusters, joint_maps, relative as shared_relative
DEST=ROOT/'sim/content/embodiments/webots'

def numbers(v):return ' '.join(format(float(x),'.17g') for x in v)
def quote(s):return json.dumps(s)
def matrix(q):
    out=np.empty(9);mujoco.mju_quat2Mat(out,np.array(q,dtype=float));return out.reshape(3,3)
def axis_angle(q):
    q=np.array(q,dtype=float);q/=np.linalg.norm(q)
    if q[0]<0:q=-q
    v=np.linalg.norm(q[1:]);return [0,0,1,0] if v<1e-12 else [*(q[1:]/v),2*math.atan2(v,q[0])]
def multiply(a,b):
    out=np.zeros(4);mujoco.mju_mulQuat(out,np.array(a),np.array(b));return out

def emit(profile,instance=None,offset=(0,0,0),controller='aarnn_webots',args=()):
    record=json.loads((OUT/f'{profile}.json').read_text());model=mujoco.MjModel.from_xml_path(str(OUT/f'{profile}.xml'))
    instance=instance or profile+'_0'; children=defaultdict(list);joints=defaultdict(list);geoms=defaultdict(list);meshes=set();fixed=[]
    for i,b in enumerate(record['bodies'],1):children[b['parent']].append(i)
    for j in record['joints']:joints[j['body']].append(j)
    for i,g in enumerate(record['geoms']):geoms[g['body']].append((i,g))
    # Webots needs a Physics owner at the moving root. Massless URDF/MJCF
    # wrapper links cannot be left as fixed supports. Merge each fixed-link
    # cluster using exact COM/inertia transforms and the parallel-axis theorem.
    leaders,clusters,composite=fixed_clusters(record)
    moving_children=defaultdict(list)
    for bid,b in enumerate(record['bodies'],1):
        if b['parent'] and leaders[bid]==bid:moving_children[leaders[b['parent']]].append(bid)
    def relative(bid,parent):return shared_relative(record,bid,parent)
    targets,constraints=joint_maps(record,model)
    target_by_joint={t['joint']:t for t in targets}
    def geometry(index,g):
        typ=g['type'];size=g['half_size_m'];q=g['quaternion_wxyz'];pos=g['position']
        if typ==6: shape=f'Box {{ size {numbers(np.array(size)*2)} }}'
        elif typ==2:shape=f'Sphere {{ radius {size[0]:.17g} subdivision 2 }}'
        elif typ==4:
            vertices=[[size[0]*math.sin(math.pi*y/8)*math.cos(2*math.pi*x/16),size[1]*math.sin(math.pi*y/8)*math.sin(2*math.pi*x/16),size[2]*math.cos(math.pi*y/8)] for y in range(9) for x in range(16)]
            faces=[]
            for y in range(8):
                for x in range(16):
                    a=y*16+x;b=y*16+(x+1)%16;c=(y+1)*16+x;d=(y+1)*16+(x+1)%16
                    if y:faces.append([a,b,c])
                    if y<7:faces.append([b,d,c])
            shape='IndexedFaceSet { coord Coordinate { point [ '+', '.join(numbers(v) for v in vertices)+' ] } coordIndex [ '+', '.join(' '.join(map(str,f))+' -1' for f in faces)+' ] }'
        elif typ in (3,5):
            shape=f'{"Capsule" if typ==3 else "Cylinder"} {{ radius {size[0]:.17g} height {size[1]*2:.17g} subdivision 16 }}'
            q=multiply(q,[math.sqrt(.5),math.sqrt(.5),0,0])
        elif typ==7:
            mid=g['mesh_id'];name=f'{instance}_MESH_{mid}'
            if mid in meshes:shape='USE '+name
            else:
                meshes.add(mid);va=model.mesh_vertadr[mid];vn=model.mesh_vertnum[mid];fa=model.mesh_faceadr[mid];fn=model.mesh_facenum[mid]
                vertices=model.mesh_vert[va:va+vn];faces=model.mesh_face[fa:fa+fn]
                shape=f'DEF {name} IndexedFaceSet {{ coord Coordinate {{ point [ '+', '.join(numbers(v) for v in vertices)+' ] } coordIndex [ '+', '.join(' '.join(map(str,f))+' -1' for f in faces)+' ] }'
        else:raise ValueError('unsupported native Webots shape '+str(typ))
        return f'Pose {{ translation {numbers(pos)} rotation {numbers(axis_angle(q))} children [ Shape {{ appearance PBRAppearance {{ baseColor {numbers(g["rgba"][:3])} transparency {1-g["rgba"][3]} roughness .8 metalness 0 }} geometry {shape} }} ] }}'
    def body(bid):
        b=record['bodies'][bid-1];root=bid==1;name=f'{instance}_BODY_{bid}'
        leader=leaders[bid]==bid
        position=np.array(b['position'])+np.array(offset) if not b['parent'] else b['position'];orientation=b['quaternion_wxyz']
        if leader and b['parent']:position,orientation=relative(bid,leaders[b['parent']])
        content=[];collision=[]
        for gid,g in geoms[bid]:
            node=geometry(gid,g);content.append(node)
            if g['collision']:
                # DEF/USE prevents duplicating an imported mesh; collision uses
                # the same local shape and transform, without a second mass.
                content[-1]=f'DEF {instance}_GEOM_{gid} '+node
                collision.append(f'USE {instance}_GEOM_{gid}')
        if root:
            content.append('Gyro { name "body_gyro" }')
            content.extend(f'DistanceSensor {{ name "proximity_{i}" translation 0 0 .1 rotation 0 0 1 {2*math.pi*i/8:.17g} type "laser" aperture .001 lookupTable [0 0 0, 3 3 0] }}' for i in range(8))
        for cid in range(model.ncam):
            if model.cam_bodyid[cid]!=bid:continue
            camera_name=mujoco.mj_id2name(model,mujoco.mjtObj.mjOBJ_CAMERA,cid)
            # Webots camera +X forward, +Y left, +Z up; MJCF camera -Z
            # forward, +X right, +Y up. Test this basis independently.
            conversion=np.array([[0,-1,0],[0,0,1],[-1,0,0]],dtype=float);cq=np.empty(4);mujoco.mju_mat2Quat(cq,conversion.ravel())
            q=multiply(model.cam_quat[cid],cq)
            content.append(f'Camera {{ name {quote(camera_name)} translation {numbers(model.cam_pos[cid])} rotation {numbers(axis_angle(q))} width 12 height 8 fieldOfView {2*math.atan(12/8*math.tan(math.radians(model.cam_fovy[cid])/2)):.17g} near .001 }}')
        if b['name']=='neck_roll_link':content.extend(['Gyro { name "head_gyro" }','Accelerometer { name "head_acceleration" }'])
        for child in children[bid]:
            if leaders[child]!=child:content.append(body(child))
        for child in moving_children[bid] if leader else []:
            js=[j for j in joints[child] if j['type']!=0]
            if len(js)>1:raise ValueError('multiple child joint frames need an explicit exporter')
            if not js:content.append(body(child));continue
            j=js[0];child_position,child_orientation=relative(child,bid);r=matrix(child_orientation);axis=r@j['axis'];anchor=child_position+r@j['position'];t=target_by_joint[j['name']]
            limit=f'minStop {j["range"][0]:.17g} maxStop {j["range"][1]:.17g}' if j['limited'] else ''
            device=f'PositionSensor {{ name {quote(t["sensor"])} }}'
            if t['driven']:
                device+=f' {"LinearMotor" if j["type"]==2 else "RotationalMotor"} {{ name {quote(t["device"])} {"maxForce" if j["type"]==2 else "maxTorque"} 1000000 maxVelocity 1000 sound "" }}'
            content.append(f'{"SliderJoint" if j["type"]==2 else "HingeJoint"} {{ jointParameters {"JointParameters" if j["type"]==2 else "HingeJointParameters"} {{ axis {numbers(axis)} '+('' if j['type']==2 else f'anchor {numbers(anchor)} ')+f'{limit} dampingConstant {t["damping"]:.17g} }} device [ {device} ] endPoint {body(child)} }}')
        mass,com,inertia=composite.get(bid,(0,np.zeros(3),np.zeros((3,3))))
        physical=f'physics Physics {{ density -1 mass {mass:.17g} centerOfMass [ {numbers(com)} ] inertiaMatrix [ {numbers(np.diag(inertia))}, {numbers([inertia[0,1],inertia[0,2],inertia[1,2]])} ] }}' if mass>0 else ''
        if any(j['type']==0 for j in joints[bid]) and not physical:raise ValueError('free root has no positive composite mass')
        # A body connected directly to the world without a freejoint is an
        # explicit fixed fixture. Dynamic children retain their own Physics.
        if not b['parent'] and not any(j['type']==0 for j in joints[bid]):physical='';fixed.append(b['name'])
        extra=f'controller {quote(controller)} controllerArgs [ '+ ' '.join(quote(str(a)) for a in args)+' ] supervisor TRUE selfCollision FALSE synchronization TRUE' if root else ''
        if leader:
            for member in clusters[bid]:
                if member==bid:continue
                p,q=relative(member,bid)
                collision.extend(f'Pose {{ translation {numbers(p)} rotation {numbers(axis_angle(q))} children [ USE {instance}_GEOM_{gid} ] }}' for gid,g in geoms[member] if g['collision'])
        # An explicit empty Group keeps a collisionless massive rotor dynamic;
        # it adds no hidden collision primitive or supporting surface.
        bounding='boundingObject Group { children [ '+ '\n'.join(collision)+' ] }' if leader else ''
        naming=f'name {quote(instance if root else b["name"])}' if leader else ''
        return f'DEF {name} {"Robot" if root else "Solid" if leader else "Pose"} {{ translation {numbers(position)} rotation {numbers(axis_angle(orientation))} {naming} {extra} children [ '+ '\n'.join(content)+f' ] {bounding} {physical} }}'
    robots='\n'.join(body(bid) for bid in children[0])
    scenery=[]
    for gid,g in geoms[0]:
        if g['type']==0:continue # One explicit world floor, not duplicate supports.
        node=geometry(gid,g)
        shape=f'DEF {instance}_SCENERY_{gid} '+node
        hull=f'boundingObject USE {instance}_SCENERY_{gid}' if g['collision'] else ''
        scenery.append(f'Solid {{ name {quote(instance+"_"+(g["name"] or str(gid)))} translation {numbers([offset[0],offset[1],0])} children [ {shape} ] {hull} }}')
    robots+='\n'+'\n'.join(scenery)
    for o in record.get('scene',{}).get('objects',[]):
        if o['cue'] in ('light','heat'):
            p=[o['position'][0]+offset[0],o['position'][1]+offset[1],o['position'][2]+max(.06,record['scene']['half_extent_m']*.12)]
            colour=[.9,.28,.08] if o['cue']=='heat' else [.9,.85,.62]
            robots+=f'\nPointLight {{ location {numbers(p)} color {numbers(colour)} intensity .8 attenuation 1 0 1 radius {record["scene"]["half_extent_m"]*2} }}'
    meta=dict(schema_version=1,profile_id=profile,model_digest=record['digest'],io_digest=record['io']['digest'],targets=targets,constraints=constraints,fixed_world_bodies=fixed,composite_mass_kg=sum(c[0] for c in composite.values()),controller_transfer=record['controller_transfer'],calibration='ODE contact/constraint response not calibrated; all self-collision suppressed; mesh collisions use Webots dynamic convex policy; fixed-link masses merged exactly',coordinates='X-forward Y-left Z-up; SI',scope='native articulated source; runtime and learned task gates independent')
    meta['scene']=record.get('scene')
    meta['world_geometry_count']=len(scenery)
    meta['digest']=digest(meta)
    return robots,meta

def world(robots):
    return '#VRML_SIM R2025a utf8\nWorldInfo { basicTimeStep 2 coordinateSystem "ENU" gravity 9.81 }\nViewpoint { orientation .57735 -.57735 -.57735 2.0944 position 3 -3 2 }\nBackground { skyColor [.3 .4 .5] }\nDirectionalLight { direction -1 1 -2 intensity 1 }\nSolid { translation 0 0 -.05 children [ Shape { appearance PBRAppearance { baseColor .4 .4 .4 roughness 1 metalness 0 } geometry Box { size 200 200 .1 } } ] boundingObject Box { size 200 200 .1 } }\n'+robots+'\n'

def products():
    result={}
    for p in NEW+LEGACY:
        robots,meta=emit(p);result[DEST/f'{p}.wbt']=world(robots);result[DEST/f'{p}.webots.json']=json.dumps(meta,indent=2)+'\n'
    return result

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--check',action='store_true');args=parser.parse_args();DEST.mkdir(parents=True,exist_ok=True)
    for path,text in products().items():
        if args.check:
            if not path.exists() or path.read_text()!=text:raise SystemExit('stale Webots export: '+str(path))
        else:path.write_text(text)
    print('Webots source verified' if args.check else 'Webots articulated source generated; engine execution remains separate')
if __name__=='__main__':main()
