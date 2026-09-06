#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"
if [[ " $* " == *" --include-examples "* ]]; then
  cargo xtask qa matrix --available --include-examples
else
  cargo xtask qa matrix --available
fi
