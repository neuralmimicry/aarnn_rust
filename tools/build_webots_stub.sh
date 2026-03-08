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

LIB_SO="$OUT_DIR/libaarnn_rust.so"
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
ENABLE_OPENMP="${ENABLE_OPENMP:-auto}"
openmp_cxxflags=()
openmp_ldflags=()
if [[ "$ENABLE_OPENMP" != "0" && "$ENABLE_OPENMP" != "false" ]]; then
  probe_bin="/tmp/nm_openmp_probe.$$"
  if printf 'int main(){return 0;}\n' | "$CXX_CMD" -std=c++17 -x c++ -fopenmp - -o "$probe_bin" >/dev/null 2>&1; then
    openmp_cxxflags=(-fopenmp)
    openmp_ldflags=(-fopenmp)
    echo "==> OpenMP enabled for C++ stub build"
    rm -f "$probe_bin"
  elif [[ "$ENABLE_OPENMP" == "1" || "$ENABLE_OPENMP" == "true" ]]; then
    echo "Error: OpenMP requested but compiler/linker does not support -fopenmp" >&2
    exit 4
  else
    echo "==> OpenMP unavailable; building without OpenMP"
  fi
fi

set -x
"$CXX_CMD" -std=c++17 -O2 "${openmp_cxxflags[@]}" -Iinclude examples/webots_controller.cpp \
  -L "$OUT_DIR" -laarnn_rust \
  "${openmp_ldflags[@]}" \
  -Wl,-rpath,'$ORIGIN' \
  -o "$OUT_DIR/webots_controller"
set +x

echo "==> Done. Binary: $OUT_DIR/webots_controller"
echo "Run: $OUT_DIR/webots_controller"
