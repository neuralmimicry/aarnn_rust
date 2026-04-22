#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/container_run_common.sh
source "${ROOT_DIR}/scripts/container_run_common.sh"

aarnn_require_cmd podman
aarnn_require_cmd ss

ORCH_IMAGE="${ORCH_IMAGE:-$(aarnn_default_workload_image orchestrator)}"
NODE_IMAGE="${NODE_IMAGE:-$(aarnn_default_workload_image node)}"
WEB_UI_IMAGE="${WEB_UI_IMAGE:-$(aarnn_default_workload_image web-ui)}"
NODE_COUNT="${NODE_COUNT:-2}"
BRAIN_ID_ORCH="${BRAIN_ID_ORCH:-cluster_master}"
CONFIG_PATH="${CONFIG_PATH:-${ROOT_DIR}/config.json}"
NETWORK_PATH="${NETWORK_PATH:-}"
OUTPUT_DIR="${OUTPUT_DIR:-${ROOT_DIR}/outputs}"
LOG_DIR="${LOG_DIR:-${ROOT_DIR}/logs}"
RUNTIME_ROOT_HOST="${RUNTIME_ROOT_HOST:-${ROOT_DIR}/data/runtime}"
ORCH_PORT="${ORCH_PORT:-$(aarnn_find_free_port 50051)}"
WEB_UI_PORT="${WEB_UI_PORT:-$(aarnn_find_free_port 8080)}"
NODE_BASE_PORT="${NODE_BASE_PORT:-50075}"

mkdir -p "${OUTPUT_DIR}" "${LOG_DIR}" "${RUNTIME_ROOT_HOST}"

PIDS=()
CONTAINERS=()

cleanup() {
    echo "Shutting down workload containers..."
    local name=""
    local pid=""
    for name in "${CONTAINERS[@]}"; do
        [ -n "$name" ] || continue
        podman stop "$name" >/dev/null 2>&1 || true
    done
    for pid in "${PIDS[@]}"; do
        [ -n "$pid" ] || continue
        kill "$pid" >/dev/null 2>&1 || true
    done
    wait "${PIDS[@]}" 2>/dev/null || true
}
trap cleanup SIGINT SIGTERM EXIT

ORCH_NAME="aarnn-orchestrator-$(date +%s)"
ORCH_LOG="${LOG_DIR}/orchestrator.container.log"
ORCH_ARGS=(
    --orchestrator
    --brain-id "${BRAIN_ID_ORCH}"
    --grpc-addr "0.0.0.0:${ORCH_PORT}"
)
ORCH_PODMAN_ARGS=(
    --rm
    --network=host
    --name "${ORCH_NAME}"
    -e NMD_TFLITE_ALLOW_LARGE=1
    -v "${OUTPUT_DIR}:/app/outputs:Z"
    -v "${LOG_DIR}:/app/logs:Z"
)
aarnn_append_optional_file_mount ORCH_PODMAN_ARGS ORCH_ARGS "${CONFIG_PATH}" /app/runtime-config.json --config
aarnn_append_optional_file_mount ORCH_PODMAN_ARGS ORCH_ARGS "${NETWORK_PATH}" /app/runtime-network.json --network

CONTAINERS+=("${ORCH_NAME}")
podman run "${ORCH_PODMAN_ARGS[@]}" "${ORCH_IMAGE}" "${ORCH_ARGS[@]}" >"${ORCH_LOG}" 2>&1 &
PIDS+=("$!")
echo "Orchestrator started: ${ORCH_LOG}"

echo "Waiting for orchestrator on ${ORCH_PORT}..."
sleep 2

for i in $(seq 1 "${NODE_COUNT}"); do
    NODE_PORT="$(aarnn_find_free_port $((NODE_BASE_PORT + i - 1)))"
    NODE_NAME="aarnn-node-${i}-$(date +%s)"
    NODE_LOG="${LOG_DIR}/node_${i}.container.log"
    CONTAINERS+=("${NODE_NAME}")
    podman run --rm --network=host --name "${NODE_NAME}" \
        -e NMD_TFLITE_ALLOW_LARGE=1 \
        -v "${OUTPUT_DIR}:/app/outputs:Z" \
        -v "${LOG_DIR}:/app/logs:Z" \
        "${NODE_IMAGE}" \
        --node \
        --brain-id "node_${i}" \
        --grpc-addr "0.0.0.0:${NODE_PORT}" \
        --orchestrator-addr "http://127.0.0.1:${ORCH_PORT}" \
        >"${NODE_LOG}" 2>&1 &
    PIDS+=("$!")
    echo "Node ${i} started: ${NODE_LOG}"
    sleep 1
done

WEB_UI_NAME="aarnn-web-ui-$(date +%s)"
WEB_UI_LOG="${LOG_DIR}/web_ui.container.log"
CONTAINERS+=("${WEB_UI_NAME}")
podman run --rm --network=host --name "${WEB_UI_NAME}" \
    -v "${RUNTIME_ROOT_HOST}:/app/data/runtime:Z" \
    "${WEB_UI_IMAGE}" \
    --listen "0.0.0.0:${WEB_UI_PORT}" \
    --orchestrator "http://127.0.0.1:${ORCH_PORT}" \
    --runtime-root /app/data/runtime \
    >"${WEB_UI_LOG}" 2>&1 &
PIDS+=("$!")

echo "----------------------------------------------------------------"
echo "Cluster running in containers."
echo "Orchestrator: http://127.0.0.1:${ORCH_PORT}"
echo "Web UI:       http://127.0.0.1:${WEB_UI_PORT}"
echo "Logs:         ${LOG_DIR}"
echo "Press Ctrl+C to stop all workload containers."
echo "----------------------------------------------------------------"

wait
