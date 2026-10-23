#!/usr/bin/env python3
"""Native MuJoCo body/habitat framebuffers and a labelled 28-profile sheet."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
from PIL import Image,ImageDraw
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'scripts'))
from embodiment import NEW,LEGACY


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);args=p.parse_args();args.output=args.output.resolve();args.output.mkdir(parents=True,exist_ok=True)
    sheet=Image.new('RGB',(1400,6*255),(28,35,40));draw=ImageDraw.Draw(sheet);reports=[]
    for i,profile in enumerate(LEGACY+NEW):
        record=json.loads((ROOT/f'sim/content/embodiments/generated/{profile}.json').read_text())
        ppm=args.output/(profile+'.ppm');state=args.output/(profile+'.json');log=args.output/(profile+'.log')
        cmd=[str(ROOT/'target/debug/aarnn-mujoco'),'--robots',profile+'=1','--control','silent','--steps','8','--view','body','--screenshot',str(ppm),'--output',str(state)]
        with log.open('w') as output:subprocess.run(cmd,cwd=ROOT,stdout=output,stderr=subprocess.STDOUT,check=True,timeout=30)
        image=Image.open(ppm);image.save(ppm.with_suffix('.png'));image.thumbnail((275,220));x=(i%5)*280;y=(i//5)*255
        sheet.paste(image,(x+(280-image.width)//2,y));draw.text((x+5,y+222),profile,fill='white');draw.text((x+5,y+236),record['fidelity']+' / appearance only',fill=(178,195,187))
        reports.append(dict(profile_id=profile,model_digest=record['digest'],evidence=str(ppm.with_suffix('.png').relative_to(ROOT)),status='pass',scope='actual native rendered body, not task/calibration acceptance'))
    sheet.save(args.output/'all-28-bodies.png')
    for profile in ('celegans','drosophila_banc','zebrafish','crow_corvus_corone'):
        ppm=args.output/(profile+'-habitat.ppm')
        with (args.output/(profile+'-habitat.log')).open('w') as output:subprocess.run([str(ROOT/'target/debug/aarnn-mujoco'),'--robots',profile+'=1','--control','silent','--steps','8','--screenshot',str(ppm)],cwd=ROOT,stdout=output,stderr=subprocess.STDOUT,check=True,timeout=30)
        Image.open(ppm).save(ppm.with_suffix('.png'))
    (args.output/'rendered.json').write_text(json.dumps(dict(status='pass',scope='native framebuffers, not learned behaviour',profiles=reports),indent=2)+'\n')
