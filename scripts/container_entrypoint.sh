#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -gt 0 ]; then
    exec "$@"
fi

workload="${AARNN_CONTAINER_WORKLOAD:-standalone}"
case "$workload" in
    standalone)
        exec /app/aarnn_rust \
            --continuous \
            --brain-id "${AARNN_BRAIN_ID:-standalone-container}"
        ;;
    orchestrator)
        exec /app/aarnn_rust \
            --orchestrator \
            --grpc-addr "${AARNN_GRPC_ADDR:-0.0.0.0:50051}" \
            --brain-id "${AARNN_BRAIN_ID:-orchestrator}"
        ;;
    node)
        exec /app/aarnn_rust \
            --node \
            --grpc-addr "${AARNN_GRPC_ADDR:-0.0.0.0:50051}" \
            --orchestrator-addr "${AARNN_ORCHESTRATOR_ADDR:-http://orchestrator:50051}" \
            --brain-id "${AARNN_BRAIN_ID:-node}"
        ;;
    web-ui)
        exec /app/web_ui \
            --listen "${AARNN_WEB_UI_LISTEN:-0.0.0.0:8080}" \
            --orchestrator "${AARNN_ORCHESTRATOR_ADDR:-http://orchestrator:50051}"
        ;;
    desktop-ui)
        exec /app/aarnn_rust \
            --ui \
            --brain-id "${AARNN_BRAIN_ID:-desktop-ui}"
        ;;
    *)
        echo "Unsupported AARNN container workload: $workload" >&2
        exit 64
        ;;
esac
