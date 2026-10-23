#!/usr/bin/env bash
# --physics-service starts a native authority for the Java/BDS observers.
# Prepare their private pins with scripts/prepare_minecraft_physics.py.
set -euo pipefail
exec "$(dirname "$0")/run_sim.sh" --sim minecraft "$@"
