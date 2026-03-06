#!/usr/bin/env bash
# Diagnostic helper for OpenCV (opencv4) and libclang detection
# Usage:
#   bash tools/check_opencv_clang.sh            # diagnostics + suggested exports
#   source tools/check_opencv_clang.sh --apply  # diagnostics + export discovered vars into current shell

set -uo pipefail

APPLY=0
for arg in "${@:-}"; do
  case "$arg" in
    --apply) APPLY=1 ;;
    --help|-h)
      cat <<EOF
Usage:
  bash   tools/check_opencv_clang.sh         # diagnostics + suggested exports
  source tools/check_opencv_clang.sh --apply # diagnostics + export into current shell

Notes:
  - Exporting variables to your current shell only works when the script is sourced.
  - When executed (not sourced), the script will only print the suggested export lines.
EOF
      exit 0
      ;;
  esac
done

# Determine if we're sourced (so we can export to the caller environment)
IS_SOURCED=0
if [ "${BASH_SOURCE[0]}" != "$0" ]; then IS_SOURCED=1; fi

echo "== Neuromorphic Demo — OpenCV/libclang diagnostics =="
date
echo "uname: $(uname -a || true)"
echo

echo "-- Rust toolchain --"
echo "rustc: $(rustc --version 2>/dev/null || echo 'not found')"
echo "cargo: $(cargo --version 2>/dev/null || echo 'not found')"
echo

echo "-- Key environment variables --"
echo "LIBCLANG_PATH=${LIBCLANG_PATH-}"
echo "LD_LIBRARY_PATH=${LD_LIBRARY_PATH-}"
echo "LLVM_CONFIG_PATH=${LLVM_CONFIG_PATH-}"
echo "PKG_CONFIG_PATH=${PKG_CONFIG_PATH-}"
echo "OpenCV_DIR=${OpenCV_DIR-}"
echo

echo "-- pkg-config (OpenCV) --"
if command -v pkg-config >/dev/null 2>&1; then
  echo "pkg-config: $(pkg-config --version)"
  if pkg-config --exists opencv4; then
    echo "opencv4 version: $(pkg-config --modversion opencv4)"
    echo "opencv4 libs:    $(pkg-config --libs opencv4)"
    echo "opencv4 cflags:  $(pkg-config --cflags opencv4)"
  else
    echo "opencv4: NOT FOUND by pkg-config"
  fi
else
  echo "pkg-config: not found"
fi
echo

echo "-- clang / llvm --"
if command -v clang >/dev/null 2>&1; then
  echo "clang: $(clang --version | head -n1)"
else
  echo "clang: not found"
fi
if command -v llvm-config >/dev/null 2>&1; then
  echo "llvm-config: $(llvm-config --version) at $(command -v llvm-config)"
  echo "llvm prefix:  $(llvm-config --prefix)"
else
  echo "llvm-config: not found"
fi
echo

echo "-- libclang discovery --"
found_any=0
FOUND_LIBCLANG_DIR=""

check_dir() {
  local d="$1"
  if [ -n "$d" ] && [ -d "$d" ]; then
    echo "Searching: $d"
    ls -1 "$d" 2>/dev/null | grep -E '^libclang(\.so(\.[0-9]+)*)?$' || true
    if ls -1 "$d" 2>/dev/null | grep -q -E '^libclang(\.so(\.[0-9]+)*)?$'; then
      found_any=1
      FOUND_LIBCLANG_DIR="$d"
    fi
  fi
}

echo "LIBCLANG_PATH: ${LIBCLANG_PATH-}"
check_dir "${LIBCLANG_PATH-}"

# Common locations
for d in \
  /usr/lib \
  /usr/local/lib \
  /usr/lib64 \
  /usr/local/lib64 \
  /opt/homebrew/opt/llvm/lib \
  /opt/llvm/lib \
  /Library/Developer/CommandLineTools/usr/lib \
  /usr/lib/llvm-18/lib /usr/lib/llvm-17/lib /usr/lib/llvm-16/lib /usr/lib/llvm-15/lib
do
  check_dir "$d"
done

echo
echo "ldconfig -p | grep libclang (if available)"
if command -v ldconfig >/dev/null 2>&1; then
  # Capture first path from ldconfig if present
  LDCONFIG_HIT=$(ldconfig -p 2>/dev/null | grep -m1 -E 'libclang(\.so(\.[0-9]+)*)?' || true)
  echo "$LDCONFIG_HIT"
  if [ -z "$FOUND_LIBCLANG_DIR" ] && [ -n "$LDCONFIG_HIT" ]; then
    FOUND_LIBCLANG_DIR=$(echo "$LDCONFIG_HIT" | sed -E 's/.* => (.*)\/(libclang.*)$/\1/' )
    if [ -n "$FOUND_LIBCLANG_DIR" ] && [ -d "$FOUND_LIBCLANG_DIR" ]; then found_any=1; fi
  fi
else
  echo "ldconfig not available"
fi

# Try llvm-config --libdir as a strong hint if not found yet
if [ "$found_any" -ne 1 ] && command -v llvm-config >/dev/null 2>&1; then
  LLVMDIR=$(llvm-config --libdir 2>/dev/null || true)
  if [ -n "$LLVMDIR" ]; then
    check_dir "$LLVMDIR"
    if [ "$found_any" -eq 1 ]; then FOUND_LIBCLANG_DIR="$LLVMDIR"; fi
  fi
fi

# OpenCV hints via pkg-config
OPENCV_PC_DIR=""
OPENCV_LIB_DIR=""
OPENCV_CMAKE_DIR=""
if command -v pkg-config >/dev/null 2>&1 && pkg-config --exists opencv4; then
  OPENCV_PC_DIR=$(pkg-config --variable=pcfiledir opencv4 2>/dev/null || true)
  OPENCV_LIB_DIR=$(pkg-config --variable=libdir opencv4 2>/dev/null || true)
  if [ -n "$OPENCV_LIB_DIR" ] && [ -d "$OPENCV_LIB_DIR/cmake/opencv4" ]; then
    OPENCV_CMAKE_DIR="$OPENCV_LIB_DIR/cmake/opencv4"
  fi
fi

echo
if [ "$found_any" -eq 1 ]; then
  echo "Result: libclang appears to be present on this system."
else
  echo "Result: libclang not found in common locations."
  echo "Hint: install LLVM/Clang dev packages and set LIBCLANG_PATH."
fi

echo
echo "-- Summary --"
echo "OpenCV via pkg-config: $(pkg-config --exists opencv4 && echo 'OK' || echo 'MISSING')"
echo "libclang present (heuristic): $([ "$found_any" -eq 1 ] && echo 'YES' || echo 'NO')"
echo
echo "If OpenCV is MISSING: install OpenCV 4 and ensure pkg-config reports opencv4."
echo "If libclang is NO: install llvm/clang dev libs and set LIBCLANG_PATH to the directory containing libclang.so."

# --- Suggested or applied exports ---
echo
echo "-- Environment exports --"
SUG_LIBCLANG_PATH=""
SUG_LD_LIBRARY_PATH=""
SUG_LLVM_CONFIG_PATH=""
SUG_PKG_CONFIG_PATH=""
SUG_OPENCV_DIR=""

if [ -n "$FOUND_LIBCLANG_DIR" ]; then
  SUG_LIBCLANG_PATH="$FOUND_LIBCLANG_DIR"
fi

if [ -n "$SUG_LIBCLANG_PATH" ]; then
  # Prepend only if not already present
  case ":${LD_LIBRARY_PATH-}:" in
    *":$SUG_LIBCLANG_PATH:"*) SUG_LD_LIBRARY_PATH="${LD_LIBRARY_PATH-}" ;;
    *) SUG_LD_LIBRARY_PATH="$SUG_LIBCLANG_PATH${LD_LIBRARY_PATH+:$LD_LIBRARY_PATH}" ;;
  esac
fi

if command -v llvm-config >/dev/null 2>&1; then
  SUG_LLVM_CONFIG_PATH=$(command -v llvm-config)
fi

if [ -n "$OPENCV_PC_DIR" ]; then
  # Prepend pc dir if missing
  case ":${PKG_CONFIG_PATH-}:" in
    *":$OPENCV_PC_DIR:"*) SUG_PKG_CONFIG_PATH="${PKG_CONFIG_PATH-}" ;;
    *) SUG_PKG_CONFIG_PATH="$OPENCV_PC_DIR${PKG_CONFIG_PATH+:$PKG_CONFIG_PATH}" ;;
  esac
fi

if [ -n "$OPENCV_CMAKE_DIR" ]; then
  SUG_OPENCV_DIR="$OPENCV_CMAKE_DIR"
fi

print_exports() {
  [ -n "$SUG_LIBCLANG_PATH" ] && echo "export LIBCLANG_PATH=$SUG_LIBCLANG_PATH"
  [ -n "$SUG_LD_LIBRARY_PATH" ] && echo "export LD_LIBRARY_PATH=$SUG_LD_LIBRARY_PATH"
  [ -n "$SUG_LLVM_CONFIG_PATH" ] && echo "export LLVM_CONFIG_PATH=$SUG_LLVM_CONFIG_PATH"
  [ -n "$SUG_PKG_CONFIG_PATH" ] && echo "export PKG_CONFIG_PATH=$SUG_PKG_CONFIG_PATH"
  [ -n "$SUG_OPENCV_DIR" ] && echo "export OpenCV_DIR=$SUG_OPENCV_DIR"
}

apply_exports() {
  [ -n "$SUG_LIBCLANG_PATH" ] && export LIBCLANG_PATH="$SUG_LIBCLANG_PATH"
  [ -n "$SUG_LD_LIBRARY_PATH" ] && export LD_LIBRARY_PATH="$SUG_LD_LIBRARY_PATH"
  [ -n "$SUG_LLVM_CONFIG_PATH" ] && export LLVM_CONFIG_PATH="$SUG_LLVM_CONFIG_PATH"
  [ -n "$SUG_PKG_CONFIG_PATH" ] && export PKG_CONFIG_PATH="$SUG_PKG_CONFIG_PATH"
  [ -n "$SUG_OPENCV_DIR" ] && export OpenCV_DIR="$SUG_OPENCV_DIR"
}

if [ "$APPLY" -eq 1 ]; then
  if [ "$IS_SOURCED" -eq 1 ]; then
    apply_exports
    echo "Applied exports into current shell:"
    print_exports
  else
    echo "Note: --apply requested but script is not sourced."
    echo "Run: source tools/check_opencv_clang.sh --apply"
    echo "Suggested export lines:"
    print_exports
  fi
else
  echo "Suggested export lines (copy/paste or run: source tools/check_opencv_clang.sh --apply):"
  print_exports
fi
