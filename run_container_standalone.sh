#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/container_run_common.sh
source "${ROOT_DIR}/scripts/container_run_common.sh"

aarnn_require_cmd podman

IMAGE_REF="${IMAGE_NAME:-$(aarnn_default_workload_image standalone)}"
BRAIN_ID="${BRAIN_ID:-standalone-container}"
CONFIG_PATH="${CONFIG_PATH:-${ROOT_DIR}/config.json}"
NETWORK_PATH="${NETWORK_PATH:-}"
OUTPUT_DIR="${OUTPUT_DIR:-${ROOT_DIR}/outputs}"
LOG_DIR="${LOG_DIR:-${ROOT_DIR}/logs}"

mkdir -p "${OUTPUT_DIR}" "${LOG_DIR}"

RUN_ARGS=(
    --continuous
    --brain-id "${BRAIN_ID}"
)
PODMAN_ARGS=(
    --rm
    --name "aarnn-standalone-$(date +%s)"
    -v "${OUTPUT_DIR}:/app/outputs:Z"
    -v "${LOG_DIR}:/app/logs:Z"
)

aarnn_append_optional_file_mount PODMAN_ARGS RUN_ARGS "${CONFIG_PATH}" /app/runtime-config.json --config
aarnn_append_optional_file_mount PODMAN_ARGS RUN_ARGS "${NETWORK_PATH}" /app/runtime-network.json --network

echo "Running standalone workload from ${IMAGE_REF}"
exec podman run "${PODMAN_ARGS[@]}" "${IMAGE_REF}" "${RUN_ARGS[@]}"
