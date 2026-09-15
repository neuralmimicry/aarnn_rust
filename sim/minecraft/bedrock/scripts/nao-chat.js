import { world, system, Player, CommandPermissionLevel, CustomCommandParamType, CustomCommandStatus } from '@minecraft/server';

/** Ordinary players have a scoped chat path, independent of operator lab commands. */
export function startNaoChat(io, getBody, connected) {
  const turns=new Map(), encountered=new Set();
  let nextEncounterTick=0;
  const nearby=player=>{
    const body=getBody();
    if(!body || !body.isValid || !connected() || player.dimension.id!==body.dimension.id ||
       Math.hypot(player.location.x-body.location.x,player.location.y-body.location.y,player.location.z-body.location.z)>24)
      throw Error('Connected NAO must be within 24 blocks in this dimension');
    return body;
  };
  const request=async(path,data)=>{
    if(!io.socialAvailable?.())throw Error('NAO social session unavailable; use the connected BDS pack and social launcher');
    const response=await io.socialRequest(path,data);
    if(response.status!==200 || response.body.length>16384)throw Error('NAO social request rejected or unavailable');
    return JSON.parse(response.body);
  };
  const stop=playerId=>{
    if(encountered.size<64)encountered.add(playerId);
    const turn=turns.get(playerId);turns.delete(playerId);
    if(turn)request('/api/stop',{player:turn.principal}).catch(()=>{});
  };
  const say=async(player,text)=>{
    const autonomous=text===null;
    try {
      const body=nearby(player);
      if(!autonomous && (typeof text!=='string' || !text.trim() || text.length>256 || /[\x00-\x1f\x7f]/.test(text)))throw Error('Use a short message without control characters');
      if(turns.has(player.id) || turns.size>=16)throw Error('A turn is pending; wait or use /aarnn:nao_stop');
      const principal='bedrock:'+player.id.replace(/[^A-Za-z0-9_.:-]/g,'_');
      const turn={principal,id:'',body,player,deadline:system.currentTick+1200,pending:true};
      turns.set(player.id,turn);
      if(!autonomous)player.sendMessage?.('You → NAO: '+text);
      const result=await request(autonomous?'/api/encounter':'/api/turn',{player:principal,text,kind:player instanceof Player?'player':'npc',sequence:system.currentTick,
        capture_ns:system.currentTick*50000000,modality:'typed',position:[
          (player.location.x-body.location.x)/24,(player.location.y-body.location.y)/24,(player.location.z-body.location.z)/24]});
      if(turns.get(player.id)!==turn)return;
      if(result.state==='observed'){turns.delete(player.id);return;}
      if(result.schema!=='NAO-SOCIAL/1' || typeof result.id!=='string')throw Error('NAO social schema mismatch');
      turn.id=result.id;turn.pending=false;
    } catch(error) {stop(player.id);if(player.isValid)player.sendMessage?.('NAO channel: '+error.message);}
  };
  // Script API 2.9 custom commands support ordinary authenticated Players. NPCs
  // cannot impersonate a Player merely by submitting a script-event payload.
  if(system.beforeEvents?.startup)system.beforeEvents.startup.subscribe(({customCommandRegistry:r})=>{
    r.registerCommand({name:'aarnn:nao',description:'Talk to nearby NAO (quote a message containing spaces)',
      permissionLevel:CommandPermissionLevel.Any,cheatsRequired:false,
      mandatoryParameters:[{name:'message',type:CustomCommandParamType.String}]},(origin,text)=>{
      if(!(origin.sourceEntity instanceof Player))return {status:CustomCommandStatus.Failure,message:'Run this command as a player'};
      const player=origin.sourceEntity;system.run(()=>say(player,text));return {status:CustomCommandStatus.Success};
    });
    r.registerCommand({name:'aarnn:nao_stop',description:'Stop your NAO conversation',permissionLevel:CommandPermissionLevel.Any,cheatsRequired:false},origin=>{
      if(!(origin.sourceEntity instanceof Player))return {status:CustomCommandStatus.Failure,message:'Player required'};
      const player=origin.sourceEntity;system.run(()=>{stop(player.id);player.sendMessage?.('NAO conversation stopped');});return {status:CustomCommandStatus.Success};
    });
  });
  world.afterEvents.playerLeave?.subscribe(event=>stop(event.playerId));
  world.afterEvents.playerInteractWithEntity?.subscribe(({player,target})=>{
    if(target.typeId==='aarnn:nao')player.sendMessage?.('Talk to NAO: /aarnn:nao "Hello NAO". Stop: /aarnn:nao_stop. Dictation can be entered through your device’s chat keyboard.');
  });
  system.runInterval(()=>{
    if(system.currentTick>=nextEncounterTick && io.socialAvailable?.() && connected()) {
      // Interval callbacks may start at any tick offset. A modulo test can
      // therefore miss every scan for the lifetime of the loaded pack.
      nextEncounterTick=system.currentTick+20;
      try {
        const body=getBody();
        for(const actor of body.dimension.getEntities({location:body.location,maxDistance:24,closest:64})) {
          if(encountered.size>=64 || turns.size>=8)break;
          if(actor.id===body.id || encountered.has(actor.id) || turns.has(actor.id) ||
              !(actor instanceof Player || actor.typeId==='minecraft:villager_v2' || actor.typeId==='minecraft:npc' || actor.typeId==='aarnn:nao'))continue;
          encountered.add(actor.id);say(actor,null);
        }
      } catch { /* No loaded NAO, so no encounter can be admitted. */ }
    }
    for(const [playerId,turn] of turns) {
      try {
        if(!turn.player.isValid || nearby(turn.player).id!==turn.body.id || system.currentTick>turn.deadline || !io.socialAvailable?.())throw Error('Conversation stopped: player, body or connection changed');
        if(turn.pending || !turn.id)continue;
        turn.pending=true;
        request('/api/poll',{player:turn.principal,id:turn.id}).then(result=>{
          if(turns.get(playerId)!==turn)return;
          nearby(turn.player);turn.pending=false;
          if(result.schema!=='NAO-SOCIAL/1' || result.id!==turn.id)throw Error('Mismatched social reply');
          if(result.state==='queued' || result.state==='active')return;
          turns.delete(playerId);
          if(result.state==='replied' && result.reply && typeof result.reply.text==='string' && result.reply.text.length<=512) {
            const text=result.reply.text;turn.player.sendMessage?.('NAO: '+text);
            // Name tags are a bounded in-world speech bubble; clear after ten
            // seconds and on entityLoad so saved speech is never replayed.
            turn.body.nameTag='NAO: '+text;
            system.runTimeout(()=>{if(turn.body.isValid && turn.body.nameTag==='NAO: '+text)turn.body.nameTag='nao · armed';},200);
          } else turn.player.sendMessage?.('NAO channel: '+String(result.state));
        }).catch(()=>{if(turns.get(playerId)===turn){stop(playerId);if(turn.player.isValid)turn.player.sendMessage?.('NAO conversation stopped; no reply was replayed');}});
      } catch(error) {stop(playerId);if(turn.player.isValid)turn.player.sendMessage?.('NAO channel: '+error.message);}
    }
  },4);
}
