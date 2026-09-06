#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"
product=host
all=false
while (($#)); do
  case "$1" in
    --all) all=true; shift ;;
    --product) product="${2:?missing product}"; shift 2 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
if [[ "$all" == true ]]; then
  while IFS=$'\t' read -r id _title; do
    cargo xtask examples run --id "$id" --product "$product"
  done < <(cargo xtask examples list)
else
  echo "use --all to run the catalogued examples" >&2
  exit 2
fi
