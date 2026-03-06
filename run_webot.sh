#!/bin/bash

# Script to start two example networks:
# 1. A standalone network running in a single process.
# 2. A distributed network (orchestrator + node) with autodiscovery.

# Cleanup function to kill all background processes on exit
cleanup() {
    echo "Shutting down networks..."
    kill $(jobs -p) 2>/dev/null
    exit
}

trap cleanup SIGINT SIGTERM EXIT

echo "Building project..."
cargo build --release --all-features

if [ $? -ne 0 ]; then
    echo "Build failed. Exiting."
    exit 1
fi

echo "Starting Distributed Orchestrator (Brain ID: cluster_master) with UI Dashboard..."
# Launching with --ui so the dashboard is visible onscreen
./target/release/aarnn_rust --orchestrator --brain-id cluster_master --grpc-addr 0.0.0.0:50051 --ui --ipc > orchestrator.log 2>&1 &

# Wait a bit for orchestrator to start broadcasting
sleep 2

echo "Starting Distributed Node (Brain ID: node_1) with Autodiscovery..."
# Node will autodiscover the orchestrator via UDP broadcast on port 50050
./target/release/aarnn_rust --node --brain-id vision --grpc-addr 0.0.0.0:50052 --ipc > vision.log 2>&1 &
sleep 2
./target/release/aarnn_rust --node --brain-id motor --grpc-addr 0.0.0.0:50053 --ipc > motor.log 2>&1 &

echo "----------------------------------------------------------------"
echo "Both networks are now running!"
echo "Network 2 Node 1 (Distributed) vision: see vision.log"
echo "Network 2 Node 2 (Distributed): see motor.log"
echo "The Orchestrator UI with Dashboard is now active onscreen."
echo "Check the 'Cluster Dashboard' section in the UI (right panel)."
echo "Press Ctrl+C to stop both networks."
echo "----------------------------------------------------------------"

# Keep the script running to maintain background jobs
wait
