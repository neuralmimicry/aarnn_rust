#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

DEFAULT_SPEC="${ROBOT_SPEC:-celegans=1}"
exec "$ROOT_DIR/scripts/run_multi_robot_webots.sh" --ui-mode web --robots "$DEFAULT_SPEC" "$@"
