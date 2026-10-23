#!/usr/bin/env python3
"""Read current repository/tool availability. No builds, installs or network calls."""
from __future__ import annotations
import argparse
import json
from pathlib import Path
import platform
import shutil
import subprocess
from pack_common import load_json

VERIFIED_PATHS=['AGENTS.md','.agent/PLANS.md','Cargo.toml','rust-toolchain.toml',
  'docs/specifications/distributed-whole-brain-emulator-v1.1.md',
  'docs/architecture/decisions/ADR-0007-managed-simulation-ingress.md',
  'scripts/robot_profiles.py','scripts/run_sim.sh','scripts/sim_content.py',
  'scripts/regenerate_simulator_assets.py','sim/content/catalog.json',
  'src/aer.rs','src/aer_transport.rs','src/peripheral.rs','src/tcp_aer_ipc_bridge.rs',
  'examples/nn_tcp_server.rs','examples/nn_uds_server.rs','src/bin/web_ui.rs',
  'web_ui/webgl-sim.js','sim/unity/Assets/NeuralMimicry/Runtime/NmRobotBase.cs',
  'sim/unreal/Source/NmAerBridge/Private/NmSimGameMode.cpp',
  'sim/minecraft/README.md','sim/minecraft/bedrock/README.md','aer_fabric_bridge/README.md',
  'tools/xtask/Cargo.toml']

def git(repo,*args):
    try:
        return subprocess.check_output(['git','-C',str(repo),*args],text=True,stderr=subprocess.DEVNULL,timeout=10).strip()
    except (OSError,subprocess.SubprocessError):
        return None

def audit(repo):
    repo=Path(repo).expanduser().resolve()
    if not (repo/'Cargo.toml').is_file():
        raise ValueError('Cargo.toml not found at supplied repository root')
    expected=load_json('manifests/robots.json')['aarnn_revision']['sha']
    head=git(repo,'rev-parse','HEAD')
    return dict(repository=str(repo),head=head,research_revision=expected,revision_matches=head==expected,
                git_status=git(repo,'status','--short'),host_architecture=platform.machine(),
                canonical_paths={p:(repo/p).exists() for p in VERIFIED_PATHS},
                new_backend_paths={p:(repo/p).exists() for p in ['sim/mujoco','sim/isaacsim']},
                command_paths={k:shutil.which(k) for k in ['cargo','rustc','python3','node','webots','Unity','UnrealEditor','java','nvidia-smi','isaacsim']},
                note='Read-only audit. Paths/commands present do not establish compatible engines, GPU/driver, model import or AER functionality. Follow root instructions before editing.')

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo',required=True)
    args=parser.parse_args()
    try:
        print(json.dumps(audit(args.repo),indent=2))
        return 0
    except (OSError,ValueError) as error:
        parser.exit(1,str(error)+'\n')

if __name__=='__main__':
    raise SystemExit(main())
