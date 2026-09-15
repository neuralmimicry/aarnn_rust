#!/usr/bin/env bash
set -euo pipefail
exec "$(dirname "$0")/run_sim.sh" --sim minecraft "$@"
