#!/usr/bin/env bash
# Native mujoco dispatch; the canonical launcher validates capabilities/bindings.
set -euo pipefail
exec "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/run_sim.sh" --sim mujoco "$@"
