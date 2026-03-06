#!/usr/bin/env bash
set -euo pipefail

# Build the Rust cdylib and the C++ Webots-friendly stub.
# Usage:
#   tools/build_webots_stub.sh [release|debug]
# Defaults to release.

here="$(cd -- "$(dirname -- "$0")" && pwd)"
root="${here%/tools}"
cd "$root"

profile="${1:-release}"
case "$profile" in
  release|debug) ;;
  *) echo "Usage: $0 [release|debug]" >&2; exit 1;;
esac

echo "==> Building Rust cdylib (profile=$profile, features=ffi_bridge)"
if [[ "$profile" == "release" ]]; then
  cargo build --release --features ffi_bridge
  OUT_DIR="target/release"
else
  cargo build --features ffi_bridge
  OUT_DIR="target/debug"
fi

LIB_SO="$OUT_DIR/libneuromorphic_demo.so"
if [[ ! -f "$LIB_SO" ]]; then
  echo "Error: $LIB_SO not found. Did the Rust build succeed and crate-type include cdylib?" >&2
  exit 2
fi

# Choose a C++ compiler: prefer $CXX, fallback to g++, then clang++.
CXX_CMD="${CXX:-}"
if [[ -z "${CXX_CMD}" ]]; then
  if command -v g++ >/dev/null 2>&1; then
    CXX_CMD="g++"
  elif command -v clang++ >/dev/null 2>&1; then
    CXX_CMD="clang++"
  else
    echo "Error: neither g++ nor clang++ is available in PATH" >&2
    exit 3
  fi
fi

echo "==> Building C++ stub with $CXX_CMD"
set -x
"$CXX_CMD" -std=c++17 -O2 -Iinclude examples/webots_controller.cpp \
  -L "$OUT_DIR" -lneuromorphic_demo \
  -Wl,-rpath,'$ORIGIN' \
  -o "$OUT_DIR/webots_controller"
set +x

echo "==> Done. Binary: $OUT_DIR/webots_controller"
echo "Run: $OUT_DIR/webots_controller"
