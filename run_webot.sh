#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

PIDS=()
CLEANED_UP=0

cleanup() {
    if [ "$CLEANED_UP" -eq 1 ]; then
        return
    fi
    CLEANED_UP=1
    if [ "${#PIDS[@]}" -eq 0 ]; then
        return
    fi
    echo "Shutting down Webots runtime..."
    for pid in "${PIDS[@]}"; do
        if [ -n "${pid:-}" ]; then
            kill -TERM "$pid" 2>/dev/null || true
        fi
    done

    # Bound shutdown latency: wait briefly for graceful exit, then force-kill.
    local deadline=$((SECONDS + 3))
    local remaining=()
    while [ "$SECONDS" -lt "$deadline" ]; do
        remaining=()
        for pid in "${PIDS[@]}"; do
            if [ -n "${pid:-}" ] && kill -0 "$pid" 2>/dev/null; then
                remaining+=("$pid")
            fi
        done
        if [ "${#remaining[@]}" -eq 0 ]; then
            break
        fi
        sleep 0.1
    done

    for pid in "${remaining[@]}"; do
        kill -KILL "$pid" 2>/dev/null || true
    done

    wait "${PIDS[@]}" 2>/dev/null || true
}

trap 'exit 0' SIGINT SIGTERM
trap cleanup EXIT

usage() {
    cat <<'USAGE'
Usage: ./run_webot.sh [options]

Options:
  --runtime <cluster|uds>  Runtime backend (default: cluster).
                           cluster: orchestrator + per-brain nodes (--ui --ipc).
                           uds:     per-brain nn_uds_server instances.
  --no-build               Skip cargo build.
  --no-diag                Skip UDS diagnostics after launch.
  --world <path>           World file to parse controllerArgs.
  --brains <csv>           Comma-separated brain IDs (overrides NM_BRAINS/world args).
  --sensory <n>            Fallback sensory neuron count before handshake (default: 25).
  --output <n>             Fallback output neuron count before handshake (default: 11).
  --threshold <f>          Spike threshold for IPC/UDS servers (default: 0.5).
  --no-orchestrator-ui     In cluster runtime, start orchestrator without UI window.
  --no-node-ui             In cluster runtime, start nodes without UI (breaks IPC server bind).
  --no-webots              Do not launch Webots; run NN backend only.
  --webots-bin <path>      Webots executable path (default: auto-detect).
  --webots-mode <mode>     Webots mode: pause|realtime|fast (default: realtime).
  --webots-headless        Launch Webots with --no-rendering --minimize.
  --skip-controller-build  Skip preflight build/check of nao_nn_controller_uds.
  --connect-timeout <sec>  Timeout waiting for controller brain connections (default: 60).
  --help                   Show this help.

Environment overrides:
  RUNTIME, NM_BRAINS, NM_INTERCONNECT, NM_AER_S_BASE, NM_AER_O_BASE,
  NM_IPC_THRESHOLD, NM_DEFAULT_SENSORY, NM_DEFAULT_OUTPUT, WORLD_FILE, LOG_DIR,
  START_WEBOTS, WEBOTS_BIN, WEBOTS_MODE, WEBOTS_HEADLESS, WEBOTS_CONNECT_TIMEOUT,
  SKIP_CONTROLLER_BUILD.
USAGE
}

RUNTIME="${RUNTIME:-cluster}"
BUILD=1
RUN_DIAG=1
ORCHESTRATOR_UI=1
NODE_UI=1
WORLD_FILE="${WORLD_FILE:-$ROOT_DIR/webots_world/worlds/neuroworld.wbt}"
LOG_DIR="${LOG_DIR:-$ROOT_DIR/logs}"
DEFAULT_S="${NM_DEFAULT_SENSORY:-25}"
DEFAULT_O="${NM_DEFAULT_OUTPUT:-11}"
THRESHOLD="${NM_IPC_THRESHOLD:-0.5}"
AER_S_BASE="${NM_AER_S_BASE:-4096}"
AER_O_BASE="${NM_AER_O_BASE:-16384}"
BRAIN_CSV="${NM_BRAINS:-}"
INTERCONNECT="${NM_INTERCONNECT:-}"
START_WEBOTS="${START_WEBOTS:-1}"
WEBOTS_BIN="${WEBOTS_BIN:-}"
WEBOTS_MODE="${WEBOTS_MODE:-realtime}"
WEBOTS_HEADLESS="${WEBOTS_HEADLESS:-0}"
WEBOTS_CONNECT_TIMEOUT="${WEBOTS_CONNECT_TIMEOUT:-60}"
SKIP_CONTROLLER_BUILD="${SKIP_CONTROLLER_BUILD:-0}"
WEBOTS_PID=""
WEBOTS_LOG=""

normalize_bool() {
    local name="$1"
    local val="$2"
    case "$val" in
        1|true|TRUE|yes|YES|on|ON) echo "1" ;;
        0|false|FALSE|no|NO|off|OFF) echo "0" ;;
        *)
            echo "Invalid boolean for $name: '$val' (use 0/1, true/false, yes/no)"
            return 1
            ;;
    esac
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --runtime)
            shift
            RUNTIME="${1:-$RUNTIME}"
            ;;
        --no-build)
            BUILD=0
            ;;
        --no-diag)
            RUN_DIAG=0
            ;;
        --world)
            shift
            WORLD_FILE="${1:-}"
            ;;
        --brains)
            shift
            BRAIN_CSV="${1:-}"
            ;;
        --sensory)
            shift
            DEFAULT_S="${1:-$DEFAULT_S}"
            ;;
        --output)
            shift
            DEFAULT_O="${1:-$DEFAULT_O}"
            ;;
        --threshold)
            shift
            THRESHOLD="${1:-$THRESHOLD}"
            ;;
        --no-orchestrator-ui)
            ORCHESTRATOR_UI=0
            ;;
        --no-node-ui)
            NODE_UI=0
            ;;
        --no-webots)
            START_WEBOTS=0
            ;;
        --webots-bin)
            shift
            WEBOTS_BIN="${1:-}"
            ;;
        --webots-mode)
            shift
            WEBOTS_MODE="${1:-$WEBOTS_MODE}"
            ;;
        --webots-headless)
            WEBOTS_HEADLESS=1
            ;;
        --skip-controller-build)
            SKIP_CONTROLLER_BUILD=1
            ;;
        --connect-timeout)
            shift
            WEBOTS_CONNECT_TIMEOUT="${1:-$WEBOTS_CONNECT_TIMEOUT}"
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            usage
            exit 1
            ;;
    esac
    shift
done

if [ "$RUNTIME" != "cluster" ] && [ "$RUNTIME" != "uds" ]; then
    echo "Invalid --runtime '$RUNTIME' (must be cluster or uds)."
    exit 1
fi

START_WEBOTS="$(normalize_bool START_WEBOTS "$START_WEBOTS")" || exit 1
WEBOTS_HEADLESS="$(normalize_bool WEBOTS_HEADLESS "$WEBOTS_HEADLESS")" || exit 1
SKIP_CONTROLLER_BUILD="$(normalize_bool SKIP_CONTROLLER_BUILD "$SKIP_CONTROLLER_BUILD")" || exit 1

if [ "$WEBOTS_MODE" != "pause" ] && [ "$WEBOTS_MODE" != "realtime" ] && [ "$WEBOTS_MODE" != "fast" ]; then
    echo "Invalid --webots-mode '$WEBOTS_MODE' (must be pause, realtime, or fast)."
    exit 1
fi

if ! [[ "$WEBOTS_CONNECT_TIMEOUT" =~ ^[0-9]+$ ]]; then
    echo "Invalid --connect-timeout '$WEBOTS_CONNECT_TIMEOUT' (must be a non-negative integer)."
    exit 1
fi

declare -A SENSOR_REGEX=()
declare -A ACTUATOR_REGEX=()
declare -A SOCKET_PATHS=()
declare -A NODE_PORTS=()
declare -A USED_PORTS=()

reserve_port() { USED_PORTS["$1"]=1; }

is_port_free() {
    local port="$1"
    if command -v ss >/dev/null 2>&1; then
        if ss -H -ltn | awk '{print $4}' | awk -F: '{print $NF}' | grep -qx "$port"; then
            return 1
        fi
        if ss -H -lun | awk '{print $4}' | awk -F: '{print $NF}' | grep -qx "$port"; then
            return 1
        fi
    fi
    return 0
}

find_free_port() {
    local start="${1:-50051}"
    local p="$start"
    while [ "$p" -le 65535 ]; do
        if is_port_free "$p" && [ -z "${USED_PORTS[$p]+x}" ]; then
            echo "$p"
            return 0
        fi
        p=$((p + 1))
    done
    return 1
}

parse_world_controller_args() {
    local world_path="$1"
    if [ ! -f "$world_path" ]; then
        return
    fi
    while IFS= read -r kv; do
        [ -z "$kv" ] && continue
        local key="${kv%%=*}"
        local value="${kv#*=}"
        case "$key" in
            NM_BRAINS)
                if [ -z "$BRAIN_CSV" ]; then
                    BRAIN_CSV="$value"
                fi
                ;;
            NM_INTERCONNECT)
                if [ -z "$INTERCONNECT" ]; then
                    INTERCONNECT="$value"
                fi
                ;;
            NM_SENSORS_*)
                SENSOR_REGEX["${key#NM_SENSORS_}"]="$value"
                ;;
            NM_ACTUATORS_*)
                ACTUATOR_REGEX["${key#NM_ACTUATORS_}"]="$value"
                ;;
            *)
                ;;
        esac
    done < <(grep -Eo '"NM_[^"]+"' "$world_path" | tr -d '"')
}

socket_for_brain() {
    local brain="$1"
    if [ "$brain" = "default" ]; then
        printf "%s/aarnn_rust.nn" "$HOME"
    else
        printf "%s/aarnn_rust.%s.nn" "$HOME" "$brain"
    fi
}

interconnect_counts_for_brain() {
    local brain="$1"
    local incoming=0
    local outgoing=0
    local link_re='^([^[:space:]]+)->([^[:space:]]+):([0-9]+)$'
    local IFS=','
    read -r -a links <<< "$INTERCONNECT"
    for link in "${links[@]}"; do
        [ -z "$link" ] && continue
        if [[ "$link" =~ $link_re ]]; then
            local src="${BASH_REMATCH[1]}"
            local dst="${BASH_REMATCH[2]}"
            local count="${BASH_REMATCH[3]}"
            if [ "$dst" = "$brain" ]; then
                incoming=$((incoming + count))
            fi
            if [ "$src" = "$brain" ]; then
                outgoing=$((outgoing + count))
            fi
        fi
    done
    printf "%s,%s" "$incoming" "$outgoing"
}

wait_for_socket() {
    local path="$1"
    for _ in $(seq 1 120); do
        if [ -S "$path" ]; then
            return 0
        fi
        sleep 0.1
    done
    return 1
}

run_diag_for_brain() {
    local brain="$1"
    local socket_path="$2"
    local diag_log="$LOG_DIR/webots_${brain}.diag.log"

    if ! command -v python3 >/dev/null 2>&1; then
        echo "[diag:$brain] python3 not found; diagnostics unavailable."
        return 1
    fi

    : >"$diag_log"
    python3 "$ROOT_DIR/tools/uds_diag.py" --socket "$socket_path" --format float >>"$diag_log" 2>&1 || true

    if ! grep -Eq "^(Outputs:|Output spikes \\(AER\\):)" "$diag_log" && [ "$RUNTIME" = "cluster" ]; then
        # Cluster nodes often start with placeholder S/O before the first controller handshake.
        python3 "$ROOT_DIR/tools/uds_diag.py" \
            --socket "$socket_path" \
            --sensory "$DEFAULT_S" \
            --output "$DEFAULT_O" \
            --handshake \
            --format float >>"$diag_log" 2>&1 || true
        python3 "$ROOT_DIR/tools/uds_diag.py" --socket "$socket_path" --format float >>"$diag_log" 2>&1 || true
    fi

    if grep -Eq "^(Outputs:|Output spikes \\(AER\\):)" "$diag_log"; then
        local detected
        detected="$(grep -Eo 'Auto-detected S=[0-9]+, O=[0-9]+' "$diag_log" | tail -n 1 || true)"
        if [ -n "$detected" ]; then
            echo "[diag:$brain] $detected"
        else
            echo "[diag:$brain] UDS round-trip succeeded."
        fi
        return 0
    fi

    echo "[diag:$brain] failed. Inspect $diag_log"
    tail -n 6 "$diag_log" | sed 's/^/  /'
    return 1
}

resolve_webots_bin() {
    if [ -n "$WEBOTS_BIN" ]; then
        if [ ! -x "$WEBOTS_BIN" ]; then
            echo "Configured Webots binary is not executable: $WEBOTS_BIN"
            return 1
        fi
        return 0
    fi
    if command -v webots >/dev/null 2>&1; then
        WEBOTS_BIN="$(command -v webots)"
        return 0
    fi
    if [ -n "${WEBOTS_HOME:-}" ] && [ -x "${WEBOTS_HOME}/webots" ]; then
        WEBOTS_BIN="${WEBOTS_HOME}/webots"
        return 0
    fi
    if [ -x "/usr/local/webots/webots" ]; then
        WEBOTS_BIN="/usr/local/webots/webots"
        return 0
    fi
    return 1
}

start_webots_world() {
    if [ ! -f "$WORLD_FILE" ]; then
        echo "Webots world file not found: $WORLD_FILE"
        return 1
    fi
    if ! resolve_webots_bin; then
        echo "Unable to locate Webots executable. Set --webots-bin or WEBOTS_BIN."
        return 1
    fi

    WEBOTS_LOG="$LOG_DIR/webots_runtime.log"
    local webots_cmd=(
        "$WEBOTS_BIN"
        --batch
        --stdout
        --stderr
        "--mode=$WEBOTS_MODE"
    )
    if [ "$WEBOTS_HEADLESS" -eq 1 ]; then
        webots_cmd+=(--no-rendering --minimize)
    fi
    webots_cmd+=("$WORLD_FILE")

    echo "Starting Webots:"
    echo "  bin: $WEBOTS_BIN"
    echo "  world: $WORLD_FILE"
    echo "  mode: $WEBOTS_MODE"
    echo "  headless rendering: $WEBOTS_HEADLESS"

    "${webots_cmd[@]}" >"$WEBOTS_LOG" 2>&1 &
    WEBOTS_PID="$!"
    PIDS+=("$WEBOTS_PID")

    sleep 2
    if ! kill -0 "$WEBOTS_PID" 2>/dev/null; then
        echo "Webots exited during startup. See $WEBOTS_LOG"
        tail -n 80 "$WEBOTS_LOG" || true
        return 1
    fi

    echo "Webots process started:"
    echo "  pid: $WEBOTS_PID"
    echo "  log: $WEBOTS_LOG"
    return 0
}

wait_for_webots_connections() {
    local timeout_s="$1"
    local deadline=$((SECONDS + timeout_s))
    local missing=()
    while [ "$SECONDS" -lt "$deadline" ]; do
        if [ -n "$WEBOTS_PID" ] && ! kill -0 "$WEBOTS_PID" 2>/dev/null; then
            echo "Webots exited before controller connections were established."
            tail -n 120 "$WEBOTS_LOG" || true
            return 1
        fi

        missing=()
        for brain in "${BRAINS[@]}"; do
            if ! grep -Fq "[nao_nn_controller_uds] Brain '$brain': Connected." "$WEBOTS_LOG"; then
                missing+=("$brain")
            fi
        done

        if [ "${#missing[@]}" -eq 0 ]; then
            echo "Webots controller connectivity confirmed for brains: $BRAIN_CSV"
            return 0
        fi
        sleep 1
    done

    local missing_csv
    missing_csv="$(IFS=','; echo "${missing[*]}")"
    echo "Timed out waiting for Webots controller connections. Missing brains: ${missing_csv:-unknown}"
    local refused_count=0
    refused_count="$(grep -Fc "Handshake failed (Connection refused)." "$WEBOTS_LOG" || true)"
    if [ "$refused_count" -gt 0 ]; then
        echo "Controller reported handshake refused $refused_count time(s)."
    fi
    echo "Recent Webots log output:"
    tail -n 120 "$WEBOTS_LOG" | sed 's/^/  /'
    return 1
}

ensure_webots_controller_binary() {
    if [ "$SKIP_CONTROLLER_BUILD" -eq 1 ]; then
        return 0
    fi

    local controller_dir="$ROOT_DIR/webots_world/controllers/nao_nn_controller_uds"
    local source_file="$controller_dir/nao_nn_controller_uds.cpp"
    local binary_file="$controller_dir/nao_nn_controller_uds"
    local build_log="$LOG_DIR/webots_controller_build.log"
    local rebuild_needed=0

    if [ ! -d "$controller_dir" ] || [ ! -f "$source_file" ]; then
        echo "Webots controller source not found at $controller_dir"
        return 1
    fi

    if [ ! -x "$binary_file" ] || [ "$source_file" -nt "$binary_file" ]; then
        rebuild_needed=1
    elif command -v strings >/dev/null 2>&1 && strings "$binary_file" | grep -q "neuromorphic_demo\\."; then
        # Legacy controller binaries can point to the wrong socket prefix.
        rebuild_needed=1
    fi

    if [ "$rebuild_needed" -eq 0 ]; then
        return 0
    fi

    if ! command -v make >/dev/null 2>&1; then
        echo "Controller rebuild required but 'make' was not found."
        return 1
    fi

    echo "Building Webots controller: nao_nn_controller_uds"
    if ! make -C "$controller_dir" >"$build_log" 2>&1; then
        echo "Failed to build Webots controller. See $build_log"
        tail -n 60 "$build_log" | sed 's/^/  /'
        return 1
    fi
    echo "Webots controller build complete."
    return 0
}

start_cluster_runtime() {
    local bin="$ROOT_DIR/target/release/aarnn_rust"
    if [ ! -x "$bin" ]; then
        echo "Missing executable: $bin"
        echo "Build with: cargo build --release --bin aarnn_rust --all-features"
        exit 1
    fi

    local orch_port
    orch_port="$(find_free_port 50051)" || {
        echo "Failed to allocate orchestrator gRPC port"
        exit 1
    }
    reserve_port "$orch_port"

    local orch_log="$LOG_DIR/webots_orchestrator.log"
    local orch_cmd=(
        "$bin"
        --orchestrator
        --brain-id cluster_master
        --grpc-addr "0.0.0.0:$orch_port"
    )
    if [ "$ORCHESTRATOR_UI" -eq 1 ]; then
        orch_cmd+=(--ui)
    fi
    "${orch_cmd[@]}" >"$orch_log" 2>&1 &
    PIDS+=("$!")

    sleep 2

    local node_port_start=50070
    for brain in "${BRAINS[@]}"; do
        local node_port
        node_port="$(find_free_port "$node_port_start")" || {
            echo "Failed to allocate port for brain '$brain'"
            exit 1
        }
        reserve_port "$node_port"
        NODE_PORTS["$brain"]="$node_port"
        node_port_start=$((node_port + 7))

        local socket_path
        socket_path="$(socket_for_brain "$brain")"
        SOCKET_PATHS["$brain"]="$socket_path"
        rm -f "$socket_path"

        local log_file="$LOG_DIR/webots_${brain}.log"
        local node_cmd=(
            "$bin"
            --node
            --brain-id "$brain"
            --grpc-addr "0.0.0.0:$node_port"
            --orchestrator-addr "http://127.0.0.1:$orch_port"
            --ipc
        )
        if [ "$NODE_UI" -eq 1 ]; then
            node_cmd+=(--ui)
        fi

        "${node_cmd[@]}" >"$log_file" 2>&1 &
        PIDS+=("$!")

        if ! wait_for_socket "$socket_path"; then
            echo "Failed to bind IPC socket for brain '$brain': $socket_path"
            echo "See log: $log_file"
            tail -n 40 "$log_file" || true
            exit 1
        fi

        local io_counts
        io_counts="$(interconnect_counts_for_brain "$brain")"
        local incoming_virtual="${io_counts%%,*}"
        local outgoing_virtual="${io_counts##*,}"
        local sensor_regex="${SENSOR_REGEX[$brain]:-.*}"
        local actuator_regex="${ACTUATOR_REGEX[$brain]:-.*}"

        echo "Brain '$brain' ready:"
        echo "  socket: $socket_path"
        echo "  node gRPC: $node_port"
        echo "  log: $log_file"
        echo "  sensor ownership regex: $sensor_regex"
        echo "  actuator ownership regex: $actuator_regex"
        echo "  virtual links in/out: $incoming_virtual/$outgoing_virtual"
    done

    echo "Orchestrator ready:"
    echo "  gRPC: $orch_port"
    echo "  log: $orch_log"
    if [ "$ORCHESTRATOR_UI" -eq 1 ]; then
        echo "  UI: enabled"
    fi
}

start_uds_runtime() {
    local bin="$ROOT_DIR/target/release/examples/nn_uds_server"
    if [ ! -x "$bin" ]; then
        echo "Missing executable: $bin"
        echo "Build with: cargo build --release --example nn_uds_server --features ui,robot_io"
        exit 1
    fi

    for brain in "${BRAINS[@]}"; do
        local socket_path
        socket_path="$(socket_for_brain "$brain")"
        SOCKET_PATHS["$brain"]="$socket_path"
        rm -f "$socket_path"

        local io_counts
        io_counts="$(interconnect_counts_for_brain "$brain")"
        local incoming_virtual="${io_counts%%,*}"
        local outgoing_virtual="${io_counts##*,}"
        local sensor_regex="${SENSOR_REGEX[$brain]:-.*}"
        local actuator_regex="${ACTUATOR_REGEX[$brain]:-.*}"
        local log_file="$LOG_DIR/webots_${brain}.log"

        "$bin" \
            --socket "$socket_path" \
            --sensory "$DEFAULT_S" \
            --output "$DEFAULT_O" \
            --threshold "$THRESHOLD" \
            --aer-sensory-base "$AER_S_BASE" \
            --aer-output-base "$AER_O_BASE" >"$log_file" 2>&1 &
        PIDS+=("$!")

        if ! wait_for_socket "$socket_path"; then
            echo "Failed to bind socket for brain '$brain': $socket_path"
            echo "See log: $log_file"
            exit 1
        fi

        echo "Brain '$brain' ready:"
        echo "  socket: $socket_path"
        echo "  log: $log_file"
        echo "  sensor ownership regex: $sensor_regex"
        echo "  actuator ownership regex: $actuator_regex"
        echo "  virtual links in/out: $incoming_virtual/$outgoing_virtual"
    done
}

parse_world_controller_args "$WORLD_FILE"

if [ -z "$BRAIN_CSV" ]; then
    BRAIN_CSV="vision,motor"
fi
IFS=',' read -r -a BRAINS <<< "$BRAIN_CSV"
if [ "${#BRAINS[@]}" -eq 0 ]; then
    echo "No brains configured; set --brains or NM_BRAINS."
    exit 1
fi

if [ "$RUNTIME" = "cluster" ] && [ "$NODE_UI" -eq 1 ] && [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
    echo "No display detected (DISPLAY/WAYLAND_DISPLAY unset)."
    echo "Falling back to --runtime uds for headless compatibility."
    RUNTIME="uds"
fi

if [ "$START_WEBOTS" -eq 1 ] && [ "$WEBOTS_HEADLESS" -eq 0 ] && [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
    echo "No display detected for Webots; enabling --webots-headless mode."
    WEBOTS_HEADLESS=1
fi

mkdir -p "$LOG_DIR"

echo "Webots bridge launch config:"
echo "  runtime: $RUNTIME"
echo "  world: $WORLD_FILE"
echo "  brains: $BRAIN_CSV"
echo "  interconnect: ${INTERCONNECT:-none}"
echo "  fallback S/O: $DEFAULT_S/$DEFAULT_O"
echo "  AER base S/O: $AER_S_BASE/$AER_O_BASE"
echo "  start webots: $START_WEBOTS"
if [ "$START_WEBOTS" -eq 1 ]; then
    echo "  webots mode: $WEBOTS_MODE"
    echo "  webots headless rendering: $WEBOTS_HEADLESS"
    echo "  skip controller build: $SKIP_CONTROLLER_BUILD"
    echo "  connect timeout (s): $WEBOTS_CONNECT_TIMEOUT"
fi
echo

if [ "$BUILD" -eq 1 ]; then
    if [ "$RUNTIME" = "cluster" ]; then
        echo "Building aarnn_rust binary..."
        cargo build --release --bin aarnn_rust --all-features
    else
        echo "Building nn_uds_server example..."
        cargo build --release --example nn_uds_server --features ui,robot_io
    fi
fi

if [ "$RUNTIME" = "cluster" ]; then
    start_cluster_runtime
else
    start_uds_runtime
fi

if [ "$RUN_DIAG" -eq 1 ]; then
    echo
    echo "Running UDS diagnostics..."
    diag_failures=0
    for brain in "${BRAINS[@]}"; do
        if ! run_diag_for_brain "$brain" "${SOCKET_PATHS[$brain]}"; then
            diag_failures=$((diag_failures + 1))
        fi
    done
    if [ "$diag_failures" -gt 0 ]; then
        echo "Diagnostics failed for $diag_failures brain(s)."
        exit 1
    fi
fi

if [ "$START_WEBOTS" -eq 1 ]; then
    echo
    if ! ensure_webots_controller_binary; then
        exit 1
    fi
    if ! start_webots_world; then
        exit 1
    fi
    echo "Waiting for Webots controller to connect to NN sockets..."
    if ! wait_for_webots_connections "$WEBOTS_CONNECT_TIMEOUT"; then
        exit 1
    fi
fi

echo
if [ "$RUNTIME" = "cluster" ]; then
    echo "Webots cluster runtime is running (orchestrator + IPC nodes)."
    echo "Use orchestrator UI for cluster controls; each brain node hosts IPC on its socket."
else
    echo "Webots UDS runtime is running (per-brain nn_uds_server)."
fi
echo "Handshake auto-remap is enabled for UDS servers and IPC mismatch hints are active."
echo "Press Ctrl+C to stop."

if [ "$START_WEBOTS" -eq 1 ] && [ -n "$WEBOTS_PID" ]; then
    wait "$WEBOTS_PID" || true
else
    wait
fi
