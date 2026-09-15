// QA-only observer, copied into an isolated server pack. Never distributed.
import { world, system, ScriptEventSource } from '@minecraft/server';
import { content } from './content.generated.js';
import { Session } from './session.js';
const report = value => console.warn('AARNN_BEDROCK_QA '+JSON.stringify(value));
const counts = new Map();
const stop = Session.prototype.stop;
Session.prototype.stop = function (reason) {
  if(reason?.startsWith('fault:')) report({kind:'failure',reason:this.profile.id+': '+reason});
  return stop.call(this,reason);
};
const poll = Session.prototype.poll;
Session.prototype.poll = function () {
  const reply = poll.call(this);
  if (reply) {
    const frames = (counts.get(this.profile.id) || 0) + 1;
    counts.set(this.profile.id, frames);
    report({kind:'response',profile:this.profile.id,frames,reply});
  }
  return reply;
};
system.afterEvents.scriptEventReceive.subscribe(event => {
  if (event.sourceType !== ScriptEventSource.Server || event.id !== 'aarnnqa:verify') return;
  try {
    const d=world.getDimension('overworld'), profiles=[];
    const npc=d.spawnEntity('minecraft:villager_v2',{x:50,y:129,z:48});
    try {
      if(npc.typeId!=='minecraft:villager_v2')throw Error('Vanilla NPC identity corrupted: '+npc.typeId);
    } finally {npc.remove();}
    for (const p of content.profiles) {
      const bodies=d.getEntities({type:'aarnn:'+p.id});
      const habitats=d.getEntities({type:'aarnn:'+p.id+'_habitat'});
      if (bodies.length!==1 || habitats.length!==1) throw Error(p.id+': entity count');
      const e=bodies[0];
      if (e.getDynamicProperty('aarnn:content')!==content.digest) throw Error('stale body');
      if (!e.nameTag.includes('disarmed')) throw Error('body still armed: '+p.id);
      if (p.kind==='fish') {
        const top=Math.max(...p.parts.map(o=>o.position[2]+o.size[2]/2))*p.body_length*16;
        for(const y of [e.location.y,e.location.y+top])
          if(d.getBlock({x:e.location.x,y,z:e.location.z}).typeId!=='minecraft:water') throw Error('fish is not submerged');
      }
      for(let i=0;i<(p.kind==='worm'?24:p.kind==='fish'?12:0);i++)
        if(e.getProperty('aarnn:bend_'+i)!==0) throw Error('non-neutral body property');
      profiles.push({profile:p.id,sensory:p.sensory,output:p.output,location:e.location,entity_id:e.id});
    }
    report({kind:'world',stage:event.message,content_digest:content.digest,profiles,entities:12,fish_submerged:true,native_npc_identity:true,status:'pass'});
  } catch(error) {report({kind:'failure',reason:String(error)});}
},{namespaces:['aarnnqa']});
