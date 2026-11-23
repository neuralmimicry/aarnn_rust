#!/usr/bin/env bash
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
# Native isaacsim dispatch; the canonical launcher validates capabilities/bindings.
set -euo pipefail
exec "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/run_sim.sh" --sim isaacsim "$@"
