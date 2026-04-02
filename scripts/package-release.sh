#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: package-release.sh [options]

Build and package AARNN Rust release artifacts.

Options:
  --version VERSION           Version label for the packaged artifacts.
  --output-dir DIR            Directory to receive the packaged artifacts.
  --target-triple TRIPLE      Optional cargo target triple.
  --platform NAME             Platform suffix in output names. Default: derived from host.
  --skip-build                Reuse existing release binaries instead of building them.
  -h, --help                  Show this help text.

Examples:
  ./scripts/package-release.sh --version 0.1.0 --output-dir ./dist
USAGE
}

log() {
  printf '%s\n' "$*"
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

default_platform() {
  local os arch
  os=$(uname -s | tr '[:upper:]' '[:lower:]')
  arch=$(uname -m)
  case "$arch" in
    amd64) arch="x86_64" ;;
    arm64) arch="aarch64" ;;
  esac
  printf '%s-%s\n' "$os" "$arch"
}

sha256_tool() {
  if command -v sha256sum >/dev/null 2>&1; then
    printf 'sha256sum\n'
  elif command -v shasum >/dev/null 2>&1; then
    printf 'shasum -a 256\n'
  else
    die "sha256sum or shasum is required"
  fi
}

binary_dir() {
  if [[ -n "$TARGET_TRIPLE" ]]; then
    printf '%s/target/%s/release\n' "$REPO_ROOT" "$TARGET_TRIPLE"
  else
    printf '%s/target/release\n' "$REPO_ROOT"
  fi
}

build_binaries() {
  local args
  args=(cargo build --locked --release --bin aarnn_rust --bin web_ui)
  if [[ -n "$TARGET_TRIPLE" ]]; then
    args+=(--target "$TARGET_TRIPLE")
  fi
  (
    cd "$REPO_ROOT"
    "${args[@]}"
  )
}

VERSION=
OUTPUT_DIR=
TARGET_TRIPLE=
PLATFORM=
SKIP_BUILD=0

while (($#)); do
  case "$1" in
    --version)
      shift
      (($#)) || die "--version requires a value"
      VERSION="$1"
      ;;
    --output-dir)
      shift
      (($#)) || die "--output-dir requires a value"
      OUTPUT_DIR="$1"
      ;;
    --target-triple)
      shift
      (($#)) || die "--target-triple requires a value"
      TARGET_TRIPLE="$1"
      ;;
    --platform)
      shift
      (($#)) || die "--platform requires a value"
      PLATFORM="$1"
      ;;
    --skip-build)
      SKIP_BUILD=1
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown option: $1"
      ;;
  esac
  shift
done

[[ -n "$VERSION" ]] || die "--version is required"
[[ -n "$OUTPUT_DIR" ]] || die "--output-dir is required"

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(CDPATH='' cd -- "$SCRIPT_DIR/.." && pwd)
PLATFORM="${PLATFORM:-$(default_platform)}"

BIN_DIR=$(binary_dir)
AARNN_BIN="$BIN_DIR/aarnn_rust"
WEB_UI_BIN="$BIN_DIR/web_ui"

if (( ! SKIP_BUILD )) || [[ ! -x "$AARNN_BIN" || ! -x "$WEB_UI_BIN" ]]; then
  log "building release binaries"
  build_binaries
fi

[[ -x "$AARNN_BIN" ]] || die "missing aarnn_rust binary: $AARNN_BIN"
[[ -x "$WEB_UI_BIN" ]] || die "missing web_ui binary: $WEB_UI_BIN"

ARCHIVE_BASENAME="aarnn_rust-${VERSION}-${PLATFORM}"
OUTPUT_DIR=$(mkdir -p "$OUTPUT_DIR" && cd "$OUTPUT_DIR" && pwd)
STAGE_ROOT="$OUTPUT_DIR/.stage"
PAYLOAD_DIR="$STAGE_ROOT/$ARCHIVE_BASENAME"
ARCHIVE_PATH="$OUTPUT_DIR/${ARCHIVE_BASENAME}.tar.gz"
CHECKSUM_PATH="$OUTPUT_DIR/${ARCHIVE_BASENAME}.sha256.txt"

rm -rf "$STAGE_ROOT"
mkdir -p "$PAYLOAD_DIR/docs"
install -m 0755 "$AARNN_BIN" "$PAYLOAD_DIR/aarnn_rust"
install -m 0755 "$WEB_UI_BIN" "$PAYLOAD_DIR/web_ui"
if [[ -f "$REPO_ROOT/config.json" ]]; then
  install -m 0644 "$REPO_ROOT/config.json" "$PAYLOAD_DIR/config.json"
fi
if [[ -f "$REPO_ROOT/README.md" ]]; then
  install -m 0644 "$REPO_ROOT/README.md" "$PAYLOAD_DIR/README.md"
fi
if [[ -f "$REPO_ROOT/docs/operations.md" ]]; then
  install -m 0644 "$REPO_ROOT/docs/operations.md" "$PAYLOAD_DIR/docs/operations.md"
fi
if [[ -f "$REPO_ROOT/docs/architecture.md" ]]; then
  install -m 0644 "$REPO_ROOT/docs/architecture.md" "$PAYLOAD_DIR/docs/architecture.md"
fi

tar -C "$STAGE_ROOT" -czf "$ARCHIVE_PATH" "$ARCHIVE_BASENAME"
checksum_cmd=$(sha256_tool)
(
  cd "$OUTPUT_DIR"
  $checksum_cmd "$(basename "$ARCHIVE_PATH")" >"$(basename "$CHECKSUM_PATH")"
)

log
log "packaged AARNN Rust release artifacts:"
log "  $ARCHIVE_PATH"
log "  $CHECKSUM_PATH"

rm -rf "$STAGE_ROOT"
