#!/usr/bin/env python3
"""Bounded Minecraft QA lanes with retained evidence and explicit capability failures."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
PROJECT = ROOT / 'sim/minecraft'
sys.path.insert(0, str(ROOT / 'scripts'))
from minecraft import java, detect, minecraft_home


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def capture(cmd):
    try: return subprocess.check_output(cmd, cwd=ROOT, text=True, stderr=subprocess.DEVNULL, timeout=10).strip()
    except (OSError, subprocess.SubprocessError): return None


def run(command, directory, env, log, timeout):
    process = subprocess.Popen(command, cwd=directory, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    try:
        code = process.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGTERM)
        try: process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL); process.wait()
        raise TimeoutError(f'Command exceeded {timeout} seconds')
    if code: raise RuntimeError(f'Command failed ({code}): {command[0]}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--lane', choices=['contract', 'world', 'visual', 'neural', 'bedrock', 'bedrock-native'], default='contract')
    args = parser.parse_args()
    base = ROOT / 'target/qa/minecraft'; base.mkdir(parents=True, exist_ok=True)
    output = Path(tempfile.mkdtemp(prefix=args.lane+'-', dir=base))
    toolchain = java(); env = dict(os.environ, NM_MINECRAFT_RESULT_DIR=str(output))
    if toolchain.get('path'):
        env['JAVA_HOME'] = str(Path(toolchain['path']).parent.parent)
    started = time.monotonic()
    result = dict(scenario='SIM-MINECRAFT-001', schema_version=1, lane=args.lane,
        started_utc=datetime.now(timezone.utc).isoformat(), seed=481516,
        git_revision=capture(['git', 'rev-parse', 'HEAD']), git_dirty=bool(capture(['git','status','--porcelain'])),
        platform=dict(os=platform.system(), architecture=platform.machine()),
        toolchain=dict(java=toolchain, python=platform.python_version(), node=capture(['node','--version'])),
        content_digest=json.loads((ROOT / 'sim/content/compiled.generated.json').read_text())['digest'],
        scenario_digest=digest(ROOT / 'qa/scenarios/SIM-MINECRAFT-001.toml'),
        input_digest=digest(ROOT / 'scripts/qa/minecraft_oracle.cjs'),
        capabilities=detect(minecraft_home()), neural_state_digest=None, event_digest=None, checkpoint=None,
        reproduction=f'cargo xtask qa run --suite simulator-minecraft{("-"+args.lane) if args.lane != "contract" else ""}')
    gradle = [str(PROJECT / ('gradlew.bat' if os.name == 'nt' else 'gradlew')), '--no-daemon']
    try:
        if args.lane!='bedrock' and not toolchain['available']: raise RuntimeError(toolchain['reason'])
        with (output / 'run.log').open('w') as log:
            if args.lane == 'bedrock-native':
                run([sys.executable,'scripts/build_minecraft_bedrock.py'],ROOT,env,log,120)
                run([sys.executable,'scripts/qa/probe_minecraft_bedrock.py'],ROOT,env,log,1800)
                report=json.loads((output/'bedrock-native.json').read_text())
                if report['status']!='pass' or len(report['profiles'])!=6 or not report['clean_exit']:
                    raise RuntimeError('Native Bedrock acceptance incomplete')
                result['native_bedrock_execution']=report
                run([sys.executable,'scripts/package_minecraft_bedrock_world.py','--evidence',str(output)],ROOT,env,log,120)
            elif args.lane == 'bedrock':
                run([sys.executable,'scripts/build_minecraft_bedrock.py'],ROOT,env,log,120)
                run(['node','scripts/qa/minecraft_oracle.cjs'],ROOT,env,log,60)
                run([sys.executable,'scripts/qa/test_bedrock_detection.py'],ROOT,env,log,60)
                run([sys.executable,'scripts/qa/test_bedrock_ports.py'],ROOT,env,log,60)
                run([sys.executable,'scripts/qa/test_bedrock_metadata.py'],ROOT,env,log,60)
                run(['node','--experimental-vm-modules','scripts/qa/test_minecraft_bedrock.cjs'],ROOT,env,log,90)
                run(['npm','ci','--ignore-scripts','--no-audit','--no-fund'],PROJECT/'bedrock',env,log,180)
                run(['npm','run','typecheck'],PROJECT/'bedrock',env,log,90)
                import bedrock
                result['bedrock_capability']=bedrock.detect()
                result['native_bedrock_execution']='not-run; this lane validates packs, official API types and controlled API fixtures'
            elif args.lane == 'contract':
                run([sys.executable,'scripts/regenerate_simulator_assets.py','--check'],ROOT,env,log,120)
                run([sys.executable,'scripts/qa/test_minecraft_detection.py'],ROOT,env,log,60)
                run(gradle+['build'],PROJECT,env,log,1200)
                test_dir=PROJECT / 'build/test-results/test'
                for f in test_dir.glob('*.xml'): shutil.copy2(f,output/f.name)
                tests=[ET.parse(f).getroot() for f in output.glob('TEST-*.xml')]
                if not tests or sum(int(t.get('tests','0')) for t in tests)<13 or any(int(t.get('failures','0'))+int(t.get('errors','0'))+int(t.get('skipped','0')) for t in tests):
                    raise RuntimeError('Required JVM tests absent, skipped or failed')
            elif args.lane == 'world':
                # A clean process exit is required. An upstream storage-close stall is a lane failure,
                # even when the retained GameTest assertions passed. Never turn timeout into a pass.
                for p in [PROJECT/'build/gametest/acceptance.json',PROJECT/'build/gametest/build/gametest-results.xml',PROJECT/'build/gametest/clean-exit.json']:
                    p.unlink(missing_ok=True)
                run(gradle+['runGameTest'],PROJECT,env,log,300)
                report = PROJECT/'build/gametest/build/gametest-results.xml'
                tests = ET.parse(report).getroot()
                if len(tests.findall('.//testcase')) != 1 or tests.find('.//testcase').get('name') != 'habitatgametests.allsixprofiles' or any(tests.findall('.//'+tag) for tag in ('failure','error','skipped')):
                    raise RuntimeError('Required native six-profile GameTest did not pass')
                # Only a successful child exit can certify this saved world. Bind the
                # certificate to the report, source and every saved file used by the packer.
                certified = [PROJECT/'build/gametest/acceptance.json', report]
                certified += [p for p in (PROJECT/'src').rglob('*') if p.is_file()]
                world = PROJECT/'build/gametest/world'
                certified += [world/'level.dat']
                for folder in ('region', 'entities'):
                    certified += list((world/folder).glob('*.mca'))
                marker = dict(content_digest=result['content_digest'], status='pass',
                    sha256={str(p.relative_to(PROJECT)):digest(p) for p in sorted(certified)})
                (PROJECT/'build/gametest/clean-exit.json').write_text(json.dumps(marker,indent=2)+'\n')
                run(gradle+['build','worldPack'],PROJECT,env,log,180)
            elif args.lane == 'visual':
                (PROJECT/'build/visual/visual-result.json').unlink(missing_ok=True)
                (PROJECT/'build/visual/config/aarnn.json').unlink(missing_ok=True)
                shutil.rmtree(PROJECT/'build/visual/screenshots',ignore_errors=True)
                run(gradle+['runVisualTest'],PROJECT,env,log,600)
                report=json.loads((PROJECT/'build/visual/visual-result.json').read_text())
                if report.get('status')!='pass' or report.get('screenshots')!=12 or report.get('content_digest')!=result['content_digest']:
                    raise RuntimeError('Native visual acceptance missing or stale')
            else:
                run([sys.executable,'scripts/qa/probe_minecraft_neural.py'],ROOT,env,log,2500)
                report=json.loads((output/'neural.json').read_text())
                selected=env.get('NM_MINECRAFT_TEST_PROFILES','')
                names=selected.split(',') if selected else [p['id'] for p in json.loads((ROOT/'sim/content/compiled.generated.json').read_text())['profiles']]
                result['verified_profiles']=names
                if sorted(p['profile'] for p in report['profiles'])!=sorted(names) or any(p['status']!='pass' for p in report['profiles']):
                    raise RuntimeError('Every selected real profile must pass')
        result.update(status='pass',reason='all required lane checks passed')
    except Exception as error:
        result.update(status='fail',reason=str(error))
    for source in [PROJECT/'build/gametest/acceptance.json',PROJECT/'build/gametest/build/gametest-results.xml',PROJECT/'build/gametest/clean-exit.json',PROJECT/'build/visual/visual-result.json']:
        if source.exists(): shutil.copy2(source,output/source.name)
    if args.lane=='visual':
        screenshots=PROJECT/'build/visual/screenshots'
        if screenshots.exists(): shutil.copytree(screenshots,output/'screenshots',dirs_exist_ok=True)
    result['artefact_sha256']={f.name:digest(f) for f in (PROJECT/'build/libs').glob('*.jar')}
    result['elapsed_seconds']=time.monotonic()-started
    if os.name=='posix':
        import resource
        usage=resource.getrusage(resource.RUSAGE_CHILDREN)
        result['resources']=dict(cpu_seconds=usage.ru_utime+usage.ru_stime,peak_child_rss_bytes=usage.ru_maxrss*(1 if sys.platform=='darwin' else 1024))
    (output/'result.json').write_text(json.dumps(result,indent=2)+'\n')
    if (output/'run.log').exists(): print((output/'run.log').read_text()[-14000:])
    print(f"Minecraft {args.lane}: {result['status']}; {result['reason']}; evidence {output}")
    return 0 if result['status']=='pass' else 1


if __name__=='__main__': sys.exit(main())
