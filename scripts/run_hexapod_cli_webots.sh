#!/usr/bin/env bash
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

DEFAULT_SPEC="${ROBOT_SPEC:-hexapod=1}"
exec "$ROOT_DIR/scripts/run_multi_robot_webots.sh" --ui-mode cli --robots "$DEFAULT_SPEC" "$@"
