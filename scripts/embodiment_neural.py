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


def install_mechanics(profile,root,element):
    if profile not in PROFILES:return
    facts=motor_facts(profile)
    # Reuse named physical servos, with declared engineering gains and the
    # existing Webots range/torque. No unpowered position-holding is enabled.
    for a in list(root.find('actuator')):
        joint=a.get('joint');j=root.find(f'.//joint[@name="{joint}"]')
        key=joint
        if profile.startswith('drosophila'):
            if '_wing_flap' in joint:key='wing_'+joint.split('_')[0]+'_flap'
            else:
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
            bounds=[0,1] if 'Phalanx' in joint else [-2.08,2.08] if joint=='HeadYaw' else [-.67,.51] if joint=='HeadPitch' else [-1.2,1.2]
        j.set('range',' '.join(map(str,bounds)));j.set('ref',str(sum(bounds)/2))
        a.tag='position';a.attrib.clear();a.set('name',joint);a.set('joint',joint)
        effort=f.get('maxTorque',1.) if f else .00005 if profile=='zebrafish' else .02 if profile.startswith('drosophila') else 1.
        kp=.01 if profile.startswith('drosophila') else .002 if profile=='zebrafish' else .5 if profile=='hexapod' else 1.
        for k,v in dict(ctrlrange=bounds,kp=kp,kv=kp*.05,forcerange=[-effort,effort]).items():a.set(k,' '.join(map(str,v)) if isinstance(v,list) else str(v))
        a.set('user',str(f.get('maxVelocity',4.) if f else 4.))
    size=root.find('size')
    if size is None:element(root,'size',nuser_actuator='1')
    else:size.set('nuser_actuator','1')
    # Native cameras measure this world's material/ray luminance. They do not
    # claim rendered RGB-camera parity with Webots.
    base=root.find('.//body[@name="'+('chassis' if profile=='hexapod' else 'thorax' if profile.startswith('drosophila') else 'torso' if profile=='nao' else 'segment_00')+'"]')
    cameras=[('hex_s_26_head_camera',[.165,0,.05],0,1.117)] if profile=='hexapod' else [(f'dros_s_{34+i:02}_eye_{side}',[.028,sign*.018,.008],sign*.65,2.4) for i,(side,sign) in enumerate([('left',1),('right',-1)])] if profile.startswith('drosophila') else [('CameraBottom',[.045,0,.10],0,1.06),('CameraTop',[.045,0,.16],0,1.06)] if profile=='nao' else [('zebrafish_s_16_eye_left',[.005,.008,.002],.7,2.85),('zebrafish_s_18_eye_right',[.005,-.008,.002],-.7,2.85)]
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
        angle=2*math.pi*(i-10)/24 if profile.startswith('drosophila') else .2 if n.endswith(('left','_l')) else -.2 if n.endswith(('right','_r')) else math.pi if 'rear' in n else 0
        reach=1.2 if profile=='hexapod' else 2.55 if profile=='nao' else .08 if profile.startswith('drosophila') else .3
        pos=[.165,.024 if n.endswith('left') else -.024,.05] if profile=='hexapod' else [.05,0,.1] if profile=='nao' else [0,0,0]
        kwargs={}
        if profile=='hexapod':kwargs['lookup']=[[0,1000],[.4,620],[.8,280],[1.2,120]]
        out.append(sensor(n,'range',body,position_m=pos,direction=[math.cos(angle),math.sin(angle),0],angle_rad=reach,**kwargs))
    return out


def readout(profile,joint):
    _,outputs=projection(profile)
    def weights(items):return [dict(channel=i,coefficient=c) for i,c in items]
    if profile in ('hexapod','nao'):return weights([(outputs.index(joint),1.)])
    if profile.startswith('drosophila'):
        if '_wing_flap' in joint:a,b=(3,17) if joint.startswith('left') else (11,29)
        else:
            leg=['lf','lm','lr','rf','rm','rr'].index(joint[:2]);kind=['coxa','femur','tibia','tarsus'].index(joint.split('_')[1]);a=leg*8+kind*2;b=a+1
        # Explicit proposed antagonistic leg groups cover all 48 imported IDs;
        # wings retain the legacy bridge's seed pair identities. This is an
        # engineering projection, not measured descending-neuron innervation.
        # No oscillator/twitch or whole-body spike-derived lift is present.
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
