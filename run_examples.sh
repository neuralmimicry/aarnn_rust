#!/bin/bash

# Script to start two example networks:
# 1. A standalone network running in a single process.
# 2. A distributed network (orchestrator + node) with autodiscovery.

# Cleanup function to kill all background processes on exit
cleanup() {
    echo "Shutting down networks..."
    for pid in "${PIDS[@]}"; do
        if [ -n "$pid" ]; then
            kill "$pid" 2>/dev/null
        fi
    done
    if [ "${#PIDS[@]}" -gt 0 ]; then
        wait "${PIDS[@]}" 2>/dev/null
    fi
}

trap cleanup SIGINT SIGTERM EXIT

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

# Initialize PIDS array and select ports
PIDS=()
ORCH_PORT="$(find_free_port 50051)"; reserve_port "$ORCH_PORT"
NODE1_PORT="$(find_free_port 50075)"; reserve_port "$NODE1_PORT"
NODE2_PORT="$(find_free_port 50087)"; reserve_port "$NODE2_PORT"

echo "Selected ports -> Orchestrator gRPC: $ORCH_PORT, Node1 gRPC: $NODE1_PORT, Node2 gRPC: $NODE2_PORT"

CONFIG_PATH="${CONFIG_PATH:-config.json}"
NETWORK_PATH="${NETWORK_PATH:-network_aarnn_6layer.json}"

CONFIG_ARG=""
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
cargo build --release --all-features

if [ $? -ne 0 ]; then
    echo "Build failed. Exiting."
    exit 1
fi

#echo "Starting Standalone Network (Brain ID: standalone)..."
# Using --continuous to keep it running in background
#./target/release/aarnn_rust --brain-id standalone --continuous > standalone.log 2>&1 &
#PIDS=("$!")

export NMD_TFLITE_ALLOW_LARGE=1

echo "Starting Distributed Orchestrator (Brain ID: cluster_master) with UI Dashboard..."
# Launching with --ui so the dashboard is visible onscreen
./target/release/aarnn_rust --orchestrator --brain-id cluster_master --grpc-addr 0.0.0.0:$ORCH_PORT "${CONFIG_ARG[@]}" "${NETWORK_ARG[@]}" --ui > orchestrator.log 2>&1 &
PIDS=("$!")

# Wait a bit for orchestrator to start broadcasting
sleep 2

echo "Starting Distributed Nodes (Brain IDs: node_1, node_2) connecting to orchestrator at http://127.0.0.1:$ORCH_PORT ..."
./target/release/aarnn_rust --node --brain-id node_1 --grpc-addr 0.0.0.0:$NODE1_PORT --orchestrator-addr http://127.0.0.1:$ORCH_PORT > node_1.log 2>&1 &
PIDS+=("$!")
sleep 1
./target/release/aarnn_rust --node --brain-id node_2 --grpc-addr 0.0.0.0:$NODE2_PORT --orchestrator-addr http://127.0.0.1:$ORCH_PORT > node_2.log 2>&1 &
PIDS+=("$!")

echo "----------------------------------------------------------------"
echo "Both networks are now running!"
echo "Network 1 (Standalone): see standalone.log"
echo "Network 2 (Distributed): see node_1.log"
echo "The Orchestrator UI with Dashboard is now active onscreen."
echo "Check the 'Cluster Dashboard' section in the UI (right panel)."
echo "Press Ctrl+C to stop both networks."
echo "----------------------------------------------------------------"

# Keep the script running to maintain background jobs
wait
