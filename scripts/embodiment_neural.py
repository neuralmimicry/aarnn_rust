# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Versioned named legacy transducers, derived from preserved Webots mappings.

These describe engineering transfers, not calibrated biology or pretrained skills.
Shared descriptors keep channel identity separate from native measurement code.
"""
import math
import re
from pathlib import Path
from embodiment_legacy import projection, ROOT

PROFILES=('hexapod','drosophila_banc','drosophila_fafb','nao','zebrafish')
VERSION='legacy-native-projections-v1'


def motor_facts(profile):
    proto={'hexapod':'HexapodRobot','drosophila_banc':'DrosophilaBancRobot','drosophila_fafb':'DrosophilaFafbRobot','zebrafish':'ZebrafishRobot'}.get(profile)
    if not proto:return {}
    text=(ROOT/f'webots_world/protos/{proto}.proto').read_text()
    result={}
    for block in re.findall(r'RotationalMotor\s*\{([^{}]*)\}',text):
        name=re.search(r'name\s+"([^"]+)"',block)
        if not name:continue
        values={k:float(v) for k,v in re.findall(r'(minPosition|maxPosition|maxTorque|maxVelocity)\s+([-+\d.eE]+)',block)}
        if 'minPosition' in values and 'maxPosition' in values:result[name[1]]=values
    return result


# NAO V6 (Aldebaran) OEM mechanics. Joint ranges (rad) and motor type with
# its reduction ratio, from Aldebaran's NAO V6 joint and motor tables; joint
# torque = motor stall torque x ratio, joint speed = motor no-load speed /
# ratio. Motors: (stall N m, no-load rpm). RHipYawPitch and LHipYawPitch are
# one physical motor (coupled by an equality). Phalanges share the hand motor.
NAO_MOTORS={1:(.065,8700.),2:(.0094,8400.),3:(.0143,10700.),4:(.0224,11400.),5:(.065,8700.)}
NAO_JOINTS={
    'HeadYaw':(-2.0857,2.0857,3,150.27),'HeadPitch':(-.6720,.5149,3,173.22),
    'LShoulderPitch':(-2.0857,2.0857,4,150.27),'RShoulderPitch':(-2.0857,2.0857,4,150.27),
    'LShoulderRoll':(-.3142,1.3265,3,173.22),'RShoulderRoll':(-1.3265,.3142,3,173.22),
    'LElbowYaw':(-2.0857,2.0857,3,150.27),'RElbowYaw':(-2.0857,2.0857,3,150.27),
    'LElbowRoll':(-1.5446,-.0349,3,173.22),'RElbowRoll':(.0349,1.5446,3,173.22),
    'LWristYaw':(-1.8238,1.8238,2,50.61),'RWristYaw':(-1.8238,1.8238,2,50.61),
    'LHipYawPitch':(-1.145303,.740810,1,201.3),'RHipYawPitch':(-1.145303,.740810,1,201.3),
    'LHipRoll':(-.379472,.790477,1,201.3),'RHipRoll':(-.790477,.379472,1,201.3),
    'LHipPitch':(-1.535889,.484090,5,130.85),'RHipPitch':(-1.535889,.484090,5,130.85),
    'LKneePitch':(-.092346,2.112528,5,130.85),'RKneePitch':(-.103083,2.120198,5,130.85),
    'LAnklePitch':(-1.189516,.922747,5,130.85),'RAnklePitch':(-1.186448,.932056,5,130.85),
    'LAnkleRoll':(-.397880,.769001,1,201.3),'RAnkleRoll':(-.768992,.397935,1,201.3),
}
# Position-servo stiffness: full stall torque at 0.1 rad error (engineering
# choice standing in for the motor boards' position loop at full stiffness).
NAO_STALL_ERROR_RAD=.1
# Rotor inertia reflected through the gearbox (rotor ~1e-7 kg m2 x ratio^2):
# an engineering estimate, not an OEM figure.
NAO_ROTOR_KG_M2=1e-7


def nao_joint(name):
    """(lo, hi, torque N m, speed rad/s, back-EMF damping N m s, armature)."""
    import math
    lo,hi,motor,ratio=NAO_JOINTS.get(name,(0.,1.,2,36.24))
    stall,rpm=NAO_MOTORS[motor]
    if 'Phalanx' in name:stall/=8   # eight phalanges share the hand motor
    torque=stall*ratio;speed=rpm/ratio*2*math.pi/60
    return lo,hi,torque,speed,torque/speed,NAO_ROTOR_KG_M2*ratio*ratio


def install_mechanics(profile,root,element):
    if profile not in PROFILES:return
    facts=motor_facts(profile)
    # Reuse named physical servos, with declared engineering gains and the
    # existing Webots range/torque. No unpowered position-holding is enabled.
    for a in list(root.find('actuator')):
        joint=a.get('joint');j=root.find(f'.//joint[@name="{joint}"]')
        key=joint
        if profile.startswith('drosophila'):
            side='left' if joint[0]=='l' else 'right';row={'f':'front','m':'mid','r':'rear'}[joint[1]]
            key=f'leg_{side}_{row}_{joint.split("_",1)[1]}'
        if profile=='zebrafish':
            if joint.startswith('tail_joint_'):
                i=int(joint.rsplit('_',1)[1]);key=f'zebrafish_o_{2*i:02}_tail_l{i}'
            else:key={'left_pectoral_pitch':'zebrafish_o_16_pec_fin_l_pitch','left_pectoral_roll':'zebrafish_o_17_pec_fin_l_roll','right_pectoral_pitch':'zebrafish_o_18_pec_fin_r_pitch','right_pectoral_roll':'zebrafish_o_19_pec_fin_r_roll'}.get(joint,joint)
        f=facts.get(key)
        bounds=[f['minPosition'],f['maxPosition']] if f else [-1.2,1.2]
        # NAO uses original primitive hardware surrogate; native revision/CAD
        # is not licensed here. Its joint envelopes are explicit assumptions.
        if profile=='nao':
            # NAO v2: OEM ranges, torques, speeds and back-EMF damping; the
            # OEM zero pose is the built pose (ref 0: legs straight, arms
            # forward), so joint angles read as on the robot.
            lo,hi,effort,speed,damping,armature=nao_joint(joint)
            bounds=[lo,hi]
            j.set('range',' '.join(map(str,bounds)));j.set('ref','0')
            j.set('damping',str(damping));j.set('armature',str(armature))
            a.tag='position';a.attrib.clear();a.set('name',joint);a.set('joint',joint)
            kp=effort/NAO_STALL_ERROR_RAD
            for k,v in dict(ctrlrange=bounds,kp=kp,kv=0.,forcerange=[-effort,effort]).items():a.set(k,' '.join(map(str,v)) if isinstance(v,list) else str(v))
            a.set('user',str(speed))
            continue
        j.set('range',' '.join(map(str,bounds)));j.set('ref',str(sum(bounds)/2))
        a.tag='position';a.attrib.clear();a.set('name',joint);a.set('joint',joint)
        effort=f.get('maxTorque',1.) if f else .00005 if profile=='zebrafish' else .02 if profile.startswith('drosophila') else 1.
        # Fly legs v2: kp .03 (v1 .01 sagged ~0.08 rad under body weight and a
        # programmed tripod walked at half the speed); native torque limits kept.
        kp=.03 if profile.startswith('drosophila') else .002 if profile=='zebrafish' else .5 if profile=='hexapod' else 1.
        for k,v in dict(ctrlrange=bounds,kp=kp,kv=kp*.05,forcerange=[-effort,effort]).items():a.set(k,' '.join(map(str,v)) if isinstance(v,list) else str(v))
        a.set('user',str(f.get('maxVelocity',4.) if f else 4.))
    size=root.find('size')
    if size is None:element(root,'size',nuser_actuator='1')
    else:size.set('nuser_actuator','1')
    # Native cameras measure this world's material/ray luminance. They do not
    # claim rendered RGB-camera parity with Webots.
    base=root.find('.//body[@name="'+('chassis' if profile=='hexapod' else 'thorax' if profile.startswith('drosophila') else 'torso' if profile=='nao' else 'segment_00')+'"]')
    cameras=[('hex_s_26_head_camera',[.165,0,.05],0,1.117)] if profile=='hexapod' else [(f'dros_s_{34+i:02}_eye_{side}',[.028,sign*.018,.008],sign*.65,2.4) for i,(side,sign) in enumerate([('left',1),('right',-1)])] if profile.startswith('drosophila') else [('CameraBottom',[.0488,0,.1496],0,1.06),('CameraTop',[.0539,0,.1944],0,1.06)] if profile=='nao' else [('zebrafish_s_16_eye_left',[.005,.008,.002],.7,2.85),('zebrafish_s_18_eye_right',[.005,-.008,.002],-.7,2.85)]
    for name,pos,yaw,fov in cameras:
        element(base,'camera',name=name,pos=pos,xyaxes=[math.sin(yaw),-math.cos(yaw),0,0,0,1],fovy=min(140,fov*180/math.pi),resolution='32 24')


def sensor(name,kind,body,**kwargs):
    return dict(name=name,kind='legacy-'+kind,body=body,units='normalised',range=[0,1],rate_hz=500,missing='fault-disarm',**kwargs)


def sensors(profile):
    names,_=projection(profile);body='chassis' if profile=='hexapod' else 'thorax' if profile.startswith('drosophila') else 'torso' if profile=='nao' else 'segment_00'
    out=[]
    for i,n in enumerate(names):
        if profile=='hexapod' and i<18:
            out.append(sensor(n,'angle',body,joint=n.replace('hex_s_'+f'{i:02}',f'hex_o_{i:03}')));continue
        if profile=='hexapod' and i<24:
            leg=['lf','lm','lr','rf','rm','rr'][i-18];out.append(sensor(n,'contact',f'hex_o_{(i-18)*3+2:03}_{leg}_tibia_link'));continue
        if profile=='nao' and n.endswith('S'):
            out.append(sensor(n,'angle',body,joint=n[:-1]));continue
        if 'accel' in n or 'gyro' in n:
            out.append(sensor(n,'accel' if 'accel' in n else 'gyro',body,axis='xyz'.index(n[-1]),mode='legacy-y-up' if profile.startswith('drosophila') or profile=='zebrafish' else 'native-z-up'));continue
        if '.on.r' in n or '.off.r' in n or (profile=='zebrafish' and 16<=i<20):
            if profile=='zebrafish':
                camera='zebrafish_s_16_eye_left' if i<18 else 'zebrafish_s_18_eye_right';cell=[0,0];shape=[1,1];mode='lum' if i%2==0 else 'gradient'
            else:
                camera=n.split('.')[0];m=re.search(r'\.(on|off)\.r(\d+)c(\d+)',n);cell=[int(m[2]),int(m[3])];shape=[8,12] if profile.startswith('drosophila') else [6,8] if profile=='nao' else [1,1];mode=m[1]
            out.append(sensor(n,'camera',body,camera=camera,cell=cell,retina_shape=shape,mode=mode));continue
        if profile=='nao' and ('Foot/Bumper' in n or 'Fsr.' in n):
            side=n[0];kwargs={} if 'Fsr.' in n else dict(direction=[0,1 if n.endswith('Left') else -1,0])
            out.append(sensor(n,'contact-force' if 'Fsr.' in n else 'contact',side+'AnkleRoll_link',axis='xyz'.index(n[-1]) if 'Fsr.' in n else 0,**kwargs));continue
        if profile.startswith('drosophila') and 'touch' in n:
            direction=[1,0,0] if n.endswith('front') else [-1,0,0] if n.endswith('rear') else [0,1,0] if n.endswith('left') else [0,-1,0]
            out.append(sensor(n,'contact',body,direction=direction));continue
        if profile=='zebrafish' and i<16:
            out.append(sensor(n,'flow',f'segment_{1+i%8:02}',axis=1,mode='left' if i<8 else 'right'));continue
        if profile=='zebrafish' and i in (24,25,26,27):
            out.append(sensor(n,{24:'flow',25:'flow',26:'depth',27:'pitch'}[i],body if i!=25 else 'segment_08',axis=0));continue
        if profile.startswith('drosophila') and i==VENTRAL_RAY:
            # Flight v3: the rear ray of the ring looks down (ventral range,
            # a proxy for the ventral optic flow and ocelli that flies use to
            # hold height). Name and order are unchanged; the walking circuit
            # never used it (it reads forward rays only).
            out.append(sensor(n,'range',body,position_m=[0,0,0],direction=[0,0,-1],angle_rad=VENTRAL_REACH_M));continue
        # NAO's sonars are 'Sonar/Left'/'Sonar/Right' (capitalised): v1 matched
        # neither suffix and aimed both straight ahead. The chest transducers
        # look ~20 deg to each side (one ray each, not the OEM 60 deg cone).
        lname=n.lower()
        angle=(2*math.pi*(i-10)/24 if profile.startswith('drosophila') else NAO_SONAR_RAD*(1 if lname.endswith('left') else -1) if profile=='nao' and 'sonar' in lname
               else .2 if n.endswith(('left','_l')) else -.2 if n.endswith(('right','_r')) else math.pi if 'rear' in n else 0)
        reach=1.2 if profile=='hexapod' else 2.55 if profile=='nao' else .08 if profile.startswith('drosophila') else .3
        pos=[.165,.024 if n.endswith('left') else -.024,.05] if profile=='hexapod' else [.05,0,.1] if profile=='nao' else [0,0,0]
        kwargs={};mount=body
        if profile=='zebrafish':
            # Rays start outside the fish's own body (v1 started inside the head
            # and every ray hit the jaw link: constant 0.99): forward rays at
            # the snout tip, the rear ray behind the caudal fin.
            mount,pos=('segment_08',[-.022,0,0]) if 'rear' in n else ('segment_00',[.0068,0,0])
        if profile=='hexapod':kwargs['lookup']=[[0,1000],[.4,620],[.8,280],[1.2,120]]
        out.append(sensor(n,'range',mount,position_m=pos,direction=[math.cos(angle),math.sin(angle),0],angle_rad=reach,**kwargs))
    return out


# NAO sonar aim (rad either side of forward).
NAO_SONAR_RAD=.35


# Drosophila flight v3: sensor 22 (dros_s_22_prox_12, formerly the rear ray)
# is a ventral range ray reaching 0.3 m.
VENTRAL_RAY=22
VENTRAL_REACH_M=.3
FLIGHT_DECODE_TAU_S=.008


def flight_commands(profile):
    """Readouts of the fly's resonant-flight-v1 commands (power,
    amplitude_left/right, centre_left/right), in that order.

    The 48 imported output IDs all carry the leg antagonist pairs below; the
    flight commands share channels the designed walking circuit leaves
    silent (tibia/tarsus agonists, which it holds at mid-range), so walking
    never recruits flight and driving flight only flexes those distal joints
    of the (tucked) legs. Mirrored channels keep left/right symmetric. The
    IO contract (418 sensors, 48 outputs, names, order) is unchanged; wing
    flap servos and their legacy seed pairs (3, 17)/(11, 29) are retired.
    """
    _,outputs=projection(profile)
    def agonist(leg,kind):return ['lf','lm','lr','rf','rm','rr'].index(leg)*8+['coxa','femur','tibia','tarsus'].index(kind)*2
    def weights(items):return [dict(channel=i,coefficient=c) for i,c in items]
    rows=[('power',[(agonist('lm','tibia'),.5),(agonist('rm','tibia'),.5)]),
          ('amplitude_left',[(agonist('lm','tarsus'),1.)]),('amplitude_right',[(agonist('rm','tarsus'),1.)]),
          # Stroke centre: + moves the stroke backward (front-leg agonist), - forward (hind-leg agonist).
          ('centre_left',[(agonist('lf','tarsus'),1.),(agonist('lr','tarsus'),-1.)]),
          ('centre_right',[(agonist('rf','tarsus'),1.),(agonist('rr','tarsus'),-1.)])]
    assert len(outputs)==48
    # 8 ms decode (legs: 20 ms): steering muscles answer within a few ms and
    # the flight attitude loop has little phase margin to spare.
    return [dict(name=n,readout=weights(r),decode_tau_s=FLIGHT_DECODE_TAU_S) for n,r in rows]


def readout(profile,joint):
    _,outputs=projection(profile)
    def weights(items):return [dict(channel=i,coefficient=c) for i,c in items]
    if profile in ('hexapod','nao'):return weights([(outputs.index(joint),1.)])
    if profile.startswith('drosophila'):
        leg=['lf','lm','lr','rf','rm','rr'].index(joint[:2]);kind=['coxa','femur','tibia','tarsus'].index(joint.split('_')[1]);a=leg*8+kind*2;b=a+1
        # Explicit proposed antagonistic leg groups cover all 48 imported IDs.
        # This is an engineering projection, not measured descending-neuron
        # innervation. Wings have no servo: the flight motor (record
        # ``flight``, flight_commands) beats them at the thoracic resonance.
        return weights([(a,.62),(b,-.38)])
    if joint.startswith('tail_joint_'):
        i=int(joint.rsplit('_',1)[1]);return weights([(2*i,1.),(2*i+1,-1.)])
    names={'left_pectoral_pitch':16,'left_pectoral_roll':17,'right_pectoral_pitch':18,'right_pectoral_roll':19}
    if joint in ('dorsal_fin_joint','caudal_fin_joint','pelvic_fin_joint','pelvic2_fin_joint'):
        i=20+2*['dorsal_fin_joint','caudal_fin_joint','pelvic_fin_joint','pelvic2_fin_joint'].index(joint)
        return weights([(i,1.),(i+1,-1.)])
    return weights([(names[joint],1.)]) if joint in names else weights([(outputs.index(joint),1.)])


def neural_names(profile):
    import json
    if profile=='nao':return projection(profile)
    d=json.loads((ROOT/f'webots_world/configs/config_{profile}_webots.io_alignment.json').read_text())
    def names(rows):return [r if isinstance(r,str) else r['connectome_node_id'] for r in rows]
    return names(d['sensory_channels']),names(d.get('actuator_channels',d.get('output_channels')))
