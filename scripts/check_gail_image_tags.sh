#!/usr/bin/env bash

set -euo pipefail

repo_root="${1:-$(pwd)}"

# Reject pinned Gail image tags to keep AARNN decoupled from Gail release cadence.
# Allowed:
#   ghcr.io/neuralmimicry/gail:latest
#   neuralmimicry/gail:latest
pattern='\b(?:ghcr\.io/)?neuralmimicry/gail:(?!latest\b)[A-Za-z0-9._-]+'

matches="$(rg -n --hidden --glob '!.git' --glob '!third_party/**' -P "${pattern}" "${repo_root}" || true)"

if [[ -n "${matches}" ]]; then
  echo "::error::Pinned Gail image tags detected. Use :latest for Gail image references."
  echo "${matches}"
  exit 1
fi

echo "Gail image tag policy check passed: no pinned Gail tags found."
