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
        if profile=='hexapod':
            from embodiment_legacy import HEX_FREENOVE,HEX_SERVO
            if HEX_FREENOVE:
                # Freenove hexapod v3: MG996R-class position servos, 0-180 deg
                # about the 90 deg centre (the built calibration pose, ref 0);
                # full torque at 0.15 rad error (a hobby servo's proportional
                # band), little damping, the servo's speed limit.
                r=HEX_SERVO['range'];bounds=[-r,r];effort=HEX_SERVO['effort']
                j.set('range',' '.join(map(str,bounds)));j.set('ref','0')
                j.set('damping','0.02');j.set('armature','0.0005')
                a.tag='position';a.attrib.clear();a.set('name',joint);a.set('joint',joint)
                kp=effort/HEX_SERVO['stall_rad']
                for k,v in dict(ctrlrange=bounds,kp=kp,kv=kp*.02,forcerange=[-effort,effort]).items():
                    a.set(k,' '.join(map(str,v)) if isinstance(v,list) else str(v))
                a.set('user',str(HEX_SERVO['speed']))
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
    if profile.startswith('drosophila'):
        # Compound eyes on the head (anatomy v4); same lateral aim and field.
        from embodiment_legacy import FLY_EYES
        base=root.find('.//body[@name="head"]')
    if profile=='hexapod':
        from embodiment_legacy import HEX_FREENOVE,HEX_HEAD
    # Freenove head camera: Raspberry Pi camera (OV5647, ~54 x 41 deg).
    cameras=[('hex_s_26_head_camera',list(HEX_HEAD['camera']) if HEX_FREENOVE else [.165,0,.05],0,math.radians(41.4) if HEX_FREENOVE else 1.117)] if profile=='hexapod' else [(f'dros_s_{34+i:02}_eye_{side}',list(FLY_EYES[i]),sign*.65,2.4) for i,(side,sign) in enumerate([('left',1),('right',-1)])] if profile.startswith('drosophila') else [('CameraBottom',[.0488,0,.1496],0,1.06),('CameraTop',[.0539,0,.1944],0,1.06)] if profile=='nao' else [('zebrafish_s_16_eye_left',[.005,.008,.002],.7,2.85),('zebrafish_s_18_eye_right',[.005,-.008,.002],-.7,2.85)]
    if profile=='nao':
        # NAO V6 cameras sit in the head and move with HeadYaw/HeadPitch (v1
        # hung them level on the torso). Poses: NAO V50 URDF / Webots V5
        # (Head frame = HeadPitch_link); the bottom camera looks 39.7 deg down
        # at the feet. Optics: OV5640, 56.3 x 43.7 deg (4:3 retina).
        base=root.find('.//body[@name="HeadPitch_link"]');cameras=[]
        for name,pos,pitch in NAO_CAMERAS:
            element(base,'camera',name=name,pos=pos,xyaxes=[0,-1,0,math.sin(pitch),0,math.cos(pitch)],fovy=NAO_CAMERA_VFOV_DEG,resolution='32 24')
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
            leg=['lf','lm','lr','rf','rm','rr'][i-18]
            # Foot contact (simulated; the kit has no foot switch): the tibia tip.
            out.append(sensor(n,'contact',f'hex_o_{(i-18)*3+2:03}_{leg}_tibia_link',region_m=[[-.02,-.02,-.125],[.02,.02,-.095]]));continue
        if profile=='nao' and n.endswith('S'):
            out.append(sensor(n,'angle',body,joint=n[:-1]));continue
        if 'accel' in n or 'gyro' in n:
            out.append(sensor(n,'accel' if 'accel' in n else 'gyro',body,axis='xyz'.index(n[-1]),mode='legacy-y-up' if profile.startswith('drosophila') or profile=='zebrafish' else 'native-z-up'));continue
        if '.on.r' in n or '.off.r' in n or (profile=='zebrafish' and 16<=i<20):
            if profile=='zebrafish':
                camera='zebrafish_s_16_eye_left' if i<18 else 'zebrafish_s_18_eye_right';cell=[0,0];shape=[1,1];mode='lum' if i%2==0 else 'gradient'
            else:
                camera=n.split('.')[0];m=re.search(r'\.(on|off)\.r(\d+)c(\d+)',n);cell=[int(m[2]),int(m[3])];shape=[8,12] if profile.startswith('drosophila') else [6,8] if profile=='nao' else [1,1];mode=m[1]
            out.append(sensor(n,'camera','HeadPitch_link' if profile=='nao' else 'head' if profile.startswith('drosophila') else body,camera=camera,cell=cell,retina_shape=shape,mode=mode));continue
        if profile=='nao' and ('Foot/Bumper' in n or 'Fsr.' in n):
            side=n[0];kwargs={} if 'Fsr.' in n else dict(region_m=nao_bumper_region(side,n.endswith('Left')))
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
        if profile=='hexapod' and 'ultrasonic' in lname:
            # HC-SR04 on the Freenove head: two transducers 26 mm apart, a ~15
            # deg beam (cone sampled by rays), sonar specular loss, 2 cm to the
            # 3 m the kit's software reads; reading 1 - distance / reach.
            from embodiment_legacy import HEX_HEAD
            sign=1 if lname.endswith('left') else -1
            x,y,z=HEX_HEAD['sonar']
            out.append(sensor(n,'range',body,position_m=[x,sign*y,z],direction=[1.,0.,0.],angle_rad=3.0,
                              rays=cone_rays([1.,0.,0.],math.radians(15.),rings=2),incidence_max_rad=round(math.pi/8,6),
                              min_m=.02,period_s=.06));continue
        if profile=='nao' and 'sonar' in lname:
            sign=1 if lname.endswith('left') else -1
            out.append(sensor(n,'range',body,position_m=[NAO_SONAR_POS[0],sign*NAO_SONAR_POS[1],NAO_SONAR_POS[2]],
                              direction=nao_sonar_axis(sign),angle_rad=NAO_SONAR_REACH_M,rays=[[x,sign*y,z] for x,y,z in cone_rays(nao_sonar_axis(1),NAO_SONAR_APERTURE_RAD)],
                              incidence_max_rad=round(math.pi/8,6),min_m=NAO_SONAR_MIN_M,period_s=NAO_SONAR_PERIOD_S));continue
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
    if profile=='nao' and NAO_HEARING:
        out+=nao_hearing()
    if profile=='zebrafish' and ZEBRAFISH_RETINA:
        out+=zebrafish_retina()
    if profile.startswith('drosophila') and FLY_HEARING:
        out+=fly_hearing()
    if profile=='zebrafish' and ZEBRAFISH_HEARING:
        out+=zebrafish_hearing()
    return out


# Drosophila hearing (optional group "hearing"): the Johnston's organ in each
# antenna's second segment senses the near-field particle velocity of sound
# (courtship sine song ~150 Hz, pulse song ~200-300 Hz; range ~100-800 Hz),
# strongly directional with the arista's rotation axis. Modelled as two
# directional receivers on the head at the antennae (catalogue position),
# aimed forward-outward, in three bands.
FLY_HEARING=True
FLY_ANTENNAE={'left':(.012,.0045,.0086),'right':(.012,-.0045,.0086)}
FLY_HEARING_BANDS_HZ=(150,300,600)


def fly_hearing():
    out=[]
    for side,pos in FLY_ANTENNAE.items():
        sign=1 if side=='left' else -1
        for band in FLY_HEARING_BANDS_HZ:
            out.append(sensor(f'dros_hear_johnston_{side}_{band}Hz','acoustic','head',position_m=list(pos),
                              direction=[.7071,sign*.7071,0],band_hz=float(band),directivity=.9,floor_db=20.,ceil_db=100.,group='hearing'))
    return out


# Zebrafish hearing (optional group "hearing"): the inner ears' otolith organs
# (saccule/utricle) sense low-frequency particle motion (~100-1000 Hz, best
# below ~400 Hz), one on each side of the hindbrain behind the eyes; nearly
# omnidirectional. The proxy uses the airborne free-field model (no water
# acoustics), so levels are relative.
ZEBRAFISH_HEARING=True
ZEBRAFISH_EARS={'left':(-.001,.003,0.),'right':(-.001,-.003,0.)}
ZEBRAFISH_HEARING_BANDS_HZ=(100,300)


def zebrafish_hearing():
    out=[]
    for side,pos in ZEBRAFISH_EARS.items():
        sign=1 if side=='left' else -1
        for band in ZEBRAFISH_HEARING_BANDS_HZ:
            out.append(sensor(f'zebrafish_hear_inner_ear_{side}_{band}Hz','acoustic','segment_00',position_m=list(pos),
                              direction=[0,sign,0],band_hz=float(band),directivity=.2,floor_db=20.,ceil_db=100.,group='hearing'))
    return out


# Zebrafish vision (optional group "vision"): v1 gave each eye one luminance
# and one gradient cell. Each lateral eye camera (163 deg wide, aimed 40 deg
# out) now also feeds a coarse retina of 6 x 8 receptive fields with ON and
# OFF channels (the transient luminance cells of aarnn_embodiment::vision),
# 96 per eye. Preserved networks bind their 32 core channels unchanged.
ZEBRAFISH_RETINA=True
ZEBRAFISH_RETINA_SHAPE=(6,8)


def zebrafish_retina():
    rows,cols=ZEBRAFISH_RETINA_SHAPE;out=[]
    for side,camera in (('left','zebrafish_s_16_eye_left'),('right','zebrafish_s_18_eye_right')):
        for r in range(rows):
            for c in range(cols):
                for mode in ('on','off'):
                    out.append(sensor(f'zebrafish_eye_{side}.{mode}.r{r}c{c}','camera','segment_00',camera=camera,cell=[r,c],
                                      retina_shape=[rows,cols],mode=mode,group='vision'))
    return out


# NAO V6 hearing (optional sensor group "hearing", appended after the
# 250-channel core so preserved networks bind unchanged): four omnidirectional
# head microphones (Aldebaran NAO6 docs: MicroFR/FL/RL/RR, 100 Hz-10 kHz;
# positions from the NAO V5 head chain, m, Head frame = HeadPitch_link), each
# read by a cochlea-like bank of four bands, plus the interaural time
# difference of the front and of the rear pair (1 = source on the left).
# Omnidirectional microphones on a 13 cm head still hear directionally: the
# head shadow is the receivers' directivity (axis out from the head centre).
NAO_MICS={'FL':(.0206,.0309,.0986),'FR':(.0206,-.0309,.0986),'RL':(-.0215,.0558,.0774),'RR':(-.0215,-.0558,.0774)}
NAO_HEAD_CENTRE=(0.,0.,.06)
NAO_HEARING_BANDS_HZ=(250,1000,3000,8000)
NAO_HEAD_SHADOW=.6
NAO_HEARING=True    # enabled after the 24 Nov V6 walking tests (their IO digest is kept in nao_learn_closedloop.V6_IO_DIGESTS)


def nao_hearing():
    out=[]
    def axis(p):
        d=[p[i]-NAO_HEAD_CENTRE[i] for i in range(3)];n=math.sqrt(sum(v*v for v in d))
        return [round(v/n,6) for v in d]
    for mic,pos in NAO_MICS.items():
        for band in NAO_HEARING_BANDS_HZ:
            out.append(sensor(f'Micro/{mic}/{band}Hz','acoustic','HeadPitch_link',position_m=list(pos),direction=axis(pos),
                              band_hz=float(band),directivity=NAO_HEAD_SHADOW,floor_db=20.,ceil_db=100.,group='hearing'))
    for pair,(right,left) in (('Front',('FR','FL')),('Rear',('RR','RL'))):
        pos=NAO_MICS[right]
        out.append(sensor(f'Micro/ITD/{pair}','acoustic','HeadPitch_link',position_m=list(pos),direction=axis(pos),band_hz=1000.,
                          directivity=NAO_HEAD_SHADOW,mode='itd',pair=f'Micro/{left}/1000Hz',group='hearing'))
    return out


# NAO sonar aim (rad either side of forward): v1 single-ray surrogate, kept
# for reference; the NAO V6 sonar below replaces it.
NAO_SONAR_RAD=.35

# NAO V6 sensors (Aldebaran NAO6 technical docs; poses from the NAO V50 URDF
# and the Webots R2025a Nao.proto, V5):
# - sonar: chest transducers at (0.0477, +-0.0416, 0.0509) m in the torso
#   frame, axis 25.8 deg out and 13.6 deg down (Webots US2/US4 rotation),
#   60 deg cone (aperture 1.04 rad), V6 range 0.20-0.80 m (below 0.20 m only
#   "an object is present"), specular loss above pi/8 incidence (Webots
#   "sonar" type);
# - cameras: Head frame, top (0.05871, 0, 0.06364) pitched 1.2 deg, bottom
#   (0.05071, 0, 0.01774) pitched 39.7 deg; vertical FOV 43.7 deg;
# - foot bumpers: two ON/OFF switches at each toe, centred at x 0.0876 m,
#   z -0.0353 m (10 mm above the sole at -0.0451 m) in the ankle frame,
#   left/right of the toe (y +0.019/-0.014 on the left foot, mirrored).
NAO_SONAR_POS=(0.0477,0.0416,0.0509)
NAO_SONAR_AXIS=(0.8754,0.4226,-0.2346)
NAO_SONAR_APERTURE_RAD=1.04
NAO_SONAR_REACH_M=0.8
NAO_SONAR_MIN_M=0.2
# Sample-and-hold: one ping every 20 ms (a 0.8 m echo returns in 4.7 ms; the
# transducer pings no faster), which keeps the 91-ray cone affordable.
NAO_SONAR_PERIOD_S=0.02
NAO_CAMERAS=[('CameraTop',[0.05871,0,0.06364],0.0209435),('CameraBottom',[0.05071,0,0.01774],0.692896)]
NAO_CAMERA_VFOV_DEG=43.7


def nao_sonar_axis(sign):
    x,y,z=NAO_SONAR_AXIS;n=math.sqrt(x*x+y*y+z*z)
    return [round(x/n,6),round(sign*y/n,6),round(z/n,6)]


def cone_rays(axis,aperture,rings=5):
    """Explicit, engine-independent rays sampling a transducer cone: the
    axis plus `rings` rings out to half the aperture, ring k carrying 6k rays
    so the spacing stays uniform (6 deg for the NAO's 60 deg cone). A beam
    is continuous: a ball always has a point facing back to the transducer,
    and with the pi/8 specular rule only rays within a few degrees of it
    echo, so sparse sampling (Webots' 6 rays) misses curved obstacles."""
    a=[float(v) for v in axis];n=math.sqrt(sum(v*v for v in a));a=[v/n for v in a]
    ref=[0.,0.,1.] if abs(a[2])<0.9 else [1.,0.,0.]
    u=[ref[1]*a[2]-ref[2]*a[1],ref[2]*a[0]-ref[0]*a[2],ref[0]*a[1]-ref[1]*a[0]];k=math.sqrt(sum(v*v for v in u));u=[v/k for v in u]
    w=[a[1]*u[2]-a[2]*u[1],a[2]*u[0]-a[0]*u[2],a[0]*u[1]-a[1]*u[0]]
    out=[[round(v,6) for v in a]]
    for ring in range(1,rings+1):
        t=aperture/2*ring/rings
        per_ring=6*ring
        for j in range(per_ring):
            phi=2*math.pi*(j+0.5*(ring%2))/per_ring
            out.append([round(math.cos(t)*a[i]+math.sin(t)*(math.cos(phi)*u[i]+math.sin(phi)*w[i]),6) for i in range(3)])
    return out


def nao_bumper_region(side,left):
    """Body-local box of one toe bumper switch (ankle frame). It spans the
    toe shell from 5 mm behind the switch to past the toe tip, from 4 mm
    above the sole to the top of the toe, on its side of the toe midline;
    standing on the floor never presses it."""
    mid=0.0025 if side=='L' else -0.0025
    y=[mid,0.07] if left else [-0.07,mid]
    return [[0.0826,y[0],-0.041],[0.13,y[1],-0.012]]


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
