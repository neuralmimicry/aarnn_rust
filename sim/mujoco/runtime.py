"""Single-owner native MuJoCo world, optional passive viewer and async AER.

Reference outputs are explicitly uncommitted; governed actuation is unavailable
until the existing Phase 8 effect gateway is connected. No physical adapters.
"""
from __future__ import annotations
import json
import math
from pathlib import Path
import sys
import numpy as np
import mujoco

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'scripts'))
from robot_aer import Binding
from sim.mujoco.aerodynamics import apply_panels


class World:
    def __init__(self, profile, instance_id=None, brain_id=None, with_obstacles=False):
        directory=ROOT/'sim/content/embodiments/generated'
        self.record=json.loads((directory/(profile+'.json')).read_text())
        self.obstacles={};self.world_config_epoch=0
        self.model=mujoco.MjModel.from_xml_path(str(directory/(profile+'.xml')))
        if with_obstacles:
            # Fixed-size voxel collision mirrors. Moving these kinematic world
            # obstacles never moves a robot or changes its neural timeline.
            import xml.etree.ElementTree as ET
            from embodiment import element
            tree=ET.parse(directory/(profile+'.xml'));root=tree.getroot()
            for slot in range(32):
                body=element(root.find('worldbody'),'body',name=f'world_voxel_{slot}',mocap='true',pos=[100+slot,100,100])
                element(body,'geom',name=f'world_voxel_{slot}_collision',type='box',size=[.5,.5,.5],rgba=[.45,.35,.25,1])
            import tempfile
            with tempfile.NamedTemporaryFile(mode='w',suffix='.xml',dir=directory) as temp:
                temp.write(ET.tostring(root,encoding='unicode'));temp.flush()
                self.model=mujoco.MjModel.from_xml_path(temp.name)

        self.data=mujoco.MjData(self.model)
        self.instance_id=instance_id or profile+'_0'; self.brain_id=brain_id or self.instance_id
        self.reset_epoch=0; self.step_index=0; self.last_output_step=-1; self.last_applied=-1
        self.energy_J=20000.; self.food_kg=.01; self.water_kg=.01
        self.rate=np.zeros(len(self.record['actuators'])); self.process_rates=np.zeros(2); self.previous_retina=np.zeros(192)
        self.client=None; self.paused=False; self.disarmed=True; self.wind=np.zeros(3)
        self.output_rejections=0;self.last_output_rejection=None;self.actuation_inhibited=False
        self.original_gain=self.model.actuator_gainprm.copy(); self.original_bias=self.model.actuator_biasprm.copy()
        self._indices(); self.disarm(); mujoco.mj_forward(self.model,self.data)

    def _indices(self):
        m=self.model
        def idx(kind,name):
            value=mujoco.mj_name2id(m,kind,name)
            if value<0: raise ValueError('missing named model element '+name)
            return value
        self.actuator_ids=[idx(mujoco.mjtObj.mjOBJ_ACTUATOR,a['name']) for a in self.record['actuators']]
        self.joint_ids=[idx(mujoco.mjtObj.mjOBJ_JOINT,a['joint']) for a in self.record['actuators']]
        self.qpos_ids=[int(m.jnt_qposadr[j]) for j in self.joint_ids]
        self.qvel_ids=[int(m.jnt_dofadr[j]) for j in self.joint_ids]
        self.panels=[]
        if self.record['profile_id']=='crow_corvus_corone':
            for side in ('left','right'):
                self.panels.append((idx(mujoco.mjtObj.mjOBJ_BODY,side+'_wing_wrist_twist_link'),[0, .06 if side=='left' else -.06,0],.045))
            self.panels.append((idx(mujoco.mjtObj.mjOBJ_BODY,'tail_fan_link'),[-.03,0,0],.008))

    def binding(self):
        return Binding(self.instance_id,self.brain_id,1,1,self.record['digest'],self.record['io']['digest'],self.reset_epoch)

    def disarm(self):
        self.disarmed=True; self.rate[:]=0; self.process_rates[:]=0; self.data.ctrl[:]=0
        self.model.actuator_gainprm[:]=0; self.model.actuator_biasprm[:]=0

    def reset(self):
        if self.client:
            self.client.close(); self.client=None
        self.disarm(); mujoco.mj_resetData(self.model,self.data)
        self.reset_epoch+=1; self.step_index=0; self.last_output_step=-1; self.last_applied=-1
        self.previous_retina[:]=0; self.energy_J=20000.; self.food_kg=.01; self.water_kg=.01
        self.obstacles={}; self.world_config_epoch+=1
        self._indices(); mujoco.mj_forward(self.model,self.data)
        # Reconnection needs an explicit new binding; no remote brain reset.

    def accept_reference(self, reply):
        if self.actuation_inhibited:raise ValueError('locally disarmed')
        if reply['brain_id']!=self.brain_id or reply['reset_epoch']!=self.reset_epoch or reply['binding_generation']!=1:
            raise ValueError('stale or wrong-brain output')
        source=reply['step']
        if source<=self.last_applied: raise ValueError('duplicate/out-of-order output')
        if source>self.step_index or (self.step_index-source)*self.model.opt.timestep>.1:
            raise ValueError('late/future reference output')
        spikes=reply['spikes']
        if len(spikes)!=len(self.record['io']['output_names']) or any(not isinstance(x,int) or x<0 for x in spikes):
            raise ValueError('invalid output shape/spikes')
        if self.record['profile_id']=='crow_corvus_corone':self.process_rates[:]=np.clip(self.process_rates+spikes[62:64],0,1)
        self.disarmed=False
        self.last_applied=source; self.last_output_step=self.step_index
        for i in range(len(self.rate)):
            self.rate[i]=max(-1.,min(1.,self.rate[i]+spikes[2*i]-spikes[2*i+1]))

    def _drives(self):
        if self.disarmed: return
        if (self.step_index-self.last_output_step)*self.model.opt.timestep>.1:
            self.disarm(); return
        self.rate*=math.exp(-self.model.opt.timestep/.02)
        self.process_rates*=math.exp(-self.model.opt.timestep/.02)
        for i,aid in enumerate(self.actuator_ids):
            active=abs(self.rate[i])>1e-4
            self.model.actuator_gainprm[aid]=self.original_gain[aid] if active else 0
            self.model.actuator_biasprm[aid]=self.original_bias[aid] if active else 0
            low,high=self.record['actuators'][i]['control_range']
            self.data.ctrl[aid]=max(low,min(high,float(self.rate[i])*(high if self.rate[i]>=0 else -low))) if active else 0

    def _retina(self):
        samples=[]; m=self.model; d=self.data
        for side in ('left','right'):
            cid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_CAMERA,'retina_'+side)
            rotation=d.cam_xmat[cid].reshape(3,3)
            excluded=int(m.cam_bodyid[cid])
            for y in range(8):
                for x in range(12):
                    ray=rotation@np.array([(x-5.5)/8,(3.5-y)/8,-1.]);ray/=np.linalg.norm(ray)
                    geom=np.array([-1],dtype=np.int32)
                    distance=mujoco.mj_ray(m,d,d.cam_xpos[cid],ray,None,1,excluded,geom)
                    luminance=.1 if distance<0 else float(np.dot(m.geom_rgba[geom[0],:3],[.2126,.7152,.0722]))
                    samples.append(luminance)
        values=np.asarray(samples);delta=values-self.previous_retina;self.previous_retina=values
        return [v for dv in delta for v in (max(0.,dv),max(0.,-dv))]

    def sensors(self):
        if any(s['kind'].startswith('legacy-') for s in self.record['io']['sensors']):
            raise RuntimeError('This named legacy sensory adapter requires the native Rust MuJoCo owner; omit --reference-python')
        m,d=self.model,self.data
        named={}
        for i,a in enumerate(self.record['actuators']):
            named[a['joint']+'_angle']=float(d.qpos[self.qpos_ids[i]])
            named[a['joint']+'_angular_velocity']=float(d.qvel[self.qvel_ids[i]])
        if self.record['profile_id']!='crow_corvus_corone':
            velocity=np.zeros(6);mujoco.mj_objectVelocity(m,d,mujoco.mjtObj.mjOBJ_BODY,1,velocity,1)
            values=[]
            for s in self.record['io']['sensors']:
                if s['kind']=='range_ray':
                    theta=s['angle_rad'];rotation=d.xmat[1].reshape(3,3)
                    ray=rotation@np.array([math.cos(theta),math.sin(theta),0.])
                    point=d.xpos[1]+rotation@np.array([0,0,.10])
                    gid=np.array([-1],dtype=np.int32)
                    distance=mujoco.mj_ray(m,d,point,ray,None,1,1,gid)
                    raw=3. if distance<0 else min(3.,float(distance))
                elif s['kind']=='body_gyro':raw=float(velocity[s['axis']])
                elif s['kind']=='legacy-projection-unresolved':raise ValueError(self.record['controller_transfer'])
                else:raw=named[s['name']]
                values.append(max(0.,min(1.,(raw-s['range'][0])/(s['range'][1]-s['range'][0]))))
            return values
        contacts=set()
        for contact in d.contact[:d.ncon]:
            for gid in (contact.geom1,contact.geom2):
                name=mujoco.mj_id2name(m,mujoco.mjtObj.mjOBJ_GEOM,int(gid))
                if name: contacts.add(name)
        retina=self._retina(); values=list(retina)
        for a in self.record['actuators']:
            values.extend([max(0.,min(1.,.5+named[a['joint']+'_angle']/3)),max(0.,min(1.,.5+named[a['joint']+'_angular_velocity']/80))])
        head=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_BODY,'neck_roll_link')
        rotation=d.xmat[head].reshape(3,3)
        velocity=np.zeros(6);mujoco.mj_objectVelocity(m,d,mujoco.mjtObj.mjOBJ_BODY,head,velocity,1)
        acceleration=np.zeros(6);mujoco.mj_objectAcceleration(m,d,mujoco.mjtObj.mjOBJ_BODY,head,acceleration,1)
        values.extend(np.clip(.5+velocity[:3]/80,0,1))
        values.extend(np.clip(.5+(acceleration[3:]-rotation.T@m.opt.gravity)/80,0,1))
        values.extend(np.clip(.5+(rotation.T@np.array([0,0,-1]))/2,0,1))
        values.extend([float('beak_gape_geom' in contacts)]*2)
        for side in ('left','right'):
            values.extend(float(f'{side}_toe{i}' in contacts) for i in (1,2,3))
            values.append(float(side+'_leg_hallux_curl_geom' in contacts))
        values.extend(float(side+'_wing_wrist_twist_geom' in contacts) for side in ('left','right'))
        # Reduced self-vocalisation is an explicit one-band engineering proxy,
        # not a recorded crow call. Other enclosure sound bands are quiet.
        values.extend([float(self.process_rates[0]) if i%8==2 else 0. for i in range(16)])
        body_velocity=np.zeros(6);mujoco.mj_objectVelocity(m,d,mujoco.mjtObj.mjOBJ_BODY,1,body_velocity,1)
        values.extend(np.clip(.5+body_velocity[3:]/40,0,1))
        values.extend([1-self.energy_J/20000.,1-self.water_kg/.01,1-self.energy_J/20000.,self.energy_J/20000.])
        values.extend([0.,0.]) # No food/water objects admitted in this mechanics scene.
        if len(values)!=492: raise ValueError('crow IO implementation mismatch')
        return [float(x) for x in values]

    def step(self):
        if self.paused: return
        if self.client:
            if self.client.fault: self.disarm()
            reply=self.client.receive()
            if reply:
                try: self.accept_reference(reply)
                except ValueError as error:
                    self.disarm();self.output_rejections+=1;self.last_output_rejection=str(error)
                    if str(error) not in ('late/future reference output','locally disarmed'):
                        self.client.fault='reference output rejected: '+str(error);self.client.stop_event.set()
        self._drives(); self.data.qfrc_applied[:]=0
        if self.panels: apply_panels(self.model,self.data,self.panels,self.wind)
        if self.panels and not self.disarmed:
            # A command-side mechanical power budget at the current joint
            # velocities; actual integrated work is charged after mj_step.
            predicted=sum(abs(self.data.ctrl[a]*self.model.actuator_gear[a,0]*self.data.qvel[v]) for a,v in zip(self.actuator_ids,self.qvel_ids))
            if predicted>25.:self.data.ctrl[:]*=25./predicted
            self.data.ctrl[:]*=min(1.,self.energy_J/2000.)
        mujoco.mj_step(self.model,self.data); self.step_index+=1
        power=float(np.sum(np.abs(self.data.actuator_force*self.data.actuator_velocity)))
        # Respiration is a reduced physiological process; no extra joint or
        # root force is fabricated. Ventilation modifies effort efficiency.
        efficiency=.20+.05*float(self.process_rates[1]) if self.panels else .25
        self.energy_J=max(0.,self.energy_J-(power/efficiency+1.+2.*float(self.process_rates[0]))*self.model.opt.timestep)
        if self.energy_J==0:self.disarm()
        if self.client and self.client.ready.is_set():
            self.client.submit(self.step_index,self.step_index*2000,self.sensors())

    def snapshot(self):
        spec=mujoco.mjtState.mjSTATE_INTEGRATION
        state=np.empty(mujoco.mj_stateSize(self.model,spec));mujoco.mj_getState(self.model,self.data,state,spec)
        return dict(schema_version=1,model_digest=self.record['digest'],io_digest=self.record['io']['digest'],
                    instance_id=self.instance_id,brain_id=self.brain_id,reset_epoch=self.reset_epoch,step=self.step_index,
                    physics=state.tolist(),rate=self.rate.tolist(),process_rates=self.process_rates.tolist(),retina=self.previous_retina.tolist(),energy_J=self.energy_J,
                    food_kg=self.food_kg,water_kg=self.water_kg,wind=self.wind.tolist(),
                    obstacles=self.obstacles,world_config_epoch=self.world_config_epoch,
                    scope='local physics; remote brain and in-flight neural exchange excluded')

    def restore(self,snapshot):
        if snapshot['schema_version']!=1 or any(snapshot[k]!=v for k,v in [('model_digest',self.record['digest']),('io_digest',self.record['io']['digest']),('instance_id',self.instance_id),('brain_id',self.brain_id)]):
            raise ValueError('snapshot model/IO/identity mismatch')
        spec=mujoco.mjtState.mjSTATE_INTEGRATION
        state=np.asarray(snapshot['physics'],dtype=float)
        if state.shape!=(mujoco.mj_stateSize(self.model,spec),) or not np.all(np.isfinite(state)): raise ValueError('invalid physics state')
        self.reset();self.reset_epoch=max(self.reset_epoch,snapshot['reset_epoch']+1)
        mujoco.mj_setState(self.model,self.data,state,spec)
        self.step_index=snapshot['step'];self.previous_retina[:]=snapshot['retina']
        self.obstacles=dict(snapshot.get('obstacles',{}));self.world_config_epoch=max(self.world_config_epoch,snapshot.get('world_config_epoch',0)+1)
        self.energy_J=snapshot['energy_J'];self.food_kg=snapshot['food_kg'];self.water_kg=snapshot['water_kg'];self.wind[:]=snapshot['wind']
        self.disarm();mujoco.mj_forward(self.model,self.data)
        # A disconnected restore stays disarmed; saved active output is never replayed.

    def place_obstacle(self,slot,position):
        if not isinstance(slot,int) or not 0<=slot<32:raise ValueError('voxel slot budget')
        point=np.asarray(position,dtype=float)
        if point.shape!=(3,) or not np.all(np.isfinite(point)) or np.any(np.abs(point)>10):raise ValueError('invalid voxel position')
        bid=mujoco.mj_name2id(self.model,mujoco.mjtObj.mjOBJ_BODY,f'world_voxel_{slot}')
        if bid<0:raise ValueError('this world has no collision-mirror capability')
        # Reject placements overlapping the current robot bounding sphere.
        if np.linalg.norm(point-self.data.xpos[1])<.9:raise ValueError('obstacle overlaps body safety envelope')
        self.data.mocap_pos[self.model.body_mocapid[bid]]=point
        self.obstacles[str(slot)]=point.tolist();self.world_config_epoch+=1
        mujoco.mj_forward(self.model,self.data)

    def poses(self):
        return dict(schema_version=1,world_id=self.instance_id,physics_owner='mujoco',profile_id=self.record['profile_id'],
                    model_digest=self.record['digest'],io_digest=self.record['io']['digest'],brain_id=self.brain_id,
                    reset_epoch=self.reset_epoch,step=self.step_index,time_us=self.step_index*2000,output_rejections=self.output_rejections,last_output_rejection=self.last_output_rejection,
                    disarmed=self.disarmed,energy_J=self.energy_J,syrinx_drive=float(self.process_rates[0]),respiratory_drive=float(self.process_rates[1]),
                    poses=[dict(name=b['name'],position=self.data.xpos[i+1].tolist(),quaternion_wxyz=self.data.xquat[i+1].tolist()) for i,b in enumerate(self.record['bodies'])])
