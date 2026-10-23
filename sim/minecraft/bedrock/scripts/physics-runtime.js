import {world,system,ScriptEventSource,CommandPermissionLevel,Player} from '@minecraft/server';
import {PhysicsSession,gamePoint,gameEuler,rotate,multiply} from './physics-session.js';
export function startPhysics(io) {
  let session=null,entities=[],scene='',geoms=[],origin=null;
  const cleanup=()=>{if(session)session.stop();session=null;for(const e of entities)try{e.remove();}catch{}entities=[];geoms=[];};
  const attach=()=>{
    cleanup();if(!io.available())throw Error('Physics service is disabled or missing its private server secret');
    const config=io.config();origin=config.origin;if(!Array.isArray(origin)||origin.length!==3||origin.some(x=>!Number.isFinite(x)))throw Error('Physics origin invalid');
    session=new PhysicsSession(config,io.request);scene='';console.warn('AARNN physics: native authority, bounding-box display, one metre per block, full-block mirrors. Partial blocks/players pause.');
  };
  const dimension=()=>world.getDimension('overworld');
  system.afterEvents.scriptEventReceive.subscribe(event=>{
    if(!['aarnn:physics_attach','aarnn:physics_stop'].includes(event.id))return;
    const p=event.sourceEntity;if(event.sourceType!==ScriptEventSource.Server&&!(p instanceof Player&&p.commandPermissionLevel>=CommandPermissionLevel.GameDirectors))return;
    try{if(event.id==='aarnn:physics_stop')cleanup();else attach();}catch(error){cleanup();console.warn('AARNN physics: '+error.message);}
  });
  function construct(model){
    geoms=model.geoms.filter(g=>g.type!==0&&g.rgba[3]>=.5);if(geoms.length>1024)throw Error('Physics display budget');
    const tag='aarnn-physics-'+session.config.world_id;
    for(const e of dimension().getEntities({type:'aarnn:physics_proxy',tags:[tag]}))e.remove();
    entities=geoms.map(()=>{const e=dimension().spawnEntity('aarnn:physics_proxy',{x:origin[0],y:origin[1],z:origin[2]});e.addTag(tag);return e;});
  }
  function render(model,state){
    const poses=new Map(state.poses.map(p=>[p.name,p]));
    geoms.forEach((g,i)=>{const b=g.body?poses.get(model.bodies[g.body-1].name):{position:[0,0,0],quaternion_wxyz:[1,0,0,0]};if(!b)throw Error('Named native body missing');
      const offset=rotate(b.quaternion_wxyz,g.position),p=gamePoint(b.position.map((v,j)=>v+offset[j]));
      let h=g.half_size_m.slice();if(g.type===2)h=[h[0],h[0],h[0]];if(g.type===3)h=[h[0],h[0],h[0]+h[1]];if(g.type===5)h=[h[0],h[0],h[1]];
      const size=gamePoint(h).map(x=>Math.max(.00001,Math.abs(x)*2)),angles=gameEuler(multiply(b.quaternion_wxyz,g.quaternion_wxyz)),e=entities[i];
      if(p.some(x=>!Number.isFinite(x)||Math.abs(x)>10000)||size.some(x=>!Number.isFinite(x)||x>100))throw Error('Pose/display bounds');
      if(i===0)e.nameTag=model.profile_id+' · '+state.controller_mode;
      e.teleport({x:origin[0]+p[0],y:origin[1]+p[1],z:origin[2]+p[2]});
      for(let j=0;j<3;j++){e.setProperty('aarnn:s'+['x','y','z'][j],size[j]);e.setProperty('aarnn:r'+['x','y','z'][j],angles[j]);}
    });
  }
  function voxels(model,state){
    const root=state.poses.find(p=>p.name===model.bodies[0].name);if(!root)throw Error('Native root missing');
    const p=gamePoint(root.position).map((v,i)=>Math.floor(v+origin[i])),out=[];
    if(dimension().getPlayers({location:{x:p[0],y:p[1],z:p[2]},maxDistance:4}).length)throw Error('Player collision mirror unavailable; world paused');
    for(let x=p[0]-2;x<=p[0]+2;x++)for(let y=p[1]-2;y<=p[1]+2;y++)for(let z=p[2]-2;z<=p[2]+2;z++){
      if(y+1<=origin[1])continue;const block=dimension().getBlock({x,y,z});if(!block)throw Error('Mirror chunk unavailable');
      if(block.isAir)continue;
      // Bedrock has no general collision-shape query. Admit only this explicit
      // full-cube set; never guess doors, slabs, fluids, plants or custom blocks.
      if(!['minecraft:stone','minecraft:smooth_stone','minecraft:cobblestone','minecraft:dirt','minecraft:grass_block','minecraft:glass','minecraft:iron_block','minecraft:gold_block','minecraft:diamond_block','minecraft:oak_planks','minecraft:white_concrete'].includes(block.typeId))throw Error('Unmapped block collision; world paused');
      out.push([x+.5-origin[0],-(z+.5-origin[2]),y+.5-origin[1]]);if(out.length>32)throw Error('Collision mirror capacity exceeded; world paused');
    }return out;
  }
  let busy=false;
  system.runInterval(()=>{
    if(!session||busy||!session.active)return;busy=true;const active=session;
    active.fetch().then(state=>{
      if(!state||session!==active)return;if(!entities.length)construct(active.model);render(active.model,state);
      const blocks=voxels(active.model,state),next=JSON.stringify(blocks);if(next!==scene){scene=next;return active.replaceVoxels(blocks);}
    }).catch(error=>{console.warn('AARNN physics stopped: '+error.message);if(session===active)cleanup();}).finally(()=>{busy=false;});
  },1);
}
