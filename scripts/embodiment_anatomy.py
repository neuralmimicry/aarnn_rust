"""Massless anatomical landmarks bound to the actual native articulation.

Original procedural illustrations, never inferred connectome locations or new
collision supports. Engineering scale and nearest-link skinning are explicit.
"""
import math
from sim_content import robot_parts


def install(profile, root, element):
    world=root.find('worldbody')
    bodies={b.get('name'):b for b in world.iter('body')}
    def detail(body,name,shape,pos,size,colour):
        return element(body,'geom',name='anatomy_'+name,type=shape,pos=pos,size=size,rgba=list(colour)+[1],mass=0,contype=0,conaffinity=0,group=2)
    if profile=='crow_corvus_corone':
        for side,sign in [('left',1),('right',-1)]:
            for i in range(9):
                detail(bodies[side+'_wing_wrist_twist_link'],f'{side}_primary_{i}','ellipsoid',[-.035,sign*(.025+i*.013),-.002], [.07-i*.002,.014,.0014],[.035+i*.0015,.042+i*.001,.052+i*.001])
        for i in range(7):
            detail(bodies['tail_fan_link'],f'tail_feather_{i}','ellipsoid',[-.025,(i-3)*.009,0],[.055,.009,.0014],[.035,.038,.045])
        for sign in (-1,1):
            detail(bodies['neck_roll_link'],f'eye_{sign}','sphere',[.025,sign*.024,.012],[.005],[.025,.018,.012])
            detail(bodies['neck_roll_link'],f'eye_glint_{sign}','sphere',[.027,sign*.028,.014],[.0012],[.46,.49,.48])
        return
    kind='fly' if profile.startswith('drosophila') else 'fish' if profile=='zebrafish' else 'hexapod' if profile=='hexapod' else None
    if not kind:return
    scale={'fly':.075,'fish':.08,'hexapod':.30}[kind]
    base=bodies['thorax' if kind=='fly' else 'segment_00' if kind=='fish' else 'chassis']
    root_position=[float(v) for v in base.get('pos','0 0 0').split()]
    if kind=='fish':
        # Shared catalogue head +X and native segment_00 both point forward.
        origins={f'segment_{i:02}':[root_position[0]-.08/9*i,0,root_position[2]] for i in range(9)}
    else:origins={base.get('name'):root_position}
    for part in robot_parts(kind):
        name=part['id']
        if kind=='fly' and name.startswith(('thorax','wing','leg_')):continue
        if kind=='fish' and name.startswith('myomere_'):continue
        if kind=='hexapod' and (part['anchor'].startswith('leg_') or name.startswith(('deck_','body_'))):continue
        p=[v*scale for v in part['position']]
        if kind=='fish':
            owner=min(origins,key=lambda n:abs(origins[n][0]-p[0]));body=bodies[owner]
            local=[p[0]-origins[owner][0],p[1],p[2]]
        else:body=base;local=p
        half=[v*scale/2 for v in part['size']]
        shape='box' if part['shape']=='box' else 'cylinder' if part['shape']=='cylinder' else 'ellipsoid'
        size=[half[0],half[2]] if shape=='cylinder' else half
        geom=detail(body,name,shape,local,size,part['colour'])
        yaw=part['yaw'];geom.set('quat',f'{math.cos(yaw/2)} 0 0 {math.sin(yaw/2)}')
