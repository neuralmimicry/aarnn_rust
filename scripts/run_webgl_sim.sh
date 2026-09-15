#!/usr/bin/env bash
# Browser WebGL simulator launcher. The browser uses the authenticated web_ui
# gateway; the Rust cluster remains the only neural executor.
set -euo pipefail
exec "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/run_sim.sh" --sim webgl "$@"
