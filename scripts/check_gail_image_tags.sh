#!/usr/bin/env bash

set -euo pipefail

repo_root="${1:-$(pwd)}"

# Reject pinned Gail image tags to keep AARNN decoupled from Gail release cadence.
# Use grep from the base runner image so this check does not silently pass when
# optional ripgrep is missing. --text lets grep inspect manifests with CRLF or
# embedded binary-looking metadata; generated third-party and Git data is out.
pattern='(ghcr[.]io/)?neuralmimicry/gail:[[:alnum:]_.-]+'
if ! command -v grep >/dev/null 2>&1; then
  echo "::error::grep is required for the Gail image tag policy check."
  exit 2
fi

set +e
candidates="$(grep -RInEo --text \
  --exclude-dir=.git \
  --exclude-dir=third_party \
  "${pattern}" "${repo_root}")"
grep_status=$?
set -e

if (( grep_status > 1 )); then
  echo "::error::Gail image tag policy scan failed (grep exit ${grep_status})."
  exit "${grep_status}"
fi

matches=""
while IFS= read -r candidate; do
  [[ -n "${candidate}" ]] || continue
  image_ref="${candidate##*:}"
  if [[ "${image_ref}" != "latest" ]]; then
    matches+="${candidate}"$'\n'
  fi
done <<< "${candidates}"

if [[ -n "${matches}" ]]; then
  echo "::error::Pinned Gail image tags detected. Use :latest for Gail image references."
  printf '%s' "${matches}"
  exit 1
fi

echo "Gail image tag policy check passed: no pinned Gail tags found."
