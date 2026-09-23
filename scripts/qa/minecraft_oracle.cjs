/* Produce independent Java adapter oracles using the maintained WebGL adapter. */
const fs = require('node:fs');
require('../../web_ui/sim-content.generated.js');
require('../../web_ui/webgl-world.js');
const content = globalThis.NmSimContent, world = globalThis.NmWorld;
const cases = content.profiles.map(p => {
  const h = content.habitats.find(h => h.id === p.habitat), history = {};
  const outputs = Array.from({length:p.output}, (_,i) => (i%7)/7);
  const poses = [[0,0,0],[0,0,0],[0,0,Math.PI/2],[.42,.2,.7],[-.9,.9,2.4]];
  return {
    profile:p.id, motion:world.motion(p,outputs), outputs,
    frames:poses.map(([x,y,heading]) => ({x,y,heading,values:world.sense(p,h,{x,z:y,heading,actuators:outputs},history)})),
    meshes:[false,true].map(anatomy => {
      const mesh = world.geometry(p.parts,anatomy,outputs,p.kind,p.muscle_channels,p.output_names);
      const indices = Array.from({length:1000},(_,i)=>Math.floor(i*(mesh.length-1)/999));
      return {anatomy,count:mesh.length,indices,values:indices.map(i=>mesh[i])};
    })
  };
});
fs.mkdirSync('sim/minecraft/build',{recursive:true});
fs.writeFileSync('sim/minecraft/build/reference.json',JSON.stringify({digest:content.digest,cases}));
console.log('Minecraft reference fixtures: six profiles, 30 sensory frames, 12 anatomy meshes.');
