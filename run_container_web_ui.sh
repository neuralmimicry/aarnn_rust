#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/container_run_common.sh
source "${ROOT_DIR}/scripts/container_run_common.sh"

aarnn_require_cmd podman
aarnn_require_cmd ss

IMAGE_REF="${IMAGE_NAME:-$(aarnn_default_workload_image web-ui)}"
ORCHESTRATOR_ADDR="${ORCHESTRATOR_ADDR:-http://127.0.0.1:50051}"
LISTEN_PORT="${LISTEN_PORT:-$(aarnn_find_free_port 8080)}"
RUNTIME_ROOT_HOST="${RUNTIME_ROOT_HOST:-${ROOT_DIR}/data/runtime}"

mkdir -p "${RUNTIME_ROOT_HOST}"

PODMAN_ARGS=(
    --rm
    --network=host
    --name "aarnn-web-ui-$(date +%s)"
    -v "${RUNTIME_ROOT_HOST}:/app/data/runtime:Z"
)
RUN_ARGS=(
    --listen "0.0.0.0:${LISTEN_PORT}"
    --orchestrator "${ORCHESTRATOR_ADDR}"
    --runtime-root /app/data/runtime
)

echo "Running web-ui workload from ${IMAGE_REF}"
echo "Web UI: http://127.0.0.1:${LISTEN_PORT}"
echo "Orchestrator: ${ORCHESTRATOR_ADDR}"
exec podman run "${PODMAN_ARGS[@]}" "${IMAGE_REF}" "${RUN_ARGS[@]}"
