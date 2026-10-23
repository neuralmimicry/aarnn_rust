/* Native service observer/environment client. No local body dynamics or AER. */
export class PhysicsSession {
  constructor(config,request) {
    for(const key of ['world_id','profile_id','model_digest','io_digest'])if(typeof config[key]!=='string'||!config[key])throw Error('Pinned physics identities required');
    this.config=config;this.request=request;this.active=true;this.pending=false;this.model=null;this.state=null;this.reset=null;this.generation=0;
  }
  async call(path,body) {
    const reply=await this.request(path,body);
    if(![200,202].includes(reply.status)||typeof reply.body!=='string'||reply.body.length>(path==='/model'?8388608:1048576))throw Error('Physics service denied/unavailable/budget');
    return JSON.parse(reply.body);
  }
  command(action){return {action,model_digest:this.config.model_digest,reset_epoch:this.reset};}
  async fetch() {
    if(!this.active||this.pending)return null;
    this.pending=true;const generation=this.generation;
    try {
      if(!this.model){const m=await this.call('/model');if(m.digest!==this.config.model_digest||m.profile_id!==this.config.profile_id||m.io?.digest!==this.config.io_digest||!Array.isArray(m.geoms)||m.geoms.length>1024)throw Error('Pinned physics model/IO mismatch');this.model=m;}
      const s=await this.call('/state');if(!this.active||generation!==this.generation)return null;
      const c=this.config,p=this.state;
      if(s.world_id!==c.world_id||s.profile_id!==c.profile_id||s.model_digest!==c.model_digest||s.io_digest!==c.io_digest||
        !Number.isSafeInteger(s.reset_epoch)||s.reset_epoch<0||!Number.isSafeInteger(s.step)||s.step<0||s.time_us!==s.step*2000||
        !Number.isSafeInteger(s.publication)||s.publication<0||!Number.isSafeInteger(s.world_config_epoch)||
        this.reset!==null&&s.reset_epoch!==this.reset||p&&(s.step<p.step||s.publication<p.publication)||!Array.isArray(s.poses)||s.poses.length>1024)throw Error('Stale/reset/mismatched physics pose; explicit reattach required');
      for(const pose of s.poses)if(typeof pose.name!=='string'||![pose.position,pose.quaternion_wxyz].every((v,i)=>Array.isArray(v)&&v.length===(i?4:3)&&v.every(Number.isFinite)))throw Error('Malformed native pose');
      this.reset=s.reset_epoch;this.state=s;return s;
    }catch(error){this.stop();throw error;}finally{this.pending=false;}
  }
  async replaceVoxels(voxels) {
    if(!this.active||this.pending||this.reset===null||!Array.isArray(voxels)||voxels.length>32||voxels.some(p=>!Array.isArray(p)||p.length!==3||p.some(x=>!Number.isFinite(x)||Math.abs(x)>10)))throw Error('Collision scene credit/capacity');
    this.pending=true;const generation=this.generation;
    const ensure=()=>{if(!this.active||this.generation!==generation)throw Error('Scene cancelled');};
    try {
      const pause=await this.call('/command',this.command('pause'));ensure();
      await this.call('/command',{...this.command('collision_set'),world_config_epoch:pause.world_config_epoch,voxels});ensure();
      await this.call('/command',this.command('resume'));
    }catch(error){this.stop();throw error;}finally{this.pending=false;}
  }
  stop(){if(!this.active)return;this.active=false;this.generation++;this.call('/stop',{}).catch(()=>{});if(this.reset!==null)this.call('/command',this.command('pause')).catch(()=>{});}
}
// Canonical X-forward/Y-left/Z-up, SI -> game X-east/Y-up/Z-south, block=metre.
export const gamePoint=p=>[p[0],p[2],-p[1]];
export function multiply(a,b){return [a[0]*b[0]-a[1]*b[1]-a[2]*b[2]-a[3]*b[3],a[0]*b[1]+a[1]*b[0]+a[2]*b[3]-a[3]*b[2],a[0]*b[2]-a[1]*b[3]+a[2]*b[0]+a[3]*b[1],a[0]*b[3]+a[1]*b[2]-a[2]*b[1]+a[3]*b[0]];}
export function rotate(q,p){return multiply(multiply(q,[0,...p]),[q[0],-q[1],-q[2],-q[3]]).slice(1);}
export function gameEuler(q){const [w,x,canonicalY,canonicalZ]=q,y=canonicalZ,z=-canonicalY;const s=Math.max(-1,Math.min(1,2*(w*y-z*x)));return [Math.atan2(2*(w*x+y*z),1-2*(x*x+y*y)),Math.asin(s),Math.atan2(2*(w*z+x*y),1-2*(y*y+z*z))].map(x=>x*180/Math.PI);}
