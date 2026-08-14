#!/usr/bin/env bash
set -euo pipefail

# Install the host kernel package only when this host is Ubuntu 24.04 with
# 64K pages and NVIDIA support is requested/detected.  Kernel packages belong
# on the host, never in the AARNN image itself.
aarnn_ensure_64k_hwe_nvidia() {
  local requested="${1:-${AARNN_ENABLE_GPU:-false}}"
  local page_size
  page_size="$(getconf PAGESIZE 2>/dev/null || printf '0')"
  if [[ "${page_size}" != "65536" ]] || [[ ! -r /etc/os-release ]]; then
    return 0
  fi
  # shellcheck disable=SC1091
  . /etc/os-release
  [[ "${ID:-}" == "ubuntu" && "${VERSION_ID:-}" == "24.04" ]] || return 0
  if [[ "${requested,,}" != "true" && "${requested,,}" != "1" ]] &&
     ! command -v nvidia-smi >/dev/null 2>&1 && [[ ! -e /dev/nvidiactl ]]; then
    return 0
  fi
  if dpkg-query -W -f='${Status}' linux-nvidia-64k-hwe-24.04 2>/dev/null | grep -q 'install ok installed'; then
    return 0
  fi

  echo "Installing linux-nvidia-64k-hwe-24.04 for the 64K-page NVIDIA host"
  if [[ "${EUID}" -eq 0 ]]; then
    apt-get update
    apt-get install -y linux-nvidia-64k-hwe-24.04
  else
    sudo apt-get update
    sudo apt-get install -y linux-nvidia-64k-hwe-24.04
  fi
}

