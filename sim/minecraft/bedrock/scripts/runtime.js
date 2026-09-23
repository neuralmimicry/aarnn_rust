import { world, system, ScriptEventSource, CommandPermissionLevel, Player } from '@minecraft/server';
import { content } from './content.generated.js';
import { reference } from './reference.generated.js';
import { Session } from './session.js';
import { startNaoChat } from './nao-chat.js';

const HALF = 16, BASE = 128, sessions = new Map(), pending = new Map();
const clamp = (v,a,b)=>Math.max(a,Math.min(b,v));
const profile = id => { const p=content.profiles.find(p=>p.id===id); if(!p)throw Error('Unknown robot profile');return p; };
const centre = p => {const i=content.profiles.indexOf(p);return {x:(i%3)*48,y:BASE,z:Math.floor(i/3)*48};};
const habitat = p => content.habitats.find(h=>h.id===p.habitat);
const hexapodJoint = name => 'aarnn:joint_'+name.replace(/^.*_[0-9]{3}_/,'');
const syncHexapodJoints = (p,e,actuators) => {
  if(p.kind!=='hexapod')return;
  p.output_names.forEach((name,i)=>e.setProperty(hexapodJoint(name),clamp(actuators[i]||0,0,1)));
};
let building = false;

export function start(io) {
  const dimension = ()=>world.getDimension('overworld');
  const robot = p => {
    const matches=dimension().getEntities({type:'aarnn:'+p.id});
    if(matches.length!==1)throw Error('Build the lab; exactly one body per profile is required');
    return matches[0];
  };
  const clear = (p,reason) => {
    const state=sessions.get(p.id);if(state)state.session.stop(reason);sessions.delete(p.id);
    try {const e=robot(p);e.nameTag=p.id+' · '+reason;for(let i=0;i<(p.kind==='worm'?24:p.kind==='fish'?12:0);i++)e.setProperty('aarnn:bend_'+i,0);syncHexapodJoints(p,e,[]);}catch{/* Body unloaded or absent; no output remains armed. */}
  };
  // The companion may have negotiated minutes before a player opens the world.
  // Wait for an actual neural response before admitting the first encounter.
  startNaoChat(io,()=>robot(profile('nao')),()=>{
    const session=sessions.get('nao')?.session;
    return session?.active===true && session.lastOutput>=0;
  });
  function *reload() {
    try {
      // Persisted ticking areas and entities may load after worldLoad. Request
      // preloading and wait; never create replacement bodies during that gap.
      dimension().runCommand('tickingarea preload aarnn_lab true');
      const deadline=system.currentTick+600;
      for(;;) {
        let complete=true;
        for(const p of content.profiles)for(const suffix of ['', '_habitat']) {
          const count=dimension().getEntities({type:'aarnn:'+p.id+suffix}).length;
          if(count>1)throw Error('Duplicate saved lab entity');
          if(count!==1)complete=false;
        }
        if(complete)break;
        if(system.currentTick>=deadline)throw Error('Saved lab did not load; inspect ticking area and saved entities');
        yield;
      }
      for(const p of content.profiles)clear(p,'disarmed after load');
      console.warn('AARNN world already exists');
    } catch(error) {console.warn('AARNN world: '+error.message);}
    finally {building=false;}
  }
  const snapshot = (p,e) => {
    const c=centre(p),location=e.location;
    return {x:(location.x-c.x)/HALF,z:-(location.z-c.z)/HALF,heading:e.getRotation().y*Math.PI/180,actuators:new Array(p.output).fill(0)};
  };
  function *build() {
    try {
      if(world.getDynamicProperty('aarnn:content') && world.getDynamicProperty('aarnn:content')!==content.digest)throw Error('Saved catalogue differs; use a new world or review migration');
      // One bounded ticking area; no dimension or chunks outside the six lab plots.
      try {dimension().runCommand('tickingarea add -16 0 -16 127 255 79 aarnn_lab true');}
      catch {console.warn('AARNN ticking area may already exist; verifying that every target chunk loads');}
      const deadline=system.currentTick+600;
      for(const p of content.profiles) {
        const c=centre(p);
        /** @type {[import('@minecraft/server').Vector3, string][]} */
        const blocks=[];
        /** @type {[import('@minecraft/server').Vector3, string][]} */
        const waterBlocks=[];
        for(let x=-16;x<16;x++)for(let z=-16;z<16;z++)blocks.push([{x:c.x+x,y:BASE-2,z:c.z+z},'minecraft:smooth_stone']);
        if(p.kind==='fish')for(let x=-17;x<=16;x++)for(let z=-17;z<=16;z++)for(let y=-1;y<=10;y++) {
          const wall=x===-17||x===16||z===-17||z===16||y===-1;
          if(wall)blocks.push([{x:c.x+x,y:BASE+y,z:c.z+z},'minecraft:glass']);
          else if(y<10)waterBlocks.push([{x:c.x+x,y:BASE+y,z:c.z+z},'minecraft:water']);
        }
        // Check each target before replacing it. An operator's existing construction
        // is never silently cleared, even if this command is run in the wrong world.
        // Finish the glass shell before introducing fluid. On slow hosts water
        // ticks can run between job yields; accept its equivalent flowing form.
        for(const [position,type] of blocks.concat(waterBlocks)) {
          let block;
          while(!block) {
            try {block=dimension().getBlock(position);}catch(error){if(!String(error).includes('Unloaded'))throw error;}
            if(system.currentTick>deadline)throw Error('Lab chunk loading deadline exceeded');
            if(!block)yield;
          }
          if(!block || !block.isAir && block.typeId!==type && !(type==='minecraft:water' && block.typeId==='minecraft:flowing_water'))throw Error('Lab area is occupied at '+JSON.stringify(position)+' ('+block?.typeId+'); use a new flat world');
          block.setType(type);yield;
        }
        for(const isHabitat of [true,false]) {
          const type='aarnn:'+p.id+(isHabitat?'_habitat':'');
          const found=dimension().getEntities({type});
          if(found.length>1)throw Error('Duplicate lab entity; inspect before continuing');
          if(!found.length) {
            const e=dimension().spawnEntity(type,{x:c.x,y:c.y+(isHabitat?0:p.body_height*HALF),z:c.z});
            e.setDynamicProperty('aarnn:content',content.digest);
            e.nameTag=isHabitat?'':p.id+' · disarmed';
          }
        }
      }
      world.setDynamicProperty('aarnn:content',content.digest);
      console.warn('AARNN_BEDROCK_WORLD '+content.digest+' six profiles; zebrafish submerged');
    } catch(error) {console.warn('AARNN world: '+error.message);}
    finally {building=false;}
  }
  /** @param {import('@minecraft/server').ScriptEventCommandMessageAfterEvent} event */
  function command(event) {
    if(!event.id.startsWith('aarnn:'))return;
    const player=event.sourceEntity;
    if(event.sourceType!==ScriptEventSource.Server && !(player instanceof Player && player.commandPermissionLevel>=CommandPermissionLevel.GameDirectors))return;
    const reply=text=>{console.warn('AARNN '+text);if(player instanceof Player)player.sendMessage('AARNN '+text);};
    try {
      if(event.message.length>128)throw Error('Command exceeds bound');
      const [id,sequence,...extra]=event.message.trim().split(/\s+/);
      if(extra.length)throw Error('Too many command arguments');
      const action=event.id.slice(6);
      if(action==='stop'){for(const p of content.profiles)clear(p,'disarmed');reply('all robots stopped');return;}
      if(action==='status'){reply(content.profiles.map(p=>p.id+': '+(sessions.get(p.id)?.session.status||'disarmed')).join('; '));return;}
      if(action==='world') {
        if(id!=='build')throw Error('Use /scriptevent aarnn:world build in a new flat world');
        if(building)throw Error('World construction already pending');
        if(world.getDynamicProperty('aarnn:content')===content.digest){building=true;system.runJob(reload());reply('loading saved lab');return;}
        building=true;system.runJob(build());reply('building bounded lab at Y=128');return;
      }
      const p=profile(id),e=robot(p),c=centre(p);
      if(e.getDynamicProperty('aarnn:content')!==content.digest)throw Error('Body content differs; use matching packs/world');
      if(action==='visit') {
        if(!(player instanceof Player))throw Error('Run visit as an operator player');
        player.teleport({x:c.x+12,y:BASE+15,z:c.z+12},{dimension:dimension(),facingLocation:e.location});
      } else if(action==='anatomy')e.setProperty('aarnn:anatomy',!e.getProperty('aarnn:anatomy'));
      else if(action==='disconnect')clear(p,'disarmed');
      else if(action==='senses') {
        const values=reference.sense(p,habitat(p),snapshot(p,e),{});
        reply(p.id+' '+values.length+' channels; mean '+(values.reduce((a,b)=>a+b,0)/values.length).toFixed(3));
      } else if(action==='connect') {
        if(!io.available())throw Error('BDS connector unavailable, disabled or stale; inspect server configuration');
        if(pending.has(p.id))throw Error('Prior request still pending; inspect bridge before reconnecting');
        clear(p,'disarmed');
        const session=new Session(p,content.digest,io.request,sequence===undefined?0:Number(sequence));
        sessions.set(p.id,{session,entity:e,pose:snapshot(p,e),history:{}});e.nameTag=p.id+' · armed';
      } else throw Error('Use world, visit, anatomy, senses, connect, disconnect, status or stop');
    } catch(error){reply(error.message);}
  }
  system.afterEvents.scriptEventReceive.subscribe(command,{namespaces:['aarnn']});
  world.afterEvents.entityLoad.subscribe(({entity})=>{
    const p=content.profiles.find(p=>entity.typeId==='aarnn:'+p.id);
    if(p)clear(p,'disarmed after load');
  });
  world.afterEvents.worldLoad.subscribe(()=>{
    for(const p of content.profiles)clear(p,'disarmed after load');
    console.warn('AARNN_BEDROCK_READY '+content.digest+' inference='+io.available());
  });
  system.runInterval(()=>{
    for(const [id,state] of sessions) {
      const p=profile(id),{session,entity:e,pose}=state;
      try {
        if(!e.isValid || !io.available()){clear(p,'disarmed: unavailable or revoked');continue;}
        const reply=session.poll();
        if(reply) {
          pose.actuators=pose.actuators.map(v=>v*.82);for(const i of reply.output_spike_indices)pose.actuators[i]=1;
          const motion=reference.motion(p,pose.actuators);pose.heading+=motion.turn*.08;
          const dir=[Math.cos(pose.heading),Math.sin(pose.heading),0],range=p.body_length*.65;
          const hit=reference.ray(habitat(p),[pose.x,pose.z,p.body_height],dir,range);
          const native=dimension().getBlockFromRay(e.location,{x:dir[0],y:0,z:-dir[1]},{maxDistance:range*HALF,includeLiquidBlocks:false,includePassableBlocks:false});
          if(hit.distance>=p.body_length*.6 && !native){pose.x=clamp(pose.x+dir[0]*motion.drive*.008,-.85,.85);pose.z=clamp(pose.z+dir[1]*motion.drive*.008,-.85,.85);}
          const c=centre(p);e.teleport({x:c.x+pose.x*HALF,y:c.y+p.body_height*HALF,z:c.z-pose.z*HALF},{rotation:{x:0,y:pose.heading*180/Math.PI}});
          for(let i=0;i<(p.kind==='worm'?24:p.kind==='fish'?12:0);i++) {
            const a=pose.actuators,indices=p.kind==='worm'?p.muscle_channels[i]:[];
            const bend=p.kind==='worm'?((a[indices[0]]||0)+(a[indices[1]]||0)-(a[indices[2]]||0)-(a[indices[3]]||0))*.025:((a[(i%8)*2]||0)-(a[(i%8)*2+1]||0))*.035;
            e.setProperty('aarnn:bend_'+i,bend);
          }
          syncHexapodJoints(p,e,pose.actuators);
        }
        if(!session.active){clear(p,session.status);continue;}
        if(!session.pending) {
          const c=centre(p);
          pose.participants=dimension().getEntities({location:e.location,maxDistance:24,closest:65})
            .filter(other=>other.id!==e.id && !other.typeId.endsWith('_habitat') && (other instanceof Player || other.typeId.startsWith('aarnn:') || other.getComponent('minecraft:health')))
            .slice(0,64).map(other=>{const box=other.getAABB();return {id:other.id,position:[(box.center.x-c.x)/HALF,-(box.center.z-c.z)/HALF,(box.center.y-c.y)/HALF],size:[box.extent.x*2/HALF,box.extent.z*2/HALF,box.extent.y*2/HALF]};});
          const request=session.submit(reference.sense(p,habitat(p),pose,state.history),system.currentTick);
          pending.set(id,request);request.finally(()=>{if(pending.get(id)===request)pending.delete(id);});
        }
      } catch(error) {
        console.warn('AARNN '+id+' body/sensory error: '+String(error));
        clear(p,'fault: body or sensory channel unavailable');
      }
    }
  },1);
}
