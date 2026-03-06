#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ANSIBLE_DIR=$(cd "${SCRIPT_DIR}/.." && pwd)

if ! command -v ansible-playbook >/dev/null 2>&1; then
  echo "ansible-playbook not found. Install Ansible first." >&2
  exit 1
fi

ansible-galaxy collection install -r "${ANSIBLE_DIR}/requirements.yml" >/dev/null

ANSIBLE_CONFIG="${ANSIBLE_DIR}/ansible.cfg" \
ansible-playbook -i "${ANSIBLE_DIR}/inventory.ini" "${ANSIBLE_DIR}/playbooks/site.yml" \
  -e run_local_tests=false "$@"
