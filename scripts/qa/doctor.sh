#!/usr/bin/env bash
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"
product=all-available
if (($#)); then
  [[ "$1" == "--product" && $# -eq 2 ]] || { echo "usage: $0 [--product PRODUCT]" >&2; exit 2; }
  product="$2"
fi
cargo xtask doctor --product "$product"
