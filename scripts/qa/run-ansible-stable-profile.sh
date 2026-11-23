#!/usr/bin/env bash
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ansible_dir="${AARNN_ANSIBLE_DIR:-/home/pbisaacs/Developer/swarmhpc/swarmhpc/ansible}"

exec python3 "${repo_root}/scripts/qa/validate-ansible-stable-profile.py" \
  --ansible-dir "${ansible_dir}" "$@"
