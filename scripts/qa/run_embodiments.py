#!/usr/bin/env python3
"""Authoritative xtask lane runner with unique retained results and 224 rows."""
import argparse
from datetime import datetime,timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import shlex
import subprocess
import sys
import tempfile
import time
ROOT=Path(__file__).resolve().parents[2]
sys.path[:0]=[str(ROOT),str(ROOT/'scripts')]


def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()

def capture(command):
    try:return subprocess.check_output(command,cwd=ROOT,text=True,stderr=subprocess.DEVNULL,timeout=10).strip()
    except (OSError,subprocess.SubprocessError):return None

def simulation_runtime_build():
    args=subprocess.check_output(['bash','-c','source scripts/webots_runtime_profile.sh\nwebots_cargo_profile_args "$(webots_runtime_profile)"'],cwd=ROOT,text=True)
    return ['cargo','build','--locked','--release',*shlex.split(args),'--bin','aarnn_rust','--bin','tcp_aer_ipc_bridge']


def run(command,output,timeout=120,env=None):
    with output.open('w') as log:
        process=subprocess.Popen(command,cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,start_new_session=True,env=env)
        try:return process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid,signal.SIGTERM)
            try:process.wait(timeout=5)
            except subprocess.TimeoutExpired:os.killpg(process.pid,signal.SIGKILL);process.wait()
            return 124


def matrix(results,output,write=True):
    source=ROOT/'.agent/robot-expansion/verification/acceptance-matrix.json'
    result=json.loads(source.read_text());result['implementation_schema']=2
    for row in result['rows']:
        profile,target=row['robot_id'],row['target'];row['status']='not_run';row['evidence']=[]
        row['gate_results']={f'LEC-{i:03}':'not_run' for i in range(1,11)}
        row['subchecks']={}
        row['exact_replica']='pending variant/mechanical/biological calibration; functional surrogates do not close this gate'
        row['reason']='Full target acceptance remains open, including trained tasks, fault coverage, performance, parity and usability.'
        record=ROOT/'sim/content/embodiments/generated'/f'{profile}.json'
        if record.exists():
            model=json.loads(record.read_text());row.update(model_digest=model['digest'],io_digest=model['io']['digest'],fidelity=model['fidelity'])
        if target=='mujoco' and profile in results.get('native_import',{}) and results['native_import'][profile].get('model_digest')==model['digest']:
            item=results['native_import'][profile]
            evidence=output/('native-rust.json' if (output/'native-rust.json').exists() else 'native-import.json')
            row['subchecks']['headless_import']={'status':'pass','scope':'load, named indices and 100 finite steps; rendered/constraint/contact/task acceptance independent','evidence':str(evidence.relative_to(ROOT))}
            row['evidence'].append(str(evidence.relative_to(ROOT)))
        if target=='webots' and profile in results.get('native_webots',{}) and results['native_webots'][profile].get('model_digest')==model['digest']:
            item=results['native_webots'][profile]
            row['subchecks']['headless_import']={'status':'pass','scope':'Rust controller, named devices and 100 native physics steps; calibration and tasks independent','evidence':item['evidence']}
            row['evidence'].append(item['evidence'])
        if target=='isaacsim':
            row['status']='blocked';row['reason']='No supported Isaac runtime found on local/designated SSH hosts. Native Kit articulation/client lifecycle remains unimplemented. Offline USD parsing is not engine acceptance.'
            row['gate_results']['LEC-002']='blocked'
        if target=='unity':
            row['status']='blocked';row['reason']='Unity Editor/runtime not discovered. Opt-in native rigid-body factory and shared Rust ABI source exist; actual Unity construction, lifecycle and target acceptance require an installed Editor.'
            row['gate_results']['LEC-002']='blocked'
        if target=='mujoco' and model.get('controller_transfer','').startswith('blocked'):
            row['status']='blocked';row['reason']='Native mechanical/NPC port exists; biological sensory/muscle and fluid/flight transfer to the preserved network is uncalibrated and deliberately fails closed.'
            row['gate_results']['LEC-003']='blocked';row['gate_results']['LEC-006']='blocked'
        if results.get('neural',{}).get(profile) and target==results.get('neural_target','mujoco'):
            evidence=output/('neural-'+profile)/'result.json'
            sample=json.loads(evidence.read_text()) if evidence.exists() else {}
            if sample.get('model_digest')==model['digest'] and sample.get('io_digest')==model['io']['digest']:
                row['subchecks']['untrained_reference_loop']={'status':'pass','scope':'strict AER1 reference Runner exchange and local motor decoding; no learned task or governed commit/fence claim','evidence':str(evidence.relative_to(ROOT))}
                row['evidence'].append(str(evidence.relative_to(ROOT)))
        if profile=='celegans' and target=='mujoco' and results.get('managed_mujoco',{}).get('status')=='pass':
            evidence=str((output/'managed.json').relative_to(ROOT))
            row['subchecks']['preserved_network_managed_cluster']={'status':'pass','scope':'real original C. elegans 24/96 projection, one/three workers, bidirectional AER1 and measured muscle effort; no NPC; calibration/learned tasks/production effects remain open','evidence':evidence}
            row['evidence'].append(evidence)
            if results['managed_mujoco'].get('viewer')=='pass':row['subchecks']['native_interactive_viewer']={'status':'pass','scope':'native rendering, default survives former 1000-step limit, signal shutdown and isolated cluster cleanup','evidence':evidence}
        legacy=results.get('legacy_neural',{}).get('profiles',{}).get(profile)
        if target=='mujoco' and legacy:
            row['subchecks']['preserved_named_neural_adapter']=legacy
            row['evidence'].append(legacy['evidence'])
            if legacy['status']=='fail':
                row['status']='fail';row['reason']=legacy['reason'];row['gate_results']['LEC-005']='fail'
        population=results.get('managed_population',{})
        if target=='mujoco' and population.get('status')=='pass' and any(w['profile']==profile for w in population['worlds']):
            evidence=str((output/'population/population.json').relative_to(ROOT))
            row['subchecks']['independent_managed_population']={'status':'pass','evidence':evidence,'scope':population['scope']}
            row['evidence'].append(evidence)
        if target=='unreal':row['reason']='Opt-in native Chaos factory and shared Rust reference ABI exist; native subchecks, calibration, lifecycle, rendering and trained tasks remain independent gates.'
        if target==results.get('lane') and profile in results.get('native_game',{}):
            item=results['native_game'][profile]
            if item.get('model_digest')==model['digest']:
                row['subchecks']['headless_import']={'status':'pass','scope':'native generated rigid bodies/constraints and 100 steps; solver calibration, lifecycle and tasks remain open','evidence':item['evidence']}
                row['evidence'].append(item['evidence'])
        if target in ('minecraft_java','minecraft_bedrock'):
            row['reason']='Existing six-profile backend preserved; new native factories/clients or authoritative game physics integration remain unimplemented. Installed Unreal is available for continued source/build work.'
        if target in ('minecraft_java','minecraft_bedrock'):
            row['reason']='Opt-in native-service observer, inert bounding-box display and full-block collision tier implemented; actual game/render, partial-block/player/door/perch, lag and neural task acceptance remain open.'
            if results.get('minecraft_service_client'):
                evidence=str((output/'run-5.log').relative_to(ROOT))
                row['subchecks']['native_service_client']={'status':'pass','scope':'edition client logic against real Rust/MuJoCo HTTP, identity/epoch checks and atomic block replacement; no Minecraft engine execution or task claim','evidence':evidence}
                row['evidence'].append(evidence)
        npc=results.get('npc_mechanics',{}).get(target,{}).get(profile)
        if npc and npc.get('model_digest')==model['digest']:
            row['subchecks']['programmed_npc_mechanics']={'status':'pass','scope':'bounded real motor patterns with no AER; source-clock arbitration/stop tests; no task, gait, learning or biological competence claim','evidence':npc['evidence']}
            row['evidence'].append(npc['evidence'])
        for key in ('fleet_reference_loop','passive_energy','rendered_capture','body_reset'):
            check=results.get(key,{}).get(target,{}).get(profile)
            if check and check.get('model_digest')==model['digest']:
                row['subchecks'][key]=check
                row['evidence'].append(check['evidence'])
                if check['status']=='fail':
                    row['status']='fail';row['reason']=check['reason']
                    row['gate_results']['LEC-004' if key=='passive_energy' else 'LEC-007' if key=='body_reset' else 'LEC-005']='fail'
        if results.get('habitat_audit',{}).get('status')=='pass':
            row['subchecks']['habitat_source_audit']={'status':'pass','scope':'all-28 canonical rounded/articulated source and native scene exports; target execution/calibration separate','evidence':str((output/'habitat-audit.json').relative_to(ROOT))}
            row['evidence'].append(str((output/'habitat-audit.json').relative_to(ROOT)))
        if target=='webgl':row['reason']='Native Rust authoritative physics service and token-authenticated collision viewer implemented; per-profile rendered/neural task acceptance remains open.'
        if results.get('service_exit')==0 and target=='webgl' and profile=='ugot_mecanum':
            evidence=str((output/'service.log').relative_to(ROOT));row['evidence'].append(evidence)
            row['subchecks']['reference_service_lifecycle']={'status':'pass','scope':'real HTTP auth/origin denial, pause/step, collision rays, checkpoint/restore epochs and latched disarm; browser rendering separate','evidence':evidence}
    assert len(result['rows'])==224
    result['gate_note']='Subchecks retain narrow passing evidence. No full LEC gate or profile/target row is promoted from an import, source hash or untrained packet exchange.'
    if write:(output/'acceptance-matrix.json').write_text(json.dumps(result,indent=2)+'\n')
    return result


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--lane',choices=['contract','mujoco','neural','usd','rust','rust-neural','rust-service','webots','webots-neural','shared','unreal','unreal-neural','unity','launchers','minecraft-service','npc','fleet','unreal-passive','unreal-render','unreal-lifecycle','capabilities','mujoco-managed','mujoco-viewer','mujoco-population','habitats','legacy-neural'],default='contract');args=parser.parse_args()
    started=time.monotonic()
    base=ROOT/'target/qa/lec-crow';base.mkdir(parents=True,exist_ok=True)
    output=Path(tempfile.mkdtemp(prefix=args.lane+'-',dir=base))
    python=os.environ.get('NM_MUJOCO_PYTHON',str(ROOT/'target/robot-mujoco-venv/bin/python'))
    usd_python=os.environ.get('NM_USD_PYTHON',str(ROOT/'target/robot-usd-venv/bin/python'))
    result=dict(schema_version=1,scenario='LEC-EXPANSION-001',lane=args.lane,started_utc=datetime.now(timezone.utc).isoformat(),
                command=['cargo','xtask','qa','run','--suite','lec-crow'+('' if args.lane=='contract' else '-'+args.lane)],
                git_revision=capture(['git','rev-parse','HEAD']),git_dirty=bool(capture(['git','status','--porcelain'])),
                platform=dict(system=platform.system(),architecture=platform.machine()),seed=481516,
                policy_digest=sha(ROOT/'sim/content/embodiments/policy.json'),scenario_digest=sha(ROOT/'qa/scenarios/LEC-EXPANSION-001.toml'),
                native_import={},native_webots={},neural={},neural_target='unreal' if args.lane=='unreal-neural' else 'webots' if args.lane=='webots-neural' else 'mujoco',governed_effects='disabled; no production gate promoted')
    witness=[]
    for folder in ['crates/aarnn-embodiment','crates/aarnn-embodiment-ffi','adapters/mujoco','adapters/webots','sim/unity/Assets/NeuralMimicry/Runtime/Embodiment','sim/unreal/Source/NmAerBridge','sim/minecraft/src','sim/minecraft/bedrock/scripts']:
        for path in sorted((ROOT/folder).rglob('*')):
            if path.is_file() and path.suffix in ('.rs','.toml','.cs','.cpp','.h','.java','.js','.json','.accesswidener'):
                witness.append(dict(path=str(path.relative_to(ROOT)),sha256=sha(path)))
    result['source_witnesses']=witness
    commands=[]
    if args.lane=='habitats':
        for name in ('scripts/embodiment_scene.py','scripts/embodiment_anatomy.py','scripts/embodiment_legacy.py','scripts/embodiment.py','scripts/embodiment_graph.py','scripts/embodiment_webots.py','scripts/embodiment_usd.py','scripts/run_webots_embodiment.py','scripts/run_native_embodiment.py','scripts/qa/probe_embodiment_habitats.py','scripts/qa/render_embodiment_habitats.py','scripts/qa/test_habitat_browser.cjs','web_ui/webgl-world.js','web_ui/physics-view.js'):
            witness.append(dict(path=name,sha256=sha(ROOT/name)))
    if args.lane in ('mujoco-managed','mujoco-viewer','mujoco-population','legacy-neural'):
        for name in ('scripts/run_sim.sh','scripts/mujoco_bindings.py','scripts/qa/probe_mujoco_managed.py','src/tcp_aer_ipc_bridge.rs','src/bin/tcp_aer_ipc_bridge.rs'):
            witness.append(dict(path=name,sha256=sha(ROOT/name)))
    if args.lane=='mujoco-population':
        for name in ('scripts/qa/probe_mujoco_population.py','scripts/webots_runtime_profile.sh','src/main.rs','Cargo.toml'):
            witness.append(dict(path=name,sha256=sha(ROOT/name)))
    if args.lane=='legacy-neural':
        for name in ('scripts/embodiment_neural.py','scripts/embodiment_legacy.py','scripts/embodiment.py','scripts/qa/probe_legacy_neural.py','scripts/qa/test_legacy_bindings.py','scripts/qa/probe_mujoco_population.py','scripts/robot_profiles.py','scripts/webots_runtime_profile.sh','sim/content/embodiments/legacy-native-v1.json','Cargo.toml','src/main.rs','sim/mujoco/runtime.py'):
            witness.append(dict(path=name,sha256=sha(ROOT/name)))
    if args.lane=='capabilities':commands=[['python3','scripts/qa/probe_embodiment_capabilities.py','--output',str(output/'capabilities.json')]]
    if args.lane=='contract':commands=[[python,'scripts/qa/test_embodiments.py'],[python,'scripts/qa/test_embodiment_graph.py'],['python3','scripts/generate_embodiment_ffi.py','--check'],[python,'scripts/qa/test_physics_service.py']]
    if args.lane=='usd':commands=[[usd_python,'scripts/embodiment_usd.py','--check']]
    if args.lane in ('mujoco','contract'):
        if not Path(python).is_file():result['capability_error']='Pinned MuJoCo interpreter unavailable'
        else:
            command=[python,'-c',"""import json,numpy as np
from sim.mujoco.runtime import World
from scripts.embodiment import NEW,LEGACY
report={}
for p in NEW+LEGACY:
 w=World(p)
 for _ in range(100):w.step()
 if not np.all(np.isfinite(w.data.qpos)):raise ValueError('nonfinite '+p)
 report[p]=dict(nq=w.model.nq,nv=w.model.nv,nu=w.model.nu,sensory=len(w.record['io']['sensors']),output=len(w.record['io']['output_names']),model_digest=w.record['digest'],io_digest=w.record['io']['digest'],scope='headless import and finite stepping only')
print(json.dumps(report))"""]
            code=run(command,output/'native-import.json');result['native_import_exit']=code
            if code==0:result['native_import']=json.loads((output/'native-import.json').read_text())
    if args.lane in ('neural','rust-neural','webots-neural','unreal-neural'):
        # Each lane builds its own prerequisites; never certify an old binary
        # left by another suite or require undocumented manual warm-up.
        prerequisites=[['cargo','build','--locked','--release','--no-default-features','--features','engine_runtime,ui,robot_io','--example','nn_tcp_server','-j2']]
        env=dict(os.environ)
        if args.lane=='rust-neural':
            sdk=Path(env.get('AARNN_MUJOCO_DIR',str(Path.home()/'Developer/mujoco-3.17.0')))
            env.update(AARNN_MUJOCO_DIR=str(sdk),LD_LIBRARY_PATH=str(sdk/'lib')+(':'+env['LD_LIBRARY_PATH'] if env.get('LD_LIBRARY_PATH') else ''))
            prerequisites.append(['cargo','build','--locked','-p','aarnn-mujoco','--features','native'])
        if args.lane=='unreal-neural':
            prerequisites.append(['cargo','build','--locked','-p','aarnn-embodiment-ffi'])
            engine=Path(os.environ.get('UE_ENGINE',str(Path.home()/'Developer/Engine')))
            prerequisites.append([str(engine/'Build/BatchFiles/Linux/Build.sh'),'NeuralMimicrySimEditor','Linux','Development','-Project='+str(ROOT/'sim/unreal/NeuralMimicrySim.uproject'),'-WaitMutex'])
        if args.lane=='webots-neural':prerequisites.append(['cargo','build','--locked','-p','aarnn-webots','--features','native'])
        for index,command in enumerate(prerequisites):
            if run(command,output/f'prerequisite-{index}.log',600,env):result['capability_error']='reference/native build failed; inspect prerequisite logs';break
        for profile in ('galbot_g1_golf','ugot_mecanum','crow_corvus_corone'):
            if result.get('capability_error'):break
            dest=output/('neural-'+profile)
            code=run([python,'scripts/qa/probe_embodiment_neural.py']+(['--rust'] if args.lane=='rust-neural' else ['--webots'] if args.lane=='webots-neural' else ['--unreal'] if args.lane=='unreal-neural' else [])+['--profile',profile,'--out',str(dest)],output/(profile+'.log'),180)
            result['neural'][profile]=(code==0)
    if args.lane=='shared':
        commands=[['python3','scripts/generate_embodiment_ffi.py','--check'],[python,'scripts/embodiment_graph.py','--check'],[python,'scripts/qa/test_embodiment_graph.py'],['cargo','test','--locked','-p','aarnn-embodiment','-p','aarnn-embodiment-ffi'],['cargo','build','--locked','-p','aarnn-embodiment-ffi'],['python3','scripts/qa/test_embodiment_ffi.py']]
    if args.lane=='fleet':
        prerequisites=[['cargo','build','--locked','--release','--no-default-features','--features','engine_runtime,ui,robot_io','--example','nn_tcp_server','-j2'],['cargo','build','--locked','-p','aarnn-embodiment-ffi','-p','aarnn-webots','--features','aarnn-webots/native']]
        for i,command in enumerate(prerequisites):
            if run(command,output/f'build-{i}.log',600):result['capability_error']='mixed-fleet prerequisite build failed';break
        result['fleet_reference_loop']={}
        if not result.get('capability_error'):
            for target in ('webots','unreal'):
                dest=output/target
                # Unreal launcher builds the native module before this lane's
                # isolated Runner processes start; a build is never a load pass.
                if target=='unreal':
                    engine=Path(os.environ.get('UE_ENGINE',str(Path.home()/'Developer/Engine')))
                    if run([str(engine/'Build/BatchFiles/Linux/Build.sh'),'NeuralMimicrySimEditor','Linux','Development','-Project='+str(ROOT/'sim/unreal/NeuralMimicrySim.uproject'),'-WaitMutex'],output/'unreal-build.log',600):result['capability_error']='Unreal build failed';continue
                code=run([python,'scripts/qa/probe_embodiment_fleet.py','--target',target,'--output',str(dest)],output/(target+'.log'),180)
                if code:result['capability_error']='mixed-fleet reference lane failed; inspect '+target+' evidence'
                if not (dest/'result.json').exists():continue
                sample=json.loads((dest/'result.json').read_text());checks={}
                for w in sample.get('instances',[]):
                    profile=w.get('profile',w.get('profile_id'))
                    status=sample.get('instance_checks',{}).get(w['instance_id'],{}).get('status','pass' if sample['status']=='pass' else 'fail')
                    if checks.get(profile,{}).get('status')=='fail':continue
                    checks[profile]=dict(status=status,model_digest=w['model_digest'],evidence=str((dest/'result.json').relative_to(ROOT)),scope='three real asynchronous brains including two UGOT identities and crow in one contact world; no contact calibration or learned task claim')
                    if status=='fail':checks[profile]['reason']='Latest mixed-fleet reference channel/identity check failed; inspect per-instance transport and metrics.'
                result['fleet_reference_loop'][target]=checks
    if args.lane in ('unreal-passive','unreal-render','unreal-lifecycle'):
        from embodiment import NEW,LEGACY
        key='body_reset' if args.lane=='unreal-lifecycle' else 'passive_energy' if args.lane=='unreal-passive' else 'rendered_capture'
        result[key]={'unreal':{}}
        groups=(NEW[:16],NEW[16:]+LEGACY) if args.lane!='unreal-render' else [('galbot_g1_golf','ugot_mecanum','crow_corvus_corone')]
        for index,group in enumerate(groups):
            dest=output/f'batch-{index}'
            command=['python3','scripts/run_native_embodiment.py','--target','unreal','--robots',','.join(p+'=1' for p in group),'--steps','1' if args.lane=='unreal-render' else '100','--control','npc' if args.lane=='unreal-lifecycle' else 'silent','--headless','--output',str(dest)]+(['--capture'] if args.lane=='unreal-render' else ['--reset-at','50'] if args.lane=='unreal-lifecycle' else [])
            if run(command,output/f'batch-{index}.log',300):result['capability_error']='native '+args.lane+' did not execute';continue
            for w in json.loads((dest/'engine-result.json').read_text())['instances']:
                limit=max(.01,.05*abs(w['initial_mechanical_energy_J']))
                passed=args.lane=='unreal-render' or w['max_mechanical_energy_J']-w['initial_mechanical_energy_J']<=limit
                check=dict(status='pass' if passed else 'fail',model_digest=w['model_digest'],evidence=str((dest/('capture.png' if args.lane=='unreal-render' else 'engine-result.json')).relative_to(ROOT)),scope='actual offscreen native framebuffer after one 2ms step; authored geometry appearance only, passive dynamics tested separately' if args.lane=='unreal-render' else 'unpowered 0.2s native contact/coupling energy; tolerance max(0.01J,5% initial mechanical energy)',initial_J=w['initial_mechanical_energy_J'],max_J=w['max_mechanical_energy_J'],tolerance_J=limit)
                if args.lane=='unreal-lifecycle':
                    passed=w['reset_epoch']==1 and w['source_step']==50 and w['inhibited'] and w['transport'] is None and w['brain_id'] is None and w['npc_steps']==50 and w['active_steps']==0
                    check.update(status='pass' if passed else 'fail',scope='body reset after 50 programmed NPC steps; epoch/source restart, disconnected stream and inhibited NPC/neural output; no remote-brain checkpoint/replay claim')
                if not passed:
                    check['reason']='Native Chaos body-reset contract failed' if args.lane=='unreal-lifecycle' else 'Native Chaos passive-energy falsification failed: solver/contact energy injection without neural or NPC drive. Mechanical calibration remains open.'
                    result['capability_error']=check['reason']
                result[key]['unreal'][w['profile_id']]=check
    if args.lane in ('unreal','unity'):
        from embodiment import NEW
        result['native_game']={}
        for index,group in enumerate((NEW[:16],NEW[16:])):
            dest=output/f'batch-{index}'
            command=['python3','scripts/run_native_embodiment.py','--target',args.lane,'--robots',','.join(p+'=1' for p in group),'--steps','100','--control','silent','--headless','--output',str(dest)]
            code=run(command,output/f'batch-{index}.log',300)
            if code:result['capability_error']='native '+args.lane+' batch did not execute; inspect capability/build/runtime evidence'
            if (dest/'engine-result.json').exists():
                for w in json.loads((dest/'engine-result.json').read_text())['instances']:
                    result['native_game'][w['profile_id']]=dict(w,evidence=str((dest/'engine-result.json').relative_to(ROOT)))
            if code==2:break
    if args.lane=='webots':
        code=run(['cargo','build','--locked','-p','aarnn-webots','--features','native'],output/'build.log',180)
        if code:result['capability_error']='native Webots Rust build failed'
        else:
            code=run([python,'scripts/embodiment_webots.py','--check'],output/'freshness.log',120)
            if code:result['capability_error']='stale native Webots sources'
            from embodiment import NEW
            for index,group in enumerate((NEW[:16],NEW[16:])):
                dest=output/f'batch-{index}'
                code=run([python,'scripts/run_webots_embodiment.py','--robots',','.join(p+'=1' for p in group),'--steps','100','--control','silent','--headless','--no-build','--output',str(dest)],output/f'batch-{index}.log',180)
                if code:result['capability_error']='native Webots import failed; inspect batch logs'
                if (dest/'result.json').exists():
                    result['native_webots'].update({w['profile']:dict(w,evidence=str((dest/'result.json').relative_to(ROOT))) for w in json.loads((dest/'result.json').read_text())['worlds'].values() if w['status']=='pass'})
    if args.lane in ('rust','rust-service'):
        sdk=Path(os.environ.get('AARNN_MUJOCO_DIR',str(Path.home()/'Developer/mujoco-3.17.0')))
        env=dict(os.environ,AARNN_MUJOCO_DIR=str(sdk),LD_LIBRARY_PATH=str(sdk/'lib')+(':'+os.environ['LD_LIBRARY_PATH'] if os.environ.get('LD_LIBRARY_PATH') else ''))
        features='service' if args.lane=='rust-service' else 'native'
        commands_native=[['cargo','test','--locked','-p','aarnn-mujoco','-p','aarnn-embodiment','-p','aarnn-aer-legacy','--features','aarnn-mujoco/'+features],['cargo','build','--locked','-p','aarnn-mujoco','--features',features]]
        for i,command in enumerate(commands_native):
            code=run(command,output/f'rust-{i}.log',180,env)
            if code:result['capability_error']=f'native Rust command failed: {command}';break
        if not result.get('capability_error'):
            from embodiment import NEW,LEGACY
            code=run([str(ROOT/'target/debug/aarnn-mujoco'),'--robots',','.join(p+'=1' for p in NEW+LEGACY),'--steps','100','--control','silent','--headless','--output',str(output/'native-rust.json')],output/'native-rust.log',120,env)
            result['native_import_exit']=code
            if code==0:result['native_import']={w['profile']:w for w in json.loads((output/'native-rust.json').read_text())['worlds']}
        if args.lane=='rust-service' and not result.get('capability_error'):
            code=run([sys.executable,'scripts/qa/test_rust_physics_service.py'],output/'service.log',60,env)
            result['service_exit']=code
            if code:result['capability_error']='native Rust HTTP authority test failed'
    if args.lane=='npc':
        from embodiment import NEW,LEGACY
        profiles=NEW+LEGACY
        sdk=Path(os.environ.get('AARNN_MUJOCO_DIR',str(Path.home()/'Developer/mujoco-3.17.0')))
        env=dict(os.environ,AARNN_MUJOCO_DIR=str(sdk),LD_LIBRARY_PATH=str(sdk/'lib')+(':'+os.environ['LD_LIBRARY_PATH'] if os.environ.get('LD_LIBRARY_PATH') else ''))
        result['npc_mechanics']={}
        prerequisite=['cargo','build','--locked','-p','aarnn-mujoco','-p','aarnn-webots','-p','aarnn-embodiment-ffi','--features','aarnn-mujoco/native,aarnn-webots/native']
        if run(prerequisite,output/'npc-build.log',180,env):result['capability_error']='NPC native build failed'
        elif run(['cargo','test','--locked','-p','aarnn-embodiment','-p','aarnn-embodiment-ffi','-p','aarnn-mujoco'],output/'npc-tests.log',180,env):result['capability_error']='NPC arbitration/native tests failed'
        else:
            path=output/'npc-mujoco.json'
            code=run([str(ROOT/'target/debug/aarnn-mujoco'),'--robots',','.join(p+'=1' for p in profiles),'--control','npc','--steps','100','--headless','--output',str(path)],output/'npc-mujoco.log',120,env)
            if code:result['capability_error']='native NPC MuJoCo run failed'
            else:result['npc_mechanics']['mujoco']={w['profile']:dict(w,evidence=str(path.relative_to(ROOT))) for w in json.loads(path.read_text())['worlds'] if w.get('npc_steps')==100 and w['controller_mode']=='programmed-NPC'}
            for target in ('webots','unreal'):
                passed={}
                for index,group in enumerate((profiles[:16],profiles[16:])):
                    dest=output/f'npc-{target}-{index}'
                    command=[python,'scripts/run_webots_embodiment.py'] if target=='webots' else ['python3','scripts/run_native_embodiment.py','--target','unreal']
                    command+=['--robots',','.join(p+'=1' for p in group),'--control','npc','--steps','100','--headless','--output',str(dest)]
                    code=run(command,output/f'npc-{target}-{index}.log',300,env)
                    if code:result['capability_error']='NPC '+target+' run failed; inspect native logs';continue
                    evidence=dest/('result.json' if target=='webots' else 'engine-result.json');sample=json.loads(evidence.read_text())
                    worlds=sample['worlds'].values() if target=='webots' else sample['instances']
                    for w in worlds:
                        if w.get('npc_steps',0)>=99:passed[w.get('profile',w.get('profile_id'))]=dict(w,evidence=str(evidence.relative_to(ROOT)))
                result['npc_mechanics'][target]=passed
            if any(len(v)!=28 for v in result['npc_mechanics'].values()):result['capability_error']='NPC profile coverage incomplete; no alias or silent omission permitted'
    command_env=None
    if args.lane=='legacy-neural':
        sdk=Path(os.environ.get('AARNN_MUJOCO_DIR',str(Path.home()/'Developer/mujoco-3.17.0')))
        command_env=dict(os.environ,AARNN_MUJOCO_DIR=str(sdk),LD_LIBRARY_PATH=str(sdk/'lib')+(':'+os.environ['LD_LIBRARY_PATH'] if os.environ.get('LD_LIBRARY_PATH') else ''))
        # Compile the exact runtime profile selected by the canonical launcher.
        # An all-features artifact requires different management credentials
        # and cannot be reused under a plaintext simulation profile.
        runtime_build=simulation_runtime_build()
        commands=[['cargo','build','--locked','-p','aarnn-mujoco','--features','viewer,service'],['cargo','test','--locked','-p','aarnn-embodiment','-p','aarnn-mujoco','--features','aarnn-mujoco/viewer,aarnn-mujoco/service'],['python3','scripts/qa/test_legacy_bindings.py'],['cargo','test','--locked','--release','--all-features','--bin','aarnn_rust','startup_snapshot_io_tests'],runtime_build,['python3','scripts/qa/probe_mujoco_population.py','--output',str(output/'population')],['python3','scripts/qa/probe_legacy_neural.py','--output',str(output)]]
        profiles=os.environ.get('NM_LEGACY_NEURAL_PROFILES','').split()
        if profiles:
            commands[-1]+=['--profiles',*profiles]
            result['legacy_profiles_requested']=profiles
    if args.lane=='mujoco-population':
        sdk=Path(os.environ.get('AARNN_MUJOCO_DIR',str(Path.home()/'Developer/mujoco-3.17.0')))
        command_env=dict(os.environ,AARNN_MUJOCO_DIR=str(sdk),LD_LIBRARY_PATH=str(sdk/'lib')+(':'+os.environ['LD_LIBRARY_PATH'] if os.environ.get('LD_LIBRARY_PATH') else ''))
        commands=[['cargo','build','--locked','-p','aarnn-mujoco','--features','viewer,service'],simulation_runtime_build(),['python3','scripts/qa/probe_mujoco_population.py','--output',str(output/'population')]]
    if args.lane=='habitats':
        sdk=Path(os.environ.get('AARNN_MUJOCO_DIR',str(Path.home()/'Developer/mujoco-3.17.0')))
        command_env=dict(os.environ,AARNN_MUJOCO_DIR=str(sdk),LD_LIBRARY_PATH=str(sdk/'lib')+(':'+os.environ['LD_LIBRARY_PATH'] if os.environ.get('LD_LIBRARY_PATH') else ''))
        commands=[['cargo','build','--locked','-p','aarnn-mujoco','-p','aarnn-webots','-p','aarnn-embodiment-ffi','--features','aarnn-mujoco/viewer,aarnn-mujoco/service,aarnn-webots/native'],['cargo','test','--locked','-p','aarnn-mujoco','-p','aarnn-embodiment','-p','aarnn-embodiment-ffi','--features','aarnn-mujoco/viewer,aarnn-mujoco/service'],[python,'scripts/qa/probe_embodiment_habitats.py','--output',str(output/'habitat-audit.json')],[python,'scripts/qa/test_embodiment_graph.py'],[usd_python,'scripts/embodiment_usd.py','--check'],['python3','scripts/qa/render_embodiment_habitats.py','--output',str(output/'rendered')],['node','scripts/qa/test_habitat_browser.cjs',str(output)],[python,'scripts/run_webots_embodiment.py','--robots','celegans=1,drosophila_banc=1,crow_corvus_corone=1,zebrafish=1','--control','npc','--headless','--steps','100','--no-build','--output',str(output/'webots')],['python3','scripts/run_native_embodiment.py','--target','unreal','--robots','celegans=1,crow_corvus_corone=1','--control','npc','--steps','100','--capture','--output',str(output/'unreal')]]
    if args.lane in ('mujoco-managed','mujoco-viewer'):
        sdk=Path(os.environ.get('AARNN_MUJOCO_DIR',str(Path.home()/'Developer/mujoco-3.17.0')))
        command_env=dict(os.environ,AARNN_MUJOCO_DIR=str(sdk),LD_LIBRARY_PATH=str(sdk/'lib')+(':'+os.environ['LD_LIBRARY_PATH'] if os.environ.get('LD_LIBRARY_PATH') else ''))
        commands=[['cargo','build','--locked','-p','aarnn-mujoco','--features','viewer,service'],['cargo','build','--release','--locked','--all-features','--bin','tcp_aer_ipc_bridge'],['cargo','test','--release','--locked','--all-features','--bin','tcp_aer_ipc_bridge'],['python3','scripts/qa/probe_mujoco_managed.py','--output',str(output)]+(['--viewer'] if args.lane=='mujoco-viewer' else [])]
    if args.lane in ('launchers','minecraft-service'):
        sdk=Path(os.environ.get('AARNN_MUJOCO_DIR',str(Path.home()/'Developer/mujoco-3.17.0')))
        command_env=dict(os.environ,AARNN_MUJOCO_DIR=str(sdk),LD_LIBRARY_PATH=str(sdk/'lib')+(':'+os.environ['LD_LIBRARY_PATH'] if os.environ.get('LD_LIBRARY_PATH') else ''))
        if args.lane=='launchers':
            commands=[['cargo','build','--locked','-p','aarnn-mujoco','-p','aarnn-webots','--features','aarnn-mujoco/service,aarnn-webots/native'],['python3','scripts/qa/test_embodiment_launchers.py']]
        else:
            command_env['JAVA_HOME']='/usr/lib/jvm/java-25-openjdk-amd64'
            commands=[['sim/minecraft/gradlew','-p','sim/minecraft','--no-daemon','compileJava','test'],['sim/minecraft/gradlew','-p','sim/minecraft','--no-daemon','compileJava','-Pminecraft_version=26.2','-Ploader_version=0.19.5','-Pfabric_version=0.160.0+26.2','-Pno_remap=true','-Pjava_release=25'],['python3','scripts/build_minecraft_bedrock.py','--output',str(output/'bedrock-pack')],['node','--experimental-vm-modules','scripts/qa/test_minecraft_physics.cjs'],['cargo','build','--locked','-p','aarnn-mujoco','--features','service'],['python3','scripts/qa/test_rust_physics_service.py']]
    codes=[]
    for i,command in enumerate(commands):
        try:codes.append(run(command,output/f'run-{i}.log',2700 if args.lane=='legacy-neural' else 1200 if args.lane=='mujoco-population' else 300,command_env))
        except OSError as error:codes.append(127);result['capability_error']=str(error)
        if codes[-1]!=0 and not (args.lane=='legacy-neural' and i==5):break
    if args.lane in ('mujoco-managed','mujoco-viewer') and (output/'managed.json').exists():result['managed_mujoco']=json.loads((output/'managed.json').read_text())
    if args.lane=='legacy-neural' and (output/'legacy-neural.json').exists():result['legacy_neural']=json.loads((output/'legacy-neural.json').read_text())
    if args.lane in ('legacy-neural','mujoco-population') and (output/'population/population.json').exists():result['managed_population']=json.loads((output/'population/population.json').read_text())
    if args.lane=='mujoco-population' and len(codes)==len(commands):result['population_exit']=codes[-1]
    if args.lane=='habitats' and (output/'habitat-audit.json').exists():result['habitat_audit']=json.loads((output/'habitat-audit.json').read_text())
    if args.lane=='minecraft-service':result['minecraft_service_client']=bool(len(codes)==len(commands) and all(c==0 for c in codes))
    result.update(exit_codes=codes,status='pass' if not result.get('capability_error') and all(c==0 for c in codes) and result.get('native_import_exit',0)==0 and all(result['neural'].values()) else 'fail',elapsed_s=time.monotonic()-started)
    result['matrix_counts']={}
    report=matrix(result,output)
    for row in report['rows']:result['matrix_counts'][row['status']]=result['matrix_counts'].get(row['status'],0)+1
    (output/'result.json').write_text(json.dumps(result,indent=2)+'\n')
    print(f"{args.lane}: {result['status']}; evidence {output}")
    for path in sorted(output.glob('run-*.log')):print(path.read_text(),end='')
    return 0 if result['status']=='pass' else 1

if __name__=='__main__':raise SystemExit(main())
