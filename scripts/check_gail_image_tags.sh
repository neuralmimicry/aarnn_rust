#!/usr/bin/env bash

set -euo pipefail

repo_root="${1:-$(pwd)}"

# Reject pinned Gail image tags to keep AARNN decoupled from Gail release cadence.
# Use git grep so generated, ignored build output cannot trigger a false
# positive. This also keeps the check independent of optional ripgrep.
pattern='(ghcr[.]io/)?neuralmimicry/gail:[[:alnum:]_.-]+'
if ! command -v git >/dev/null 2>&1; then
  echo "::error::git is required for the Gail image tag policy check."
  exit 2
fi

set +e
candidates="$(git -C "${repo_root}" grep -nEo "${pattern}" -- . ':!third_party/**')"
grep_status=$?
set -e

if (( grep_status > 1 )); then
  echo "::error::Gail image tag policy scan failed (git grep exit ${grep_status})."
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
