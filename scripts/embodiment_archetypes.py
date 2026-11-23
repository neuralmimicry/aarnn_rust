# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Original reduced mechanical archetypes; no OEM CAD is inferred.

Published envelopes come from the pinned research manifest. All internal
geometry, mass distribution, drives and joint limits are versioned assumptions.
"""
import math


WHEEL_NO_LOAD_RAD_S = 25.0
# UBTECH UGOT full-kit specification (model ERXI101): 8 servos, max torque
# approx. 13.0 kgf.cm, max speed >= 60 rpm; 4 gear motors, approx. 2.0 kgf.cm,
# >= 360 rpm. A DC actuator's speed limit is its back-EMF: joint damping =
# stall torque / no-load speed. v1 gave every UGOT joint the gear-motor torque.
UGOT_SERVO_NM = 13.0 * 0.0980665
UGOT_SERVO_RAD_S = 60 * 2 * 3.141592653589793 / 60
UGOT_MOTOR_NM = 2.0 * 0.0980665
UGOT_MOTOR_RAD_S = 360 * 2 * 3.141592653589793 / 60


def build(profile, description, element, base_world):
    root,world=base_world(profile)
    parameters=description['parameters']
    variant=description.get('selected_variant','original-v1')
    dims=parameters.get('body_dimensions_wdh',{}).get('value') or [.30,.30,.25]
    mass=parameters.get('mass',{}).get('value') or 2.5
    if profile=='xbot_s_pro':dims=[1.13,1.28,1.6];mass=400;variant='lec_brochure'
    if profile=='cofe_smart_bar':dims=[1.5,.8,1.3];mass=150;variant='original-bar-v1'
    if profile=='xbot_ic60':mass=300
    width,depth,height=dims
    actuator=element(root,'actuator')
    def geom(parent,name,shape,size,mass,pos=(0,0,0),**kw):
        return element(parent,'geom',name=name,type=shape,size=size,mass=mass,pos=pos,rgba=[.15,.35,.5,1],**kw)
    def joint(parent,name,pos,size,link_mass,axis=(0,1,0),limits=(-1.5,1.5),shape='box',effort=2,no_load=None,geom_pos=(0,0,0)):
        child=element(parent,'body',name=name+'_link',pos=pos)
        attrs=dict(name=name,axis=axis,type='hinge')
        if limits:attrs['range']=limits
        else:attrs['limited']='false'
        if no_load:
            attrs['damping']=effort/no_load
        elif name.startswith('wheel'):
            # Drive-v3: a DC wheel motor's speed limit is its back-EMF,
            # damping = stall torque / no-load speed (25 rad/s). The default
            # .05 N m s/rad held 0.1-0.2 N m UGOT wheels to 1-4 rad/s: those
            # bases crawled and a balancing base could not drive under its COM.
            attrs['damping']=effort/WHEEL_NO_LOAD_RAD_S
        element(child,'joint',**attrs)
        geom(child,name+'_collision',shape,size,link_mass,pos=geom_pos)
        element(actuator,'motor',name=name,joint=name,ctrlrange=[-1,1],gear=effort)
        return child
    def wheels(parent,side_x=(-1,1),wheel_mass=None,radius=None,ugot=False):
        radius=radius or min(width,depth)*.13
        wm=wheel_mass if wheel_mass is not None else mass*.02
        for x in side_x:
            for y in (-1,1):
                drive=dict(effort=UGOT_MOTOR_NM,no_load=UGOT_MOTOR_RAD_S) if ugot else dict(effort=mass*.04)
                wheel=joint(parent,f'wheel_{"front" if x>0 else "rear"}_{"left" if y>0 else "right"}',
                            [x*depth*.33,y*width*.43,0],[radius,radius*.24],wm,limits=None,shape='cylinder',**drive)
                wheel.find('geom').set('quat','0.707106781 0.707106781 0 0')
                if x>0 and len(side_x)==2:
                    # Drive-v2: the front pair rolls like casters (low contact
                    # friction; priority 1 so MuJoCo does not take the ground's
                    # 0.8 by maximum). Four fixed high-friction wheels could not
                    # skid-steer: half torque yawed a 35 kg base 0.003 rad in
                    # 0.6 s. Delivery robots steer differentially on casters.
                    wheel.find('geom').set('friction','.05 .005 .0001');wheel.find('geom').set('priority','1')
    def arm(parent,prefix,pos,n=6,arm_mass=1,servo=None):
        for i in range(n):
            parent=joint(parent,f'{prefix}_joint{i+1}',pos if i==0 else [.08,0,.05], [.05,.015,.02],arm_mass/n,
                         axis=[0,0,1] if i%3==0 else [0,1,0],**(servo or dict(effort=2)))
        for sign in (-1,1):
            finger=element(parent,'body',name=f'{prefix}_finger_{sign}',pos=[.06,sign*.025,0])
            name=f'{prefix}_grip_{sign}';element(finger,'joint',name=name,type='slide',axis=[0,sign,0],range=[-.015,.015])
            geom(finger,name+'_contact','box',[.03,.005,.015],.025)
            if sign==1:element(actuator,'motor',name=name,joint=name,ctrlrange=[-1,1],gear=5)
        eq=root.find('equality')
        if eq is None:eq=element(root,'equality')
        element(eq,'joint',joint1=prefix+'_grip_-1',joint2=prefix+'_grip_1',polycoef=[0,1,0,0,0])
    category=description['category']
    if profile.startswith('keenon_'):
        radius=min(width,depth)*.13
        base=element(world,'body',name='chassis',pos=[0,0,radius+.006]);element(base,'freejoint',name='root')
        geom(base,'base_collision','box',[depth*.42,width*.42,height*.075],mass*(.70 if profile in ('keenon_w3','keenon_c30','keenon_c40') else .72),pos=[0,0,height*.06])
        wheels(base)
        geom(base,'mast','box',[depth*.07,width*.07,height*.4],mass*.15,pos=[-depth*.3,0,height*.5])
        levels=3 if profile in ('keenon_t8','keenon_t11') else 4
        if profile in ('keenon_c30','keenon_c40'):levels=1
        for i in range(levels):
            geom(base,f'tray_{i}','box',[depth*.39,width*.40,.008],mass*.05/levels,pos=[0,0,height*(.25+.65*i/max(1,levels-1))])
        if profile=='keenon_w3':
            for i in range(2):
                joint(base,f'compartment_door_{i}',[depth*.4,-width*.4,height*(.35+i*.4)],[.008,width*.4,height*.15],mass*.01,axis=[0,0,1],limits=[0,1.5],effort=3)
        if profile in ('keenon_c30','keenon_c40'):
            joint(base,'cleaning_brush',[0,0,-radius+.012],[width*.22,.008],mass*.02,axis=[0,0,1],limits=None,shape='cylinder',effort=.5)
        # A separately massive contact payload, never glued/teleported to a tray.
        payload=element(world,'body',name='payload',pos=[0,0,radius+height*.25+.04]);element(payload,'freejoint',name='payload_free')
        geom(payload,'payload_contact','box',[.03,.03,.025],.5)
    elif profile.startswith('ugot_'):
        base=element(world,'body',name='controller_module',pos=[0,0,.20]);element(base,'freejoint',name='root')
        geom(base,'controller_collision','box',[.10,.065,.035],1.3)
        servo=dict(effort=UGOT_SERVO_NM,no_load=UGOT_SERVO_RAD_S)
        motor=dict(effort=UGOT_MOTOR_NM,no_load=UGOT_MOTOR_RAD_S)
        if profile in ('ugot_quadruped','ugot_spider'):
            # The kit's 8 servos give two per leg (v1 modelled three, all at
            # gear-motor torque). Dog: hip and knee pitch under the body;
            # spider: hip yaw at the corner and a knee that lifts the leg.
            for x in (-1,1):
                for y in (-1,1):
                    if profile=='ugot_quadruped':
                        hip=joint(base,f'leg_{x}_{y}_0',[x*.07,y*.07,-.02],[.012,.012,.03],.10,axis=[0,1,0],geom_pos=[0,0,-.03],**servo)
                        joint(hip,f'leg_{x}_{y}_1',[0,0,-.06],[.01,.01,.035],.10,axis=[0,1,0],geom_pos=[0,0,-.035],**servo)
                    else:
                        hip=joint(base,f'leg_{x}_{y}_0',[x*.07,y*.05,0],[.012,.03,.012],.10,axis=[0,0,1],geom_pos=[0,y*.03,0],**servo)
                        joint(hip,f'leg_{x}_{y}_1',[0,y*.06,0],[.012,.012,.04],.10,axis=[1,0,0],geom_pos=[0,y*.01,-.035],**servo)
        elif profile in ('ugot_balance','ugot_wheelleg'):
            for y in (-1,1):
                parent=base
                if profile=='ugot_wheelleg':
                    for i in range(2):parent=joint(parent,f'leg_{y}_{i}',[0,y*.065,-.025] if i==0 else [0,0,-.06],[.015,.012,.04],.15,**servo)
                child=joint(parent,f'wheel_{y}',[0,y*.09,-.07] if parent is base else [0,0,-.06],[.05,.015],.20,limits=None,shape='cylinder',**motor)
                child.find('geom').set('quat','0.707106781 0.707106781 0 0')
        else:
            base.set('pos','0 0 .07');wheels(base,wheel_mass=.20,radius=.05,ugot=True)
            if profile=='ugot_engineer':arm(base,'engineer',[0,0,.04],3,.30,servo=servo)
            if profile=='ugot_transform':
                for sign in (-1,1):joint(base,f'transform_hinge_{sign}',[sign*.06,0,.04],[.05,.055,.012],.15,**servo)
    else:
        # Bolted kitchen/kiosk base is visibly fixed. Arms are original generic
        # mechanisms, not an assertion about an unavailable OEM arm design.
        base=element(world,'body',name='fixed_counter',pos=[0,0,height*.22])
        geom(base,'counter','box',[depth*.45,width*.45,height*.22],mass*.8)
        if profile=='botinkit_omni':
            pan=joint(base,'wok_tilt',[0,0,height*.25],[.24,.025],mass*.10,shape='cylinder',effort=15)
            joint(pan,'wok_rotation',[0,0,.03],[.20,.02],mass*.02,axis=[0,0,1],limits=None,shape='cylinder',effort=5)
        else:
            for side in ((-1,1) if profile=='xbot_s_pro' else (1,)):
                arm(base,f'original_arm_{side}',[0,side*width*.18,height*.25],6,mass*.025)
            cup=element(world,'body',name='cup',pos=[depth*.2,0,height*.48+.04]);element(cup,'freejoint',name='cup_free')
            # Hollow cup approximated by a base and four contact walls.
            geom(cup,'cup_base','box',[.035,.035,.003],.04)
            for axis in (0,1):
                for sign in (-1,1):
                    pos=[0,0,.04];pos[axis]=sign*.033;size=[.035,.035,.04];size[axis]=.003
                    geom(cup,f'cup_wall_{axis}_{sign}','box',size,.015,pos=pos)
    # Assumed mounting frame for reduced exteroception, not task labels.
    element(base,'site',name='body_imu',size=.003)
    element(base,'camera',name='forward_camera',pos=[depth*.45,0,.05],xyaxes=[0,-1,0,0,0,1],fovy=75)
    # Collision between adjacent reduced links is intentionally suppressed by
    # MuJoCo's parent filtering; no scripted force or stabilising root is added.
    return root
