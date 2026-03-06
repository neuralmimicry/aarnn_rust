#!/usr/bin/env bash
set -euo pipefail

IMAGE_NAME=${1:-"ghcr.io/neuralmimicry/aarnn_rust:brainregions-arm64"}
BRAIN_ID=${2:-"motor"}

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

mkdir -p "$HOME/.cache" "$PWD/outputs" "$PWD/logs"

podman run --rm \
  --network=host \
  --user "$(id -u):$(id -g)" \
  -e DISPLAY="$DISPLAY" \
  -e XAUTHORITY=/tmp/.Xauthority \
  -e XDG_CACHE_HOME=/tmp/cache \
  -e FONTCONFIG_PATH=/etc/fonts \
  -e LIBGL_ALWAYS_SOFTWARE=1 \
  -e MESA_GL_VERSION_OVERRIDE=3.3 \
  -e MESA_LOADER_DRIVER_OVERRIDE=llvmpipe \
  -v "$XAUTH:/tmp/.Xauthority:ro" \
  -v "$HOME/.cache:/tmp/cache:Z" \
  -v "$PWD/outputs:/app/outputs:Z" \
  -v "$PWD/logs:/app/logs:Z" \
  "$IMAGE_NAME" \
  --brain-id "$BRAIN_ID" --ui --trace
