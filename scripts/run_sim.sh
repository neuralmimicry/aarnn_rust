#!/usr/bin/env bash
# run_sim.sh — Unified AARNN simulator launcher
#
# Selects a native physics/reference or preserved legacy simulator route.
# Implemented legacy projections may reuse the existing local simulation cluster.

export NM_MORPHO_ASYNC="${NM_MORPHO_ASYNC:-1}"
#
# Usage:
#   ./run_sim.sh [options]
#
# Options:
#   --sim <webots|unreal|unity|webgl|minecraft|mujoco|isaacsim|all>
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
#                                      hexapod, nao, zebrafish; plus 22 physical profiles:
#                       galbot_g1_golf, crow_corvus_corone, ugot_mecanum,
#                       ugot_quadruped, ugot_spider, ugot_balance, ugot_wheelleg,
#                       ugot_engineer, ugot_transform, keenon_t8, keenon_t9,
#                       keenon_t10, keenon_t11, keenon_w3, keenon_s100,
#                       keenon_c30, keenon_c40, cofe_robot_cafe, cofe_smart_bar,
#                       botinkit_omni, xbot_ic60, xbot_s_pro.
#                     New Webots/Unity/Unreal profiles dispatch native factories;
#                     MuJoCo supports all 28. Isaac needs its own supported runtime.
#                     Minecraft/WebGL functional mode uses --physics-service.
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
#                     MuJoCo C. elegans uses this same cluster and preserved network.
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
#   --reference-python  Explicit MuJoCo Python reference/passive-viewer comparison.
#   --physics-service  For webgl/mujoco/minecraft: private native Rust physics service and
#                     WebGL viewer, one body, explicit --reference-binding only.
#                     --token-file PATH [--port N] are forwarded to the service.
#   --control <auto|npc|silent|aarnn>
#                     Auto: programmed NPC until a verified brain is ready.
#                     A connected silent brain owns its body; no NPC drive mixes in.
#                     Explicit control also selects physical ports of old profiles.
#   --headless --steps N --realtime --reference-bindings MAP.json
#                     Native physics options; per-instance reference bindings are opt-in.
#                     MuJoCo opens a persistent Rust viewer by default. All six legacy
#                     profiles load their preserved brains; --control npc|silent opts out.
#                     New profiles remain NPC unless selected with --network-map.
#                     --headless --steps N is a bounded run; --json prints full state.
#   --view body|habitat  Initial MuJoCo camera framing; F switches while running.
#   --output PATH    Evidence file (MuJoCo) or new directory (Webots/Unity/Unreal).
#                     Unity needs NM_UNITY_EDITOR; Unreal uses --engine or UE_ENGINE.
#                     --all does not claim native expansion target parity.
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
source "$ROOT_DIR/scripts/webots_runtime_profile.sh"

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
EMBODIMENT_REFERENCE_PYTHON=0
EMBODIMENT_SERVICE=0
EMBODIMENT_CONTROL=""
MUJOCO_MANAGED=0
MUJOCO_PID=""
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
WEBGL_RUNTIME_FEATURES="${NM_WEBGL_RUNTIME_FEATURES:-engine_runtime,ui,robot_io,morpho,cuda}"

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
    --reference-python)
      EMBODIMENT_REFERENCE_PYTHON=1
      ;;
    --control)
      shift
      EMBODIMENT_CONTROL="${1:?--control requires auto, npc, silent or aarnn}"
      WEBOTS_PASSTHROUGH_ARGS+=(--control "$EMBODIMENT_CONTROL")
      ;;
    --physics-service)
      EMBODIMENT_SERVICE=1
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

if [ "$EMBODIMENT_SERVICE" -eq 1 ]; then
  if [[ "$SIM_BACKEND" != webgl && "$SIM_BACKEND" != mujoco && "$SIM_BACKEND" != minecraft ]] || [ "$CLUSTER_NODE_COUNT_SET" -eq 1 ] || [ -n "$CONFIG_MAP_CSV$NETWORK_MAP_CSV" ] || [ "$EMBODIMENT_REFERENCE_PYTHON" -eq 1 ]; then
    echo 'Physics service requires --sim webgl|mujoco|minecraft, one reference body and explicit binding; cluster remapping is unavailable.' >&2
    exit 2
  fi
  embodiment_profile="$(python3 -c 'import sys; sys.path.insert(0,"scripts"); from robot_profiles import iter_brain_instances; rows=list(iter_brain_instances(sys.argv[1])); assert len(rows)==1,"physics service requires exactly one body"; print(rows[0][1])' "$ROBOT_SPEC")"
  export AARNN_MUJOCO_DIR="${AARNN_MUJOCO_DIR:-$HOME/Developer/mujoco-3.17.0}"
  export LD_LIBRARY_PATH="$AARNN_MUJOCO_DIR/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
  if [ "$NO_BUILD" -eq 0 ]; then cargo build --locked -p aarnn-mujoco --features service; fi
  exec "$ROOT_DIR/target/debug/aarnn-physics-service" --profile "$embodiment_profile" --port "$WEBGL_PORT" "${WEBOTS_PASSTHROUGH_ARGS[@]}"
fi

if [ -n "$EMBODIMENT_CONTROL" ] && [[ "$SIM_BACKEND" = webgl || "$SIM_BACKEND" = minecraft || "$SIM_BACKEND" = all ]]; then
  echo 'Explicit physical control requires --physics-service for WebGL/Minecraft, or one native target at a time; preview/combined routes do not accept this flag.' >&2
  exit 2
fi

python3 "$ROBOT_PROFILES_PY" validate-target "$SIM_BACKEND" "$ROBOT_SPEC"

if [ "$SIM_BACKEND" = webots ] && { [ -n "$EMBODIMENT_CONTROL" ] || python3 "$ROBOT_PROFILES_PY" has-new "$ROBOT_SPEC"; }; then
  if [ "$CLUSTER_NODE_COUNT_SET" -eq 1 ] || [ -n "$CONFIG_MAP_CSV$NETWORK_MAP_CSV" ]; then
    echo 'Native Webots expansion reference mode requires explicit --reference-bindings; cluster remapping is unavailable.' >&2
    exit 2
  fi
  embodiment_python="${NM_MUJOCO_PYTHON:-$ROOT_DIR/target/robot-mujoco-venv/bin/python}"
  embodiment_options=()
  if [ "$NO_BUILD" -eq 1 ]; then embodiment_options+=(--no-build); fi
  exec "$embodiment_python" "$ROOT_DIR/scripts/run_webots_embodiment.py" --robots "$ROBOT_SPEC" "${embodiment_options[@]}" "${WEBOTS_PASSTHROUGH_ARGS[@]}"
fi

# New game-engine bodies use the canonical physical graph and shared Rust ABI.
if [[ "$SIM_BACKEND" = unity || "$SIM_BACKEND" = unreal ]] && { [ -n "$EMBODIMENT_CONTROL" ] || python3 "$ROBOT_PROFILES_PY" has-new "$ROBOT_SPEC"; }; then
  if [ "$CLUSTER_NODE_COUNT_SET" -eq 1 ] || [ -n "$CONFIG_MAP_CSV$NETWORK_MAP_CSV" ]; then
    echo 'Native graph reference mode requires explicit --reference-bindings; distributed remapping is unavailable.' >&2
    exit 2
  fi
  embodiment_options=()
  if [ "$NO_BUILD" -eq 1 ]; then embodiment_options+=(--no-build); fi
  export UE_ENGINE
  exec python3 "$ROOT_DIR/scripts/run_native_embodiment.py" --target "$SIM_BACKEND" --robots "$ROBOT_SPEC" "${embodiment_options[@]}" "${WEBOTS_PASSTHROUGH_ARGS[@]}"
fi

# Native embodiment adapters own their isolated environment and lifecycle.
# Forward only simulation options; they never start a physical peripheral.
if [ "$SIM_BACKEND" = mujoco ] && [ "$EMBODIMENT_REFERENCE_PYTHON" -eq 0 ] && [[ "$EMBODIMENT_CONTROL" != npc && "$EMBODIMENT_CONTROL" != silent ]]; then
  explicit_bindings=0
  for arg in "${WEBOTS_PASSTHROUGH_ARGS[@]}"; do
    case "$arg" in --reference-binding|--reference-bindings) explicit_bindings=1;; esac
  done
  if [ "$explicit_bindings" -eq 0 ] && python3 "$ROBOT_PROFILES_PY" managed-brains "$ROBOT_SPEC" --network-map "$NETWORK_MAP_CSV" >/dev/null
  then MUJOCO_MANAGED=1; fi
fi
if { [ "$SIM_BACKEND" = mujoco ] && [ "$MUJOCO_MANAGED" -eq 0 ]; } || [ "$SIM_BACKEND" = isaacsim ]; then
  if [ "$CLUSTER_NODE_COUNT_SET" -eq 1 ] || [ -n "$CONFIG_MAP_CSV$NETWORK_MAP_CSV" ]; then
    echo 'Native embodiment reference mode requires explicit --reference-bindings; cluster remapping is unavailable.' >&2
    exit 2
  fi
  if [ "$SIM_BACKEND" = mujoco ]; then
    if [ "$EMBODIMENT_REFERENCE_PYTHON" -eq 0 ]; then
      export AARNN_MUJOCO_DIR="${AARNN_MUJOCO_DIR:-$HOME/Developer/mujoco-3.17.0}"
      if [ ! -f "$AARNN_MUJOCO_DIR/lib/libmujoco.so.3.17.0" ]; then
        echo 'MuJoCo 3.17.0 native SDK unavailable; set AARNN_MUJOCO_DIR to its unpacked directory.' >&2
        exit 2
      fi
      export LD_LIBRARY_PATH="$AARNN_MUJOCO_DIR/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
      if [ "$NO_BUILD" -eq 0 ]; then cargo build --locked -p aarnn-mujoco --features viewer; fi
      embodiment_binary="${NM_MUJOCO_BIN:-$ROOT_DIR/target/debug/aarnn-mujoco}"
      exec "$embodiment_binary" --robots "$ROBOT_SPEC" --assets "$ROOT_DIR/sim/content/embodiments/generated" "${WEBOTS_PASSTHROUGH_ARGS[@]}"
    fi
    embodiment_python="${NM_MUJOCO_PYTHON:-$ROOT_DIR/target/robot-mujoco-venv/bin/python}"
  else
    embodiment_python="${NM_ISAACSIM_PYTHON:-}"
    if [ -z "$embodiment_python" ]; then
      echo 'Isaac Sim unavailable: set NM_ISAACSIM_PYTHON to its supported, separate python.sh.' >&2
      exit 2
    fi
  fi
  exec "$embodiment_python" "$ROOT_DIR/scripts/run_embodied_sim.py" --sim "$SIM_BACKEND" --robots "$ROBOT_SPEC" "${WEBOTS_PASSTHROUGH_ARGS[@]}"
fi

# Validate --sim value
case "$SIM_BACKEND" in
  webots|unreal|unity|webgl|minecraft|mujoco|all) ;;
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
if [ "$CLUSTER_NODE_COUNT_SET" -eq 1 ] && [ "$SIM_BACKEND" != mujoco ]; then
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
  if [ -n "$MUJOCO_PID" ] && kill -0 "$MUJOCO_PID" 2>/dev/null; then
    kill -TERM "$MUJOCO_PID" 2>/dev/null || true
    wait "$MUJOCO_PID" 2>/dev/null || true
  fi
  MUJOCO_PID=""
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
  if [ "$MUJOCO_MANAGED" -eq 1 ]; then
    brain_lines="$(python3 "$ROBOT_PROFILES_PY" managed-brains "$ROBOT_SPEC" --network-map "$NETWORK_MAP_CSV")" || exit 1
  elif ! brain_lines="$(parse_robot_spec "$ROBOT_SPEC")"; then
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
      line="$(grep -F " - Network ${brain}:" "$placement_log" 2>/dev/null | tail -n 1 || true)"
      if [[ "$line" != *"Distributed across ${expected} nodes"* ]]; then
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
  if [ "$MUJOCO_MANAGED" -eq 1 ] && [ "$CLUSTER_NODE_COUNT_SET" -eq 0 ] && [ -z "${NM_CLUSTER_NODES:-}" ] && [ "$CLUSTER_NODE_COUNT" -lt "$total" ]; then
    CLUSTER_NODE_COUNT="$total"
  fi
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

  if [ "$MUJOCO_MANAGED" -eq 1 ]; then
    for (( i=0; i<total; i++ )); do
      python3 "$ROOT_DIR/scripts/mujoco_bindings.py" --profile "${BRAIN_TYPES[$i]}" --brain "${BRAIN_IDS[$i]}" \
        --network "$(robot_network_file "${BRAIN_TYPES[$i]}")" --network-map "$cluster_network_map" \
        --config "$(robot_config_file "${BRAIN_TYPES[$i]}")" --config-map "$cluster_config_map" \
        --host "$TCP_HOST" --port "$((TCP_BASE_PORT+i))" --output "$ENV_READY_DIR"
    done
  fi

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
    --connect-timeout "$TCP_READY_TIMEOUT"
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
    local -a embodiment_args=()
    if [ "$MUJOCO_MANAGED" -eq 1 ]; then embodiment_args=(--embodiment-contract "$ENV_READY_DIR/${brain_id}.contract.json"); fi
    rm -f "$ready_file"
    "$TCP_AER_BRIDGE_BIN" \
      --listen "$TCP_HOST:$port" --ipc "$ipc_path" \
      --sensory "$(robot_sensory "$brain_type")" \
      --output "$(robot_output "$brain_type")" \
      --ready-file "$ready_file" \
      --arm-file "$ARM_FILE" \
      "${embodiment_args[@]}" \
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
  echo "run_sim.sh: waiting for simulator environment readiness (all bridge handshakes, timeout ${TCP_READY_TIMEOUT}s) …"
  while [ "$SECONDS" -le "$deadline" ]; do
    if [ -n "$MUJOCO_PID" ] && ! kill -0 "$MUJOCO_PID" 2>/dev/null; then
      echo "run_sim.sh: MuJoCo exited before environment readiness; see $CLUSTER_LOG_DIR/mujoco.log" >&2
      return 1
    fi
    ready_count=0
    local i
    for (( i=0; i<total; i++ )); do
      local pid="${BRIDGE_PIDS[$i]}"
      local brain_id="${BRAIN_IDS[$i]}"
      if ! kill -0 "$pid" 2>/dev/null; then
        echo "run_sim.sh: bridge for ${brain_id} exited before the simulator environment became ready." >&2
        echo "  See: $CLUSTER_LOG_DIR/bridge_${brain_id}.log" >&2
        exit 1
      fi
      if [ -f "$ENV_READY_DIR/${brain_id}.ready" ]; then
        ready_count=$((ready_count + 1))
      fi
    done
    if [ "$ready_count" -eq "$total" ]; then
      echo "run_sim.sh: simulator environment ready; all ${total} brain handshake(s) accepted."
      return 0
    fi
    sleep 1
  done
  echo "run_sim.sh: timed out waiting for simulator environment handshakes (${ready_count}/${total})." >&2
  exit 1
}

send_distributed_start() {
  local brain_id="$1"
  local control_bin="$ROOT_DIR/target/release/aarnn_rust"
  if [ ! -x "$control_bin" ]; then
    echo "run_sim.sh: missing cluster-control binary: $control_bin" >&2
    exit 1
  fi
  local runtime_features
  runtime_features="$(webots_runtime_profile)"
  local -a control_env=()
  if ! webots_profile_has_feature "$runtime_features" management_v1; then
    # run_webot.sh removes local-management TLS variables for the default
    # plaintext Webots profile. Match that child runtime for this control
    # client; otherwise rustls attempts TLS against the plaintext listener and
    # reports InvalidContentType during the handshake.
    control_env=(
      env
      -u NM_GRPC_TLS_CERT
      -u NM_GRPC_TLS_KEY
      -u NM_GRPC_TLS_CA
      -u NM_GRPC_TLS_DOMAIN
    )
  fi
  "${control_env[@]}" "$control_bin" \
    --orchestrator-addr "http://127.0.0.1:$CLUSTER_ORCHESTRATOR_PORT" \
    --cluster-control-network "$brain_id" \
    --cluster-control-action start
}

arm_distributed_networks() {
  # Desired placement precedes asynchronous LoadNetwork. A missing worker
  # network ignores legacy Start; membership may also change during import.
  wait_for_distributed_networks_loaded
  wait_for_distributed_placement_ready
  local brain_id
  for brain_id in "${BRAIN_IDS[@]}"; do
    echo "run_sim.sh: arming distributed network '$brain_id' after environment readiness …"
    send_distributed_start "$brain_id"
  done

  wait_for_distributed_networks_armed
  local temporary="$ARM_FILE.tmp-$$"
  printf 'armed\n' >"$temporary"
  mv -f "$temporary" "$ARM_FILE"
  echo "run_sim.sh: distributed neural processing armed after environment readiness."
}

distributed_worker_log_paths() {
  local brain_id
  for brain_id in "${BRAIN_IDS[@]}"; do
    printf '%s\n' "$CLUSTER_LOG_DIR/webots_${brain_id}.log"
  done
  local extra_index=1
  local extra_workers=$((CLUSTER_NODE_COUNT - ${#BRAIN_IDS[@]}))
  while [ "$extra_index" -le "$extra_workers" ]; do
    printf '%s\n' "$CLUSTER_LOG_DIR/webots_${BRAIN_IDS[0]}_worker_$(printf '%02d' "$extra_index").log"
    extra_index=$((extra_index + 1))
  done
}

wait_for_distributed_networks_loaded() {
  local deadline=$((SECONDS + TCP_READY_TIMEOUT))
  local -a worker_logs=()
  mapfile -t worker_logs < <(distributed_worker_log_paths)
  echo "run_sim.sh: waiting for initial network loads on all distributed workers …"
  while [ "$SECONDS" -le "$deadline" ]; do
    local all_ready=1 log_file brain_id
    for log_file in "${worker_logs[@]}"; do
      for brain_id in "${BRAIN_IDS[@]}"; do
        if ! grep -Fq "Network ${brain_id} worker started on node " "$log_file" 2>/dev/null; then
          all_ready=0
          break 2
        fi
      done
    done
    if [ "$all_ready" -eq 1 ]; then
      echo "run_sim.sh: initial network loads confirmed on all ${#worker_logs[@]} distributed worker(s)."
      return 0
    fi
    for pid in "${CLUSTER_PIDS[@]}"; do
      if ! kill -0 "$pid" 2>/dev/null; then
        echo "run_sim.sh: distributed runtime exited before initial network loads completed." >&2
        exit 1
      fi
    done
    sleep 1
  done
  echo "run_sim.sh: timed out waiting for initial distributed network loads." >&2
  exit 1
}

wait_for_distributed_networks_armed() {
  local deadline=$((SECONDS + TCP_READY_TIMEOUT))
  local next_retry=$((SECONDS + 5))
  local -a worker_logs=()
  mapfile -t worker_logs < <(distributed_worker_log_paths)
  local brain_id

  echo "run_sim.sh: waiting for Start to reach all distributed workers …"
  while [ "$SECONDS" -le "$deadline" ]; do
    local -a missing_brains=()
    local log_file brain_ready
    for brain_id in "${BRAIN_IDS[@]}"; do
      brain_ready=1
      for log_file in "${worker_logs[@]}"; do
        if ! grep -Fq "simulator network ${brain_id} armed (playing=true)" "$log_file" 2>/dev/null; then
          brain_ready=0
          break
        fi
      done
      if [ "$brain_ready" -eq 0 ]; then missing_brains+=("$brain_id"); fi
    done
    if [ "${#missing_brains[@]}" -eq 0 ]; then
      echo "run_sim.sh: Start reached all ${#worker_logs[@]} distributed worker(s) for all ${#BRAIN_IDS[@]} network(s)."
      return 0
    fi
    if [ "$SECONDS" -ge "$next_retry" ]; then
      # Start only sets playing=true in the existing reference RPC. Retry
      # unacknowledged delivery after membership/load changes, without Reset,
      # Repeat, biological time advancement or a new actuator route.
      for brain_id in "${missing_brains[@]}"; do
        echo "run_sim.sh: retrying idempotent Start for '$brain_id' pending worker acknowledgement …"
        send_distributed_start "$brain_id"
      done
      next_retry=$((SECONDS + 5))
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
  if [ "$NO_BUILD" -eq 1 ]; then
    WEBOTS_PASSTHROUGH_ARGS+=(--no-build)
  fi
  exec "$WEBOTS_SCRIPT" --robots "$ROBOT_SPEC" "${WEBOTS_PASSTHROUGH_ARGS[@]+"${WEBOTS_PASSTHROUGH_ARGS[@]}"}"
}

launch_webots_background() {
  if [ ! -x "$WEBOTS_SCRIPT" ]; then
    echo "run_sim.sh: Webots launcher not found or not executable: $WEBOTS_SCRIPT" >&2
    exit 1
  fi
  if [ "$NO_BUILD" -eq 1 ]; then
    WEBOTS_PASSTHROUGH_ARGS+=(--no-build)
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
  # Keep the delegated backend, cluster-control client, and WebGL gateway on
  # one transport profile.  The parent launcher may have provisioned local
  # management credentials for another simulator path, but the default WebGL
  # profile is plaintext reference gRPC.
  export NM_WEBOTS_RUNTIME_FEATURES="$WEBGL_RUNTIME_FEATURES"
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
    # Keep the WebGL backend on the same explicit local profile as the
    # delegated Webots launcher.  --all-features enables management_v1, whose
    # orchestrator deliberately fails closed unless production bearer/TLS
    # credentials are configured; the local WebGL cluster uses the plaintext
    # reference gRPC path instead.
    local -a webgl_runtime_args=()
    read -r -a webgl_runtime_args <<< "$(webots_cargo_profile_args "$WEBGL_RUNTIME_FEATURES")"
    cargo build --release --locked "${webgl_runtime_args[@]}" --bin aarnn_rust
    # web_ui is a headless HTTP gateway in this mode.  Keep its feature graph
    # explicit too, so a stale default/all-feature build cannot reintroduce an
    # authenticated management client/server mismatch.
    cargo build --release --locked --no-default-features --features engine_runtime --bin web_ui
  fi
  if [ ! -x "$ROOT_DIR/target/release/aarnn_rust" ] || [ ! -x "$ROOT_DIR/target/release/web_ui" ]; then
    echo "run_sim.sh: WebGL requires target/release/aarnn_rust and target/release/web_ui." >&2
    exit 1
  fi
  if ! command -v strings >/dev/null 2>&1 || ! strings "$ROOT_DIR/target/release/aarnn_rust" | grep -F "[IpcUdsServer] Bound to" >/dev/null; then
    echo "run_sim.sh: target/release/aarnn_rust was built without the robot_io IPC runtime." >&2
    echo "  Re-run without --no-build, or include robot_io in NM_WEBGL_RUNTIME_FEATURES." >&2
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
  local -a web_ui_env=()
  if ! webots_profile_has_feature "$WEBGL_RUNTIME_FEATURES" management_v1; then
    web_ui_env=(
      env
      -u NM_GRPC_TLS_CERT
      -u NM_GRPC_TLS_KEY
      -u NM_GRPC_TLS_CA
      -u NM_GRPC_TLS_DOMAIN
    )
  fi
  "${web_ui_env[@]}" "$ROOT_DIR/target/release/web_ui" \
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
  mujoco)
    export AARNN_MUJOCO_DIR="${AARNN_MUJOCO_DIR:-$HOME/Developer/mujoco-3.17.0}"
    export LD_LIBRARY_PATH="$AARNN_MUJOCO_DIR/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    if [ "$NO_BUILD" -eq 0 ]; then cargo build --locked -p aarnn-mujoco --features viewer; fi
    # Config/network maps are consumed by the shared runtime, never passed to
    # a physics owner. All other native options retain their original argv.
    native_args=()
    i=0
    while [ "$i" -lt "${#WEBOTS_PASSTHROUGH_ARGS[@]}" ]; do
      arg="${WEBOTS_PASSTHROUGH_ARGS[$i]}"
      case "$arg" in
        --config-map|--network-map) i=$((i+2)); continue;;
      esac
      native_args+=("$arg");i=$((i+1))
    done
    start_distributed_tcp_servers
    echo "run_sim.sh: MuJoCo logs and evidence: $CLUSTER_LOG_DIR"
    NM_AER_REFERENCE_STARTUP_TIMEOUT_MS="$((TCP_READY_TIMEOUT>3600?3600000:TCP_READY_TIMEOUT*1000))" \
    "${NM_MUJOCO_BIN:-$ROOT_DIR/target/debug/aarnn-mujoco}" --robots "$ROBOT_SPEC" \
      --assets "$ROOT_DIR/sim/content/embodiments/generated" \
      --reference-bindings "$ENV_READY_DIR/mujoco-bindings.json" --arm-file "$ARM_FILE" \
      --output "$CLUSTER_LOG_DIR/mujoco.json" "${native_args[@]}" \
      > >(tee "$CLUSTER_LOG_DIR/mujoco.log") 2> >(tee -a "$CLUSTER_LOG_DIR/mujoco.log" >&2) &
    MUJOCO_PID=$!
    wait_for_environment_bridges_ready
    arm_distributed_networks
    wait "$MUJOCO_PID"
    MUJOCO_PID=""
    ;;
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
    if [ "$DISTRIBUTED_MODE" -eq 1 ]; then
      wait_for_environment_bridges_ready
      arm_distributed_networks
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
    if [ "$DISTRIBUTED_MODE" -eq 1 ]; then
      arm_distributed_networks
    fi
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
