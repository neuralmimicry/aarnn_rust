#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/container_run_common.sh
source "${ROOT_DIR}/scripts/container_run_common.sh"

aarnn_require_cmd podman
aarnn_require_cmd xauth

IMAGE_REF="${1:-${IMAGE_NAME:-$(aarnn_default_workload_image desktop-ui)}}"
BRAIN_ID="${2:-${BRAIN_ID:-motor}}"
CONFIG_PATH="${CONFIG_PATH:-${ROOT_DIR}/config.json}"
NETWORK_PATH="${NETWORK_PATH:-}"
OUTPUT_DIR="${OUTPUT_DIR:-${ROOT_DIR}/outputs}"
LOG_DIR="${LOG_DIR:-${ROOT_DIR}/logs}"
CACHE_DIR="${CACHE_DIR:-${HOME}/.cache}"

if [ -z "${DISPLAY:-}" ]; then
    echo "DISPLAY is not set. Use ssh -X/-Y or a local X server." >&2
    exit 1
fi

XAUTHORITY_SRC="${XAUTHORITY:-${HOME}/.Xauthority}"
if [ ! -f "${XAUTHORITY_SRC}" ]; then
    echo "Source Xauthority file not found: ${XAUTHORITY_SRC}" >&2
    exit 1
fi

XAUTH=/tmp/.podman.xauth
rm -f "${XAUTH}"
touch "${XAUTH}"
if ! xauth -f "${XAUTHORITY_SRC}" extract "${XAUTH}" "$DISPLAY" >/dev/null 2>&1; then
    echo "Failed to extract Xauthority for DISPLAY=${DISPLAY}" >&2
    xauth -f "${XAUTHORITY_SRC}" list >&2 || true
    exit 1
fi

mkdir -p "${OUTPUT_DIR}" "${LOG_DIR}" "${CACHE_DIR}"

RUN_ARGS=(
    --brain-id "${BRAIN_ID}"
    --ui
    --trace
)
PODMAN_ARGS=(
    --rm
    --network=host
    --user "$(id -u):$(id -g)"
    --name "aarnn-desktop-ui-$(date +%s)"
    -e DISPLAY="${DISPLAY}"
    -e XAUTHORITY=/tmp/.Xauthority
    -e XDG_CACHE_HOME=/tmp/cache
    -e FONTCONFIG_PATH=/etc/fonts
    -e LIBGL_ALWAYS_SOFTWARE=1
    -e MESA_GL_VERSION_OVERRIDE=3.3
    -e MESA_LOADER_DRIVER_OVERRIDE=llvmpipe
    -v "${XAUTH}:/tmp/.Xauthority:ro"
    -v "${CACHE_DIR}:/tmp/cache:Z"
    -v "${OUTPUT_DIR}:/app/outputs:Z"
    -v "${LOG_DIR}:/app/logs:Z"
)

aarnn_append_optional_file_mount PODMAN_ARGS RUN_ARGS "${CONFIG_PATH}" /app/runtime-config.json --config
aarnn_append_optional_file_mount PODMAN_ARGS RUN_ARGS "${NETWORK_PATH}" /app/runtime-network.json --network

echo "Running desktop-ui workload from ${IMAGE_REF}"
exec podman run "${PODMAN_ARGS[@]}" "${IMAGE_REF}" "${RUN_ARGS[@]}"
