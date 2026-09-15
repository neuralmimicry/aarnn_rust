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
MOD_VERSION = '0.1.0'

# Each entry is an independently built compatibility target.  Minecraft's
# client API is not treated as source-compatible across these profiles.
MINECRAFT_PROFILES = {
    '26.2': dict(minecraft='26.2', loader_min='0.19.5', fabric_api='0.160.0+26.2',
                 java_major=25, no_remap=True),
    '1.21.1': dict(minecraft='1.21.1', loader_min='0.17.3', fabric_api='0.107.0+1.21.1',
                   java_major=21, no_remap=False),
}
DEFAULT_MINECRAFT_VERSION = '1.21.1'


def profile_artifact(profile_id, classifier=''):
    """Return the build output for a profile without falling back across versions."""
    if profile_id not in MINECRAFT_PROFILES:
        raise ValueError(f'Unsupported Minecraft compatibility profile: {profile_id}')
    suffix = f'-{classifier}' if classifier else ''
    return ROOT / 'sim/minecraft/build/libs' / f'aarnn-minecraft-{profile_id}-{MOD_VERSION}{suffix}.jar'


def minecraft_home():
    if os.environ.get('NM_MINECRAFT_DIR'):
        return Path(os.environ['NM_MINECRAFT_DIR']).expanduser()
    if sys.platform == 'win32':
        return Path(os.environ.get('APPDATA', Path.home())) / '.minecraft'
    if sys.platform == 'darwin':
        return Path.home() / 'Library/Application Support/minecraft'
    return Path.home() / '.minecraft'


def java(min_major=21):
    candidates = []
    for key in ['NM_MINECRAFT_JAVA_HOME', 'JAVA_HOME']:
        if os.environ.get(key):
            candidates.append(Path(os.environ[key]) / 'bin' / ('java.exe' if os.name == 'nt' else 'java'))
    candidates += sorted(Path('/usr/lib/jvm').glob('*/bin/java'), reverse=True)
    if shutil.which('java'):
        candidates.append(Path(shutil.which('java')))
    seen = set()
    for candidate in candidates:
        if candidate in seen:
            continue
        seen.add(candidate)
        try:
            result = subprocess.run([str(candidate), '-version'], capture_output=True, text=True, timeout=5)
            text = result.stderr + result.stdout
            match = re.search(r'version "(\d+)(?:\.(\d+))?', text)
            major = int(match[2] if match and match[1] == '1' else match[1]) if match else 0
            if result.returncode == 0 and major >= min_major:
                return dict(available=True, path=str(candidate.resolve()), major=major)
        except (OSError, subprocess.SubprocessError):
            pass
    return dict(available=False, reason=f'Java {min_major}+ unavailable; set NM_MINECRAFT_JAVA_HOME')


def read_json(path, limit=1024*1024):
    if path.stat().st_size > limit:
        raise ValueError('metadata exceeds bound')
    return json.loads(path.read_text())


def _version_tuple(value):
    try:
        return tuple(int(part) for part in re.findall(r'\d+', value))
    except (TypeError, ValueError):
        return ()


def _game_dirs(directory, game_dir, versions):
    """Return launcher-selected game directories without reading account data."""
    if game_dir is not None:
        return {version['id']: [game_dir] for version in versions}
    selected = {version['id']: [] for version in versions}
    profile_path = directory / 'launcher_profiles.json'
    if profile_path.exists():
        try:
            data = read_json(profile_path)
            if not isinstance(data, dict) or not isinstance(data.get('profiles', {}), dict):
                raise ValueError('invalid launcher profiles')
            for profile in data['profiles'].values():
                if not isinstance(profile, dict):
                    raise ValueError('invalid launcher profile')
                version_id = profile.get('lastVersionId')
                configured = profile.get('gameDir')
                if version_id in selected and configured:
                    path = Path(configured).expanduser()
                    if path not in selected[version_id]:
                        selected[version_id].append(path)
        except (OSError, ValueError, TypeError) as error:
            selected['_errors'] = [f'launcher profile metadata: {type(error).__name__}']
    for version in versions:
        if not selected.get(version['id']):
            selected[version['id']] = [directory]
    return selected


def _read_mods(game_dir):
    mods, errors = [], []
    for path in sorted((game_dir / 'mods').glob('*.jar')):
        try:
            with zipfile.ZipFile(path) as archive:
                if 'fabric.mod.json' not in archive.namelist():
                    raise ValueError('not a Fabric mod')
                if archive.getinfo('fabric.mod.json').file_size > 65536:
                    raise ValueError('mod metadata exceeds bound')
                data = json.loads(archive.read('fabric.mod.json'))
                if (not isinstance(data, dict) or not isinstance(data.get('id'), str)
                        or not isinstance(data.get('version'), str)
                        or not isinstance(data.get('depends', {}), dict)):
                    raise ValueError('invalid Fabric metadata')
                mods.append(dict(file=path.name, id=data['id'], version=data['version'],
                                 minecraft=data.get('depends', {}).get('minecraft')))
        except (OSError, ValueError, zipfile.BadZipFile, KeyError, TypeError) as error:
            errors.append(f'{path.name}: {type(error).__name__}')
    ids = [mod['id'] for mod in mods]
    for mod_id in set(ids):
        if ids.count(mod_id) > 1:
            errors.append(f'Duplicate mod id: {mod_id}; select one compatible JAR')
    return mods, errors


def detect(directory, game_dir=None, minecraft_version=None):
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
    requested = minecraft_version or os.environ.get('NM_MINECRAFT_VERSION')
    if requested and requested not in MINECRAFT_PROFILES:
        errors.append(f'Unsupported Minecraft compatibility profile: {requested}')
        requested = None
    launcher_dirs = _game_dirs(directory,
                               game_dir or (Path(os.environ['NM_MINECRAFT_GAME_DIR']).expanduser()
                                            if os.environ.get('NM_MINECRAFT_GAME_DIR') else None), versions)
    errors.extend(launcher_dirs.pop('_errors', []))
    reports = []
    for profile_id, profile in MINECRAFT_PROFILES.items():
        if requested and requested != profile_id:
            continue
        for version in versions:
            if version['minecraft'] != profile['minecraft']:
                continue
            loader_ok = (_version_tuple(version['fabric_loader']) >= _version_tuple(profile['loader_min'])
                         if version['fabric_loader'] else False)
            if not loader_ok:
                continue
            for resolved_game in launcher_dirs.get(version['id'], [directory]):
                mods, local_errors = _read_mods(resolved_game)
                api = [mod for mod in mods if mod['id'] == 'fabric-api' and mod['version'] == profile['fabric_api']
                       and (mod['minecraft'] in (None, profile['minecraft']) or str(mod['minecraft']).startswith('~'+profile['minecraft']))]
                aarnn = [mod for mod in mods if mod['id'] == 'aarnn' and mod['version'] == MOD_VERSION
                         and mod['minecraft'] == profile['minecraft']]
                java_report = java(profile['java_major'])
                if java_report.get('available') and java_report.get('major', 0) < profile['java_major']:
                    java_report = dict(available=False,
                                       reason=f"Java {profile['java_major']}+ unavailable; detected Java {java_report.get('major')}")
                reports.append(dict(profile_id=profile_id, minecraft=profile['minecraft'],
                                    loader_min=profile['loader_min'], fabric_api=profile['fabric_api'],
                                    java_major=profile['java_major'], no_remap=profile['no_remap'],
                                    id=version['id'], fabric_loader=version['fabric_loader'],
                                    game_directory=str(resolved_game), java=java_report,
                                    mods=mods, compatible_fabric_api=bool(api),
                                    compatible_aarnn_mod=bool(aarnn), errors=local_errors))
    # Prefer a complete installed target, then the newest recognized target. The
    # explicit version option/env var above narrows this to one profile.
    def ready(report):
        return (report['java']['available'] and report['compatible_fabric_api']
                and report['compatible_aarnn_mod'] and not report['errors'])
    reports.sort(key=lambda report: (ready(report), _version_tuple(report['minecraft'])), reverse=True)
    selected = reports[0] if reports else None
    default_profile = MINECRAFT_PROFILES.get(requested or DEFAULT_MINECRAFT_VERSION)
    launcher = shutil.which('minecraft-launcher') or shutil.which('MinecraftLauncher.exe')
    if not launcher and sys.platform == 'darwin' and Path('/Applications/Minecraft.app').exists():
        launcher = '/Applications/Minecraft.app'
    matching = [report for report in reports if report['java']['available']]
    if selected:
        errors.extend(selected['errors'])
    return dict(schema_version=1, required_minecraft=selected['minecraft'] if selected else default_profile['minecraft'],
                directory=str(directory), game_directory=selected['game_directory'] if selected else str(game_dir or directory),
                launcher=launcher, java=selected['java'] if selected else java(default_profile['java_major']),
                versions=versions, compatible_profiles=matching,
                compatible_loader_profiles=reports,
                profiles=reports, mods=selected['mods'] if selected else [],
                compatible_fabric_api=selected['compatible_fabric_api'] if selected else False,
                compatible_aarnn_mod=selected['compatible_aarnn_mod'] if selected else False,
                selected_profile=selected['profile_id'] if selected else None, errors=errors)


def select_edition(requested, java_report, bedrock_report):
    if requested != 'auto':
        return requested
    # Java is the preferred frontend whenever its launcher and a recognized
    # Java runtime/profile are available. Engine preflight below still reports
    # missing Fabric/API/AARNN jars; auto mode must not hide that by switching
    # to a Bedrock frontend merely because a BDS server happens to be installed.
    java_available = (java_report.get('launcher') and
                      java_report.get('java', {}).get('available') and
                      java_report.get('compatible_loader_profiles'))
    java_complete = (java_report.get('compatible_profiles') and
                     java_report.get('compatible_fabric_api') and
                     java_report.get('compatible_aarnn_mod') and
                     not java_report.get('errors'))
    if java_available or java_complete:
        return 'java'
    return 'bedrock' if bedrock_report.get('server') else 'java'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['doctor', 'java', 'launch', 'wait-bridge', 'edition', 'artifact'])
    parser.add_argument('--edition',choices=['auto','java','bedrock'],default=os.environ.get('NM_MINECRAFT_EDITION','auto'))
    parser.add_argument('--bedrock-dir',type=Path)
    parser.add_argument('--pid', type=int)
    parser.add_argument('--minecraft-dir', type=Path, default=minecraft_home())
    parser.add_argument('--game-dir', type=Path)
    parser.add_argument('--minecraft-version', choices=sorted(MINECRAFT_PROFILES),
                        default=os.environ.get('NM_MINECRAFT_VERSION'))
    parser.add_argument('--kind', choices=['mod', 'bridge'], default='mod')
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
        profile = MINECRAFT_PROFILES.get(args.minecraft_version or DEFAULT_MINECRAFT_VERSION)
        result = java(profile['java_major'])
        if result['available']:
            print(result['path']); return 0
        print(result['reason'], file=sys.stderr); return 3
    result = detect(args.minecraft_dir, args.game_dir, args.minecraft_version)
    import bedrock
    native = bedrock.detect(args.bedrock_dir)
    edition = select_edition(args.edition, result, native)
    if args.command == 'edition':
        print(edition);return 0
    if args.command == 'artifact':
        profile_id = result.get('selected_profile') or args.minecraft_version or DEFAULT_MINECRAFT_VERSION
        path = profile_artifact(profile_id, 'bridge' if args.kind == 'bridge' else '')
        print(path)
        return 0 if path.is_file() else 3
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
        profile_id = result.get('selected_profile') or args.minecraft_version or DEFAULT_MINECRAFT_VERSION
        profile = MINECRAFT_PROFILES[profile_id]
        if not any(report.get('profile_id') == profile_id for report in result.get('compatible_loader_profiles', [])):
            reasons.append(f"Fabric Loader >={profile['loader_min']} for Minecraft {profile['minecraft']} not installed")
        if not result['compatible_fabric_api']:
            reasons.append(f"Fabric API {profile['fabric_api']} not installed in the selected game directory")
        if not result['compatible_aarnn_mod']:
            reasons.append(f"AARNN {MOD_VERSION} mod for Minecraft {profile['minecraft']} not installed in the selected game directory")
    if args.require == 'bridge':
        profile_id = result.get('selected_profile') or args.minecraft_version or DEFAULT_MINECRAFT_VERSION
        if not profile_artifact(profile_id, 'bridge').is_file():
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
        print(f"Select the detected Fabric {result.get('selected_profile') or result['required_minecraft']} profile in the launcher; no launcher account settings were changed.")
    return 0


if __name__ == '__main__':
    sys.exit(main())
