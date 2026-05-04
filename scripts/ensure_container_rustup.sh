#!/usr/bin/env bash
set -euo pipefail

TARGET_TRIPLE="${1:-}"

clear_dir_contents() {
  local dir="$1"
  mkdir -p "$dir"
  find "$dir" -mindepth 1 -maxdepth 1 -exec rm -rf {} +
}

reset_rustup_state() {
  rm -rf \
    /cargo/bin \
    /cargo/env \
    /cargo/.crates.toml \
    /cargo/.crates2.json \
    /cargo/.package-cache
  clear_dir_contents /rustup
}

install_rustup() {
  reset_rustup_state
  curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal --default-toolchain stable
}

if [[ ! -x /cargo/bin/rustup ]]; then
  install_rustup
fi

# shellcheck disable=SC1091
. /cargo/env

if ! rustup --version >/dev/null 2>&1; then
  echo "Existing rustup cache is not runnable; reinstalling toolchain cache." >&2
  install_rustup
  # shellcheck disable=SC1091
  . /cargo/env
fi

if [[ -n "$TARGET_TRIPLE" ]]; then
  rustup target add "$TARGET_TRIPLE"
fi
