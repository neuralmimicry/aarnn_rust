#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

PIDS=()
REMOTE_PROC_HOSTS=()
REMOTE_PROC_PIDS=()
REMOTE_PROC_TAGS=()
REMOTE_CONTAINER_HOSTS=()
REMOTE_CONTAINER_NAMES=()
REMOTE_SSH_ARGV=()
REMOTE_SSH_READY=0
CLEANED_UP=0
LOCAL_RUST_UI_LOG=""
WEBOTS_RECORD_WORLD_FILE=""
WEBOTS_RECORD_PROGRESS_PID=""

cleanup() {
    if [ "$CLEANED_UP" -eq 1 ]; then
        return
    fi
    CLEANED_UP=1
    if [ "${#PIDS[@]}" -gt 0 ]; then
        echo "Shutting down local Webots runtime..."
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
    fi

    if [ "${#REMOTE_PROC_PIDS[@]}" -gt 0 ] && [ "$REMOTE_SSH_READY" -eq 1 ]; then
        echo "Shutting down remote cluster processes..."
        local i
        for i in "${!REMOTE_PROC_PIDS[@]}"; do
            local host="${REMOTE_PROC_HOSTS[$i]}"
            local pid="${REMOTE_PROC_PIDS[$i]}"
            local tag="${REMOTE_PROC_TAGS[$i]}"
            if [ -z "${host:-}" ] || [ -z "${pid:-}" ]; then
                continue
            fi
            LC_ALL=C LANG=C "${REMOTE_SSH_ARGV[@]}" "${REMOTE_USER}@${host}" "kill -TERM $pid" >/dev/null 2>&1 || true
            LC_ALL=C LANG=C "${REMOTE_SSH_ARGV[@]}" "${REMOTE_USER}@${host}" "sleep 0.2; kill -KILL $pid" >/dev/null 2>&1 || true
            echo "  remote $tag on $host (pid $pid) stopped"
        done
    fi

    if [ "${#REMOTE_CONTAINER_NAMES[@]}" -gt 0 ] && [ "$REMOTE_SSH_READY" -eq 1 ]; then
        local i
        for i in "${!REMOTE_CONTAINER_NAMES[@]}"; do
            local host="${REMOTE_CONTAINER_HOSTS[$i]}"
            local name="${REMOTE_CONTAINER_NAMES[$i]}"
            if [ -z "${host:-}" ] || [ -z "${name:-}" ]; then
                continue
            fi
            remote_exec_script "$host" bash -s -- "$name" <<'EOS' >/dev/null 2>&1 || true
set -euo pipefail
NAME="$1"
if command -v podman >/dev/null 2>&1; then
    podman rm -f "$NAME" >/dev/null 2>&1 || true
fi
EOS
        done
    fi

    if [ -n "${WEBOTS_RECORD_WORLD_FILE:-}" ]; then
        rm -f "$WEBOTS_RECORD_WORLD_FILE" "${WEBOTS_RECORD_WORLD_FILE%.*}.wbproj" 2>/dev/null || true
    fi
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
  --sensory <n>            Pre-handshake fallback sensory neuron count (default: 25).
  --output <n>             Pre-handshake fallback output neuron count (default: 11).
  --threshold <f>          Spike threshold for IPC/UDS servers (default: 0.5).
  --config <path>          NetworkConfig JSON to load in backend nodes/servers.
  --network <path>         Snapshot JSON to import in backend nodes/servers.
  --config-map <csv>       Per-brain NetworkConfig mapping, e.g. banc=/a.json,fafb=/b.json.
  --network-map <csv>      Per-brain snapshot mapping, e.g. banc=/a.json,fafb=/b.json.
  --orchestrator-port <n>  Fixed orchestrator gRPC port (default: auto-allocate).
  --no-orchestrator-ui     In cluster runtime, start orchestrator without UI window.
  --no-node-ui             In cluster runtime, start nodes without UI (breaks IPC server bind).
  --node-ui-hidden         Keep node UI processes hidden (IPC still binds; orchestrator UI visible).
  --single-orchestrator-ui In cluster runtime, run only one orchestrator process with --ui --ipc.
                           Requires exactly one brain (e.g., --brains default).
  --remote-compute         Run cluster compute on remote hosts over SSH, while Webots stays local.
  --remote-hosts <csv>     Remote compute hosts (default: 192.168.1.60,192.168.1.72).
  --remote-host-weights    Optional host weights csv, e.g. 192.168.1.60=1.4,192.168.1.72=1.0.
  --remote-user <user>     SSH user for remote hosts (default: current user).
  --remote-root <path>     Project root on remote hosts (default: local repo root path).
  --remote-orchestrator-host <host|auto>
                           Host for orchestrator process (default: auto, highest weighted reachable host).
  --remote-web-ui-host <host|auto|off>
                           Host for web_ui process in remote mode (default: auto; picks strongest host).
  --remote-web-ui-port <n> Remote web_ui listen port (default: 8080).
  --remote-web-ui-api-port <n>
                           Deprecated (ignored in web mode; kept for compatibility).
  --remote-ui-mode <web|rust>
                           Browser UI backend in remote mode (default: web).
                           rust mode is deprecated and automatically mapped to web.
  --local-rust-ui         Launch a local native rust_ui window in remote-compute mode.
  --no-local-rust-ui      Disable local native rust_ui launch in remote-compute mode.
  --no-remote-pre-clean Do not stop previous remote aarnn_rust/web_ui processes before launch.
  --remote-webots-host <host>
                           Informational local Webots host label (default: 192.168.1.70).
  --remote-sync-data <auto|always|never>
                           Remote dataset sync policy (default: auto).
                           auto: sync data/ once per host, then skip unless forced.
  --remote-rsync-compress <auto|on|off>
                           Compression policy for rsync fallback (default: auto).
                           auto disables compression for RFC1918 LAN hosts.
  --remote-ssh-opts <str>  Extra SSH options appended to remote launch commands.
  --no-webots              Do not launch Webots; run NN backend only.
  --webots-bin <path>      Webots executable path (default: auto-detect).
  --webots-mode <mode>     Webots mode: pause|realtime|fast (default: realtime).
  --webots-headless        Launch Webots minimized; uses --no-rendering unless recording.
  --webots-record          Record the Webots rendered view to an MP4 via Supervisor.
  --webots-record-file <path>
                           Output movie path (default: logs/webots_recording.mp4).
  --webots-record-duration-ms <ms>
                           Stop/finalize recording after this simulation duration.
  --webots-record-width <px>
                           Recording width in pixels (default: 1280).
  --webots-record-height <px>
                           Recording height in pixels (default: 720).
  --webots-record-quality <1-100>
                           Recording quality passed to Webots (default: 85).
  --webots-record-acceleration <n>
                           Movie playback acceleration passed to Webots (default: 1).
  --webots-record-quit-on-done
                           Quit Webots after a duration-limited recording is finalized.
  --webots-record-progress
                           Show a console progress bar for duration-limited recording.
  --no-webots-record-progress
                           Disable console recording progress.
  --skip-controller-build  Skip preflight build/check of nao_nn_controller_uds.
  --connect-timeout <sec>  Timeout waiting for controller brain connections (default: 60).
  --cluster-distribution-timeout <sec>
                           Timeout waiting for remote cluster distribution per network (default: 300).
  --help                   Show this help.

Environment overrides:
  RUNTIME, NM_BRAINS, NM_INTERCONNECT, NM_AER_S_BASE, NM_AER_O_BASE,
  NM_IPC_THRESHOLD, NM_DEFAULT_SENSORY, NM_DEFAULT_OUTPUT, WORLD_FILE, LOG_DIR,
  NM_UDS_RECV_TIMEOUT_MS, NM_IPC_TIMEOUT_GRACE_MS, NM_IPC_TIMEOUT_LOG_INTERVAL_MS,
  NM_IPC_UDS_CTRL_BUF_BYTES, NM_IPC_WINDOW_MIN, NM_IPC_WINDOW_INIT, NM_IPC_WINDOW_MAX,
  NM_IPC_SEND_BUDGET_MAX, NM_IPC_STRICT_LOCKSTEP, NM_IPC_QUEUE_MAX_FRAMES, NM_WEBOTS_STEP_SLEEP_MS,
  NM_IPC_FORCE_AER, NM_IPC_DISABLE_AER, NM_IPC_MAX_RAW_BYTES,
  NM_IPC_AER_THRESHOLD, NM_IPC_AER_MAX_EVENTS, NM_IPC_AER_MAX_PACKET_BYTES,
  START_WEBOTS, WEBOTS_BIN, WEBOTS_MODE, WEBOTS_HEADLESS, WEBOTS_CONNECT_TIMEOUT,
  WEBOTS_CLUSTER_DISTRIBUTION_TIMEOUT, NM_WEBOTS_RECORD, NM_WEBOTS_RECORD_FILE,
  NM_WEBOTS_RECORD_WIDTH, NM_WEBOTS_RECORD_HEIGHT, NM_WEBOTS_RECORD_DURATION_MS,
  NM_WEBOTS_RECORD_QUALITY, NM_WEBOTS_RECORD_ACCELERATION, NM_WEBOTS_RECORD_CODEC,
  NM_WEBOTS_RECORD_PROGRESS, NM_WEBOTS_RECORD_PROGRESS_INTERVAL_MS,
  SKIP_CONTROLLER_BUILD, NM_CONFIG_FILE, NM_NETWORK_FILE, NM_CONFIG_MAP, NM_NETWORK_MAP, NM_ORCHESTRATOR_PORT,
  NM_NODE_UI_HIDDEN, NM_REMOTE_COMPUTE, NM_REMOTE_HOSTS, NM_REMOTE_HOST_WEIGHTS,
  NM_REMOTE_USER, NM_REMOTE_ROOT, NM_REMOTE_ORCHESTRATOR_HOST, NM_REMOTE_WEB_UI_HOST,
  NM_REMOTE_WEB_UI_PORT, NM_REMOTE_WEB_UI_API_PORT, NM_REMOTE_UI_MODE,
  NM_LOCAL_RUST_UI, NM_REMOTE_WEBOTS_HOST, NM_REMOTE_SSH_OPTS, NM_REMOTE_LOG_DIR,
  NM_REMOTE_PRE_CLEAN, NM_REMOTE_SYNC_DATA, NM_REMOTE_RSYNC_COMPRESS,
  NM_DISTRIBUTE_STARTUP_SNAPSHOT, NM_DISTRIBUTED_AUTOSTART, NM_PRELOAD_NODE_NETWORK,
  NM_REMOTE_RUNTIME_FEATURES, NM_REALTIME_POLICY, NM_REALTIME_IPC, NM_REALTIME_DISABLE_GROWTH,
  NM_REALTIME_DISABLE_MORPHO, NM_REALTIME_DISABLE_METABOLIC,
  NM_REALTIME_DISABLE_PRUNING, NM_REALTIME_MORPHO_INTERVAL_MS,
  NM_REALTIME_METABOLIC_INTERVAL_MS, NM_REALTIME_MORPHO_MAX_SYNAPSES,
  NM_MORPHO_ASYNC.
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
WEBOTS_CLUSTER_DISTRIBUTION_TIMEOUT="${WEBOTS_CLUSTER_DISTRIBUTION_TIMEOUT:-300}"
WEBOTS_RECORD="${NM_WEBOTS_RECORD:-0}"
WEBOTS_RECORD_FILE="${NM_WEBOTS_RECORD_FILE:-}"
WEBOTS_RECORD_WIDTH="${NM_WEBOTS_RECORD_WIDTH:-1280}"
WEBOTS_RECORD_HEIGHT="${NM_WEBOTS_RECORD_HEIGHT:-720}"
WEBOTS_RECORD_DURATION_MS="${NM_WEBOTS_RECORD_DURATION_MS:-0}"
WEBOTS_RECORD_QUALITY="${NM_WEBOTS_RECORD_QUALITY:-85}"
WEBOTS_RECORD_ACCELERATION="${NM_WEBOTS_RECORD_ACCELERATION:-1}"
WEBOTS_RECORD_CODEC="${NM_WEBOTS_RECORD_CODEC:-0}"
WEBOTS_RECORD_CAPTION="${NM_WEBOTS_RECORD_CAPTION:-0}"
WEBOTS_RECORD_QUIT_ON_DONE="${NM_WEBOTS_RECORD_QUIT_ON_DONE:-}"
WEBOTS_RECORD_PROGRESS="${NM_WEBOTS_RECORD_PROGRESS:-auto}"
WEBOTS_RECORD_PROGRESS_INTERVAL_MS="${NM_WEBOTS_RECORD_PROGRESS_INTERVAL_MS:-500}"
SKIP_CONTROLLER_BUILD="${SKIP_CONTROLLER_BUILD:-0}"
UDS_RECV_TIMEOUT_MS="${NM_UDS_RECV_TIMEOUT_MS:-150}"
IPC_TIMEOUT_GRACE_MS="${NM_IPC_TIMEOUT_GRACE_MS:-1500}"
IPC_TIMEOUT_LOG_INTERVAL_MS="${NM_IPC_TIMEOUT_LOG_INTERVAL_MS:-5000}"
IPC_UDS_CTRL_BUF_BYTES="${NM_IPC_UDS_CTRL_BUF_BYTES:-524288}"
IPC_WINDOW_MIN="${NM_IPC_WINDOW_MIN:-1}"
IPC_WINDOW_INIT="${NM_IPC_WINDOW_INIT:-1}"
IPC_WINDOW_MAX="${NM_IPC_WINDOW_MAX:-1}"
IPC_SEND_BUDGET_MAX="${NM_IPC_SEND_BUDGET_MAX:-1}"
IPC_STRICT_LOCKSTEP="${NM_IPC_STRICT_LOCKSTEP:-1}"
WEBOTS_STEP_SLEEP_MS="${NM_WEBOTS_STEP_SLEEP_MS:-0}"
CONFIG_FILE="${NM_CONFIG_FILE:-}"
NETWORK_FILE="${NM_NETWORK_FILE:-}"
CONFIG_MAP_CSV="${NM_CONFIG_MAP:-}"
NETWORK_MAP_CSV="${NM_NETWORK_MAP:-}"
ORCHESTRATOR_PORT="${NM_ORCHESTRATOR_PORT:-}"
NODE_UI_HIDDEN="${NM_NODE_UI_HIDDEN:-0}"
SINGLE_ORCHESTRATOR_UI="${NM_SINGLE_ORCHESTRATOR_UI:-0}"
DISTRIBUTE_STARTUP_SNAPSHOT="${NM_DISTRIBUTE_STARTUP_SNAPSHOT:-1}"
DISTRIBUTED_AUTOSTART="${NM_DISTRIBUTED_AUTOSTART:-1}"
PRELOAD_NODE_NETWORK="${NM_PRELOAD_NODE_NETWORK:-1}"
REMOTE_COMPUTE="${NM_REMOTE_COMPUTE:-0}"
REMOTE_HOSTS="${NM_REMOTE_HOSTS:-192.168.1.60,192.168.1.72}"
REMOTE_HOST_WEIGHTS="${NM_REMOTE_HOST_WEIGHTS:-}"
REMOTE_USER="${NM_REMOTE_USER:-${USER:-}}"
REMOTE_ROOT_DIR="${NM_REMOTE_ROOT:-$ROOT_DIR}"
REMOTE_LOG_DIR="${NM_REMOTE_LOG_DIR:-$REMOTE_ROOT_DIR/logs}"
REMOTE_ORCHESTRATOR_HOST="${NM_REMOTE_ORCHESTRATOR_HOST:-auto}"
REMOTE_WEB_UI_HOST="${NM_REMOTE_WEB_UI_HOST:-auto}"
REMOTE_WEB_UI_PORT="${NM_REMOTE_WEB_UI_PORT:-8080}"
REMOTE_WEB_UI_API_PORT="${NM_REMOTE_WEB_UI_API_PORT:-}"
REMOTE_UI_MODE="${NM_REMOTE_UI_MODE:-web}"
LOCAL_RUST_UI="${NM_LOCAL_RUST_UI:-0}"
REMOTE_WEBOTS_HOST="${NM_REMOTE_WEBOTS_HOST:-192.168.1.70}"
REMOTE_SSH_OPTS="${NM_REMOTE_SSH_OPTS:-}"
REMOTE_QUIET="${NM_REMOTE_QUIET:-1}"
REMOTE_PRE_CLEAN="${NM_REMOTE_PRE_CLEAN:-1}"
REMOTE_SYNC_DATA="${NM_REMOTE_SYNC_DATA:-auto}"
REMOTE_RSYNC_COMPRESS="${NM_REMOTE_RSYNC_COMPRESS:-auto}"
REALTIME_POLICY="${NM_REALTIME_POLICY:-biomimicry}"
REALTIME_IPC="${NM_REALTIME_IPC:-auto}"
REALTIME_DISABLE_GROWTH="${NM_REALTIME_DISABLE_GROWTH:-auto}"
REALTIME_DISABLE_MORPHO="${NM_REALTIME_DISABLE_MORPHO:-auto}"
REALTIME_DISABLE_METABOLIC="${NM_REALTIME_DISABLE_METABOLIC:-auto}"
REALTIME_DISABLE_PRUNING="${NM_REALTIME_DISABLE_PRUNING:-auto}"
REALTIME_MORPHO_INTERVAL_MS="${NM_REALTIME_MORPHO_INTERVAL_MS:-}"
REALTIME_METABOLIC_INTERVAL_MS="${NM_REALTIME_METABOLIC_INTERVAL_MS:-}"
REALTIME_MORPHO_MAX_SYNAPSES="${NM_REALTIME_MORPHO_MAX_SYNAPSES:-}"
MORPHO_ASYNC="${NM_MORPHO_ASYNC:-auto}"
WEB_UI_RUNTIME_ROOT="${NM_WEB_UI_RUNTIME_ROOT:-$ROOT_DIR/data/runtime}"
WEB_UI_DEFAULT_RUNTIME_USER="${NM_WEB_UI_DEFAULT_RUNTIME_USER:-}"
WEBOTS_PID=""
WEBOTS_LOG=""

export NM_UDS_RECV_TIMEOUT_MS="$UDS_RECV_TIMEOUT_MS"
export NM_IPC_TIMEOUT_GRACE_MS="$IPC_TIMEOUT_GRACE_MS"
export NM_IPC_TIMEOUT_LOG_INTERVAL_MS="$IPC_TIMEOUT_LOG_INTERVAL_MS"
export NM_IPC_UDS_CTRL_BUF_BYTES="$IPC_UDS_CTRL_BUF_BYTES"
export NM_IPC_WINDOW_MIN="$IPC_WINDOW_MIN"
export NM_IPC_WINDOW_INIT="$IPC_WINDOW_INIT"
export NM_IPC_WINDOW_MAX="$IPC_WINDOW_MAX"
export NM_IPC_SEND_BUDGET_MAX="$IPC_SEND_BUDGET_MAX"
export NM_IPC_STRICT_LOCKSTEP="$IPC_STRICT_LOCKSTEP"
export NM_WEBOTS_STEP_SLEEP_MS="$WEBOTS_STEP_SLEEP_MS"
export NM_AER_S_BASE="$AER_S_BASE"
export NM_AER_O_BASE="$AER_O_BASE"

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

normalize_bool_auto() {
    local name="$1"
    local val="$2"
    case "$val" in
        auto|AUTO) echo "auto" ;;
        1|true|TRUE|yes|YES|on|ON) echo "1" ;;
        0|false|FALSE|no|NO|off|OFF) echo "0" ;;
        *)
            echo "Invalid $name: '$val' (use auto, 0/1, true/false, yes/no)"
            return 1
            ;;
    esac
}

rsync_supports_option() {
    local option="$1"
    rsync --help 2>/dev/null | grep -q -- "$option"
}

host_is_private_lan() {
    local host="$1"
    case "$host" in
        10.*|192.168.*|172.1[6-9].*|172.2[0-9].*|172.3[0-1].*)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

resolve_sync_compress_mode() {
    local host="$1"
    local mode="$REMOTE_RSYNC_COMPRESS"
    if [ "$mode" = "auto" ]; then
        if host_is_private_lan "$host"; then
            mode="off"
        else
            mode="on"
        fi
    fi
    printf "%s" "$mode"
}

rclone_supports_flag() {
    local flag="$1"
    rclone help flags 2>/dev/null | grep -q -- "$flag"
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
        --config)
            shift
            CONFIG_FILE="${1:-}"
            ;;
        --network)
            shift
            NETWORK_FILE="${1:-}"
            ;;
        --config-map)
            shift
            CONFIG_MAP_CSV="${1:-}"
            ;;
        --network-map)
            shift
            NETWORK_MAP_CSV="${1:-}"
            ;;
        --orchestrator-port)
            shift
            ORCHESTRATOR_PORT="${1:-}"
            ;;
        --no-orchestrator-ui)
            ORCHESTRATOR_UI=0
            ;;
        --no-node-ui)
            NODE_UI=0
            ;;
        --node-ui-hidden)
            NODE_UI_HIDDEN=1
            ;;
        --single-orchestrator-ui)
            SINGLE_ORCHESTRATOR_UI=1
            ;;
        --remote-compute)
            REMOTE_COMPUTE=1
            ;;
        --remote-hosts)
            shift
            REMOTE_HOSTS="${1:-$REMOTE_HOSTS}"
            ;;
        --remote-host-weights)
            shift
            REMOTE_HOST_WEIGHTS="${1:-$REMOTE_HOST_WEIGHTS}"
            ;;
        --remote-user)
            shift
            REMOTE_USER="${1:-$REMOTE_USER}"
            ;;
        --remote-root)
            shift
            REMOTE_ROOT_DIR="${1:-$REMOTE_ROOT_DIR}"
            ;;
        --remote-orchestrator-host)
            shift
            REMOTE_ORCHESTRATOR_HOST="${1:-$REMOTE_ORCHESTRATOR_HOST}"
            ;;
        --remote-web-ui-host)
            shift
            REMOTE_WEB_UI_HOST="${1:-$REMOTE_WEB_UI_HOST}"
            ;;
        --remote-web-ui-port)
            shift
            REMOTE_WEB_UI_PORT="${1:-$REMOTE_WEB_UI_PORT}"
            ;;
        --remote-web-ui-api-port)
            shift
            REMOTE_WEB_UI_API_PORT="${1:-$REMOTE_WEB_UI_API_PORT}"
            ;;
        --remote-ui-mode)
            shift
            REMOTE_UI_MODE="${1:-$REMOTE_UI_MODE}"
            ;;
        --local-rust-ui)
            LOCAL_RUST_UI=1
            ;;
        --no-local-rust-ui)
            LOCAL_RUST_UI=0
            ;;
        --remote-webots-host)
            shift
            REMOTE_WEBOTS_HOST="${1:-$REMOTE_WEBOTS_HOST}"
            ;;
        --remote-sync-data)
            shift
            REMOTE_SYNC_DATA="${1:-$REMOTE_SYNC_DATA}"
            ;;
        --remote-rsync-compress)
            shift
            REMOTE_RSYNC_COMPRESS="${1:-$REMOTE_RSYNC_COMPRESS}"
            ;;
        --no-remote-pre-clean)
            REMOTE_PRE_CLEAN=0
            ;;
        --remote-ssh-opts)
            shift
            REMOTE_SSH_OPTS="${1:-$REMOTE_SSH_OPTS}"
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
        --webots-record)
            WEBOTS_RECORD=1
            ;;
        --webots-record-file)
            shift
            WEBOTS_RECORD_FILE="${1:-$WEBOTS_RECORD_FILE}"
            WEBOTS_RECORD=1
            ;;
        --webots-record-duration-ms)
            shift
            WEBOTS_RECORD_DURATION_MS="${1:-$WEBOTS_RECORD_DURATION_MS}"
            WEBOTS_RECORD=1
            ;;
        --webots-record-width)
            shift
            WEBOTS_RECORD_WIDTH="${1:-$WEBOTS_RECORD_WIDTH}"
            WEBOTS_RECORD=1
            ;;
        --webots-record-height)
            shift
            WEBOTS_RECORD_HEIGHT="${1:-$WEBOTS_RECORD_HEIGHT}"
            WEBOTS_RECORD=1
            ;;
        --webots-record-quality)
            shift
            WEBOTS_RECORD_QUALITY="${1:-$WEBOTS_RECORD_QUALITY}"
            WEBOTS_RECORD=1
            ;;
        --webots-record-acceleration)
            shift
            WEBOTS_RECORD_ACCELERATION="${1:-$WEBOTS_RECORD_ACCELERATION}"
            WEBOTS_RECORD=1
            ;;
        --webots-record-quit-on-done)
            WEBOTS_RECORD_QUIT_ON_DONE=1
            WEBOTS_RECORD=1
            ;;
        --webots-record-progress)
            WEBOTS_RECORD_PROGRESS=1
            WEBOTS_RECORD=1
            ;;
        --no-webots-record-progress)
            WEBOTS_RECORD_PROGRESS=0
            ;;
        --skip-controller-build)
            SKIP_CONTROLLER_BUILD=1
            ;;
        --connect-timeout)
            shift
            WEBOTS_CONNECT_TIMEOUT="${1:-$WEBOTS_CONNECT_TIMEOUT}"
            ;;
        --cluster-distribution-timeout)
            shift
            WEBOTS_CLUSTER_DISTRIBUTION_TIMEOUT="${1:-$WEBOTS_CLUSTER_DISTRIBUTION_TIMEOUT}"
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
WEBOTS_RECORD="$(normalize_bool NM_WEBOTS_RECORD "$WEBOTS_RECORD")" || exit 1
WEBOTS_RECORD_CAPTION="$(normalize_bool NM_WEBOTS_RECORD_CAPTION "$WEBOTS_RECORD_CAPTION")" || exit 1
WEBOTS_RECORD_PROGRESS="$(normalize_bool_auto NM_WEBOTS_RECORD_PROGRESS "$WEBOTS_RECORD_PROGRESS")" || exit 1
if [ -n "$WEBOTS_RECORD_QUIT_ON_DONE" ]; then
    WEBOTS_RECORD_QUIT_ON_DONE="$(normalize_bool NM_WEBOTS_RECORD_QUIT_ON_DONE "$WEBOTS_RECORD_QUIT_ON_DONE")" || exit 1
fi
SKIP_CONTROLLER_BUILD="$(normalize_bool SKIP_CONTROLLER_BUILD "$SKIP_CONTROLLER_BUILD")" || exit 1
NODE_UI_HIDDEN="$(normalize_bool NODE_UI_HIDDEN "$NODE_UI_HIDDEN")" || exit 1
SINGLE_ORCHESTRATOR_UI="$(normalize_bool SINGLE_ORCHESTRATOR_UI "$SINGLE_ORCHESTRATOR_UI")" || exit 1
DISTRIBUTE_STARTUP_SNAPSHOT="$(normalize_bool NM_DISTRIBUTE_STARTUP_SNAPSHOT "$DISTRIBUTE_STARTUP_SNAPSHOT")" || exit 1
DISTRIBUTED_AUTOSTART="$(normalize_bool NM_DISTRIBUTED_AUTOSTART "$DISTRIBUTED_AUTOSTART")" || exit 1
REMOTE_COMPUTE="$(normalize_bool REMOTE_COMPUTE "$REMOTE_COMPUTE")" || exit 1
REMOTE_QUIET="$(normalize_bool REMOTE_QUIET "$REMOTE_QUIET")" || exit 1
REMOTE_PRE_CLEAN="$(normalize_bool REMOTE_PRE_CLEAN "$REMOTE_PRE_CLEAN")" || exit 1
LOCAL_RUST_UI="$(normalize_bool LOCAL_RUST_UI "$LOCAL_RUST_UI")" || exit 1

if [ "$WEBOTS_RECORD" -eq 1 ]; then
    if [ "$REMOTE_COMPUTE" -eq 0 ] && [ "$RUNTIME" = "cluster" ]; then
        echo "Webots recording enabled; switching local AARNN runtime to uds for headless CLI capture."
        RUNTIME="uds"
        ORCHESTRATOR_UI=0
        NODE_UI_HIDDEN=1
        SINGLE_ORCHESTRATOR_UI=0
        LOCAL_RUST_UI=0
    elif [ "$REMOTE_COMPUTE" -eq 1 ]; then
        echo "Webots recording enabled; disabling local AARNN UI surfaces for headless capture."
        ORCHESTRATOR_UI=0
        NODE_UI_HIDDEN=1
        SINGLE_ORCHESTRATOR_UI=0
        LOCAL_RUST_UI=0
    fi
fi

case "${REALTIME_POLICY,,}" in
    conservative|safe|legacy)
        REALTIME_POLICY="conservative"
        ;;
    biomimicry|bio|balanced)
        REALTIME_POLICY="biomimicry"
        ;;
    *)
        echo "Invalid NM_REALTIME_POLICY='$REALTIME_POLICY' (use conservative|biomimicry)."
        exit 1
        ;;
esac

if [ "$REALTIME_IPC" = "auto" ]; then
    if [ "$START_WEBOTS" -eq 1 ]; then
        REALTIME_IPC="1"
    else
        REALTIME_IPC="0"
    fi
fi
REALTIME_IPC="$(normalize_bool NM_REALTIME_IPC "$REALTIME_IPC")" || exit 1

if [ "$REALTIME_DISABLE_GROWTH" = "auto" ]; then
    if [ "$REALTIME_POLICY" = "biomimicry" ]; then
        REALTIME_DISABLE_GROWTH="0"
    else
        REALTIME_DISABLE_GROWTH="$REALTIME_IPC"
    fi
fi
REALTIME_DISABLE_GROWTH="$(normalize_bool NM_REALTIME_DISABLE_GROWTH "$REALTIME_DISABLE_GROWTH")" || exit 1

if [ "$REALTIME_DISABLE_MORPHO" = "auto" ]; then
    if [ "$REALTIME_POLICY" = "biomimicry" ]; then
        REALTIME_DISABLE_MORPHO="0"
    else
        REALTIME_DISABLE_MORPHO="$REALTIME_IPC"
    fi
fi
REALTIME_DISABLE_MORPHO="$(normalize_bool NM_REALTIME_DISABLE_MORPHO "$REALTIME_DISABLE_MORPHO")" || exit 1

if [ "$REALTIME_DISABLE_METABOLIC" = "auto" ]; then
    if [ "$REALTIME_POLICY" = "biomimicry" ]; then
        REALTIME_DISABLE_METABOLIC="0"
    else
        REALTIME_DISABLE_METABOLIC="$REALTIME_IPC"
    fi
fi
REALTIME_DISABLE_METABOLIC="$(normalize_bool NM_REALTIME_DISABLE_METABOLIC "$REALTIME_DISABLE_METABOLIC")" || exit 1

if [ "$REALTIME_DISABLE_PRUNING" = "auto" ]; then
    if [ "$REALTIME_POLICY" = "biomimicry" ]; then
        REALTIME_DISABLE_PRUNING="0"
    else
        REALTIME_DISABLE_PRUNING="$REALTIME_IPC"
    fi
fi
REALTIME_DISABLE_PRUNING="$(normalize_bool NM_REALTIME_DISABLE_PRUNING "$REALTIME_DISABLE_PRUNING")" || exit 1

if [ "$REALTIME_POLICY" = "biomimicry" ]; then
    if [ -z "$REALTIME_MORPHO_INTERVAL_MS" ]; then
        REALTIME_MORPHO_INTERVAL_MS="80"
    fi
    if [ -z "$REALTIME_METABOLIC_INTERVAL_MS" ]; then
        REALTIME_METABOLIC_INTERVAL_MS="120"
    fi
    if [ -z "$REALTIME_MORPHO_MAX_SYNAPSES" ]; then
        REALTIME_MORPHO_MAX_SYNAPSES="350000"
    fi
fi

export NM_REALTIME_POLICY="$REALTIME_POLICY"
export NM_REALTIME_IPC="$REALTIME_IPC"
export NM_REALTIME_DISABLE_GROWTH="$REALTIME_DISABLE_GROWTH"
export NM_REALTIME_DISABLE_MORPHO="$REALTIME_DISABLE_MORPHO"
export NM_REALTIME_DISABLE_METABOLIC="$REALTIME_DISABLE_METABOLIC"
export NM_REALTIME_DISABLE_PRUNING="$REALTIME_DISABLE_PRUNING"
if [ -n "$REALTIME_MORPHO_INTERVAL_MS" ]; then
    export NM_REALTIME_MORPHO_INTERVAL_MS="$REALTIME_MORPHO_INTERVAL_MS"
fi
if [ -n "$REALTIME_METABOLIC_INTERVAL_MS" ]; then
    export NM_REALTIME_METABOLIC_INTERVAL_MS="$REALTIME_METABOLIC_INTERVAL_MS"
fi
if [ -n "$REALTIME_MORPHO_MAX_SYNAPSES" ]; then
    export NM_REALTIME_MORPHO_MAX_SYNAPSES="$REALTIME_MORPHO_MAX_SYNAPSES"
fi
if [ "$MORPHO_ASYNC" = "auto" ]; then
    MORPHO_ASYNC="$REALTIME_IPC"
fi
MORPHO_ASYNC="$(normalize_bool NM_MORPHO_ASYNC "$MORPHO_ASYNC")" || exit 1
export NM_MORPHO_ASYNC="$MORPHO_ASYNC"

if [ "$WEBOTS_MODE" != "pause" ] && [ "$WEBOTS_MODE" != "realtime" ] && [ "$WEBOTS_MODE" != "fast" ]; then
    echo "Invalid --webots-mode '$WEBOTS_MODE' (must be pause, realtime, or fast)."
    exit 1
fi

if ! [[ "$WEBOTS_CONNECT_TIMEOUT" =~ ^[0-9]+$ ]]; then
    echo "Invalid --connect-timeout '$WEBOTS_CONNECT_TIMEOUT' (must be a non-negative integer)."
    exit 1
fi

if ! [[ "$WEBOTS_CLUSTER_DISTRIBUTION_TIMEOUT" =~ ^[0-9]+$ ]]; then
    echo "Invalid --cluster-distribution-timeout '$WEBOTS_CLUSTER_DISTRIBUTION_TIMEOUT' (must be a non-negative integer)."
    exit 1
fi

for pair in \
    "NM_WEBOTS_RECORD_WIDTH:$WEBOTS_RECORD_WIDTH" \
    "NM_WEBOTS_RECORD_HEIGHT:$WEBOTS_RECORD_HEIGHT" \
    "NM_WEBOTS_RECORD_DURATION_MS:$WEBOTS_RECORD_DURATION_MS" \
    "NM_WEBOTS_RECORD_QUALITY:$WEBOTS_RECORD_QUALITY" \
    "NM_WEBOTS_RECORD_ACCELERATION:$WEBOTS_RECORD_ACCELERATION" \
    "NM_WEBOTS_RECORD_CODEC:$WEBOTS_RECORD_CODEC" \
    "NM_WEBOTS_RECORD_PROGRESS_INTERVAL_MS:$WEBOTS_RECORD_PROGRESS_INTERVAL_MS"; do
    key="${pair%%:*}"
    val="${pair#*:}"
    if ! [[ "$val" =~ ^[0-9]+$ ]]; then
        echo "Invalid $key '$val' (must be a non-negative integer)."
        exit 1
    fi
done
if [ "$WEBOTS_RECORD_WIDTH" -lt 16 ] || [ "$WEBOTS_RECORD_WIDTH" -gt 16384 ]; then
    echo "Invalid NM_WEBOTS_RECORD_WIDTH '$WEBOTS_RECORD_WIDTH' (must be 16..16384)."
    exit 1
fi
if [ "$WEBOTS_RECORD_HEIGHT" -lt 16 ] || [ "$WEBOTS_RECORD_HEIGHT" -gt 16384 ]; then
    echo "Invalid NM_WEBOTS_RECORD_HEIGHT '$WEBOTS_RECORD_HEIGHT' (must be 16..16384)."
    exit 1
fi
if [ "$WEBOTS_RECORD_QUALITY" -lt 1 ] || [ "$WEBOTS_RECORD_QUALITY" -gt 100 ]; then
    echo "Invalid NM_WEBOTS_RECORD_QUALITY '$WEBOTS_RECORD_QUALITY' (must be 1..100)."
    exit 1
fi
if [ "$WEBOTS_RECORD_ACCELERATION" -lt 1 ] || [ "$WEBOTS_RECORD_ACCELERATION" -gt 512 ]; then
    echo "Invalid NM_WEBOTS_RECORD_ACCELERATION '$WEBOTS_RECORD_ACCELERATION' (must be 1..512)."
    exit 1
fi
if [ "$WEBOTS_RECORD_CODEC" -gt 64 ]; then
    echo "Invalid NM_WEBOTS_RECORD_CODEC '$WEBOTS_RECORD_CODEC' (must be 0..64)."
    exit 1
fi
if [ "$WEBOTS_RECORD_PROGRESS_INTERVAL_MS" -lt 100 ] || [ "$WEBOTS_RECORD_PROGRESS_INTERVAL_MS" -gt 600000 ]; then
    echo "Invalid NM_WEBOTS_RECORD_PROGRESS_INTERVAL_MS '$WEBOTS_RECORD_PROGRESS_INTERVAL_MS' (must be 100..600000)."
    exit 1
fi
if [ "$WEBOTS_RECORD_PROGRESS" = "auto" ]; then
    if [ "$WEBOTS_RECORD" -eq 1 ] && [ "$WEBOTS_RECORD_DURATION_MS" -gt 0 ] && [ -t 1 ]; then
        WEBOTS_RECORD_PROGRESS=1
    else
        WEBOTS_RECORD_PROGRESS=0
    fi
fi
if [ "$WEBOTS_RECORD_PROGRESS" -eq 1 ] && [ "$WEBOTS_RECORD_DURATION_MS" -le 0 ]; then
    echo "Webots recording progress requires --webots-record-duration-ms; disabling progress for open-ended recording."
    WEBOTS_RECORD_PROGRESS=0
fi

if ! [[ "$WEBOTS_STEP_SLEEP_MS" =~ ^[0-9]+$ ]]; then
    echo "Invalid NM_WEBOTS_STEP_SLEEP_MS '$WEBOTS_STEP_SLEEP_MS' (must be a non-negative integer in milliseconds)."
    exit 1
fi
if [ "$WEBOTS_STEP_SLEEP_MS" -gt 600000 ]; then
    echo "Invalid NM_WEBOTS_STEP_SLEEP_MS '$WEBOTS_STEP_SLEEP_MS' (must be <= 600000 milliseconds)."
    exit 1
fi

if [ -n "$CONFIG_FILE" ] && [ ! -f "$CONFIG_FILE" ]; then
    echo "Config file not found: $CONFIG_FILE"
    exit 1
fi

if [ -n "$NETWORK_FILE" ] && [ ! -f "$NETWORK_FILE" ]; then
    echo "Network snapshot not found: $NETWORK_FILE"
    exit 1
fi

if [ -n "$ORCHESTRATOR_PORT" ] && ! [[ "$ORCHESTRATOR_PORT" =~ ^[0-9]+$ ]]; then
    echo "Invalid --orchestrator-port '$ORCHESTRATOR_PORT' (must be an integer)."
    exit 1
fi

if ! [[ "$REMOTE_WEB_UI_PORT" =~ ^[0-9]+$ ]]; then
    echo "Invalid --remote-web-ui-port '$REMOTE_WEB_UI_PORT' (must be an integer)."
    exit 1
fi

if [ -n "$REMOTE_WEB_UI_API_PORT" ] && ! [[ "$REMOTE_WEB_UI_API_PORT" =~ ^[0-9]+$ ]]; then
    echo "Invalid --remote-web-ui-api-port '$REMOTE_WEB_UI_API_PORT' (must be an integer)."
    exit 1
fi

if [ "$REMOTE_UI_MODE" = "rust" ]; then
    echo "remote-ui-mode=rust is deprecated and disabled; using web_ui instead."
    REMOTE_UI_MODE="web"
fi
if [ "$REMOTE_UI_MODE" != "web" ]; then
    echo "Invalid --remote-ui-mode '$REMOTE_UI_MODE' (must be web)."
    exit 1
fi

if [ "$REMOTE_SYNC_DATA" != "auto" ] && [ "$REMOTE_SYNC_DATA" != "always" ] && [ "$REMOTE_SYNC_DATA" != "never" ]; then
    echo "Invalid --remote-sync-data '$REMOTE_SYNC_DATA' (must be auto, always, or never)."
    exit 1
fi

if [ "$REMOTE_RSYNC_COMPRESS" != "auto" ] && [ "$REMOTE_RSYNC_COMPRESS" != "on" ] && [ "$REMOTE_RSYNC_COMPRESS" != "off" ]; then
    echo "Invalid --remote-rsync-compress '$REMOTE_RSYNC_COMPRESS' (must be auto, on, or off)."
    exit 1
fi

declare -A SENSOR_REGEX=()
declare -A ACTUATOR_REGEX=()
declare -A SOCKET_PATHS=()
declare -A NODE_PORTS=()
declare -A USED_PORTS=()
declare -A CONFIG_FILE_MAP=()
declare -A NETWORK_FILE_MAP=()

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

declare -a REMOTE_HOST_LIST=()
declare -A REMOTE_HOST_WEIGHT_MAP=()

cmd_to_string() {
    local out=""
    local arg
    for arg in "$@"; do
        printf -v out '%s%q ' "$out" "$arg"
    done
    printf "%s" "${out% }"
}

abs_path_from_root() {
    local p="$1"
    if [[ "$p" = /* ]]; then
        printf "%s" "$p"
    else
        printf "%s/%s" "$ROOT_DIR" "$p"
    fi
}

remote_path_for_local() {
    local local_path
    local_path="$(abs_path_from_root "$1")"
    if [[ "$local_path" = "$ROOT_DIR/"* ]]; then
        printf "%s/%s" "$REMOTE_ROOT_DIR" "${local_path#$ROOT_DIR/}"
    else
        printf "%s" "$local_path"
    fi
}

build_remote_workspace_bindings_json() {
    local raw="${NM_RUNTIME_WORKSPACE_BINDINGS:-}"
    [ -z "$raw" ] && return 0

    python3 - "$ROOT_DIR" "$REMOTE_ROOT_DIR" "$raw" <<'PY'
import json
import os
import sys

root_dir = os.path.abspath(sys.argv[1])
remote_root = sys.argv[2]
bindings = json.loads(sys.argv[3])

prefix = root_dir + os.sep

def remap(value):
    if isinstance(value, str):
        if value == root_dir:
            return remote_root
        if value.startswith(prefix):
            return remote_root + value[len(root_dir):]
        return value
    if isinstance(value, list):
        return [remap(item) for item in value]
    if isinstance(value, dict):
        return {key: remap(item) for key, item in value.items()}
    return value

print(json.dumps(remap(bindings), separators=(",", ":")))
PY
}

trim_ws() {
    local value="$1"
    value="${value#"${value%%[![:space:]]*}"}"
    value="${value%"${value##*[![:space:]]}"}"
    printf "%s" "$value"
}

parse_brain_file_map() {
    local raw="$1"
    local map_name="$2"
    local label="$3"
    local -n map_ref="$map_name"
    map_ref=()
    [ -z "$raw" ] && return

    local IFS=','
    read -r -a pairs <<< "$raw"
    local pair
    for pair in "${pairs[@]}"; do
        pair="$(trim_ws "$pair")"
        [ -z "$pair" ] && continue
        if [[ "$pair" != *=* ]]; then
            echo "Invalid $label map entry '$pair' (expected brain=/path)."
            exit 1
        fi
        local brain="${pair%%=*}"
        local path="${pair#*=}"
        brain="$(trim_ws "$brain")"
        path="$(trim_ws "$path")"
        if [ -z "$brain" ] || [ -z "$path" ]; then
            echo "Invalid $label map entry '$pair' (brain and path are required)."
            exit 1
        fi
        map_ref["$brain"]="$path"
    done
}

canonicalize_and_validate_map_paths() {
    local map_name="$1"
    local label="$2"
    local -n map_ref="$map_name"
    local brain
    for brain in "${!map_ref[@]}"; do
        local raw_path="${map_ref[$brain]}"
        local abs_path
        abs_path="$(abs_path_from_root "$raw_path")"
        if [ ! -f "$abs_path" ]; then
            echo "$label file not found for brain '$brain': $raw_path"
            exit 1
        fi
        map_ref["$brain"]="$abs_path"
    done
}

map_to_csv() {
    local map_name="$1"
    local -n map_ref="$map_name"
    local out=""
    local brain
    for brain in "${!map_ref[@]}"; do
        out+="${out:+,}${brain}=${map_ref[$brain]}"
    done
    printf "%s" "$out"
}

config_for_brain() {
    local brain="$1"
    if [ -n "${CONFIG_FILE_MAP[$brain]+x}" ]; then
        printf "%s" "${CONFIG_FILE_MAP[$brain]}"
    else
        printf "%s" "${CONFIG_FILE:-}"
    fi
}

network_for_brain() {
    local brain="$1"
    if [ -n "${NETWORK_FILE_MAP[$brain]+x}" ]; then
        printf "%s" "${NETWORK_FILE_MAP[$brain]}"
    else
        printf "%s" "${NETWORK_FILE:-}"
    fi
}

remote_config_for_brain() {
    local brain="$1"
    local local_path
    local_path="$(config_for_brain "$brain")"
    if [ -n "$local_path" ]; then
        remote_path_for_local "$local_path"
    fi
}

remote_network_for_brain() {
    local brain="$1"
    local local_path
    local_path="$(network_for_brain "$brain")"
    if [ -n "$local_path" ]; then
        remote_path_for_local "$local_path"
    fi
}

build_orchestrator_network_specs_json() {
    local mode="${1:-local}"
    local -a triples=()
    local brain
    for brain in "${BRAINS[@]}"; do
        local cfg=""
        local net=""
        if [ "$mode" = "remote" ]; then
            cfg="$(remote_config_for_brain "$brain")"
            net="$(remote_network_for_brain "$brain")"
        else
            cfg="$(config_for_brain "$brain")"
            net="$(network_for_brain "$brain")"
        fi
        triples+=("$brain" "$cfg" "$net")
    done

    python3 - "${triples[@]}" <<'PY'
import json
import sys

args = sys.argv[1:]
if len(args) % 3 != 0:
    raise SystemExit(2)

specs = []
for i in range(0, len(args), 3):
    network_id, config_path, network_path = args[i:i + 3]
    network_id = network_id.strip()
    if not network_id:
        continue
    spec = {"network_id": network_id}
    if config_path:
        spec["config_path"] = config_path
    if network_path:
        spec["network_path"] = network_path
    specs.append(spec)

print(json.dumps(specs, separators=(",", ":")))
PY
}

parse_remote_hosts() {
    REMOTE_HOST_LIST=()
    local IFS=','
    read -r -a _hosts <<< "$REMOTE_HOSTS"
    local host
    for host in "${_hosts[@]}"; do
        host="$(echo "$host" | xargs)"
        [ -z "$host" ] && continue
        REMOTE_HOST_LIST+=("$host")
    done
}

parse_remote_weights() {
    REMOTE_HOST_WEIGHT_MAP=()
    [ -z "$REMOTE_HOST_WEIGHTS" ] && return
    local IFS=','
    read -r -a _pairs <<< "$REMOTE_HOST_WEIGHTS"
    local pair
    for pair in "${_pairs[@]}"; do
        pair="$(echo "$pair" | xargs)"
        [ -z "$pair" ] && continue
        if [[ "$pair" != *=* ]]; then
            continue
        fi
        local host="${pair%%=*}"
        local weight="${pair#*=}"
        host="$(echo "$host" | xargs)"
        weight="$(echo "$weight" | xargs)"
        [ -z "$host" ] && continue
        [ -z "$weight" ] && continue
        REMOTE_HOST_WEIGHT_MAP["$host"]="$weight"
    done
}

remote_weight_for_host() {
    local host="$1"
    printf "%s" "${REMOTE_HOST_WEIGHT_MAP[$host]:-1.0}"
}

float_gt() {
    local a="$1"
    local b="$2"
    awk -v a="$a" -v b="$b" 'BEGIN { exit !(a > b) }'
}

copies_for_weight() {
    local w="$1"
    awk -v w="$w" 'BEGIN { c = int(w + 0.5); if (c < 1) c = 1; print c }'
}

init_remote_ssh() {
    if [ -z "$REMOTE_USER" ]; then
        echo "Missing --remote-user (and USER is empty)."
        exit 1
    fi
    REMOTE_SSH_ARGV=(ssh -o BatchMode=yes -o ConnectTimeout=8)
    if [ -n "$REMOTE_SSH_OPTS" ]; then
        local extra=()
        read -r -a extra <<< "$REMOTE_SSH_OPTS"
        if [ "${#extra[@]}" -gt 0 ]; then
            REMOTE_SSH_ARGV+=("${extra[@]}")
        fi
    fi
    REMOTE_SSH_READY=1
}

sync_remote_source_tree_with_rsync() {
    local host="$1"
    if ! command -v rsync >/dev/null 2>&1; then
        echo "rsync not found locally."
        return 127
    fi

    local use_compress
    use_compress="$(resolve_sync_compress_mode "$host")"
    echo "  sync backend for $host: rsync (compression=$use_compress)"

    local -a rsync_common=(
        rsync
        -a
        --delete
        --partial
        --inplace
        --human-readable
        --exclude '.git/'
        --exclude '.idea/'
        --exclude '.venv/'
        --exclude 'target/'
        --exclude 'logs/'
        --exclude '__pycache__/'
        --exclude '*.pyc'
        --exclude 'node_modules/'
        --exclude '.cache/'
    )
    if rsync_supports_option '--delete-delay'; then
        rsync_common+=(--delete-delay)
    fi
    if [ "$use_compress" = "on" ]; then
        rsync_common+=(-z)
        if rsync_supports_option '--compress-choice'; then
            rsync_common+=(--compress-choice=zstd)
        fi
        if rsync_supports_option '--compress-level'; then
            rsync_common+=(--compress-level=1)
        fi
    else
        rsync_common+=(--whole-file)
    fi

    # Fast code/runtime mirror each run; dataset handled separately by policy.
    LC_ALL=C LANG=C "${rsync_common[@]}" \
        --exclude 'data/' \
        -e "${REMOTE_SSH_ARGV[*]}" \
        "$ROOT_DIR/" \
        "${REMOTE_USER}@${host}:$REMOTE_ROOT_DIR/" || return 1

    local data_mode="$REMOTE_SYNC_DATA"
    if [ "$data_mode" = "auto" ]; then
        if remote_exec_script "$host" "[ -f \"$REMOTE_ROOT_DIR/.nm_data_synced\" ]" >/dev/null 2>&1; then
            data_mode="never"
        elif remote_exec_script "$host" \
            "[ -d \"$REMOTE_ROOT_DIR/data\" ] && [ \"\$(ls -A \"$REMOTE_ROOT_DIR/data\" 2>/dev/null)\" != \"\" ]" \
            >/dev/null 2>&1; then
            data_mode="never"
            remote_exec_script "$host" \
                "date -u +%FT%TZ > \"$REMOTE_ROOT_DIR/.nm_data_synced\"" >/dev/null 2>&1 || true
        else
            data_mode="always"
        fi
    fi
    echo "  data sync mode for $host: $data_mode"

    if [ "$data_mode" = "always" ] && [ -d "$ROOT_DIR/data" ]; then
        echo "Syncing large data/ tree to $host (initial sync or forced mode)..."
        LC_ALL=C LANG=C "${rsync_common[@]}" \
            -e "${REMOTE_SSH_ARGV[*]}" \
            "$ROOT_DIR/data/" \
            "${REMOTE_USER}@${host}:$REMOTE_ROOT_DIR/data/" || return 1
        remote_exec_script "$host" \
            "date -u +%FT%TZ > \"$REMOTE_ROOT_DIR/.nm_data_synced\"" >/dev/null 2>&1 || true
    fi

    return 0
}

sync_remote_source_tree_with_rclone() {
    local host="$1"
    if ! command -v rclone >/dev/null 2>&1; then
        echo "rclone not found locally."
        return 127
    fi

    local remote_root="${REMOTE_ROOT_DIR%/}/"
    local remote_data="${REMOTE_ROOT_DIR%/}/data/"

    local -a rclone_common=(
        rclone
        sync
        --config /dev/null
        --sftp-host "$host"
        --sftp-user "$REMOTE_USER"
        --sftp-shell-type unix
        --sftp-md5sum-command md5sum
        --sftp-sha1sum-command sha1sum
        --checkers 16
        --transfers 16
        --retries 3
        --low-level-retries 10
        --stats 10s
    )
    if rclone_supports_flag '--fast-list'; then
        rclone_common+=(--fast-list)
    fi
    echo "  sync backend for $host: rclone (sftp)"

    # Fast code/runtime mirror each run; dataset handled separately by policy.
    "${rclone_common[@]}" \
        --exclude '.git/**' \
        --exclude '.idea/**' \
        --exclude '.venv/**' \
        --exclude 'target/**' \
        --exclude 'logs/**' \
        --exclude '__pycache__/**' \
        --exclude '*.pyc' \
        --exclude 'node_modules/**' \
        --exclude '.cache/**' \
        --exclude 'data/**' \
        "$ROOT_DIR/" \
        ":sftp:$remote_root" || return 1

    local data_mode="$REMOTE_SYNC_DATA"
    if [ "$data_mode" = "auto" ]; then
        if remote_exec_script "$host" "[ -f \"$REMOTE_ROOT_DIR/.nm_data_synced\" ]" >/dev/null 2>&1; then
            data_mode="never"
        elif remote_exec_script "$host" \
            "[ -d \"$REMOTE_ROOT_DIR/data\" ] && [ \"\$(ls -A \"$REMOTE_ROOT_DIR/data\" 2>/dev/null)\" != \"\" ]" \
            >/dev/null 2>&1; then
            data_mode="never"
            remote_exec_script "$host" \
                "date -u +%FT%TZ > \"$REMOTE_ROOT_DIR/.nm_data_synced\"" >/dev/null 2>&1 || true
        else
            data_mode="always"
        fi
    fi
    echo "  data sync mode for $host: $data_mode"

    if [ "$data_mode" = "always" ] && [ -d "$ROOT_DIR/data" ]; then
        echo "Syncing large data/ tree to $host (initial sync or forced mode)..."
        "${rclone_common[@]}" \
            "$ROOT_DIR/data/" \
            ":sftp:$remote_data" || return 1
        remote_exec_script "$host" \
            "date -u +%FT%TZ > \"$REMOTE_ROOT_DIR/.nm_data_synced\"" >/dev/null 2>&1 || true
    fi

    return 0
}

sync_remote_source_tree() {
    local host="$1"
    if [ -z "$REMOTE_USER" ]; then
        echo "Missing remote user for source sync."
        return 1
    fi
    remote_exec_script "$host" "mkdir -p \"$REMOTE_ROOT_DIR\"" >/dev/null 2>&1 || return 1

    if command -v rclone >/dev/null 2>&1; then
        if sync_remote_source_tree_with_rclone "$host"; then
            return 0
        fi
        echo "  rclone sync failed on $host; falling back to rsync."
    fi

    if command -v rsync >/dev/null 2>&1; then
        if sync_remote_source_tree_with_rsync "$host"; then
            return 0
        fi
        return 1
    fi

    echo "Neither rclone nor rsync is available locally; cannot sync to $host."
    return 1
}

remote_reachable() {
    local host="$1"
    remote_exec_script "$host" "echo ok" >/dev/null 2>&1
}

choose_best_remote_host() {
    local best=""
    local best_weight="0"
    local host
    for host in "${REMOTE_HOST_LIST[@]}"; do
        if ! remote_reachable "$host"; then
            continue
        fi
        local w
        w="$(remote_weight_for_host "$host")"
        if [ -z "$best" ] || float_gt "$w" "$best_weight"; then
            best="$host"
            best_weight="$w"
        fi
    done
    printf "%s" "$best"
}

remote_exec_script() {
    local host="$1"
    shift
    local remote_cmd=""
    if [ "$#" -eq 1 ]; then
        remote_cmd="LC_ALL=C LANG=C $1"
    else
        remote_cmd="LC_ALL=C LANG=C $(cmd_to_string "$@")"
    fi
    LC_ALL=C LANG=C "${REMOTE_SSH_ARGV[@]}" "${REMOTE_USER}@${host}" "$remote_cmd"
}

remote_preclean_host() {
    local host="$1"
    remote_exec_script "$host" bash -s -- "$REMOTE_ROOT_DIR" <<'EOS'
set -euo pipefail
ROOT="$1"
patterns=(
    "$ROOT/target/release/aarnn_rust --orchestrator"
    "$ROOT/target/release/aarnn_rust --node"
    "target/release/aarnn_rust --orchestrator"
    "target/release/aarnn_rust --node"
    "$ROOT/target/release/web_ui --listen"
    "target/release/web_ui --listen"
)

matched=0
for pattern in "${patterns[@]}"; do
    if pgrep -u "$USER" -f "$pattern" >/dev/null 2>&1; then
        matched=1
        pkill -TERM -u "$USER" -f "$pattern" >/dev/null 2>&1 || true
    fi
done

if [ "$matched" -eq 1 ]; then
    sleep 0.4
    for pattern in "${patterns[@]}"; do
        pkill -KILL -u "$USER" -f "$pattern" >/dev/null 2>&1 || true
    done
fi

if command -v podman >/dev/null 2>&1; then
    if podman ps -aq --filter label=nm.runtime=rust_ui_novnc | grep -q .; then
        matched=1
        podman ps -aq --filter label=nm.runtime=rust_ui_novnc | xargs -r podman rm -f >/dev/null 2>&1 || true
    fi
fi

echo "$matched"
EOS
}

remote_preclean_runtime() {
    local host
    local cleaned_any=0
    for host in "${REMOTE_HOST_LIST[@]}"; do
        if ! remote_reachable "$host"; then
            continue
        fi
        local matched
        matched="$(remote_preclean_host "$host" 2>/dev/null || echo "0")"
        if [ "$matched" = "1" ]; then
            cleaned_any=1
            echo "Stopped prior remote runtime processes on $host."
        fi
    done
    if [ "$cleaned_any" -eq 0 ]; then
        echo "No prior remote runtime processes were detected on compute hosts."
    fi
}

register_remote_proc() {
    local host="$1"
    local pid="$2"
    local tag="$3"
    REMOTE_PROC_HOSTS+=("$host")
    REMOTE_PROC_PIDS+=("$pid")
    REMOTE_PROC_TAGS+=("$tag")
}

register_remote_container() {
    local host="$1"
    local name="$2"
    REMOTE_CONTAINER_HOSTS+=("$host")
    REMOTE_CONTAINER_NAMES+=("$name")
}

remote_start_bg() {
    local host="$1"
    local tag="$2"
    local cmd_str="$3"
    local remote_log_dir="${4:-$REMOTE_ROOT_DIR/logs}"
    local cmd_b64
    cmd_b64="$(printf "%s" "$cmd_str" | base64 | tr -d '\n')"
    local output
    output="$(remote_exec_script "$host" bash -s -- "$REMOTE_ROOT_DIR" "$remote_log_dir" "$tag" "$cmd_b64" <<'EOS'
set -euo pipefail
ROOT="$1"
LOG_DIR="$2"
TAG="$3"
CMD_B64="$4"
if command -v base64 >/dev/null 2>&1; then
    CMD="$(printf '%s' "$CMD_B64" | base64 -d)"
elif command -v python3 >/dev/null 2>&1; then
    CMD="$(python3 - "$CMD_B64" <<'PY'
import base64
import sys
print(base64.b64decode(sys.argv[1]).decode("utf-8"), end="")
PY
)"
else
    echo "Neither base64 nor python3 is available to decode remote command."
    exit 1
fi
mkdir -p "$LOG_DIR"
cd "$ROOT"
nohup bash -lc "$CMD" >"$LOG_DIR/$TAG.log" 2>&1 &
echo $!
EOS
)"
    local pid
    pid="$(echo "$output" | awk '/^[0-9]+$/ {v=$0} END {print v}')"
    if [ -z "$pid" ] || ! [[ "$pid" =~ ^[0-9]+$ ]]; then
        echo "Failed to start remote process '$tag' on $host (output: $output)"
        return 1
    fi
    register_remote_proc "$host" "$pid" "$tag"
    echo "$pid"
}

wait_for_remote_socket_path() {
    local host="$1"
    local socket_path="$2"
    local timeout_s="${3:-20}"
    local deadline=$((SECONDS + timeout_s))
    while [ "$SECONDS" -lt "$deadline" ]; do
        if remote_exec_script "$host" bash -s -- "$socket_path" <<'EOS' >/dev/null 2>&1
set -euo pipefail
SOCKET_PATH="$1"
[ -S "$SOCKET_PATH" ]
EOS
        then
            return 0
        fi
        sleep 0.2
    done
    return 1
}

start_remote_rust_ui_novnc() {
    local host="$1"
    local browser_port="$2"
    local brain="$3"
    local container_name
    container_name="nm-rust-ui-novnc-${brain}-${browser_port}"
    local output
    output="$(remote_exec_script "$host" bash -s -- "$browser_port" "$container_name" <<'EOS'
set -euo pipefail
PORT="$1"
CONTAINER_NAME="$2"
IMAGE="${NM_REMOTE_NOVNC_IMAGE:-docker.io/theasp/novnc:latest}"
if ! command -v podman >/dev/null 2>&1; then
    echo "podman not found on remote host"
    exit 127
fi
podman rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true
CID="$(
    podman run -d \
      --name "$CONTAINER_NAME" \
      --label nm.runtime=rust_ui_novnc \
      --label nm.runtime.hosted_by=run_webot \
      -p "${PORT}:8080" \
      -v /tmp/.X11-unix:/tmp/.X11-unix \
      -e DISPLAY=:0 \
      -e RUN_XTERM=no \
      -e DISPLAY_WIDTH="${NM_REMOTE_UI_WIDTH:-1920}" \
      -e DISPLAY_HEIGHT="${NM_REMOTE_UI_HEIGHT:-1080}" \
      "$IMAGE"
)"
if [ -z "$CID" ]; then
    exit 1
fi
echo "$CID"
EOS
    )" || return 1
    if [ -z "$output" ]; then
        return 1
    fi
    register_remote_container "$host" "$container_name"
    if ! wait_for_remote_socket_path "$host" "/tmp/.X11-unix/X0" 20; then
        echo "Remote noVNC container did not expose /tmp/.X11-unix/X0 on $host"
        return 1
    fi
    return 0
}

wait_for_remote_port() {
    local host="$1"
    local port="$2"
    local timeout_s="${3:-30}"
    local deadline=$((SECONDS + timeout_s))
    while [ "$SECONDS" -lt "$deadline" ]; do
        if timeout 1 bash -lc "cat < /dev/null > /dev/tcp/$host/$port" >/dev/null 2>&1; then
            return 0
        fi
        sleep 0.3
    done
    return 1
}

remote_is_port_free() {
    local host="$1"
    local port="$2"
    remote_exec_script "$host" bash -s -- "$port" <<'EOS' >/dev/null 2>&1
set -euo pipefail
PORT="$1"
if command -v ss >/dev/null 2>&1; then
    if ss -H -ltn | awk '{print $4}' | awk -F: '{print $NF}' | grep -qx "$PORT"; then
        exit 1
    fi
    if ss -H -lun | awk '{print $4}' | awk -F: '{print $NF}' | grep -qx "$PORT"; then
        exit 1
    fi
fi
exit 0
EOS
}

find_remote_free_port() {
    local host="$1"
    local start="${2:-50051}"
    local output
    output="$(remote_exec_script "$host" bash -s -- "$start" <<'EOS'
set -euo pipefail
START="$1"
is_port_free() {
    local p="$1"
    if command -v ss >/dev/null 2>&1; then
        if ss -H -ltn | awk '{print $4}' | awk -F: '{print $NF}' | grep -qx "$p"; then
            return 1
        fi
        if ss -H -lun | awk '{print $4}' | awk -F: '{print $NF}' | grep -qx "$p"; then
            return 1
        fi
    fi
    return 0
}
p="$START"
while [ "$p" -le 65535 ]; do
    if is_port_free "$p"; then
        echo "$p"
        exit 0
    fi
    p=$((p + 1))
done
exit 1
EOS
)" || return 1
    local port
    port="$(echo "$output" | awk '/^[0-9]+$/ {v=$0} END {print v}')"
    [ -n "$port" ] || return 1
    printf "%s" "$port"
}

wait_for_http_ready() {
    local url="$1"
    local timeout_s="${2:-30}"
    local deadline=$((SECONDS + timeout_s))
    while [ "$SECONDS" -lt "$deadline" ]; do
        if python3 - "$url" <<'PY' >/dev/null 2>&1
import sys, urllib.request
url = sys.argv[1]
with urllib.request.urlopen(url, timeout=1.0) as resp:
    sys.exit(0 if resp.status < 500 else 1)
PY
        then
            return 0
        fi
        sleep 0.4
    done
    return 1
}

wait_for_cluster_distribution() {
    local web_ui_url="$1"
    local orchestrator_addr="$2"
    local network_id="$3"
    local timeout_s="${4:-$WEBOTS_CLUSTER_DISTRIBUTION_TIMEOUT}"
    local poll_out=""
    local poll_status=""
    local poll_dist_count="0"
    local poll_net_count="0"
    local poll_node_count="0"
    local poll_msg=""
    local last_poll_out=""
    local progress_every_s=10
    local next_progress=$((SECONDS + progress_every_s))
    local deadline=$((SECONDS + timeout_s))
    while [ "$SECONDS" -lt "$deadline" ]; do
        poll_out="$(python3 - "$web_ui_url" "$orchestrator_addr" "$network_id" <<'PY' 2>/dev/null
import json
import sys
import urllib.error
import urllib.parse
import urllib.request

web_ui_url = sys.argv[1].rstrip("/")
addr = sys.argv[2]
network_id = sys.argv[3]
query = urllib.parse.urlencode({"addr": addr})
url = f"{web_ui_url}/api/status?{query}"
try:
    with urllib.request.urlopen(url, timeout=1.5) as resp:
        payload = json.loads(resp.read().decode("utf-8", errors="replace") or "{}")
except urllib.error.HTTPError as e:
    body = ""
    try:
        body = (e.read() or b"").decode("utf-8", errors="replace")
    except Exception:
        body = ""
    body = body.strip().replace("\n", " ")
    if len(body) > 220:
        body = body[:220] + "..."
    detail = f"{e}; body={body}" if body else str(e)
    print(f"error|0|0|0|{detail}")
    raise SystemExit(2)
except Exception as e:
    detail = str(e).strip().replace("\n", " ")
    if len(detail) > 220:
        detail = detail[:220] + "..."
    print(f"error|0|0|0|{detail}")
    raise SystemExit(2)

nodes = payload.get("nodes", [])
networks = payload.get("networks", [])
node_count = len(nodes) if isinstance(nodes, list) else 0
net_count = len(networks) if isinstance(networks, list) else 0

found = None
if isinstance(networks, list):
    for net in networks:
        if str(net.get("network_id", "")) == network_id:
            found = net
            break

if found is None:
    print(f"missing|0|{net_count}|{node_count}|network_missing")
    raise SystemExit(1)

dist = found.get("distribution", [])
dist_count = len(dist) if isinstance(dist, list) else 0
if dist_count > 0:
    print(f"ready|{dist_count}|{net_count}|{node_count}|ok")
    raise SystemExit(0)

print(f"pending|{dist_count}|{net_count}|{node_count}|distribution_empty")
raise SystemExit(1)
PY
)" || true
        last_poll_out="$poll_out"
        IFS='|' read -r poll_status poll_dist_count poll_net_count poll_node_count poll_msg <<<"$poll_out"
        if [ "${poll_status:-}" = "ready" ]; then
            return 0
        fi
        if [ "$SECONDS" -ge "$next_progress" ]; then
            echo "  waiting for distribution '$network_id': status=${poll_status:-unknown} dist=${poll_dist_count:-0} nets=${poll_net_count:-0} nodes=${poll_node_count:-0} (${SECONDS}s elapsed)"
            next_progress=$((SECONDS + progress_every_s))
        fi
        sleep 0.5
    done
    if [ -n "$last_poll_out" ]; then
        echo "  final distribution wait state for '$network_id': $last_poll_out"
    fi
    return 1
}

print_cluster_status_debug() {
    local web_ui_url="$1"
    local orchestrator_addr="$2"
    local focus_network="${3:-}"
    python3 - "$web_ui_url" "$orchestrator_addr" "$focus_network" <<'PY'
import json
import sys
import urllib.error
import urllib.parse
import urllib.request

web_ui_url = sys.argv[1].rstrip("/")
addr = sys.argv[2]
focus = sys.argv[3].strip()
query = urllib.parse.urlencode({"addr": addr})
url = f"{web_ui_url}/api/status?{query}"

try:
    with urllib.request.urlopen(url, timeout=2.5) as resp:
        payload = json.loads(resp.read().decode("utf-8", errors="replace") or "{}")
except urllib.error.HTTPError as e:
    body = ""
    try:
        body = (e.read() or b"").decode("utf-8", errors="replace")
    except Exception:
        body = ""
    body = body.strip().replace("\n", " ")
    if len(body) > 400:
        body = body[:400] + "..."
    if body:
        print(f"Cluster status debug failed: {e}; body={body}")
    else:
        print(f"Cluster status debug failed: {e}")
    raise SystemExit(1)
except Exception as e:
    detail = str(e).strip().replace("\n", " ")
    if len(detail) > 400:
        detail = detail[:400] + "..."
    print(f"Cluster status debug failed: {detail}")
    raise SystemExit(1)

nodes = payload.get("nodes", [])
networks = payload.get("networks", [])
if not isinstance(nodes, list):
    nodes = []
if not isinstance(networks, list):
    networks = []

print(f"Cluster status snapshot: nodes={len(nodes)} networks={len(networks)}")
for n in nodes:
    node_id = n.get("node_id", "")
    active = n.get("active_networks", [])
    if not isinstance(active, list):
        active = []
    print(f"  node={node_id} active_networks={len(active)}")

for net in networks:
    nid = str(net.get("network_id", ""))
    dist = net.get("distribution", [])
    dist_count = len(dist) if isinstance(dist, list) else 0
    total = net.get("total_neurons", 0)
    playing = net.get("playing", False)
    mark = "*" if focus and nid == focus else " "
    print(f" {mark}network={nid} dist={dist_count} total_neurons={total} playing={playing}")
PY
}

start_local_rust_ui_client() {
    local orchestrator_addr="$1"
    local brain="$2"
    if [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
        echo "Local rust_ui requested, but no display is available (DISPLAY/WAYLAND_DISPLAY unset)."
        return 1
    fi
    local bin="$ROOT_DIR/target/release/aarnn_rust"
    if [ ! -x "$bin" ]; then
        echo "Local rust_ui requested, but executable is missing: $bin"
        echo "Build with: cargo build --release --bin aarnn_rust --all-features"
        return 1
    fi

    local rust_ui_log="$LOG_DIR/rust_ui_${brain}.log"
    local rust_ui_cmd=(
        "$bin"
        --ui
        --ui-remote-only
        --brain-id "$brain"
        --orchestrator-addr "$orchestrator_addr"
    )
    local brain_config
    brain_config="$(config_for_brain "$brain")"
    if [ -n "$brain_config" ]; then
        rust_ui_cmd+=(--config "$brain_config")
    fi
    local brain_network
    brain_network="$(network_for_brain "$brain")"
    if [ -n "$brain_network" ]; then
        rust_ui_cmd+=(--network "$brain_network")
    fi

    NM_UI_REMOTE_ORCHESTRATORS="$orchestrator_addr" \
        "${rust_ui_cmd[@]}" >"$rust_ui_log" 2>&1 &
    local rust_ui_pid="$!"
    PIDS+=("$rust_ui_pid")
    LOCAL_RUST_UI_LOG="$rust_ui_log"
    return 0
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

prepare_recording_world() {
    if [ "$WEBOTS_RECORD" -ne 1 ]; then
        return 0
    fi
    if [ ! -f "$WORLD_FILE" ]; then
        return 0
    fi
    if grep -Eq 'controller[[:space:]]+"nm_world_recorder"' "$WORLD_FILE" \
        && grep -Eq 'supervisor[[:space:]]+TRUE' "$WORLD_FILE"; then
        return 0
    fi

    local world_dir
    local world_base
    local world_stem
    world_dir="$(cd "$(dirname "$WORLD_FILE")" && pwd)"
    world_base="$(basename "$WORLD_FILE")"
    world_stem="${world_base%.*}"

    WEBOTS_RECORD_WORLD_FILE="$(mktemp "${world_dir}/.${world_stem}.recording.XXXXXX.wbt")"
    python3 - "$WORLD_FILE" "$WEBOTS_RECORD_WORLD_FILE" <<'PY'
from pathlib import Path
import re
import sys

src = Path(sys.argv[1])
dst = Path(sys.argv[2])
text = src.read_text(encoding="utf-8")
text = re.sub(
    r"\n?Supervisor\s*\{\s*name\s+\"NM_WORLD_RECORDER\"\s*controller\s+\"nm_world_recorder\"\s*\}\s*",
    "\n",
    text,
    flags=re.MULTILINE,
)
dst.write_text(text, encoding="utf-8")
PY
    cat >>"$WEBOTS_RECORD_WORLD_FILE" <<'EOF'

# Automatic movie recorder Supervisor added by run_webot.sh for --webots-record.
Robot {
  name "NM_WORLD_RECORDER"
  supervisor TRUE
  controller "nm_world_recorder"
}
EOF
    WORLD_FILE="$WEBOTS_RECORD_WORLD_FILE"
    echo "Recording Supervisor was not present; launching recording world copy: $WORLD_FILE"
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
    local timeout_s="${2:-$WEBOTS_CONNECT_TIMEOUT}"
    local watched_pid="${3:-}"
    if ! [[ "$timeout_s" =~ ^[0-9]+$ ]]; then
        timeout_s=60
    fi
    local deadline=$((SECONDS + timeout_s))
    while [ "$SECONDS" -lt "$deadline" ]; do
        if [ -S "$path" ]; then
            return 0
        fi
        if [ -n "$watched_pid" ] && ! kill -0 "$watched_pid" 2>/dev/null; then
            return 1
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

    if [ "$RUNTIME" = "cluster" ] && grep -Eq 'Auto-detected S=[0-9]+, O=[0-9]+' "$diag_log"; then
        # In cluster runtime, pre-Webots diagnostics can receive size hints before
        # the full controller data loop is active. Treat this as IPC-ready.
        local detected
        detected="$(grep -Eo 'Auto-detected S=[0-9]+, O=[0-9]+' "$diag_log" | tail -n 1 || true)"
        if [ -n "$detected" ]; then
            echo "[diag:$brain] $detected (IPC hint received; full round-trip deferred until controller connects)"
        else
            echo "[diag:$brain] IPC hint received; full round-trip deferred until controller connects"
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
    if [ "$WEBOTS_HEADLESS" -eq 1 ] && [ "$WEBOTS_RECORD" -eq 1 ]; then
        webots_cmd+=(--minimize)
    elif [ "$WEBOTS_HEADLESS" -eq 1 ]; then
        webots_cmd+=(--no-rendering --minimize)
    fi
    webots_cmd+=("$WORLD_FILE")

    echo "Starting Webots:"
    echo "  bin: $WEBOTS_BIN"
    echo "  world: $WORLD_FILE"
    echo "  mode: $WEBOTS_MODE"
    echo "  headless rendering: $WEBOTS_HEADLESS"
    if [ "$WEBOTS_RECORD" -eq 1 ]; then
        echo "  recording: $WEBOTS_RECORD_FILE"
    fi

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

render_recording_progress_bar() {
    local pct="$1"
    local elapsed_ms="$2"
    local total_ms="$3"
    local width=34
    local filled=$((pct * width / 100))
    local empty=$((width - filled))
    local fill=""
    local rest=""
    printf -v fill '%*s' "$filled" ''
    printf -v rest '%*s' "$empty" ''
    fill="${fill// /#}"
    rest="${rest// /-}"
    printf "\rWebots recording [%s%s] %3d%% %s/%sms" "$fill" "$rest" "$pct" "$elapsed_ms" "$total_ms"
}

watch_webots_recording_progress() {
    local log_file="$1"
    local total_ms="$2"
    local webots_pid="$3"
    local last_pct=-1
    local next_text_pct=0
    local printed_inline=0
    local line=""
    local elapsed_ms=0
    local duration_ms="$total_ms"
    local pct=0

    while kill -0 "$webots_pid" 2>/dev/null; do
        line="$(grep -a '\[nm_world_recorder\] progress ' "$log_file" 2>/dev/null | tail -n 1 || true)"
        if [[ "$line" =~ elapsed_ms=([0-9]+)[[:space:]]+duration_ms=([0-9]+)[[:space:]]+pct=([0-9]+) ]]; then
            elapsed_ms="${BASH_REMATCH[1]}"
            duration_ms="${BASH_REMATCH[2]}"
            pct="${BASH_REMATCH[3]}"
            if [ "$pct" -gt 100 ]; then
                pct=100
            fi
            if [ "$pct" -ne "$last_pct" ]; then
                if [ -t 1 ]; then
                    render_recording_progress_bar "$pct" "$elapsed_ms" "$duration_ms"
                    printed_inline=1
                elif [ "$pct" -ge "$next_text_pct" ] || [ "$pct" -ge 100 ]; then
                    echo "Webots recording progress: ${pct}% (${elapsed_ms}/${duration_ms}ms)"
                    next_text_pct=$((pct + 5))
                fi
                last_pct="$pct"
            fi
        fi
        if grep -aq 'Video creation finished\|recording ready' "$log_file" 2>/dev/null; then
            if [ "$last_pct" -lt 100 ]; then
                if [ -t 1 ]; then
                    render_recording_progress_bar 100 "$total_ms" "$total_ms"
                    printed_inline=1
                else
                    echo "Webots recording progress: 100% (${total_ms}/${total_ms}ms)"
                fi
            fi
            break
        fi
        sleep 0.5
    done

    if [ "$printed_inline" -eq 1 ]; then
        printf "\n"
    fi
}

start_webots_recording_progress() {
    if [ "$WEBOTS_RECORD" -ne 1 ] || [ "$WEBOTS_RECORD_PROGRESS" -ne 1 ]; then
        return 0
    fi
    if [ "$WEBOTS_RECORD_DURATION_MS" -le 0 ] || [ -z "${WEBOTS_PID:-}" ]; then
        return 0
    fi
    echo "Webots recording progress enabled."
    watch_webots_recording_progress "$WEBOTS_LOG" "$WEBOTS_RECORD_DURATION_MS" "$WEBOTS_PID" &
    WEBOTS_RECORD_PROGRESS_PID="$!"
    PIDS+=("$WEBOTS_RECORD_PROGRESS_PID")
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
    if [ -n "$ORCHESTRATOR_PORT" ]; then
        orch_port="$ORCHESTRATOR_PORT"
        if ! is_port_free "$orch_port"; then
            echo "Requested orchestrator port is already in use: $orch_port"
            exit 1
        fi
    else
        orch_port="$(find_free_port 50051)" || {
            echo "Failed to allocate orchestrator gRPC port"
            exit 1
        }
    fi
    reserve_port "$orch_port"

    if [ "$SINGLE_ORCHESTRATOR_UI" -eq 1 ]; then
        if [ "${#BRAINS[@]}" -ne 1 ]; then
            echo "--single-orchestrator-ui requires exactly one brain (current: $BRAIN_CSV)"
            exit 1
        fi
        local brain="${BRAINS[0]}"
        local socket_path
        socket_path="$(socket_for_brain "$brain")"
        SOCKET_PATHS["$brain"]="$socket_path"
        rm -f "$socket_path"

        local orch_log="$LOG_DIR/webots_orchestrator.log"
        local orch_cmd=(
            env
            "NM_REALTIME_IPC=$REALTIME_IPC"
            "NM_REALTIME_DISABLE_GROWTH=$REALTIME_DISABLE_GROWTH"
            "NM_REALTIME_DISABLE_MORPHO=$REALTIME_DISABLE_MORPHO"
            "NM_REALTIME_DISABLE_METABOLIC=$REALTIME_DISABLE_METABOLIC"
            "NM_REALTIME_DISABLE_PRUNING=$REALTIME_DISABLE_PRUNING"
            "NM_MORPHO_ASYNC=$MORPHO_ASYNC"
            "$bin"
            --orchestrator
            --brain-id "$brain"
            --grpc-addr "0.0.0.0:$orch_port"
            --ipc
            --ui
        )
        local brain_config
        brain_config="$(config_for_brain "$brain")"
        if [ -n "$brain_config" ]; then
            orch_cmd+=(--config "$brain_config")
        fi
        local brain_network
        brain_network="$(network_for_brain "$brain")"
        if [ -n "$brain_network" ]; then
            orch_cmd+=(--network "$brain_network")
        fi
        "${orch_cmd[@]}" >"$orch_log" 2>&1 &
        local orch_pid="$!"
        PIDS+=("$orch_pid")

        if ! wait_for_socket "$socket_path" "$WEBOTS_CONNECT_TIMEOUT" "$orch_pid"; then
            echo "Failed to bind IPC socket for brain '$brain' within ${WEBOTS_CONNECT_TIMEOUT}s: $socket_path"
            echo "See log: $orch_log"
            tail -n 40 "$orch_log" || true
            exit 1
        fi

        echo "Single-process orchestrator runtime ready:"
        echo "  brain: $brain"
        echo "  socket: $socket_path"
        echo "  gRPC: $orch_port"
        echo "  log: $orch_log"
        return 0
    fi

    local orch_log="$LOG_DIR/webots_orchestrator.log"
    local orch_specs_json=""
    orch_specs_json="$(build_orchestrator_network_specs_json local)" || {
        echo "Failed to build orchestrator startup network map."
        exit 1
    }
    local orch_cmd=(
        env
        "NM_ORCHESTRATOR_NETWORK_SPECS=$orch_specs_json"
        "NM_DISTRIBUTE_STARTUP_SNAPSHOT=$DISTRIBUTE_STARTUP_SNAPSHOT"
        "NM_DISTRIBUTED_AUTOSTART=$DISTRIBUTED_AUTOSTART"
        "NM_REALTIME_IPC=$REALTIME_IPC"
        "NM_REALTIME_DISABLE_GROWTH=$REALTIME_DISABLE_GROWTH"
        "NM_REALTIME_DISABLE_MORPHO=$REALTIME_DISABLE_MORPHO"
        "NM_REALTIME_DISABLE_METABOLIC=$REALTIME_DISABLE_METABOLIC"
        "NM_REALTIME_DISABLE_PRUNING=$REALTIME_DISABLE_PRUNING"
        "NM_MORPHO_ASYNC=$MORPHO_ASYNC"
        "$bin"
        --orchestrator
        --brain-id orchestrator
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
            env
            "NM_PRELOAD_NODE_NETWORK=$PRELOAD_NODE_NETWORK"
            "NM_REALTIME_IPC=$REALTIME_IPC"
            "NM_REALTIME_DISABLE_GROWTH=$REALTIME_DISABLE_GROWTH"
            "NM_REALTIME_DISABLE_MORPHO=$REALTIME_DISABLE_MORPHO"
            "NM_REALTIME_DISABLE_METABOLIC=$REALTIME_DISABLE_METABOLIC"
            "NM_REALTIME_DISABLE_PRUNING=$REALTIME_DISABLE_PRUNING"
            "NM_MORPHO_ASYNC=$MORPHO_ASYNC"
            "$bin"
            --node
            --brain-id "$brain"
            --grpc-addr "0.0.0.0:$node_port"
            --orchestrator-addr "http://127.0.0.1:$orch_port"
            --ipc
        )
        local brain_config
        brain_config="$(config_for_brain "$brain")"
        if [ -n "$brain_config" ]; then
            node_cmd+=(--config "$brain_config")
        fi
        local brain_network
        brain_network="$(network_for_brain "$brain")"
        if [ -n "$brain_network" ]; then
            node_cmd+=(--network "$brain_network")
        fi
        if [ "$NODE_UI" -eq 1 ]; then
            node_cmd+=(--ui)
        fi

        if [ "$NODE_UI" -eq 1 ] && [ "$NODE_UI_HIDDEN" -eq 1 ]; then
            NM_UI_HIDDEN=1 "${node_cmd[@]}" >"$log_file" 2>&1 &
        else
            "${node_cmd[@]}" >"$log_file" 2>&1 &
        fi
        local node_pid="$!"
        PIDS+=("$node_pid")

        if ! wait_for_socket "$socket_path" "$WEBOTS_CONNECT_TIMEOUT" "$node_pid"; then
            echo "Failed to bind IPC socket for brain '$brain' within ${WEBOTS_CONNECT_TIMEOUT}s: $socket_path"
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

build_remote_compute_binaries() {
    if [ "${#REMOTE_HOST_LIST[@]}" -eq 0 ]; then
        echo "No remote hosts configured for remote compute build."
        exit 1
    fi
    local host
    for host in "${REMOTE_HOST_LIST[@]}"; do
        if ! remote_reachable "$host"; then
            echo "Skipping remote build on $host (SSH unreachable)."
            continue
        fi
        echo "Syncing local source tree to $host ..."
        if ! sync_remote_source_tree "$host"; then
            echo "Remote source sync failed on $host"
            exit 1
        fi
        echo "Building remote binaries on $host ..."
        if ! remote_exec_script "$host" bash -s -- "$REMOTE_ROOT_DIR" <<'EOS'
set -euo pipefail
ROOT="$1"
if [ -f "$HOME/.cargo/env" ]; then
    # Non-interactive ssh shells skip ~/.bashrc; source cargo explicitly.
    . "$HOME/.cargo/env"
fi
export PATH="$HOME/.cargo/bin:$PATH"
if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo not found on remote host. Install Rust/cargo or ensure ~/.cargo/env exists."
    exit 127
fi

have_cmd() {
    command -v "$1" >/dev/null 2>&1
}

unique_lines() {
    awk '!seen[$0]++'
}

extract_version() {
    local path="$1"
    local base
    base="$(basename "$path")"
    if [[ "$base" =~ \.so\.([0-9]+([.][0-9]+)*)$ ]]; then
        printf '%s\n' "${BASH_REMATCH[1]}"
        return 0
    fi
    return 1
}

version_major() {
    local version="$1"
    printf '%s\n' "${version%%.*}"
}

build_search_dirs() {
    {
        printf '%s\n' \
            /usr/local/lib \
            /usr/local/lib64 \
            /usr/lib \
            /usr/lib64 \
            /lib \
            /lib64 \
            /usr/lib/x86_64-linux-gnu \
            /opt/homebrew/lib \
            /opt/local/lib
        compgen -G '/usr/lib/llvm-*/lib' || true
        compgen -G '/usr/local/llvm*/lib' || true
        compgen -G '/opt/llvm*/lib' || true
        if [[ -n "${LD_LIBRARY_PATH:-}" ]]; then
            tr ':' '\n' <<<"${LD_LIBRARY_PATH}"
        fi
    } | sed '/^$/d' | unique_lines
}

pick_best_libclang() {
    local dir candidate version major
    while IFS= read -r dir; do
        [[ -d "$dir" ]] || continue
        while IFS= read -r candidate; do
            version="$(extract_version "$candidate" || true)"
            [[ -n "$version" ]] || continue
            major="$(version_major "$version")"
            [[ "$major" =~ ^[0-9]+$ ]] || continue
            if (( major < 22 )); then
                printf '%s\t%s\n' "$version" "$candidate"
            fi
        done < <(
            find "$dir" -maxdepth 1 \( -type f -o -type l \) \
                \( -name 'libclang.so.*' -o -name 'libclang-*.so.*' \) 2>/dev/null || true
        )
    done < <(build_search_dirs) \
        | sort -t $'\t' -k1,1V \
        | tail -n 1 \
        | cut -f2-
}

find_matching_clang() {
    local lib_dir="$1"
    local candidate
    for candidate in \
        "${lib_dir%/lib}/bin/clang" \
        "${lib_dir%/lib64}/bin/clang" \
        /usr/local/bin/clang \
        /usr/bin/clang
    do
        if [[ -x "$candidate" ]]; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done
    return 1
}

find_matching_llvm_config() {
    local lib_dir="$1"
    local candidate
    for candidate in \
        "${lib_dir%/lib}/bin/llvm-config" \
        "${lib_dir%/lib64}/bin/llvm-config" \
        /usr/local/bin/llvm-config \
        /usr/bin/llvm-config
    do
        if [[ -x "$candidate" ]]; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done
    return 1
}

clang_major_version() {
    local clang_bin="$1"
    local version_line version major
    version_line="$("$clang_bin" --version 2>/dev/null | head -n 1 || true)"
    [[ -n "$version_line" ]] || return 1
    if [[ "$version_line" =~ clang[[:space:]]+version[[:space:]]+([0-9]+([.][0-9]+)*) ]]; then
        version="${BASH_REMATCH[1]}"
        major="$(version_major "$version")"
        printf '%s\n' "$major"
        return 0
    fi
    return 1
}

configure_compatible_clang_env() {
    local libclang_path
    libclang_path="$(pick_best_libclang)"
    if [[ -z "$libclang_path" ]]; then
        echo "[remote-build] compatible libclang (<22) not found; continuing with current environment"
        return 0
    fi

    local lib_dir
    lib_dir="$(dirname "$libclang_path")"
    local clang_bin llvm_config_bin clang_major
    clang_bin="$(find_matching_clang "$lib_dir" || true)"
    llvm_config_bin="$(find_matching_llvm_config "$lib_dir" || true)"
    if [[ -z "$clang_bin" ]] || [[ -z "$llvm_config_bin" ]]; then
        echo "[remote-build] matching clang/llvm-config not found for $libclang_path; continuing with current environment"
        return 0
    fi
    clang_major="$(clang_major_version "$clang_bin" || true)"
    if [[ -z "$clang_major" ]] || (( clang_major >= 22 )); then
        echo "[remote-build] clang from $clang_bin is incompatible; continuing with current environment"
        return 0
    fi

    export PATH
    PATH="$(dirname "$clang_bin"):${PATH}"
    export LIBCLANG_PATH="$libclang_path"
    export LD_LIBRARY_PATH="${lib_dir}${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
    export LLVM_CONFIG_PATH="$llvm_config_bin"
    export CLANG_PATH="$clang_bin"
    echo "[remote-build] using libclang=$LIBCLANG_PATH clang=$CLANG_PATH llvm-config=$LLVM_CONFIG_PATH"
}

configure_compatible_clang_env

if have_cmd mpicc; then
    export MPICC
    MPICC="$(command -v mpicc)"
fi
if have_cmd mpicxx; then
    export MPICXX
    MPICXX="$(command -v mpicxx)"
fi

cd "$ROOT"
REMOTE_FEATURES="${NM_REMOTE_RUNTIME_FEATURES:-growth3d,morpho}"
if [ -n "$REMOTE_FEATURES" ]; then
    cargo build --release --bin aarnn_rust --features "$REMOTE_FEATURES"
    cargo build --release --bin web_ui --features "$REMOTE_FEATURES"
else
    cargo build --release --bin aarnn_rust
    cargo build --release --bin web_ui
fi
EOS
        then
            echo "Remote build failed on $host"
            exit 1
        fi
    done
}

start_remote_cluster_runtime() {
    if [ "$RUNTIME" != "cluster" ]; then
        echo "--remote-compute currently supports --runtime cluster only."
        exit 1
    fi
    local bridge_tool="$ROOT_DIR/tools/uds_webui_bridge.py"
    if [ ! -f "$bridge_tool" ]; then
        echo "Missing bridge tool: $bridge_tool"
        exit 1
    fi
    if ! command -v python3 >/dev/null 2>&1; then
        echo "python3 is required for remote UDS/web_ui bridge mode."
        exit 1
    fi

    parse_remote_hosts
    parse_remote_weights
    init_remote_ssh

    if [ "${#REMOTE_HOST_LIST[@]}" -eq 0 ]; then
        echo "No remote hosts configured. Use --remote-hosts."
        exit 1
    fi

    local reachable_count=0
    local host
    for host in "${REMOTE_HOST_LIST[@]}"; do
        if remote_reachable "$host"; then
            reachable_count=$((reachable_count + 1))
        fi
    done
    if [ "$reachable_count" -eq 0 ]; then
        echo "None of the remote hosts are reachable over SSH as $REMOTE_USER."
        exit 1
    fi

    if [ "$REMOTE_PRE_CLEAN" -eq 1 ]; then
        remote_preclean_runtime
    fi

    if [ "$BUILD" -eq 1 ]; then
        build_remote_compute_binaries
    fi

    local orchestrator_host="$REMOTE_ORCHESTRATOR_HOST"
    if [ "$orchestrator_host" = "auto" ]; then
        orchestrator_host="$(choose_best_remote_host)"
    fi
    if [ -z "$orchestrator_host" ]; then
        echo "Failed to pick a reachable orchestrator host."
        exit 1
    fi
    if ! remote_reachable "$orchestrator_host"; then
        echo "Configured orchestrator host is unreachable: $orchestrator_host"
        exit 1
    fi

    local web_ui_host="$REMOTE_WEB_UI_HOST"
    if [ "$web_ui_host" = "off" ]; then
        echo "--remote-web-ui-host=off is unsupported with Webots bridge mode."
        exit 1
    fi
    if [ "$web_ui_host" = "auto" ]; then
        web_ui_host="$(choose_best_remote_host)"
    fi
    if [ -z "$web_ui_host" ]; then
        echo "Failed to pick a reachable web_ui host."
        exit 1
    fi
    if ! remote_reachable "$web_ui_host"; then
        echo "Configured web_ui host is unreachable: $web_ui_host"
        exit 1
    fi

    local orch_port=""
    if [ -n "$ORCHESTRATOR_PORT" ]; then
        orch_port="$ORCHESTRATOR_PORT"
        if ! remote_is_port_free "$orchestrator_host" "$orch_port"; then
            echo "Configured orchestrator port is already in use on $orchestrator_host:$orch_port"
            exit 1
        fi
    else
        orch_port="$(find_remote_free_port "$orchestrator_host" 50051)" || {
            echo "Failed to allocate remote orchestrator port on $orchestrator_host"
            exit 1
        }
    fi
    ORCHESTRATOR_PORT="$orch_port"
    local orchestrator_addr_public="http://$orchestrator_host:$orch_port"

    local web_ui_api_port="$REMOTE_WEB_UI_PORT"
    local web_ui_api_url=""
    local browser_ui_url="http://$web_ui_host:$REMOTE_WEB_UI_PORT"
    local remote_runtime_root="$WEB_UI_RUNTIME_ROOT"
    remote_runtime_root="$(remote_path_for_local "$remote_runtime_root")"
    local remote_workspace_bindings_json=""
    if [ -n "${NM_RUNTIME_WORKSPACE_BINDINGS:-}" ]; then
        remote_workspace_bindings_json="$(build_remote_workspace_bindings_json)" || {
            echo "Failed to translate runtime workspace bindings for remote hosts."
            exit 1
        }
    fi
    if [ -n "$REMOTE_WEB_UI_API_PORT" ] && [ "$REMOTE_WEB_UI_API_PORT" != "$REMOTE_WEB_UI_PORT" ]; then
        echo "Ignoring --remote-web-ui-api-port=$REMOTE_WEB_UI_API_PORT in web mode."
    fi
    if ! remote_is_port_free "$web_ui_host" "$REMOTE_WEB_UI_PORT"; then
        echo "Remote web_ui port is already in use on $web_ui_host:$REMOTE_WEB_UI_PORT"
        echo "Set --remote-web-ui-port to a free port."
        exit 1
    fi

    local orch_specs_json=""
    orch_specs_json="$(build_orchestrator_network_specs_json remote)" || {
        echo "Failed to build remote orchestrator startup network map."
        exit 1
    }

    local orch_cmd=(
        env
        "NM_DISTRIBUTE_STARTUP_SNAPSHOT=$DISTRIBUTE_STARTUP_SNAPSHOT"
        "NM_DISTRIBUTED_AUTOSTART=$DISTRIBUTED_AUTOSTART"
        "NM_ORCHESTRATOR_NETWORK_SPECS=$orch_specs_json"
        "NM_REALTIME_IPC=$REALTIME_IPC"
        "NM_REALTIME_DISABLE_GROWTH=$REALTIME_DISABLE_GROWTH"
        "NM_REALTIME_DISABLE_MORPHO=$REALTIME_DISABLE_MORPHO"
        "NM_REALTIME_DISABLE_METABOLIC=$REALTIME_DISABLE_METABOLIC"
        "NM_REALTIME_DISABLE_PRUNING=$REALTIME_DISABLE_PRUNING"
        "NM_MORPHO_ASYNC=$MORPHO_ASYNC"
        "NM_REALTIME_MORPHO_INTERVAL_MS=$REALTIME_MORPHO_INTERVAL_MS"
        "NM_REALTIME_METABOLIC_INTERVAL_MS=$REALTIME_METABOLIC_INTERVAL_MS"
        "NM_REALTIME_MORPHO_MAX_SYNAPSES=$REALTIME_MORPHO_MAX_SYNAPSES"
        target/release/aarnn_rust
        --orchestrator
        --brain-id orchestrator
        --grpc-addr "0.0.0.0:$orch_port"
    )
    if [ "$REMOTE_QUIET" -eq 1 ]; then
        orch_cmd+=(--quiet)
    fi
    local orch_cmd_str
    orch_cmd_str="$(cmd_to_string "${orch_cmd[@]}")"
    remote_start_bg "$orchestrator_host" "remote_orchestrator" "$orch_cmd_str" "$REMOTE_LOG_DIR" >/dev/null

    if ! wait_for_remote_port "$orchestrator_host" "$orch_port" 40; then
        echo "Remote orchestrator did not open $orchestrator_host:$orch_port in time."
        exit 1
    fi

    local node_port_start=50070
    local launched_nodes=0
    local brain
    for brain in "${BRAINS[@]}"; do
        local remote_network
        remote_network="$(remote_network_for_brain "$brain")"
        local remote_config
        remote_config="$(remote_config_for_brain "$brain")"
        for host in "${REMOTE_HOST_LIST[@]}"; do
            if ! remote_reachable "$host"; then
                echo "Skipping remote node on $host (SSH unreachable)."
                continue
            fi
            local node_port
            node_port="$(find_remote_free_port "$host" "$node_port_start")" || {
                echo "Failed to allocate remote node port on $host for brain '$brain'"
                exit 1
            }
            node_port_start=$((node_port + 7))
            local node_weight
            node_weight="$(remote_weight_for_host "$host")"
            local node_cmd=(
                env "NM_CAPACITY_MULTIPLIER=$node_weight" "NM_PRELOAD_NODE_NETWORK=$PRELOAD_NODE_NETWORK" \
                    "NM_REALTIME_IPC=$REALTIME_IPC" \
                    "NM_REALTIME_DISABLE_GROWTH=$REALTIME_DISABLE_GROWTH" \
                    "NM_REALTIME_DISABLE_MORPHO=$REALTIME_DISABLE_MORPHO" \
                    "NM_REALTIME_DISABLE_METABOLIC=$REALTIME_DISABLE_METABOLIC" \
                    "NM_REALTIME_DISABLE_PRUNING=$REALTIME_DISABLE_PRUNING" \
                    "NM_MORPHO_ASYNC=$MORPHO_ASYNC" \
                    "NM_REALTIME_MORPHO_INTERVAL_MS=$REALTIME_MORPHO_INTERVAL_MS" \
                    "NM_REALTIME_METABOLIC_INTERVAL_MS=$REALTIME_METABOLIC_INTERVAL_MS" \
                    "NM_REALTIME_MORPHO_MAX_SYNAPSES=$REALTIME_MORPHO_MAX_SYNAPSES"
                target/release/aarnn_rust
                --node
                --brain-id "$brain"
                --grpc-addr "0.0.0.0:$node_port"
                --orchestrator-addr "$orchestrator_addr_public"
            )
            if [ -n "$remote_workspace_bindings_json" ]; then
                node_cmd=("env" "NM_CAPACITY_MULTIPLIER=$node_weight" "NM_PRELOAD_NODE_NETWORK=$PRELOAD_NODE_NETWORK" \
                    "NM_REALTIME_IPC=$REALTIME_IPC" \
                    "NM_REALTIME_DISABLE_GROWTH=$REALTIME_DISABLE_GROWTH" \
                    "NM_REALTIME_DISABLE_MORPHO=$REALTIME_DISABLE_MORPHO" \
                    "NM_REALTIME_DISABLE_METABOLIC=$REALTIME_DISABLE_METABOLIC" \
                    "NM_REALTIME_DISABLE_PRUNING=$REALTIME_DISABLE_PRUNING" \
                    "NM_MORPHO_ASYNC=$MORPHO_ASYNC" \
                    "NM_REALTIME_MORPHO_INTERVAL_MS=$REALTIME_MORPHO_INTERVAL_MS" \
                    "NM_REALTIME_METABOLIC_INTERVAL_MS=$REALTIME_METABOLIC_INTERVAL_MS" \
                    "NM_REALTIME_MORPHO_MAX_SYNAPSES=$REALTIME_MORPHO_MAX_SYNAPSES" \
                    "NM_RUNTIME_WORKSPACE_BINDINGS=$remote_workspace_bindings_json" \
                    target/release/aarnn_rust
                    --node
                    --brain-id "$brain"
                    --grpc-addr "0.0.0.0:$node_port"
                    --orchestrator-addr "$orchestrator_addr_public")
            fi
            if [ "$REMOTE_QUIET" -eq 1 ]; then
                node_cmd+=(--quiet)
            fi
            if [ -n "$remote_config" ]; then
                node_cmd+=(--config "$remote_config")
            fi
            if [ -n "$remote_network" ]; then
                node_cmd+=(--network "$remote_network")
            fi
            local node_cmd_str
            node_cmd_str="$(cmd_to_string "${node_cmd[@]}")"
            remote_start_bg "$host" "remote_node_${brain}_${node_port}" "$node_cmd_str" "$REMOTE_LOG_DIR" >/dev/null
            NODE_PORTS["${brain}@${host}"]="$node_port"
            launched_nodes=$((launched_nodes + 1))
        done
    done
    if [ "$launched_nodes" -eq 0 ]; then
        echo "No remote nodes were started."
        exit 1
    fi

    local web_ui_orchestrator_addr="$orchestrator_addr_public"
    local web_ui_cmd=()
    if [ -n "$WEB_UI_DEFAULT_RUNTIME_USER" ]; then
        web_ui_cmd=(env "NM_WEB_UI_DEFAULT_RUNTIME_USER=$WEB_UI_DEFAULT_RUNTIME_USER" \
            "NM_WEB_UI_RUNTIME_ROOT=$remote_runtime_root" \
            target/release/web_ui
            --listen "0.0.0.0:$web_ui_api_port"
            --orchestrator "$web_ui_orchestrator_addr"
            --auth-mode none)
    else
        web_ui_cmd=(env
            "NM_WEB_UI_RUNTIME_ROOT=$remote_runtime_root"
            target/release/web_ui
            --listen "0.0.0.0:$web_ui_api_port"
            --orchestrator "$web_ui_orchestrator_addr"
            --auth-mode none)
    fi
    local web_ui_cmd_str
    web_ui_cmd_str="$(cmd_to_string "${web_ui_cmd[@]}")"
    remote_start_bg "$web_ui_host" "remote_web_ui_cluster" "$web_ui_cmd_str" "$REMOTE_LOG_DIR" >/dev/null

    web_ui_api_url="http://$web_ui_host:$web_ui_api_port"
    if ! wait_for_http_ready "$web_ui_api_url/api/config" 40; then
        echo "Remote web_ui API did not become ready at $web_ui_api_url"
        exit 1
    fi
    for brain in "${BRAINS[@]}"; do
        if ! wait_for_cluster_distribution "$web_ui_api_url" "$web_ui_orchestrator_addr" "$brain" "$WEBOTS_CLUSTER_DISTRIBUTION_TIMEOUT"; then
            echo "Cluster distribution for network '$brain' was not ready within ${WEBOTS_CLUSTER_DISTRIBUTION_TIMEOUT}s."
            print_cluster_status_debug "$web_ui_api_url" "$web_ui_orchestrator_addr" "$brain" || true
            echo "Hint: rerun with NM_REMOTE_QUIET=0 for verbose remote process output."
            exit 1
        fi
    done

    if [ "$LOCAL_RUST_UI" -eq 1 ]; then
        local first_brain="${BRAINS[0]}"
        if ! start_local_rust_ui_client "$orchestrator_addr_public" "$first_brain"; then
            echo "Failed to launch local native rust_ui client."
            exit 1
        fi
    fi

    local -A bridge_logs=()
    for brain in "${BRAINS[@]}"; do
        local socket_path
        socket_path="$(socket_for_brain "$brain")"
        SOCKET_PATHS["$brain"]="$socket_path"
        rm -f "$socket_path"

        local bridge_log="$LOG_DIR/webots_bridge_${brain}.log"
        local bridge_cmd=(
            python3 "$bridge_tool"
            --socket "$socket_path"
            --web-ui-url "$web_ui_api_url"
            --orchestrator "$orchestrator_addr_public"
            --network-id "$brain"
            --threshold "$THRESHOLD"
            --default-s "$DEFAULT_S"
            --default-o "$DEFAULT_O"
        )
        "${bridge_cmd[@]}" >"$bridge_log" 2>&1 &
        local bridge_pid="$!"
        PIDS+=("$bridge_pid")
        if ! wait_for_socket "$socket_path" "$WEBOTS_CONNECT_TIMEOUT" "$bridge_pid"; then
            echo "Failed to bind local bridge socket for brain '$brain' within ${WEBOTS_CONNECT_TIMEOUT}s: $socket_path"
            echo "See log: $bridge_log"
            tail -n 60 "$bridge_log" || true
            exit 1
        fi
        bridge_logs["$brain"]="$bridge_log"
    done

    echo "Remote compute runtime ready:"
    echo "  brains: $BRAIN_CSV"
    echo "  webots host (local): $REMOTE_WEBOTS_HOST"
    echo "  orchestrator host: $orchestrator_host:$orch_port"
    echo "  browser ui: $browser_ui_url"
    echo "  web_ui api: $web_ui_api_url"
    if [ "$LOCAL_RUST_UI" -eq 1 ]; then
        echo "  local rust_ui: enabled"
        echo "  local rust_ui log: $LOCAL_RUST_UI_LOG"
    else
        echo "  local rust_ui: disabled"
    fi
    echo "  remote root: $REMOTE_ROOT_DIR"
    echo "  remote logs: $REMOTE_LOG_DIR"
    for brain in "${BRAINS[@]}"; do
        echo "  local bridge socket [$brain]: ${SOCKET_PATHS[$brain]}"
    done
    for brain in "${BRAINS[@]}"; do
        echo "  local bridge log [$brain]: ${bridge_logs[$brain]}"
    done
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

        local uds_cmd=(
            env
            "NM_REALTIME_IPC=$REALTIME_IPC"
            "NM_REALTIME_DISABLE_GROWTH=$REALTIME_DISABLE_GROWTH"
            "NM_REALTIME_DISABLE_MORPHO=$REALTIME_DISABLE_MORPHO"
            "NM_REALTIME_DISABLE_METABOLIC=$REALTIME_DISABLE_METABOLIC"
            "NM_REALTIME_DISABLE_PRUNING=$REALTIME_DISABLE_PRUNING"
            "NM_MORPHO_ASYNC=$MORPHO_ASYNC"
            "NM_REALTIME_MORPHO_INTERVAL_MS=$REALTIME_MORPHO_INTERVAL_MS"
            "NM_REALTIME_METABOLIC_INTERVAL_MS=$REALTIME_METABOLIC_INTERVAL_MS"
            "NM_REALTIME_MORPHO_MAX_SYNAPSES=$REALTIME_MORPHO_MAX_SYNAPSES"
            "$bin"
            --socket "$socket_path"
            --sensory "$DEFAULT_S"
            --output "$DEFAULT_O"
            --threshold "$THRESHOLD"
            --aer-sensory-base "$AER_S_BASE"
            --aer-output-base "$AER_O_BASE"
        )
        local brain_config
        brain_config="$(config_for_brain "$brain")"
        if [ -n "$brain_config" ]; then
            uds_cmd+=(--config "$brain_config")
        fi
        local brain_network
        brain_network="$(network_for_brain "$brain")"
        if [ -n "$brain_network" ]; then
            uds_cmd+=(--network "$brain_network")
        fi
        "${uds_cmd[@]}" >"$log_file" 2>&1 &
        local uds_pid="$!"
        PIDS+=("$uds_pid")

        if ! wait_for_socket "$socket_path" "$WEBOTS_CONNECT_TIMEOUT" "$uds_pid"; then
            echo "Failed to bind socket for brain '$brain' within ${WEBOTS_CONNECT_TIMEOUT}s: $socket_path"
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

prepare_recording_world
parse_world_controller_args "$WORLD_FILE"

if [ -z "$BRAIN_CSV" ]; then
    BRAIN_CSV="vision,motor"
fi
IFS=',' read -r -a BRAINS <<< "$BRAIN_CSV"
if [ "${#BRAINS[@]}" -eq 0 ]; then
    echo "No brains configured; set --brains or NM_BRAINS."
    exit 1
fi

if [ -n "$CONFIG_FILE" ]; then
    CONFIG_FILE="$(abs_path_from_root "$CONFIG_FILE")"
    if [ ! -f "$CONFIG_FILE" ]; then
        echo "Config file not found: $CONFIG_FILE"
        exit 1
    fi
fi
if [ -n "$NETWORK_FILE" ]; then
    NETWORK_FILE="$(abs_path_from_root "$NETWORK_FILE")"
    if [ ! -f "$NETWORK_FILE" ]; then
        echo "Network snapshot not found: $NETWORK_FILE"
        exit 1
    fi
fi

parse_brain_file_map "$CONFIG_MAP_CSV" CONFIG_FILE_MAP "config"
parse_brain_file_map "$NETWORK_MAP_CSV" NETWORK_FILE_MAP "network"
canonicalize_and_validate_map_paths CONFIG_FILE_MAP "Config"
canonicalize_and_validate_map_paths NETWORK_FILE_MAP "Network snapshot"

declare -A _BRAIN_SET=()
for brain in "${BRAINS[@]}"; do
    _BRAIN_SET["$brain"]=1
done

for brain in "${!CONFIG_FILE_MAP[@]}"; do
    if [ -z "${_BRAIN_SET[$brain]+x}" ]; then
        echo "Warning: config map contains '$brain' but it is not in --brains ($BRAIN_CSV)."
    fi
done
for brain in "${!NETWORK_FILE_MAP[@]}"; do
    if [ -z "${_BRAIN_SET[$brain]+x}" ]; then
        echo "Warning: network map contains '$brain' but it is not in --brains ($BRAIN_CSV)."
    fi
done
unset _BRAIN_SET

if [ -n "$CONFIG_FILE" ] || [ "${#CONFIG_FILE_MAP[@]}" -gt 0 ]; then
    for brain in "${BRAINS[@]}"; do
        if [ -z "$(config_for_brain "$brain")" ]; then
            echo "No config file resolved for brain '$brain' (set --config or --config-map)."
            exit 1
        fi
    done
fi
if [ -n "$NETWORK_FILE" ] || [ "${#NETWORK_FILE_MAP[@]}" -gt 0 ]; then
    for brain in "${BRAINS[@]}"; do
        if [ -z "$(network_for_brain "$brain")" ]; then
            echo "No network file resolved for brain '$brain' (set --network or --network-map)."
            exit 1
        fi
    done
fi

if [ "$REMOTE_COMPUTE" -eq 1 ] && [ "$RUNTIME" != "cluster" ]; then
    echo "--remote-compute requires --runtime cluster."
    exit 1
fi

if [ "$RUNTIME" = "cluster" ] && [ "$REMOTE_COMPUTE" -eq 0 ] && [ "$NODE_UI" -eq 1 ] && [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
    echo "No display detected (DISPLAY/WAYLAND_DISPLAY unset)."
    echo "Falling back to --runtime uds for headless compatibility."
    RUNTIME="uds"
fi

if [ "$START_WEBOTS" -eq 1 ] && [ "$WEBOTS_HEADLESS" -eq 0 ] && [ "$WEBOTS_RECORD" -eq 0 ] && [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
    echo "No display detected for Webots; enabling --webots-headless mode."
    WEBOTS_HEADLESS=1
fi
if [ "$START_WEBOTS" -eq 1 ] && [ "$WEBOTS_RECORD" -eq 1 ] && [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
    echo "Webots recording requested without DISPLAY/WAYLAND_DISPLAY; use a desktop or virtual framebuffer so rendered movie frames are available."
fi

mkdir -p "$LOG_DIR"
if [ "$WEBOTS_RECORD" -eq 1 ]; then
    if [ -z "$WEBOTS_RECORD_FILE" ]; then
        WEBOTS_RECORD_FILE="$LOG_DIR/webots_recording.mp4"
    fi
    case "$WEBOTS_RECORD_FILE" in
        /*) ;;
        *) WEBOTS_RECORD_FILE="$ROOT_DIR/$WEBOTS_RECORD_FILE" ;;
    esac
    export NM_WEBOTS_RECORD=1
    export NM_WEBOTS_RECORD_FILE="$WEBOTS_RECORD_FILE"
    export NM_WEBOTS_RECORD_WIDTH="$WEBOTS_RECORD_WIDTH"
    export NM_WEBOTS_RECORD_HEIGHT="$WEBOTS_RECORD_HEIGHT"
    export NM_WEBOTS_RECORD_DURATION_MS="$WEBOTS_RECORD_DURATION_MS"
    export NM_WEBOTS_RECORD_QUALITY="$WEBOTS_RECORD_QUALITY"
    export NM_WEBOTS_RECORD_ACCELERATION="$WEBOTS_RECORD_ACCELERATION"
    export NM_WEBOTS_RECORD_CODEC="$WEBOTS_RECORD_CODEC"
    export NM_WEBOTS_RECORD_CAPTION="$WEBOTS_RECORD_CAPTION"
    export NM_WEBOTS_RECORD_PROGRESS="$WEBOTS_RECORD_PROGRESS"
    export NM_WEBOTS_RECORD_PROGRESS_INTERVAL_MS="$WEBOTS_RECORD_PROGRESS_INTERVAL_MS"
    export NM_WEBOTS_RECORD_DIR="$LOG_DIR"
    if [ -n "$WEBOTS_RECORD_QUIT_ON_DONE" ]; then
        export NM_WEBOTS_RECORD_QUIT_ON_DONE="$WEBOTS_RECORD_QUIT_ON_DONE"
    fi
else
    export NM_WEBOTS_RECORD=0
fi

echo "Webots bridge launch config:"
echo "  runtime: $RUNTIME"
echo "  world: $WORLD_FILE"
echo "  brains: $BRAIN_CSV"
echo "  interconnect: ${INTERCONNECT:-none}"
echo "  pre-handshake fallback S/O: $DEFAULT_S/$DEFAULT_O"
echo "  AER base S/O: $AER_S_BASE/$AER_O_BASE"
echo "  UDS recv timeout (ms): $UDS_RECV_TIMEOUT_MS"
echo "  IPC timeout grace/log interval (ms): $IPC_TIMEOUT_GRACE_MS/$IPC_TIMEOUT_LOG_INTERVAL_MS"
echo "  IPC UDS ctrl buffer (bytes): $IPC_UDS_CTRL_BUF_BYTES"
echo "  IPC window min/init/max: $IPC_WINDOW_MIN/$IPC_WINDOW_INIT/$IPC_WINDOW_MAX"
echo "  IPC send budget max: $IPC_SEND_BUDGET_MAX (strict_lockstep=$IPC_STRICT_LOCKSTEP)"
echo "  webots extra step sleep (ms): $WEBOTS_STEP_SLEEP_MS"
if [ "$WEBOTS_RECORD" -eq 1 ]; then
    echo "  webots recording: $WEBOTS_RECORD_FILE (${WEBOTS_RECORD_WIDTH}x${WEBOTS_RECORD_HEIGHT}, duration_ms=$WEBOTS_RECORD_DURATION_MS)"
    echo "  webots recording progress: $WEBOTS_RECORD_PROGRESS (interval_ms=$WEBOTS_RECORD_PROGRESS_INTERVAL_MS)"
fi
echo "  config file: ${CONFIG_FILE:-none}"
echo "  network file: ${NETWORK_FILE:-none}"
if [ "${#CONFIG_FILE_MAP[@]}" -gt 0 ]; then
    echo "  config map: $(map_to_csv CONFIG_FILE_MAP)"
fi
if [ "${#NETWORK_FILE_MAP[@]}" -gt 0 ]; then
    echo "  network map: $(map_to_csv NETWORK_FILE_MAP)"
fi
echo "  orchestrator port: ${ORCHESTRATOR_PORT:-auto}"
echo "  node ui hidden: $NODE_UI_HIDDEN"
echo "  realtime policy: $REALTIME_POLICY"
echo "  realtime ipc policy: $REALTIME_IPC (growth=$REALTIME_DISABLE_GROWTH morpho=$REALTIME_DISABLE_MORPHO metabolic=$REALTIME_DISABLE_METABOLIC pruning=$REALTIME_DISABLE_PRUNING)"
echo "  morpho async worker: $MORPHO_ASYNC"
if [ -n "$REALTIME_MORPHO_INTERVAL_MS" ]; then
    echo "  realtime morpho interval override (ms): $REALTIME_MORPHO_INTERVAL_MS"
fi
if [ -n "$REALTIME_METABOLIC_INTERVAL_MS" ]; then
    echo "  realtime metabolic interval override (ms): $REALTIME_METABOLIC_INTERVAL_MS"
fi
if [ -n "$REALTIME_MORPHO_MAX_SYNAPSES" ]; then
    echo "  realtime morpho safe max synapses: $REALTIME_MORPHO_MAX_SYNAPSES"
fi
    echo "  single orchestrator ui: $SINGLE_ORCHESTRATOR_UI"
    echo "  distribute startup snapshot: $DISTRIBUTE_STARTUP_SNAPSHOT"
    echo "  distributed autostart: $DISTRIBUTED_AUTOSTART"
    echo "  remote compute: $REMOTE_COMPUTE"
if [ "$REMOTE_COMPUTE" -eq 1 ]; then
    echo "  remote hosts: $REMOTE_HOSTS"
    echo "  remote host weights: ${REMOTE_HOST_WEIGHTS:-auto(resource-based)}"
    echo "  remote user: $REMOTE_USER"
    echo "  remote root: $REMOTE_ROOT_DIR"
    echo "  remote orchestrator host: $REMOTE_ORCHESTRATOR_HOST"
    echo "  remote web_ui host: $REMOTE_WEB_UI_HOST"
    echo "  remote ui mode: $REMOTE_UI_MODE"
    echo "  remote web_ui port: $REMOTE_WEB_UI_PORT"
    if command -v rclone >/dev/null 2>&1; then
        echo "  remote sync backend: rclone (rsync fallback)"
    else
        echo "  remote sync backend: rsync"
    fi
    echo "  remote sync data: $REMOTE_SYNC_DATA"
    echo "  remote rsync compression: $REMOTE_RSYNC_COMPRESS"
    echo "  remote runtime features: ${NM_REMOTE_RUNTIME_FEATURES:-growth3d,morpho}"
    if [ -n "$REMOTE_WEB_UI_API_PORT" ]; then
        echo "  remote web_ui api port: ${REMOTE_WEB_UI_API_PORT} (ignored in web mode)"
    fi
    echo "  local rust_ui: $LOCAL_RUST_UI"
    echo "  remote quiet mode: $REMOTE_QUIET"
    echo "  remote pre-clean: $REMOTE_PRE_CLEAN"
    echo "  local webots host label: $REMOTE_WEBOTS_HOST"
    echo "  cluster distribution timeout (s): $WEBOTS_CLUSTER_DISTRIBUTION_TIMEOUT"
fi
echo "  start webots: $START_WEBOTS"
if [ "$START_WEBOTS" -eq 1 ]; then
    echo "  webots mode: $WEBOTS_MODE"
    echo "  webots headless rendering: $WEBOTS_HEADLESS"
    echo "  skip controller build: $SKIP_CONTROLLER_BUILD"
    echo "  connect timeout (s): $WEBOTS_CONNECT_TIMEOUT"
fi
echo

if [ "$BUILD" -eq 1 ]; then
    if [ "$REMOTE_COMPUTE" -eq 1 ]; then
        echo "Remote compute mode selected: building on remote hosts during remote startup."
        if [ "$LOCAL_RUST_UI" -eq 1 ]; then
            echo "Building local rust_ui binary for native local display..."
            cargo build --release --bin aarnn_rust --all-features
        fi
    elif [ "$RUNTIME" = "cluster" ]; then
        echo "Building aarnn_rust binary..."
        cargo build --release --bin aarnn_rust --all-features
    else
        echo "Building nn_uds_server example..."
        cargo build --release --example nn_uds_server --features ui,robot_io
    fi
fi

if [ "$REMOTE_COMPUTE" -eq 1 ]; then
    start_remote_cluster_runtime
elif [ "$RUNTIME" = "cluster" ]; then
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
        if [ "$RUNTIME" = "cluster" ] && [ "$START_WEBOTS" -eq 1 ]; then
            echo "Diagnostics failed for $diag_failures brain(s) before Webots start."
            echo "Continuing because cluster sockets are up; controller connectivity check is authoritative."
        else
            echo "Diagnostics failed for $diag_failures brain(s)."
            exit 1
        fi
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
if [ "$REMOTE_COMPUTE" -eq 1 ]; then
    echo "Webots remote cluster runtime is running (remote orchestrator + remote nodes + local bridge)."
    echo "web_ui is running on the selected remote host for cluster visibility/control."
    if [ "$LOCAL_RUST_UI" -eq 1 ]; then
        echo "Local native rust_ui is running and auto-attached to the remote orchestrator."
    fi
elif [ "$RUNTIME" = "cluster" ]; then
    echo "Webots cluster runtime is running (orchestrator + IPC nodes)."
    echo "Use orchestrator UI for cluster controls; each brain node hosts IPC on its socket."
else
    echo "Webots UDS runtime is running (per-brain nn_uds_server)."
fi
echo "Handshake auto-remap is enabled for UDS servers and IPC mismatch hints are active."
echo "Press Ctrl+C to stop."

start_webots_recording_progress

if [ "$START_WEBOTS" -eq 1 ] && [ -n "$WEBOTS_PID" ]; then
    wait "$WEBOTS_PID" || true
else
    wait
fi
