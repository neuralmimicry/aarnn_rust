#!/usr/bin/env bash
# run_sim.sh — Unified AARNN simulator launcher
#
# Selects the simulation backend (Webots, Unreal, Unity, WebGL, or all three)
# and
# starts the appropriate AARNN brain processes for the requested robot spec.

export NM_MORPHO_ASYNC="${NM_MORPHO_ASYNC:-1}"
#
# Usage:
#   ./run_sim.sh [options]
#
# Options:
#   --sim <webots|unreal|unity|webgl|minecraft|all>
#                     Simulation backend (default: webots).
#                       webots  — launch Webots + AARNN via run_multi_robot_webots.sh
#                       unreal  — load distributed brains paused, launch Unreal,
#                                 then arm processing after every robot handshake
#                       unity   — start AARNN brains; press Play in the Unity editor
#                       webgl   — start the distributed AARNN runtime and authenticated
#                                 web gateway; open the printed URL in a browser
#                       all     — launch Webots AND (Unreal + brains) concurrently
#                       minecraft — detected Fabric client + local authenticated TCP companion
#   --robots <spec>   Robot spec, e.g. "celegans=1,hexapod=2,nao=1"
#                     (default: celegans=1).
#                     Supported types: celegans, drosophila_banc, drosophila_fafb,
#                                      hexapod, nao, zebrafish
#                     Aliases: worm/worms/c_elegans → celegans
#                              drosophila/fly/flies/fruitfly/banc → drosophila_banc
#                              fafb → drosophila_fafb
#                              hex/hexapods/freenove → hexapod
#                              naos → nao
#                              danio/danio_rerio/fish/zfish/zf/zebrafishes → zebrafish
#   --tcp-host <host> TCP bind host for unreal/unity backends (default: 127.0.0.1).
#   --tcp-base-port <n>
#                     First TCP port to allocate across brain instances (default: 7890).
#   --tcp-ready-timeout <seconds>
#                     Time to wait for each TCP brain server to bind before launching
#                     Unreal/Unity (default: 600).
#   --node <n>        Number of distributed worker nodes for the selected backend
#   --nodes <n>       Alias for --node; distribute the configured brain shards
#                     across this many workers for Webots, Unreal, Unity, or WebGL.
#   --no-build        Skip the selected brain-runtime build (reuse release binaries).
#   --all-features    Build standalone nn_tcp_server with cargo --all-features.
#   --engine <path>   Unreal Engine directory (…/UnrealEngine/Engine).
#                     Default: $UE_ENGINE or /home/pbisaacs/Developer/Engine.
#   --uproject <path> Unreal .uproject to launch (default: sim/unreal/NeuralMimicrySim.uproject).
#   --map <name>      Boot map for --sim unreal (default: Template_Default).
#   --no-engine       For unreal/all: start brain servers only, don't launch Unreal.
#   --minecraft-edition <auto|java|bedrock>
#                     Detect Java/Fabric or native Bedrock Dedicated Server; force a named edition.
#   --bedrock-dir <path>
#                     Existing, configured Bedrock Dedicated Server directory.
#   --web-port <n>    Browser WebGL gateway port (default: 8080).
#   --orchestrator-port <n>
#                     Fixed cluster gRPC port for WebGL (default: auto-select).
#   --config-map <csv>
#                     Per-brain config map CSV (forwarded to Webots launcher).
#   --network-map <csv>
#                     Per-brain network map CSV (forwarded to Webots launcher).
#   --help            Show this usage message.
#   --nao-social      Fresh local NAO social reference model, chat and autonomous
#                     encounters. One NAO; see sim/nao/README.md.
#   [other args]      Remaining args are forwarded to run_multi_robot_webots.sh when
#                     --sim webots is active.
#
# Examples:
#   # Start Webots with two hexapods and one NAO:
#   ./run_sim.sh --sim webots --robots "hexapod=2,nao=1"
#
#   # Start TCP servers for Unreal with three C. elegans brains:
#   ./run_sim.sh --sim unreal --robots "celegans=3"
#
#   # Start both Webots and TCP servers simultaneously:
#   ./run_sim.sh --sim all --robots "celegans=1,hexapod=1"
#
#   # Distribute C. elegans shards across three workers for Unreal:
#   ./run_sim.sh --sim unreal --robots "celegans=1" --node 3

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
ROBOT_PROFILES_PY="$ROOT_DIR/scripts/robot_profiles.py"

LOCAL_MANAGEMENT_ROOT="${NM_LOCAL_MANAGEMENT_ROOT:-$ROOT_DIR/data/simulator-runtime}"

if [ ! -f "$ROBOT_PROFILES_PY" ]; then
  echo "run_sim.sh: missing shared robot profile helper: $ROBOT_PROFILES_PY" >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# Defaults
# ---------------------------------------------------------------------------
SIM_BACKEND="${SIM_BACKEND:-webots}"
MINECRAFT_EDITION="${NM_MINECRAFT_EDITION:-auto}"
ROBOT_SPEC="${ROBOT_SPEC:-celegans=1}"
TCP_HOST="${TCP_HOST:-127.0.0.1}"
TCP_BASE_PORT="${TCP_BASE_PORT:-7890}"
TCP_READY_TIMEOUT="${TCP_READY_TIMEOUT:-600}"
CLUSTER_NODE_COUNT="${NM_CLUSTER_NODES:-1}"
CLUSTER_NODE_COUNT_SET=0
NO_BUILD=0
NAO_SOCIAL=0
BUILD_ALL_FEATURES=1
WEBOTS_PASSTHROUGH_ARGS=()
CONFIG_MAP_CSV=""
NETWORK_MAP_CSV=""

# Unreal Engine launch configuration (used when --sim unreal/all).
UE_ENGINE="${UE_ENGINE:-/home/pbisaacs/Developer/Engine}"
UPROJECT="${UPROJECT:-$ROOT_DIR/sim/unreal/NeuralMimicrySim.uproject}"
UE_MAP="${UE_MAP:-/Engine/Maps/Templates/Template_Default}"
UE_GAMEMODE="/Script/NmAerBridge.NmSimGameMode"
LAUNCH_ENGINE=1              # 0 = start brain servers only (no engine window)
WEBGL_HOST="${WEBGL_HOST:-127.0.0.1}"
WEBGL_PORT="${WEBGL_PORT:-8080}"
WEBGL_ORCHESTRATOR_PORT="${WEBGL_ORCHESTRATOR_PORT:-}"
WEBGL_RUNTIME_FEATURES="${NM_WEBGL_RUNTIME_FEATURES:-engine_runtime,ui,robot_io,cuda}"

# ---------------------------------------------------------------------------
# Robot type tables
# ---------------------------------------------------------------------------
robot_profile_field() {
  local robot_type="$1"
  local field="$2"
  python3 "$ROBOT_PROFILES_PY" profile-field "$robot_type" "$field" --root-dir "$ROOT_DIR"
}

robot_sensory() {
  robot_profile_field "$1" sensory
}

robot_output() {
  robot_profile_field "$1" output
}

robot_network_file() {
  robot_profile_field "$1" network
}

robot_config_file() {
  robot_profile_field "$1" config
}

# ---------------------------------------------------------------------------
# Arg parsing
# ---------------------------------------------------------------------------
usage() {
  sed -n '/^# Usage:/,/^[^#]/{ /^[^#]/d; s/^# \{0,2\}//; p }' "$0"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --sim)
      shift
      SIM_BACKEND="${1:-$SIM_BACKEND}"
      ;;
    --robots|--robot-counts)
      shift
      ROBOT_SPEC="${1:-$ROBOT_SPEC}"
      ;;
    --tcp-host)
      shift
      TCP_HOST="${1:-$TCP_HOST}"
      ;;
    --tcp-base-port)
      shift
      TCP_BASE_PORT="${1:-$TCP_BASE_PORT}"
      ;;
    --tcp-ready-timeout)
      shift
      TCP_READY_TIMEOUT="${1:-$TCP_READY_TIMEOUT}"
      ;;
    --node|--nodes)
      shift
      CLUSTER_NODE_COUNT="${1:-}"
      CLUSTER_NODE_COUNT_SET=1
      ;;
    --node=*|--nodes=*)
      CLUSTER_NODE_COUNT="${1#*=}"
      CLUSTER_NODE_COUNT_SET=1
      ;;
    --no-build)
      NO_BUILD=1
      ;;
    --all-features)
      BUILD_ALL_FEATURES=1
      ;;
    --engine)
      shift
      UE_ENGINE="${1:-$UE_ENGINE}"
      ;;
    --uproject)
      shift
      UPROJECT="${1:-$UPROJECT}"
      ;;
    --map)
      shift
      UE_MAP="${1:-$UE_MAP}"
      ;;
    --no-engine)
      LAUNCH_ENGINE=0
      ;;
    --nao-social)
      NAO_SOCIAL=1
      ;;
    --minecraft-edition)
      shift
      MINECRAFT_EDITION="${1:-auto}"
      ;;
    --bedrock-dir)
      shift
      export NM_BEDROCK_DIR="${1:-}"
      ;;
    --web-port|--webgl-port)
      shift
      WEBGL_PORT="${1:-$WEBGL_PORT}"
      ;;
    --orchestrator-port)
      shift
      WEBGL_ORCHESTRATOR_PORT="${1:-$WEBGL_ORCHESTRATOR_PORT}"
      ;;
    --config-map)
      shift
      CONFIG_MAP_CSV="${1:-}"
      WEBOTS_PASSTHROUGH_ARGS+=(--config-map "$CONFIG_MAP_CSV")
      ;;
    --network-map)
      shift
      NETWORK_MAP_CSV="${1:-}"
      WEBOTS_PASSTHROUGH_ARGS+=(--network-map "$NETWORK_MAP_CSV")
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    --)
      shift
      WEBOTS_PASSTHROUGH_ARGS+=("$@")
      break
      ;;
    *)
      WEBOTS_PASSTHROUGH_ARGS+=("$1")
      ;;
  esac
  shift
done

if [ "$NAO_SOCIAL" -eq 1 ]; then
  if [ "$SIM_BACKEND" = all ] || [ "$CLUSTER_NODE_COUNT" != 1 ] || [ -n "$CONFIG_MAP_CSV$NETWORK_MAP_CSV" ]; then
    echo 'NAO social reference requires one simulator and one fresh local brain; custom/distributed snapshots are unsupported.' >&2
    exit 2
  fi
  social_args=(--sim "$SIM_BACKEND" --body-port "$TCP_BASE_PORT" --minecraft-edition "$MINECRAFT_EDITION")
  if [ "$LAUNCH_ENGINE" -eq 0 ]; then social_args+=(--no-engine); fi
  if [ -n "${NM_BEDROCK_DIR:-}" ]; then social_args+=(--bedrock-dir "$NM_BEDROCK_DIR"); fi
  export UE_ENGINE
  exec python3 "$ROOT_DIR/scripts/run_nao_social.py" "${social_args[@]}"
fi

# Validate --sim value
case "$SIM_BACKEND" in
  webots|unreal|unity|webgl|minecraft|all) ;;
  *)
    echo "run_sim.sh: invalid --sim value '$SIM_BACKEND' (must be webots, unreal, unity, webgl, minecraft, or all)" >&2
    exit 1
    ;;
esac

# Validate --tcp-base-port
if ! [[ "$TCP_BASE_PORT" =~ ^[0-9]+$ ]] || [ "$TCP_BASE_PORT" -lt 1 ] || [ "$TCP_BASE_PORT" -gt 65534 ]; then
  echo "run_sim.sh: --tcp-base-port must be an integer in [1..65534], got '$TCP_BASE_PORT'" >&2
  exit 1
fi

# Validate --tcp-ready-timeout
if ! [[ "$TCP_READY_TIMEOUT" =~ ^[0-9]+$ ]] || [ "$TCP_READY_TIMEOUT" -lt 1 ]; then
  echo "run_sim.sh: --tcp-ready-timeout must be a positive integer, got '$TCP_READY_TIMEOUT'" >&2
  exit 1
fi

if ! [[ "$WEBGL_PORT" =~ ^[0-9]+$ ]] || [ "$WEBGL_PORT" -lt 1 ] || [ "$WEBGL_PORT" -gt 65535 ]; then
  echo "run_sim.sh: --web-port must be an integer in [1..65535], got '$WEBGL_PORT'" >&2
  exit 1
fi
if [ -n "$WEBGL_ORCHESTRATOR_PORT" ] && { ! [[ "$WEBGL_ORCHESTRATOR_PORT" =~ ^[0-9]+$ ]] || [ "$WEBGL_ORCHESTRATOR_PORT" -lt 1 ] || [ "$WEBGL_ORCHESTRATOR_PORT" -gt 65535 ]; }; then
  echo "run_sim.sh: --orchestrator-port must be an integer in [1..65535], got '$WEBGL_ORCHESTRATOR_PORT'" >&2
  exit 1
fi

if [ "$CLUSTER_NODE_COUNT_SET" -eq 1 ] && { ! [[ "$CLUSTER_NODE_COUNT" =~ ^[0-9]+$ ]] || [ "$CLUSTER_NODE_COUNT" -lt 1 ]; }; then
  echo "run_sim.sh: --node/--nodes must be a positive integer, got '$CLUSTER_NODE_COUNT'" >&2
  exit 1
fi
if [ "$CLUSTER_NODE_COUNT_SET" -eq 1 ]; then
  WEBOTS_PASSTHROUGH_ARGS+=(--nodes "$CLUSTER_NODE_COUNT")
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "run_sim.sh: python3 is required to prepare the local management environment" >&2
  exit 1
fi
eval "$(python3 "$ROOT_DIR/scripts/local_management_env.py" \
  --runtime-root "$LOCAL_MANAGEMENT_ROOT" --shell)"

# ---------------------------------------------------------------------------
# Robot spec parser — produces parallel arrays: BRAIN_IDS[], BRAIN_TYPES[]
# ---------------------------------------------------------------------------
parse_robot_spec() {
  local spec="$1"
  python3 "$ROBOT_PROFILES_PY" brains "$spec"
}

# ---------------------------------------------------------------------------
# Build nn_tcp_server binary
# ---------------------------------------------------------------------------
TCP_SERVER_BIN=""
TCP_AER_BRIDGE_BIN=""

locate_tcp_server_bin() {
  # Prefer examples sub-directory, fall back to root of target/release
  if [ -x "$ROOT_DIR/target/release/examples/nn_tcp_server" ]; then
    TCP_SERVER_BIN="$ROOT_DIR/target/release/examples/nn_tcp_server"
  elif [ -x "$ROOT_DIR/target/release/nn_tcp_server" ]; then
    TCP_SERVER_BIN="$ROOT_DIR/target/release/nn_tcp_server"
  else
    TCP_SERVER_BIN=""
  fi
}

build_tcp_server() {
  if [ "$NO_BUILD" -eq 1 ]; then
    locate_tcp_server_bin
    if [ -z "$TCP_SERVER_BIN" ]; then
      echo "run_sim.sh: --no-build specified but nn_tcp_server binary not found" >&2
      echo "  Expected at: $ROOT_DIR/target/release/examples/nn_tcp_server" >&2
      if [ "$BUILD_ALL_FEATURES" -eq 1 ]; then
        echo "  Build with: cargo build --release --all-features --example nn_tcp_server" >&2
      fi
      exit 1
    fi
    return
  fi

  echo "run_sim.sh: building nn_tcp_server …"
  (
    cd "$ROOT_DIR"
    cargo build --release --all-features --example nn_tcp_server
  )
  locate_tcp_server_bin
  if [ -z "$TCP_SERVER_BIN" ]; then
    echo "run_sim.sh: build succeeded but nn_tcp_server binary could not be located" >&2
    exit 1
  fi
}

locate_tcp_aer_bridge_bin() {
  if [ -x "$ROOT_DIR/target/release/tcp_aer_ipc_bridge" ]; then
    TCP_AER_BRIDGE_BIN="$ROOT_DIR/target/release/tcp_aer_ipc_bridge"
  else
    TCP_AER_BRIDGE_BIN=""
  fi
}

build_tcp_aer_bridge() {
  if [ "$NO_BUILD" -eq 1 ]; then
    locate_tcp_aer_bridge_bin
    if [ -z "$TCP_AER_BRIDGE_BIN" ]; then
      echo "run_sim.sh: --no-build specified but Rust TCP/AER IPC bridge was not found." >&2
      echo "  Expected at: $ROOT_DIR/target/release/tcp_aer_ipc_bridge" >&2
      echo "  Build with: cargo build --release --locked --all-features --bin tcp_aer_ipc_bridge" >&2
      exit 1
    fi
    return
  fi

  echo "run_sim.sh: building Rust TCP/AER IPC bridge …"
  (
    cd "$ROOT_DIR"
    cargo build --release --locked --all-features --bin tcp_aer_ipc_bridge
  )
  locate_tcp_aer_bridge_bin
  if [ -z "$TCP_AER_BRIDGE_BIN" ]; then
    echo "run_sim.sh: bridge build succeeded but target/release/tcp_aer_ipc_bridge was not found." >&2
    exit 1
  fi
}

# ---------------------------------------------------------------------------
# Process tracking for TCP brain servers
# ---------------------------------------------------------------------------
TCP_PIDS=()
TCP_PORTS=()
CLUSTER_PIDS=()
BRIDGE_PIDS=()
BRIDGE_PORTS=()
CLUSTER_SOCKET_DIR=""
CLUSTER_LOG_DIR=""
CLUSTER_ORCHESTRATOR_PORT=""
ENV_READY_DIR=""
ARM_FILE=""
DISTRIBUTED_MODE=0
WEBGL_BACKEND_PID=""
WEBGL_WEB_PID=""

cleanup_tcp_servers() {
  local sig="${1:-TERM}"
  if [ "${#TCP_PIDS[@]}" -gt 0 ]; then
    echo ""
    echo "run_sim.sh: shutting down TCP brain servers (SIG${sig}) …"
    local pid
    for pid in "${TCP_PIDS[@]}"; do
      if kill -0 "$pid" 2>/dev/null; then
        kill -"$sig" "$pid" 2>/dev/null || true
      fi
    done
    # Give processes a moment then force-kill any survivors
    sleep 1
    for pid in "${TCP_PIDS[@]}"; do
      if kill -0 "$pid" 2>/dev/null; then
        kill -KILL "$pid" 2>/dev/null || true
      fi
    done
    TCP_PIDS=()
    TCP_PORTS=()
  fi
}

cleanup_distributed_runtime() {
  if [ "${#BRIDGE_PIDS[@]}" -gt 0 ]; then
    echo ""
    echo "run_sim.sh: shutting down distributed TCP bridges …"
    local pid
    for pid in "${BRIDGE_PIDS[@]}"; do
      if kill -0 "$pid" 2>/dev/null; then
        kill -TERM "$pid" 2>/dev/null || true
      fi
    done
  fi
  if [ "${#CLUSTER_PIDS[@]}" -gt 0 ]; then
    echo "run_sim.sh: shutting down distributed brain cluster …"
    for pid in "${CLUSTER_PIDS[@]}"; do
      kill -TERM -- "-$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null || true
    done
  fi
  sleep 1
  for pid in "${BRIDGE_PIDS[@]}"; do
    if [ -n "${pid:-}" ] && kill -0 "$pid" 2>/dev/null; then
      kill -KILL "$pid" 2>/dev/null || true
    fi
  done
  for pid in "${CLUSTER_PIDS[@]}"; do
    if [ -n "${pid:-}" ]; then
      kill -KILL -- "-$pid" 2>/dev/null || kill -KILL "$pid" 2>/dev/null || true
    fi
  done
  BRIDGE_PIDS=()
  BRIDGE_PORTS=()
  CLUSTER_ORCHESTRATOR_PORT=""
  ENV_READY_DIR=""
  ARM_FILE=""
  CLUSTER_PIDS=()
  if [ -n "$CLUSTER_SOCKET_DIR" ] && [ -d "$CLUSTER_SOCKET_DIR" ]; then
    rm -rf "$CLUSTER_SOCKET_DIR"
  fi
}

cleanup_all() {
  cleanup_tcp_servers TERM
  cleanup_distributed_runtime
  if [ -n "${WEBGL_WEB_PID:-}" ] && kill -0 "$WEBGL_WEB_PID" 2>/dev/null; then
    echo "run_sim.sh: shutting down WebGL web gateway (pid $WEBGL_WEB_PID) …"
    kill -TERM "$WEBGL_WEB_PID" 2>/dev/null || true
  fi
  WEBGL_WEB_PID=""
  if [ -n "${WEBGL_BACKEND_PID:-}" ] && kill -0 "$WEBGL_BACKEND_PID" 2>/dev/null; then
    echo "run_sim.sh: shutting down WebGL cluster runtime (pid $WEBGL_BACKEND_PID) …"
    kill -TERM -- "-$WEBGL_BACKEND_PID" 2>/dev/null || kill -TERM "$WEBGL_BACKEND_PID" 2>/dev/null || true
  fi
  WEBGL_BACKEND_PID=""
  # If we launched a Webots subprocess in "all" mode, kill it too
  if [ -n "${WEBOTS_PID:-}" ] && kill -0 "$WEBOTS_PID" 2>/dev/null; then
    echo "run_sim.sh: shutting down Webots launcher (pid $WEBOTS_PID) …"
    kill -TERM "$WEBOTS_PID" 2>/dev/null || true
  fi
  # If we launched Unreal Engine, kill it too
  if [ -n "${UE_PID:-}" ] && kill -0 "$UE_PID" 2>/dev/null; then
    echo "run_sim.sh: shutting down Unreal Engine (pid $UE_PID) …"
    kill -TERM "$UE_PID" 2>/dev/null || true
  fi
}

free_tcp_port() {
  local requested="${1:-0}"
  python3 - "$requested" <<'PY'
import socket
import sys
requested = int(sys.argv[1])
sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
try:
    if requested:
        sock.bind(("127.0.0.1", requested))
    else:
        sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
finally:
    sock.close()
PY
}

tcp_port_listening() {
  local port="$1"

  if command -v ss >/dev/null 2>&1; then
    ss -H -ltn "sport = :$port" 2>/dev/null | grep -q .
    return
  fi

  local connect_host="$TCP_HOST"
  case "$connect_host" in
    0.0.0.0|::|"")
      connect_host="127.0.0.1"
      ;;
  esac

  (exec 3<>"/dev/tcp/${connect_host}/${port}") >/dev/null 2>&1
}

wait_for_tcp_servers_ready() {
  local total="${#TCP_PIDS[@]}"
  local deadline=$((SECONDS + TCP_READY_TIMEOUT))
  local ready_count=0

  echo ""
  echo "run_sim.sh: waiting for TCP brain server readiness (timeout ${TCP_READY_TIMEOUT}s) …"

  while [ "$SECONDS" -le "$deadline" ]; do
    ready_count=0

    local i
    for (( i=0; i<total; i++ )); do
      local pid="${TCP_PIDS[$i]}"
      local port="${TCP_PORTS[$i]}"

      if ! kill -0 "$pid" 2>/dev/null; then
        local rc=0
        set +e
        wait "$pid"
        rc=$?
        set -e
        echo "run_sim.sh: brain server on ${TCP_HOST}:${port} exited before listening (exit $rc)." >&2
        exit 1
      fi

      if tcp_port_listening "$port"; then
        ready_count=$((ready_count + 1))
      fi
    done

    if [ "$ready_count" -eq "$total" ]; then
      echo "run_sim.sh: all TCP brain server(s) are listening."
      return 0
    fi

    sleep 1
  done

  echo "run_sim.sh: timed out waiting for TCP brain servers to listen." >&2
  echo "  Ready: $ready_count/$total" >&2
  echo "  Increase --tcp-ready-timeout for large network snapshots." >&2
  exit 1
}

trap 'cleanup_all' EXIT
trap 'echo ""; echo "run_sim.sh: interrupted."; cleanup_all; exit 130' INT TERM

# ---------------------------------------------------------------------------
# Launch TCP brain servers
# ---------------------------------------------------------------------------
resolve_brain_arrays() {
  BRAIN_IDS=()
  BRAIN_TYPES=()
  local brain_lines=""
  if ! brain_lines="$(parse_robot_spec "$ROBOT_SPEC")"; then
    exit 1
  fi
  while IFS=' ' read -r brain_id brain_type; do
    [ -n "${brain_id:-}" ] || continue
    BRAIN_IDS+=("$brain_id")
    BRAIN_TYPES+=("$brain_type")
  done <<< "$brain_lines"

  local total="${#BRAIN_IDS[@]}"
  if [ "$total" -eq 0 ]; then
    echo "run_sim.sh: no brain instances resolved from spec '$ROBOT_SPEC'" >&2
    exit 1
  fi
}

start_tcp_servers() {
  build_tcp_server
  resolve_brain_arrays
  local total="${#BRAIN_IDS[@]}"

  echo ""
  echo "run_sim.sh: launching $total TCP brain server(s) …"
  echo ""
  printf "  %-28s  %-22s  %s  %s  %s\n" "Brain ID" "Address" "Sensory" "Output" "PID"
  printf "  %-28s  %-22s  %s  %s  %s\n" \
    "----------------------------" "----------------------" "-------" "------" "-------"

  local i
  for (( i=0; i<total; i++ )); do
    local brain_id="${BRAIN_IDS[$i]}"
    local brain_type="${BRAIN_TYPES[$i]}"
    local port=$(( TCP_BASE_PORT + i ))
    local sensory
    local output
    sensory="$(robot_sensory "$brain_type")"
    output="$(robot_output "$brain_type")"
    local net_file
    net_file="$(robot_network_file "$brain_type")"
    local cfg_file
    cfg_file="$(robot_config_file "$brain_type")"

    if tcp_port_listening "$port"; then
      echo "run_sim.sh: ${TCP_HOST}:${port} already has a TCP listener." >&2
      echo "  Stop the existing process or choose a different --tcp-base-port." >&2
      exit 1
    fi

    local cmd=(
      env
      "NM_REALTIME_IPC=${NM_REALTIME_IPC:-1}"
      "$TCP_SERVER_BIN"
      --tcp "$TCP_HOST:$port"
      --sensory "$sensory"
      --output "$output"
    )
    if [ -f "$net_file" ]; then
      cmd+=(--network "$net_file")
    fi
    if [ -f "$cfg_file" ]; then
      cmd+=(--config "$cfg_file")
    fi

    "${cmd[@]}" &
    local pid=$!
    # Fail fast if the server exits immediately (e.g., address already in use).
    sleep 0.1
    if ! kill -0 "$pid" 2>/dev/null; then
      local rc=0
      set +e
      wait "$pid"
      rc=$?
      set -e
      echo "run_sim.sh: failed to start brain '$brain_id' on ${TCP_HOST}:${port} (exit $rc)." >&2
      echo "  Ensure the address/port is free or choose a different --tcp-base-port." >&2
      exit 1
    fi
    TCP_PIDS+=("$pid")
    TCP_PORTS+=("$port")

    printf "  %-28s  %-22s  %-7s  %-6s  %s\n" \
      "$brain_id" "${TCP_HOST}:${port}" "sensory=${sensory}" "output=${output}" "$pid"
  done

  wait_for_tcp_servers_ready

  echo ""
  echo "run_sim.sh: $total brain server(s) ready on $TCP_HOST:$TCP_BASE_PORT – $(( TCP_BASE_PORT + total - 1 ))"
}

wait_for_ipc_sockets() {
  local deadline=$((SECONDS + TCP_READY_TIMEOUT))
  local brain
  while [ "$SECONDS" -le "$deadline" ]; do
    local all_ready=1
    for brain in "${BRAIN_IDS[@]}"; do
      if [ "$brain" = "default" ]; then
        [ -S "$CLUSTER_SOCKET_DIR/aarnn_rust.nn" ] || { all_ready=0; break; }
      else
        [ -S "$CLUSTER_SOCKET_DIR/aarnn_rust.${brain}.nn" ] || { all_ready=0; break; }
      fi
    done
    if [ "$all_ready" -eq 1 ]; then return 0; fi
    for pid in "${CLUSTER_PIDS[@]}"; do
      if ! kill -0 "$pid" 2>/dev/null; then
        echo "run_sim.sh: distributed runtime exited before its IPC sockets became ready." >&2
        exit 1
      fi
    done
    sleep 1
  done
  echo "run_sim.sh: timed out waiting for distributed IPC sockets." >&2
  exit 1
}

wait_for_distributed_workers_ready() {
  local expected="$CLUSTER_NODE_COUNT"
  local deadline=$((SECONDS + TCP_READY_TIMEOUT))
  local ready_marker="worker processes: ${expected} ("
  while [ "$SECONDS" -le "$deadline" ]; do
    if grep -Fq "$ready_marker" "$CLUSTER_LOG_DIR/runtime.log" 2>/dev/null; then
      echo "run_sim.sh: distributed runtime registered $expected worker process(es)."
      local node_inventory
      node_inventory="$(grep -F 'registered node IDs:' "$CLUSTER_LOG_DIR/runtime.log" | tail -n 1 || true)"
      if [ -n "$node_inventory" ]; then
        echo "  $node_inventory"
      fi
      echo "  cluster log: $CLUSTER_LOG_DIR/runtime.log"
      return 0
    fi
    for pid in "${CLUSTER_PIDS[@]}"; do
      if ! kill -0 "$pid" 2>/dev/null; then
        echo "run_sim.sh: distributed runtime exited before all worker processes registered." >&2
        echo "  See: $CLUSTER_LOG_DIR/runtime.log" >&2
        tail -n 60 "$CLUSTER_LOG_DIR/runtime.log" >&2 || true
        exit 1
      fi
    done
    sleep 1
  done
  echo "run_sim.sh: timed out waiting for $expected distributed worker processes to register." >&2
  echo "  See: $CLUSTER_LOG_DIR/runtime.log" >&2
  tail -n 60 "$CLUSTER_LOG_DIR/runtime.log" >&2 || true
  exit 1
}

wait_for_distributed_placement_ready() {
  local expected="$CLUSTER_NODE_COUNT"
  local deadline=$((SECONDS + TCP_READY_TIMEOUT))
  local -a placement_lines=()
  local brain line
  local placement_log="$CLUSTER_LOG_DIR/webots_orchestrator.log"

  echo "run_sim.sh: waiting for placement across all ${expected} worker(s) …"
  while [ "$SECONDS" -le "$deadline" ]; do
    placement_lines=()
    local all_ready=1
    for brain in "${BRAIN_IDS[@]}"; do
      line="$(grep -F " - Network ${brain}:" "$placement_log" 2>/dev/null \
        | grep -F "Distributed across ${expected} nodes" | tail -n 1 || true)"
      if [ -z "$line" ]; then
        all_ready=0
        break
      fi
      placement_lines+=("$line")
    done
    if [ "$all_ready" -eq 1 ]; then
      echo "run_sim.sh: distributed placement confirmed for all ${#BRAIN_IDS[@]} network(s):"
      printf '  %s\n' "${placement_lines[@]}"
      return 0
    fi
    for pid in "${CLUSTER_PIDS[@]}"; do
      if ! kill -0 "$pid" 2>/dev/null; then
        echo "run_sim.sh: distributed runtime exited before placement was confirmed." >&2
        echo "  See: $placement_log" >&2
        tail -n 80 "$placement_log" >&2 || true
        exit 1
      fi
    done
    sleep 1
  done
  echo "run_sim.sh: timed out waiting for placement across ${expected} worker(s)." >&2
  echo "  See: $placement_log" >&2
  tail -n 80 "$placement_log" >&2 || true
  exit 1
}

wait_for_tcp_bridges_ready() {
  local total="${#BRIDGE_PIDS[@]}"
  local deadline=$((SECONDS + TCP_READY_TIMEOUT))
  local ready_count=0
  echo ""
  echo "run_sim.sh: waiting for distributed TCP bridge readiness (timeout ${TCP_READY_TIMEOUT}s) …"
  while [ "$SECONDS" -le "$deadline" ]; do
    ready_count=0
    local i
    for (( i=0; i<total; i++ )); do
      local pid="${BRIDGE_PIDS[$i]}"
      local port="${BRIDGE_PORTS[$i]}"
      if ! kill -0 "$pid" 2>/dev/null; then
        echo "run_sim.sh: distributed TCP bridge on ${TCP_HOST}:${port} exited before listening." >&2
        exit 1
      fi
      if tcp_port_listening "$port"; then ready_count=$((ready_count + 1)); fi
    done
    if [ "$ready_count" -eq "$total" ]; then
      echo "run_sim.sh: all distributed TCP bridge(s) are listening."
      return 0
    fi
    sleep 1
  done
  echo "run_sim.sh: timed out waiting for distributed TCP bridges (${ready_count}/${total})." >&2
  exit 1
}

start_distributed_tcp_servers() {
  resolve_brain_arrays
  local total="${#BRAIN_IDS[@]}"
  if [ "$CLUSTER_NODE_COUNT" -lt "$total" ]; then
    echo "run_sim.sh: --node/--nodes=$CLUSTER_NODE_COUNT requires at least one worker per brain ($total)." >&2
    exit 1
  fi
  build_tcp_aer_bridge

  DISTRIBUTED_MODE=1
  CLUSTER_SOCKET_DIR="$(mktemp -d "${TMPDIR:-/tmp}/aarnn-sim-ipc.XXXXXX")"
  CLUSTER_LOG_DIR="$ROOT_DIR/logs/sim_cluster_${BASHPID}"
  ENV_READY_DIR="$CLUSTER_LOG_DIR/environment_ready"
  ARM_FILE="$ENV_READY_DIR/neural_runtime_armed"
  CLUSTER_ORCHESTRATOR_PORT="$(free_tcp_port 0)"
  mkdir -p "$CLUSTER_LOG_DIR"
  mkdir -p "$ENV_READY_DIR"
  rm -f "$ARM_FILE"
  local cluster_config_map=""
  local cluster_network_map=""
  local i brain_id brain_type path
  for (( i=0; i<total; i++ )); do
    brain_id="${BRAIN_IDS[$i]}"
    brain_type="${BRAIN_TYPES[$i]}"
    path="$(robot_config_file "$brain_type")"
    if [ -f "$path" ]; then
      cluster_config_map="${cluster_config_map:+$cluster_config_map,}${brain_id}=${path}"
    fi
    path="$(robot_network_file "$brain_type")"
    if [ -f "$path" ]; then
      cluster_network_map="${cluster_network_map:+$cluster_network_map,}${brain_id}=${path}"
    fi
  done
  [ -n "$CONFIG_MAP_CSV" ] && cluster_config_map="${cluster_config_map:+$cluster_config_map,}${CONFIG_MAP_CSV}"
  [ -n "$NETWORK_MAP_CSV" ] && cluster_network_map="${cluster_network_map:+$cluster_network_map,}${NETWORK_MAP_CSV}"

  local cluster_cmd=(
    env
    "NM_IPC_SOCKET_DIR=$CLUSTER_SOCKET_DIR"
    "NM_CLUSTER_NODES=$CLUSTER_NODE_COUNT"
    # A simulator request must not consume the network's initial autonomous
    # activity while the environment is still loading.  The launcher arms the
    # network through the existing cluster-control RPC after all bridge
    # handshakes complete.
    "NM_DISTRIBUTED_AUTOSTART=0"
    "LOG_DIR=$CLUSTER_LOG_DIR"
    "$ROOT_DIR/run_webot.sh"
    # Keep the orchestrator dashboard visible for simulator runs.  The
    # per-brain IPC UIs remain hidden, while this single authoritative view
    # reports every registered worker, including headless shard workers.
    --runtime cluster --no-webots --no-diag
    --node-ui-hidden --nodes "$CLUSTER_NODE_COUNT"
    --orchestrator-port "$CLUSTER_ORCHESTRATOR_PORT"
    --brains "$(IFS=,; echo "${BRAIN_IDS[*]}")"
  )
  if [ "$NO_BUILD" -eq 1 ]; then cluster_cmd+=(--no-build); fi
  [ -n "$cluster_config_map" ] && cluster_cmd+=(--config-map "$cluster_config_map")
  [ -n "$cluster_network_map" ] && cluster_cmd+=(--network-map "$cluster_network_map")

  echo ""
  echo "run_sim.sh: launching distributed brain runtime with $CLUSTER_NODE_COUNT worker(s) …"
  setsid "${cluster_cmd[@]}" >"$CLUSTER_LOG_DIR/runtime.log" 2>&1 &
  CLUSTER_PIDS+=("$!")
  wait_for_ipc_sockets
  wait_for_distributed_workers_ready
  wait_for_distributed_placement_ready

  echo ""
  echo "run_sim.sh: launching $total distributed TCP bridge(s) …"
  for (( i=0; i<total; i++ )); do
    brain_id="${BRAIN_IDS[$i]}"
    brain_type="${BRAIN_TYPES[$i]}"
    local port=$((TCP_BASE_PORT + i))
    if tcp_port_listening "$port"; then
      echo "run_sim.sh: ${TCP_HOST}:${port} already has a TCP listener." >&2
      exit 1
    fi
    local ipc_path="$CLUSTER_SOCKET_DIR/aarnn_rust.${brain_id}.nn"
    if [ "$brain_id" = "default" ]; then ipc_path="$CLUSTER_SOCKET_DIR/aarnn_rust.nn"; fi
    local log_file="$CLUSTER_LOG_DIR/bridge_${brain_id}.log"
    local ready_file="$ENV_READY_DIR/${brain_id}.ready"
    rm -f "$ready_file"
    "$TCP_AER_BRIDGE_BIN" \
      --listen "$TCP_HOST:$port" --ipc "$ipc_path" \
      --sensory "$(robot_sensory "$brain_type")" \
      --output "$(robot_output "$brain_type")" \
      --ready-file "$ready_file" \
      --arm-file "$ARM_FILE" \
      >"$log_file" 2>&1 &
    BRIDGE_PIDS+=("$!")
    BRIDGE_PORTS+=("$port")
    printf "  %-28s  %-22s  %s  %s  %s\n" \
      "$brain_id" "${TCP_HOST}:${port}" \
      "sensory=$(robot_sensory "$brain_type")" "output=$(robot_output "$brain_type")" "${BRIDGE_PIDS[$i]}"
  done
  wait_for_tcp_bridges_ready
  echo "run_sim.sh: distributed brain bridges ready on $TCP_HOST:$TCP_BASE_PORT – $((TCP_BASE_PORT + total - 1))"
}

wait_for_environment_bridges_ready() {
  local total="${#BRIDGE_PIDS[@]}"
  local deadline=$((SECONDS + TCP_READY_TIMEOUT))
  local ready_count=0

  echo ""
  echo "run_sim.sh: waiting for Unreal environment readiness (all bridge handshakes, timeout ${TCP_READY_TIMEOUT}s) …"
  while [ "$SECONDS" -le "$deadline" ]; do
    ready_count=0
    local i
    for (( i=0; i<total; i++ )); do
      local pid="${BRIDGE_PIDS[$i]}"
      local brain_id="${BRAIN_IDS[$i]}"
      if ! kill -0 "$pid" 2>/dev/null; then
        echo "run_sim.sh: bridge for ${brain_id} exited before the Unreal environment became ready." >&2
        echo "  See: $CLUSTER_LOG_DIR/bridge_${brain_id}.log" >&2
        exit 1
      fi
      if [ -f "$ENV_READY_DIR/${brain_id}.ready" ]; then
        ready_count=$((ready_count + 1))
      fi
    done
    if [ "$ready_count" -eq "$total" ]; then
      echo "run_sim.sh: Unreal environment ready; all ${total} brain handshake(s) accepted."
      return 0
    fi
    sleep 1
  done
  echo "run_sim.sh: timed out waiting for Unreal environment handshakes (${ready_count}/${total})." >&2
  exit 1
}

arm_distributed_networks() {
  local control_bin="$ROOT_DIR/target/release/aarnn_rust"
  if [ ! -x "$control_bin" ]; then
    echo "run_sim.sh: missing cluster-control binary: $control_bin" >&2
    exit 1
  fi
  local brain_id
  for brain_id in "${BRAIN_IDS[@]}"; do
    echo "run_sim.sh: arming distributed network '$brain_id' after environment readiness …"
    "$control_bin" \
      --orchestrator-addr "http://127.0.0.1:$CLUSTER_ORCHESTRATOR_PORT" \
      --cluster-control-network "$brain_id" \
      --cluster-control-action start
  done

  wait_for_distributed_networks_armed
  local temporary="$ARM_FILE.tmp-$$"
  printf 'armed\n' >"$temporary"
  mv -f "$temporary" "$ARM_FILE"
  echo "run_sim.sh: distributed neural processing armed after environment readiness."
}

wait_for_distributed_networks_armed() {
  local deadline=$((SECONDS + TCP_READY_TIMEOUT))
  local -a worker_logs=()
  local brain_id
  for brain_id in "${BRAIN_IDS[@]}"; do
    worker_logs+=("$CLUSTER_LOG_DIR/webots_${brain_id}.log")
  done
  local extra_index=1
  local extra_workers=$((CLUSTER_NODE_COUNT - ${#BRAIN_IDS[@]}))
  while [ "$extra_index" -le "$extra_workers" ]; do
    worker_logs+=("$CLUSTER_LOG_DIR/webots_${BRAIN_IDS[0]}_worker_$(printf '%02d' "$extra_index").log")
    extra_index=$((extra_index + 1))
  done

  echo "run_sim.sh: waiting for Start to reach all distributed workers …"
  while [ "$SECONDS" -le "$deadline" ]; do
    local all_ready=1
    local log_file
    for log_file in "${worker_logs[@]}"; do
      for brain_id in "${BRAIN_IDS[@]}"; do
        if ! grep -Fq "simulator network ${brain_id} armed (playing=true)" "$log_file" 2>/dev/null; then
          all_ready=0
          break 2
        fi
      done
    done
    if [ "$all_ready" -eq 1 ]; then
      echo "run_sim.sh: Start reached all ${#worker_logs[@]} distributed worker(s) for all ${#BRAIN_IDS[@]} network(s)."
      return 0
    fi
    sleep 1
  done
  echo "run_sim.sh: timed out waiting for distributed workers to apply Start." >&2
  exit 1
}
# Block until the brain servers exit (used when no engine is auto-launched).
serve_and_wait() {
  local total="${#BRAIN_IDS[@]}"
  echo ""
  echo "run_sim.sh: connect your simulation engine using:"
  echo "   Host : $TCP_HOST"
  echo "   Ports: $TCP_BASE_PORT – $(( TCP_BASE_PORT + total - 1 ))"
  echo ""
  echo "Press Ctrl-C to stop all servers."
  echo ""
  if [ "$DISTRIBUTED_MODE" -eq 1 ]; then
    wait "${BRIDGE_PIDS[@]}"
  else
    wait "${TCP_PIDS[@]}"
  fi
}

# ---------------------------------------------------------------------------
# Launch the Unreal Engine project (standalone -game) against the brain servers
# ---------------------------------------------------------------------------
launch_unreal_engine() {
  local ue_bin="$UE_ENGINE/Binaries/Linux/UnrealEditor"

  if [ "$LAUNCH_ENGINE" -ne 1 ]; then
    echo "run_sim.sh: Unreal launch disabled by --no-engine; no Unreal simulator process will be started."
    serve_and_wait
    return
  fi

  if [ ! -x "$ue_bin" ]; then
    echo "run_sim.sh: Unreal Engine binary not found: $ue_bin" >&2
    echo "  Set --engine <UE_root>/Engine or UE_ENGINE, or pass --no-engine to run" >&2
    echo "  the brain servers only." >&2
    return 1
  fi
  if [ ! -f "$UPROJECT" ]; then
    echo "run_sim.sh: Unreal project not found: $UPROJECT" >&2
    echo "  Pass --uproject <path> or set UPROJECT." >&2
    return 1
  fi

  # The GameMode reads these to spawn and wire the robots (ports match above).
  export NM_UE_ROBOTS="$ROBOT_SPEC"
  export NM_AARNN_HOST="$TCP_HOST"
  export NM_AARNN_BASE_PORT="$TCP_BASE_PORT"

  echo ""
  echo "run_sim.sh: launching Unreal Engine — robots spawn and connect automatically."
  echo "   Engine : $ue_bin"
  echo "   Project: $UPROJECT"
  echo "   Map    : $UE_MAP  (GameMode: $UE_GAMEMODE)"
  echo ""

  # Standalone game; force our GameMode via the map URL so robots auto-spawn.
  "$ue_bin" "$UPROJECT" "${UE_MAP}?game=${UE_GAMEMODE}" -game \
    -windowed -resx=1280 -resy=720 -stdout &
  UE_PID=$!
  echo "run_sim.sh: Unreal Engine started (pid $UE_PID). Close its window to stop."

  if [ "$DISTRIBUTED_MODE" -eq 1 ]; then
    wait_for_environment_bridges_ready
    arm_distributed_networks
  fi

  # When the engine exits, tear the brain servers down (handled by cleanup_all).
  wait "$UE_PID"
}

# ---------------------------------------------------------------------------
# Launch Webots via existing multi-robot launcher
# ---------------------------------------------------------------------------
WEBOTS_SCRIPT="$ROOT_DIR/scripts/run_multi_robot_webots.sh"

launch_webots() {
  if [ ! -x "$WEBOTS_SCRIPT" ]; then
    echo "run_sim.sh: Webots launcher not found or not executable: $WEBOTS_SCRIPT" >&2
    exit 1
  fi
  exec "$WEBOTS_SCRIPT" --robots "$ROBOT_SPEC" "${WEBOTS_PASSTHROUGH_ARGS[@]+"${WEBOTS_PASSTHROUGH_ARGS[@]}"}"
}

launch_webots_background() {
  if [ ! -x "$WEBOTS_SCRIPT" ]; then
    echo "run_sim.sh: Webots launcher not found or not executable: $WEBOTS_SCRIPT" >&2
    exit 1
  fi
  "$WEBOTS_SCRIPT" --robots "$ROBOT_SPEC" "${WEBOTS_PASSTHROUGH_ARGS[@]+"${WEBOTS_PASSTHROUGH_ARGS[@]}"}" &
  WEBOTS_PID=$!
  echo "run_sim.sh: Webots launcher started (pid $WEBOTS_PID)"
}

# ---------------------------------------------------------------------------
# Launch the browser WebGL simulator through the authenticated web gateway.
# ---------------------------------------------------------------------------
launch_webgl() {
  resolve_brain_arrays
  local total="${#BRAIN_IDS[@]}"
  if [ "$CLUSTER_NODE_COUNT" -lt "$total" ]; then
    echo "run_sim.sh: WebGL --node/--nodes=$CLUSTER_NODE_COUNT requires at least one worker per brain ($total)." >&2
    echo "  Use --nodes $total or a larger value so every brain retains an IPC owner." >&2
    exit 1
  fi

  local orch_port="$WEBGL_ORCHESTRATOR_PORT"
  if [ -z "$orch_port" ]; then
    orch_port="$(free_tcp_port 0)"
  else
    orch_port="$(free_tcp_port "$orch_port")"
  fi
  local web_port
  web_port="$(free_tcp_port "$WEBGL_PORT")"
  local brain_csv
  brain_csv="$(IFS=,; echo "${BRAIN_IDS[*]}")"
  local log_dir="$ROOT_DIR/logs/webgl_sim_${BASHPID}"
  mkdir -p "$log_dir"
  local ipc_dir="$log_dir/ipc"
  mkdir -p "$ipc_dir"

  local config_map="" network_map="" i brain_id brain_type path
  for (( i=0; i<total; i++ )); do
    brain_id="${BRAIN_IDS[$i]}"
    brain_type="${BRAIN_TYPES[$i]}"
    path="$(robot_config_file "$brain_type")"
    [ -f "$path" ] && config_map="${config_map:+$config_map,}${brain_id}=${path}"
    path="$(robot_network_file "$brain_type")"
    [ -f "$path" ] && network_map="${network_map:+$network_map,}${brain_id}=${path}"
  done
  [ -n "$CONFIG_MAP_CSV" ] && config_map="${config_map:+$config_map,}${CONFIG_MAP_CSV}"
  [ -n "$NETWORK_MAP_CSV" ] && network_map="${network_map:+$network_map,}${NETWORK_MAP_CSV}"

  if [ "$NO_BUILD" -eq 0 ]; then
    echo "run_sim.sh: building the AARNN cluster and browser gateway …"
    cargo build --release --locked --all-features --bin aarnn_rust
    cargo build --release --locked --all-features --bin web_ui
  fi
  if [ ! -x "$ROOT_DIR/target/release/aarnn_rust" ] || [ ! -x "$ROOT_DIR/target/release/web_ui" ]; then
    echo "run_sim.sh: WebGL requires target/release/aarnn_rust and target/release/web_ui." >&2
    exit 1
  fi
  if ! command -v strings >/dev/null 2>&1 || ! strings "$ROOT_DIR/target/release/aarnn_rust" | grep -F "[IpcUdsServer] Bound to" >/dev/null; then
    echo "run_sim.sh: target/release/aarnn_rust was built without the robot_io IPC runtime." >&2
    echo "  Re-run without --no-build, or build with --all-features." >&2
    exit 1
  fi

  local backend_cmd=(
    env
    "NM_BRAINS=$brain_csv"
    "NM_IPC_SOCKET_DIR=$ipc_dir"
    "LOG_DIR=$log_dir"
    "$ROOT_DIR/run_webot.sh"
    --runtime cluster --no-webots --no-diag --no-orchestrator-ui
    --node-ui-hidden --nodes "$CLUSTER_NODE_COUNT" --brains "$brain_csv"
    --orchestrator-port "$orch_port" --no-build
  )
  [ -n "$config_map" ] && backend_cmd+=(--config-map "$config_map")
  [ -n "$network_map" ] && backend_cmd+=(--network-map "$network_map")
  echo "run_sim.sh: launching WebGL cluster with $CLUSTER_NODE_COUNT worker(s) …"
  setsid "${backend_cmd[@]}" >"$log_dir/cluster.log" 2>&1 &
  WEBGL_BACKEND_PID="$!"

  local deadline=$((SECONDS + TCP_READY_TIMEOUT))
  while [ "$SECONDS" -le "$deadline" ]; do
    if ! kill -0 "$WEBGL_BACKEND_PID" 2>/dev/null; then
      echo "run_sim.sh: WebGL cluster exited before gRPC became ready; see $log_dir/cluster.log" >&2
      exit 1
    fi
    if tcp_port_listening "$orch_port"; then break; fi
    sleep 1
  done
  if ! tcp_port_listening "$orch_port"; then
    echo "run_sim.sh: timed out waiting for WebGL orchestrator on port $orch_port." >&2
    exit 1
  fi

  echo "run_sim.sh: launching browser gateway on $WEBGL_HOST:$web_port …"
  "$ROOT_DIR/target/release/web_ui" \
    --listen "$WEBGL_HOST:$web_port" \
    --orchestrator "http://127.0.0.1:$orch_port" \
    --default-network "${BRAIN_IDS[0]}" \
    --runtime-root "$log_dir/runtime" \
    >"$log_dir/web_ui.log" 2>&1 &
  WEBGL_WEB_PID="$!"
  local web_url="http://$WEBGL_HOST:$web_port"
  for _ in $(seq 1 "$TCP_READY_TIMEOUT"); do
    if curl --fail --silent --show-error --max-time 1 "$web_url/api/config" >/dev/null 2>&1; then break; fi
    if ! kill -0 "$WEBGL_WEB_PID" 2>/dev/null; then
      echo "run_sim.sh: WebGL gateway exited during startup; see $log_dir/web_ui.log" >&2
      exit 1
    fi
    sleep 1
  done
  if ! curl --fail --silent --show-error --max-time 1 "$web_url/api/config" >/dev/null 2>&1; then
    echo "run_sim.sh: timed out waiting for WebGL gateway at $web_url." >&2
    exit 1
  fi
  echo ""
  echo "AARNN WebGL simulator is ready: $web_url/sim/webgl?network_id=${BRAIN_IDS[0]}"
  echo "  brains: $brain_csv"
  echo "  workers: $CLUSTER_NODE_COUNT"
  echo "  logs: $log_dir"
  echo "Open the URL in a browser and select the matching brain/network ID."
  wait "$WEBGL_WEB_PID"
}

prepare_minecraft_token() {
  local edition="$1"
  if [ "$edition" != java ] || [ -n "${AARNN_MINECRAFT_TOKEN:-}" ]; then
    return 0
  fi

  local token
  if ! token="$(python3 -c 'import secrets; print(secrets.token_urlsafe(32))')" || [ "${#token}" -lt 24 ]; then
    echo "run_sim.sh: could not create a private Minecraft session token; set AARNN_MINECRAFT_TOKEN explicitly." >&2
    return 1
  fi
  export AARNN_MINECRAFT_TOKEN="$token"
  echo "run_sim.sh: generated a private Minecraft session token for this run; any launcher-started Java client will inherit it."
}

# ---------------------------------------------------------------------------
# Main dispatch
# ---------------------------------------------------------------------------
case "$SIM_BACKEND" in
  webots)
    launch_webots
    ;;
  unreal)
    if [ "$CLUSTER_NODE_COUNT_SET" -eq 1 ]; then
      start_distributed_tcp_servers
    else
      start_tcp_servers
    fi
    launch_unreal_engine
    ;;
  unity)
    # No Unity CLI integration; start the brains and let the user press Play in
    # the Unity editor (see scripts/run_unity_sim.sh for project setup).
    if [ "$CLUSTER_NODE_COUNT_SET" -eq 1 ]; then
      start_distributed_tcp_servers
    else
      start_tcp_servers
    fi
    serve_and_wait
    ;;
  webgl)
    launch_webgl
    ;;
  minecraft)
    # Detect every required capability before starting any brain process.
    minecraft_edition="$(python3 "$ROOT_DIR/scripts/minecraft.py" edition --edition "$MINECRAFT_EDITION")"
    prepare_minecraft_token "$minecraft_edition"
    python3 "$ROOT_DIR/scripts/minecraft.py" doctor --require bridge --edition "$minecraft_edition"
    if [ "$LAUNCH_ENGINE" -eq 1 ]; then
      python3 "$ROOT_DIR/scripts/minecraft.py" doctor --require engine --edition "$minecraft_edition"
    fi
    minecraft_bridge_jar="$(python3 "$ROOT_DIR/scripts/minecraft.py" artifact --kind bridge --edition "$minecraft_edition")"
    resolve_brain_arrays
    minecraft_profiles="$(IFS=,; echo "${BRAIN_TYPES[*]}")"
    if [ "${#BRAIN_TYPES[@]}" -gt 6 ] || [ "$(printf '%s\n' "${BRAIN_TYPES[@]}" | sort -u | wc -l)" -ne "${#BRAIN_TYPES[@]}" ]; then
      echo "Minecraft lab supports one robot of each profile (six maximum)." >&2
      exit 2
    fi
    if [ "$TCP_HOST" != 127.0.0.1 ]; then
      echo "Minecraft companion requires loopback Rust TCP endpoints." >&2
      exit 2
    fi
    if [ "$CLUSTER_NODE_COUNT_SET" -eq 1 ]; then start_distributed_tcp_servers; else start_tcp_servers; fi
    minecraft_java="$(python3 "$ROOT_DIR/scripts/minecraft.py" java)"
    "$minecraft_java" -jar "$minecraft_bridge_jar" \
      --base-port "$TCP_BASE_PORT" --profiles "$minecraft_profiles" &
    minecraft_bridge_pid="$!"
    TCP_PIDS+=("$minecraft_bridge_pid")
    python3 "$ROOT_DIR/scripts/minecraft.py" wait-bridge --pid "$minecraft_bridge_pid"
    if [ "$minecraft_edition" = bedrock ]; then
      echo "Bedrock lab: /scriptevent aarnn:world build; /scriptevent aarnn:visit <profile>; /scriptevent aarnn:stop"
      if [ "$LAUNCH_ENGINE" -eq 1 ]; then
        python3 "$ROOT_DIR/scripts/minecraft.py" launch --edition bedrock
      else
        wait "$minecraft_bridge_pid"
      fi
    else
      if [ "$LAUNCH_ENGINE" -eq 1 ]; then python3 "$ROOT_DIR/scripts/minecraft.py" launch --edition java; fi
      echo "Minecraft lab: /aarnn world, /aarnn visit <profile>, /aarnn connect <profile>; stop with /aarnn stop."
      wait "$minecraft_bridge_pid"
    fi
    ;;
  all)
    launch_webots_background
    if [ "$CLUSTER_NODE_COUNT_SET" -eq 1 ]; then
      start_distributed_tcp_servers
    else
      start_tcp_servers
    fi
    launch_unreal_engine
    ;;
esac
