#!/bin/bash
set -euo pipefail

# Keep relative binaries, snapshots, logs and runtime state within the
# checkout containing this launcher, including when invoked by absolute path.
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

# Select ports
ORCH_PORT="$(find_free_port 50051)"; reserve_port "$ORCH_PORT"
NODE1_PORT="$(find_free_port 50075)"; reserve_port "$NODE1_PORT"
NODE2_PORT="$(find_free_port 50087)"; reserve_port "$NODE2_PORT"
WEB_UI_PORT="$(find_free_port 8080)"; reserve_port "$WEB_UI_PORT"

echo "Selected ports -> Orchestrator gRPC: $ORCH_PORT, Node1 gRPC: $NODE1_PORT, Node2 gRPC: $NODE2_PORT, Web UI: $WEB_UI_PORT"

CONFIG_PATH="${CONFIG_PATH:-config.json}"
NETWORK_PATH="${NETWORK_PATH:-network.json}"
EXAMPLE_RUNTIME_ROOT="${EXAMPLE_RUNTIME_ROOT:-data/examples-runtime}"
BIN_DIR="${AARNN_BIN_DIR:-target/release}"
mkdir -p "$EXAMPLE_RUNTIME_ROOT"

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
    # Keep this launcher on the local reference profile. The authenticated
    # management_v1 profile is intentionally not part of example startup.
    # Build both entry points in one package feature graph so Cargo does not
    # rebuild the shared library between the orchestrator and dashboard.
    cargo build --release --locked --no-default-features \
        --bin aarnn_rust --bin web_ui --features "engine_runtime,ui"
fi

#echo "Starting Standalone Network (Brain ID: standalone)..."
# Using --continuous to keep it running in background
#./target/release/aarnn_rust --brain-id standalone --continuous > standalone.log 2>&1 &
#PIDS=("$!")

export NMD_TFLITE_ALLOW_LARGE=1

echo "Starting Distributed Orchestrator (Brain ID: cluster_master)..."
"$BIN_DIR/aarnn_rust" --orchestrator --brain-id cluster_master \
    --grpc-addr 0.0.0.0:$ORCH_PORT --advertise-addr 127.0.0.1:$ORCH_PORT \
    "${CONFIG_ARG[@]}" "${NETWORK_ARG[@]}" > orchestrator.log 2>&1 &
PIDS=("$!")

# Wait a bit for orchestrator to start broadcasting
sleep 2
if ! kill -0 "${PIDS[0]}" 2>/dev/null; then
    echo "Orchestrator exited during startup; see orchestrator.log" >&2
    exit 1
fi

echo "Starting Distributed Nodes (Brain IDs: node_1, node_2) connecting to orchestrator at http://127.0.0.1:$ORCH_PORT ..."
"$BIN_DIR/aarnn_rust" --node --brain-id node_1 \
    --grpc-addr 0.0.0.0:$NODE1_PORT --advertise-addr 127.0.0.1:$NODE1_PORT \
    --orchestrator-addr http://127.0.0.1:$ORCH_PORT > node_1.log 2>&1 &
PIDS+=("$!")
sleep 1
if ! kill -0 "${PIDS[1]}" 2>/dev/null; then
    echo "Node node_1 exited during startup; see node_1.log" >&2
    exit 1
fi
"$BIN_DIR/aarnn_rust" --node --brain-id node_2 \
    --grpc-addr 0.0.0.0:$NODE2_PORT --advertise-addr 127.0.0.1:$NODE2_PORT \
    --orchestrator-addr http://127.0.0.1:$ORCH_PORT > node_2.log 2>&1 &
PIDS+=("$!")
sleep 1
if ! kill -0 "${PIDS[2]}" 2>/dev/null; then
    echo "Node node_2 exited during startup; see node_2.log" >&2
    exit 1
fi

echo "Start Web interface on http://127.0.0.1:$WEB_UI_PORT"
NM_RUNTIME_RESUME_EXISTING_WORKSPACES=0 \
NM_RUNTIME_RECONCILE_INTERVAL_MS=1000 \
NM_RUNTIME_AUTOSCALER_INTERVAL_MS=2000 \
"$BIN_DIR/web_ui" \
    --listen "127.0.0.1:$WEB_UI_PORT" \
    --orchestrator "http://127.0.0.1:$ORCH_PORT" \
    --runtime-root "$EXAMPLE_RUNTIME_ROOT" \
    > webui.log 2>&1 &
PIDS+=("$!")

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

echo "----------------------------------------------------------------"
echo "Both networks are now running!"
echo "Network 1 (Standalone): see standalone.log"
echo "Network 2 (Distributed): see node_1.log"
echo "The Orchestrator is now active."
echo "Orchestrator gRPC: http://127.0.0.1:$ORCH_PORT"
echo "Web dashboard URL (port $WEB_UI_PORT): $WEB_UI_URL"
echo "Check the 'Cluster Dashboard' section in the UI (right panel)."
echo "Press Ctrl+C to stop both networks."
echo "----------------------------------------------------------------"

# Keep the script running to maintain background jobs
wait
