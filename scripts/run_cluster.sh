#!/usr/bin/env bash
set -Eeuo pipefail

# Run a local AARNN instance or a local orchestrator plus one or more workers.
# This launcher is intentionally process based so it exercises the same binary
# roles used by the container and Kubernetes launchers without requiring a
# container runtime.

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
cd "${ROOT_DIR}"

usage() {
    cat <<'USAGE'
Usage: scripts/run_cluster.sh [options]

Run one local standalone brain or a local orchestrator with workers.

Modes:
  --standalone              Run one continuous standalone brain.
  --nodes COUNT              Run an orchestrator with COUNT workers.
                            COUNT=1 is a single-worker cluster; COUNT>=2 is
                            a multi-worker cluster. Default: 1.

Options:
  --brain-id ID              Orchestrator/standalone brain ID.
  --config PATH              Optional network configuration JSON.
  --network PATH             Optional network snapshot JSON.
  --bin-dir PATH             Release binary directory. Default: target/release.
  --logs PATH                Log directory. Default: logs/local-cluster.
  --runtime-root PATH        Web runtime state directory.
  --orchestrator-port PORT   Fixed orchestrator gRPC port; otherwise auto.
  --node-port-start PORT     First worker gRPC port; otherwise auto.
  --web-port PORT            Fixed web UI port; otherwise auto.
  --no-web                   Do not start web_ui.
  --no-build                 Reuse binaries already in --bin-dir.
  --features LIST             Cargo features for a build.
                            Default: engine_runtime,ui,cuda.
  -h, --help                 Show this help.

Environment equivalents:
  AARNN_CLUSTER_NODES, AARNN_CLUSTER_BRAIN_ID, AARNN_CLUSTER_NO_WEB,
  AARNN_CLUSTER_NO_BUILD, AARNN_CLUSTER_FEATURES, AARNN_CLUSTER_LOG_DIR.

Examples:
  scripts/run_cluster.sh --nodes 1
  scripts/run_cluster.sh --nodes 3 --network network.json
  scripts/run_cluster.sh --standalone --no-web
USAGE
}

MODE="cluster"
NODE_COUNT="${AARNN_CLUSTER_NODES:-1}"
BRAIN_ID="${AARNN_CLUSTER_BRAIN_ID:-cluster_master}"
CONFIG_PATH="${CONFIG_PATH:-config.json}"
NETWORK_PATH="${NETWORK_PATH:-}"
BIN_DIR="${AARNN_BIN_DIR:-target/release}"
LOG_DIR="${AARNN_CLUSTER_LOG_DIR:-logs/local-cluster}"
RUNTIME_ROOT="${AARNN_CLUSTER_RUNTIME_ROOT:-data/local-cluster-runtime}"
FEATURES="${AARNN_CLUSTER_FEATURES:-engine_runtime,ui,cuda}"
NO_WEB="${AARNN_CLUSTER_NO_WEB:-0}"
NO_BUILD="${AARNN_CLUSTER_NO_BUILD:-0}"
ORCH_PORT="${AARNN_CLUSTER_ORCHESTRATOR_PORT:-}"
NODE_PORT_START="${AARNN_CLUSTER_NODE_PORT_START:-50075}"
WEB_PORT="${AARNN_CLUSTER_WEB_PORT:-}"

while (($#)); do
    case "$1" in
        --standalone) MODE="standalone"; shift ;;
        --nodes) NODE_COUNT="${2:?--nodes requires a count}"; shift 2 ;;
        --brain-id) BRAIN_ID="${2:?--brain-id requires an ID}"; shift 2 ;;
        --config) CONFIG_PATH="${2:?--config requires a path}"; shift 2 ;;
        --network) NETWORK_PATH="${2:?--network requires a path}"; shift 2 ;;
        --bin-dir) BIN_DIR="${2:?--bin-dir requires a path}"; shift 2 ;;
        --logs) LOG_DIR="${2:?--logs requires a path}"; shift 2 ;;
        --runtime-root) RUNTIME_ROOT="${2:?--runtime-root requires a path}"; shift 2 ;;
        --orchestrator-port) ORCH_PORT="${2:?--orchestrator-port requires a port}"; shift 2 ;;
        --node-port-start) NODE_PORT_START="${2:?--node-port-start requires a port}"; shift 2 ;;
        --web-port) WEB_PORT="${2:?--web-port requires a port}"; shift 2 ;;
        --no-web) NO_WEB=1; shift ;;
        --no-build) NO_BUILD=1; shift ;;
        --features) FEATURES="${2:?--features requires a comma separated list}"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

require_positive_integer() {
    local value="$1"
    local label="$2"
    if ! [[ "$value" =~ ^[0-9]+$ ]] || (( value < 1 )); then
        echo "$label must be a positive integer: $value" >&2
        exit 2
    fi
}

require_port() {
    local value="$1"
    local label="$2"
    require_positive_integer "$value" "$label"
    if (( value > 65535 )); then
        echo "$label must be in 1..65535: $value" >&2
        exit 2
    fi
}

if [[ "$MODE" == "cluster" ]]; then
    require_positive_integer "$NODE_COUNT" "--nodes"
    require_port "${NODE_PORT_START}" "--node-port-start"
    [[ -z "$ORCH_PORT" ]] || require_port "$ORCH_PORT" "--orchestrator-port"
    [[ -z "$WEB_PORT" ]] || require_port "$WEB_PORT" "--web-port"
fi

if [[ ! -f "$CONFIG_PATH" && "$CONFIG_PATH" != "config.json" ]]; then
    echo "configuration file does not exist: $CONFIG_PATH" >&2
    exit 2
fi
if [[ -n "$NETWORK_PATH" && ! -f "$NETWORK_PATH" ]]; then
    echo "network snapshot does not exist: $NETWORK_PATH" >&2
    exit 2
fi

if [[ "$NO_BUILD" != "1" ]]; then
    echo "Building local cluster binaries with features: $FEATURES"
    cargo build --release --locked --no-default-features \
        --bin aarnn_rust --bin web_ui --features "$FEATURES"
fi

AARNN_BIN="${BIN_DIR}/aarnn_rust"
WEB_BIN="${BIN_DIR}/web_ui"
if [[ ! -x "$AARNN_BIN" ]]; then
    echo "missing executable: $AARNN_BIN (build first or use --no-build correctly)" >&2
    exit 1
fi
if [[ "$MODE" == "cluster" && "$NO_WEB" != "1" && ! -x "$WEB_BIN" ]]; then
    echo "missing executable: $WEB_BIN (build first or use --no-build correctly)" >&2
    exit 1
fi

if [[ "$MODE" == "cluster" ]] && ! command -v ss >/dev/null 2>&1; then
    echo "ss is required for local port allocation and readiness checks" >&2
    exit 1
fi
if [[ "$MODE" == "cluster" && "$NO_WEB" != "1" ]] && ! command -v curl >/dev/null 2>&1; then
    echo "curl is required when the web UI is enabled" >&2
    exit 1
fi

declare -A USED_PORTS=()
is_port_free() {
    local port="$1"
    ! ss -H -ltn | awk '{print $4}' | awk -F: '{print $NF}' | grep -qx "$port" \
        && ! ss -H -lun | awk '{print $4}' | awk -F: '{print $NF}' | grep -qx "$port"
}

find_free_port() {
    local start="$1"
    local port="$start"
    while (( port <= 65535 )); do
        if [[ -z "${USED_PORTS[$port]+set}" ]] && is_port_free "$port"; then
            USED_PORTS[$port]=1
            FOUND_PORT="$port"
            return 0
        fi
        ((port++))
    done
    echo "no free port available at or above $start" >&2
    return 1
}

reserve_explicit_port() {
    local port="$1"
    local label="$2"
    if [[ -n "${USED_PORTS[$port]+set}" ]]; then
        echo "$label conflicts with another configured listener: $port" >&2
        exit 2
    fi
    if ! is_port_free "$port"; then
        echo "$label is already in use: $port" >&2
        exit 2
    fi
    USED_PORTS[$port]=1
}

if [[ "$MODE" == "standalone" ]]; then
    mkdir -p "$LOG_DIR"
    echo "Starting standalone brain '$BRAIN_ID'; log: $LOG_DIR/standalone.log"
    standalone_args=(--continuous --brain-id "$BRAIN_ID")
    [[ -f "$CONFIG_PATH" ]] && standalone_args+=(--config "$CONFIG_PATH")
    [[ -n "$NETWORK_PATH" ]] && standalone_args+=(--network "$NETWORK_PATH")
    exec "$AARNN_BIN" "${standalone_args[@]}" \
        >"$LOG_DIR/standalone.log" 2>&1
fi

FOUND_PORT=""
if [[ -z "$ORCH_PORT" ]]; then
    find_free_port 50051
    ORCH_PORT="$FOUND_PORT"
else
    reserve_explicit_port "$ORCH_PORT" "--orchestrator-port"
fi
for ((index=1; index<=NODE_COUNT; index++)); do
    find_free_port "$((NODE_PORT_START + index - 1))"
    NODE_PORTS[index]="$FOUND_PORT"
done
if [[ "$NO_WEB" != "1" ]]; then
    if [[ -z "$WEB_PORT" ]]; then
        find_free_port 8080
        WEB_PORT="$FOUND_PORT"
    else
        reserve_explicit_port "$WEB_PORT" "--web-port"
    fi
fi

mkdir -p "$LOG_DIR" "$RUNTIME_ROOT"
PIDS=()
CLEANUP_DONE=0
cleanup() {
    if (( CLEANUP_DONE == 1 )); then return; fi
    CLEANUP_DONE=1
    trap - EXIT INT TERM
    for pid in "${PIDS[@]}"; do kill -TERM "$pid" 2>/dev/null || true; done
    for _ in {1..20}; do
        local_running=0
        for pid in "${PIDS[@]}"; do
            if kill -0 "$pid" 2>/dev/null; then local_running=1; break; fi
        done
        (( local_running == 0 )) && break
        sleep 0.1
    done
    for pid in "${PIDS[@]}"; do kill -KILL "$pid" 2>/dev/null || true; done
    wait "${PIDS[@]}" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

CONFIG_ARGS=()
[[ -f "$CONFIG_PATH" ]] && CONFIG_ARGS=(--config "$CONFIG_PATH")
NETWORK_ARGS=()
[[ -n "$NETWORK_PATH" ]] && NETWORK_ARGS=(--network "$NETWORK_PATH")

export NMD_TFLITE_ALLOW_LARGE=1
echo "Starting orchestrator '$BRAIN_ID' on 127.0.0.1:$ORCH_PORT"
"$AARNN_BIN" --orchestrator --brain-id "$BRAIN_ID" \
    --grpc-addr "0.0.0.0:$ORCH_PORT" --advertise-addr "127.0.0.1:$ORCH_PORT" \
    "${CONFIG_ARGS[@]}" "${NETWORK_ARGS[@]}" \
    >"$LOG_DIR/orchestrator.log" 2>&1 &
PIDS+=("$!")

wait_for_port() {
    local port="$1"
    local pid="$2"
    for _ in {1..100}; do
        if ! kill -0 "$pid" 2>/dev/null; then return 1; fi
        if ss -H -ltn | awk '{print $4}' | awk -F: '{print $NF}' | grep -qx "$port"; then return 0; fi
        sleep 0.1
    done
    return 1
}

if ! wait_for_port "$ORCH_PORT" "${PIDS[0]}"; then
    echo "orchestrator did not listen on port $ORCH_PORT; see $LOG_DIR/orchestrator.log" >&2
    exit 1
fi

for ((index=1; index<=NODE_COUNT; index++)); do
    node_port="${NODE_PORTS[index]}"
    echo "Starting node_${index} on 127.0.0.1:$node_port"
    "$AARNN_BIN" --node --brain-id "node_${index}" \
        --grpc-addr "0.0.0.0:$node_port" --advertise-addr "127.0.0.1:$node_port" \
        --orchestrator-addr "http://127.0.0.1:$ORCH_PORT" \
        >"$LOG_DIR/node_${index}.log" 2>&1 &
    node_pid="$!"
    PIDS+=("$node_pid")
    if ! wait_for_port "$node_port" "$node_pid"; then
        echo "node_${index} did not listen on port $node_port; see $LOG_DIR/node_${index}.log" >&2
        exit 1
    fi
done

if [[ "$NO_WEB" != "1" ]]; then
    echo "Starting web UI on http://127.0.0.1:$WEB_PORT"
    NM_RUNTIME_RESUME_EXISTING_WORKSPACES=0 \
    NM_RUNTIME_RECONCILE_INTERVAL_MS=1000 \
    NM_RUNTIME_AUTOSCALER_INTERVAL_MS=2000 \
    "$WEB_BIN" --listen "127.0.0.1:$WEB_PORT" \
        --orchestrator "http://127.0.0.1:$ORCH_PORT" \
        --runtime-root "$RUNTIME_ROOT" \
        >"$LOG_DIR/web_ui.log" 2>&1 &
    web_pid="$!"
    PIDS+=("$web_pid")
    web_ready=0
    for _ in {1..100}; do
        if curl --fail --silent --show-error --max-time 1 \
            "http://127.0.0.1:$WEB_PORT/api/config" >/dev/null 2>&1; then
            web_ready=1
            break
        fi
        kill -0 "$web_pid" 2>/dev/null || break
        sleep 0.1
    done
    if (( web_ready == 0 )); then
        echo "web UI did not become ready; see $LOG_DIR/web_ui.log" >&2
        exit 1
    fi
fi

echo "----------------------------------------------------------------"
echo "Local AARNN cluster is running with $NODE_COUNT worker(s)."
echo "Orchestrator: http://127.0.0.1:$ORCH_PORT"
if [[ "$NO_WEB" != "1" ]]; then echo "Web UI:       http://127.0.0.1:$WEB_PORT"; fi
echo "Logs:         $LOG_DIR"
echo "Press Ctrl+C to stop all processes."
echo "----------------------------------------------------------------"
wait
