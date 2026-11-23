#!/usr/bin/env bash
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "${ROOT_DIR}/scripts/build_container.sh" "$@"
