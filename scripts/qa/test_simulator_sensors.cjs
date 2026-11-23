// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
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
// Brain-output decode, vectors replicated from crates/aarnn-embodiment
// (actuation.rs graded_position_readout_spans_the_full_joint_range,
// celegans.rs pulse_density_code_decodes_linearly) at the reference 2 ms frame.
{
 const hex=c.profiles.find(p=>p.id==='hexapod'),femur=hex.output_names.indexOf('hex_o_001_lf_femur'),lo=-1.4,hi=-.2;
 assert.equal(femur,1);
 for(const density of [0,.5,1]){
  const a=Array(hex.output).fill(0);let phase=0;
  for(let step=1;step<=400;step++){phase+=density;const events=[];if(phase>=1-1e-12){phase-=1;events.push(femur);}w.decodeOutputs(hex,a,events,'pulse_density',2);}
  const target=lo+Math.max(0,Math.min(1,a[femur]))*(hi-lo),expected=lo+density*(hi-lo);
  assert.ok(Math.abs(target-expected)<.08*(hi-lo),'density '+density+': '+target+' vs '+expected);
 }
 const mdr=worm.output_names.findIndex(n=>n.endsWith('MDR05'));
 for(const [p,floor] of [[.2,.6],[.5,.8]]){
  const graded=Array(worm.output).fill(0),spiking=Array(worm.output).fill(0);let phase=0,sg=0,ss=0,n=0;
  for(let f=0;f<5000;f++){phase+=p;const events=[];if(phase>=1){phase-=1;events.push(mdr);}
   w.decodeOutputs(worm,graded,events,'pulse_density',2);w.decodeOutputs(worm,spiking,events,undefined,2);
   if(f>=2500){sg+=graded[mdr];ss+=spiking[mdr];n++;}}
  assert.ok(Math.abs(sg/n-p)<.01,'p '+p+': '+sg/n);
  assert.ok(ss/n>floor,'spike display compresses: '+ss/n);
 }
 const one=Array(worm.output).fill(0);w.decodeOutputs(worm,one,[mdr],'pulse_density',2);
 assert.ok(Math.abs(one[mdr]-(1-Math.exp(-2/55)))<1e-15,'C. elegans graded filter uses the 55 ms muscle constant');
 const spike=[0,.5];w.decodeOutputs({kind:'fish'},spike,[0],'spikes',1);
 assert.deepEqual(spike,[1,.5*.82],'spiking display is unchanged');
 assert.equal(w.outputCode(undefined),'spikes');assert.equal(w.outputCode(null),'spikes');
 assert.equal(w.outputCode('pulse_density'),'pulse_density');
 assert.throws(()=>w.outputCode('analog'));assert.throws(()=>w.decodeOutputs(hex,Array(18).fill(0),[],'analog',1));
}
console.log('Output-code negotiation and pulse-density decode fixtures passed.');
