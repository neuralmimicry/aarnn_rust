#!/usr/bin/env python3
"""Actual preserved-network neural adapters; retain failures and continue profiles."""
import argparse
import json
from pathlib import Path
from probe_mujoco_managed import probe,digest,ROOT


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--profiles',nargs='+',default=['hexapod','nao','zebrafish','drosophila_banc','drosophila_fafb','celegans'])
    args=parser.parse_args();args.output=args.output.resolve();args.output.mkdir(parents=True,exist_ok=True)
    paths=[ROOT/('network_'+p+'.json') for p in args.profiles]+[ROOT/'webots_world/worlds/multi_neuroworld.wbt',ROOT/'webots_world/worlds/.multi_neuroworld.wbproj']
    original={str(p.relative_to(ROOT)):digest(p) for p in paths if p.exists()}
    rows={}
    for profile in args.profiles:
        dest=args.output/profile;dest.mkdir(exist_ok=True)
        try:
            result=probe(dest,profile=profile,workers=[1])
            (dest/'managed.json').write_text(json.dumps(result,indent=2)+'\n')
            row=dict(status='pass',evidence=str(dest/'managed.json'),scope='real preserved network AER1, measured effort, zero NPC; no learned task claim')
        except Exception as error:
            row=dict(status='fail',evidence=str(dest),reason=type(error).__name__+': '+str(error),scope='actual native managed startup/exchange; failure retained')
        model=json.loads((ROOT/f'sim/content/embodiments/generated/{profile}.json').read_text())
        row.update(model_digest=model['digest'],io_digest=model['io']['digest'])
        rows[profile]=row
        (args.output/'legacy-neural.json').write_text(json.dumps(dict(status='pass' if all(r['status']=='pass' for r in rows.values()) else 'fail',profiles=rows,preserved_sources=original),indent=2)+'\n')
        print(profile,row['status'],flush=True)
    assert original=={name:digest(ROOT/name) for name in original},'Original snapshot/world changed'
    return 0 if all(r['status']=='pass' for r in rows.values()) else 1


if __name__=='__main__':raise SystemExit(main())
