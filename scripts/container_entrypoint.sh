#!/usr/bin/env bash
set -euo pipefail

app_root="${AARNN_APP_ROOT:-/app}"
workload="${AARNN_CONTAINER_WORKLOAD:-standalone}"
default_bin=""
default_args=()

case "$workload" in
    standalone)
        default_bin="${app_root}/aarnn_rust"
        default_args=(
            --continuous
            --brain-id "${AARNN_BRAIN_ID:-standalone-container}"
        )
        ;;
    orchestrator|stable-orchestrator)
        default_bin="${app_root}/aarnn_rust"
        default_args=(
            --orchestrator
            --grpc-addr "${AARNN_GRPC_ADDR:-0.0.0.0:50051}"
            --brain-id "${AARNN_BRAIN_ID:-orchestrator}"
        )
        ;;
    node|stable-node)
        default_bin="${app_root}/aarnn_rust"
        default_args=(
            --node
            --grpc-addr "${AARNN_GRPC_ADDR:-0.0.0.0:50051}"
            --orchestrator-addr "${AARNN_ORCHESTRATOR_ADDR:-http://orchestrator:50051}"
            --brain-id "${AARNN_BRAIN_ID:-node}"
        )
        ;;
    web-ui)
        default_bin="${app_root}/web_ui"
        default_args=(
            --listen "${AARNN_WEB_UI_LISTEN:-0.0.0.0:8080}"
            --orchestrator "${AARNN_ORCHESTRATOR_ADDR:-http://orchestrator:50051}"
        )
        ;;
    desktop-ui)
        default_bin="${app_root}/aarnn_rust"
        default_args=(
            --ui
            --brain-id "${AARNN_BRAIN_ID:-desktop-ui}"
        )
        ;;
    *)
        echo "Unsupported AARNN container workload: $workload" >&2
        exit 64
        ;;
esac

# A deployment may provide a host- or provider-bound identity.  Keep the
# default compatibility workloads unchanged when it is absent, but never
# manufacture a second identity source inside the container wrapper.
if [[ "$workload" == "node" || "$workload" == "stable-node" || "$workload" == "orchestrator" || "$workload" == "stable-orchestrator" ]] && [[ -n "${AARNN_NODE_ID:-}" ]]; then
    default_args+=(--node-id "${AARNN_NODE_ID}")
fi

if [ "$#" -gt 0 ]; then
    case "$1" in
        --)
            shift
            if [ "$#" -eq 0 ]; then
                exec "${default_bin}" "${default_args[@]}"
            fi
            exec "${default_bin}" "$@"
            ;;
        -*)
            exec "${default_bin}" "$@"
            ;;
        *)
            exec "$@"
            ;;
    esac
fi

exec "${default_bin}" "${default_args[@]}"
