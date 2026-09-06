#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"
echo '{"status":"not-run","reason":"registered physical device and approved hardware lane are not available on this host"}'
