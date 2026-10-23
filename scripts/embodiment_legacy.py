"""Additive primitive mechanics for the six existing neural projections.

These are engineering bodies in the existing illustrative metre scale. Named
neural projections are retained verbatim; unresolved biological readout maps
fail closed rather than being assigned to arbitrary joints. No old brain is
loaded, edited or resized by this compiler.
"""
import json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
LEGACY=('celegans','drosophila_banc','drosophila_fafb','hexapod','nao','zebrafish')


def projection(profile):
    if profile=='nao':
        data=json.loads((ROOT/'network_nao.json').read_text())['connectome_labels']
        return data['sensory_nodes'],data['output_nodes']
    data=json.loads((ROOT/f'webots_world/configs/config_{profile}_webots.io_alignment.json').read_text())
    def name(item):return item if isinstance(item,str) else item.get('actuator_name',item.get('device_port'))
    return [name(x) for x in data['sensory_channels']],[name(x) for x in data.get('actuator_channels',data.get('output_channels',[]))]


def build(profile,element,base_world):
    root,world=base_world(profile);actuators=element(root,'actuator')
    def part(parent,name,pos,size,mass,joint_name=None,axis=(0,1,0),limits=(-1.2,1.2)):
        body=element(parent,'body',name=name,pos=pos)
        if joint_name:
            element(body,'joint',name=joint_name,axis=axis,range=limits,damping=.02)
            if profile=='celegans':
                element(actuators,'position',name=joint_name,joint=joint_name,ctrlrange=limits,kp=.05,kv=.0015,forcerange=[-.006,.006])
            else: element(actuators,'motor',name=joint_name,joint=joint_name,ctrlrange=[-1,1],gear=.05)
        if profile=='celegans':
            import math
            index=int(name.rsplit('_',1)[1]);t=(index+.6)/24
            radius=.0135*(.22+.78*math.sin(math.pi*t)**.48)
            element(body,'geom',name=name+'_collision',type='capsule',fromto=[-.0048,0,0,.0048,0,0],size=radius,mass=mass,rgba=[.76,.63,.39,1])
            # Massless cuticle annuli and four body-wall quadrants follow each
            # actual spine link; they never drive or support the robot.
            for offset in (-.0035,.0035):
                element(body,'geom',name=f'{name}_annulus_{"a" if offset<0 else "b"}',type='ellipsoid',pos=[offset,0,0],size=[.00028,radius*1.007,radius*1.007],rgba=[.54,.43,.25,1],mass=0,contype=0,conaffinity=0,group=2)
            for q,(y,z) in enumerate(((1,1),(-1,1),(1,-1),(-1,-1))):
                if index==23 and q==2:continue # Canonical MVL24 is absent.
                element(body,'geom',name=f'{name}_muscle_quadrant_{q}',type='capsule',fromto=[-.004,y*radius*.52,z*radius*.52,.004,y*radius*.52,z*radius*.52],size=.0012,rgba=[.69,.32,.21,1],mass=0,contype=0,conaffinity=0,group=2)
            element(body,'geom',name=name+'_intestine',type='capsule',fromto=[-.006,0,0,.006,0,0],size=.0022,rgba=[.58,.46,.19,1],mass=0,contype=0,conaffinity=0,group=2)
            if index in (20,22):element(body,'geom',name=name+'_pharyngeal_bulb',type='ellipsoid',pos=[0,0,.0015],size=[.004,.0035,.0035],rgba=[.84,.77,.57,1],mass=0,contype=0,conaffinity=0,group=2)
        else:
            shape='capsule' if profile.startswith('drosophila') and any(k in name for k in ('coxa','femur','tibia','tarsus')) else 'ellipsoid' if profile.startswith('drosophila') or profile=='zebrafish' else 'box'
            if shape=='capsule':element(body,'geom',name=name+'_collision',type=shape,fromto=[0,-size[1]*.7,0,0,size[1]*.7,0],size=min(size[0],size[2]),mass=mass,rgba=[.38,.25,.12,1])
            else:element(body,'geom',name=name+'_collision',type=shape,size=size,mass=mass,rgba=[.4,.45,.3,1])
        return body
    if profile in ('celegans','zebrafish'):
        segments=24 if profile=='celegans' else 9
        length=.34 if profile=='celegans' else .08;radius=.017 if profile=='celegans' else .006
        base=part(world,'segment_00',[-length/2 if profile=='celegans' else length/2,0,.017 if profile=='celegans' else .3],[length/segments*.44,radius,radius],.0068 if profile=='celegans' else .0002)
        element(base,'freejoint',name='root');parent=base
        for i in range(1,segments):parent=part(parent,f'segment_{i:02}',[(1 if profile=='celegans' else -1)*length/segments,0,0],[length/segments*.44,radius,radius],.0068 if profile=='celegans' else .00015,
                                                               f'celegans_spine_{i+1:02}' if profile=='celegans' else f'tail_joint_{i-1}',axis=[0,0,1],limits=[-.38,.38])
        if profile=='zebrafish':
            for side in ('left','right'):
                fin=part(base,side+'_pectoral',[0,.01 if side=='left' else -.01,0],[.007,.008,.001],.00002,side+'_pectoral_pitch')
                part(fin,side+'_pectoral_roll',[0,0,0],[.005,.005,.001],.00001,side+'_pectoral_roll')
            for kind,where in [('dorsal',[0,0,.007]),('caudal',[-.008,0,0]),('pelvic',[0,.006,-.004]),('pelvic2',[0,-.006,-.004])]:
                part(parent if kind=='caudal' else base,kind+'_fin',where,[.004,.003,.0007],.000002,kind+'_fin_joint')
            for n in ('zebrafish_o_28_jaw','zebrafish_o_29_operculum_l','zebrafish_o_30_operculum_r'):
                part(base,n+'_link',[.004,.004 if n.endswith('_l') else -.004 if n.endswith('_r') else 0,0],[.002,.001,.001],.000001,n)
    elif profile=='nao':
        base=part(world,'torso',[0,0,.40],[.07,.09,.10],2.5);element(base,'freejoint',name='root')
        _,outputs=projection(profile)
        head=base
        for n in ('HeadYaw','HeadPitch'):head=part(head,n+'_link',[0,0,.12] if head is base else [0,0,0],[.04,.04,.04],.15,n,axis=[0,0,1] if n=='HeadYaw' else [0,1,0])
        for side,sign in [('L',1),('R',-1)]:
            leg=base
            for n in ('HipYawPitch','HipRoll','HipPitch','KneePitch','AnklePitch','AnkleRoll'):
                full=side+n;leg=part(leg,full+'_link',[0,sign*.05,-.08] if leg is base else [0,0,-.035],[.018,.018,.025],.12,full)
            arm=base
            for n in ('ShoulderPitch','ShoulderRoll','ElbowYaw','ElbowRoll','WristYaw'):
                full=side+n;arm=part(arm,full+'_link',[0,sign*.12,.04] if arm is base else [0,sign*.03,-.02],[.015,.02,.02],.06,full)
            for i in range(1,9):part(arm,side+f'Phalanx{i}_link',[.008*(i%4),sign*.02,.008*(i//4)],[.006,.008,.004],.002,side+f'Phalanx{i}')
        # Original primitive surrogate: no upstream NAO geometry/licence inferred.
    else:
        fly=profile.startswith('drosophila');size=[.025,.018,.014] if fly else [.135,.085,.018]
        base=part(world,'thorax' if fly else 'chassis',[0,0,.09 if fly else .19],size,.003 if fly else .25);element(base,'freejoint',name='root')
        for side,sign in [('l',1),('r',-1)]:
            for row,x in [('f',.7),('m',0),('r',-.7)]:
                parent=base
                for i,kind in enumerate(('coxa','femur','tibia','tarsus') if fly else ('coxa','femur','tibia')):
                    name=f'{side}{row}_{kind}'
                    # Hexapod names match the authoritative named output ordering.
                    if not fly:
                        _,outputs=projection(profile);name=next(n for n in outputs if n.endswith('_'+name))
                    scale=.03 if fly else .08
                    parent=part(parent,name+'_link',[x*size[0],sign*size[1],0] if i==0 else [0,sign*scale*.65,-scale*.35],
                                [scale*.15,scale*.35,scale*.15],.0003 if fly else .035,name,axis=[0,0,1] if i==0 else [1,0,0])
        if fly:
            for side,sign in [('left',1),('right',-1)]:part(base,side+'_wing',[0,sign*.025,.01],[.025,.025,.001],.0001,side+'_wing_flap',axis=[1,0,0])
    # Reduced primitive rigs overlap at articulation pivots. Explicitly disable
    # intra-body collisions; collisions against the environment remain active.
    for geom in base.findall('.//geom'):
        if geom.get('group')!='2':geom.set('contype','2');geom.set('conaffinity','1')
    from embodiment_neural import install_mechanics
    install_mechanics(profile,root,element)
    return root


def celegans_sensors():
    """Named, ideal simulation proxies from CelegansRobot.proto, Y-up -> Z-up.

    Body attachment follows the nearest segment; light/heat are declared scene
    irradiance proxies, not a measured chemical/thermal transducer.
    """
    names,_=projection('celegans')
    out=[]
    for i,name in enumerate(names):
        s=dict(name=name,kind='celegans-inertial-v1' if i<6 else 'celegans-contact-v1' if i<8 else 'celegans-irradiance-v1' if i<12 else 'celegans-range-v1',
               units='normalised',range=[0,1],rate_hz=500,missing='fault-disarm')
        if i<6:s.update(axis=i%3)
        elif i<8:s.update(body='segment_23' if i==6 else 'segment_00')
        elif i<12:
            x,y=(.172,.03 if i==8 else -.03) if i<10 else (.095,.04 if i==10 else -.04)
            s.update(body='segment_23' if i<10 else 'segment_19',position_m=[x-(-.17+(.34/24)*(23 if i<10 else 19)),y,.012 if i<10 else .016],axis=0 if i>=10 else 1)
        else:
            x,y,angle,reach=[(.182,0,0,.26),(.168,.03,.36,.23),(.168,-.03,-.36,.23),(.06,.046,1.2,.38),(.06,-.046,-1.2,.38),(-.074,.03,2.46,.36),(-.074,-.03,-2.46,.36),(.126,0,0,.92),(-.126,0,3.14159,.92),(.110,.078,.95,1.28),(.110,-.078,-.95,1.28),(-.168,0,3.14159,1.30)][i-12]
            segment=max(0,min(23,round((x+.17)/(.34/24))))
            s.update(body=f'segment_{segment:02}',position_m=[x-(-.17+segment*.34/24),y,0],angle_rad=angle,range=[0,reach])
        out.append(s)
    return out
