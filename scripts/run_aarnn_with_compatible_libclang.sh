#!/usr/bin/env bash
# shellcheck disable=SC2317
#
# Run aarnn_rust with a libclang version older than 22.
#
# Why this exists:
# - Some systems may have multiple libclang versions installed.
# - Newer libclang releases (22+) can generate incompatible bindings for this build.
# - We therefore auto-detect the newest available libclang with major version < 22,
#   then configure the environment so Cargo/bindgen uses that exact library.
#
# Behavior:
# - Searches common library locations plus anything already in LD_LIBRARY_PATH.
# - Prefers the highest semantic version below 22.
# - Exports PATH, LIBCLANG_PATH, LD_LIBRARY_PATH, LLVM_CONFIG_PATH, CLANG_PATH,
#   MPICC, and MPICXX consistently.
# - Verifies the selected clang major version is < 22 before running.
#
# Usage:
#   ./run_aarnn_with_compatible_libclang.sh
#
# Optional overrides:
#   CARGO_BIN=aarnn_rust
#   BRAIN_ID=motor
#   EXTRA_CARGO_ARGS="--features mpi"
#   EXTRA_APP_ARGS="--some-flag value"
#   SEARCH_DIRS="/opt/llvm/lib /usr/local/lib /usr/lib"
#
# Notes:
# - This script assumes clang/llvm-config are installed alongside the chosen libclang.
# - It does not hardcode a specific LLVM patch version.

set -Eeuo pipefail

###############################################################################
# Logging helpers
###############################################################################

log() {
  printf '[INFO] %s\n' "$*" >&2
}

warn() {
  printf '[WARN] %s\n' "$*" >&2
}

die() {
  printf '[ERROR] %s\n' "$*" >&2
  exit 1
}

###############################################################################
# Configurable inputs
###############################################################################

CARGO_BIN="${CARGO_BIN:-aarnn_rust}"
BRAIN_ID="${BRAIN_ID:-motor}"
EXTRA_CARGO_ARGS="${EXTRA_CARGO_ARGS:-}"
EXTRA_APP_ARGS="${EXTRA_APP_ARGS:-}"

###############################################################################
# Utility helpers
###############################################################################

# Return 0 if command exists.
have_cmd() {
  command -v "$1" >/dev/null 2>&1
}

# Print unique lines while preserving order.
unique_lines() {
  awk '!seen[$0]++'
}

# Extract the numeric version suffix from filenames such as:
#   libclang.so.21.1.8
#   libclang-cpp.so.21
# If no suffix is found, print nothing.
extract_version() {
  local path="$1"
  local base
  base="$(basename "$path")"

  if [[ "$base" =~ \.so\.([0-9]+([.][0-9]+)*)$ ]]; then
    printf '%s\n' "${BASH_REMATCH[1]}"
    return 0
  fi

  return 1
}

# Get the major version component from a dotted version string.
version_major() {
  local version="$1"
  printf '%s\n' "${version%%.*}"
}

###############################################################################
# Candidate discovery
###############################################################################

# Build the directory search list.
build_search_dirs() {
  {
    # User-provided search roots first, if any.
    if [[ -n "${SEARCH_DIRS:-}" ]]; then
      tr ' ' '\n' <<<"${SEARCH_DIRS}"
    fi

    # Common installation locations.
    printf '%s\n' \
      /usr/local/lib \
      /usr/local/lib64 \
      /usr/lib \
      /usr/lib64 \
      /lib \
      /lib64 \
      /usr/lib/x86_64-linux-gnu \
      /opt/homebrew/lib \
      /opt/local/lib

    # Common LLVM-specific roots.
    compgen -G '/usr/lib/llvm-*/lib' || true
    compgen -G '/usr/local/llvm*/lib' || true
    compgen -G '/opt/llvm*/lib' || true

    # Already-configured loader paths.
    if [[ -n "${LD_LIBRARY_PATH:-}" ]]; then
      tr ':' '\n' <<<"${LD_LIBRARY_PATH}"
    fi
  } \
    | sed '/^$/d' \
    | unique_lines
}

# Find candidate libclang shared libraries.
find_libclang_candidates() {
  local dir
  while IFS= read -r dir; do
    [[ -d "$dir" ]] || continue

    # We only want versioned libclang shared objects.
    # Unversioned symlinks like libclang.so are ignored because they may point
    # at an unsuitable major version.
    find "$dir" -maxdepth 1 -type f \
      \( -name 'libclang.so.*' -o -name 'libclang-*.so.*' \) \
      2>/dev/null || true

    # Also include symlinks if the system ships versioned symlink chains.
    find "$dir" -maxdepth 1 -type l \
      \( -name 'libclang.so.*' -o -name 'libclang-*.so.*' \) \
      2>/dev/null || true
  done < <(build_search_dirs)
}

# Pick the newest libclang whose major version is less than 22.
pick_best_libclang() {
  local candidate version major

  while IFS= read -r candidate; do
    version="$(extract_version "$candidate" || true)"
    [[ -n "$version" ]] || continue

    major="$(version_major "$version")"
    [[ "$major" =~ ^[0-9]+$ ]] || continue

    if (( major < 22 )); then
      # Output as: version<TAB>path
      printf '%s\t%s\n' "$version" "$candidate"
    fi
  done < <(find_libclang_candidates) \
    | sort -t $'\t' -k1,1V \
    | tail -n 1 \
    | cut -f2-
}

###############################################################################
# LLVM tool alignment
###############################################################################

# Given a lib directory, try to locate a matching clang binary.
find_matching_clang() {
  local lib_dir="$1"
  local candidate

  # Try nearby bin dirs first.
  for candidate in \
    "${lib_dir%/lib}/bin/clang" \
    "${lib_dir%/lib64}/bin/clang" \
    /usr/local/bin/clang \
    /usr/bin/clang
  do
    if [[ -x "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done

  return 1
}

# Given a lib directory, try to locate a matching llvm-config binary.
find_matching_llvm_config() {
  local lib_dir="$1"
  local candidate

  for candidate in \
    "${lib_dir%/lib}/bin/llvm-config" \
    "${lib_dir%/lib64}/bin/llvm-config" \
    /usr/local/bin/llvm-config \
    /usr/bin/llvm-config
  do
    if [[ -x "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done

  return 1
}

# Extract the clang major version from "clang --version".
clang_major_version() {
  local clang_bin="$1"
  local version_line version major

  version_line="$("$clang_bin" --version 2>/dev/null | head -n 1 || true)"
  [[ -n "$version_line" ]] || return 1

  # Example:
  #   clang version 21.1.8 (...)
  if [[ "$version_line" =~ clang[[:space:]]+version[[:space:]]+([0-9]+([.][0-9]+)*) ]]; then
    version="${BASH_REMATCH[1]}"
    major="$(version_major "$version")"
    printf '%s\n' "$major"
    return 0
  fi

  return 1
}

###############################################################################
# Main selection logic
###############################################################################

main() {
  have_cmd cargo || die "cargo is required but was not found in PATH"
  have_cmd mpicc || warn "mpicc not found in PATH; MPI builds may fail"
  have_cmd mpicxx || warn "mpicxx not found in PATH; MPI C++ builds may fail"

  local libclang_path
  libclang_path="$(pick_best_libclang)"

  [[ -n "$libclang_path" ]] || die \
    "Could not find any versioned libclang shared library with major version < 22"

  local lib_dir
  lib_dir="$(dirname "$libclang_path")"

  local clang_bin llvm_config_bin
  clang_bin="$(find_matching_clang "$lib_dir" || true)"
  llvm_config_bin="$(find_matching_llvm_config "$lib_dir" || true)"

  [[ -n "$clang_bin" ]] || die "Could not locate a clang binary compatible with: $libclang_path"
  [[ -n "$llvm_config_bin" ]] || die "Could not locate llvm-config compatible with: $libclang_path"

  local clang_major
  clang_major="$(clang_major_version "$clang_bin" || true)"
  [[ -n "$clang_major" ]] || die "Unable to determine clang version from: $clang_bin"

  if (( clang_major >= 22 )); then
    die "Resolved clang is version $clang_major, but this script only allows versions < 22"
  fi

  # Export environment in a way that strongly encourages bindgen/clang-sys to
  # use the exact library and matching toolchain we selected.
  export PATH
  PATH="$(dirname "$clang_bin"):${PATH}"

  export LIBCLANG_PATH="$libclang_path"
  export LD_LIBRARY_PATH="${lib_dir}${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
  export LLVM_CONFIG_PATH="$llvm_config_bin"
  export CLANG_PATH="$clang_bin"

  # Use MPI wrapper compilers if present.
  if have_cmd mpicc; then
    export MPICC
    MPICC="$(command -v mpicc)"
  fi

  if have_cmd mpicxx; then
    export MPICXX
    MPICXX="$(command -v mpicxx)"
  fi

  log "Selected libclang:    $LIBCLANG_PATH"
  log "Selected clang:       $CLANG_PATH"
  log "Selected llvm-config: $LLVM_CONFIG_PATH"
  log "Selected lib dir:     $lib_dir"
  log "Detected clang major: $clang_major"

  # Clean first so stale generated bindings do not survive between runs.
  cargo clean

  # Build the cargo command as an array to preserve quoting correctly.
  local -a cargo_cmd=(
    cargo run
    --bin "$CARGO_BIN"
    --release
    --all-features
    --
    --brain-id "$BRAIN_ID"
    --ui
    --quiet
  )

  # Append optional extra Cargo-side args before the `--` separator if provided.
  if [[ -n "$EXTRA_CARGO_ARGS" ]]; then
    # shellcheck disable=SC2206
    local -a extra_cargo=( $EXTRA_CARGO_ARGS )
    cargo_cmd=(
      cargo run
      "${extra_cargo[@]}"
      --bin "$CARGO_BIN"
      --release
      --all-features
      --
      --brain-id "$BRAIN_ID"
      --ui
      --quiet
    )
  fi

  # Append optional app-side args if provided.
  if [[ -n "$EXTRA_APP_ARGS" ]]; then
    # shellcheck disable=SC2206
    local -a extra_app=( $EXTRA_APP_ARGS )
    cargo_cmd+=( "${extra_app[@]}" )
  fi

  log "Running Cargo with compatible LLVM/libclang configuration"
  "${cargo_cmd[@]}"
}

main "$@"
