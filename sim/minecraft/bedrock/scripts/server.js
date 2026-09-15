import { HttpRequest, HttpRequestMethod, HttpHeader, http } from '@minecraft/server-net';
import { variables, secrets } from '@minecraft/server-admin';
import { content } from './content.generated.js';
import { start } from './runtime.js';
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
