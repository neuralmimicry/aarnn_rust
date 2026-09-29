#!/usr/bin/env bash
set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

export NM_MORPHO_ASYNC="${NM_MORPHO_ASYNC:-1}"

PIDS=()
cleanup() {
    trap - EXIT INT TERM
    for pid in "${PIDS[@]}"; do kill -TERM "$pid" 2>/dev/null || true; done
    for _ in {1..20}; do
        running=0
        for pid in "${PIDS[@]}"; do
            if kill -0 "$pid" 2>/dev/null; then running=1; break; fi
        done
        (( running == 0 )) && break
        sleep 0.1
    done
    for pid in "${PIDS[@]}"; do kill -KILL "$pid" 2>/dev/null || true; done
    wait "${PIDS[@]}" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# Ensure the binary is built
cargo build --all-features --bin aarnn_rust

CLUSTER_BRAIN_ID="cluster_master"
ORCH_PORT="${AARNN_ORCH_PORT:-$((30000 + RANDOM % 20000))}"
NODE1_PORT="${AARNN_NODE1_PORT:-50052}"
NODE2_PORT="${AARNN_NODE2_PORT:-50053}"

echo "Starting Cluster GA Search..."
echo "Orchestrator: localhost:$ORCH_PORT"
echo "Node 1: localhost:$NODE1_PORT"
echo "Node 2: localhost:$NODE2_PORT"

# Start Orchestrator
# It will run its own GA search in the background and aggregate results from nodes
./target/debug/aarnn_rust --orchestrator --brain-id "$CLUSTER_BRAIN_ID" --auto-ga \
    --grpc-addr "127.0.0.1:$ORCH_PORT" --advertise-addr "127.0.0.1:$ORCH_PORT" --quiet --ui \
    > orchestrator.log 2>&1 &
PIDS+=("$!")

sleep 3

# Start Node 1
./target/debug/aarnn_rust --node --node-id node_1 --brain-id "$CLUSTER_BRAIN_ID" --auto-ga \
    --orchestrator-addr "http://127.0.0.1:$ORCH_PORT" --grpc-addr "127.0.0.1:$NODE1_PORT" \
    --advertise-addr "127.0.0.1:$NODE1_PORT" --quiet > node_1.log 2>&1 &
PIDS+=("$!")

# Start Node 2
./target/debug/aarnn_rust --node --node-id node_2 --brain-id "$CLUSTER_BRAIN_ID" --auto-ga \
    --orchestrator-addr "http://127.0.0.1:$ORCH_PORT" --grpc-addr "127.0.0.1:$NODE2_PORT" \
    --advertise-addr "127.0.0.1:$NODE2_PORT" --quiet > node_2.log 2>&1 &
PIDS+=("$!")

echo "Cluster started. Monitoring orchestrator.log for GA progress..."
echo "Press Ctrl+C to stop the cluster."

# Follow the orchestrator log to show progress to the user
tail -f orchestrator.log
