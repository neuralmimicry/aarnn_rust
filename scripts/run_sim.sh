#!/usr/bin/env bash
# run_sim.sh — Unified AARNN simulator launcher
#
# Selects the simulation backend (Webots, Unreal, Unity, or all three) and
# starts the appropriate AARNN brain processes for the requested robot spec.
#
# Usage:
#   ./run_sim.sh [options]
#
# Options:
#   --sim <webots|unreal|unity|all>
#                     Simulation backend (default: webots).
#                       webots  — launch Webots + AARNN via run_multi_robot_webots.sh
#                       unreal  — start AARNN brains AND launch the Unreal project
#                                 (robots spawn and connect automatically)
#                       unity   — start AARNN brains; press Play in the Unity editor
#                       all     — launch Webots AND (Unreal + brains) concurrently
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
#   --no-build        Skip cargo build of nn_tcp_server.
#   --all-features    Build nn_tcp_server with cargo --all-features (parity testing).
#   --engine <path>   Unreal Engine directory (…/UnrealEngine/Engine).
#                     Default: $UE_ENGINE or /home/pbisaacs/Developer/Engine.
#   --uproject <path> Unreal .uproject to launch (default: sim/unreal/NeuralMimicrySim.uproject).
#   --map <name>      Boot map for --sim unreal (default: Template_Default).
#   --no-engine       For unreal/all: start brain servers only, don't launch Unreal.
#   --config-map <csv>
#                     Per-brain config map CSV (forwarded to Webots launcher).
#   --network-map <csv>
#                     Per-brain network map CSV (forwarded to Webots launcher).
#   --help            Show this usage message.
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

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
ROBOT_PROFILES_PY="$ROOT_DIR/scripts/robot_profiles.py"

if [ ! -f "$ROBOT_PROFILES_PY" ]; then
  echo "run_sim.sh: missing shared robot profile helper: $ROBOT_PROFILES_PY" >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# Defaults
# ---------------------------------------------------------------------------
SIM_BACKEND="${SIM_BACKEND:-webots}"
ROBOT_SPEC="${ROBOT_SPEC:-celegans=1}"
TCP_HOST="${TCP_HOST:-127.0.0.1}"
TCP_BASE_PORT="${TCP_BASE_PORT:-7890}"
TCP_READY_TIMEOUT="${TCP_READY_TIMEOUT:-600}"
NO_BUILD=0
BUILD_ALL_FEATURES=0
WEBOTS_PASSTHROUGH_ARGS=()

# Unreal Engine launch configuration (used when --sim unreal/all).
UE_ENGINE="${UE_ENGINE:-/home/pbisaacs/Developer/Engine}"
UPROJECT="${UPROJECT:-$ROOT_DIR/sim/unreal/NeuralMimicrySim.uproject}"
UE_MAP="${UE_MAP:-/Engine/Maps/Templates/Template_Default}"
UE_GAMEMODE="/Script/NmAerBridge.NmSimGameMode"
LAUNCH_ENGINE=1              # 0 = start brain servers only (no engine window)

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
    --config-map|--network-map)
      WEBOTS_PASSTHROUGH_ARGS+=("$1" "${2:-}")
      shift
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

# Validate --sim value
case "$SIM_BACKEND" in
  webots|unreal|unity|all) ;;
  *)
    echo "run_sim.sh: invalid --sim value '$SIM_BACKEND' (must be webots, unreal, unity, or all)" >&2
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
      else
        echo "  Build with: cargo build --release --features ui,robot_io --example nn_tcp_server" >&2
      fi
      exit 1
    fi
    return
  fi

  echo "run_sim.sh: building nn_tcp_server …"
  (
    cd "$ROOT_DIR"
    if [ "$BUILD_ALL_FEATURES" -eq 1 ]; then
      cargo build --release --all-features --example nn_tcp_server
    else
      cargo build --release --features ui,robot_io --example nn_tcp_server
    fi
  )
  locate_tcp_server_bin
  if [ -z "$TCP_SERVER_BIN" ]; then
    echo "run_sim.sh: build succeeded but nn_tcp_server binary could not be located" >&2
    exit 1
  fi
}

# ---------------------------------------------------------------------------
# Process tracking for TCP brain servers
# ---------------------------------------------------------------------------
TCP_PIDS=()
TCP_PORTS=()

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
  fi
}

cleanup_all() {
  cleanup_tcp_servers TERM
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
start_tcp_servers() {
  build_tcp_server

  # Parse spec into parallel arrays
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

# Block until the brain servers exit (used when no engine is auto-launched).
serve_and_wait() {
  echo ""
  echo "run_sim.sh: connect your simulation engine using:"
  echo "   Host : $TCP_HOST"
  echo "   Ports: $TCP_BASE_PORT – $(( TCP_BASE_PORT + ${#TCP_PIDS[@]} - 1 ))"
  echo ""
  echo "Press Ctrl-C to stop all servers."
  echo ""
  wait "${TCP_PIDS[@]}"
}

# ---------------------------------------------------------------------------
# Launch the Unreal Engine project (standalone -game) against the brain servers
# ---------------------------------------------------------------------------
launch_unreal_engine() {
  local ue_bin="$UE_ENGINE/Binaries/Linux/UnrealEditor"

  if [ "$LAUNCH_ENGINE" -ne 1 ]; then
    serve_and_wait
    return
  fi

  if [ ! -x "$ue_bin" ]; then
    echo "run_sim.sh: Unreal Engine binary not found: $ue_bin" >&2
    echo "  Set --engine <UE_root>/Engine or UE_ENGINE, or pass --no-engine to" >&2
    echo "  run the brain servers only and launch Unreal yourself." >&2
    serve_and_wait
    return
  fi
  if [ ! -f "$UPROJECT" ]; then
    echo "run_sim.sh: Unreal project not found: $UPROJECT" >&2
    echo "  Pass --uproject <path> or set UPROJECT. Falling back to server-only." >&2
    serve_and_wait
    return
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
# Main dispatch
# ---------------------------------------------------------------------------
case "$SIM_BACKEND" in
  webots)
    launch_webots
    ;;
  unreal)
    start_tcp_servers
    launch_unreal_engine
    ;;
  unity)
    # No Unity CLI integration; start the brains and let the user press Play in
    # the Unity editor (see scripts/run_unity_sim.sh for project setup).
    start_tcp_servers
    serve_and_wait
    ;;
  all)
    launch_webots_background
    start_tcp_servers
    launch_unreal_engine
    ;;
esac
