# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Additive primitive mechanics for the six existing neural projections.

These are engineering bodies in the existing illustrative metre scale. Named
neural projections are retained verbatim; unresolved biological readout maps
fail closed rather than being assigned to arbitrary joints. No old brain is
loaded, edited or resized by this compiler.
"""
import json
import math
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
LEGACY=('celegans','drosophila_banc','drosophila_fafb','hexapod','nao','zebrafish')

# Zebrafish mechanics v2: an adult-proportioned functional surrogate (8 cm
# trunk, depth ~21 % and width ~11 % of length, tapering to the peduncle) of
# neutral density. Calibrated with a programmed 6 Hz, 1 L head-to-tail wave
# under resistive hydrodynamics (scene ``hydrodynamics``): with the native
# servo gains and torque limits it swims head-first at ~0.6 L/s; v1's
# disc-shaped segments swam backwards and its tail could not bend.
FISH_HALF_LENGTH=.0044
FISH_HALF_WIDTH=(.0045,.0045,.0045,.0042,.0036,.0030,.0024,.0018,.0014)
FISH_HALF_HEIGHT=(.0060,.0078,.0082,.0078,.0068,.0055,.0042,.0032,.0026)
FISH_SPACING=.08/9
# Fins coincide with (sit just inside) the catalogue's anatomical fins, so the
# shared look and the physics agree: caudal centre 0.51 x 8 cm behind the
# trunk centre, dorsal over segment 6 (sim_content.robot_parts('fish')).
FISH_CAUDAL=(.0080,.0006,.0122)
FISH_DORSAL=(.0070,.0005,.0060)
FISH_DORSAL_SEGMENT=6
FISH_DAMPING=1e-5
FISH_ARMATURE=1e-7
# Fly legs: passive damping below the servo's own kv (5e-4) and armature
# above the ~1e-8 kg m2 link inertia only by what a 2 ms step needs.
FLY_DAMPING=2e-4
FLY_ARMATURE=1e-6
# Drosophila mechanics v3 (flight). Legs are flight-proportioned (0.05 g per
# link; v2 carried 0.3 g per link, 70 % of a 10.4 g body), so the 4.3 g fly
# hovers at about half the stroke range. Each wing is hinged at its root on
# the thorax side, 12 mm above the thorax centre (~16 mm above the centre of
# mass): a stroke joint (+q moves the wing backward on both sides, so the
# right axis is mirrored) carrying a pitch (wing rotation) joint and an
# elliptical blade. The blade is centred on its pitch axis: with the blade
# a quarter chord behind the axis, its inertia made stroke-amplitude-
# dependent pitching couples (~7e-4 N m per rad of amplitude) that a 40 ms
# neural loop cannot trim. Stroke damping is the drive's derivative gain
# (implicit in the engine; see aarnn_embodiment::flight), armatures are
# what a 2 ms step needs. Wing membranes do not collide.
FLY_LEG_LINK_MASS=.0003
FLY_WING=dict(root=(0.,.02,.012),hub=.006,span=.06,chord=.025,thickness=.0004,blade_mass=4e-5,hub_mass=1e-5,
              stroke_damping=5e-4,stroke_armature=2e-7,stroke_range=1.6,pitch_damping=7e-6,pitch_armature=1e-8,pitch_range=1.4)
# resonant-flight-v1 motor (record ``flight``). Wingbeat 40 Hz: 12.5 steps
# of 2 ms per beat; at 20 Hz the stroke's inertial and aerodynamic reaction
# rocked the light body +-70 degrees within each beat. Wing rotation leads
# the stroke by 0.5 rad of phase (advanced rotation, +65 % lift over
# rotation timed from the measured stroke). Calibration (fixed body,
# MuJoCo, these wings): mean lift ~0.072 N at a 0.7 rad stroke amplitude
# (the 0.042 N weight near 0.53 rad); differential amplitude rolls
# (~9e-3 N m per unit asymmetry), a common stroke-centre shift pitches
# (+0.2 rad back: 7e-4 N m nose-down), a differential one yaws (2.6e-3 N m/rad).
FLY_FLIGHT=dict(wingbeat_hz=40.,drive_stiffness=.03,torque_limit=.2,amplitude_max=1.2,centre_max=.4,fold=1.45,
                recruitment=[.2,.5],hinge_angle=.5,hinge_rate=50.,hinge_lead=.5,hinge_stiffness=2e-3,panel_span=.7)


# Drosophila anatomy v4 ("looks like a fly"): proportions from the flybody
# model of D. melanogaster (TuragaLab/flybody, Apache-2.0; a CT-based adult
# female in cm) at this body's 50x scale (the thorax is ~25 x 18 mm half-
# axes, the real ~0.5 x 0.36 mm). The thorax's 3 g is split into a head
# (0.6 g) and abdomen (0.5 g) whose moments about the thorax centre cancel,
# so the total mass and centre of mass the flight motor was calibrated with
# are kept (v3 hung both as massless decoration). The eyes are on the head.
# Legs: attachment on the ventral pro-/meso-/metathorax and segment lengths
# (coxa, femur incl. trochanter, tibia, tarsus) per pair; forelegs point
# forward, hindlegs back (v3: six identical 22 mm-link legs splayed straight
# out). Each segment is a capsule from its joint to the next; the coxa
# yaws (protraction/retraction) and the distal joints flex in the leg's
# plane. Link masses stay at v3's 0.3 g (lighter legs let the unchanged
# walking circuit flip the body, delivery record 23 Nov).
FLY_THORAX=dict(size=[.025,.018,.014],mass=.0019,spawn=.1)
FLY_HEAD=dict(pos=[.0225,0,.0034],size=[.0105,.0109,.0101],mass=.0006)
FLY_ABDOMEN=dict(pos=[-.027,0,-.00075],size=[.0199,.0113,.0101],mass=.0005)
FLY_LEGS={'f':dict(attach=(.0159,.0105,-.0136),yaw=.7,lengths=(.0219,.0349,.0255,.0250)),
          'm':dict(attach=(-.0072,.0121,-.0213),yaw=.09,lengths=(.0141,.0415,.0334,.0310)),
          'r':dict(attach=(-.0189,.0096,-.0201),yaw=-.7,lengths=(.0122,.0390,.0358,.0325))}
# Real leg proportions (above) are built only with FLY_REAL_LEGS (flight retune pending).
FLY_REAL_LEGS=False
FLY_LEG_PITCH=(-.96,.26,-1.05,-.44)   # coxa down, femur raised, tibia down, tarsus to the ground
FLY_LEG_RADIUS=(.0032,.0028,.0021,.0014)
# Compound eyes on the head's sides (catalogue eye centres, head frame).
FLY_EYES=[(.0023,.0094,.0019),(.0023,-.0094,.0019)]


def fly_body(part,element,world):
    t=FLY_THORAX
    base=part(world,'thorax',[0,0,t['spawn']],t['size'],t['mass']);element(base,'freejoint',name='root')
    for name,spec in (('head',FLY_HEAD),('abdomen',FLY_ABDOMEN)):
        body=element(base,'body',name=name,pos=spec['pos'])
        element(body,'geom',name=name+'_collision',type='ellipsoid',size=spec['size'],mass=spec['mass'],rgba=[.45,.27,.11,1])
    if not FLY_REAL_LEGS:
        # v3 legs (identical, splayed sideways): the flight motor and its
        # control laws are calibrated with them. Real proportions lost
        # attitude in the flight proof (24 Nov: 2 s with v3 masses; 1.5-9 s
        # with 0.05 g links, ideal controller included), so they wait for a
        # flight retune.
        for side,sign in [('l',1),('r',-1)]:
            for row,x in [('f',.7),('m',0),('r',-.7)]:
                parent=base
                for i,kind in enumerate(('coxa','femur','tibia','tarsus')):
                    name=f'{side}{row}_{kind}';scale=.03
                    parent=part(parent,name+'_link',[x*t['size'][0],sign*t['size'][1],0] if i==0 else [0,sign*scale*.65,-scale*.35],
                                [scale*.15,scale*.35,scale*.15],FLY_LEG_LINK_MASS,name,axis=[0,0,1] if i==0 else [1,0,0])
        return base
    for side,sign in [('l',1),('r',-1)]:
        for row in 'fmr':
            leg=FLY_LEGS[row];parent=base
            pos=[leg['attach'][0],sign*leg['attach'][1],leg['attach'][2]]
            for i,kind in enumerate(('coxa','femur','tibia','tarsus')):
                name=f'{side}{row}_{kind}';length=leg['lengths'][i];pitch=FLY_LEG_PITCH[i];radius=FLY_LEG_RADIUS[i]
                body=part(parent,name+'_link',pos,[radius,length/1.4,radius],FLY_LEG_LINK_MASS,name,axis=[0,0,1] if i==0 else [1,0,0])
                if i==0:
                    yaw=-sign*leg['yaw'];body.set('quat',f'{math.cos(yaw/2):.6f} 0 0 {math.sin(yaw/2):.6f}')
                end=[0.,round(sign*length*math.cos(pitch),6),round(length*math.sin(pitch),6)]
                geom=body.find("geom[@name='"+name+"_link_collision']")
                geom.set('fromto',' '.join(str(v) for v in [0,0,0]+end));geom.set('size',str(radius))
                parent=body;pos=end
    return base


# Hexapod v3: the Freenove Big Hexapod Robot Kit for Raspberry Pi (FNK0052).
# Facts from Freenove's open-source code and tutorial (CC BY-NC-SA 3.0; only
# numbers are used here, no code): legs l1 33 mm (coxa), l2 90 mm (femur), l3
# 110 mm (tibia); six coxa servos on the body perimeter at (x right, y
# forward, mm) (55, 76), (85, 0), (55, -76) and mirrored, each leg radial
# (54 deg from lateral for the corner legs); 18 standard metal-gear leg
# servos (MG996R class: ~0.92 N m at 4.8-5 V, 0.17 s/60 deg, 55 g) and two
# SG90 head servos; HC-SR04 ultrasonic and Raspberry Pi camera on the head;
# MPU6050 on the shield; four 18650 cells; acrylic plates. Assembled mass ~2.2
# kg (estimated from parts: listings give 2.66 kg packed). Servos span 0-180
# deg about their 90 deg centre, the calibration pose (coxa radial, femur
# horizontal, tibia vertical), which is the built pose here. v2 was a 0.88 kg
# robot with 56 mm links splayed sideways and ~3.5 N m servos. The kit has
# no foot switches and its hobby servos report no angle: the contract's
# foot-contact and joint-angle channels are simulated proprioception.
HEX_FREENOVE=True
HEX_LINKS=(.033,.090,.110)
HEX_MOUNTS={'f':(.076,.055,math.radians(36.)),'m':(0.,.085,math.radians(90.)),'r':(-.076,.055,math.radians(144.))}
HEX_CHASSIS=dict(half=[.085,.07,.028],mass=1.1,spawn=.13)
HEX_LINK_MASS=(.07,.075,.025)          # coxa (femur servo), femur (tibia servo), tibia (acrylic)
HEX_SERVO=dict(effort=.92,speed=6.16,stall_rad=.15,range=1.5708)   # MG996R class, 0-180 deg
HEX_HEAD=dict(sonar=(.105,.013,.06),camera=(.10,0.,.085))


def freenove_hexapod_body(part,element,world,profile):
    c=HEX_CHASSIS
    base=part(world,'chassis',[0,0,c['spawn']],c['half'],c['mass']);element(base,'freejoint',name='root')
    # Massless acrylic plates and head (illustration; group 2).
    for z in (c['half'][2],-c['half'][2]):
        element(base,'geom',name=f'hex_plate_{"top" if z>0 else "bottom"}',type='cylinder',pos=[0,0,z],size=[.11,.003],
                rgba=[.08,.08,.09,1],mass=0,contype=0,conaffinity=0,group=2)
    element(base,'geom',name='hex_head',type='box',pos=[HEX_HEAD['sonar'][0]-.012,0,HEX_HEAD['sonar'][2]],size=[.012,.024,.012],
            rgba=[.1,.2,.45,1],mass=0,contype=0,conaffinity=0,group=2)
    _,outputs=projection(profile)
    for side,sign in [('l',1),('r',-1)]:
        for row in 'fmr':
            mx,my,yaw=HEX_MOUNTS[row]
            # Leg frame: rotated so the leg points along sign*y, as v2's legs,
            # so the joint axes and sign conventions of the contract are kept.
            frame_yaw=sign*yaw-sign*math.pi/2
            parent=base
            for i,kind in enumerate(('coxa','femur','tibia')):
                name=next(n for n in outputs if n.endswith(f'_{side}{row}_{kind}'))
                if i==0:pos=[mx,sign*my,0]
                elif i==1:pos=[0,sign*HEX_LINKS[0],0]
                else:pos=[0,sign*HEX_LINKS[1],0]
                body=part(parent,name+'_link',pos,[.01,HEX_LINKS[i]/1.4,.01],HEX_LINK_MASS[i],name,axis=[0,0,1] if i==0 else [1,0,0],
                          limits=(-HEX_SERVO['range'],HEX_SERVO['range']))
                if i==0:body.set('quat',f'{math.cos(frame_yaw/2):.6f} 0 0 {math.sin(frame_yaw/2):.6f}')
                geom=body.find("geom[@name='"+name+"_link_collision']")
                end=[0,sign*HEX_LINKS[i],0] if i<2 else [0,0,-HEX_LINKS[2]]
                geom.set('type','capsule');geom.set('fromto',' '.join(str(round(v,6)) for v in [0,0,0]+end))
                geom.set('size',str([.012,.010,.006][i]))
                if i==2:
                    element(body,'geom',name=name+'_foot',type='sphere',pos=[0,0,-HEX_LINKS[2]],size=[.008],mass=0,
                            rgba=[.15,.15,.15,1],friction=[1.,.01,.001])
                parent=body
    return base


def fly_wings(element,base):
    """Two root-hinged wings (stroke + pitch joints), visual veins massless."""
    w=FLY_WING
    for side,sign in (('left',1),('right',-1)):
        x,y,z=w['root']
        stroke=element(base,'body',name=side+'_wing',pos=[x,sign*y,z])
        element(stroke,'joint',name=side+'_wing_flap',axis=[0,0,sign],range=[-w['stroke_range'],w['stroke_range']],
                damping=w['stroke_damping'],armature=w['stroke_armature'])
        element(stroke,'geom',name=side+'_wing_hub',type='capsule',fromto=[0,0,0,0,sign*w['hub'],0],size=.001,mass=w['hub_mass'],rgba=[.45,.35,.2,1])
        blade=element(stroke,'body',name=side+'_wing_blade',pos=[0,sign*w['hub'],0])
        element(blade,'joint',name=side+'_wing_pitch',axis=[0,1,0],range=[-w['pitch_range'],w['pitch_range']],
                damping=w['pitch_damping'],armature=w['pitch_armature'])
        element(blade,'geom',name=side+'_wing_membrane',type='ellipsoid',pos=[0,sign*w['span']/2,0],
                size=[w['chord']/2,w['span']/2,w['thickness']],mass=w['blade_mass'],rgba=[.75,.8,.85,.45])
        for k,(dx,length) in enumerate(((.006,.92),(0.,.98),(-.006,.85))):
            element(blade,'geom',name=f'{side}_wing_vein_{k}',type='box',pos=[dx,sign*w['span']*length/2,.0005],
                    size=[.0004,w['span']*length/2*.96,.0002],rgba=[.38,.33,.25,.8],mass=0,contype=0,conaffinity=0,group=2)


def fly_flight(model,mujoco,profile):
    """resonant-flight-v1 record for the compiled fly (stroke springs tuned to
    the wing's own stroke inertia at the wingbeat frequency)."""
    import math
    from embodiment_neural import flight_commands
    f=FLY_FLIGHT;omega=2*math.pi*f['wingbeat_hz']
    inertia=[float(model.dof_M0[model.jnt_dofadr[mujoco.mj_name2id(model,mujoco.mjtObj.mjOBJ_JOINT,s+'_wing_flap')]]) for s in ('left','right')]
    if abs(inertia[0]-inertia[1])>1e-12*max(inertia):raise ValueError('fly wings must be mirror images')
    return dict(model='resonant-flight-v1',wingbeat_hz=f['wingbeat_hz'],stroke_inertia_kg_m2=inertia[0],
                stroke_stiffness_n_m_per_rad=inertia[0]*omega*omega,drive_stiffness_n_m_per_rad=f['drive_stiffness'],
                drive_damping_n_m_s_per_rad=FLY_WING['stroke_damping'],drive_torque_limit_n_m=f['torque_limit'],
                amplitude_max_rad=f['amplitude_max'],centre_max_rad=f['centre_max'],fold_rad=f['fold'],recruitment=f['recruitment'],
                hinge_angle_rad=f['hinge_angle'],hinge_rate_rad_s=f['hinge_rate'],hinge_lead_rad=f['hinge_lead'],
                hinge_stiffness_n_m_per_rad=f['hinge_stiffness'],
                wings=[dict(side=s,stroke_joint=s+'_wing_flap',pitch_joint=s+'_wing_pitch') for s in ('left','right')],
                commands=flight_commands(profile),
                note='shared thoracic phase at the stroke resonance; neural power, per-side amplitude and stroke centre; '
                     'engineering surrogate of asynchronous flight muscle and steering muscles, not measured physiology')


def fly_aerodynamics():
    """Scene ``aerodynamics`` (quasi-steady-panel-v2) of the fly's wing blades.

    One panel per blade at 70 % span on the pitch axis (hinge torques would
    otherwise kick the light body at every stroke reversal). The polar keeps
    v1's flat-plate lift (slope 4.5/rad, CLmax 1.5) with stall drag giving
    CD ~1.4 at 45 degrees, as measured on revolving insect wings
    (Dickinson, Lehmann & Sane 1999). No unsteady lift, added mass or wake.
    """
    import math
    w=FLY_WING;area=math.pi*w['span']*w['chord']/4
    panels=[dict(body=s+'_wing_blade',point_m=[0.,sign*w['span']*FLY_FLIGHT['panel_span'],0.],area_m2=area,polar='insect_wing')
            for s,sign in (('left',1),('right',-1))]
    return dict(aerodynamics=dict(model='quasi-steady-panel-v2',density_kg_m3=1.225,
                                  polars=dict(insect_wing=dict(lift_slope_per_rad=4.5,lift_max=1.5,drag_zero=.08,drag_induced=.15,drag_stall=6.)),
                                  panels=panels,note='quasi-steady wing blades at the stroke and wing-rotation velocity; engineering estimate'))
FISH_SKIN=[.80,.74,.55,1]      # silvery-gold interstripe
FISH_STRIPE=[.17,.23,.47,1]    # blue-black horizontal stripes
FISH_MEMBRANE=[.62,.70,.62,.9]


def fish_segment_size(i):
    return [FISH_HALF_LENGTH,FISH_HALF_WIDTH[i],FISH_HALF_HEIGHT[i]]


def neutral_mass(size):
    import math
    return 1000.*4./3.*math.pi*size[0]*size[1]*size[2]


def fish_pigment(element,body,name,size):
    """Continuous skin and blue horizontal stripes (massless, visual only).

    The hydrodynamic segments abut without overlap (no double-counted area);
    the skin overlaps its neighbours so the trunk reads as one body.
    """
    import math
    a,w,h=FISH_SPACING*.78,size[1]*1.03,size[2]*1.03
    element(body,'geom',name=f'{name}_skin',type='ellipsoid',size=[a,w,h],rgba=FISH_SKIN,mass=0,contype=0,conaffinity=0,group=2)
    for k,zf in enumerate((.42,0.,-.42)):
        y=w*math.sqrt(1-zf*zf)*.96
        for side in (1,-1):
            element(body,'geom',name=f'{name}_stripe_{k}_{"l" if side>0 else "r"}',type='ellipsoid',pos=[0,side*y,zf*h],
                    size=[a*.98,w*.08,h*.09],rgba=FISH_STRIPE,mass=0,contype=0,conaffinity=0,group=2)


def fish_fin(element,actuators,parent,name,body_pos,geom_pos,size,axis,hidden=False):
    body=element(parent,'body',name=name,pos=body_pos)
    element(body,'joint',name=name+'_joint',axis=axis,range=[-1.2,1.2],damping=FISH_DAMPING,armature=FISH_ARMATURE)
    # Gains, range and torque are set from the native servo facts later.
    element(actuators,'motor',name=name+'_joint',joint=name+'_joint',ctrlrange=[-1,1],gear=.05)
    # Group 3: physical but not drawn where a catalogue fin already shows it.
    element(body,'geom',name=name+'_collision',type='ellipsoid',pos=geom_pos,size=size,mass=neutral_mass(size),rgba=FISH_MEMBRANE,**({'group':3} if hidden else {}))
    if name=='caudal_fin':
        for k,zf in enumerate((.35,0.,-.35)):
            element(body,'geom',name=f'{name}_stripe_{k}',type='ellipsoid',pos=[geom_pos[0]*.8,0,zf*size[2]],
                    size=[size[0]*.75,size[1]*1.3,size[2]*.06],rgba=FISH_STRIPE,mass=0,contype=0,conaffinity=0,group=2)
    return body


def projection(profile):
    if profile=='nao':
        data=json.loads((ROOT/'network_nao.json').read_text())['connectome_labels']
        return data['sensory_nodes'],data['output_nodes']
    data=json.loads((ROOT/f'webots_world/configs/config_{profile}_webots.io_alignment.json').read_text())
    def name(item):return item if isinstance(item,str) else item.get('actuator_name',item.get('device_port'))
    return [name(x) for x in data['sensory_channels']],[name(x) for x in data.get('actuator_channels',data.get('output_channels',[]))]


# NAO v2: Aldebaran H25 V6 link lengths (mm: NeckOffsetZ 126.5, Shoulder
# 0/98/100, UpperArm 105, ElbowOffsetY 15, LowerArm 55.95, HandOffsetX 57.75,
# HandOffsetZ 12.31, Hip 0/50/-85, Thigh 100, Tibia 102.9, FootHeight 45.19),
# per-link masses (kg, total 5.305) and joint axes, built in the OEM zero
# pose (legs straight, arms forward). Simple primitive shells; no OEM CAD.
# The sole (160 x 88 mm, ankle 50 mm from the heel) is an engineering
# estimate: Aldebaran does not publish it.
NAO_MASS={'torso':1.0496,'HeadYaw':.07842,'HeadPitch':.60533,'ShoulderPitch':.09304,'ShoulderRoll':.15777,'ElbowYaw':.06483,
          'ElbowRoll':.07761,'WristYaw':.18533,'HipYawPitch':.06981,'HipRoll':.14053,'HipPitch':.38968,'KneePitch':.30142,
          'AnklePitch':.13416,'AnkleRoll':.17184}
NAO_SOLE=(.16,.088,.05)   # length, width, heel-to-ankle (m): estimate
NAO_PHALANX_KG=.002
NAO_SHELL=[.9,.9,.88,1]
NAO_TRIM=[.25,.3,.38,1]


def nao_body(element,world,actuators):
    import math
    standing_z=.085+.100+.1029+.04519
    base=element(world,'body',name='torso',pos=[0,0,round(standing_z+.002,4)])
    element(base,'freejoint',name='root')
    element(base,'geom',name='torso_collision',type='box',pos=[-.005,0,.025],size=[.045,.068,.08],mass=NAO_MASS['torso'],rgba=NAO_SHELL)
    element(base,'geom',name='torso_chest_plate',type='box',pos=[.04,0,.04],size=[.006,.045,.05],rgba=NAO_TRIM,mass=0,contype=0,conaffinity=0,group=2)

    def link(parent,joint,pos,axis,geoms,mass):
        body=element(parent,'body',name=joint+'_link',pos=pos)
        element(body,'joint',name=joint,axis=axis,range=[-1,1],damping=.02)
        element(actuators,'motor',name=joint,joint=joint,ctrlrange=[-1,1],gear=.05)
        for i,(kind,attrs) in enumerate(geoms):
            element(body,'geom',name=joint+('_collision' if i==0 else f'_shell_{i}'),type=kind,mass=mass if i==0 else 0,
                    rgba=NAO_SHELL if i==0 else NAO_TRIM,**attrs,**({} if i==0 else dict(contype=0,conaffinity=0,group=2)))
        return body
    neck=link(base,'HeadYaw',[0,0,.1265],[0,0,1],[('cylinder',dict(fromto=[0,0,0,0,0,.03],size=[.015]))],NAO_MASS['HeadYaw'])
    link(neck,'HeadPitch',[0,0,0],[0,1,0],[('ellipsoid',dict(pos=[0,0,.06],size=[.05,.065,.06])),
                                              ('ellipsoid',dict(pos=[.035,0,.065],size=[.018,.05,.03]))],NAO_MASS['HeadPitch'])
    for side,sign in (('L',1),('R',-1)):
        # Arm, zero pose pointing forward (+x).
        sh=link(base,side+'ShoulderPitch',[0,sign*.098,.1],[0,1,0],[('sphere',dict(size=[.025]))],NAO_MASS['ShoulderPitch'])
        up=link(sh,side+'ShoulderRoll',[0,0,0],[0,0,1],[('capsule',dict(fromto=[.015,0,0,.09,0,0],size=[.022]))],NAO_MASS['ShoulderRoll'])
        el=link(up,side+'ElbowYaw',[.105,sign*.015,0],[1,0,0],[('sphere',dict(size=[.018]))],NAO_MASS['ElbowYaw'])
        fa=link(el,side+'ElbowRoll',[0,0,0],[0,0,1],[('capsule',dict(fromto=[.01,0,0,.05,0,0],size=[.02]))],NAO_MASS['ElbowRoll'])
        hand=link(fa,side+'WristYaw',[.05595,0,0],[1,0,0],[('box',dict(pos=[.03,0,-.006],size=[.03,.018,.022]))],NAO_MASS['WristYaw']-8*NAO_PHALANX_KG)
        for i in range(1,9):
            name=side+f'Phalanx{i}'
            link(hand,name,[.058+.006*((i-1)%3),sign*(.012-.008*((i-1)%4)),-.0123-.004*((i-1)//4)],[0,sign,0],
                 [('capsule',dict(fromto=[0,0,0,.018,0,0],size=[.004]))],NAO_PHALANX_KG)
        # Leg: HipYawPitch about the 45 deg Y-Z axis (left (0,1,-1), right (0,1,1)).
        r=math.sqrt(.5)
        pel=link(base,side+'HipYawPitch',[0,sign*.05,-.085],[0,r,-sign*r],[('sphere',dict(size=[.02]))],NAO_MASS['HipYawPitch'])
        hip=link(pel,side+'HipRoll',[0,0,0],[1,0,0],[('sphere',dict(size=[.028]))],NAO_MASS['HipRoll'])
        th=link(hip,side+'HipPitch',[0,0,0],[0,1,0],[('capsule',dict(fromto=[0,0,-.02,0,0,-.085],size=[.033]))],NAO_MASS['HipPitch'])
        sk=link(th,side+'KneePitch',[0,0,-.1],[0,1,0],[('capsule',dict(fromto=[0,0,-.015,0,0,-.09],size=[.028])),
                                                       ('box',dict(pos=[.026,0,-.02],size=[.006,.022,.02]))],NAO_MASS['KneePitch'])
        an=link(sk,side+'AnklePitch',[0,0,-.1029],[0,1,0],[('sphere',dict(size=[.022]))],NAO_MASS['AnklePitch'])
        length,width,heel=NAO_SOLE
        link(an,side+'AnkleRoll',[0,0,0],[1,0,0],[('box',dict(pos=[length/2-heel,sign*.005,-.04519+.012],size=[length/2,width/2,.012],friction=[1.,.01,.001])),
                                                  ('box',dict(pos=[length/2-heel-.01,sign*.005,-.02],size=[length/2-.02,width/2-.008,.012]))],NAO_MASS['AnkleRoll'])
    return base


def build(profile,element,base_world):
    root,world=base_world(profile);actuators=element(root,'actuator')
    def part(parent,name,pos,size,mass,joint_name=None,axis=(0,1,0),limits=(-1.2,1.2)):
        body=element(parent,'body',name=name,pos=pos)
        if joint_name:
            # C. elegans mechanics v3 (calibrated with a programmed 0.5 Hz,
            # 0.65 L travelling wave): low hinge damping and a 30 mN m servo
            # so the body can follow biological bending; v2 (damping .02,
            # 6 mN m) tracked 28 % of the wave and could not crawl.
            # Zebrafish mechanics v2: v1's generic hinge damping (.02) and the
            # default armature (1e-4 kg m2, ~1e4 x a segment's inertia) let the
            # native 0.2 mN m tail servos bend at most ~0.01 rad/s, so no tail
            # wave could form. Passive trunk damping and armature are now small.
            # Drosophila mechanics v2, same cause: damping .02 against the
            # 0.01 N m/rad fly servos gave a 2 s settling time, so legs moved
            # 0.008 rad of a 0.08 rad stepping command at 3.5-5 Hz.
            fish={'armature':FISH_ARMATURE} if profile=='zebrafish' else {'armature':FLY_ARMATURE} if profile.startswith('drosophila') else {}
            element(body,'joint',name=joint_name,axis=axis,range=limits,damping=.002 if profile=='celegans' else FISH_DAMPING if profile=='zebrafish'
                    else FLY_DAMPING if profile.startswith('drosophila') else .02,**fish)
            if profile=='celegans':
                element(actuators,'position',name=joint_name,joint=joint_name,ctrlrange=limits,kp=.2,kv=.0015,forcerange=[-.03,.03])
            else: element(actuators,'motor',name=joint_name,joint=joint_name,ctrlrange=[-1,1],gear=.05)
        if profile=='celegans':
            import math
            index=int(name.rsplit('_',1)[1]);t=(index+.6)/24
            radius=.0135*(.22+.78*math.sin(math.pi*t)**.48)
            # Group 1 marks the worm's own collision cuticle so mounted range
            # rays can exclude every segment (not only the mount body).
            # Low Coulomb friction: on agar the resistive-force substrate
            # (native adapter) carries the anisotropic drag that propels it.
            # priority=1: MuJoCo otherwise mixes contact friction by maximum,
            # so the ground's 0.8 overrode the cuticle and isotropic Coulomb
            # friction swamped the anisotropic substrate drag.
            element(body,'geom',name=name+'_collision',type='capsule',fromto=[-.0048,0,0,.0048,0,0],size=radius,mass=mass,rgba=[.76,.63,.39,1],
                    group=1,friction=[.02,.005,.0001],priority=1)
            # Massless cuticle annuli and four body-wall quadrants follow each
            # actual spine link; they never drive or support the robot.
            for offset in (-.0035,.0035):
                element(body,'geom',name=f'{name}_annulus_{"a" if offset<0 else "b"}',type='ellipsoid',pos=[offset,0,0],size=[.00028,radius*1.007,radius*1.007],rgba=[.54,.43,.25,1],mass=0,contype=0,conaffinity=0,group=2)
            # Quadrants follow the side-lying worm of the v3 transducer:
            # +y ventral, -y dorsal (in-plane bending), +z left, -z right,
            # q = MVL, MDL, MVR, MDR. Segment s carries muscle row 24 - s, so
            # the absent canonical MVL24 is on the tail segment.
            for q,(y,z) in enumerate(((1,1),(-1,1),(1,-1),(-1,-1))):
                if index==0 and q==0:continue # Canonical MVL24 is absent.
                element(body,'geom',name=f'{name}_muscle_quadrant_{q}',type='capsule',fromto=[-.004,y*radius*.52,z*radius*.52,.004,y*radius*.52,z*radius*.52],size=.0012,rgba=[.69,.32,.21,1],mass=0,contype=0,conaffinity=0,group=2)
            element(body,'geom',name=name+'_intestine',type='capsule',fromto=[-.006,0,0,.006,0,0],size=.0022,rgba=[.58,.46,.19,1],mass=0,contype=0,conaffinity=0,group=2)
            if index in (20,22):element(body,'geom',name=name+'_pharyngeal_bulb',type='ellipsoid',pos=[0,0,.0015],size=[.004,.0035,.0035],rgba=[.84,.77,.57,1],mass=0,contype=0,conaffinity=0,group=2)
        else:
            shape='capsule' if profile.startswith('drosophila') and any(k in name for k in ('coxa','femur','tibia','tarsus')) else 'ellipsoid' if profile.startswith('drosophila') or profile=='zebrafish' else 'box'
            if shape=='capsule':element(body,'geom',name=name+'_collision',type=shape,fromto=[0,-size[1]*.7,0,0,size[1]*.7,0],size=min(size[0],size[2]),mass=mass,rgba=[.38,.25,.12,1])
            elif profile=='zebrafish':
                # Neutral density: mass = water density x ellipsoid volume.
                element(body,'geom',name=name+'_collision',type=shape,size=size,mass=neutral_mass(size),
                        rgba=FISH_SKIN if name.startswith('segment_') else FISH_MEMBRANE)
                if name.startswith('segment_'):fish_pigment(element,body,name,size)
            else:element(body,'geom',name=name+'_collision',type=shape,size=size,mass=mass,rgba=[.4,.45,.3,1])
        return body
    if profile in ('celegans','zebrafish'):
        segments=24 if profile=='celegans' else 9
        length=.34 if profile=='celegans' else .08;radius=.017 if profile=='celegans' else .006
        def trunk(i):return fish_segment_size(i) if profile=='zebrafish' else [length/segments*.44,radius,radius]
        # The fish spawns at mid-depth inside its aquarium (v1: 0.3 m, above the walls).
        base=part(world,'segment_00',[-length/2 if profile=='celegans' else length/2,0,.017 if profile=='celegans' else .15],trunk(0),.0068 if profile=='celegans' else .0002)
        element(base,'freejoint',name='root');parent=base;trunk_bodies=[base]
        for i in range(1,segments):
            parent=part(parent,f'segment_{i:02}',[(1 if profile=='celegans' else -1)*length/segments,0,0],trunk(i),.0068 if profile=='celegans' else .00015,
                        f'celegans_spine_{i+1:02}' if profile=='celegans' else f'tail_joint_{i-1}',axis=[0,0,1],limits=[-.38,.38])
            trunk_bodies.append(parent)
        if profile=='zebrafish':
            # Thin fin membranes; joint names and neural readouts unchanged.
            for side in ('left','right'):
                fin=part(base,side+'_pectoral',[0,.01 if side=='left' else -.01,0],[.006,.007,.0004],.00002,side+'_pectoral_pitch')
                part(fin,side+'_pectoral_roll',[0,0,0],[.004,.005,.0004],.00001,side+'_pectoral_roll')
                # The catalogue's pectoral fins carry the look.
                for b in (fin,fin.find('body')):b.find('geom').set('group','3')
            # Erect dorsal fin folding about the lateral axis and a vertical
            # caudal fin hinged at the peduncle (yaw, like the trunk): the
            # membranes that pass the trunk wave to the water. Actuator order
            # (dorsal, caudal, pelvic, pelvic2) is unchanged from v1.
            dorsal=trunk_bodies[FISH_DORSAL_SEGMENT];top=fish_segment_size(FISH_DORSAL_SEGMENT)[2]
            fish_fin(element,actuators,dorsal,'dorsal_fin',[.0037,0,top*.8],[0,0,.012-top*.8],FISH_DORSAL,[0,1,0],hidden=True)
            # Hinge at the peduncle (4 mm behind segment 8); fin centre at the
            # catalogue caudal fin (-0.0408 m from the trunk origin line).
            fish_fin(element,actuators,parent,'caudal_fin',[-.004,0,0],[-.0057,0,0],FISH_CAUDAL,[0,0,1])
            for kind,where in [('pelvic',[-.004,.0045,-.006]),('pelvic2',[-.004,-.0045,-.006])]:
                part(base,kind+'_fin',where,[.004,.003,.0004],.000002,kind+'_fin_joint')
            for n in ('zebrafish_o_28_jaw','zebrafish_o_29_operculum_l','zebrafish_o_30_operculum_r'):
                part(base,n+'_link',[.004,.004 if n.endswith('_l') else -.004 if n.endswith('_r') else 0,0],[.002,.001,.001],.000001,n)
    elif profile=='nao':
        base=nao_body(element,world,actuators)
        # The two HipYawPitch joints are one physical motor on the robot.
        element(element(root,'equality'),'joint',joint1='LHipYawPitch',joint2='RHipYawPitch',polycoef=[0,1,0,0,0])
    elif profile.startswith('drosophila'):
        base=fly_body(part,element,world)
        fly_wings(element,base)
    elif profile=='hexapod' and HEX_FREENOVE:
        base=freenove_hexapod_body(part,element,world,profile)
    else:
        fly=False;size=[.135,.085,.018]
        base=part(world,'chassis',[0,0,.19],size,.25);element(base,'freejoint',name='root')
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
                                [scale*.15,scale*.35,scale*.15],FLY_LEG_LINK_MASS if fly else .035,name,axis=[0,0,1] if i==0 else [1,0,0])
        if fly:fly_wings(element,base)
    # Reduced primitive rigs overlap at articulation pivots. Explicitly disable
    # intra-body collisions; collisions against the environment remain active.
    for geom in base.findall('.//geom'):
        if geom.get('group')!='2':geom.set('contype','2');geom.set('conaffinity','1')
    for body in base.iter('body'):
        if body.get('name','').endswith(('_wing','_wing_blade')):
            for geom in body.findall('geom'):geom.set('contype','0');geom.set('conaffinity','0')
    from embodiment_neural import install_mechanics
    install_mechanics(profile,root,element)
    return root


# Field sampled by each named chemical/thermal/light/gas channel (v2). The
# neural projection of each channel is in scripts/build_celegans_network_json.py.
CELEGANS_FIELDS = {8: 'light', 9: 'light', 10: 'temperature', 11: 'temperature',
                   12: 'attractant_soluble', 13: 'repellent_soluble', 14: 'repellent_soluble',
                   15: 'attractant_volatile', 16: 'attractant_volatile',
                   17: 'repellent_soluble', 18: 'repellent_soluble', 19: 'oxygen', 20: 'oxygen'}
BEND_REGIONS = 12
BEND_RANGE_RAD = .25


def celegans_bend_names():
    return [f'celegans_s_{24 + 2*(k-1) + j:02d}_bend_{side}{k:02d}'
            for k in range(1, BEND_REGIONS + 1) for j, side in enumerate(('dorsal', 'ventral'))]


def celegans_sensors():
    """Named simulation transducers (celegans-sensors-v2), Y-up -> Z-up mounts.

    0-5 rectified high-pass vibration (tap) and rotation-rate magnitudes;
    6-7 contact; 8-20 analytic stimulus fields sampled at the original mounts;
    21-23 proximity rays that exclude the whole worm; 24-47 rectified
    dorsal/ventral body bend at 12 points head (01) to tail (12).
    """
    names,_=projection('celegans')
    names=list(names[:24])+celegans_bend_names()
    out=[]
    for i,name in enumerate(names):
        kind=('celegans-vibration-v2' if 'accel' in name else 'celegans-rotation-v2') if i<6 else 'celegans-contact-v1' if i<8 \
            else 'celegans-field-v2' if i<21 else 'celegans-range-v2' if i<24 else 'celegans-bend-v1'
        s=dict(name=name,kind=kind,units='normalised',range=[0,1],rate_hz=500,missing='fault-disarm')
        if i>=24:
            k=(i-24)//2+1
            s.update(axis=k,mode='dorsal' if (i-24)%2==0 else 'ventral',range=[0,BEND_RANGE_RAD],units='rad')
            out.append(s);continue
        if i in CELEGANS_FIELDS:s.update(mode=CELEGANS_FIELDS[i])
        if i<6:s.update(axis=i%3)
        elif i<8:
            # Gentle-touch receptive fields (v3): the anterior touch cells
            # (ALM, AVM) cover the front half of the body, PLM the back half
            # (v2 sensed only the nose and tail-tip segments). `body` keeps
            # the extreme segment for engines that read one body.
            front=i==6
            s.update(body='segment_23' if front else 'segment_00',
                     bodies=[f'segment_{k:02}' for k in (range(12,24) if front else range(0,12))])
        elif i<21:
            # Field samplers sit on the sensilla of the neurons they drive:
            # amphids at the nose (light, heat, taste, odour, O2) and
            # phasmids/PQR at the tail. Head-ward x is +; L/R is +/-y.
            x,y,z={8:(.168,.012,.008),9:(.168,-.012,.008),10:(.160,.012,.008),11:(.160,-.012,.008),
                   12:(.180,0,0),13:(.172,.008,0),14:(.172,-.008,0),15:(.170,.010,0),16:(.170,-.010,0),
                   17:(-.160,.006,0),18:(-.160,-.006,0),19:(.170,0,.004),20:(-.163,0,.004)}[i]
            segment=max(0,min(23,round((x+.17)/(.34/24))))
            s.update(body=f'segment_{segment:02}',position_m=[x-(-.17+segment*.34/24),y,z])
        else:
            x,y,angle,reach=[(.182,0,0,.26),(.168,.03,.36,.23),(.168,-.03,-.36,.23),(.06,.046,1.2,.38),(.06,-.046,-1.2,.38),(-.074,.03,2.46,.36),(-.074,-.03,-2.46,.36),(.126,0,0,.92),(-.126,0,3.14159,.92),(.110,.078,.95,1.28),(.110,-.078,-.95,1.28),(-.168,0,3.14159,1.30)][i-12]
            segment=max(0,min(23,round((x+.17)/(.34/24))))
            s.update(body=f'segment_{segment:02}',position_m=[x-(-.17+segment*.34/24),y,0],angle_rad=angle,range=[0,reach])
        out.append(s)
    return out
