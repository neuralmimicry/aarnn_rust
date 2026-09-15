#!/bin/bash
set -euo pipefail

# Resolve all relative paths from this checkout even when the launcher is
# invoked through an absolute path from another working directory. This keeps
# binaries, snapshots, logs and runtime state tied to the launcher that was
# selected by the operator.
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Script to start two example networks:
# 1. A standalone network running in a single process.
# 2. A distributed network (orchestrator + node) with autodiscovery.

# Initialise before installing traps so an early prerequisite failure can
# still run the cleanup handler safely under `set -u`.
PIDS=()
CLEANUP_DONE=0

# Cleanup function to kill all background processes on exit
cleanup() {
    if [ "$CLEANUP_DONE" -eq 1 ]; then
        return
    fi
    CLEANUP_DONE=1
    trap - EXIT SIGINT SIGTERM
    echo "Shutting down networks..."
    for pid in "${PIDS[@]}"; do
        if [ -n "$pid" ]; then
            kill "$pid" 2>/dev/null || true
        fi
    done
    # Child nodes can be inside a blocking network call during shutdown. Give
    # them a bounded grace period, then force only the processes launched by
    # this script so Ctrl+C cannot leave a cluster behind or hang forever.
    for _ in {1..10}; do
        running=0
        for pid in "${PIDS[@]}"; do
            if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
                running=1
                break
            fi
        done
        [ "$running" -eq 0 ] && break
        sleep 0.1
    done
    for pid in "${PIDS[@]}"; do
        if [ -n "$pid" ]; then
            kill -KILL "$pid" 2>/dev/null || true
        fi
    done
    wait "${PIDS[@]}" 2>/dev/null || true
}

trap cleanup SIGINT SIGTERM EXIT

if ! command -v curl >/dev/null 2>&1; then
    echo "curl is required to verify the web dashboard readiness" >&2
    exit 1
fi

# ----- Dynamic port selection helpers -----
# Track reserved ports in this script run to avoid accidental reuse
declare -A USED_PORTS=()
reserve_port() { USED_PORTS[$1]=1; }

# Check if a port is free for both TCP and UDP
is_port_free() {
    local port="$1"
    # TCP listeners
    if ss -H -ltn | awk '{print $4}' | awk -F: '{print $NF}' | grep -qx "$port"; then
        return 1
    fi
    # UDP listeners
    if ss -H -lun | awk '{print $4}' | awk -F: '{print $NF}' | grep -qx "$port"; then
        return 1
    fi
    return 0
}

# Find the next available port at or above a starting value
find_free_port() {
    local start="${1:-50051}"
    local p="$start"
    while [ "$p" -le 65535 ]; do
        if is_port_free "$p" && [ -z "${USED_PORTS[$p]+x}" ]; then
            echo "$p"
            return 0
        fi
        p=$((p+1))
    done
    echo ""; return 1
}

require_process() {
    local pid="$1"
    local label="$2"
    if ! kill -0 "$pid" 2>/dev/null; then
        echo "$label exited during startup; see its log" >&2
        exit 1
    fi
}

# Select ports.  The starts are overridable so a second launcher can be
# isolated from stale clients that still reconnect to a previous orchestrator
# endpoint even after its listener has gone away.
validate_port_start() {
    local name="$1"
    local value="$2"
    case "$value" in
        ''|*[!0-9]*)
            echo "$name must be an integer in the range 1..65535" >&2
            exit 1
            ;;
    esac
    if [ "$value" -lt 1 ] || [ "$value" -gt 65535 ]; then
        echo "$name must be an integer in the range 1..65535" >&2
        exit 1
    fi
}

ORCH_PORT_START="${AARNN_ORCH_PORT_START:-50051}"
NODE1_PORT_START="${AARNN_NODE1_PORT_START:-50075}"
NODE2_PORT_START="${AARNN_NODE2_PORT_START:-50087}"
WEB_UI_PORT_START="${AARNN_WEB_PORT_START:-8080}"
validate_port_start AARNN_ORCH_PORT_START "$ORCH_PORT_START"
validate_port_start AARNN_NODE1_PORT_START "$NODE1_PORT_START"
validate_port_start AARNN_NODE2_PORT_START "$NODE2_PORT_START"
validate_port_start AARNN_WEB_PORT_START "$WEB_UI_PORT_START"
ORCH_PORT="$(find_free_port "$ORCH_PORT_START")"; reserve_port "$ORCH_PORT"
NODE1_PORT="$(find_free_port "$NODE1_PORT_START")"; reserve_port "$NODE1_PORT"
NODE2_PORT="$(find_free_port "$NODE2_PORT_START")"; reserve_port "$NODE2_PORT"
WEB_UI_PORT="$(find_free_port "$WEB_UI_PORT_START")"; reserve_port "$WEB_UI_PORT"

echo "Selected ports -> Orchestrator gRPC: $ORCH_PORT, Node1 gRPC: $NODE1_PORT, Node2 gRPC: $NODE2_PORT, Web UI: $WEB_UI_PORT"

# The launcher has explicit node endpoints, so keep its discovery beacon local
# to an otherwise unused loopback target by default.  This prevents a stale
# node from another example run from joining the new test cluster through the
# broadcast beacon.  Operators can restore a configured discovery target with
# NM_DISCOVERY_TARGETS or AARNN_DISCOVERY_TARGETS.
if [ -z "${NM_DISCOVERY_TARGETS:-}" ]; then
    export NM_DISCOVERY_TARGETS="${AARNN_DISCOVERY_TARGETS:-127.0.0.1:59999}"
fi
if [ -z "${NM_DISCOVERY_DISABLE_DEFAULTS:-}" ]; then
    export NM_DISCOVERY_DISABLE_DEFAULTS="${AARNN_DISCOVERY_DISABLE_DEFAULTS:-1}"
fi

CONFIG_PATH="${CONFIG_PATH:-config.json}"
NETWORK_PATH="${NETWORK_PATH:-network.json}"
EXAMPLE_RUNTIME_ROOT="${EXAMPLE_RUNTIME_ROOT:-data/examples-runtime}"
BIN_DIR="${AARNN_BIN_DIR:-target/release}"
mkdir -p "$EXAMPLE_RUNTIME_ROOT"

# An explicit audio source is loaded by the native Rust UI at startup.  Keep
# this opt-in so the example launcher remains useful on machines without the
# operator's media files, while validating the requested source before any
# cluster processes are started.
if [ -n "${AARNN_AUDIO_FILE:-}" ]; then
    if [ ! -f "$AARNN_AUDIO_FILE" ] || [ ! -r "$AARNN_AUDIO_FILE" ]; then
        echo "AARNN_AUDIO_FILE is not a readable regular file: $AARNN_AUDIO_FILE" >&2
        exit 1
    fi
    AARNN_AUDIO_SENSORY_NEURONS="${AARNN_AUDIO_SENSORY_NEURONS:-64}"
    case "$AARNN_AUDIO_SENSORY_NEURONS" in
        ''|*[!0-9]*)
            echo "AARNN_AUDIO_SENSORY_NEURONS must be a positive integer" >&2
            exit 1
            ;;
    esac
    if [ "$AARNN_AUDIO_SENSORY_NEURONS" -lt 1 ]; then
        echo "AARNN_AUDIO_SENSORY_NEURONS must be at least 1" >&2
        exit 1
    fi
    if [ "$AARNN_AUDIO_SENSORY_NEURONS" -gt 65536 ]; then
        echo "AARNN_AUDIO_SENSORY_NEURONS must be at most 65536" >&2
        exit 1
    fi
    export AARNN_AUDIO_FILE AARNN_AUDIO_SENSORY_NEURONS
    echo "Native Rust UI audio source: $AARNN_AUDIO_FILE (sensory neurons: $AARNN_AUDIO_SENSORY_NEURONS)"
fi

# The normal example uses the checked-in snapshot unchanged. The optional
# verification mode makes a private growth-capable copy, then exercises the
# live compatibility sharder through a worker failure and automatic
# relocation. It deliberately keeps layer splitting out of the probe: that
# is a separate topology transaction and would obscure this regression.
VERIFY_SHARD_GROWTH="${AARNN_VERIFY_SHARD_GROWTH:-0}"
if [ "$VERIFY_SHARD_GROWTH" = "1" ]; then
    if ! command -v python3 >/dev/null 2>&1; then
        echo "python3 is required for AARNN_VERIFY_SHARD_GROWTH=1" >&2
        exit 1
    fi
    if [ ! -f "$NETWORK_PATH" ]; then
        echo "AARNN_VERIFY_SHARD_GROWTH=1 requires a network snapshot at '$NETWORK_PATH'" >&2
        exit 1
    fi
    VERIFY_NETWORK_PATH="$EXAMPLE_RUNTIME_ROOT/shard-growth-network.json"
    python3 "$SCRIPT_DIR/scripts/qa/verify_example_sharding.py" \
        --make-fixture "$NETWORK_PATH" "$VERIFY_NETWORK_PATH" \
        --headroom "${AARNN_VERIFY_GROWTH_HEADROOM:-1024}" >/dev/null
    NETWORK_PATH="$VERIFY_NETWORK_PATH"
    echo "Using private growth verification snapshot: $NETWORK_PATH"
fi

NATIVE_UI_ARGS=()
NATIVE_UI_STATUS="disabled"
if [ "${AARNN_NATIVE_UI:-1}" != "0" ]; then
    NATIVE_UI_ARGS=(--ui)
    NATIVE_UI_STATUS="active onscreen"
fi

CONFIG_ARG=()
if [ -f "$CONFIG_PATH" ]; then
    CONFIG_ARG=(--config "$CONFIG_PATH")
    echo "Using config: $CONFIG_PATH"
else
    echo "Config file '$CONFIG_PATH' not found; using defaults"
fi

NETWORK_ARG=()
if [ -f "$NETWORK_PATH" ]; then
    NETWORK_ARG=(--network "$NETWORK_PATH")
    echo "Using network snapshot: $NETWORK_PATH"
else
    echo "Network snapshot '$NETWORK_PATH' not found; skipping --network"
fi

echo "Building project..."
if [ "${AARNN_SKIP_BUILD:-0}" = "1" ]; then
    echo "Skipping build (AARNN_SKIP_BUILD=1); using binaries from $BIN_DIR"
else
    # Keep the local example profile explicit.  In particular, do not use
    # --all-features here: management_v1 is a production-only, authenticated
    # control-plane profile and requires bearer credentials plus mTLS.
    # Build both entry points in one package feature graph.  Building the
    # binaries separately with different feature sets makes Cargo rebuild the
    # shared library before the dashboard can start.
    cargo build --release --locked --no-default-features \
        --bin aarnn_rust --bin web_ui --features "engine_runtime,ui,cuda"
fi

#echo "Starting Standalone Network (Brain ID: standalone)..."
# Using --continuous to keep it running in background
#./target/release/aarnn_rust --brain-id standalone --continuous > standalone.log 2>&1 &
#PIDS=("$!")

export NMD_TFLITE_ALLOW_LARGE=1

echo "Starting Distributed Orchestrator (Brain ID: cluster_master)..."
"$BIN_DIR/aarnn_rust" --orchestrator --brain-id cluster_master \
    --grpc-addr "0.0.0.0:$ORCH_PORT" --advertise-addr "127.0.0.1:$ORCH_PORT" \
    "${CONFIG_ARG[@]}" "${NETWORK_ARG[@]}" "${NATIVE_UI_ARGS[@]}" > orchestrator.log 2>&1 &
PIDS=("$!")

# Wait a bit for orchestrator to start broadcasting
sleep 2
require_process "${PIDS[0]}" "Orchestrator"

echo "Starting Distributed Nodes (Brain IDs: node_1, node_2) connecting to orchestrator at http://127.0.0.1:$ORCH_PORT ..."
"$BIN_DIR/aarnn_rust" --node --brain-id node_1 \
    --grpc-addr "0.0.0.0:$NODE1_PORT" --advertise-addr "127.0.0.1:$NODE1_PORT" \
    --orchestrator-addr "http://127.0.0.1:$ORCH_PORT" > node_1.log 2>&1 &
NODE1_PID="$!"
PIDS+=("$!")
sleep 1
require_process "${PIDS[1]}" "Node node_1"
"$BIN_DIR/aarnn_rust" --node --brain-id node_2 \
    --grpc-addr "0.0.0.0:$NODE2_PORT" --advertise-addr "127.0.0.1:$NODE2_PORT" \
    --orchestrator-addr "http://127.0.0.1:$ORCH_PORT" > node_2.log 2>&1 &
NODE2_PID="$!"
PIDS+=("$!")
sleep 1
require_process "${PIDS[2]}" "Node node_2"

echo "Starting web dashboard on http://127.0.0.1:$WEB_UI_PORT ..."
NM_RUNTIME_RESUME_EXISTING_WORKSPACES=0 \
NM_RUNTIME_RECONCILE_INTERVAL_MS=1000 \
NM_RUNTIME_AUTOSCALER_INTERVAL_MS=2000 \
"$BIN_DIR/web_ui" \
    --listen "127.0.0.1:$WEB_UI_PORT" \
    --orchestrator "http://127.0.0.1:$ORCH_PORT" \
    --runtime-root "$EXAMPLE_RUNTIME_ROOT" \
    > webui.log 2>&1 &
PIDS+=("$!")

# Do not report a URL until the dashboard has bound its port and is serving
# assets. This also turns an early web-ui startup failure into an actionable
# launcher error instead of leaving the user with a dead browser tab.
WEB_UI_URL="http://127.0.0.1:$WEB_UI_PORT"
for _ in {1..30}; do
    if curl --fail --silent --show-error --max-time 1 "$WEB_UI_URL/api/config" >/dev/null 2>&1; then
        break
    fi
    if ! kill -0 "${PIDS[-1]}" 2>/dev/null; then
        echo "Web UI exited before becoming ready; see webui.log" >&2
        exit 1
    fi
    sleep 1
done
if ! curl --fail --silent --show-error --max-time 1 "$WEB_UI_URL/api/config" >/dev/null 2>&1; then
    echo "Web UI did not become ready at $WEB_UI_URL; see webui.log" >&2
    exit 1
fi

if [ "$VERIFY_SHARD_GROWTH" = "1" ]; then
    python3 "$SCRIPT_DIR/scripts/qa/verify_example_sharding.py" \
        --base-url "$WEB_UI_URL" \
        --orchestrator "127.0.0.1:$ORCH_PORT" \
        --node1-pid "$NODE1_PID" \
        --node2-pid "$NODE2_PID" \
        --timeout "${AARNN_VERIFY_TIMEOUT_S:-50}"
    echo "Shard relocation and post-relocation growth verification passed."
    exit 0
fi

echo "----------------------------------------------------------------"
echo "The distributed example network is running."
echo "Orchestrator gRPC: http://127.0.0.1:$ORCH_PORT"
echo "Web dashboard URL (port $WEB_UI_PORT): $WEB_UI_URL"
echo "Native Rust UI: $NATIVE_UI_STATUS."
echo "Node logs: node_1.log and node_2.log"
echo "Check the 'Cluster Dashboard' section in either UI."
echo "Press Ctrl+C to stop the example network and dashboard."
echo "----------------------------------------------------------------"

# Keep the script running to maintain background jobs
wait
