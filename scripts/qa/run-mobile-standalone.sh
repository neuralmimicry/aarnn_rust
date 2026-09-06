#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
product=all-available
while (($#)); do
  case "$1" in
    --product) product="${2:?missing product}"; shift 2 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
cd "$repo_root"
cargo xtask qa run --suite mobile-contract --product "$product"
