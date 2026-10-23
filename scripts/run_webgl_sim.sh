#!/usr/bin/env bash
# Browser WebGL simulator launcher. The browser uses the authenticated web_ui
# gateway; the Rust cluster remains the only neural executor.
# --physics-service --robots <profile>=1 selects native MuJoCo authority;
# --token-file <private path> stores its independent simulation token.
set -euo pipefail
exec "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/run_sim.sh" --sim webgl "$@"
