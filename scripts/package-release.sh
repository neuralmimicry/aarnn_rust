#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: package-release.sh [options]

Build and package AARNN Rust release artifacts.

Options:
  --version VERSION              Version label for the packaged artifacts.
  --output-dir DIR               Directory to receive the packaged artifacts.
  --target-triple TRIPLE         Optional cargo target triple.
  --platform NAME                Platform suffix in output names. Default: derived from host.
  --artifact-suffix NAME         Optional extra suffix appended to artifact filenames.
  --deb-arch ARCH                Also build a Debian package for linux using ARCH (amd64 or arm64).
  --cargo-features FEATURES      Cargo feature selection. Default: cargo defaults.
  --cargo-build-targets TARGETS  Space-delimited cargo binary targets. Default: "aarnn_rust web_ui".
  --skip-build                   Reuse existing release binaries instead of building them.
  -h, --help                     Show this help text.

Examples:
  ./scripts/package-release.sh --version 0.1.0 --output-dir ./dist
  ./scripts/package-release.sh --version 0.1.0 --output-dir ./dist --platform linux-x86_64 --deb-arch amd64
  ./scripts/package-release.sh --version 0.1.0 --output-dir ./dist --artifact-suffix ubuntu24.04
  ./scripts/package-release.sh --version 0.1.0 --output-dir ./dist --cargo-features standalone_workload --cargo-build-targets "aarnn_rust"
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
  local target_root
  target_root="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
  if [[ -n "$TARGET_TRIPLE" ]]; then
    printf '%s/%s/release\n' "$target_root" "$TARGET_TRIPLE"
  else
    printf '%s/release\n' "$target_root"
  fi
}

parse_build_targets() {
  local bin=""
  BUILD_TARGET_LIST=()
  for bin in $CARGO_BUILD_TARGETS; do
    [[ -n "$bin" ]] || continue
    BUILD_TARGET_LIST+=("$bin")
  done
  if ((${#BUILD_TARGET_LIST[@]} == 0)); then
    die "--cargo-build-targets resolved to an empty target list"
  fi
}

resolve_binary_paths() {
  local bin=""
  BIN_PATHS=()
  for bin in "${BUILD_TARGET_LIST[@]}"; do
    BIN_PATHS+=("${BIN_DIR}/${bin}")
  done
}

sanitize_artifact_suffix() {
  local raw="$1" sanitized
  [[ -n "$raw" ]] || {
    printf '\n'
    return
  }

  sanitized=$(
    printf '%s' "$raw" \
      | sed -E 's/[^A-Za-z0-9._+-]+/-/g; s/^-+//; s/-+$//'
  )
  [[ -n "$sanitized" ]] || die "unable to derive artifact suffix from '$raw'"
  printf '%s\n' "$sanitized"
}

validate_deb_arch() {
  case "$1" in
    amd64|arm64)
      ;;
    *)
      die "unsupported Debian architecture: $1"
      ;;
  esac
}

debian_package_version() {
  local version sanitized
  version="$1"
  [[ -n "$version" ]] || die "Debian package version is empty"
  sanitized=$(
    printf '%s' "$version" \
      | tr '-' '~' \
      | sed -E 's/[^A-Za-z0-9.+:~]+/./g; s/^[^A-Za-z0-9]+//; s/[^A-Za-z0-9]+$//'
  )
  [[ -n "$sanitized" ]] || die "unable to derive Debian package version from '$version'"
  printf '%s\n' "$sanitized"
}

compute_deb_depends() {
  if ! command -v dpkg-shlibdeps >/dev/null 2>&1; then
    printf '\n'
    return
  fi

  local work_dir stage_root output depends binary staged_path
  local -a analyze_args=()
  work_dir=$(mktemp -d)

  stage_root="$work_dir/root/usr/bin"
  install -d -m 0755 "$work_dir/debian" "$stage_root"
  cat >"$work_dir/debian/control" <<'CONTROL'
Source: aarnn-rust
Section: admin
Priority: optional
Maintainer: NeuralMimicry <opensource@neuralmimicry.ai>
Standards-Version: 4.7.0
Package: aarnn-rust
Architecture: any
Description: temporary shlibdeps metadata carrier
CONTROL

  for binary in "$@"; do
    [[ -n "$binary" && -f "$binary" ]] || continue
    staged_path="$stage_root/$(basename "$binary")"
    install -m 0755 "$binary" "$staged_path"
    analyze_args+=("-e" "./${staged_path#$work_dir/}")
  done

  if ((${#analyze_args[@]} == 0)); then
    rm -rf "$work_dir"
    printf '\n'
    return
  fi

  output=$(
    cd "$work_dir"
    dpkg-shlibdeps --ignore-missing-info -O -Tdebian/substvars \
      "${analyze_args[@]}" 2>/dev/null || true
  )
  rm -rf "$work_dir"

  depends=$(printf '%s\n' "$output" | sed -n 's/^shlibs:Depends=//p' | tail -n 1)
  printf '%s\n' "$depends" | sed -E 's/^[[:space:]]+//; s/[[:space:]]+$//; s/, ,/, /g; s/^, //; s/, $//'
}

create_debian_package() {
  local deb_version deb_stage_root deb_root deb_path depends docs_root share_root
  local idx=""
  local -a staged_bin_paths=()

  [[ "$PLATFORM" == linux* ]] || die "--deb-arch is only supported for linux platforms"
  validate_deb_arch "$DEB_ARCH"
  command -v dpkg-deb >/dev/null 2>&1 || die "dpkg-deb is required when --deb-arch is used"

  deb_version=$(debian_package_version "$VERSION")
  deb_stage_root="$OUTPUT_DIR/.deb-stage"
  deb_root="$deb_stage_root/root"
  deb_path="$OUTPUT_DIR/aarnn-rust_${deb_version}${ARTIFACT_SUFFIX_FILENAME_PART}_${DEB_ARCH}.deb"
  docs_root="$deb_root/usr/share/doc/aarnn-rust"
  share_root="$deb_root/usr/share/aarnn-rust"

  rm -rf "$deb_stage_root"
  install -d -m 0755 \
    "$deb_root/DEBIAN" \
    "$deb_root/usr/bin" \
    "$docs_root" \
    "$share_root"

  for idx in "${!BUILD_TARGET_LIST[@]}"; do
    install -m 0755 "${BIN_PATHS[$idx]}" "$deb_root/usr/bin/${BUILD_TARGET_LIST[$idx]}"
    staged_bin_paths+=("$deb_root/usr/bin/${BUILD_TARGET_LIST[$idx]}")
  done

  if [[ -f "$REPO_ROOT/config.json" ]]; then
    install -m 0644 "$REPO_ROOT/config.json" "$share_root/config.json"
  fi
  if [[ -f "$REPO_ROOT/README.md" ]]; then
    install -m 0644 "$REPO_ROOT/README.md" "$docs_root/README.md"
  fi
  if [[ -f "$REPO_ROOT/docs/operations.md" ]]; then
    install -m 0644 "$REPO_ROOT/docs/operations.md" "$docs_root/operations.md"
  fi
  if [[ -f "$REPO_ROOT/docs/architecture.md" ]]; then
    install -m 0644 "$REPO_ROOT/docs/architecture.md" "$docs_root/architecture.md"
  fi

  depends=$(compute_deb_depends "${staged_bin_paths[@]}")

  {
    printf 'Package: aarnn-rust\n'
    printf 'Version: %s\n' "$deb_version"
    printf 'Section: admin\n'
    printf 'Priority: optional\n'
    printf 'Architecture: %s\n' "$DEB_ARCH"
    if [[ -n "$depends" ]]; then
      printf 'Depends: %s\n' "$depends"
    fi
    printf 'Maintainer: NeuralMimicry <opensource@neuralmimicry.ai>\n'
    printf 'Homepage: https://github.com/neuralmimicry/aarnn_rust\n'
    printf 'Description: AARNN runtime binaries and web UI assets\n'
    printf ' The aarnn-rust package installs the selected AARNN runtime binaries,\n'
    printf ' bundled configuration, and operations documentation.\n'
  } >"$deb_root/DEBIAN/control"

  dpkg-deb --build --root-owner-group "$deb_root" "$deb_path" >/dev/null
  artifacts+=("$deb_path")
  rm -rf "$deb_stage_root"
}

build_binaries() {
  local -a args=(cargo build --locked --release)
  local bin=""

  if [[ -n "$TARGET_TRIPLE" ]]; then
    args+=(--target "$TARGET_TRIPLE")
  fi

  case "$CARGO_FEATURES" in
    ""|default)
      ;;
    all|all-features)
      args+=(--all-features)
      ;;
    *)
      args+=(--no-default-features --features "$CARGO_FEATURES")
      ;;
  esac

  for bin in "${BUILD_TARGET_LIST[@]}"; do
    args+=(--bin "$bin")
  done

  (
    cd "$REPO_ROOT"
    "${args[@]}"
  )
}

VERSION=
OUTPUT_DIR=
TARGET_TRIPLE=
PLATFORM=
DEB_ARCH=
CARGO_FEATURES=
CARGO_BUILD_TARGETS="aarnn_rust web_ui"
ARTIFACT_SUFFIX=
SKIP_BUILD=0
BUILD_TARGET_LIST=()
BIN_PATHS=()

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
    --artifact-suffix)
      shift
      (($#)) || die "--artifact-suffix requires a value"
      ARTIFACT_SUFFIX="$1"
      ;;
    --deb-arch)
      shift
      (($#)) || die "--deb-arch requires a value"
      DEB_ARCH="$1"
      ;;
    --cargo-features)
      shift
      (($#)) || die "--cargo-features requires a value"
      CARGO_FEATURES="$1"
      ;;
    --cargo-build-targets)
      shift
      (($#)) || die "--cargo-build-targets requires a value"
      CARGO_BUILD_TARGETS="$1"
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
ARTIFACT_SUFFIX="$(sanitize_artifact_suffix "$ARTIFACT_SUFFIX")"
ARTIFACT_SUFFIX_FILENAME_PART=''
if [[ -n "$ARTIFACT_SUFFIX" ]]; then
  ARTIFACT_SUFFIX_FILENAME_PART="_${ARTIFACT_SUFFIX}"
fi

parse_build_targets
BIN_DIR=$(binary_dir)
resolve_binary_paths

if (( ! SKIP_BUILD )); then
  log "building release binaries"
  build_binaries
fi

resolve_binary_paths
for idx in "${!BUILD_TARGET_LIST[@]}"; do
  [[ -x "${BIN_PATHS[$idx]}" ]] || die "missing ${BUILD_TARGET_LIST[$idx]} binary: ${BIN_PATHS[$idx]}"
done

ARCHIVE_BASENAME="aarnn_rust-${VERSION}-${PLATFORM}"
if [[ -n "$ARTIFACT_SUFFIX" ]]; then
  ARCHIVE_BASENAME="${ARCHIVE_BASENAME}-${ARTIFACT_SUFFIX}"
fi
OUTPUT_DIR=$(mkdir -p "$OUTPUT_DIR" && cd "$OUTPUT_DIR" && pwd)
STAGE_ROOT="$OUTPUT_DIR/.stage"
PAYLOAD_DIR="$STAGE_ROOT/$ARCHIVE_BASENAME"
ARCHIVE_PATH="$OUTPUT_DIR/${ARCHIVE_BASENAME}.tar.gz"
CHECKSUM_PATH="$OUTPUT_DIR/${ARCHIVE_BASENAME}.sha256.txt"

rm -rf "$STAGE_ROOT"
mkdir -p "$PAYLOAD_DIR/docs"
for idx in "${!BUILD_TARGET_LIST[@]}"; do
  install -m 0755 "${BIN_PATHS[$idx]}" "$PAYLOAD_DIR/${BUILD_TARGET_LIST[$idx]}"
done
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
artifacts=("$ARCHIVE_PATH")

if [[ -n "$DEB_ARCH" ]]; then
  create_debian_package
fi

checksum_cmd=$(sha256_tool)
(
  cd "$OUTPUT_DIR"
  relative_artifacts=()
  for artifact in "${artifacts[@]}"; do
    relative_artifacts+=("$(basename "$artifact")")
  done
  $checksum_cmd "${relative_artifacts[@]}" >"$(basename "$CHECKSUM_PATH")"
)

log
log "packaged AARNN Rust release artifacts:"
for artifact in "${artifacts[@]}" "$CHECKSUM_PATH"; do
  log "  $artifact"
done

rm -rf "$STAGE_ROOT"
