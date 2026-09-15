/* Shared habitat renderer and bounded reference sensor adapter.
 * Coordinates: X forward, Y left, Z up. This is a kinematic sandbox, not CFD. */
(function (root) {
  "use strict";
  var TAU = Math.PI * 2;
  function clamp(v, lo, hi) { return Math.max(lo, Math.min(hi, v)); }
  function dot(a,b) { return a[0]*b[0]+a[1]*b[1]+a[2]*b[2]; }
  function sub(a,b) { return a.map(function(v,i){return v-b[i];}); }
  function cross(a,b) { return [a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]]; }
  function unit(a) { var n=Math.sqrt(dot(a,a))||1; return a.map(function(v){return v/n;}); }
  function mul(a,b) { var c=new Float32Array(16); for(var j=0;j<4;j++) for(var i=0;i<4;i++) for(var k=0;k<4;k++) c[j*4+i]+=a[k*4+i]*b[j*4+k]; return c; }
  function view(eye,target) { var z=unit(sub(eye,target)),x=unit(cross([0,0,1],z)),y=cross(z,x); return new Float32Array([x[0],y[0],z[0],0,x[1],y[1],z[1],0,x[2],y[2],z[2],0,-dot(x,eye),-dot(y,eye),-dot(z,eye),1]); }
  function perspective(aspect) { var f=1/Math.tan(.55),n=.015,z=30; return new Float32Array([f/aspect,0,0,0,0,f,0,0,0,0,(z+n)/(n-z),-1,0,0,2*z*n/(n-z),0]); }
  function transform(x,y,z,yaw,scale) {var c=Math.cos(yaw)*scale,s=Math.sin(yaw)*scale;return new Float32Array([c,s,0,0,-s,c,0,0,0,0,scale,0,x,y,z,1]);}

  function unitTriangles(shape) {
    var triangles=[];
    function tri(a,b,c){triangles.push(a,b,c);}
    if(shape==='box') {
      [[0,1,2],[0,2,3],[4,6,5],[4,7,6],[0,4,5],[0,5,1],[1,5,6],[1,6,2],[2,6,7],[2,7,3],[3,7,4],[3,4,0]].forEach(function(f){
        var p=[[-.5,-.5,-.5],[.5,-.5,-.5],[.5,.5,-.5],[-.5,.5,-.5],[-.5,-.5,.5],[.5,-.5,.5],[.5,.5,.5],[-.5,.5,.5]];tri(p[f[0]],p[f[1]],p[f[2]]);
      });
    } else if(shape==='cylinder') {
      for(var i=0;i<16;i++){var a=TAU*i/16,b=TAU*(i+1)/16;var p=[.5*Math.cos(a),.5*Math.sin(a),-.5],q=[.5*Math.cos(b),.5*Math.sin(b),-.5],r=[q[0],q[1],.5],s=[p[0],p[1],.5];tri(p,q,r);tri(p,r,s);tri([0,0,.5],s,r);tri([0,0,-.5],q,p);}
    } else {
      function point(lat,lon){return [.5*Math.cos(lat)*Math.cos(lon),.5*Math.cos(lat)*Math.sin(lon),.5*Math.sin(lat)];}
      for(var j=0;j<8;j++)for(var k=0;k<12;k++){var l=-Math.PI/2+j*Math.PI/8,m=l+Math.PI/8,u=k*TAU/12,v=(k+1)*TAU/12;tri(point(l,u),point(l,v),point(m,v));tri(point(l,u),point(m,v),point(m,u));}
    }
    return triangles;
  }
  var SHAPES={box:unitTriangles('box'),sphere:unitTriangles('sphere'),cylinder:unitTriangles('cylinder')};
  function geometry(objects, anatomy, actuators, kind, muscleChannels) {
    var data=[];
    objects.forEach(function(o){
      if(o.internal&&!anatomy)return;
      if(anatomy&&((kind==='worm'&&o.id.indexOf('cuticle_')===0)||(kind==='fish'&&o.id.indexOf('myomere_')===0)))return;
      var bend=0;
      if(actuators&&o.anchor.indexOf('segment_')===0){var s=Number(o.anchor.slice(8)); if(kind==='worm'){var ids=muscleChannels[s];bend=((actuators[ids[0]]||0)+(actuators[ids[1]]||0)-(actuators[ids[2]]||0)-(actuators[ids[3]]||0))*.025;} else bend=((actuators[(s%8)*2]||0)-(actuators[(s%8)*2+1]||0))*.035;}
      var c=Math.cos(o.yaw),sine=Math.sin(o.yaw),base=SHAPES[o.shape];
      for(var i=0;i<base.length;i+=3){var vertices=[];
        for(var j=0;j<3;j++){var p=base[i+j],x=p[0]*o.size[0],y=p[1]*o.size[1];vertices.push([o.position[0]+x*c-y*sine,o.position[1]+x*sine+y*c+bend,o.position[2]+p[2]*o.size[2]]);}
        var normal=unit(cross(sub(vertices[1],vertices[0]),sub(vertices[2],vertices[0])));
        // Two-sided thin membranes and shared diffuse light keep small anatomy legible.
        var light=o.material==='emissive'?1: .44+.56*Math.abs(dot(normal,unit([-.35,-.5,1])));
        vertices.forEach(function(p){data.push(p[0],p[1],p[2],o.colour[0]*light,o.colour[1]*light,o.colour[2]*light);});
      }
    });
    return new Float32Array(data);
  }
  function field(habitat,kind,p) {
    var value=0;
    habitat.objects.forEach(function(o){if(o.cue!==kind||o.radius<=0)return;var d=sub(p,o.position),r2=dot(d,d)/(o.radius*o.radius);value+=o.strength/(1+r2*4);});
    return clamp(value,0,1);
  }
  // Activity-to-motion reference only. Never infer paired muscles from parity
  // of an index: worm outputs are grouped by named quadrant and omit MVL24.
  function motion(profile, actuators) {
    var left=0,right=0,count=0;
    if(profile.kind==='worm') {
      profile.muscle_channels.forEach(function(ids){
        ids.forEach(function(id,q){if(id<0)return;var value=actuators[id]||0;
          if(q===0||q===2)left+=value;else right+=value;count++;});
      });
    } else if(profile.kind==='fish') {
      for(var i=0;i<16;i++){if(i%2)right+=actuators[i]||0;else left+=actuators[i]||0;count++;}
    } else if(profile.kind==='hexapod') {
      for(var j=0;j<18;j++){if(j<9)left+=actuators[j]||0;else right+=actuators[j]||0;count++;}
    } else {
      // Imported fly readouts have no validated muscle-to-joint map. Aggregate
      // activity can advance this preview; it cannot establish a biological gait.
      actuators.forEach(function(v){left+=v*.5;right+=v*.5;count++;});
    }
    return {drive:(left+right)/Math.max(1,count),turn:(right-left)/Math.max(1,count)};
  }
  function ray(habitat, origin, direction, range) {
    var best=range,colour=habitat.substrate;
    habitat.objects.forEach(function(o){
      if(o.internal||o.material==='water')return;
      var c=Math.cos(o.yaw),s=Math.sin(o.yaw),d=sub(origin,o.position);
      var p=[d[0]*c+d[1]*s,-d[0]*s+d[1]*c,d[2]],v=[direction[0]*c+direction[1]*s,-direction[0]*s+direction[1]*c,direction[2]];
      var t0=0,t1=best;
      // Conservative box rays are an explicit proximity/vision proxy for all shapes.
      for(var a=0;a<3;a++){var half=o.size[a]*.5;if(Math.abs(v[a])<1e-9){if(Math.abs(p[a])>half)return;}else{var x=(-half-p[a])/v[a],y=(half-p[a])/v[a];t0=Math.max(t0,Math.min(x,y));t1=Math.min(t1,Math.max(x,y));if(t1<t0)return;}}
      if(t0>1e-5&&t0<best){best=t0;colour=o.colour;}
    });
    return {distance:best,luminance:colour[0]*.2126+colour[1]*.7152+colour[2]*.0722};
  }
  // Engine adapters pass bounded current actor bounds in habitat coordinates.
  // This routes nonverbal interaction through the same visual/proximity/contact
  // sensors as other scene objects, preserving every profile's existing I/O.
  function withParticipants(habitat, participants) {
    var objects=habitat.objects.slice();
    (participants||[]).slice(0,64).forEach(function (p) {
      if(!p || typeof p.id!=='string' || !Array.isArray(p.position) || !Array.isArray(p.size) || p.position.length!==3 || p.size.length!==3 || !p.position.every(Number.isFinite) || !p.size.every(function(v){return Number.isFinite(v)&&v>0;}))return;
      objects.push({id:'participant_'+p.id,shape:'box',position:p.position.slice(),size:p.size.slice(),colour:[.25,.65,.85],yaw:0,internal:false,material:'participant',cue:'',radius:0,strength:0,anchor:'root',solid:true});
    });
    return Object.assign({},habitat,{objects:objects});
  }
  function sense(profile, habitat, robot, previous) {
    if(robot.participants)habitat=withParticipants(habitat,robot.participants);
    var values=new Array(profile.sensory).fill(0), names=profile.sensor_names;
    var p=[robot.x,robot.z,profile.body_height],heading=robot.heading;
    function probe(angle,elevation,range){var a=heading+angle;return ray(habitat,p,[Math.cos(a)*Math.cos(elevation),Math.sin(a)*Math.cos(elevation),Math.sin(elevation)],range||.6);}
    function paired(fieldName,side){var a=heading+Math.PI/2;return field(habitat,fieldName,[p[0]+Math.cos(a)*side*.04,p[1]+Math.sin(a)*side*.04,p[2]]);}
    names.forEach(function(name,i){
      if(/accel|gyro/.test(name)){values[i]=.5;return;}
      if(/coxa|femur|tibia/.test(name)){values[i]=.5+(robot.actuators[i]||0)*.35;return;}
      if(/\.on\.|\.off\./.test(name)){
        var match=/r(\d+)c(\d+)/.exec(name),r=match?Number(match[1]):0,c=match?Number(match[2]):0;
        var cols=profile.kind==='fly'?12:1,rows=profile.kind==='fly'?8:1;
        var side=name.indexOf('right')>=0?-1:1,key=(side>0?'l':'r')+r+'_'+c;
        var lum=probe(side*.6+((c+.5)/cols-.5)*1.7,(.5-(r+.5)/rows)*1.15,2).luminance;
        var delta=previous[key]===undefined?0:lum-previous[key];
        values[i]=clamp((name.indexOf('.on.')>=0?delta:-delta)*4,0,1);
        if(name.indexOf('.off.')>=0)previous[key]=lum;
      }else if(/eye_.*_(lum|grad)/.test(name)){
        var eyeSide=name.indexOf('right')>=0?-1:1,eyeKey='fishEye'+eyeSide;
        var eyeLum=probe(eyeSide*.8,0,2).luminance;
        values[i]=name.endsWith('_lum')?eyeLum:.5+clamp((eyeLum-(previous[eyeKey]===undefined?eyeLum:previous[eyeKey]))*5,-1,1)*.5;
        if(name.endsWith('_grad'))previous[eyeKey]=eyeLum;
      }else if(/chem|taste|olfactory/.test(name)){values[i]=paired('chemical',/right|_r\b/.test(name)?-1:1);}
      else if(/heat/.test(name)){values[i]=paired('heat',name.indexOf('right')>=0?-1:1);}
      else if(/light|eye_.*lum/.test(name)){values[i]=paired('light',name.indexOf('right')>=0?-1:1);}
      else if(/flow/.test(name)){values[i]=field(habitat,'flow',p);}
      else if(/depth/.test(name)){var water=habitat.objects.find(function(o){return o.id==='water_volume';});values[i]=water?clamp((water.position[2]+water.size[2]/2-p[2])/water.size[2],0,1):0;}
      else if(/pitch/.test(name)){values[i]=.5;}
      else if(/foot/.test(name)){values[i]=1;}
      else if(/touch|prox|far|lateralline|ultrasonic/.test(name)){
        var angle=/rear|posterior/.test(name)?Math.PI:0;
        if(/left|lateralline_l/.test(name))angle+=Math.PI/2;
        if(/right|lateralline_r/.test(name))angle-=Math.PI/2;
        var prox=/prox_(\d+)/.exec(name);if(prox)angle=Number(prox[1])*TAU/24;
        var hit=probe(angle,0,.6);values[i]=/touch/.test(name)?(hit.distance<profile.body_length*.55?1:0):1-hit.distance/.6;
      }
    });
    if(profile.kind==='nao'){
      values[0]=1-probe(.25,0).distance/.6;values[1]=1-probe(-.25,0).distance/.6;
      for(var i=2;i<8;i++)values[i]=.5;
      for(var j=23;j<49;j++)values[j]=.5+(robot.actuators[j-23]||0)*.35;
      for(var eye=0;eye<2;eye++)for(var px=0;px<48;px++){
        var key='nao'+eye+'_'+px,lum=probe((eye===0?.15:-.15)+(px%8/7-.5)*1.3,(.5-Math.floor(px/8)/5)*1.0,2).luminance;
        var diff=previous[key]===undefined?0:lum-previous[key];values[58+eye*96+px]=clamp(diff*4,0,1);values[106+eye*96+px]=clamp(-diff*4,0,1);previous[key]=lum;
      }
    }
    return values;
  }

  function World(gl,canvas) {
    this.gl=gl;this.canvas=canvas;this.yaw=-1.25;this.pitch=.83;this.distance=2.65;this.anatomy=false;this.focusRobot=false;
    var vs='attribute vec3 p;attribute vec3 c;uniform mat4 m;varying vec3 v;void main(){gl_Position=m*vec4(p,1.0);v=c;}';
    var fs='precision mediump float;varying vec3 v;uniform float opacity;void main(){gl_FragColor=vec4(v,opacity);}';
    function shader(type,source){var s=gl.createShader(type);gl.shaderSource(s,source);gl.compileShader(s);if(!gl.getShaderParameter(s,gl.COMPILE_STATUS))throw Error(gl.getShaderInfoLog(s));return s;}
    this.program=gl.createProgram();gl.attachShader(this.program,shader(gl.VERTEX_SHADER,vs));gl.attachShader(this.program,shader(gl.FRAGMENT_SHADER,fs));gl.linkProgram(this.program);
    if(!gl.getProgramParameter(this.program,gl.LINK_STATUS))throw Error(gl.getProgramInfoLog(this.program));
    this.pos=gl.getAttribLocation(this.program,'p');this.col=gl.getAttribLocation(this.program,'c');this.matrix=gl.getUniformLocation(this.program,'m');
    this.opacity=gl.getUniformLocation(this.program,'opacity');
    this.scene=gl.createBuffer();this.water=gl.createBuffer();this.body=gl.createBuffer();this.participants=gl.createBuffer();this.bodyDirty=true;
    var self=this,drag=null;canvas.style.touchAction='none';
    canvas.addEventListener('pointerdown',function(e){drag=[e.clientX,e.clientY];canvas.setPointerCapture(e.pointerId);});
    canvas.addEventListener('pointermove',function(e){if(!drag)return;self.yaw-=(e.clientX-drag[0])*.007;self.pitch=clamp(self.pitch+(e.clientY-drag[1])*.007,.15,1.48);drag=[e.clientX,e.clientY];});
    canvas.addEventListener('pointerup',function(){drag=null;});canvas.addEventListener('pointercancel',function(){drag=null;});
    canvas.addEventListener('wheel',function(e){e.preventDefault();self.distance=clamp(self.distance*Math.exp(e.deltaY*.001),.35,5);},{passive:false});
    canvas.addEventListener('keydown',function(e){if(e.key==='ArrowLeft')self.yaw-=.1;else if(e.key==='ArrowRight')self.yaw+=.1;else if(e.key==='+')self.distance=Math.max(.35,self.distance*.9);else if(e.key==='-')self.distance=Math.min(5,self.distance*1.1);else return;e.preventDefault();});
  }
  World.prototype.setProfile=function(profile,habitat){this.profile=profile;this.habitat=habitat;var data=geometry(habitat.objects.filter(function(o){return o.material!=='water';}),false);this.gl.bindBuffer(this.gl.ARRAY_BUFFER,this.scene);this.gl.bufferData(this.gl.ARRAY_BUFFER,data,this.gl.STATIC_DRAW);this.sceneCount=data.length/6;data=geometry(habitat.objects.filter(function(o){return o.material==='water';}),false);this.gl.bindBuffer(this.gl.ARRAY_BUFFER,this.water);this.gl.bufferData(this.gl.ARRAY_BUFFER,data,this.gl.STATIC_DRAW);this.waterCount=data.length/6;this.bodyDirty=true;};
  World.prototype.render=function(robot){
    var gl=this.gl,c=this.canvas,scale=Math.min(2,root.devicePixelRatio||1),w=Math.max(320,Math.floor(c.clientWidth*scale)),h=Math.max(240,Math.floor(c.clientHeight*scale));
    if(c.width!==w||c.height!==h){c.width=w;c.height=h;}gl.viewport(0,0,w,h);gl.clearColor(.055,.082,.095,1);gl.clear(gl.COLOR_BUFFER_BIT|gl.DEPTH_BUFFER_BIT);gl.enable(gl.DEPTH_TEST);gl.useProgram(this.program);
    var target=this.focusRobot?[robot.x,robot.z,this.profile.body_height+(this.profile.kind==='nao'?this.profile.body_length*.5:0)]:[0,0,.13],eye=[target[0]+Math.cos(this.yaw)*Math.cos(this.pitch)*this.distance,target[1]+Math.sin(this.yaw)*Math.cos(this.pitch)*this.distance,target[2]+Math.sin(this.pitch)*this.distance];
    var vp=mul(perspective(w/h),view(eye,target)),self=this;
    function draw(buffer,count,m){gl.bindBuffer(gl.ARRAY_BUFFER,buffer);gl.enableVertexAttribArray(self.pos);gl.vertexAttribPointer(self.pos,3,gl.FLOAT,false,24,0);gl.enableVertexAttribArray(self.col);gl.vertexAttribPointer(self.col,3,gl.FLOAT,false,24,12);gl.uniformMatrix4fv(self.matrix,false,m);gl.drawArrays(gl.TRIANGLES,0,count);}
    gl.uniform1f(this.opacity,1);draw(this.scene,this.sceneCount,vp);
    if(robot.participants && robot.participants.length){var actors=geometry(withParticipants({objects:[]},robot.participants).objects,false);gl.bindBuffer(gl.ARRAY_BUFFER,this.participants);gl.bufferData(gl.ARRAY_BUFFER,actors,gl.DYNAMIC_DRAW);draw(this.participants,actors.length/6,vp);}
    if(this.bodyDirty){var data=geometry(this.profile.parts,this.anatomy,robot.actuators,this.profile.kind,this.profile.muscle_channels);gl.bindBuffer(gl.ARRAY_BUFFER,this.body);gl.bufferData(gl.ARRAY_BUFFER,data,gl.DYNAMIC_DRAW);this.bodyCount=data.length/6;this.bodyDirty=false;}
    draw(this.body,this.bodyCount,mul(vp,transform(robot.x,robot.z,this.profile.body_height,robot.heading,this.profile.body_length)));
    if(this.waterCount){gl.enable(gl.BLEND);gl.blendFunc(gl.SRC_ALPHA,gl.ONE_MINUS_SRC_ALPHA);gl.depthMask(false);gl.uniform1f(this.opacity,.18);draw(this.water,this.waterCount,vp);gl.depthMask(true);gl.disable(gl.BLEND);}
  };
  root.NmWorld={Renderer:World,sense:sense,field:field,ray:ray,geometry:geometry,motion:motion,withParticipants:withParticipants};
})(typeof window!=='undefined'?window:globalThis);
