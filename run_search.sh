#!/bin/bash

# Kill any existing instances of the demo
pkill -f aarnn_rust || true

# Ensure the binary is built
cargo build --all-features

echo "Starting Cluster GA Search..."
echo "Orchestrator: localhost:50051"
echo "Node 1: localhost:50052"
echo "Node 2: localhost:50053"

# Start Orchestrator
# It will run its own GA search in the background and aggregate results from nodes
./target/debug/aarnn_rust --orchestrator --auto-ga --grpc-addr 127.0.0.1:50051 --quiet --ui > orchestrator.log 2>&1 &
ORCH_PID=$!

sleep 3

# Start Node 1
./target/debug/aarnn_rust --node --auto-ga --orchestrator-addr http://127.0.0.1:50051 --grpc-addr 127.0.0.1:50052 --quiet > node_1.log 2>&1 &
NODE1_PID=$!

# Start Node 2
./target/debug/aarnn_rust --node --auto-ga --orchestrator-addr http://127.0.0.1:50051 --grpc-addr 127.0.0.1:50053 --quiet > node_2.log 2>&1 &
NODE2_PID=$!

echo "Cluster started. Monitoring orchestrator.log for GA progress..."
echo "Press Ctrl+C to stop the cluster."

# Function to handle exit
cleanup() {
    echo "Stopping cluster..."
    kill $ORCH_PID $NODE1_PID $NODE2_PID
    exit
}

trap cleanup SIGINT

# Follow the orchestrator log to show progress to the user
tail -f orchestrator.log
