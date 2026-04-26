#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: prepare_container_package.sh [options]

Build or reuse the workload-specific Debian package consumed by Containerfile.

Options:
  --workload NAME             Container workload name. Default: standalone.
  --arch ARCH                 Target architecture (amd64 or arm64). Default: host arch.
  --output-dir DIR            Staging directory copied into the container build context.
                              Default: ./dist/container
  --cache-dir DIR             Cache directory for reusable workload packages.
                              Default: ./.container-cache/debs
  --cargo-features FEATURES   Override Cargo feature selection.
  --cargo-build-targets LIST  Override space-delimited cargo binary targets.
  --force                     Rebuild the package even when the cache fingerprint matches.
  -h, --help                  Show this help text.
USAGE
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

normalize_arch() {
  case "$1" in
    x86_64|amd64) printf '%s\n' 'amd64' ;;
    aarch64|arm64) printf '%s\n' 'arm64' ;;
    *) die "unsupported architecture: $1" ;;
  esac
}

host_arch() {
  normalize_arch "$(uname -m)"
}

rust_target_for_arch() {
  case "$1" in
    amd64) printf '%s\n' 'x86_64-unknown-linux-gnu' ;;
    arm64) printf '%s\n' 'aarch64-unknown-linux-gnu' ;;
    *) die "unsupported architecture: $1" ;;
  esac
}

platform_for_arch() {
  case "$1" in
    amd64) printf '%s\n' 'linux-x86_64' ;;
    arm64) printf '%s\n' 'linux-aarch64' ;;
    *) die "unsupported architecture: $1" ;;
  esac
}

deb_arch_for_arch() {
  case "$1" in
    amd64|arm64) printf '%s\n' "$1" ;;
    *) die "unsupported architecture: $1" ;;
  esac
}

cargo_version() {
  sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -n 1
}

resolve_abs_dir() {
  local dir="$1"
  mkdir -p "$dir"
  (
    cd "$dir"
    pwd
  )
}

collect_fingerprint_inputs() {
  local rel path
  for rel in \
    Cargo.toml \
    Cargo.lock \
    build.rs \
    config.json \
    docs/operations.md \
    docs/architecture.md \
    proto \
    src \
    web_ui \
    third_party/ibverbs-sys \
    third_party/mpi-sys \
    scripts/container_workloads.sh \
    scripts/prepare_container_package.sh \
    scripts/package-release.sh
  do
    path="$ROOT_DIR/$rel"
    if [[ -d "$path" ]]; then
      find "$path" -type f -print0
    elif [[ -f "$path" ]]; then
      printf '%s\0' "$path"
    fi
  done | sort -z
}

compute_fingerprint() {
  {
    printf 'workload=%s\n' "$WORKLOAD"
    printf 'arch=%s\n' "$ARCH"
    printf 'platform=%s\n' "$PLATFORM"
    printf 'rust_target=%s\n' "$RUST_TARGET"
    printf 'deb_arch=%s\n' "$DEB_ARCH"
    printf 'cargo_features=%s\n' "$CARGO_FEATURES"
    printf 'cargo_build_targets=%s\n' "$CARGO_BUILD_TARGETS"
    printf 'package_build_image=%s\n' "$PACKAGE_BUILD_IMAGE"
    while IFS= read -r -d '' file; do
      sha256sum "$file"
    done < <(collect_fingerprint_inputs)
  } | sha256sum | awk '{print $1}'
}

build_package_in_container() {
  command -v podman >/dev/null 2>&1 || die "podman is required to build container-targeted packages"

  local build_cache_root container_platform
  build_cache_root="$CACHE_DIR/_build-env/$ARCH"
  container_platform="linux/$ARCH"

  mkdir -p \
    "$PACKAGE_CACHE_DIR" \
    "$build_cache_root/cargo" \
    "$build_cache_root/rustup" \
    "$build_cache_root/target"

  podman run --rm --pull=missing \
    --platform "$container_platform" \
    -e DEBIAN_FRONTEND=noninteractive \
    -e CARGO_HOME=/cargo \
    -e RUSTUP_HOME=/rustup \
    -e CARGO_TARGET_DIR=/target \
    -v "$ROOT_DIR:/workspace:ro" \
    -v "$PACKAGE_CACHE_DIR:/out" \
    -v "$build_cache_root/cargo:/cargo" \
    -v "$build_cache_root/rustup:/rustup" \
    -v "$build_cache_root/target:/target" \
    -w /workspace \
    "$PACKAGE_BUILD_IMAGE" \
    bash -s -- "$RUST_TARGET" "$VERSION" "$PLATFORM" "$DEB_ARCH" "$CARGO_FEATURES" "$CARGO_BUILD_TARGETS" <<'SCRIPT'
set -euo pipefail

RUST_TARGET="$1"
VERSION="$2"
PLATFORM="$3"
DEB_ARCH="$4"
CARGO_FEATURES="$5"
CARGO_BUILD_TARGETS="$6"

apt-get update
apt-get install -y --no-install-recommends \
  ca-certificates \
  curl \
  xz-utils \
  build-essential \
  pkg-config \
  libssl-dev \
  protobuf-compiler \
  dpkg-dev \
  clang \
  libclang-dev \
  cmake \
  git \
  perl \
  python3 \
  libnl-3-dev \
  libnl-route-3-dev \
  libudev-dev \
  ocl-icd-opencl-dev \
  libopenmpi-dev \
  libopencv-dev \
  libgtk-3-dev \
  libasound2-dev \
  libv4l-dev \
  libgl1-mesa-dev \
  libegl1-mesa-dev \
  libx11-dev \
  libxext-dev \
  libxrender-dev \
  libice-dev \
  libsm-dev \
  libxcursor-dev \
  libxi-dev \
  libxrandr-dev \
  libxcomposite-dev \
  libxdamage-dev \
  libxfixes-dev \
  libxkbcommon-dev \
  libxkbcommon-x11-dev \
  libwayland-dev \
  libx11-xcb-dev \
  libxcb-randr0-dev \
  libxcb-shape0-dev \
  libxcb-xfixes0-dev \
  libfontconfig1-dev \
  libfreetype6-dev

if [[ ! -x /cargo/bin/rustup ]]; then
  curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal --default-toolchain stable
fi

# shellcheck disable=SC1091
. /cargo/env
rustup target add "$RUST_TARGET"

/workspace/scripts/package-release.sh \
  --version "$VERSION" \
  --output-dir /out \
  --target-triple "$RUST_TARGET" \
  --platform "$PLATFORM" \
  --deb-arch "$DEB_ARCH" \
  --cargo-features "$CARGO_FEATURES" \
  --cargo-build-targets "$CARGO_BUILD_TARGETS"
SCRIPT
}

WORKLOAD='standalone'
ARCH=''
OUTPUT_DIR=''
CACHE_DIR=''
CARGO_FEATURES=''
CARGO_BUILD_TARGETS=''
FORCE='false'

while (($#)); do
  case "$1" in
    --workload)
      shift
      (($#)) || die "--workload requires a value"
      WORKLOAD="$1"
      ;;
    --arch)
      shift
      (($#)) || die "--arch requires a value"
      ARCH="$1"
      ;;
    --output-dir)
      shift
      (($#)) || die "--output-dir requires a value"
      OUTPUT_DIR="$1"
      ;;
    --cache-dir)
      shift
      (($#)) || die "--cache-dir requires a value"
      CACHE_DIR="$1"
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
    --force)
      FORCE='true'
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

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
ROOT_DIR=$(CDPATH='' cd -- "$SCRIPT_DIR/.." && pwd)
# shellcheck source=scripts/container_workloads.sh
source "$SCRIPT_DIR/container_workloads.sh"

aarnn_container_validate_workload "$WORKLOAD" >/dev/null
ARCH="${ARCH:-$(host_arch)}"
ARCH="$(normalize_arch "$ARCH")"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/dist/container}"
CACHE_DIR="${CACHE_DIR:-$ROOT_DIR/.container-cache/debs}"
CARGO_FEATURES="${CARGO_FEATURES:-$(aarnn_container_workload_features "$WORKLOAD")}"
CARGO_BUILD_TARGETS="${CARGO_BUILD_TARGETS:-$(aarnn_container_workload_targets "$WORKLOAD")}"
RUST_TARGET="$(rust_target_for_arch "$ARCH")"
PLATFORM="$(platform_for_arch "$ARCH")"
DEB_ARCH="$(deb_arch_for_arch "$ARCH")"
VERSION="$(cargo_version)"
PACKAGE_BUILD_IMAGE="${CONTAINER_DEB_BUILD_IMAGE:-${PACKAGE_BUILD_IMAGE:-ubuntu:24.04}}"
OUTPUT_DIR="$(resolve_abs_dir "${OUTPUT_DIR:-$ROOT_DIR/dist/container}")"
CACHE_DIR="$(resolve_abs_dir "${CACHE_DIR:-$ROOT_DIR/.container-cache/debs}")"
PACKAGE_CACHE_DIR="$CACHE_DIR/$WORKLOAD/$ARCH"
FINGERPRINT_FILE="$PACKAGE_CACHE_DIR/.fingerprint"
FINGERPRINT="$(compute_fingerprint)"

mkdir -p "$PACKAGE_CACHE_DIR"

cached_deb() {
  find "$PACKAGE_CACHE_DIR" -maxdepth 1 -type f -name 'aarnn-rust_*_*.deb' | sort | head -n 1
}

DEB_PATH="$(cached_deb || true)"
if [[ "$FORCE" == 'true' || ! -f "$FINGERPRINT_FILE" || "$(cat "$FINGERPRINT_FILE" 2>/dev/null || true)" != "$FINGERPRINT" || -z "$DEB_PATH" ]]; then
  rm -rf "$PACKAGE_CACHE_DIR"
  mkdir -p "$PACKAGE_CACHE_DIR"
  build_package_in_container
  printf '%s\n' "$FINGERPRINT" >"$FINGERPRINT_FILE"
  DEB_PATH="$(cached_deb || true)"
fi

[[ -n "$DEB_PATH" && -f "$DEB_PATH" ]] || die "failed to prepare cached Debian package"

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"
STAGED_DEB="$OUTPUT_DIR/$(basename "$DEB_PATH")"
cp -f "$DEB_PATH" "$STAGED_DEB"

printf '%s\n' "$STAGED_DEB"
