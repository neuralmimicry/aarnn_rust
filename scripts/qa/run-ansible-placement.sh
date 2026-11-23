#!/usr/bin/env bash
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ansible_dir="${AARNN_ANSIBLE_DIR:-/home/pbisaacs/Developer/swarmhpc/swarmhpc/ansible}"
inventory="${AARNN_ANSIBLE_INVENTORY:-${ansible_dir}/inventory/hosts.ini}"
grants="${AARNN_COMPUTE_GRANTS:-qc00,qc02,qc03,qc04,sm00,sm01}"

exec python3 "${repo_root}/scripts/qa/ansible_placement_smoke.py" \
  --ansible-dir "${ansible_dir}" \
  --inventory "${inventory}" \
  --grant-compute "${grants}" \
  "$@"
