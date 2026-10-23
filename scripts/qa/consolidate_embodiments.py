#!/usr/bin/env python3
"""Consolidate narrow current evidence without promoting complete LEC gates."""
import argparse
from collections import Counter
from datetime import datetime,timezone
import hashlib
import json
from pathlib import Path
import sys
ROOT=Path(__file__).resolve().parents[2]
sys.path[:0]=[str(ROOT/'scripts'),str(ROOT/'scripts/qa')]
from run_embodiments import matrix

def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,default=ROOT/'qa/fixtures/lec-crow');args=p.parse_args();args.output=args.output.resolve();args.output.mkdir(parents=True,exist_ok=True)
    policy=sha(ROOT/'sim/content/embodiments/policy.json');latest={};legacy_latest={};population_latest=None
    for path in sorted((ROOT/'target/qa/lec-crow').glob('*/result.json')):
        value=json.loads(path.read_text())
        if not value.get('lane') or value.get('policy_digest')!=policy:continue
        lane=value['lane']
        if lane not in latest or value['started_utc']>latest[lane][1]['started_utc']:latest[lane]=(path,value)
        population_exit=value.get('population_exit')
        if lane=='legacy-neural' and len(value.get('exit_codes',[]))>5:population_exit=value['exit_codes'][5]
        if population_exit is not None and (population_latest is None or value['started_utc']>population_latest[1]['started_utc']):
            population_latest=(path,value,population_exit)
        for profile,check in value.get('legacy_neural',{}).get('profiles',{}).items():
            if profile not in legacy_latest or value['started_utc']>legacy_latest[profile][1]['started_utc']:
                legacy_latest[profile]=(path,value,check)
    if not latest:raise SystemExit('No retained current-policy evidence; run the catalogued lanes first')
    report=matrix({},args.output,write=False);rows={(r['robot_id'],r['target']):r for r in report['rows']};evidence=[]
    for lane,(path,result) in sorted(latest.items()):
        transient=matrix(result,path.parent,write=False)
        changed=[w['path'] for w in result.get('source_witnesses',[]) if not (ROOT/w['path']).is_file() or sha(ROOT/w['path'])!=w['sha256']]
        source_current=bool(result.get('source_witnesses')) and not changed
        evidence.append(dict(lane=lane,status=result['status'],started_utc=result['started_utc'],result=str(path.relative_to(ROOT)),sha256=sha(path),command=result['command'],source_matches_worktree=source_current,changed_source_paths=changed))
        for row in transient['rows']:
            dest=rows[(row['robot_id'],row['target'])]
            if row['status']=='fail':dest['status']='fail';dest['reason']=row['reason']
            for gate,state in row['gate_results'].items():
                if state!='not_run':dest['gate_results'][gate]=state
            for name,check in row['subchecks'].items():
                check['source_matches_worktree']=source_current
                dest['subchecks'][lane+':'+name]=check
                if check['evidence'] not in dest['evidence']:dest['evidence'].append(check['evidence'])
        # A regenerated model/source digest invalidates positive evidence; it
        # cannot erase an observed failure. Keep the latest failing lane open
        # until a new run of that lane demonstrates recovery.
        for name in ('fleet_reference_loop','passive_energy','rendered_capture','body_reset'):
            for engine,profiles in result.get(name,{}).items():
                for profile,check in profiles.items():
                    if check.get('status')!='fail':continue
                    dest=rows[(profile,engine)]
                    current_model=check.get('model_digest')==dest.get('model_digest')
                    retained=dict(check,source_matches_worktree=source_current,model_matches_worktree=current_model,historical_failure_pending_retest=not(source_current and current_model))
                    dest['status']='fail'
                    dest['reason']=check.get('reason','Latest recorded '+name+' failed.')+(' Source/model changed; failure remains unresolved pending a current targeted rerun.' if retained['historical_failure_pending_retest'] else '')
                    dest['gate_results']['LEC-004' if name=='passive_energy' else 'LEC-007' if name=='body_reset' else 'LEC-005']='fail'
                    dest['subchecks'][lane+':'+name]=retained
                    if check['evidence'] not in dest['evidence']:dest['evidence'].append(check['evidence'])
        if result.get('neural_target'):
            for profile,passed in result.get('neural',{}).items():
                if passed:continue
                dest=rows[(profile,result['neural_target'])];dest['status']='fail';dest['reason']='Latest required reference-loop subcheck failed; inspect retained evidence. Full acceptance remains open.'
                log=str((path.parent/(profile+'.log')).relative_to(ROOT));dest['evidence'].append(log);dest['subchecks'][lane+':untrained_reference_loop']={'status':'fail','evidence':log,'scope':'latest real target/reference probe'}
        if lane=='fleet':
            for engine in ('webots','unreal'):
                detail=path.parent/engine/'physics'/('result.json' if engine=='webots' else 'engine-result.json')
                if not detail.exists():continue
                sample=json.loads(detail.read_text());worlds=sample['worlds'].values() if engine=='webots' else sample['instances']
                for w in worlds:
                    if not (w.get('transport') or {}).get('fault'):continue
                    dest=rows[(w.get('profile',w.get('profile_id')),engine)];link=str(detail.relative_to(ROOT))
                    dest['status']='fail';dest['reason']='Latest mixed-fleet reference channel fault: '+w['transport']['fault']+'. Unrelated instances retained separate progress; complete healthy-fleet acceptance remains open.'
                    dest['gate_results']['LEC-005']='fail'
                    dest['subchecks']['fleet:channel_health']={'status':'fail','evidence':link,'scope':'actual native mixed-world channel fault; no timeout converted to success'};dest['evidence'].append(link)
    if population_latest is not None and population_latest[2]!=0:
        path,result,_=population_latest;link=str((path.parent/'population/run.log').relative_to(ROOT))
        current=bool(result.get('source_witnesses')) and all((ROOT/w['path']).is_file() and sha(ROOT/w['path'])==w['sha256'] for w in result['source_witnesses'])
        for profile in ('hexapod','celegans','ugot_mecanum'):
            dest=rows[(profile,'mujoco')]
            dest['status']='fail';dest['reason']='Latest mixed managed population failed startup/exchange acceptance; individual adapter passes do not close this failure. Inspect retained population log.'
            dest['gate_results']['LEC-007']='fail'
            dest['subchecks']['managed_population']={'status':'fail','evidence':link,'source_matches_worktree':current,'scope':'two distinct hexapod brains, one worm brain and one NPC; startup and independent progress'}
            dest['evidence'].append(link)
    # A targeted repair reruns only selected profiles. Retain each other
    # profile's latest measured result, including unresolved import failures.
    for profile,(path,result,check) in legacy_latest.items():
        dest=rows[(profile,'mujoco')]
        current=bool(result.get('source_witnesses')) and all((ROOT/w['path']).is_file() and sha(ROOT/w['path'])==w['sha256'] for w in result['source_witnesses'])
        retained=dict(check,source_matches_worktree=current,model_matches_worktree=check.get('model_digest')==dest['model_digest'],result=str(path.relative_to(ROOT)))
        dest['subchecks']['preserved_named_neural_adapter']=retained
        if check['evidence'] not in dest['evidence']:dest['evidence'].append(check['evidence'])
        if check['status']=='fail':
            dest['status']='fail';dest['reason']=check['reason'];dest['gate_results']['LEC-005']='fail'
    # The preserved six-profile backend has its own existing native lane. A
    # failed startup must remain visible even when the new HTTP clients pass.
    candidates=list((ROOT/'target/qa/minecraft').glob('bedrock-native-*/result.json'))
    if candidates:
        path=max(candidates,key=lambda p:json.loads(p.read_text())['started_utc'])
        result=json.loads(path.read_text());native=path.parent/'bedrock-native.json'
        evidence.append(dict(lane='preserved-bedrock-native',status=result['status'],started_utc=result['started_utc'],result=str(path.relative_to(ROOT)),sha256=sha(path),command=result['reproduction']))
        if native.exists():
            sample=json.loads(native.read_text());passed={p['profile'] for p in sample.get('profiles',[]) if p['status']=='pass'}
            for profile in passed:
                dest=rows[(profile,'minecraft_bedrock')];log=str(native.relative_to(ROOT))
                dest['subchecks']['preserved_native_bridge']={'status':'pass','evidence':log,'scope':'preserved six-profile illustrative mode; actual BDS/Java bridge/Rust snapshot packets, no new physical NPC/task claim'};dest['evidence'].append(log)
            for log in sorted(path.parent.glob('*-rust.log')):
                profile=log.name.removesuffix('-rust.log')
                if profile in passed or (profile,'minecraft_bedrock') not in rows:continue
                dest=rows[(profile,'minecraft_bedrock')];link=str(log.relative_to(ROOT))
                dest['status']='fail';dest['reason']='Preserved native Bedrock reference snapshot did not reach listener readiness within the 180s acceptance budget. Actual CPU-reference selector verified; existing network unchanged.'
                dest['gate_results']['LEC-005']='fail'
                dest['subchecks']['preserved_native_bridge']={'status':'fail','evidence':link,'scope':'real preserved snapshot startup/native bridge regression; inspect parent run.log'};dest['evidence'].append(link)
    capability=latest.get('capabilities')
    if capability and (capability[0].parent/'capabilities.json').exists():
        link=str((capability[0].parent/'capabilities.json').relative_to(ROOT))
        for row in rows.values():
            if row['target'] in ('isaacsim','unity'):row['evidence'].append(link)
    arm=ROOT/'target/qa/lec-crow/arm64-current/result.json'
    verification=arm.with_name('source-verification.json')
    if arm.exists() and verification.exists():
        check=json.loads(verification.read_text());current=all((ROOT/w['path']).is_file() and sha(ROOT/w['path'])==w['sha256'] for w in check['current_source_witnesses'])
        sample=json.loads(arm.read_text());link=str(arm.relative_to(ROOT))
        evidence.append(dict(lane='arm64-native',status=sample['status'],started_utc=datetime.fromtimestamp(arm.stat().st_mtime,timezone.utc).isoformat(),result=link,sha256=sha(arm),command=sample['command'],source_matches_worktree=current))
        for row in rows.values():
            if row['target']=='mujoco':
                row['evidence'].append(link);row['subchecks']['arm64_native_tests']={'status':'pass','evidence':link,'source_matches_worktree':current,'scope':'actual ARM64 CPU SDK/shared ABI/28-model ownership tests; no GPU, reference-Runner or learned-task claim'}
    report.update(generated_utc=datetime.now(timezone.utc).isoformat(),full_acceptance_complete=False,evidence_index=evidence)
    (args.output/'acceptance-matrix.current.json').write_text(json.dumps(report,indent=2)+'\n')
    counts=Counter(r['status'] for r in report['rows']);targets=list(dict.fromkeys(r['target'] for r in report['rows']));profiles=list(dict.fromkeys(r['robot_id'] for r in report['rows']))
    text=['# Current expansion evidence','',f'Generated {report["generated_utc"]}. Full profile/target acceptance is incomplete. Passing subchecks are narrow imports/reference loops, not trained tasks or governed output. Historical evidence remains retained: `source_matches_worktree` and `changed_source_paths` identify source differences; a historical pass does not certify changed code.', '',f'224 rows: '+', '.join(f'{key}={count}' for key,count in sorted(counts.items()))+'.','', '| Profile | '+' | '.join(targets)+' |','| --- | '+' | '.join('---' for _ in targets)+' |']
    for profile in profiles:text.append('| '+profile+' | '+' | '.join(rows[(profile,t)]['status'] for t in targets)+' |')
    text.extend(['','Detailed model/IO hashes, subchecks and evidence: [acceptance-matrix.current.json](acceptance-matrix.current.json).','','Latest lane results:',''])
    for item in evidence:text.append(f'- `{item["lane"]}`: **{item["status"]}**, `{item["result"]}`, SHA-256 `{item["sha256"]}`.')
    (args.output/'CURRENT.md').write_text('\n'.join(text)+'\n')
    print(json.dumps(dict(rows=len(rows),counts=counts,output=str(args.output))))
if __name__=='__main__':main()
