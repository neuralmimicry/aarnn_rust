#!/usr/bin/env python3
"""Offline USD 26.5 emitter. Does not import or initialise Isaac/Kit.

Run in the separate USD conversion venv. Isaac runtime evidence is independent.
"""
import argparse
import json
import math
import os
from pathlib import Path
from pxr import Gf, Sdf, Tf, Usd, UsdGeom, UsdPhysics, UsdLux
ROOT=Path(__file__).resolve().parents[1]
DIRECTORY=ROOT/'sim/content/embodiments/generated'


def quat(values):return Gf.Quatf(float(values[0]),Gf.Vec3f(*values[1:]))


def axis_quat(axis):
    # USD joint axis X is rotated onto the arbitrary model axis.
    rotation=Gf.Rotation(Gf.Vec3d(1,0,0),Gf.Vec3d(*axis))
    q=rotation.GetQuat();return Gf.Quatf(q.GetReal(),Gf.Vec3f(q.GetImaginary()))


def emit(record,path):
    stage=Usd.Stage.CreateNew(str(path))
    UsdGeom.SetStageUpAxis(stage,UsdGeom.Tokens.z);UsdGeom.SetStageMetersPerUnit(stage,1)
    root=UsdGeom.Xform.Define(stage,'/Robot');stage.SetDefaultPrim(root.GetPrim())
    root.GetPrim().SetCustomDataByKey('aarnnModelDigest',record['digest'])
    root.GetPrim().SetCustomDataByKey('aarnnIODigest',record['io']['digest'])
    official=record['profile_id']=='galbot_g1_golf'
    if official:
        source=ROOT/'.agent/robot-expansion/assets/galbot-g1-golf/usd/galbot_one_golf.usda'
        root.GetPrim().GetReferences().AddReference(os.path.relpath(source,path.parent))
    if not official:UsdPhysics.ArticulationRootAPI.Apply(root.GetPrim())
    bodies=[] if official else record['bodies'];paths={0:Sdf.Path('/Habitat')}
    UsdGeom.Xform.Define(stage,'/Habitat')
    # Preserve body hierarchy, local transforms and fixed relationships.
    for bid,b in enumerate(bodies,1):
        parent=paths[b['parent']] if b['parent'] else Sdf.Path('/Robot')
        bp=Sdf.Path('/Robot').AppendChild(Tf.MakeValidIdentifier(b['name']));paths[bid]=bp
        body=UsdGeom.Xform.Define(stage,bp)
        body.AddTranslateOp().Set(Gf.Vec3d(*b['world_position']))
        body.AddOrientOp().Set(quat(b['world_quaternion_wxyz']))
        if b['mass_kg']>0:
            UsdPhysics.RigidBodyAPI.Apply(body.GetPrim())
            mass=UsdPhysics.MassAPI.Apply(body.GetPrim())
            mass.CreateMassAttr(float(b['mass_kg']));mass.CreateCenterOfMassAttr(Gf.Vec3f(*b['com_m']))
            mass.CreateDiagonalInertiaAttr(Gf.Vec3f(*b['diagonal_inertia_kgm2']))
            mass.CreatePrincipalAxesAttr(quat(b['inertia_orientation_wxyz']))
    # MJCF primitive half extents are converted explicitly to USD dimensions.
    for index,g in enumerate(record['geoms']):
        if official and g['body']:continue
        gp=paths[g['body']].AppendChild(Tf.MakeValidIdentifier(g['name'] or 'geom_'+str(index)))
        size=g['half_size_m'];typ=g['type']
        if typ==0:
            extent=record.get('scene',{}).get('half_extent_m',10)*1.2
            shape=UsdGeom.Cube.Define(stage,gp);shape.CreateSizeAttr(2.);scale=[extent,extent,.025]
        elif typ==6:
            shape=UsdGeom.Cube.Define(stage,gp);shape.CreateSizeAttr(2.);scale=size
        elif typ in (2,4):
            shape=UsdGeom.Sphere.Define(stage,gp);shape.CreateRadiusAttr(1.);scale=size if typ==4 else [size[0]]*3
        elif typ in (3,5):
            shape=(UsdGeom.Capsule if typ==3 else UsdGeom.Cylinder).Define(stage,gp)
            shape.CreateRadiusAttr(size[0]);shape.CreateHeightAttr(size[1]*2);shape.CreateAxisAttr('Z');scale=[1,1,1]
        else:raise ValueError(f'unsupported primitive type {typ}; do not silently substitute meshes')
        position=list(g['position'])
        if typ==0:position[2]-=.025
        shape.AddTranslateOp().Set(Gf.Vec3d(*position));shape.AddOrientOp().Set(quat(g['quaternion_wxyz']))
        shape.AddScaleOp().Set(Gf.Vec3f(*scale));shape.CreateDisplayColorAttr([Gf.Vec3f(*g['rgba'][:3])])
        shape.CreateDisplayOpacityAttr([float(g['rgba'][3])])
        if g['collision']:UsdPhysics.CollisionAPI.Apply(shape.GetPrim())
    for o in record.get('scene',{}).get('objects',[]):
        if o['cue'] not in ('light','heat'):continue
        lamp=UsdLux.SphereLight.Define(stage,'/Habitat/'+Tf.MakeValidIdentifier('cue_'+o['id']))
        p=list(o['position']);p[2]+=max(.06,record['scene']['half_extent_m']*.12)
        lamp.AddTranslateOp().Set(Gf.Vec3d(*p));lamp.CreateIntensityAttr(400.)
        lamp.CreateRadiusAttr(.025);lamp.CreateColorAttr(Gf.Vec3f(*([.9,.28,.08] if o['cue']=='heat' else [.9,.85,.62])))
    actuators={a['joint']:a for a in record['actuators']}
    jointed={j['body'] for j in record['joints'] if j['type']!=0}
    for j in [] if official else record['joints']:
        if j['type']==0:continue
        if j['type'] not in (2,3):raise ValueError('unsupported spherical joint needs explicit adapter')
        b=bodies[j['body']-1];jp=Sdf.Path('/Robot').AppendChild(Tf.MakeValidIdentifier('joint_'+j['name']))
        joint=(UsdPhysics.PrismaticJoint if j['type']==2 else UsdPhysics.RevoluteJoint).Define(stage,jp)
        joint.GetPrim().SetCustomDataByKey('aarnnJointName',j['name'])
        joint.CreateAxisAttr('X');joint.CreateBody1Rel().SetTargets([paths[j['body']]])
        if b['parent']:joint.CreateBody0Rel().SetTargets([paths[b['parent']]])
        # Child joint pivot is transformed into parent coordinates at rest.
        rotation=Gf.Rotation(Gf.Quatd(*[float(b['quaternion_wxyz'][0]),Gf.Vec3d(*b['quaternion_wxyz'][1:])]))
        local=Gf.Vec3d(*j['position']);p0=Gf.Vec3d(*b['position'])+rotation.TransformDir(local)
        joint.CreateLocalPos0Attr(Gf.Vec3f(p0));joint.CreateLocalPos1Attr(Gf.Vec3f(local))
        jq=axis_quat(j['axis'])
        joint.CreateLocalRot0Attr(quat(b['quaternion_wxyz'])*jq);joint.CreateLocalRot1Attr(jq)
        if j['limited']:
            factor=1 if j['type']==2 else 180/math.pi
            joint.CreateLowerLimitAttr(j['range'][0]*factor);joint.CreateUpperLimitAttr(j['range'][1]*factor)
        if j['name'] in actuators:
            drive=UsdPhysics.DriveAPI.Apply(joint.GetPrim(),'linear' if j['type']==2 else 'angular')
            drive.CreateTypeAttr('force');drive.CreateStiffnessAttr(0);drive.CreateDampingAttr(0)
            # Initially disarmed. The runtime must apply bounded named efforts.
            drive.CreateMaxForceAttr(0)
    for bid,b in enumerate(bodies,1):
        if bid in jointed or any(j['body']==bid and j['type']==0 for j in record['joints']):continue
        joint=UsdPhysics.FixedJoint.Define(stage,'/Robot/'+Tf.MakeValidIdentifier('fixed_'+b['name']))
        joint.CreateBody1Rel().SetTargets([paths[bid]])
        if b['parent']:joint.CreateBody0Rel().SetTargets([paths[b['parent']]])
        joint.CreateLocalPos0Attr(Gf.Vec3f(*b['position']));joint.CreateLocalRot0Attr(quat(b['quaternion_wxyz']))
    stage.GetRootLayer().Save()


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--check',action='store_true');args=parser.parse_args()
    import tempfile
    for source in sorted(DIRECTORY.glob('*.json')):
        if source.name.endswith('.config.json') or source.name=='registry.generated.json':continue
        record=json.loads(source.read_text());path=source.with_suffix('.usda')
        # Temporary sibling preserves relative upstream references.
        with tempfile.TemporaryDirectory(dir=DIRECTORY) as temp:
            temporary=Path(temp)/path.name
            emit(record,temporary)
            text=temporary.read_text()
            # Official reference relative path differs by the temporary directory.
            if record['profile_id']=='galbot_g1_golf':text=text.replace('../../../../../.agent/','../../../../.agent/')
            if args.check:
                if not path.exists() or path.read_text()!=text:raise SystemExit('stale USD: '+str(path))
            else:path.write_text(text)
    print('USD assets verified' if args.check else 'USD assets generated (not Isaac execution evidence)')

if __name__=='__main__':main()
