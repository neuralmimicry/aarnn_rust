#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/container_run_common.sh
source "${ROOT_DIR}/scripts/container_run_common.sh"

aarnn_require_cmd podman
aarnn_require_cmd ss
aarnn_require_cmd curl
aarnn_require_cmd python3

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
ORCH_PORT_START="${AARNN_ORCH_PORT_START:-$((30000 + RANDOM % 20000))}"
ORCH_PORT="${ORCH_PORT:-$(aarnn_find_free_port "$ORCH_PORT_START")}"
WEB_UI_PORT="${WEB_UI_PORT:-$(aarnn_find_free_port 8080)}"
NODE_BASE_PORT="${NODE_BASE_PORT:-50075}"

case "${NODE_COUNT}" in
    ''|*[!0-9]*|0)
        echo "NODE_COUNT must be a positive integer" >&2
        exit 2
        ;;
esac

mkdir -p "${OUTPUT_DIR}" "${LOG_DIR}" "${RUNTIME_ROOT_HOST}"

PIDS=()
CONTAINERS=()
WORKER_IDS=()
WORKER_PORTS=()

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
    --execution-mode "distributed,sharded"
    --execution-scope cluster
    --execution-desired-shards "${NODE_COUNT}"
)
ORCH_PODMAN_ARGS=(
    --rm
    --network=host
    --name "${ORCH_NAME}"
    -e NM_MORPHO_ASYNC="${NM_MORPHO_ASYNC:-1}"
    -e NMD_TFLITE_ALLOW_LARGE=1
    -e NM_DISTRIBUTE_STARTUP_SNAPSHOT=1
    -e NM_DISTRIBUTED_AUTOSTART=1
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
    NODE_ID="node_${i}"
    NODE_NAME="aarnn-node-${i}-$(date +%s)"
    NODE_LOG="${LOG_DIR}/node_${i}.container.log"
    CONTAINERS+=("${NODE_NAME}")
    podman run --rm --network=host --name "${NODE_NAME}" \
        -e NM_MORPHO_ASYNC="${NM_MORPHO_ASYNC:-1}" \
        -e NMD_TFLITE_ALLOW_LARGE=1 \
        -v "${OUTPUT_DIR}:/app/outputs:Z" \
        -v "${LOG_DIR}:/app/logs:Z" \
        "${NODE_IMAGE}" \
        --node \
        --node-id "${NODE_ID}" \
        --brain-id "${BRAIN_ID_ORCH}" \
        --grpc-addr "0.0.0.0:${NODE_PORT}" \
        --advertise-addr "127.0.0.1:${NODE_PORT}" \
        --orchestrator-addr "http://127.0.0.1:${ORCH_PORT}" \
        --execution-mode distributed,sharded \
        --execution-scope cluster \
        --execution-desired-shards "${NODE_COUNT}" \
        >"${NODE_LOG}" 2>&1 &
    NODE_PID="$!"
    PIDS+=("${NODE_PID}")
    WORKER_IDS+=("${NODE_ID}")
    WORKER_PORTS+=("${NODE_PORT}")
    echo "Worker ${NODE_ID} for brain ${BRAIN_ID_ORCH} started: ${NODE_LOG}"
    sleep 1
done

WEB_UI_NAME="aarnn-web-ui-$(date +%s)"
WEB_UI_LOG="${LOG_DIR}/web_ui.container.log"
CONTAINERS+=("${WEB_UI_NAME}")
podman run --rm --network=host --name "${WEB_UI_NAME}" \
    -e NM_MORPHO_ASYNC="${NM_MORPHO_ASYNC:-1}" \
    -v "${RUNTIME_ROOT_HOST}:/app/data/runtime:Z" \
    "${WEB_UI_IMAGE}" \
    --listen "0.0.0.0:${WEB_UI_PORT}" \
    --orchestrator "http://127.0.0.1:${ORCH_PORT}" \
    --runtime-root /app/data/runtime \
    >"${WEB_UI_LOG}" 2>&1 &
WEB_UI_PID="$!"
PIDS+=("${WEB_UI_PID}")

WEB_UI_URL="http://127.0.0.1:${WEB_UI_PORT}"
web_ready=0
for _ in {1..60}; do
    if curl --fail --silent --show-error --max-time 1 "${WEB_UI_URL}/api/config" >/dev/null 2>&1; then
        web_ready=1
        break
    fi
    kill -0 "${WEB_UI_PID}" 2>/dev/null || break
    sleep 0.5
done
if [[ "${web_ready}" != "1" ]]; then
    echo "Web UI did not become ready at ${WEB_UI_URL}; see ${WEB_UI_LOG}" >&2
    exit 1
fi

cluster_check_args=(
    --base-url "${WEB_UI_URL}"
    --orchestrator "http://127.0.0.1:${ORCH_PORT}"
    --brain-id "${BRAIN_ID_ORCH}"
    --timeout "${AARNN_CLUSTER_READY_TIMEOUT_S:-45}"
)
for index in "${!WORKER_IDS[@]}"; do
    pid_index=$((index + 1))
    cluster_check_args+=(
        --worker "${WORKER_IDS[index]}=127.0.0.1:${WORKER_PORTS[index]}"
        --pid "${WORKER_IDS[index]}=${PIDS[pid_index]}"
    )
done
if ! CLUSTER_WORKERS="$(python3 "${ROOT_DIR}/scripts/qa/wait_for_local_cluster.py" "${cluster_check_args[@]}")"; then
    echo "Container cluster did not become ready; recent service logs follow:" >&2
    tail -n 60 "${ORCH_LOG}" "${WEB_UI_LOG}" >&2 || true
    for i in $(seq 1 "${NODE_COUNT}"); do
        tail -n 40 "${LOG_DIR}/node_${i}.container.log" >&2 || true
    done
    exit 1
fi

echo "----------------------------------------------------------------"
echo "Cluster running in containers."
echo "Local cluster master / I/O ingress brain: ${BRAIN_ID_ORCH}"
echo "Joined workers: ${CLUSTER_WORKERS}"
echo "Orchestrator: http://127.0.0.1:${ORCH_PORT}"
echo "Web UI:       http://127.0.0.1:${WEB_UI_PORT}"
echo "Logs:         ${LOG_DIR}"
echo "Press Ctrl+C to stop all workload containers."
echo "----------------------------------------------------------------"

wait
