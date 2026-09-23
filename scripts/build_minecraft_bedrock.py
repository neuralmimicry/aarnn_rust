#!/usr/bin/env python3
"""Generate native Bedrock packs from the shared catalogue; no game binaries or neural code."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import shutil
import struct
import uuid
import zipfile
import zlib

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT/'sim/minecraft/bedrock'
OUT = ROOT/'sim/minecraft/build/bedrock'
NS = uuid.UUID('cf89c7f2-7977-4937-b4a1-d6a354e3c938')
IDS = {key:str(uuid.uuid5(NS,key)) for key in ('behaviour','data','script','resource','resources')}
VERSION = [0,1,0]


def encoded(data):
    return json.dumps(data, separators=(',',':'),sort_keys=True)+'\n'


def png(colours):
    def chunk(name,data):
        return struct.pack('>I',len(data))+name+data+struct.pack('>I',zlib.crc32(name+data)&0xffffffff)
    pixels = [tuple(round(c*255) for c in rgb)+(255,) for rgb in colours]
    pixels += [(255,255,255,255)]*(4096-len(pixels))
    raw=b''.join(b'\0'+b''.join(bytes(p) for p in pixels[y*64:(y+1)*64]) for y in range(64))
    return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',64,64,8,6,0,0,0))+chunk(b'IDAT',zlib.compress(raw))+chunk(b'IEND',b'')


def slices(shape):
    if shape=='box': return [([0,0,0],[1,1,1])]
    # Bounded cuboid approximation. The native Bedrock entity format has no
    # arbitrary ellipsoid primitive; preserve the shared full extents and colour.
    result=[]
    for j in range(5):
        z=(j-2)/5
        for k in range(5):
            y=(k-2)/5
            radius2=.25-y*y-(z*z if shape!='cylinder' else 0)
            if radius2<=0:continue
            result.append(([0,y,z],[2*math.sqrt(radius2),.2,.2]))
    return result


def hexapod_joint_name(side,leg,depth):
    return f'aarnn_joint_{side}_{leg}_{depth}'


def hexapod_joint_specs(profile):
    if profile['kind']!='hexapod':return []
    result=[]
    for side in (-1,1):
        for leg in range(3):
            body_x=.27-.27*leg
            pivots=((body_x,side*.23,.01),(body_x,side*.44,-.07),(body_x,side*.57,-.17))
            for depth,pivot in enumerate(pivots):
                parent=hexapod_joint_name(side,leg,depth-1) if depth else None
                result.append((hexapod_joint_name(side,leg,depth),pivot,parent))
    return result


def hexapod_endpoint_names(profile):
    if profile['kind']!='hexapod':return []
    names=[]
    for side in ('l','r'):
        for leg in ('f','m','r'):
            for joint in ('coxa','femur','tibia'):names.append(f'{side}{leg}_{joint}')
    return names


def geometry(profile,objects,colours,anatomy=False,habitat=False):
    scale=256*(1 if habitat else profile['body_length'])
    bones=[]
    if not habitat:
        for name,pivot,parent in hexapod_joint_specs(profile):
            converted=[pivot[0]*scale,pivot[2]*scale,-pivot[1]*scale]
            bone=dict(name=name,pivot=converted,rotation=[0,0,0],cubes=[])
            if parent:bone['parent']=parent
            bones.append(bone)
    for obj in objects:
        if obj['material']=='water' or obj['internal'] and not anatomy:continue
        if anatomy and ((profile['kind']=='worm' and obj['id'].startswith('cuticle_')) or (profile['kind']=='fish' and obj['id'].startswith('myomere_'))):continue
        x,y,z=obj['position']; centre=[x*scale,z*scale,-y*scale]
        colour=colours.index(tuple(obj['colour']));uv=[colour%64,colour//64]
        cubes=[]
        for offset,extent in slices(obj['shape']):
            size=[extent[i]*obj['size'][i]*scale for i in range(3)]
            off=[offset[i]*obj['size'][i]*scale for i in range(3)]
            converted_size=[size[0],size[2],size[1]]
            position=[centre[0]+off[0],centre[1]+off[2],centre[2]-off[1]]
            cubes.append(dict(origin=[round(position[i]-converted_size[i]/2,6) for i in range(3)],
                              size=[round(v,6) for v in converted_size],
                              uv={face:dict(uv=uv,uv_size=[1,1]) for face in ('north','south','east','west','up','down')}))
        bone=dict(name=obj['id'].replace('.','_'),pivot=centre,rotation=[0,math.degrees(obj['yaw']),0],cubes=cubes)
        if not habitat and profile['kind']=='hexapod' and obj['anchor'].startswith('leg_'):
            side,leg=obj['anchor'].split('_')[1:]
            if obj['id'].startswith('foot_'):depth=2
            elif obj['id'].startswith(('servo_','link_')):depth=int(obj['id'].rsplit('_',1)[1])
            else:depth=-1
            if 0<=depth<=2:bone['parent']=hexapod_joint_name(int(side),int(leg),depth)
        bones.append(bone)
    name=profile['id']+('_habitat' if habitat else '_anatomy' if anatomy else '')
    return {'format_version':'1.12.0','minecraft:geometry':[dict(description=dict(identifier='geometry.aarnn.'+name,
        texture_width=64,texture_height=64,visible_bounds_width=40,visible_bounds_height=24,visible_bounds_offset=[0,8,0]),bones=bones)]}


def generate(directory):
    data=json.loads((ROOT/'sim/content/compiled.generated.json').read_text())
    objects=[o for h in data['habitats'] for o in h['objects']]+[o for p in data['profiles'] for o in p['parts']]
    colours=sorted({tuple(o['colour']) for o in objects})
    if len(colours)>4096:raise ValueError('Palette budget exceeded')
    def write(path,body):
        p=directory/path;p.parent.mkdir(parents=True,exist_ok=True)
        p.write_bytes(body if isinstance(body,bytes) else body.encode() if isinstance(body,str) else encoded(body).encode())
    write('resource/textures/aarnn/palette.png',png(colours))
    write('resource/manifest.json',dict(format_version=2,header=dict(name='AARNN Sensory Lab resources',description='Six shared robot equivalents; '+data['digest'],uuid=IDS['resource'],version=VERSION,min_engine_version=[1,26,30]),modules=[dict(type='resources',uuid=IDS['resources'],version=VERSION)]))
    for p in data['profiles']:
        h=next(h for h in data['habitats'] if h['id']==p['habitat'])
        for habitat in (False,True):
            name=p['id']+('_habitat' if habitat else '')
            geom={'default':'geometry.aarnn.'+name}
            if not habitat:geom['anatomy']='geometry.aarnn.'+name+'_anatomy'
            write('resource/models/entity/'+name+'.geo.json',geometry(p,h['objects'] if habitat else p['parts'],colours,habitat=habitat))
            if not habitat:write('resource/models/entity/'+name+'_anatomy.geo.json',geometry(p,p['parts'],colours,anatomy=True))
            description=dict(identifier='aarnn:'+name,materials={'default':'entity_alphatest'},textures={'default':'textures/aarnn/palette'},geometry=geom,render_controllers=['controller.render.aarnn_habitat' if habitat else 'controller.render.aarnn_robot'])
            properties={'aarnn:anatomy':dict(type='bool',default=False,client_sync=True)}
            if not habitat and p['kind']=='hexapod':
                endpoint_names=hexapod_endpoint_names(p)
                if endpoint_names!=[n.split('_',3)[3] for n in p['output_names']]:raise ValueError('Hexapod output catalogue is not canonical')
                properties.update({'aarnn:joint_'+joint:dict(type='float',range=[0.0,1.0],default=0.0,client_sync=True) for joint in endpoint_names})
                animation='animation.aarnn.'+name+'.joints'
                description.update(animations={'joints':animation},scripts={'animate':['joints']})
                animated={}
                for depth,joint in enumerate(('coxa','femur','tibia')):
                    axis=[0,1,0] if depth==0 else [1,0,0]
                    for side in (-1,1):
                        for leg in range(3):
                            suffix=('l' if side>0 else 'r')+('f' if leg==0 else 'm' if leg==1 else 'r')+'_'+joint
                            expression="query.property('aarnn:joint_"+suffix+"') * 40.0"
                            rotation=[expression if axis[i] else 0 for i in range(3)]
                            animated[hexapod_joint_name(side,leg,depth)]=dict(rotation=rotation)
                write('resource/animations/'+name+'.animation.json',{'format_version':'1.8.0','animations':{animation:dict(loop=True,bones=animated)}})
            elif not habitat and p['kind'] in ('worm','fish'):
                count=24 if p['kind']=='worm' else 12
                properties.update({'aarnn:bend_'+str(i):dict(type='float',range=[-1.0,1.0],default=0.0,client_sync=True) for i in range(count)})
                animation='animation.aarnn.'+name+'.activity'
                description.update(animations={'activity':animation},scripts={'animate':['activity']})
                animated={o['id'].replace('.','_'):dict(position=[0,0,"-query.property('aarnn:bend_"+str(int(o['anchor'][8:]))+"') * "+str(256*p['body_length'])]) for o in p['parts'] if o['anchor'].startswith('segment_')}
                write('resource/animations/'+name+'.animation.json',{'format_version':'1.8.0','animations':{animation:dict(loop=True,bones=animated)}})
            write('resource/entity/'+name+'.entity.json',{'format_version':'1.10.0','minecraft:client_entity':{'description':description}})
            entity={'format_version':'1.21.0','minecraft:entity':{'description':dict(identifier='aarnn:'+name,is_spawnable=False,is_summonable=True,is_experimental=False,properties=properties),
                'components':{'minecraft:physics':dict(has_collision=False,has_gravity=False),'minecraft:collision_box':dict(width=1,height=1),'minecraft:persistent':{},'minecraft:pushable':dict(is_pushable=False,is_pushable_by_piston=False),'minecraft:health':dict(value=100,max=100),'minecraft:nameable':dict(always_show=not habitat,allow_name_tag_renaming=False)}}}
            for variant in ('offline','server'):write(variant+'/entities/'+name+'.json',entity)
    write('resource/render_controllers/aarnn.render_controllers.json',{'format_version':'1.8.0','render_controllers':{
        'controller.render.aarnn_habitat':dict(geometry='Geometry.default',materials=[{'*':'Material.default'}],textures=['Texture.default']),
        'controller.render.aarnn_robot':dict(arrays={'geometries':{'Array.body':['Geometry.default','Geometry.anatomy']}},geometry="Array.body[query.property('aarnn:anatomy')]",materials=[{'*':'Material.default'}],textures=['Texture.default'])}})
    for variant in ('offline','server'):
        deps=[dict(module_name='@minecraft/server',version='2.9.0'),dict(uuid=IDS['resource'],version=VERSION)]
        if variant=='server':deps += [dict(module_name=n,version='1.0.0-beta') for n in ('@minecraft/server-net','@minecraft/server-admin')]
        write(variant+'/manifest.json',dict(format_version=2,header=dict(name='AARNN Sensory Lab '+variant,description='Shared sandbox; '+data['digest'],uuid=IDS['behaviour'],version=VERSION,min_engine_version=[1,26,30]),
            modules=[dict(type='data',uuid=IDS['data'],version=VERSION),dict(type='script',language='javascript',entry='scripts/main.js',uuid=IDS['script'],version=VERSION)],dependencies=deps))
        write(variant+'/scripts/content.generated.js','// Generated from sim/content/compiled.generated.json\nexport const content = '+encoded(data)+';\n')
        write(variant+'/scripts/reference.generated.js','// Generated verbatim from web_ui/webgl-world.js; no separate sensory implementation.\n'+(ROOT/'web_ui/webgl-world.js').read_text()+'\nexport const reference = globalThis.NmWorld;\n')
        for name in ('runtime.js','session.js','nao-chat.js'):write(variant+'/scripts/'+name,(SOURCE/'scripts'/name).read_text())
        write(variant+'/scripts/main.js',(SOURCE/'scripts'/('server.js' if variant=='server' else 'offline.js')).read_text())
    write('server-config/permissions.json',dict(allowed_modules=['@minecraft/server','@minecraft/server-admin','@minecraft/server-net']))
    write('server-config/variables.json',dict(AARNN_ALLOW_SANDBOX_INFERENCE=False,AARNN_CONTENT_DIGEST=data['digest']))
    write('world_behavior_packs.json',[dict(pack_id=IDS['behaviour'],version=VERSION)])
    write('world_resource_packs.json',[dict(pack_id=IDS['resource'],version=VERSION)])
    write('bedrock-content.json',dict(content_digest=data['digest'],pack_ids=IDS,api_version='2.9.0',minimum_engine=[1,26,30],profiles=[p['id'] for p in data['profiles']],habitat_objects=sum(len(h['objects']) for h in data['habitats']),neural_runtime='external Rust only'))
    return data


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,default=OUT);args=parser.parse_args()
    generate(args.output)
    for variant in ('offline','server'):
        name='AARNN-Bedrock-'+variant+'.mcaddon'
        with zipfile.ZipFile(args.output/name,'w',zipfile.ZIP_DEFLATED) as archive:
            for folder,target in ((variant,'AARNN_Lab_BP'),('resource','AARNN_Lab_RP')):
                for path in sorted((args.output/folder).rglob('*')):
                    if path.is_file():
                        entry=zipfile.ZipInfo(target+'/'+str(path.relative_to(args.output/folder)),(2026,1,1,0,0,0));entry.compress_type=zipfile.ZIP_DEFLATED
                        archive.writestr(entry,path.read_bytes())
    with zipfile.ZipFile(args.output/'AARNN-Bedrock-server-overlay.zip','w',zipfile.ZIP_DEFLATED) as archive:
        for folder,target in (('server','behavior_packs/AARNN_Lab_BP'),('resource','resource_packs/AARNN_Lab_RP'),('server-config','config/'+IDS['script'])):
            for path in sorted((args.output/folder).rglob('*')):
                if path.is_file():
                    entry=zipfile.ZipInfo(target+'/'+str(path.relative_to(args.output/folder)),(2026,1,1,0,0,0));entry.compress_type=zipfile.ZIP_DEFLATED
                    archive.writestr(entry,path.read_bytes())
        for name in ('world_behavior_packs.json','world_resource_packs.json'):
            archive.writestr(zipfile.ZipInfo('worlds/AARNN-Sensory-Lab/'+name,(2026,1,1,0,0,0)),(args.output/name).read_bytes())
    print('Bedrock packs:',args.output)


if __name__=='__main__':main()
