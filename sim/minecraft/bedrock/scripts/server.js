import { HttpRequest, HttpRequestMethod, HttpHeader, http } from '@minecraft/server-net';
import { variables, secrets } from '@minecraft/server-admin';
import { content } from './content.generated.js';
import { start } from './runtime.js';
import { startPhysics } from './physics-runtime.js';
const permitted = () => variables.get('AARNN_ALLOW_SANDBOX_INFERENCE') === true &&
  variables.get('AARNN_CONTENT_DIGEST') === content.digest && secrets.get('AARNN_AUTHORIZATION') !== undefined;
const social = () => permitted() && variables.get('AARNN_ALLOW_NAO_CHAT')===true && secrets.get('AARNN_NAO_AUTHORIZATION')!==undefined;
const localPort=(name,fallback)=>{const p=variables.get(name)??fallback;if(typeof p!=='number'||!Number.isInteger(p)||p<1||p>65535)throw Error('Invalid local companion port');return p;};
start({available:permitted,socialAvailable:social,socialRequest:(path,body)=>{
  if(!social() || !['/api/turn','/api/encounter','/api/poll','/api/stop','/api/leave'].includes(path))throw Error('NAO chat disabled');
  const request=new HttpRequest('http://127.0.0.1:'+localPort('AARNN_NAO_PORT',62621)+path);
  request.method=HttpRequestMethod.Post;request.body=JSON.stringify(body);request.timeout=5;
  request.headers=[new HttpHeader('Content-Type','application/json'),new HttpHeader('Authorization',secrets.get('AARNN_NAO_AUTHORIZATION'))];
  return http.request(request);
},request:(body, timeout)=>{
  if (!permitted()) throw Error('BDS inference is disabled or configuration is stale');
  const request = new HttpRequest('http://127.0.0.1:'+localPort('AARNN_INFERENCE_PORT',62620)+'/api/aer/infer');
  request.method = HttpRequestMethod.Post; request.body = JSON.stringify(body); request.timeout = timeout;
  request.headers = [new HttpHeader('Content-Type','application/json'),new HttpHeader('Authorization',secrets.get('AARNN_AUTHORIZATION'))];
  return http.request(request);
}});

// Independent opt-in physics authority route; existing inference/chat grants do
// not grant access to it. Tokens are server-admin secrets, never world data.
startPhysics({
  available:()=>variables.get('AARNN_ALLOW_PHYSICS_SERVICE')===true&&secrets.get('AARNN_PHYSICS_AUTHORIZATION')!==undefined,
  config:()=>({world_id:variables.get('AARNN_PHYSICS_WORLD_ID'),profile_id:variables.get('AARNN_PHYSICS_PROFILE_ID'),model_digest:variables.get('AARNN_PHYSICS_MODEL_DIGEST'),io_digest:variables.get('AARNN_PHYSICS_IO_DIGEST'),origin:[variables.get('AARNN_PHYSICS_ORIGIN_X')??0,variables.get('AARNN_PHYSICS_ORIGIN_Y')??64,variables.get('AARNN_PHYSICS_ORIGIN_Z')??0]}),
  request:(path,body)=>{
    if(variables.get('AARNN_ALLOW_PHYSICS_SERVICE')!==true||!['/model','/state','/command','/stop'].includes(path))throw Error('Physics route disabled');
    const request=new HttpRequest('http://127.0.0.1:'+localPort('AARNN_PHYSICS_PORT',8092)+path);
    request.method=body===undefined?HttpRequestMethod.Get:HttpRequestMethod.Post;request.timeout=2;
    if(body!==undefined)request.body=JSON.stringify(body);
    request.headers=[new HttpHeader('Content-Type','application/json'),new HttpHeader('Authorization',secrets.get('AARNN_PHYSICS_AUTHORIZATION'))];
    return http.request(request);
  }
});
