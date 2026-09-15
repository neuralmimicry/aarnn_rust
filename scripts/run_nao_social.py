#!/usr/bin/env python3
"""Run a fresh NAO social reference brain and bounded local simulator/chat interfaces."""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time

from build_nao_social import build
from nao_social import ROOT, CONTRACT, Hub
from nao_social_server import Body, BoundedHttp, handler, tcp_loop, uds_loop


def listener(preferred):
    stream = socket.socket()
    stream.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    try:
        stream.bind(('127.0.0.1', preferred))
    except OSError:
        stream.bind(('127.0.0.1', 0))
    stream.listen(2)
    return stream


def wait_port(process, port, timeout=180):
    deadline = time.monotonic()+timeout
    while time.monotonic()<deadline:
        if process.poll() is not None:
            raise RuntimeError('Child exited before readiness; inspect the run log')
        try:
            with socket.create_connection(('127.0.0.1', port), timeout=.2):
                return
        except OSError:
            time.sleep(.1)
    raise RuntimeError('Child readiness deadline exceeded')


def stop_process(process):
    if process.poll() is None:
        # Only signal a group when this task explicitly created its session.
        # This also stops engine shell wrappers and their owned children.
        group = os.name == 'posix' and os.getpgid(process.pid) == process.pid
        # Let managed BDS forward its console stop and close its world first.
        process.terminate()
        try:
            process.wait(timeout=45)
        except subprocess.TimeoutExpired:
            if group:
                os.killpg(process.pid, signal.SIGKILL)
            else:
                process.kill()
            process.wait()
        if group:
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--sim', choices=['webgl', 'webots', 'unity', 'unreal', 'minecraft'], default='webgl')
    p.add_argument('--no-engine', action='store_true', help='Run brain/proxy only; print simulator connection settings')
    p.add_argument('--http-port', type=int, default=62621)
    p.add_argument('--body-port', type=int, default=7890)
    p.add_argument('--run-dir', type=Path)
    p.add_argument('--minecraft-edition', choices=['auto', 'java', 'bedrock'], default=os.environ.get('NM_MINECRAFT_EDITION', 'auto'))
    p.add_argument('--bedrock-dir', type=Path)
    args = p.parse_args()
    if not 0 <= args.http_port <= 65535 or not 0 <= args.body_port <= 65535:
        p.error('Ports must be in 0..65535')
    binary = ROOT/'target/release/examples/nn_tcp_server'
    if not binary.is_file():
        p.error('Build first: cargo build --locked --release --features ui,robot_io --example nn_tcp_server')
    engine = None
    if not args.no_engine and args.sim=='webots':
        engine = shutil.which('webots')
        if not engine:
            p.error('Webots unavailable; install it or use --no-engine')
    if not args.no_engine and args.sim=='unreal':
        engine = Path(os.environ.get('UE_ENGINE', str(Path.home()/'Developer/Engine')))/'Binaries/Linux/UnrealEditor'
        if not engine.is_file():
            p.error('Unreal Editor unavailable; set UE_ENGINE or use --no-engine')
    minecraft_edition = None
    if args.sim=='minecraft':
        from minecraft import java, select_edition, detect, minecraft_home
        from bedrock import detect as detect_bedrock
        minecraft_java = java().get('path')
        if not minecraft_java:
            p.error('Java 21+ is required by the Minecraft companion')
        minecraft_edition = select_edition(args.minecraft_edition, detect(minecraft_home()), detect_bedrock(args.bedrock_dir))
        if not (ROOT/'sim/minecraft/build/libs/aarnn-minecraft-1.21.1-0.1.0-bridge.jar').is_file():
            p.error('Build the Minecraft JARs first; see sim/minecraft/README.md')
    parent = ROOT/'target/nao-social'
    parent.mkdir(parents=True, exist_ok=True)
    if args.run_dir:
        directory = args.run_dir.resolve()
        directory.mkdir(mode=0o700)  # Never reuse a session or overwrite a world.
    else:
        directory = Path(tempfile.mkdtemp(prefix='run-', dir=parent))
    children, handles, sockets, threads, engines = [], [], [], [], []
    stop = threading.Event()
    http = body = None
    temporary_world = None
    def child(command, name, env):
        log = (directory/(name+'.log')).open('w')
        handles.append(log)
        process = subprocess.Popen([str(x) for x in command], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        children.append(process)
        return process
    def halt(*_args):
        stop.set()
    signal.signal(signal.SIGINT, halt)
    signal.signal(signal.SIGTERM, halt)
    try:
        raw = (ROOT/'network_nao.json').read_bytes()
        model = build(json.loads(raw), hashlib.sha256(raw).hexdigest())
        snapshot = directory/'network_nao_social.json'
        snapshot.write_text(json.dumps(model, separators=(',', ':'))+'\n')
        backend_reservation = listener(0)
        backend_port = backend_reservation.getsockname()[1]
        backend_reservation.close()
        env = dict(os.environ, RAYON_NUM_THREADS=os.environ.get('RAYON_NUM_THREADS', '4'), NM_REALTIME_IPC='1')
        rust = child([binary, '--tcp', f'127.0.0.1:{backend_port}', '--network', snapshot, '--sensory', '282', '--output', str(CONTRACT['output'])], 'rust', env)
        wait_port(rust, backend_port)
        # A failed import must not silently become a default neural network.
        log = (directory/'rust.log').read_text()
        if any(s in log for s in ('failed parsing snapshot', 'startup snapshot import failed', 'continuing with defaults', 'failed reading snapshot')):
            raise RuntimeError('Rust rejected the social snapshot')
        adapter_token, join_token = secrets.token_urlsafe(32), secrets.token_urlsafe(32)
        hub = Hub()
        body = Body(('127.0.0.1', backend_port), hub)
        try:
            http = BoundedHttp(('127.0.0.1', args.http_port), handler(body, adapter_token, join_token))
        except OSError:
            http = BoundedHttp(('127.0.0.1', 0), handler(body, adapter_token, join_token))
        url = f'http://127.0.0.1:{http.server_port}'
        tcp = listener(args.body_port)
        sockets.append(tcp)
        body_port = tcp.getsockname()[1]
        # Keep Unix socket paths below the operating-system limit independently
        # of the workspace/run directory length.
        uds_dir = tempfile.TemporaryDirectory(prefix='aarnn-nao-')
        uds_path = str(Path(uds_dir.name)/'body.sock')
        uds = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        uds.bind(uds_path)
        sockets.append(uds)
        settings = dict(schema=CONTRACT['schema'], url=url, adapter_token=adapter_token,
                        join_token=join_token, body_port=body_port, uds=uds_path,
                        model_sha256=hashlib.sha256(snapshot.read_bytes()).hexdigest())
        session_file = directory/'session.json'
        with session_file.open('x') as stream:
            os.chmod(session_file, 0o600)
            json.dump(settings, stream, indent=2)
        env.update(NM_NAO_SESSION_FILE=str(session_file), NM_NAO_CHAT_URL=url,
                   NM_NAO_ADAPTER_TOKEN=adapter_token, NM_NAO_JOIN_TOKEN=join_token,
                   NM_NAO_SOCKET=uds_path, NM_AARNN_BASE_PORT=str(body_port), NM_AARNN_HOST='127.0.0.1', NM_UE_ROBOTS='nao=1',
                   NM_CAMERA_RETINA_WIDTH='8', NM_CAMERA_RETINA_HEIGHT='6', NM_IPC_LOCKSTEP_MAX_WAIT_MS='5000')
        with (directory/'environment.sh').open('x') as stream:
            os.chmod(stream.name, 0o600)
            for key in ('NM_NAO_SESSION_FILE', 'NM_NAO_CHAT_URL', 'NM_NAO_ADAPTER_TOKEN', 'NM_NAO_JOIN_TOKEN', 'NM_NAO_SOCKET', 'NM_AARNN_BASE_PORT', 'NM_AARNN_HOST', 'NM_UE_ROBOTS', 'NM_CAMERA_RETINA_WIDTH', 'NM_CAMERA_RETINA_HEIGHT', 'NM_IPC_LOCKSTEP_MAX_WAIT_MS'):
                stream.write('export '+key+'='+shlex.quote(env[key])+'\n')
        for target, arguments in ((http.serve_forever, ()), (tcp_loop, (tcp, body, stop)), (uds_loop, (uds, body, stop))):
            thread = threading.Thread(target=target, args=arguments, daemon=True)
            threads.append(thread)
            thread.start()
        print(f'NAO social reference ready. Chat: {url}/ · WebGL: {url}/world?robot=nao&network_id=nao&nao_social=1', flush=True)
        print(f'Private session/invitation: {session_file}\nBody: 127.0.0.1:{body_port} (250/40); Rust social model: 282/49\nSource {directory}/environment.sh before starting a native simulator.', flush=True)
        if args.sim=='minecraft':
            token = secrets.token_urlsafe(32)
            env['AARNN_MINECRAFT_TOKEN'] = token
            companion_reservation = listener(62620)
            companion_port = companion_reservation.getsockname()[1]
            companion_reservation.close()
            bridge = child([minecraft_java, '-jar', ROOT/'sim/minecraft/build/libs/aarnn-minecraft-1.21.1-0.1.0-bridge.jar', '--port', str(companion_port), '--base-port', str(body_port), '--profiles', 'nao'], 'minecraft-bridge', env)
            wait_port(bridge, companion_port)
            settings.update(minecraft_edition=minecraft_edition, minecraft_port=companion_port, minecraft_token=token)
            session_file.write_text(json.dumps(settings, indent=2)+'\n')
            with (directory/'environment.sh').open('a') as stream:
                stream.write('export AARNN_MINECRAFT_TOKEN='+shlex.quote(token)+'\n')
            from nao_social_install import prepare_minecraft
            install = prepare_minecraft(directory, settings, minecraft_edition, args.bedrock_dir)
            print('Isolated Minecraft installation: '+str(install), flush=True)
            if not args.no_engine:
                if minecraft_edition=='bedrock':
                    # Uses the existing allocator on every launch; no unmanaged
                    # BDS spawn, existing-world writer or listener is disturbed.
                    engine_process = child([sys.executable, ROOT/'scripts/minecraft.py', 'launch', '--edition', 'bedrock', '--bedrock-dir', install], 'bedrock', env)
                    engines.append(engine_process)
                    print('Bedrock server is starting; see bedrock.log for allocated ports and console instructions.', flush=True)
                else:
                    print('Use a separate Fabric 1.21.1 launcher profile with this game directory, then join the lab world. See sim/nao/README.md.', flush=True)
        elif engine and args.sim=='unreal':
            engines.append(child([engine, ROOT/'sim/unreal/NeuralMimicrySim.uproject', '/Engine/Maps/Entry?game=/Script/NmAerBridge.NmSimGameMode', '-game', '-windowed', '-resx=1280', '-resy=900'], 'unreal', env))
        elif engine and args.sim=='webots':
            world = (ROOT/'webots_world/worlds/neuroworld.wbt').read_text()
            world = world.replace('controllerArgs [ "NM_BRAINS=default" ]', 'window "nao_chat" controllerArgs [ "NM_BRAINS=default" ]')
            temporary_world = ROOT/'webots_world/worlds'/('.nao_social_'+directory.name+'.wbt')
            temporary_world.write_text(world)
            engines.append(child([engine, temporary_world], 'webots', env))
        elif args.sim=='unity':
            print('Open sim/unity and press Play in a NAO scene; set the connector to the printed body port. Chat uses NM_NAO_SESSION_FILE.', flush=True)
        while not stop.wait(.5):
            if any(process.poll()==0 for process in engines):
                return 0
            if any(process.poll() is not None for process in children):
                raise RuntimeError('A task-owned process exited; stopping the reference session. Inspect run logs.')
        return 0
    finally:
        stop.set()
        if http:
            http.shutdown()
            http.server_close()
        if body:
            body.hub.detach(body.hub.body)
            body.close()
        for process in reversed(children):
            stop_process(process)
        for thread in threads:
            thread.join(timeout=1)
        for stream in sockets:
            stream.close()
        for handle in handles:
            handle.close()
        if temporary_world:
            temporary_world.unlink(missing_ok=True)
        if 'uds_dir' in locals():
            uds_dir.cleanup()


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, RuntimeError) as error:
        print('NAO social unavailable: '+str(error), file=sys.stderr)
        sys.exit(2)
