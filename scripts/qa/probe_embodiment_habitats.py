#!/usr/bin/env python3
"""Cross-profile native scene/morphology audit, separate from task acceptance."""
import argparse
import json
from pathlib import Path
import sys
import numpy as np
import mujoco
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'scripts'))
from embodiment import NEW,LEGACY,OUT,products
from embodiment_graph import compile_graph


def audit():
    rows=[]
    for profile in NEW+LEGACY:
        record=json.loads((OUT/f'{profile}.json').read_text())
        scene=record['scene'];graph=compile_graph(profile)
        assert len(scene['objects'])>=50 and scene['cues'],profile
        m=mujoco.MjModel.from_xml_path(str(OUT/f'{profile}.xml'));d=mujoco.MjData(m)
        assert abs(sum(m.body_mass)-record['total_mass_kg'])<1e-10
        for _ in range(100):mujoco.mj_step(m,d)
        assert np.all(np.isfinite(d.qpos)),profile
        native_world={g['name'] for g in record['geoms'] if not g['body'] and g['type']!=0}
        planes={g['name'] for g in record['geoms'] if not g['body'] and g['type']==0}
        assert native_world=={g['name'] for g in graph['world_geoms'] if g['name'] not in planes},profile
        webots=(ROOT/f'sim/content/embodiments/webots/{profile}.wbt').read_text()
        assert all(name in webots for name in native_world),profile
        fields=[o['id'] for o in scene['objects'] if o['cue']]
        rows.append(dict(profile_id=profile,habitat_id=scene['habitat_id'],objects=len(scene['objects']),native_world_geoms=len(native_world),cue_objects=fields,model_digest=record['digest'],io_digest=record['io']['digest'],mass_kg=record['total_mass_kg'],primitive_types=sorted({g['type'] for g in record['geoms'] if g['body']}),status='pass',scope='native load/finite state and source scene export, not learned behaviour/biological calibration'))
    worm=json.loads((OUT/'celegans.json').read_text())
    hulls=[g for g in worm['geoms'] if g['name'].startswith('segment_') and g['name'].endswith('_collision')]
    assert len(hulls)==24 and all(g['type']==3 for g in hulls)
    assert max(g['half_size_m'][0] for g in hulls)>1.8*min(g['half_size_m'][0] for g in hulls)
    assert len([g for g in worm['geoms'] if '_muscle_quadrant_' in g['name']])==95
    assert not any('eye' in g['name'] for g in worm['geoms'])
    m=mujoco.MjModel.from_xml_path(str(OUT/'celegans.xml'));d=mujoco.MjData(m);mujoco.mj_forward(m,d)
    group=np.array([1,1,0,1,1,1],dtype=np.uint8);geom=np.array([-1],dtype=np.int32)
    # The front taste/range proxy now detects a real nearby collidable cue.
    head=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_BODY,'segment_23')
    point=d.xpos[head]+[.182-d.xpos[head,0],0,0]
    before=mujoco.mj_ray(m,d,point,np.array([1.,0,0]),group,True,head,geom)
    detected=mujoco.mj_id2name(m,mujoco.mjtObj.mjOBJ_GEOM,int(geom[0]))
    assert 0<before<.26 and detected.startswith('habitat_'),(before,detected)
    # Diagnostic environment perturbation, no neural or persisted state.
    g=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_GEOM,'habitat_food_contact_0');m.geom_pos[g,1]+=2.;mujoco.mj_forward(m,d)
    after=mujoco.mj_ray(m,d,point,np.array([1.,0,0]),group,True,head,geom)
    assert abs(before-after)>.02,(before,after)
    for path,expected in products().items():assert path.read_text()==expected,str(path)
    targets=['webots','unity','unreal','webgl','minecraft_java','minecraft_bedrock','mujoco','isaacsim']
    target_rows=[]
    for r in rows:
        for target in targets:
            target_rows.append(dict(profile_id=r['profile_id'],target=target,status='blocked' if target in ('unity','isaacsim') else 'not_run',source_scene='pass',reason='Native toolchain unavailable; generated assets are separate' if target in ('unity','isaacsim') else 'Source scene audit passes; native/rendered result must be attached separately',calibration='not validated'))
    return dict(status='pass',scope='source/native scene and morphology audit, not full target acceptance',profiles=rows,target_rows=target_rows,worm_stimulus=dict(detected=detected,range_before_m=float(before),range_after_m=float(after)),remaining=['Unity runtime unavailable','Isaac runtime unavailable','Minecraft remains voxel bounding geometry','WebGL imported meshes remain explicitly labelled bounds proxies','Native Rust fish water has assumed buoyancy/drag; other native factories lack equivalent fluids and hydrodynamic calibration remains open','Unreal aquatic translucent material unimplemented; water volume omitted','Biological, fluid, contact and learned task calibration remain open'])


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);args=p.parse_args()
    result=audit();args.output.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(status=result['status'],profiles=len(result['profiles']),rows=len(result['target_rows']),worm_stimulus=result['worm_stimulus'])))
