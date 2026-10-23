/* Session/transform tests. Native game execution remains a separate gate. */
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict');
const watchdog=setTimeout(()=>{console.error("session test did not complete");process.exit(2);},10000);
(async()=>{
  const file=path.resolve(__dirname,'../../sim/minecraft/bedrock/scripts/physics-session.js');
  const module=new vm.SourceTextModule(fs.readFileSync(file,'utf8'));await module.link(()=>{throw Error('unexpected import');});await module.evaluate();
  const {PhysicsSession,gamePoint,gameEuler,rotate,multiply}=module.namespace;
  const config={world_id:'one',profile_id:'ugot_mecanum',model_digest:'model',io_digest:'io'};
  const state={...config,reset_epoch:0,step:1,time_us:2000,publication:1,world_config_epoch:0,poses:[{name:'root',position:[0,0,1],quaternion_wxyz:[1,0,0,0]}]};
  const model={digest:'model',profile_id:'ugot_mecanum',io:{digest:'io'},geoms:[]};
  const commands=[];
  let current=state;
  const session=new PhysicsSession(config,async(p,b)=>{commands.push([p,b]);return {status:200,body:JSON.stringify(p==='/model'?model:p==='/state'?current:{world_config_epoch:2})};});
  assert.equal((await session.fetch()).step,1);await session.replaceVoxels([[2,0,.5]]);
  assert.equal(commands.filter(([p,b])=>p==='/command').map(([p,b])=>b.action).join(','),'pause,collision_set,resume');
  assert.equal(commands.some(([p])=>p.includes('infer')),false);
  current={...state,reset_epoch:1};await assert.rejects(session.fetch(),/explicit reattach/);assert.equal(session.active,false);
  // In-flight collision update cannot resume after a stop.
  let resolve;const pendingCalls=[];
  const stopped=new PhysicsSession(config,async(p,b)=>{pendingCalls.push(b?.action||p);if(b?.action==='pause')return new Promise(r=>{resolve=r;});return {status:200,body:'{}'};});
  stopped.reset=0;const work=stopped.replaceVoxels([]);const originalResolve=resolve;stopped.stop();originalResolve({status:200,body:'{"world_config_epoch":0}'});await assert.rejects(work);assert.equal(pendingCalls.includes('resume'),false);
  assert.equal(Array.from(gamePoint([2,3,4])).join(','),'2,4,-3');
  const c=Math.SQRT1_2,q=[c,0,0,c];
  assert.ok(Math.abs(gameEuler(q)[1]-90)<1e-6);
  assert.ok(Math.abs(rotate(q,[1,0,0])[1]-1)<1e-12);
  // Optional real native service lane: this observer submits no sensory/AER data.
  if(process.env.NM_TEST_PHYSICS_URL){
    const request=async(p,b)=>{const r=await fetch(process.env.NM_TEST_PHYSICS_URL+p,{method:b===undefined?'GET':'POST',headers:{Authorization:'Bearer '+process.env.NM_TEST_PHYSICS_TOKEN,'Content-Type':'application/json'},...(b===undefined?{}:{body:JSON.stringify(b)})});return {status:r.status,body:await r.text()};};
    const first=JSON.parse((await request('/state')).body);const pin={world_id:first.world_id,profile_id:first.profile_id,model_digest:first.model_digest,io_digest:first.io_digest};
    const live=new PhysicsSession(pin,request);assert.ok(await live.fetch());await live.replaceVoxels([[2,0,.5]]);const actual=await live.fetch();assert.equal(actual.obstacles['0'].join(','),'2,0,0.5');assert.equal(actual.authority,'MuJoCo-3.17.0-Rust-reference');live.stop();
  }
  clearTimeout(watchdog);console.log(JSON.stringify({status:'pass',scope:'Bedrock-independent session/transform logic; optional real native HTTP, no game-engine or learned-task claim'}));
})().catch(error=>{console.error(error);process.exit(1);});
