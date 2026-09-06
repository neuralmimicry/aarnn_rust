#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
suite=mobile-contract
product=android
while (($#)); do
  case "$1" in
    --suite) suite="${2:?missing suite}"; shift 2 ;;
    --product) product="${2:?missing product}"; shift 2 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
cd "$repo_root"
cargo xtask qa run --suite "$suite" --product "$product"
