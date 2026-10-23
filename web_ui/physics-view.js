// The service owns dynamics. Projection/interpolation never becomes sensed state.
const $=id=>document.getElementById(id);
let running=false, state=null, model=null, generation=0;
async function request(path,body){
  const response=await fetch(path,{method:body?'POST':'GET',headers:{Authorization:'Bearer '+$('token').value,...(body?{'Content-Type':'application/json'}:{})},...(body?{body:JSON.stringify(body)}:{})});
  const value=await response.json();if(!response.ok)throw Error(value.error||value.reason||'Physics request failed');return value;
}
let renderer=null;
function draw(){
  if(!renderer){const canvas=$('view'),gl=canvas.getContext('webgl',{antialias:true});if(!gl)throw Error('WebGL unavailable');renderer=new window.NmWorld.Renderer(gl,canvas);}
  renderer.setAuthoritativeState(model,state);renderer.render({x:0,z:0,heading:0});
}

async function poll(epoch){
  if(!running||epoch!==generation)return;
  try{
    const next=await request('/state');if(!running||epoch!==generation)return;
    if(state&&next.reset_epoch<state.reset_epoch)throw Error('Stale world epoch');
    if(next.model_digest!==model.digest)throw Error('Model changed; reconnect required');
    state=next;draw();$('status').textContent=JSON.stringify({profile:model.profile_id,habitat:model.scene?.title,fidelity:model.fidelity,geometry:model.geoms.some(g=>g.type===7)?'imported mesh bounds are visual proxies; physics uses native model':'native analytic primitives',variant:model.variant_id,brain:state.brain_id,authority:state.authority,quality:state.quality,controller:state.controller_mode,npc_policy:state.controller_mode==='programmed-NPC'?state.npc_policy:undefined,epoch:state.reset_epoch,step:state.step,disarmed:state.disarmed,energy_J:state.energy_J},null,2);
    setTimeout(()=>poll(epoch),100);
  }catch(error){running=false;$('status').textContent=error.message;}
}
$('connect').onclick=async()=>{try{model=await request('/model');state=null;running=true;poll(++generation);}catch(error){$('status').textContent=error.message;}};
$('detach').onclick=()=>{running=false;generation++;$('status').textContent='View detached; the authorised world continues.';};
$('stop').onclick=async()=>{try{await request('/stop',{});}catch(error){$('status').textContent=error.message;}};
for(const button of document.querySelectorAll('[data-action]'))button.onclick=async()=>{
  if(!state)return;
  const body={action:button.dataset.action,reset_epoch:state.reset_epoch,model_digest:model.digest};
  if(body.action==='obstacle'){body.slot=0;body.position=$('position').value.split(',').map(Number);}
  try{await request('/command',body);}catch(error){$('status').textContent=error.message;}
};
