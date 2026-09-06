#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ansible_dir="${AARNN_ANSIBLE_DIR:-/home/pbisaacs/Developer/swarmhpc/swarmhpc/ansible}"

exec python3 "${repo_root}/scripts/qa/validate-ansible-stable-profile.py" \
  --ansible-dir "${ansible_dir}" "$@"
