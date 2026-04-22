#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/container_run_common.sh
source "${ROOT_DIR}/scripts/container_run_common.sh"

aarnn_require_cmd podman
aarnn_require_cmd ss

IMAGE_REF="${IMAGE_NAME:-$(aarnn_default_workload_image orchestrator)}"
GRPC_PORT="${GRPC_PORT:-$(aarnn_find_free_port 50051)}"
BRAIN_ID="${BRAIN_ID:-orchestrator}"
CONFIG_PATH="${CONFIG_PATH:-${ROOT_DIR}/config.json}"
NETWORK_PATH="${NETWORK_PATH:-}"
OUTPUT_DIR="${OUTPUT_DIR:-${ROOT_DIR}/outputs}"
LOG_DIR="${LOG_DIR:-${ROOT_DIR}/logs}"

mkdir -p "${OUTPUT_DIR}" "${LOG_DIR}"

RUN_ARGS=(
    --orchestrator
    --brain-id "${BRAIN_ID}"
    --grpc-addr "0.0.0.0:${GRPC_PORT}"
)
PODMAN_ARGS=(
    --rm
    --network=host
    --name "aarnn-orchestrator-$(date +%s)"
    -e NMD_TFLITE_ALLOW_LARGE=1
    -v "${OUTPUT_DIR}:/app/outputs:Z"
    -v "${LOG_DIR}:/app/logs:Z"
)

aarnn_append_optional_file_mount PODMAN_ARGS RUN_ARGS "${CONFIG_PATH}" /app/runtime-config.json --config
aarnn_append_optional_file_mount PODMAN_ARGS RUN_ARGS "${NETWORK_PATH}" /app/runtime-network.json --network

echo "Running orchestrator workload from ${IMAGE_REF}"
echo "gRPC address: http://127.0.0.1:${GRPC_PORT}"
echo "UDP discovery remains on container default port 50050."
exec podman run "${PODMAN_ARGS[@]}" "${IMAGE_REF}" "${RUN_ARGS[@]}"
