#!/usr/bin/env python3
"""Minecraft capability detection. Reads version/mod metadata, never account or credential files."""
import argparse
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import zipfile
import socket
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
MC_VERSION = '1.21.1'
MOD_VERSION = '0.1.0'
API_VERSION = '0.107.0+1.21.1'


def minecraft_home():
    if os.environ.get('NM_MINECRAFT_DIR'):
        return Path(os.environ['NM_MINECRAFT_DIR']).expanduser()
    if sys.platform == 'win32':
        return Path(os.environ.get('APPDATA', Path.home())) / '.minecraft'
    if sys.platform == 'darwin':
        return Path.home() / 'Library/Application Support/minecraft'
    return Path.home() / '.minecraft'


def java():
    candidates = []
    for key in ['NM_MINECRAFT_JAVA_HOME', 'JAVA_HOME']:
        if os.environ.get(key):
            candidates.append(Path(os.environ[key]) / 'bin' / ('java.exe' if os.name == 'nt' else 'java'))
    candidates += list(Path('/usr/lib/jvm').glob('*21*/bin/java'))
    if shutil.which('java'):
        candidates.append(Path(shutil.which('java')))
    for candidate in candidates:
        try:
            result = subprocess.run([str(candidate), '-version'], capture_output=True, text=True, timeout=5)
            text = result.stderr + result.stdout
            match = re.search(r'version "(\d+)(?:\.(\d+))?', text)
            major = int(match[2] if match and match[1] == '1' else match[1]) if match else 0
            if result.returncode == 0 and major >= 21:
                return dict(available=True, path=str(candidate.resolve()), major=major)
        except (OSError, subprocess.SubprocessError):
            pass
    return dict(available=False, reason='Java 21+ unavailable; set NM_MINECRAFT_JAVA_HOME')


def read_json(path, limit=1024*1024):
    if path.stat().st_size > limit:
        raise ValueError('metadata exceeds bound')
    return json.loads(path.read_text())


def detect(directory, game_dir=None):
    versions, errors = [], []
    for path in sorted((directory / 'versions').glob('*/*.json')):
        try:
            data = read_json(path)
            if not isinstance(data, dict) or not isinstance(data.get('libraries', []), list):
                raise ValueError('invalid version metadata')
            if any(not isinstance(entry, dict) or not isinstance(entry.get('name'), str) for entry in data.get('libraries', [])):
                raise ValueError('invalid library metadata')
            loaders = [entry['name'].split(':')[-1] for entry in data.get('libraries', [])
                       if entry.get('name', '').startswith('net.fabricmc:fabric-loader:')]
            versions.append(dict(id=data.get('id', path.stem), minecraft=data.get('inheritsFrom', data.get('id')),
                                 fabric_loader=loaders[0] if loaders else None))
        except (OSError, ValueError, KeyError, TypeError) as error:
            errors.append(f'{path.name}: {type(error).__name__}')
    def compatible(v):
        try:
            return v['minecraft'] == MC_VERSION and tuple(map(int, v['fabric_loader'].split('.'))) >= (0,17,3)
        except (TypeError, ValueError, AttributeError):
            return False
    matching = [v for v in versions if compatible(v)]
    game_dir = game_dir or (Path(os.environ['NM_MINECRAFT_GAME_DIR']).expanduser() if os.environ.get('NM_MINECRAFT_GAME_DIR') else None)
    resolved_game = game_dir or directory
    # Launcher profiles carry game directories. Read only these non-secret settings.
    if game_dir is None and matching and (directory / 'launcher_profiles.json').exists():
        try:
            data = read_json(directory / 'launcher_profiles.json')
            if not isinstance(data, dict) or not isinstance(data.get('profiles', {}), dict):
                raise ValueError('invalid launcher profiles')
            for profile in data.get('profiles', {}).values():
                if not isinstance(profile, dict):
                    raise ValueError('invalid launcher profile')
                if profile.get('lastVersionId') in [v['id'] for v in matching] and profile.get('gameDir'):
                    resolved_game = Path(profile['gameDir']).expanduser()
                    break
        except (OSError, ValueError, TypeError) as error:
            errors.append(f'launcher profile metadata: {type(error).__name__}')
    mods = []
    for path in sorted((resolved_game / 'mods').glob('*.jar')):
        try:
            with zipfile.ZipFile(path) as archive:
                if 'fabric.mod.json' not in archive.namelist():
                    raise ValueError('not a Fabric mod')
                if archive.getinfo('fabric.mod.json').file_size > 65536:
                    raise ValueError('mod metadata exceeds bound')
                data = json.loads(archive.read('fabric.mod.json'))
                if not isinstance(data, dict) or not isinstance(data.get('id'), str) or not isinstance(data.get('version'), str) or not isinstance(data.get('depends', {}), dict):
                    raise ValueError('invalid Fabric metadata')
                mods.append(dict(file=path.name, id=data.get('id'), version=data.get('version'),
                                 minecraft=data.get('depends', {}).get('minecraft')))
        except (OSError, ValueError, zipfile.BadZipFile, KeyError, TypeError) as error:
            errors.append(f'{path.name}: {type(error).__name__}')
    ids = [m['id'] for m in mods]
    for mod_id in set(ids):
        if ids.count(mod_id) > 1:
            errors.append(f'Duplicate mod id: {mod_id}; select one compatible JAR')
    api = [m for m in mods if m['id'] == 'fabric-api' and m['version'] == API_VERSION]
    aarnn = [m for m in mods if m['id'] == 'aarnn' and m['version'] == MOD_VERSION and m['minecraft'] == MC_VERSION]
    launcher = shutil.which('minecraft-launcher') or shutil.which('MinecraftLauncher.exe')
    if not launcher and sys.platform == 'darwin' and Path('/Applications/Minecraft.app').exists():
        launcher = '/Applications/Minecraft.app'
    return dict(schema_version=1, required_minecraft=MC_VERSION, directory=str(directory), game_directory=str(resolved_game),
                launcher=launcher, java=java(), versions=versions, compatible_profiles=matching,
                mods=mods, compatible_fabric_api=bool(api), compatible_aarnn_mod=bool(aarnn), errors=errors)


def select_edition(requested, java_report, bedrock_report):
    if requested != 'auto':
        return requested
    if java_report.get('compatible_profiles') and java_report.get('compatible_fabric_api') and java_report.get('compatible_aarnn_mod') and not java_report.get('errors'):
        return 'java'
    return 'bedrock' if bedrock_report.get('server') else 'java'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['doctor', 'java', 'launch', 'wait-bridge', 'edition'])
    parser.add_argument('--edition',choices=['auto','java','bedrock'],default=os.environ.get('NM_MINECRAFT_EDITION','auto'))
    parser.add_argument('--bedrock-dir',type=Path)
    parser.add_argument('--pid', type=int)
    parser.add_argument('--minecraft-dir', type=Path, default=minecraft_home())
    parser.add_argument('--game-dir', type=Path)
    parser.add_argument('--require', choices=['engine', 'bridge'], default='engine')
    args = parser.parse_args()
    if args.command == 'wait-bridge':
        deadline = time.monotonic() + 75
        while time.monotonic() < deadline:
            try:
                if not args.pid: raise ValueError('--pid required')
                os.kill(args.pid, 0)
                request = urllib.request.Request('http://127.0.0.1:62620/api/aarnn/health',
                    headers={'Authorization': 'Bearer '+os.environ.get('AARNN_MINECRAFT_TOKEN', '')})
                with urllib.request.urlopen(request, timeout=.5) as response:
                    data = json.loads(response.read(4097))
                expected = read_json(ROOT / 'sim/content/compiled.generated.json', 8*1024*1024)['digest']
                if data.get('content_digest') != expected:
                    raise ValueError('Companion content mismatch')
                print('Authenticated Minecraft companion ready'); return 0
            except ProcessLookupError:
                print('Minecraft companion exited before readiness', file=sys.stderr); return 3
            except (OSError, ValueError):
                time.sleep(.1)
        print('Minecraft companion did not become ready within 75 seconds', file=sys.stderr); return 3
    if args.command == 'java':
        result = java()
        if result['available']:
            print(result['path']); return 0
        print(result['reason'], file=sys.stderr); return 3
    result = detect(args.minecraft_dir, args.game_dir)
    import bedrock
    native = bedrock.detect(args.bedrock_dir)
    edition = select_edition(args.edition, result, native)
    if args.command == 'edition':
        print(edition);return 0
    if edition == 'bedrock' and args.require == 'engine':
        if args.command == 'launch':return bedrock.launch(native)
        print(json.dumps(native,indent=2));return 3 if native['errors'] else 0
    result['edition'] = edition
    result['bedrock_server_detected'] = native.get('server') is not None
    reasons = []
    if args.require == 'engine' and result['errors']:
        reasons.append('Unreadable or malformed installation metadata; inspect errors before launch')
    if not result['java']['available']:
        reasons.append(result['java']['reason'])
    if args.require == 'engine':
        if not result['launcher']: reasons.append('Minecraft launcher not found')
        if not result['compatible_profiles']: reasons.append('Fabric Loader >=0.17.3 for Minecraft 1.21.1 not installed')
        if not result['compatible_fabric_api']: reasons.append('Fabric API for Minecraft 1.21.1 not installed in the selected game directory')
        if not result['compatible_aarnn_mod']: reasons.append('AARNN 0.1.0 mod for Minecraft 1.21.1 not installed in the selected game directory')
    if args.require == 'bridge':
        if not (ROOT / 'sim/minecraft/build/libs/aarnn-minecraft-0.1.0-bridge.jar').is_file():
            reasons.append('Build the companion JAR with cargo xtask qa run --suite simulator-minecraft')
        if len(os.environ.get('AARNN_MINECRAFT_TOKEN', '')) < 24:
            reasons.append('Set AARNN_MINECRAFT_TOKEN to at least 24 characters in both bridge and Minecraft server environments')
        try:
            with socket.socket() as listener:
                listener.bind(('127.0.0.1', 62620))
        except OSError:
            reasons.append('Minecraft companion port 127.0.0.1:62620 is already in use')
    result.update(status='unavailable' if reasons else 'ready', reasons=reasons)
    print(json.dumps(result, indent=2))
    if reasons:
        print('No engine or brain was launched. See sim/minecraft/README.md. Use --no-engine on hosts serving only Rust brains.', file=sys.stderr)
        return 3
    if args.command == 'launch':
        command = ['open', result['launcher']] if sys.platform == 'darwin' else [result['launcher']]
        subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        print('Select the detected Fabric 1.21.1 profile in the launcher; no launcher account settings were changed.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
