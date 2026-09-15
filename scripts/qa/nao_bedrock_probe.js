// QA-only native NPC/bubble observer. Never included in distributed packs.
import { world, system, ScriptEventSource } from '@minecraft/server';
let active=false, reported=false;
system.afterEvents.scriptEventReceive.subscribe(event=>{
  if(event.sourceType!==ScriptEventSource.Server || event.id!=='aarnnqa:encounter')return;
  const d=world.getDimension('overworld'),n=d.getEntities({type:'aarnn:nao'})[0];
  const npc=d.spawnEntity('minecraft:villager_v2',{x:n.location.x+2,y:n.location.y+1,z:n.location.z});
  console.warn('AARNN_NPC_ID '+npc.typeId);
  active=true;
},{namespaces:['aarnnqa']});
system.runInterval(()=>{
  if(!active || reported)return;
  const n=world.getDimension('overworld').getEntities({type:'aarnn:nao'})[0];
  if(n?.nameTag.startsWith('NAO:')){
    reported=true;
    console.warn('AARNN_SOCIAL_NATIVE '+JSON.stringify({bubble:n.nameTag,tick:system.currentTick}));
  }
},4);
