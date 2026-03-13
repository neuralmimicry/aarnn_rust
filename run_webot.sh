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
            "${REMOTE_SSH_ARGV[@]}" "${REMOTE_USER}@${host}" "kill -TERM $pid" >/dev/null 2>&1 || true
            "${REMOTE_SSH_ARGV[@]}" "${REMOTE_USER}@${host}" "sleep 0.2; kill -KILL $pid" >/dev/null 2>&1 || true
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
  --config <path>          NetworkConfig JSON to load in backend nodes/servers.
  --network <path>         Snapshot JSON to import in backend nodes/servers.
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
                           Rsync compression policy (default: auto).
                           auto disables compression for RFC1918 LAN hosts.
  --remote-ssh-opts <str>  Extra SSH options appended to remote launch commands.
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
  SKIP_CONTROLLER_BUILD, NM_CONFIG_FILE, NM_NETWORK_FILE, NM_ORCHESTRATOR_PORT,
  NM_NODE_UI_HIDDEN, NM_REMOTE_COMPUTE, NM_REMOTE_HOSTS, NM_REMOTE_HOST_WEIGHTS,
  NM_REMOTE_USER, NM_REMOTE_ROOT, NM_REMOTE_ORCHESTRATOR_HOST, NM_REMOTE_WEB_UI_HOST,
  NM_REMOTE_WEB_UI_PORT, NM_REMOTE_WEB_UI_API_PORT, NM_REMOTE_UI_MODE,
  NM_LOCAL_RUST_UI, NM_REMOTE_WEBOTS_HOST, NM_REMOTE_SSH_OPTS, NM_REMOTE_LOG_DIR,
  NM_REMOTE_PRE_CLEAN, NM_REMOTE_SYNC_DATA, NM_REMOTE_RSYNC_COMPRESS.
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
CONFIG_FILE="${NM_CONFIG_FILE:-}"
NETWORK_FILE="${NM_NETWORK_FILE:-}"
ORCHESTRATOR_PORT="${NM_ORCHESTRATOR_PORT:-}"
NODE_UI_HIDDEN="${NM_NODE_UI_HIDDEN:-0}"
SINGLE_ORCHESTRATOR_UI="${NM_SINGLE_ORCHESTRATOR_UI:-0}"
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

rsync_supports_option() {
    local option="$1"
    rsync --help 2>/dev/null | grep -q -- "$option"
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
NODE_UI_HIDDEN="$(normalize_bool NODE_UI_HIDDEN "$NODE_UI_HIDDEN")" || exit 1
SINGLE_ORCHESTRATOR_UI="$(normalize_bool SINGLE_ORCHESTRATOR_UI "$SINGLE_ORCHESTRATOR_UI")" || exit 1
REMOTE_COMPUTE="$(normalize_bool REMOTE_COMPUTE "$REMOTE_COMPUTE")" || exit 1
REMOTE_QUIET="$(normalize_bool REMOTE_QUIET "$REMOTE_QUIET")" || exit 1
REMOTE_PRE_CLEAN="$(normalize_bool REMOTE_PRE_CLEAN "$REMOTE_PRE_CLEAN")" || exit 1
LOCAL_RUST_UI="$(normalize_bool LOCAL_RUST_UI "$LOCAL_RUST_UI")" || exit 1

if [ "$WEBOTS_MODE" != "pause" ] && [ "$WEBOTS_MODE" != "realtime" ] && [ "$WEBOTS_MODE" != "fast" ]; then
    echo "Invalid --webots-mode '$WEBOTS_MODE' (must be pause, realtime, or fast)."
    exit 1
fi

if ! [[ "$WEBOTS_CONNECT_TIMEOUT" =~ ^[0-9]+$ ]]; then
    echo "Invalid --connect-timeout '$WEBOTS_CONNECT_TIMEOUT' (must be a non-negative integer)."
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

sync_remote_source_tree() {
    local host="$1"
    if ! command -v rsync >/dev/null 2>&1; then
        echo "rsync not found locally; skipping source sync to $host."
        return 0
    fi
    if [ -z "$REMOTE_USER" ]; then
        echo "Missing remote user for source sync."
        return 1
    fi
    remote_exec_script "$host" "mkdir -p \"$REMOTE_ROOT_DIR\"" >/dev/null 2>&1 || return 1

    local use_compress="$REMOTE_RSYNC_COMPRESS"
    if [ "$use_compress" = "auto" ]; then
        case "$host" in
            10.*|192.168.*|172.1[6-9].*|172.2[0-9].*|172.3[0-1].*)
                use_compress="off"
                ;;
            *)
                use_compress="on"
                ;;
        esac
    fi
    echo "  rsync options for $host: compression=$use_compress"

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
    "${rsync_common[@]}" \
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
        "${rsync_common[@]}" \
            -e "${REMOTE_SSH_ARGV[*]}" \
            "$ROOT_DIR/data/" \
            "${REMOTE_USER}@${host}:$REMOTE_ROOT_DIR/data/" || return 1
        remote_exec_script "$host" \
            "date -u +%FT%TZ > \"$REMOTE_ROOT_DIR/.nm_data_synced\"" >/dev/null 2>&1 || true
    fi
}

remote_reachable() {
    local host="$1"
    "${REMOTE_SSH_ARGV[@]}" "${REMOTE_USER}@${host}" "echo ok" >/dev/null 2>&1
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
    "${REMOTE_SSH_ARGV[@]}" "${REMOTE_USER}@${host}" "$@"
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
    local timeout_s="${4:-35}"
    local deadline=$((SECONDS + timeout_s))
    while [ "$SECONDS" -lt "$deadline" ]; do
        if python3 - "$web_ui_url" "$orchestrator_addr" "$network_id" <<'PY' >/dev/null 2>&1
import json
import sys
import urllib.parse
import urllib.request

web_ui_url = sys.argv[1].rstrip("/")
addr = sys.argv[2]
network_id = sys.argv[3]
query = urllib.parse.urlencode({"addr": addr})
url = f"{web_ui_url}/api/status?{query}"
with urllib.request.urlopen(url, timeout=1.2) as resp:
    payload = json.loads(resp.read().decode("utf-8", errors="replace") or "{}")
for net in payload.get("networks", []):
    if str(net.get("network_id", "")) != network_id:
        continue
    dist = net.get("distribution", [])
    if isinstance(dist, list) and len(dist) > 0:
        sys.exit(0)
sys.exit(1)
PY
        then
            return 0
        fi
        sleep 0.5
    done
    return 1
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
    if [ -n "$CONFIG_FILE" ]; then
        rust_ui_cmd+=(--config "$CONFIG_FILE")
    fi
    if [ -n "$NETWORK_FILE" ]; then
        rust_ui_cmd+=(--network "$NETWORK_FILE")
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
            "$bin"
            --orchestrator
            --brain-id "$brain"
            --grpc-addr "0.0.0.0:$orch_port"
            --ipc
            --ui
        )
        if [ -n "$CONFIG_FILE" ]; then
            orch_cmd+=(--config "$CONFIG_FILE")
        fi
        if [ -n "$NETWORK_FILE" ]; then
            orch_cmd+=(--network "$NETWORK_FILE")
        fi
        "${orch_cmd[@]}" >"$orch_log" 2>&1 &
        PIDS+=("$!")

        if ! wait_for_socket "$socket_path"; then
            echo "Failed to bind IPC socket for brain '$brain': $socket_path"
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
    local orch_cmd=(
        "$bin"
        --orchestrator
        --brain-id cluster_master
        --grpc-addr "0.0.0.0:$orch_port"
    )
    if [ -n "$CONFIG_FILE" ]; then
        orch_cmd+=(--config "$CONFIG_FILE")
    fi
    if [ -n "$NETWORK_FILE" ]; then
        orch_cmd+=(--network "$NETWORK_FILE")
    fi
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
        if [ -n "$CONFIG_FILE" ]; then
            node_cmd+=(--config "$CONFIG_FILE")
        fi
        if [ -n "$NETWORK_FILE" ]; then
            node_cmd+=(--network "$NETWORK_FILE")
        fi
        if [ "$NODE_UI" -eq 1 ]; then
            node_cmd+=(--ui)
        fi

        if [ "$NODE_UI" -eq 1 ] && [ "$NODE_UI_HIDDEN" -eq 1 ]; then
            NM_UI_HIDDEN=1 "${node_cmd[@]}" >"$log_file" 2>&1 &
        else
            "${node_cmd[@]}" >"$log_file" 2>&1 &
        fi
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
cargo build --release --bin aarnn_rust
cargo build --release --bin web_ui
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
    if [ "${#BRAINS[@]}" -ne 1 ]; then
        echo "--remote-compute currently requires exactly one brain. Current: $BRAIN_CSV"
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

    local brain="${BRAINS[0]}"
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

    local remote_network=""
    local remote_config=""
    if [ -n "$NETWORK_FILE" ]; then
        remote_network="$(remote_path_for_local "$NETWORK_FILE")"
    fi
    if [ -n "$CONFIG_FILE" ]; then
        remote_config="$(remote_path_for_local "$CONFIG_FILE")"
    fi

    local web_ui_api_port="$REMOTE_WEB_UI_PORT"
    local web_ui_api_url=""
    local browser_ui_url="http://$web_ui_host:$REMOTE_WEB_UI_PORT"
    if [ -n "$REMOTE_WEB_UI_API_PORT" ] && [ "$REMOTE_WEB_UI_API_PORT" != "$REMOTE_WEB_UI_PORT" ]; then
        echo "Ignoring --remote-web-ui-api-port=$REMOTE_WEB_UI_API_PORT in web mode."
    fi
    if ! remote_is_port_free "$web_ui_host" "$REMOTE_WEB_UI_PORT"; then
        echo "Remote web_ui port is already in use on $web_ui_host:$REMOTE_WEB_UI_PORT"
        echo "Set --remote-web-ui-port to a free port."
        exit 1
    fi

    local orch_cmd=(
        env "NM_DISTRIBUTE_STARTUP_SNAPSHOT=0"
        target/release/aarnn_rust
        --orchestrator
        --brain-id "$brain"
        --grpc-addr "0.0.0.0:$orch_port"
    )
    if [ "$REMOTE_QUIET" -eq 1 ]; then
        orch_cmd+=(--quiet)
    fi
    if [ -n "$remote_config" ]; then
        orch_cmd+=(--config "$remote_config")
    fi
    if [ -n "$remote_network" ]; then
        orch_cmd+=(--network "$remote_network")
    fi
    local orch_cmd_str
    orch_cmd_str="$(cmd_to_string "${orch_cmd[@]}")"
    remote_start_bg "$orchestrator_host" "remote_orchestrator_${brain}" "$orch_cmd_str" "$REMOTE_LOG_DIR" >/dev/null

    if ! wait_for_remote_port "$orchestrator_host" "$orch_port" 40; then
        echo "Remote orchestrator did not open $orchestrator_host:$orch_port in time."
        exit 1
    fi

    local node_port_start=50070
    for host in "${REMOTE_HOST_LIST[@]}"; do
        if ! remote_reachable "$host"; then
            echo "Skipping remote node on $host (SSH unreachable)."
            continue
        fi
        local node_port
        node_port="$(find_remote_free_port "$host" "$node_port_start")" || {
            echo "Failed to allocate remote node port on $host"
            exit 1
        }
        node_port_start=$((node_port_start + 7))
        local node_weight
        node_weight="$(remote_weight_for_host "$host")"
        local node_cmd=(
            env "NM_CAPACITY_MULTIPLIER=$node_weight" "NM_PRELOAD_NODE_NETWORK=1"
            target/release/aarnn_rust
            --node
            --brain-id "$brain"
            --grpc-addr "0.0.0.0:$node_port"
            --orchestrator-addr "http://$orchestrator_host:$orch_port"
        )
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
        NODE_PORTS["$host"]="$node_port"
    done

    local web_ui_cmd=(
        target/release/web_ui
        --listen "0.0.0.0:$web_ui_api_port"
        --orchestrator "http://$orchestrator_host:$orch_port"
        --auth-mode none
    )
    local web_ui_cmd_str
    web_ui_cmd_str="$(cmd_to_string "${web_ui_cmd[@]}")"
    remote_start_bg "$web_ui_host" "remote_web_ui_${brain}" "$web_ui_cmd_str" "$REMOTE_LOG_DIR" >/dev/null

    web_ui_api_url="http://$web_ui_host:$web_ui_api_port"
    if ! wait_for_http_ready "$web_ui_api_url/api/config" 40; then
        echo "Remote web_ui API did not become ready at $web_ui_api_url"
        exit 1
    fi
    if ! wait_for_cluster_distribution "$web_ui_api_url" "http://$orchestrator_host:$orch_port" "$brain" 45; then
        echo "Cluster distribution for network '$brain' was not ready in time."
        exit 1
    fi
    if [ "$LOCAL_RUST_UI" -eq 1 ]; then
        if ! start_local_rust_ui_client "http://$orchestrator_host:$orch_port" "$brain"; then
            echo "Failed to launch local native rust_ui client."
            exit 1
        fi
    fi

    local socket_path
    socket_path="$(socket_for_brain "$brain")"
    SOCKET_PATHS["$brain"]="$socket_path"
    rm -f "$socket_path"

    local bridge_log="$LOG_DIR/webots_bridge_${brain}.log"
    local bridge_cmd=(
        python3 "$bridge_tool"
        --socket "$socket_path"
        --web-ui-url "$web_ui_api_url"
        --orchestrator "http://$orchestrator_host:$orch_port"
        --network-id "$brain"
        --threshold "$THRESHOLD"
        --default-s "$DEFAULT_S"
        --default-o "$DEFAULT_O"
    )
    "${bridge_cmd[@]}" >"$bridge_log" 2>&1 &
    PIDS+=("$!")
    if ! wait_for_socket "$socket_path"; then
        echo "Failed to bind local bridge socket for brain '$brain': $socket_path"
        echo "See log: $bridge_log"
        tail -n 60 "$bridge_log" || true
        exit 1
    fi

    echo "Remote compute runtime ready:"
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
    echo "  local bridge socket: $socket_path"
    echo "  local bridge log: $bridge_log"
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
            "$bin"
            --socket "$socket_path"
            --sensory "$DEFAULT_S"
            --output "$DEFAULT_O"
            --threshold "$THRESHOLD"
            --aer-sensory-base "$AER_S_BASE"
            --aer-output-base "$AER_O_BASE"
        )
        if [ -n "$CONFIG_FILE" ]; then
            uds_cmd+=(--config "$CONFIG_FILE")
        fi
        if [ -n "$NETWORK_FILE" ]; then
            uds_cmd+=(--network "$NETWORK_FILE")
        fi
        "${uds_cmd[@]}" >"$log_file" 2>&1 &
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

if [ "$REMOTE_COMPUTE" -eq 1 ] && [ "$RUNTIME" != "cluster" ]; then
    echo "--remote-compute requires --runtime cluster."
    exit 1
fi

if [ "$RUNTIME" = "cluster" ] && [ "$REMOTE_COMPUTE" -eq 0 ] && [ "$NODE_UI" -eq 1 ] && [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
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
echo "  config file: ${CONFIG_FILE:-none}"
echo "  network file: ${NETWORK_FILE:-none}"
echo "  orchestrator port: ${ORCHESTRATOR_PORT:-auto}"
echo "  node ui hidden: $NODE_UI_HIDDEN"
echo "  single orchestrator ui: $SINGLE_ORCHESTRATOR_UI"
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
    echo "  remote sync data: $REMOTE_SYNC_DATA"
    echo "  remote rsync compression: $REMOTE_RSYNC_COMPRESS"
    if [ -n "$REMOTE_WEB_UI_API_PORT" ]; then
        echo "  remote web_ui api port: ${REMOTE_WEB_UI_API_PORT} (ignored in web mode)"
    fi
    echo "  local rust_ui: $LOCAL_RUST_UI"
    echo "  remote quiet mode: $REMOTE_QUIET"
    echo "  remote pre-clean: $REMOTE_PRE_CLEAN"
    echo "  local webots host label: $REMOTE_WEBOTS_HOST"
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

if [ "$START_WEBOTS" -eq 1 ] && [ -n "$WEBOTS_PID" ]; then
    wait "$WEBOTS_PID" || true
else
    wait
fi
