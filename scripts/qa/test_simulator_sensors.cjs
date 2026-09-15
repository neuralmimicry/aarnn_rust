/* Spatial/transducer fixtures: fixed geometry, no network or wall-clock oracle. */
const assert=require('node:assert/strict');
require('../../web_ui/sim-content.generated.js');
require('../../web_ui/webgl-world.js');
const c=globalThis.NmSimContent, w=globalThis.NmWorld;
const synthetic={substrate:[0,0,0],objects:[{id:'target',shape:'box',position:[.5,0,0],size:[.1,.2,.2],colour:[1,1,1],yaw:0,cue:'chemical',radius:.5,strength:1}]};
assert.ok(Math.abs(w.ray(synthetic,[0,0,0],[1,0,0],1).distance-.45)<1e-12);
assert.equal(w.ray(synthetic,[0,0,0],[-1,0,0],1).distance,1);
assert.equal(w.field(synthetic,'chemical',[.5,0,0]),1);
assert.ok(Math.abs(w.field(synthetic,'chemical',[0,0,0])-.2)<1e-12);
assert.equal(w.field(synthetic,'heat',[.5,0,0]),0);
for(const p of c.profiles){
 const h=c.habitats.find(h=>h.id===p.habitat),history={};
 const robot={x:0,z:0,heading:0,actuators:Array(p.output).fill(0)};
 const first=w.sense(p,h,robot,history),still=w.sense(p,h,robot,history);
 assert.equal(first.length,p.sensory);assert.deepEqual(first,still);
 assert.ok(first.every(x=>Number.isFinite(x)&&x>=0&&x<=1));
 for(const cutaway of [false,true]){const mesh=w.geometry(p.parts,cutaway,robot.actuators,p.kind,p.muscle_channels);assert.equal(mesh.length%18,0);assert.ok(mesh.length>0&&mesh.length<2000000);assert.ok(mesh.every(Number.isFinite));}
}
const worm=c.profiles.find(p=>p.id==='celegans'),agar=c.habitats.find(h=>h.id==='agar');
const near=w.sense(worm,agar,{x:.42,z:.2,heading:0,actuators:[]},{});
const far=w.sense(worm,agar,{x:-.9,z:.9,heading:0,actuators:[]},{});
assert.ok(near[12]>far[12]+.2,'food location must alter chemical input');
const fly=c.profiles.find(p=>p.id==='drosophila_banc'),orchard=c.habitats.find(h=>h.id==='orchard');
const history={},pose={x:0,z:0,heading:0,actuators:[]};
w.sense(fly,orchard,pose,history);pose.heading=Math.PI/2;
const turned=w.sense(fly,orchard,pose,history);
assert.ok(turned.slice(34).some(x=>x>0),'turning past actual landmarks must produce retinal events');
assert.ok(w.sense(fly,orchard,pose,history).slice(34).every(x=>x===0),'stationary retina must settle');
// MVULVA is a separate readout and must never drive preview body locomotion.
const muscleOnly=Array(worm.output).fill(0);
muscleOnly[worm.output_names.indexOf('celegans_o_095_MVULVA')]=1;
assert.deepEqual(w.motion(worm,muscleOnly),{drive:0,turn:0});
muscleOnly[worm.muscle_channels[0][0]]=1;
assert.ok(w.motion(worm,muscleOnly).drive>0);
assert.ok(w.motion(worm,muscleOnly).turn<0);
console.log('Spatial field, proximity, canonical dimensions, retinal history and bounded geometry fixtures passed.');
// Every profile receives a participant through its existing spatial sensors.
for(const p of c.profiles){
 const base={objects:[],substrate:[.1,.1,.1]},robot={x:0,z:0,heading:0,actuators:[]},history={};
 const before=w.sense(p,base,robot,history);
 robot.participants=[{id:'npc',position:[.056,0,p.body_height],size:[.1,.1,.5]}];
 const after=w.sense(p,base,robot,history);
 assert.ok(after.some((v,i)=>Math.abs(v-before[i])>1e-8),p.id+' must sense a nearby participant');
 assert.equal(after.length,p.sensory);
 assert.equal(w.withParticipants(base,Array(100).fill(robot.participants[0])).objects.length,64);
 assert.equal(w.withParticipants(base,[{id:'bad',position:[NaN,0,0],size:[1,1,1]}]).objects.length,0);
}
