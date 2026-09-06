#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
suite=mobile-contract
while (($#)); do
  case "$1" in
    --suite) suite="${2:?missing suite}"; shift 2 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
cd "$repo_root"
if [[ ! -d apps/ios ]]; then
  echo '{"product":"ios","status":"not-run","reason":"Xcode project absent"}'
  exit 0
fi
cargo xtask qa run --suite "$suite" --product ios
