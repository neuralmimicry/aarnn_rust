#!/bin/bash
set -euo pipefail

# Resolve all relative paths from this checkout even when the launcher is
# invoked through an absolute path from another working directory. This keeps
# binaries, snapshots, logs and runtime state tied to the launcher that was
# selected by the operator.
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

export NM_MORPHO_ASYNC="${NM_MORPHO_ASYNC:-1}"

# Start one local distributed brain with its local orchestrator serving as the
# cluster master and workstation I/O ingress. Workers join that brain using
# separate stable node IDs; the dashboard probe verifies this distinction.

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

# Choose an isolated high port by default so workers from a previous example
# run that still retry 50051 cannot silently join this new local cell.
ORCH_PORT_START="${AARNN_ORCH_PORT_START:-$((30000 + RANDOM % 20000))}"
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

# --all-features includes management_v1. The shared helper keeps this
# loopback launcher on the authenticated mTLS path used by the complete build.
if ! command -v python3 >/dev/null 2>&1; then
    echo "python3 is required to prepare the local management environment" >&2
    exit 1
fi
eval "$(python3 "$SCRIPT_DIR/scripts/local_management_env.py" \
    --runtime-root "$EXAMPLE_RUNTIME_ROOT" --shell)"

# Configure a sensory layer before any workers load the brain. Audio playback
# remains opt-in; this lets the user choose a file later from the dashboard
# without starting with a network that has nowhere to route its samples.
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
export AARNN_AUDIO_SENSORY_NEURONS

if [ -n "${AARNN_AUDIO_FILE:-}" ]; then
    if [ ! -f "$AARNN_AUDIO_FILE" ] || [ ! -r "$AARNN_AUDIO_FILE" ]; then
        echo "AARNN_AUDIO_FILE is not a readable regular file: $AARNN_AUDIO_FILE" >&2
        exit 1
    fi
    export AARNN_AUDIO_FILE
    echo "Native Rust UI audio source: $AARNN_AUDIO_FILE (sensory neurons: $AARNN_AUDIO_SENSORY_NEURONS)"
fi

# A UI file picker is commonly used after startup, so create the run-local
# sensory contract regardless of whether playback was requested on the
# command line. The helper preserves any positive width already configured in
# either input document and never edits the checked-in files.
AUDIO_IO_CONFIG_PATH="$EXAMPLE_RUNTIME_ROOT/audio-input-config.json"
python3 "$SCRIPT_DIR/scripts/qa/prepare_audio_io_contract.py" \
    --config "$CONFIG_PATH" \
    --network "$NETWORK_PATH" \
    --output "$AUDIO_IO_CONFIG_PATH" \
    --sensory-neurons "$AARNN_AUDIO_SENSORY_NEURONS"
CONFIG_PATH="$AUDIO_IO_CONFIG_PATH"
echo "Prepared managed sensory I/O contract: $CONFIG_PATH"

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
    # Build both entry points in one complete package feature graph. Building
    # the binaries separately with different feature sets makes Cargo rebuild
    # the shared library before the dashboard can start.
    cargo build --release --locked --all-features \
        --bin aarnn_rust --bin web_ui
fi

# The live launcher below remains a compatibility smoke for the current
# orchestrator. Run the new deterministic hierarchy gate explicitly so this
# laptop example also cross-checks communication grouping, physical-area
# parents, layer children and capacity-driven sub-shards. It is opt-in to keep
# ordinary example startup independent of Cargo's test profile.
VERIFY_HIERARCHICAL_SHARDING="${AARNN_VERIFY_HIERARCHICAL_SHARDING:-0}"
if [ "$VERIFY_HIERARCHICAL_SHARDING" = "1" ]; then
    cargo test --locked --all-features \
        --test hierarchical_sharding -- --nocapture
    echo "Hierarchical network/area/layer/sub-shard latency verification passed."
fi

#echo "Starting Standalone Network (Brain ID: standalone)..."
# Using --continuous to keep it running in background
#./target/release/aarnn_rust --brain-id standalone --continuous > standalone.log 2>&1 &
#PIDS=("$!")

export NMD_TFLITE_ALLOW_LARGE=1

EXECUTION_ARGS=(
    --execution-mode "distributed,sharded"
    --execution-scope cluster
    --execution-desired-shards 2
)

CLUSTER_BRAIN_ID="cluster_master"
NODE1_ID="node_1"
NODE2_ID="node_2"

# Keep the orchestrator's advertised autostart state and each worker's local
# managed Runner state consistent. A per-command assignment on only the
# orchestrator leaves sensory admission paused on the selected worker.
export NM_DISTRIBUTED_AUTOSTART=1

echo "Starting local cluster master and I/O ingress (Brain ID: $CLUSTER_BRAIN_ID)..."
NM_DISTRIBUTE_STARTUP_SNAPSHOT=1 \
"$BIN_DIR/aarnn_rust" --orchestrator --brain-id "$CLUSTER_BRAIN_ID" \
    --grpc-addr "0.0.0.0:$ORCH_PORT" --advertise-addr "127.0.0.1:$ORCH_PORT" \
    "${CONFIG_ARG[@]}" "${NETWORK_ARG[@]}" "${EXECUTION_ARGS[@]}" \
    "${NATIVE_UI_ARGS[@]}" > orchestrator.log 2>&1 &
PIDS=("$!")

# Wait a bit for orchestrator to start broadcasting
sleep 2
require_process "${PIDS[0]}" "Orchestrator"

echo "Starting worker nodes $NODE1_ID and $NODE2_ID for brain $CLUSTER_BRAIN_ID at http://127.0.0.1:$ORCH_PORT ..."
# These workers have no matching local copy of the run-local, I/O-aligned
# startup snapshot. Do not let the default repository config start a placeholder
# network before the orchestrator's LoadNetwork command arrives; the loaded
# cluster snapshot is the authority for topology and sensory width.
NM_PRELOAD_NODE_NETWORK=0 \
"$BIN_DIR/aarnn_rust" --node --node-id "$NODE1_ID" --brain-id "$CLUSTER_BRAIN_ID" \
    --grpc-addr "0.0.0.0:$NODE1_PORT" --advertise-addr "127.0.0.1:$NODE1_PORT" \
    --orchestrator-addr "http://127.0.0.1:$ORCH_PORT" "${EXECUTION_ARGS[@]}" > node_1.log 2>&1 &
NODE1_PID="$!"
PIDS+=("$!")
sleep 1
require_process "${PIDS[1]}" "Node node_1"
NM_PRELOAD_NODE_NETWORK=0 \
"$BIN_DIR/aarnn_rust" --node --node-id "$NODE2_ID" --brain-id "$CLUSTER_BRAIN_ID" \
    --grpc-addr "0.0.0.0:$NODE2_PORT" --advertise-addr "127.0.0.1:$NODE2_PORT" \
    --orchestrator-addr "http://127.0.0.1:$ORCH_PORT" "${EXECUTION_ARGS[@]}" > node_2.log 2>&1 &
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

# Process creation is not proof that a worker joined or hosts the intended
# brain. Check the same status projection as the dashboard, including stable
# NodeIds, active brain membership, and the absence of worker-ID brain records.
if ! CLUSTER_WORKERS="$(python3 "$SCRIPT_DIR/scripts/qa/wait_for_local_cluster.py" \
    --base-url "$WEB_UI_URL" \
    --orchestrator "http://127.0.0.1:$ORCH_PORT" \
    --brain-id "$CLUSTER_BRAIN_ID" \
    --worker "$NODE1_ID=127.0.0.1:$NODE1_PORT" --pid "$NODE1_ID=$NODE1_PID" \
    --worker "$NODE2_ID=127.0.0.1:$NODE2_PORT" --pid "$NODE2_ID=$NODE2_PID" \
    --timeout "${AARNN_CLUSTER_READY_TIMEOUT_S:-45}")"; then
    echo "Distributed cluster did not become ready; recent service logs follow:" >&2
    tail -n 60 orchestrator.log node_1.log node_2.log webui.log >&2
    exit 1
fi
echo "Cluster worker readiness verified: $CLUSTER_WORKERS"

if [ "$VERIFY_HIERARCHICAL_SHARDING" = "1" ]; then
    python3 "$SCRIPT_DIR/scripts/qa/verify_hierarchical_sharding.py" \
        --base-url "$WEB_UI_URL" \
        --orchestrator "127.0.0.1:$ORCH_PORT" \
        --timeout "${AARNN_VERIFY_TIMEOUT_S:-50}"
    echo "Live area/layer/sub-shard placement telemetry verified in the web status path."
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
echo "Local cluster master / I/O ingress brain: $CLUSTER_BRAIN_ID"
echo "Joined workers: $CLUSTER_WORKERS"
echo "Orchestrator gRPC: http://127.0.0.1:$ORCH_PORT"
echo "Web dashboard URL (port $WEB_UI_PORT): $WEB_UI_URL"
echo "Native Rust UI: $NATIVE_UI_STATUS."
echo "Node logs: node_1.log and node_2.log"
echo "Check the 'Cluster Dashboard' section in either UI."
echo "Press Ctrl+C to stop the example network and dashboard."
echo "----------------------------------------------------------------"

# Keep the script running to maintain background jobs
wait
