#!/usr/bin/env bash
# One root entry point for all canonical simulator targets and profile recipes.
set -euo pipefail
exec "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/scripts/run_sim.sh" "$@"
