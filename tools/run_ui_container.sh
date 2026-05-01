#!/usr/bin/env bash
set -euo pipefail

IMAGE_NAME=${1:-""}
BRAIN_ID=${2:-"motor"}

if [ -z "$IMAGE_NAME" ]; then
  case "$(uname -m)" in
    x86_64|amd64) IMAGE_ARCH="amd64" ;;
    aarch64|arm64) IMAGE_ARCH="arm64" ;;
    *) IMAGE_ARCH="$(uname -m)" ;;
  esac
  IMAGE_NAME="ghcr.io/neuralmimicry/aarnn_rust:engine-desktop-ui-${IMAGE_ARCH}"
fi

UI_RENDERER="${NM_UI_RENDERER:-glow}"
CACHE_DIR="${CACHE_DIR:-$PWD/.container-cache/desktop-ui}"

if ! command -v xauth >/dev/null 2>&1; then
  echo "xauth not found. Install xauth and try again." >&2
  exit 1
fi

if [ -z "${DISPLAY:-}" ]; then
  echo "DISPLAY is not set. Use ssh -X/-Y to connect with X11 forwarding." >&2
  exit 1
fi

XAUTH=/tmp/.podman.xauth
rm -f "$XAUTH"
touch "$XAUTH"

# Export the current DISPLAY cookie into a dedicated file for the container.
XAUTHORITY_SRC="${XAUTHORITY:-$HOME/.Xauthority}"
if [ ! -f "$XAUTHORITY_SRC" ]; then
  echo "Source Xauthority file not found: $XAUTHORITY_SRC" >&2
  exit 1
fi

if ! xauth -f "$XAUTHORITY_SRC" extract "$XAUTH" "$DISPLAY" >/dev/null 2>&1; then
  echo "Failed to extract Xauthority for DISPLAY=$DISPLAY from $XAUTHORITY_SRC" >&2
  echo "Available entries:" >&2
  xauth -f "$XAUTHORITY_SRC" list >&2 || true
  exit 1
fi

if ! xauth -f "$XAUTH" list >/dev/null 2>&1; then
  echo "Failed to create Xauthority file at $XAUTH" >&2
  exit 1
fi

mkdir -p "$CACHE_DIR" "$PWD/outputs" "$PWD/logs"

PODMAN_ARGS=(
  --rm
  --network=host
  --ipc=host
  --userns=keep-id
  --user "$(id -u):$(id -g)"
  -e DISPLAY="$DISPLAY"
  -e XAUTHORITY=/tmp/.Xauthority
  -e XDG_CACHE_HOME=/tmp/cache
  -e MESA_SHADER_CACHE_DIR=/tmp/cache/mesa_shader_cache
  -e FONTCONFIG_PATH=/etc/fonts
  -e NM_UI_RENDERER="$UI_RENDERER"
  -e WINIT_UNIX_BACKEND=x11
  -e LIBGL_ALWAYS_SOFTWARE=1
  -e LIBGL_DRI3_DISABLE=1
  -e MESA_GL_VERSION_OVERRIDE=3.3
  -e MESA_LOADER_DRIVER_OVERRIDE=llvmpipe
  -v "$XAUTH:/tmp/.Xauthority:ro"
  -v "$CACHE_DIR:/tmp/cache:Z"
  -v "$PWD/outputs:/app/outputs:Z"
  -v "$PWD/logs:/app/logs:Z"
)

if [ -d /tmp/.X11-unix ]; then
  PODMAN_ARGS+=( -v /tmp/.X11-unix:/tmp/.X11-unix:ro )
fi

podman run "${PODMAN_ARGS[@]}" \
  "$IMAGE_NAME" \
  --brain-id "$BRAIN_ID" --ui --trace
