"""Original reduced mechanical archetypes; no OEM CAD is inferred.

Published envelopes come from the pinned research manifest. All internal
geometry, mass distribution, drives and joint limits are versioned assumptions.
"""
import math


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
    def joint(parent,name,pos,size,link_mass,axis=(0,1,0),limits=(-1.5,1.5),shape='box',effort=2):
        child=element(parent,'body',name=name+'_link',pos=pos)
        attrs=dict(name=name,axis=axis,type='hinge')
        if limits:attrs['range']=limits
        else:attrs['limited']='false'
        element(child,'joint',**attrs)
        geom(child,name+'_collision',shape,size,link_mass)
        element(actuator,'motor',name=name,joint=name,ctrlrange=[-1,1],gear=effort)
        return child
    def wheels(parent,side_x=(-1,1),wheel_mass=None,radius=None):
        radius=radius or min(width,depth)*.13
        wm=wheel_mass if wheel_mass is not None else mass*.02
        for x in side_x:
            for y in (-1,1):
                wheel=joint(parent,f'wheel_{"front" if x>0 else "rear"}_{"left" if y>0 else "right"}',
                            [x*depth*.33,y*width*.43,0],[radius,radius*.24],wm,limits=None,shape='cylinder',effort=mass*.04)
                wheel.find('geom').set('quat','0.707106781 0.707106781 0 0')
    def arm(parent,prefix,pos,n=6,arm_mass=1):
        for i in range(n):
            parent=joint(parent,f'{prefix}_joint{i+1}',pos if i==0 else [.08,0,.05], [.05,.015,.02],arm_mass/n,
                         axis=[0,0,1] if i%3==0 else [0,1,0],effort=2)
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
        if profile in ('ugot_quadruped','ugot_spider'):
            for x in (-1,1):
                for y in (-1,1):
                    parent=base
                    for i in range(3):
                        spread=.035 if profile=='ugot_quadruped' else .065
                        parent=joint(parent,f'leg_{x}_{y}_{i}',[x*.07,y*.05,0] if i==0 else [x*.02,y*spread,-.04],
                                     [.012,.025,.035],.10,axis=[0,0,1] if i==0 else [1,0,0],effort=.196133)
        elif profile in ('ugot_balance','ugot_wheelleg'):
            for y in (-1,1):
                parent=base
                if profile=='ugot_wheelleg':
                    for i in range(2):parent=joint(parent,f'leg_{y}_{i}',[0,y*.065,-.025] if i==0 else [0,0,-.06],[.015,.012,.04],.15,effort=.196133)
                child=joint(parent,f'wheel_{y}',[0,y*.09,-.07] if parent is base else [0,0,-.06],[.05,.015],.20,limits=None,shape='cylinder',effort=.196133)
                child.find('geom').set('quat','0.707106781 0.707106781 0 0')
        else:
            base.set('pos','0 0 .07');wheels(base,wheel_mass=.20,radius=.05)
            if profile=='ugot_engineer':arm(base,'engineer',[0,0,.04],3,.30)
            if profile=='ugot_transform':
                for sign in (-1,1):joint(base,f'transform_hinge_{sign}',[sign*.06,0,.04],[.05,.055,.012],.15,effort=.196133)
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
