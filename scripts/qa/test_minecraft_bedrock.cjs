/* Native Bedrock pack contracts; actual engine execution is a separate capability. */
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict');
const root=path.resolve(__dirname,'../..'),packs=path.join(root,'sim/minecraft/build/bedrock');
const content=JSON.parse(fs.readFileSync(path.join(root,'sim/content/compiled.generated.json')));
const fixtures=JSON.parse(fs.readFileSync(path.join(root,'sim/minecraft/build/reference.json')));
const report={profiles:[],native_engine:'not-run',scope:'generated packs, reference sensors, session failure paths and simulated Bedrock API lifecycle'};
function files(dir){return fs.readdirSync(dir,{withFileTypes:true}).flatMap(p=>p.isDirectory()?files(path.join(dir,p.name)):[path.join(dir,p.name)]);}
async function main(){
  const context=vm.createContext({console});const cache=new Map();let api;
  async function module(file){
    file=path.resolve(file);if(cache.has(file))return cache.get(file);
    const m=new vm.SourceTextModule(fs.readFileSync(file,'utf8'),{context,identifier:file});cache.set(file,m);
    await m.link((specifier,ref)=>specifier==='@minecraft/server'?api:module(path.resolve(path.dirname(ref.identifier),specifier)));return m;
  }
  const reference=await module(path.join(packs,'server/scripts/reference.generated.js'));await reference.evaluate();
  const sessionModule=await module(path.join(packs,'server/scripts/session.js'));await sessionModule.evaluate();
  const {Session}=sessionModule.namespace;
  for(const p of content.profiles){
    const h=content.habitats.find(h=>h.id===p.habitat),fixture=fixtures.cases.find(c=>c.profile===p.id),previous={};
    for(const f of fixture.frames){
      const values=reference.namespace.reference.sense(p,h,{x:f.x,z:f.y,heading:f.heading,actuators:fixture.outputs},previous);
      assert.equal(values.length,p.sensory);for(let i=0;i<values.length;i++)assert.ok(Math.abs(values[i]-f.values[i])<=1e-10);
    }
    const geometry=JSON.parse(fs.readFileSync(path.join(packs,'resource/models/entity',p.id+'.geo.json')))['minecraft:geometry'][0];
    assert.ok(geometry.bones.length>20);assert.ok(geometry.bones.length<=512);
    for(const b of geometry.bones)for(const cube of b.cubes){assert.ok(cube.origin.every(Number.isFinite));assert.ok(cube.size.every(n=>Number.isFinite(n)&&n>0));}
    if(p.kind==='hexapod') {
      const endpointNames=['lf_coxa','lf_femur','lf_tibia','lm_coxa','lm_femur','lm_tibia','lr_coxa','lr_femur','lr_tibia','rf_coxa','rf_femur','rf_tibia','rm_coxa','rm_femur','rm_tibia','rr_coxa','rr_femur','rr_tibia'];
      const entity=JSON.parse(fs.readFileSync(path.join(packs,'server/entities/hexapod.json')));
      const props=entity['minecraft:entity'].description.properties;
      assert.deepEqual(endpointNames.map(n=>props['aarnn:joint_'+n]?.default),endpointNames.map(()=>0));
      const animation=JSON.parse(fs.readFileSync(path.join(packs,'resource/animations/hexapod.animation.json')));
      assert.equal(Object.keys(animation.animations['animation.aarnn.hexapod.joints'].bones).length,18);
      assert.ok(geometry.bones.some(b=>b.parent==='aarnn_joint_1_0_0'));
    }
    let resolve,seen;
    const session=new Session(p,content.digest,(body,timeout)=>{seen={body,timeout};return new Promise(r=>{resolve=r;});});
    const pending=session.submit(new Array(p.sensory).fill(.7),100);await Promise.resolve();
    assert.equal(seen.body.network_id,p.id);assert.equal(seen.body.dt_ms,1);assert.equal(seen.body.capture_clock,'bedrock_server_tick');
    assert.throws(()=>session.submit([],101));
    resolve({status:200,body:JSON.stringify({network_id:p.id,content_digest:content.digest,output_step_index:1,output_spike_indices:[0]})});
    await pending;assert.equal(session.poll().output_spike_indices[0],0);assert.equal(session.poll(),null);
    const next=session.submit(new Array(p.sensory).fill(0),101);await Promise.resolve();
    assert.equal(seen.body.step_index,2);
    resolve({status:200,body:JSON.stringify({network_id:p.id,content_digest:content.digest,output_step_index:2,output_spike_indices:[]})});
    await next;assert.equal(session.poll().output_step_index,2);assert.equal(session.active,true);
    const stale=session.submit(new Array(p.sensory).fill(0),102);await Promise.resolve();session.stop();
    resolve({status:200,body:JSON.stringify({network_id:p.id,content_digest:content.digest,output_step_index:3,output_spike_indices:[0]})});
    await stale;assert.equal(session.poll(),null);assert.equal(session.active,false);
    const bad=new Session(p,content.digest,()=>Promise.resolve({status:200,body:JSON.stringify({network_id:p.id,content_digest:content.digest,output_step_index:1,output_spike_indices:[p.output]})}));
    await bad.submit(new Array(p.sensory).fill(0),103);assert.equal(bad.active,false);
    const timeout=new Session(p,content.digest,()=>Promise.reject(Error('timeout')));await timeout.submit(new Array(p.sensory).fill(0),104);assert.equal(timeout.active,false);
    report.profiles.push({profile:p.id,status:'pass',sensory:p.sensory,output:p.output,bones:geometry.bones.length});
  }
  for(const file of files(packs).filter(p=>p.endsWith('.json')))JSON.parse(fs.readFileSync(file));
  const offline=JSON.parse(fs.readFileSync(path.join(packs,'offline/manifest.json'))),server=JSON.parse(fs.readFileSync(path.join(packs,'server/manifest.json')));
  assert.equal(offline.header.uuid,server.header.uuid);assert.equal(offline.dependencies.some(d=>d.module_name==='@minecraft/server-net'),false);
  assert.equal(server.dependencies.some(d=>d.module_name==='@minecraft/server-net'),true);
  // Drive the real script using a controlled Bedrock API stand-in. This checks
  // world generation, submersion, idempotence and command authorisation, not rendering.
  const blocks=new Map(),entities=[],properties=new Map(),events={},jobs=[],intervals=[],commands=new Map();
  let tick=0;
  const locationKey=p=>`${p.x},${p.y},${p.z}`;
  const dimension={runCommand:()=>({successCount:1}),getBlock:p=>({get isAir(){return !blocks.has(locationKey(p));},get typeId(){return blocks.get(locationKey(p));},setType:type=>blocks.set(locationKey(p),type)}),
    getEntities:({type})=>entities.filter(e=>!type || e.typeId===type),spawnEntity:(type,location)=>{
      const props=new Map(),e={id:String(entities.length),dimension,getComponent:()=>null,getAABB:()=>({center:location,extent:{x:.3,y:.9,z:.3}}),typeId:type,location,isValid:true,nameTag:'',getDynamicProperty:k=>props.get(k),setDynamicProperty:(k,v)=>props.set(k,v),getProperty:k=>props.get(k),setProperty:(k,v)=>props.set(k,v),getRotation:()=>({x:0,y:0}),teleport:p=>{e.location=p;}};entities.push(e);return e;
    },getBlockFromRay:()=>undefined};
  const mock={world:{getDimension:()=>dimension,getDynamicProperty:k=>properties.get(k),setDynamicProperty:(k,v)=>properties.set(k,v),afterEvents:{worldLoad:{subscribe:fn=>events.load=fn},entityLoad:{subscribe:fn=>events.entityLoad=fn}}},
    system:{get currentTick(){return tick;},runJob:job=>jobs.push(job),runInterval:fn=>intervals.push(fn),run:fn=>fn(),runTimeout:()=>{},beforeEvents:{startup:{subscribe:fn=>fn({customCommandRegistry:{registerCommand:(spec,fn)=>commands.set(spec.name,{spec,fn})}})}},afterEvents:{scriptEventReceive:{subscribe:fn=>events.command=fn}}},ScriptEventSource:{Server:'Server'},CustomCommandParamType:{String:'string'},CustomCommandStatus:{Success:'success',Failure:'failure'},CommandPermissionLevel:{GameDirectors:1,Any:0},Player:class Player{}};
  api=new vm.SyntheticModule(Object.keys(mock),function(){for(const [k,v] of Object.entries(mock))this.setExport(k,v);},{context});
  const runtimePath=path.join(packs,'server/scripts/runtime.js');
  const runtime=new vm.SourceTextModule(fs.readFileSync(runtimePath,'utf8'),{context,identifier:runtimePath});
  await runtime.link((specifier,ref)=>specifier==='@minecraft/server'?api:module(path.resolve(path.dirname(ref.identifier),specifier)));
  let enabled=false;const socialCalls=[];const socialRequest=(route,data)=>{socialCalls.push({route,data});return Promise.resolve({status:200,body:JSON.stringify({schema:'NAO-SOCIAL/1',id:'turn',state:route==='/api/poll'?'replied':'queued',reply:{text:'Hello from the controlled neural-output fixture'}})});};
  let holdNao=false,releaseNao;
  await runtime.evaluate();runtime.namespace.start({socialAvailable:()=>enabled,socialRequest,available:()=>enabled,request:body=>{
    const response={status:200,body:JSON.stringify({network_id:body.network_id,content_digest:content.digest,output_step_index:body.step_index+1,output_spike_indices:[0]})};
    return holdNao && body.network_id==='nao'?new Promise(resolve=>{releaseNao=()=>resolve(response);}):Promise.resolve(response);
  }});events.load();
  const ordinary=new mock.Player();ordinary.typeId='minecraft:player';ordinary.commandPermissionLevel=0;
  events.command({id:'aarnn:world',message:'build',sourceType:'Entity',sourceEntity:ordinary});assert.equal(jobs.length,0);
  events.command({id:'aarnn:world',message:'build',sourceType:'Server'});assert.equal(jobs.length,1);
  let iterations=0;for(const job of jobs)for(;;){const r=job.next();if(r.done)break;if(++iterations%100===0)tick++;assert.ok(iterations<20000);}
  assert.equal(entities.length,12);assert.equal(properties.get('aarnn:content'),content.digest);
  const fish=entities.find(e=>e.typeId==='aarnn:zebrafish');
  for(const y of [Math.floor(fish.location.y),Math.floor(fish.location.y)+1])assert.equal(blocks.get(locationKey({x:fish.location.x,y,z:fish.location.z})),'minecraft:water');
  const operator=new mock.Player();operator.typeId='minecraft:player';operator.commandPermissionLevel=1;operator.sendMessage=()=>{};
  events.command({id:'aarnn:world',message:'build',sourceType:'Entity',sourceEntity:operator});assert.equal(entities.length,12);assert.equal(jobs.length,2);
  assert.equal(jobs[1].next().done,true);
  events.command({id:'aarnn:connect',message:'zebrafish',sourceType:'Server'});assert.match(fish.nameTag,/disarmed/);
  enabled=true;
  for(const p of content.profiles)events.command({id:'aarnn:connect',message:p.id,sourceType:'Server'});
  for(let i=0;i<4;i++){tick++;intervals.forEach(fn=>fn());await new Promise(setImmediate);}
  for(const p of content.profiles)assert.match(entities.find(e=>e.typeId==='aarnn:'+p.id).nameTag,/ · armed$/);
  const hexapod=entities.find(e=>e.typeId==='aarnn:hexapod');
  assert.equal(hexapod.getProperty('aarnn:joint_lf_coxa'),1);
  assert.equal(hexapod.getProperty('aarnn:joint_lm_coxa'),0);
  const nao=entities.find(e=>e.typeId==='aarnn:nao');
  Object.assign(ordinary,{id:'ordinary',dimension,location:{...nao.location},isValid:true,sendMessage:()=>{}});
  assert.equal(commands.get('aarnn:nao').spec.permissionLevel,0);
  assert.equal(commands.get('aarnn:nao').spec.cheatsRequired,false);
  assert.equal(commands.get('aarnn:nao').fn({sourceEntity:{}},'hello').status,'failure');
  assert.equal(commands.get('aarnn:nao').fn({sourceEntity:ordinary},'hello').status,'success');
  await new Promise(setImmediate);intervals.forEach(fn=>fn());await new Promise(setImmediate);
  assert.ok(socialCalls.some(c=>c.route==='/api/turn' && c.data.player==='bedrock:ordinary'));
  assert.match(nao.nameTag,/controlled neural-output/);
  const calls=socialCalls.length;ordinary.location.x+=30;
  commands.get('aarnn:nao').fn({sourceEntity:ordinary},'out of range');await new Promise(setImmediate);
  assert.equal(socialCalls.length,calls);
  commands.get('aarnn:nao_stop').fn({sourceEntity:ordinary});
  events.command({id:'aarnn:disconnect',message:'nao',sourceType:'Server'});
  holdNao=true;events.command({id:'aarnn:connect',message:'nao',sourceType:'Server'});
  const npc=dimension.spawnEntity('minecraft:villager_v2',{...nao.location});
  // Simulate a callback offset that never lands on a multiple of twenty.
  tick=1001;
  for(let i=0;i<8;i++){tick+=4;intervals.forEach(fn=>fn());await new Promise(setImmediate);}
  assert.equal(socialCalls.filter(c=>c.route==='/api/encounter' && c.data.player==='bedrock:'+npc.id).length,0);
  assert.equal(typeof releaseNao,'function');holdNao=false;releaseNao();await new Promise(setImmediate);
  for(let i=0;i<12;i++){tick+=4;intervals.forEach(fn=>fn());await new Promise(setImmediate);}
  assert.equal(socialCalls.filter(c=>c.route==='/api/encounter' && c.data.player==='bedrock:'+npc.id).length,1);
  assert.match(nao.nameTag,/controlled neural-output/);
  events.entityLoad({entity:fish});assert.match(fish.nameTag,/disarmed after load/);
  enabled=false;intervals.forEach(fn=>fn());
  for(const p of content.profiles)assert.match(entities.find(e=>e.typeId==='aarnn:'+p.id).nameTag,/disarmed/);
  report.world={entities:entities.length,blocks:blocks.size,fish_submerged:true};
  const out=process.env.NM_MINECRAFT_RESULT_DIR;if(out)fs.writeFileSync(path.join(out,'bedrock.json'),JSON.stringify(report,null,2));
  console.log(JSON.stringify(report));
}
main().catch(error=>{console.error(error);process.exitCode=1;});
