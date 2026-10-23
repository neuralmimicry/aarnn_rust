#!/usr/bin/env bash
# Native isaacsim dispatch; the canonical launcher validates capabilities/bindings.
set -euo pipefail
exec "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/run_sim.sh" --sim isaacsim "$@"
