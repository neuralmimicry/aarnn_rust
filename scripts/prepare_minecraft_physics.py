#!/usr/bin/env python3
"""Prepare private, reviewable Java/BDS viewer configuration; no game mutation."""
import argparse
import json
import os
from pathlib import Path
import urllib.request

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--port',type=int,default=8092);p.add_argument('--token-file',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--origin',default='0,64,0');a=p.parse_args()
    origin=[float(x) for x in a.origin.split(',')]
    import math
    if len(origin)!=3 or not all(math.isfinite(x) for x in origin) or not 1<=a.port<=65535:p.error('finite XYZ origin and port required')
    if a.token_file.stat().st_mode&0o077:p.error('token file must be private (0600)')
    token=a.token_file.read_text().strip()
    url='http://127.0.0.1:'+str(a.port)
    request=urllib.request.Request(url+'/state',headers={'Authorization':'Bearer '+token})
    with urllib.request.urlopen(request,timeout=3) as response:state=json.loads(response.read(1024*1024))
    a.output=a.output.resolve();a.output.mkdir(parents=True,exist_ok=True)
    def private(name,value):
        path=a.output/name
        with path.open('x',opener=lambda path,flags:os.open(path,flags,0o600)) as f:f.write(json.dumps(value,indent=2)+'\n')
    pins={k:state[k] for k in ['world_id','profile_id','model_digest','io_digest']}
    private('java-physics.json',dict(schema_version=1,endpoint=url,token_environment='AARNN_PHYSICS_TOKEN',origin=origin,**pins))
    variables=dict(AARNN_ALLOW_PHYSICS_SERVICE=True,AARNN_PHYSICS_PORT=a.port,AARNN_PHYSICS_ORIGIN_X=origin[0],AARNN_PHYSICS_ORIGIN_Y=origin[1],AARNN_PHYSICS_ORIGIN_Z=origin[2])
    variables.update({'AARNN_PHYSICS_'+k.upper():v for k,v in pins.items()})
    private('bedrock-physics-variables.json',variables)
    private('bedrock-physics-secrets.json',dict(AARNN_PHYSICS_AUTHORIZATION='Bearer '+token))
    print('Private viewer configuration prepared in '+str(a.output)+'. Java uses AARNN_PHYSICS_CONFIG and AARNN_PHYSICS_TOKEN. Merge the BDS fields with its existing private variables/secrets; preserve independent grants. No game files were changed.')
if __name__=='__main__':main()
