#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

UI_MODE="${UI_MODE:-rust}"  # rust|web|cli
UI_MODE_SET_BY_USER=0
ROBOT_SPEC="${ROBOT_SPEC:-celegans=1}"
REMOTE_COMPUTE="${REMOTE_COMPUTE:-0}"
ORCHESTRATOR_PORT="${ORCHESTRATOR_PORT:-50051}"
WEB_UI_LISTEN="${WEB_UI_LISTEN:-0.0.0.0:8080}"
WORLD_FILE="${WORLD_FILE:-$ROOT_DIR/webots_world/worlds/multi_neuroworld.wbt}"
TMP_CELEGANS_WORLD="${TMP_CELEGANS_WORLD:-/tmp/aarnn_tmp_celegans_assets_ignore.wbt}"
TMP_DROSOPHILA_WORLD="${TMP_DROSOPHILA_WORLD:-/tmp/aarnn_tmp_drosophila_assets_ignore.wbt}"
WEBOTS_RUNTIME_ROOT="${WEBOTS_RUNTIME_ROOT:-${NM_RUNTIME_ROOT:-$ROOT_DIR/data/runtime}}"
WEBOTS_RUNTIME_USER="${WEBOTS_RUNTIME_USER:-webots}"
WEBOTS_WORKSPACE_PREFIX="${WEBOTS_WORKSPACE_PREFIX:-webots}"
WEBOTS_WORKSPACE_AUTOSAVE_STEPS="${WEBOTS_WORKSPACE_AUTOSAVE_STEPS:-10}"
WEBOTS_WORKSPACE_RESUME_EXISTING_SET_BY_USER=0
if [ -n "${WEBOTS_WORKSPACE_RESUME_EXISTING+x}" ]; then
  WEBOTS_WORKSPACE_RESUME_EXISTING_SET_BY_USER=1
fi
WEBOTS_WORKSPACE_RESUME_EXISTING="${WEBOTS_WORKSPACE_RESUME_EXISTING:-1}"
WEBOTS_WORKSPACE_RESUME_EFFECTIVE="$WEBOTS_WORKSPACE_RESUME_EXISTING"

COUNT_CELEGANS_OVERRIDE=""
COUNT_DROSOPHILA_BANC_OVERRIDE=""
COUNT_DROSOPHILA_FAFB_OVERRIDE=""
COUNT_HEXAPOD_OVERRIDE=""
COUNT_NAO_OVERRIDE=""
COUNT_ZEBRAFISH_OVERRIDE=""

# Core asset paths
CELEGANS_NETWORK_FILE="${CELEGANS_NETWORK_FILE:-$ROOT_DIR/network_celegans.json}"
CELEGANS_CONFIG_FILE="${CELEGANS_CONFIG_FILE:-$ROOT_DIR/webots_world/configs/config_celegans_webots.json}"
CELEGANS_PROTO_FILE="${CELEGANS_PROTO_FILE:-$ROOT_DIR/webots_world/protos/CelegansRobot.proto}"

DROSOPHILA_BANC_NETWORK_FILE="${DROSOPHILA_BANC_NETWORK_FILE:-$ROOT_DIR/network_drosophila_banc.json}"
DROSOPHILA_FAFB_NETWORK_FILE="${DROSOPHILA_FAFB_NETWORK_FILE:-$ROOT_DIR/network_drosophila_fafb.json}"
DROSOPHILA_BANC_CONFIG_FILE="${DROSOPHILA_BANC_CONFIG_FILE:-$ROOT_DIR/webots_world/configs/config_drosophila_banc_webots.json}"
DROSOPHILA_FAFB_CONFIG_FILE="${DROSOPHILA_FAFB_CONFIG_FILE:-$ROOT_DIR/webots_world/configs/config_drosophila_fafb_webots.json}"
DROSOPHILA_BANC_PROTO_FILE="${DROSOPHILA_BANC_PROTO_FILE:-$ROOT_DIR/webots_world/protos/DrosophilaBancRobot.proto}"
DROSOPHILA_FAFB_PROTO_FILE="${DROSOPHILA_FAFB_PROTO_FILE:-$ROOT_DIR/webots_world/protos/DrosophilaFafbRobot.proto}"

NAO_NETWORK_FILE="${NAO_NETWORK_FILE:-$ROOT_DIR/network_nao.json}"
NAO_CONFIG_FILE="${NAO_CONFIG_FILE:-$ROOT_DIR/webots_world/configs/config_nao_webots.json}"

HEXAPOD_NETWORK_FILE="${HEXAPOD_NETWORK_FILE:-$ROOT_DIR/network_hexapod.json}"
HEXAPOD_CONFIG_FILE="${HEXAPOD_CONFIG_FILE:-$ROOT_DIR/webots_world/configs/config_hexapod_webots.json}"
HEXAPOD_PROTO_FILE="${HEXAPOD_PROTO_FILE:-$ROOT_DIR/webots_world/protos/HexapodRobot.proto}"
HEXAPOD_TEMPLATE_FILE="${HEXAPOD_TEMPLATE_FILE:-$ROOT_DIR/network.json}"
HEXAPOD_NETWORK_SCRIPT="${HEXAPOD_NETWORK_SCRIPT:-$ROOT_DIR/scripts/build_hexapod_network_json.py}"
HEXAPOD_CAMERA_RETINA_WIDTH="${HEXAPOD_CAMERA_RETINA_WIDTH:-1}"
HEXAPOD_CAMERA_RETINA_HEIGHT="${HEXAPOD_CAMERA_RETINA_HEIGHT:-1}"
HEXAPOD_EXPECTED_OUTPUT="${HEXAPOD_EXPECTED_OUTPUT:-18}"
HEXAPOD_HIDDEN_LAYERS="${HEXAPOD_HIDDEN_LAYERS:-6}"
HEXAPOD_HIDDEN_PER_LAYER="${HEXAPOD_HIDDEN_PER_LAYER:-${HEXAPOD_HIDDEN_NEURONS:-96}}"
HEXAPOD_AARNN_DEPTH="${HEXAPOD_AARNN_DEPTH:-4}"
HEXAPOD_GROWTH_HEADROOM="${HEXAPOD_GROWTH_HEADROOM:-1.8}"
HEXAPOD_REBUILD_NETWORK="${HEXAPOD_REBUILD_NETWORK:-0}"

# Drosophila network build controls
DROSOPHILA_BANC_DIR="${DROSOPHILA_BANC_DIR:-$ROOT_DIR/data/drosophila/BANC v626}"
DROSOPHILA_FAFB_DIR="${DROSOPHILA_FAFB_DIR:-$ROOT_DIR/data/drosophila/FAFB v783}"
DROSOPHILA_TEMPLATE_FILE="${DROSOPHILA_TEMPLATE_FILE:-$ROOT_DIR/network.json}"
DROSOPHILA_EYE_CAMERAS="${DROSOPHILA_EYE_CAMERAS:-1}"
DROSOPHILA_EYE_RETINA_WIDTH="${DROSOPHILA_EYE_RETINA_WIDTH:-12}"
DROSOPHILA_EYE_RETINA_HEIGHT="${DROSOPHILA_EYE_RETINA_HEIGHT:-8}"
DROSOPHILA_EYE_CAMERA_WIDTH="${DROSOPHILA_EYE_CAMERA_WIDTH:-32}"
DROSOPHILA_EYE_CAMERA_HEIGHT="${DROSOPHILA_EYE_CAMERA_HEIGHT:-24}"
DROSOPHILA_MAX_HIDDEN="${DROSOPHILA_MAX_HIDDEN:-20000}"
DROSOPHILA_MAX_OUTPUT="${DROSOPHILA_MAX_OUTPUT:-48}"
DROSOPHILA_MIN_SYN_COUNT="${DROSOPHILA_MIN_SYN_COUNT:-1}"
DROSOPHILA_WEIGHT_TRANSFORM="${DROSOPHILA_WEIGHT_TRANSFORM:-sqrt}"
DROSOPHILA_HIDDEN_LAYER_WIDTH="${DROSOPHILA_HIDDEN_LAYER_WIDTH:-512}"
DROSOPHILA_LONG_RANGE_POLICY="${DROSOPHILA_LONG_RANGE_POLICY:-fold}"
DROSOPHILA_REBUILD_NETWORK="${DROSOPHILA_REBUILD_NETWORK:-0}"

case "${DROSOPHILA_EYE_CAMERAS,,}" in
  1|true|yes|on|enabled) DROSOPHILA_EYE_CAMERAS=1 ;;
  0|false|no|off|disabled) DROSOPHILA_EYE_CAMERAS=0 ;;
  *)
    echo "Invalid DROSOPHILA_EYE_CAMERAS='$DROSOPHILA_EYE_CAMERAS' (use 0/1 or off/on)."
    exit 1
    ;;
esac
for _v in DROSOPHILA_EYE_RETINA_WIDTH DROSOPHILA_EYE_RETINA_HEIGHT DROSOPHILA_EYE_CAMERA_WIDTH DROSOPHILA_EYE_CAMERA_HEIGHT; do
  if ! [[ "${!_v}" =~ ^[0-9]+$ ]] || [ "${!_v}" -le 0 ]; then
    echo "Invalid ${_v}='${!_v}' (must be positive integer)."
    exit 1
  fi
done
DROSOPHILA_BASE_SENSORY=34
DROSOPHILA_EYE_CHANNELS=0
if [ "$DROSOPHILA_EYE_CAMERAS" = "1" ]; then
  DROSOPHILA_EYE_CHANNELS=$((4 * DROSOPHILA_EYE_RETINA_WIDTH * DROSOPHILA_EYE_RETINA_HEIGHT))
fi
DROSOPHILA_EXPECTED_SENSORY_DEFAULT=$((DROSOPHILA_BASE_SENSORY + DROSOPHILA_EYE_CHANNELS))
DROSOPHILA_EXPECTED_SENSORY="${DROSOPHILA_EXPECTED_SENSORY:-$DROSOPHILA_EXPECTED_SENSORY_DEFAULT}"
DROSOPHILA_MAX_SENSORY="${DROSOPHILA_MAX_SENSORY:-$DROSOPHILA_EXPECTED_SENSORY}"
for _v in DROSOPHILA_EXPECTED_SENSORY DROSOPHILA_MAX_SENSORY; do
  if ! [[ "${!_v}" =~ ^[0-9]+$ ]] || [ "${!_v}" -le 0 ]; then
    echo "Invalid ${_v}='${!_v}' (must be positive integer)."
    exit 1
  fi
done

for _v in HEXAPOD_CAMERA_RETINA_WIDTH HEXAPOD_CAMERA_RETINA_HEIGHT HEXAPOD_EXPECTED_OUTPUT HEXAPOD_HIDDEN_LAYERS HEXAPOD_HIDDEN_PER_LAYER HEXAPOD_AARNN_DEPTH; do
  if ! [[ "${!_v}" =~ ^[0-9]+$ ]] || [ "${!_v}" -le 0 ]; then
    echo "Invalid ${_v}='${!_v}' (must be positive integer)."
    exit 1
  fi
done
if ! python3 - "$HEXAPOD_GROWTH_HEADROOM" <<'PY'
import sys
try:
    v = float(sys.argv[1])
except Exception:
    raise SystemExit(1)
if v < 1.0:
    raise SystemExit(1)
raise SystemExit(0)
PY
then
  echo "Invalid HEXAPOD_GROWTH_HEADROOM='${HEXAPOD_GROWTH_HEADROOM}' (must be >= 1.0)."
  exit 1
fi

# Hexapod base channels:
#   18 joint position sensors
#   6 foot contact sensors
#   3 accelerometer channels
#   3 gyro channels
#   2 ultrasonic channels
HEXAPOD_BASE_NON_CAMERA_SENSORY=32
HEXAPOD_CAMERA_CHANNELS=$((2 * HEXAPOD_CAMERA_RETINA_WIDTH * HEXAPOD_CAMERA_RETINA_HEIGHT))
HEXAPOD_EXPECTED_SENSORY_DEFAULT=$((HEXAPOD_BASE_NON_CAMERA_SENSORY + HEXAPOD_CAMERA_CHANNELS))
HEXAPOD_EXPECTED_SENSORY="${HEXAPOD_EXPECTED_SENSORY:-$HEXAPOD_EXPECTED_SENSORY_DEFAULT}"
if ! [[ "$HEXAPOD_EXPECTED_SENSORY" =~ ^[0-9]+$ ]] || [ "$HEXAPOD_EXPECTED_SENSORY" -le 0 ]; then
  echo "Invalid HEXAPOD_EXPECTED_SENSORY='${HEXAPOD_EXPECTED_SENSORY}' (must be positive integer)."
  exit 1
fi

CELEGANS_TEMPLATE_FILE="${CELEGANS_TEMPLATE_FILE:-$ROOT_DIR/network.json}"
CELEGANS_CONNECTOME_FILE="${CELEGANS_CONNECTOME_FILE:-$ROOT_DIR/celegans.py}"
CELEGANS_REBUILD_NETWORK="${CELEGANS_REBUILD_NETWORK:-0}"
CELEGANS_REBUILD_ASSETS="${CELEGANS_REBUILD_ASSETS:-0}"
DROSOPHILA_REBUILD_ASSETS="${DROSOPHILA_REBUILD_ASSETS:-0}"

# Zebrafish network build controls
ZEBRAFISH_DATA_FILE="${ZEBRAFISH_DATA_FILE:-$ROOT_DIR/data/zebrafish/04152019.csv}"
ZEBRAFISH_NETWORK_FILE="${ZEBRAFISH_NETWORK_FILE:-$ROOT_DIR/network_zebrafish.json}"
ZEBRAFISH_CONFIG_FILE="${ZEBRAFISH_CONFIG_FILE:-$ROOT_DIR/webots_world/configs/config_zebrafish_webots.json}"
ZEBRAFISH_PROTO_FILE="${ZEBRAFISH_PROTO_FILE:-$ROOT_DIR/webots_world/protos/ZebrafishRobot.proto}"
ZEBRAFISH_TEMPLATE_FILE="${ZEBRAFISH_TEMPLATE_FILE:-$ROOT_DIR/network.json}"
ZEBRAFISH_NETWORK_SCRIPT="${ZEBRAFISH_NETWORK_SCRIPT:-$ROOT_DIR/scripts/build_zebrafish_network_json.py}"
ZEBRAFISH_ASSET_SCRIPT="${ZEBRAFISH_ASSET_SCRIPT:-$ROOT_DIR/scripts/build_webots_zebrafish_assets.py}"
ZEBRAFISH_MAX_HIDDEN="${ZEBRAFISH_MAX_HIDDEN:-2000}"
ZEBRAFISH_REBUILD_NETWORK="${ZEBRAFISH_REBUILD_NETWORK:-0}"
ZEBRAFISH_REBUILD_ASSETS="${ZEBRAFISH_REBUILD_ASSETS:-0}"

if ! [[ "$ZEBRAFISH_MAX_HIDDEN" =~ ^[0-9]+$ ]] || [ "$ZEBRAFISH_MAX_HIDDEN" -le 0 ]; then
  echo "Invalid ZEBRAFISH_MAX_HIDDEN='${ZEBRAFISH_MAX_HIDDEN}' (must be positive integer)."
  exit 1
fi
ZEBRAFISH_EXPECTED_SENSORY=32
ZEBRAFISH_EXPECTED_OUTPUT=32

NAO_TEMPLATE_FILE="${NAO_TEMPLATE_FILE:-$ROOT_DIR/network.json}"
NAO_NETWORK_SCRIPT="${NAO_NETWORK_SCRIPT:-$ROOT_DIR/scripts/build_nao_network_json.py}"
NAO_PROTO_FILE="${NAO_PROTO_FILE:-}"
NAO_CAMERA_RETINA_WIDTH="${NAO_CAMERA_RETINA_WIDTH:-8}"
NAO_CAMERA_RETINA_HEIGHT="${NAO_CAMERA_RETINA_HEIGHT:-6}"
NAO_EXPECTED_SENSORY="${NAO_EXPECTED_SENSORY:-$((58 + 4 * NAO_CAMERA_RETINA_WIDTH * NAO_CAMERA_RETINA_HEIGHT))}"
NAO_EXPECTED_OUTPUT="${NAO_EXPECTED_OUTPUT:-40}"
NAO_HIDDEN_LAYERS="${NAO_HIDDEN_LAYERS:-4}"
NAO_HIDDEN_PER_LAYER="${NAO_HIDDEN_PER_LAYER:-${NAO_HIDDEN_NEURONS:-64}}"
NAO_AARNN_DEPTH="${NAO_AARNN_DEPTH:-3}"
NAO_GROWTH_HEADROOM="${NAO_GROWTH_HEADROOM:-1.6}"
NAO_REBUILD_NETWORK="${NAO_REBUILD_NETWORK:-0}"

# Keep runtime encoder dimensions aligned with generated NAO network I/O dimensions.
NM_CAMERA_RETINA_WIDTH="${NM_CAMERA_RETINA_WIDTH:-$NAO_CAMERA_RETINA_WIDTH}"
NM_CAMERA_RETINA_HEIGHT="${NM_CAMERA_RETINA_HEIGHT:-$NAO_CAMERA_RETINA_HEIGHT}"
export NM_CAMERA_RETINA_WIDTH NM_CAMERA_RETINA_HEIGHT

PASS_THROUGH_ARGS=()

pass_through_has_arg() {
  local needle="$1"
  local arg
  for arg in "${PASS_THROUGH_ARGS[@]}"; do
    if [ "$arg" = "$needle" ] || [[ "$arg" == "$needle="* ]]; then
      return 0
    fi
  done
  return 1
}

webots_recording_requested() {
  case "${NM_WEBOTS_RECORD:-}" in
    1|true|TRUE|yes|YES|on|ON) return 0 ;;
  esac

  local arg
  for arg in "${PASS_THROUGH_ARGS[@]}"; do
    case "$arg" in
      --webots-record|--webots-record-file|--webots-record-file=*|\
      --webots-record-duration-ms|--webots-record-duration-ms=*|\
      --webots-record-width|--webots-record-width=*|\
      --webots-record-height|--webots-record-height=*|\
      --webots-record-quality|--webots-record-quality=*|\
      --webots-record-acceleration|--webots-record-acceleration=*|\
      --webots-record-quit-on-done|--webots-record-progress)
        return 0
        ;;
    esac
  done
  return 1
}

single_brain_config_io_defaults() {
  local config_file="$1"
  python3 - "$config_file" <<'PY'
import json
import sys

path = sys.argv[1]
try:
    with open(path, "r", encoding="utf-8") as fh:
        cfg = json.load(fh)
    sensory = int(cfg.get("num_sensory_neurons", 0))
    output = int(cfg.get("num_output_neurons", 0))
except Exception:
    raise SystemExit(0)

if sensory > 0 and output > 0:
    print(f"{sensory} {output}")
PY
}

usage() {
  cat <<'USAGE'
Usage: scripts/run_multi_robot_webots.sh [options] [run_webot passthrough args]

Options:
  --ui-mode <rust|web|cli>   Frontend mode (default: rust; recording forces cli).
  --robots <spec>            Robot count spec, e.g.
                             "drosophila_fafb=1,drosophila_banc=3,celegans=2,hexapod=1,nao=3"
  --celegans <n>             Override celegans count.
  --drosophila-banc <n>      Override BANC drosophila count.
  --drosophila-fafb <n>      Override FAFB drosophila count.
  --hexapod <n>              Override hexapod count.
  --nao <n>                  Override Nao count.
  --world <path>             Output mixed world path.
  --help                     Show this help.

Environment:
  UI_MODE, ROBOT_SPEC, REMOTE_COMPUTE, ORCHESTRATOR_PORT, WEB_UI_LISTEN,
  WEBOTS_RUNTIME_ROOT, WEBOTS_RUNTIME_USER, WEBOTS_WORKSPACE_PREFIX,
  WEBOTS_WORKSPACE_AUTOSAVE_STEPS, WEBOTS_WORKSPACE_RESUME_EXISTING,
  CELEGANS_* / DROSOPHILA_* / HEXAPOD_* / NAO_* path and build variables,
  plus run_webot.sh passthrough env (notably NM_UDS_RECV_TIMEOUT_MS,
  NM_IPC_TIMEOUT_GRACE_MS, NM_IPC_TIMEOUT_LOG_INTERVAL_MS,
  NM_IPC_UDS_CTRL_BUF_BYTES, NM_IPC_WINDOW_MIN/INIT/MAX, NM_IPC_SEND_BUDGET_MAX,
  NM_IPC_FORCE_AER, NM_IPC_AER_THRESHOLD, WEBOTS_CONNECT_TIMEOUT,
  WEBOTS_CLUSTER_DISTRIBUTION_TIMEOUT, NM_WEBOTS_RECORD, NM_WEBOTS_RECORD_FILE,
  NM_WEBOTS_RECORD_WIDTH, NM_WEBOTS_RECORD_HEIGHT, NM_WEBOTS_RECORD_DURATION_MS,
  NM_WEBOTS_RECORD_QUALITY, NM_WEBOTS_RECORD_ACCELERATION,
  NM_WEBOTS_RECORD_PROGRESS, NM_WEBOTS_RECORD_PROGRESS_INTERVAL_MS).

Recording:
  Pass Webots recording options through after the robot options, for example:
  scripts/run_multi_robot_webots.sh --robots celegans=1 \
    --webots-mode fast --webots-headless --webots-record \
    --webots-record-duration-ms 10000 --webots-record-progress
  - Recording mode runs AARNN headless/cli so rendering can focus on Webots.

Notes:
  - All robot instances are placed into a single Webots world.
  - Each instance gets a unique brain ID.
  - A single cluster runtime is launched with per-brain network/config mapping.
  - When repeated robot kinds are launched, default workspace resume is auto-disabled
    to keep all instances seeded from the same snapshot (override explicitly with
    WEBOTS_WORKSPACE_RESUME_EXISTING=1).
USAGE
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --ui-mode)
      shift
      UI_MODE="${1:-$UI_MODE}"
      UI_MODE_SET_BY_USER=1
      ;;
    --robots|--robot-counts)
      shift
      ROBOT_SPEC="${1:-$ROBOT_SPEC}"
      ;;
    --celegans)
      shift
      COUNT_CELEGANS_OVERRIDE="${1:-}"
      ;;
    --drosophila-banc)
      shift
      COUNT_DROSOPHILA_BANC_OVERRIDE="${1:-}"
      ;;
    --drosophila-fafb)
      shift
      COUNT_DROSOPHILA_FAFB_OVERRIDE="${1:-}"
      ;;
    --hexapod)
      shift
      COUNT_HEXAPOD_OVERRIDE="${1:-}"
      ;;
    --nao)
      shift
      COUNT_NAO_OVERRIDE="${1:-}"
      ;;
    --zebrafish)
      shift
      COUNT_ZEBRAFISH_OVERRIDE="${1:-}"
      ;;
    --world)
      shift
      WORLD_FILE="${1:-$WORLD_FILE}"
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    --)
      shift
      PASS_THROUGH_ARGS+=("$@")
      break
      ;;
    *)
      PASS_THROUGH_ARGS+=("$1")
      ;;
  esac
  shift
done

if [ "$UI_MODE" != "rust" ] && [ "$UI_MODE" != "web" ] && [ "$UI_MODE" != "cli" ]; then
  echo "Invalid --ui-mode '$UI_MODE' (must be rust, web, or cli)."
  exit 1
fi
if webots_recording_requested && [ "$UI_MODE" != "cli" ]; then
  if [ "$UI_MODE_SET_BY_USER" -eq 1 ]; then
    echo "Webots recording enabled; overriding --ui-mode $UI_MODE with cli so AARNN runs headless during capture."
  else
    echo "Webots recording enabled; using cli mode so AARNN runs headless during capture."
  fi
  UI_MODE="cli"
fi

# Preserve configured AARNN bio depth unless the caller explicitly opts into
# dynamic depth auto-scaling.
if [ -z "${NM_AUTO_AARNN_DEPTH+x}" ]; then
  export NM_AUTO_AARNN_DEPTH=0
fi

eval "$(
python3 - <<'PY' "$ROBOT_SPEC"
import re
import sys

spec = sys.argv[1]
counts = {"celegans": 0, "drosophila_banc": 0, "drosophila_fafb": 0, "hexapod": 0, "nao": 0, "zebrafish": 0}
aliases = {
    "celegans": "celegans",
    "worm": "celegans",
    "worms": "celegans",
    "c_elegans": "celegans",
    "drosophila": "drosophila_banc",
    "fly": "drosophila_banc",
    "flies": "drosophila_banc",
    "fruitfly": "drosophila_banc",
    "fruitflies": "drosophila_banc",
    "drosophila_banc": "drosophila_banc",
    "banc_drosophila": "drosophila_banc",
    "drosophila_banc_v626": "drosophila_banc",
    "banc": "drosophila_banc",
    "drosophila_fafb": "drosophila_fafb",
    "fafb_drosophila": "drosophila_fafb",
    "drosophila_fafb_v783": "drosophila_fafb",
    "fafb": "drosophila_fafb",
    "hexapod": "hexapod",
    "hex": "hexapod",
    "hexapods": "hexapod",
    "freenove_hexapod": "hexapod",
    "big_hexapod": "hexapod",
    "freenove": "hexapod",
    "six_legged": "hexapod",
    "nao": "nao",
    "naos": "nao",
    "zebrafish": "zebrafish",
    "zebrafishes": "zebrafish",
    "danio": "zebrafish",
    "danio_rerio": "zebrafish",
    "fish": "zebrafish",
    "zfish": "zebrafish",
    "zf": "zebrafish",
}

for token in re.split(r"[;,]", spec):
    token = token.strip()
    if not token:
        continue
    if "=" not in token:
        raise SystemExit(f"Invalid robot token '{token}' (expected key=value)")
    key_raw, value_raw = token.split("=", 1)
    key_norm = re.sub(r"[^a-z0-9]+", "_", key_raw.strip().lower()).strip("_")
    canonical = aliases.get(key_norm)
    if canonical is None:
        if "fafb" in key_norm and ("drosophila" in key_norm or "fly" in key_norm):
            canonical = "drosophila_fafb"
        elif "banc" in key_norm and ("drosophila" in key_norm or "fly" in key_norm):
            canonical = "drosophila_banc"
        elif "hexapod" in key_norm or "freenove" in key_norm:
            canonical = "hexapod"
        elif "zebra" in key_norm or "danio" in key_norm:
            canonical = "zebrafish"
        else:
            raise SystemExit(f"Unknown robot key '{key_raw}'")
    try:
        value = int(value_raw.strip())
    except Exception:
        raise SystemExit(f"Invalid count '{value_raw}' for key '{key_raw}'")
    if value < 0:
        raise SystemExit(f"Count must be >= 0 for key '{key_raw}'")
    counts[canonical] = value

for key, value in counts.items():
    print(f"COUNT_{key.upper()}={value}")
PY
)"

apply_override_count() {
  local var_name="$1"
  local override="$2"
  if [ -n "$override" ]; then
    if ! [[ "$override" =~ ^[0-9]+$ ]]; then
      echo "Invalid count override for $var_name: '$override' (must be >= 0 integer)."
      exit 1
    fi
    printf -v "$var_name" "%s" "$override"
  fi
}

apply_override_count COUNT_CELEGANS "$COUNT_CELEGANS_OVERRIDE"
apply_override_count COUNT_DROSOPHILA_BANC "$COUNT_DROSOPHILA_BANC_OVERRIDE"
apply_override_count COUNT_DROSOPHILA_FAFB "$COUNT_DROSOPHILA_FAFB_OVERRIDE"
apply_override_count COUNT_HEXAPOD "$COUNT_HEXAPOD_OVERRIDE"
apply_override_count COUNT_NAO "$COUNT_NAO_OVERRIDE"
apply_override_count COUNT_ZEBRAFISH "$COUNT_ZEBRAFISH_OVERRIDE"

TOTAL_ROBOTS=$((COUNT_CELEGANS + COUNT_DROSOPHILA_BANC + COUNT_DROSOPHILA_FAFB + COUNT_HEXAPOD + COUNT_NAO + COUNT_ZEBRAFISH))
if [ "$TOTAL_ROBOTS" -le 0 ]; then
  echo "Robot counts resolve to zero. Use --robots or count overrides to add robots."
  exit 1
fi

network_matches_selection() {
  local net_path="$1"
  local expect_dataset="$2"
  local want_sensory="$3"
  python3 - "$net_path" "$expect_dataset" "$want_sensory" "$DROSOPHILA_MAX_HIDDEN" "$DROSOPHILA_HIDDEN_LAYER_WIDTH" "$DROSOPHILA_LONG_RANGE_POLICY" <<'PY'
import json
import sys
from pathlib import Path

net_path = Path(sys.argv[1])
expect_dataset = sys.argv[2]
want_sensory = int(sys.argv[3])
want_hidden = int(sys.argv[4])
want_width = int(sys.argv[5])
want_policy = sys.argv[6].strip().lower()
try:
    data = json.loads(net_path.read_text(encoding="utf-8"))
except Exception:
    raise SystemExit(1)
labels = data.get("connectome_labels") or {}
if str(labels.get("dataset", "")).strip() != expect_dataset:
    raise SystemExit(1)
sel = labels.get("selection") or {}
if int(sel.get("max_sensory", -1)) != want_sensory:
    raise SystemExit(1)
if int(sel.get("max_hidden", -1)) != want_hidden:
    raise SystemExit(1)
if int(sel.get("hidden_layer_width", -1)) != want_width:
    raise SystemExit(1)
if str(sel.get("long_range_policy", "")).strip().lower() != want_policy:
    raise SystemExit(1)
net = data.get("net") or {}
if int(net.get("num_sensory_neurons", -1)) != want_sensory:
    raise SystemExit(1)
spike_io = net.get("spike_io") or {}
if str(spike_io.get("profile", "")).strip().lower() not in {"drosophila"}:
    raise SystemExit(1)
if (labels.get("topology_projection") or {}).get("mode") != "region_inferred_balanced":
    raise SystemExit(1)
if not isinstance(data.get("topo"), dict):
    raise SystemExit(1)
raise SystemExit(0)
PY
}

drosophila_config_matches_sensory() {
  local cfg_path="$1"
  python3 - "$cfg_path" "$DROSOPHILA_EXPECTED_SENSORY" <<'PY'
import json
import sys
from pathlib import Path

cfg_path = Path(sys.argv[1])
want_s = int(sys.argv[2])
try:
    data = json.loads(cfg_path.read_text(encoding="utf-8"))
except Exception:
    raise SystemExit(1)
if int((data.get("num_sensory_neurons", 0) or 0)) != want_s:
    raise SystemExit(1)
raise SystemExit(0)
PY
}

celegans_network_valid() {
  local net_path="$1"
  python3 - "$net_path" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
try:
    data = json.loads(path.read_text(encoding="utf-8"))
except Exception:
    raise SystemExit(1)

if not isinstance(data, dict):
    raise SystemExit(1)
net = data.get("net") or {}
if not isinstance(net, dict):
    raise SystemExit(1)

sensory = int(net.get("num_sensory_neurons", 0) or 0)
output = int(net.get("num_output_neurons", 0) or 0)
hidden = int(net.get("num_hidden_per_layer_initial", 0) or 0)
layers = int(net.get("num_hidden_layers", 0) or 0)
expected_sensory = 24
if sensory <= 0 or output <= 0:
    raise SystemExit(1)
if sensory != expected_sensory:
    raise SystemExit(1)
if output != 96:
    raise SystemExit(1)
if layers != 1 or hidden != 302:
    raise SystemExit(1)
if not bool(net.get("growth_enabled", False)):
    raise SystemExit(1)
if not bool(net.get("use_morphology", False)):
    raise SystemExit(1)
if int(net.get("aarnn_layer_depth", 0) or 0) < 1:
    raise SystemExit(1)

labels = data.get("connectome_labels") or {}
if not isinstance(labels, dict):
    raise SystemExit(1)
sensory_nodes = labels.get("sensory_nodes")
if not isinstance(sensory_nodes, list) or len(sensory_nodes) != sensory:
    raise SystemExit(1)
if len({str(v) for v in sensory_nodes}) != len(sensory_nodes):
    raise SystemExit(1)
hidden_nodes = labels.get("hidden_nodes")
if not isinstance(hidden_nodes, list) or len(hidden_nodes) != hidden:
    raise SystemExit(1)

projection = labels.get("sensory_projection") or {}
if not isinstance(projection, dict):
    raise SystemExit(1)
for node in sensory_nodes:
    targets = projection.get(node)
    if not isinstance(targets, list) or not targets:
        raise SystemExit(1)

w_in = data.get("w_in") or {}
rows = int(w_in.get("rows", 0) or 0)
cols = int(w_in.get("cols", 0) or 0)
flat = w_in.get("data")
if rows <= 0 or cols != sensory:
    raise SystemExit(1)
if not isinstance(flat, list) or len(flat) != rows * cols:
    raise SystemExit(1)

for c in range(cols):
    nonzero = False
    for r in range(rows):
        try:
            v = float(flat[r * cols + c])
        except Exception:
            raise SystemExit(1)
        if abs(v) > 1e-12:
            nonzero = True
            break
    if not nonzero:
        raise SystemExit(1)

raise SystemExit(0)
PY
}

celegans_config_matches_sensory() {
  local cfg_path="$1"
  python3 - "$cfg_path" <<'PY'
import json
import sys
from pathlib import Path

cfg_path = Path(sys.argv[1])
expected_s = 24
try:
    data = json.loads(cfg_path.read_text(encoding="utf-8"))
except Exception:
    raise SystemExit(1)
if int((data.get("num_sensory_neurons", 0) or 0)) != expected_s:
    raise SystemExit(1)
raise SystemExit(0)
PY
}

celegans_sensor_alignment_matches_proto() {
  local net_path="$1"
  local proto_path="$2"
  python3 - "$net_path" "$proto_path" <<'PY'
import json
import re
import sys
from pathlib import Path

net_path = Path(sys.argv[1])
proto_path = Path(sys.argv[2])
try:
    data = json.loads(net_path.read_text(encoding="utf-8"))
except Exception:
    raise SystemExit(1)
labels = data.get("connectome_labels") or {}
sensory_nodes = labels.get("sensory_nodes")
if not isinstance(sensory_nodes, list) or not sensory_nodes:
    raise SystemExit(1)

try:
    proto_text = proto_path.read_text(encoding="utf-8")
except Exception:
    raise SystemExit(1)

device_names = []
seen = set()
for name in re.findall(r'name\s+"(celegans_s_[0-9]{2}_[^"]+)"', proto_text):
    if name not in seen:
        seen.add(name)
        device_names.append(name)

if not device_names:
    raise SystemExit(1)

expanded = []
for name in device_names:
    if name.endswith("_vibration_accel") or name.endswith("_vibration_gyro"):
        expanded.extend((f"{name}.x", f"{name}.y", f"{name}.z"))
    else:
        expanded.append(name)

if expanded != sensory_nodes:
    raise SystemExit(1)

raise SystemExit(0)
PY
}

nao_network_valid() {
  local net_path="$1"
  local cfg_path="$2"
  python3 - "$net_path" "$cfg_path" "$NAO_EXPECTED_SENSORY" "$NAO_EXPECTED_OUTPUT" "$NAO_HIDDEN_LAYERS" "$NAO_AARNN_DEPTH" "$NAO_GROWTH_HEADROOM" <<'PY'
import json
import math
import sys
from pathlib import Path

net_path = Path(sys.argv[1])
cfg_path = Path(sys.argv[2])
expected_s = int(sys.argv[3])
expected_o = int(sys.argv[4])
expected_layers = int(sys.argv[5])
expected_depth = max(1, min(int(float(sys.argv[6])), 5))
expected_headroom = max(1.0, float(sys.argv[7]))

try:
    snap = json.loads(net_path.read_text(encoding="utf-8"))
    cfg = json.loads(cfg_path.read_text(encoding="utf-8"))
except Exception:
    raise SystemExit(1)

if not isinstance(snap, dict) or not isinstance(cfg, dict):
    raise SystemExit(1)

net = snap.get("net") or {}
if not isinstance(net, dict):
    raise SystemExit(1)

if int(net.get("num_sensory_neurons", -1)) != expected_s:
    raise SystemExit(1)
if int(net.get("num_output_neurons", -1)) != expected_o:
    raise SystemExit(1)
layers = int(net.get("num_hidden_layers", 0) or 0)
hidden = int(net.get("num_hidden_per_layer_initial", 0) or 0)
if hidden <= 0:
    raise SystemExit(1)
if layers != expected_layers:
    raise SystemExit(1)
if int(net.get("aarnn_layer_depth", 0) or 0) != expected_depth:
    raise SystemExit(1)
if not bool(net.get("growth_enabled", False)):
    raise SystemExit(1)
if not bool(net.get("use_morphology", False)):
    raise SystemExit(1)
if not bool(net.get("sleep_enabled", False)):
    raise SystemExit(1)
if not bool(net.get("aarnn_import_topology_rewire_enabled", False)):
    raise SystemExit(1)
if float(net.get("aarnn_import_topology_rewire_keep_fraction", 1.0) or 1.0) >= 1.0:
    raise SystemExit(1)

if int(cfg.get("num_sensory_neurons", -1)) != expected_s:
    raise SystemExit(1)
if int(cfg.get("num_output_neurons", -1)) != expected_o:
    raise SystemExit(1)
if int(cfg.get("num_hidden_layers", 0) or 0) != expected_layers:
    raise SystemExit(1)
if int(cfg.get("num_hidden_per_layer_initial", 0) or 0) <= 0:
    raise SystemExit(1)
if int(cfg.get("aarnn_layer_depth", 0) or 0) != expected_depth:
    raise SystemExit(1)
if not bool(cfg.get("growth_enabled", False)):
    raise SystemExit(1)
if not bool(cfg.get("use_morphology", False)):
    raise SystemExit(1)
if not bool(cfg.get("sleep_enabled", False)):
    raise SystemExit(1)
if not bool(cfg.get("aarnn_import_topology_rewire_enabled", False)):
    raise SystemExit(1)
if float(cfg.get("aarnn_import_topology_rewire_keep_fraction", 1.0) or 1.0) >= 1.0:
    raise SystemExit(1)

w_in = snap.get("w_in") or {}
w_fwd = snap.get("w_hh_fwd") or []
w_bwd = snap.get("w_hh_bwd") or []
w_rec = snap.get("w_hh_rec") or []
w_out = snap.get("w_out") or {}
if not isinstance(w_fwd, list) or len(w_fwd) != expected_layers - 1:
    raise SystemExit(1)
if not isinstance(w_bwd, list) or len(w_bwd) != expected_layers - 1:
    raise SystemExit(1)
if not isinstance(w_rec, list) or len(w_rec) != expected_layers:
    raise SystemExit(1)

if int(w_in.get("cols", 0) or 0) != expected_s:
    raise SystemExit(1)
first_h = int(w_in.get("rows", 0) or 0)
if first_h <= 0:
    raise SystemExit(1)
if int(w_out.get("rows", 0) or 0) != expected_o:
    raise SystemExit(1)

layer_sizes = []
for idx, rec in enumerate(w_rec):
    rec = rec or {}
    rows = int(rec.get("rows", 0) or 0)
    cols = int(rec.get("cols", 0) or 0)
    if rows <= 0 or cols <= 0 or rows != cols:
        raise SystemExit(1)
    if idx == 0 and rows != first_h:
        raise SystemExit(1)
    layer_sizes.append(rows)

for i, mat in enumerate(w_fwd):
    mat = mat or {}
    rows = int(mat.get("rows", 0) or 0)
    cols = int(mat.get("cols", 0) or 0)
    if rows != layer_sizes[i + 1] or cols != layer_sizes[i]:
        raise SystemExit(1)

for i, mat in enumerate(w_bwd):
    mat = mat or {}
    rows = int(mat.get("rows", 0) or 0)
    cols = int(mat.get("cols", 0) or 0)
    if rows != layer_sizes[i] or cols != layer_sizes[i + 1]:
        raise SystemExit(1)

if int(w_out.get("cols", 0) or 0) != layer_sizes[-1]:
    raise SystemExit(1)

initial_neuron_count = expected_s + expected_o + sum(layer_sizes)
required_budget = int(math.ceil(initial_neuron_count * expected_headroom))
if int(net.get("max_total_neurons", 0) or 0) < required_budget:
    raise SystemExit(1)
if int(cfg.get("max_total_neurons", 0) or 0) < required_budget:
    raise SystemExit(1)

labels = snap.get("connectome_labels") or {}
s_nodes = labels.get("sensory_nodes")
o_nodes = labels.get("output_nodes")
if not isinstance(s_nodes, list) or len(s_nodes) != expected_s:
    raise SystemExit(1)
if not isinstance(o_nodes, list) or len(o_nodes) != expected_o:
    raise SystemExit(1)
hidden_sizes = labels.get("hidden_layer_sizes")
if not isinstance(hidden_sizes, list) or len(hidden_sizes) != expected_layers:
    raise SystemExit(1)
if any(int(v) <= 0 for v in hidden_sizes):
    raise SystemExit(1)
if int(hidden_sizes[0]) != first_h:
    raise SystemExit(1)

raise SystemExit(0)
PY
}

hexapod_network_valid() {
  local net_path="$1"
  local cfg_path="$2"
  local proto_path="$3"
  python3 - "$net_path" "$cfg_path" "$proto_path" "$HEXAPOD_EXPECTED_SENSORY" "$HEXAPOD_EXPECTED_OUTPUT" "$HEXAPOD_HIDDEN_LAYERS" "$HEXAPOD_AARNN_DEPTH" "$HEXAPOD_CAMERA_RETINA_WIDTH" "$HEXAPOD_CAMERA_RETINA_HEIGHT" <<'PY'
import json
import re
import sys
from pathlib import Path

net_path = Path(sys.argv[1])
cfg_path = Path(sys.argv[2])
proto_path = Path(sys.argv[3])
expected_s = int(sys.argv[4])
expected_o = int(sys.argv[5])
expected_layers = int(sys.argv[6])
expected_depth = max(1, min(int(sys.argv[7]), 5))
retina_w = int(sys.argv[8])
retina_h = int(sys.argv[9])

NAME_RE = re.compile(r'\bname\s+"([^"]+)"')
TYPE_RE = re.compile(r'\btype\s+"([^"]+)"')
CAM_RE = re.compile(r"^(?P<base>.+)\.(?P<polarity>on|off)\.r(?P<row>\d+)c(?P<col>\d+)$")

def iter_node_blocks(text: str, node_type: str):
    pat = re.compile(rf"\b{re.escape(node_type)}\b\s*\{{")
    for m in pat.finditer(text):
        start = m.end() - 1
        depth = 0
        i = start
        n = len(text)
        while i < n:
            ch = text[i]
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    yield text[start : i + 1]
                    break
            i += 1

def first_name(block: str):
    m = NAME_RE.search(block)
    if not m:
        return None
    name = m.group(1).strip()
    return name or None

def index_digits(n: int) -> int:
    max_index = max(0, n - 1)
    digits = 1
    while max_index >= 10:
        max_index //= 10
        digits += 1
    return max(2, digits)

def camera_channels(name: str):
    row_digits = index_digits(retina_h)
    col_digits = index_digits(retina_w)
    out = []
    for r in range(retina_h):
        for c in range(retina_w):
            out.append(f"{name}.on.r{r:0{row_digits}d}c{c:0{col_digits}d}")
            out.append(f"{name}.off.r{r:0{row_digits}d}c{c:0{col_digits}d}")
    return out

def dedupe_keep_order(values):
    seen = set()
    out = []
    for value in values:
        if value in seen:
            continue
        seen.add(value)
        out.append(value)
    return out

def sort_sensor_channels(channels):
    axis_order = {"x": 0, "y": 1, "z": 2, "mean": 3, "center": 4, "mean_gray": 3, "center_gray": 4}
    def key(channel: str):
        cam = CAM_RE.match(channel)
        if cam:
            base = cam.group("base")
            row = int(cam.group("row"))
            col = int(cam.group("col"))
            polarity = 0 if cam.group("polarity") == "on" else 1
            return (base, 0, 0, row, col, polarity, channel)
        if "." in channel:
            base, axis = channel.rsplit(".", 1)
            if axis in axis_order:
                return (base, 0, 1, axis_order[axis], 0, 0, channel)
        return (channel, 1, 0, 0, 0, 0, channel)
    return sorted(dedupe_keep_order(channels), key=key)

def parse_proto_channels(path: Path):
    text = path.read_text(encoding="utf-8")
    sensor_devices = []
    output_devices = []

    for block in iter_node_blocks(text, "Accelerometer"):
        name = first_name(block) or "accelerometer"
        sensor_devices.append((name, [f"{name}.x", f"{name}.y", f"{name}.z"]))
    for block in iter_node_blocks(text, "Camera"):
        name = first_name(block)
        if name:
            sensor_devices.append((name, camera_channels(name)))
    for block in iter_node_blocks(text, "Gyro"):
        name = first_name(block) or "gyro"
        sensor_devices.append((name, [f"{name}.x", f"{name}.y", f"{name}.z"]))
    for node_type in ("DistanceSensor", "LightSensor", "PositionSensor"):
        for block in iter_node_blocks(text, node_type):
            name = first_name(block)
            if name:
                sensor_devices.append((name, [name]))
    for block in iter_node_blocks(text, "TouchSensor"):
        name = first_name(block)
        if not name:
            continue
        t_match = TYPE_RE.search(block)
        t = (t_match.group(1).strip().lower() if t_match else "")
        if "force-3d" in t or "force3d" in t:
            sensor_devices.append((name, [f"{name}.x", f"{name}.y", f"{name}.z"]))
        else:
            sensor_devices.append((name, [name]))

    for node_type in ("RotationalMotor", "LinearMotor"):
        for block in iter_node_blocks(text, node_type):
            name = first_name(block)
            if name:
                output_devices.append(name)

    sensory = []
    for _, channels in sorted(sensor_devices, key=lambda it: it[0]):
        sensory.extend(channels)
    sensory = sort_sensor_channels(sensory)
    outputs = sorted(dedupe_keep_order(output_devices))
    return sensory, outputs

try:
    snap = json.loads(net_path.read_text(encoding="utf-8"))
    cfg = json.loads(cfg_path.read_text(encoding="utf-8"))
except Exception:
    raise SystemExit(1)

if not isinstance(snap, dict) or not isinstance(cfg, dict):
    raise SystemExit(1)

net = snap.get("net") or {}
if not isinstance(net, dict):
    raise SystemExit(1)

if int(net.get("num_sensory_neurons", -1)) != expected_s:
    raise SystemExit(1)
if int(net.get("num_output_neurons", -1)) != expected_o:
    raise SystemExit(1)
if int(net.get("num_hidden_layers", 0) or 0) != expected_layers:
    raise SystemExit(1)
if int(net.get("aarnn_layer_depth", 0) or 0) != expected_depth:
    raise SystemExit(1)
if str((net.get("clumping_design") or "")).strip().lower() != "hexapod":
    raise SystemExit(1)
spike_io = net.get("spike_io") or {}
if str((spike_io.get("profile") or "")).strip().lower() != "hexapod":
    raise SystemExit(1)
if not bool(net.get("growth_enabled", False)):
    raise SystemExit(1)
if not bool(net.get("use_morphology", False)):
    raise SystemExit(1)

if int(cfg.get("num_sensory_neurons", -1)) != expected_s:
    raise SystemExit(1)
if int(cfg.get("num_output_neurons", -1)) != expected_o:
    raise SystemExit(1)
if int(cfg.get("num_hidden_layers", 0) or 0) != expected_layers:
    raise SystemExit(1)
if int(cfg.get("aarnn_layer_depth", 0) or 0) != expected_depth:
    raise SystemExit(1)
cfg_spike = cfg.get("spike_io") or {}
if str((cfg_spike.get("profile") or "")).strip().lower() != "hexapod":
    raise SystemExit(1)

w_in = snap.get("w_in") or {}
w_out = snap.get("w_out") or {}
w_fwd = snap.get("w_hh_fwd") or []
w_bwd = snap.get("w_hh_bwd") or []
w_rec = snap.get("w_hh_rec") or []
if int(w_in.get("cols", 0) or 0) != expected_s:
    raise SystemExit(1)
if int(w_out.get("rows", 0) or 0) != expected_o:
    raise SystemExit(1)
if not isinstance(w_fwd, list) or len(w_fwd) != expected_layers - 1:
    raise SystemExit(1)
if not isinstance(w_bwd, list) or len(w_bwd) != expected_layers - 1:
    raise SystemExit(1)
if not isinstance(w_rec, list) or len(w_rec) != expected_layers:
    raise SystemExit(1)

labels = snap.get("connectome_labels") or {}
s_nodes = labels.get("sensory_nodes")
o_nodes = labels.get("output_nodes")
if not isinstance(s_nodes, list) or len(s_nodes) != expected_s:
    raise SystemExit(1)
if not isinstance(o_nodes, list) or len(o_nodes) != expected_o:
    raise SystemExit(1)

proto_s, proto_o = parse_proto_channels(proto_path)
if len(proto_s) != expected_s:
    raise SystemExit(1)
if len(proto_o) < expected_o:
    raise SystemExit(1)
if s_nodes != proto_s:
    raise SystemExit(1)
if o_nodes != proto_o[:expected_o]:
    raise SystemExit(1)

io_map_path = cfg_path.with_name(f"{cfg_path.stem}.io_alignment.json")
if not io_map_path.exists():
    raise SystemExit(1)
try:
    io_map = json.loads(io_map_path.read_text(encoding="utf-8"))
except Exception:
    raise SystemExit(1)
if int(len(io_map.get("sensory_channels") or [])) != expected_s:
    raise SystemExit(1)
if int(len(io_map.get("output_channels") or [])) != expected_o:
    raise SystemExit(1)

raise SystemExit(0)
PY
}

if [ "$COUNT_CELEGANS" -gt 0 ]; then
  CELEGANS_NETWORK_SCRIPT="$ROOT_DIR/scripts/build_celegans_network_json.py"
  CELEGANS_ASSET_SCRIPT="$ROOT_DIR/scripts/build_webots_celegans_assets.py"
  CELEGANS_NETWORK_STALE=0
  if [ -f "$CELEGANS_NETWORK_SCRIPT" ] && { [ ! -f "$CELEGANS_NETWORK_FILE" ] || [ "$CELEGANS_NETWORK_SCRIPT" -nt "$CELEGANS_NETWORK_FILE" ]; }; then
    CELEGANS_NETWORK_STALE=1
  fi
  if [ -f "$CELEGANS_CONNECTOME_FILE" ] && { [ ! -f "$CELEGANS_NETWORK_FILE" ] || [ "$CELEGANS_CONNECTOME_FILE" -nt "$CELEGANS_NETWORK_FILE" ]; }; then
    CELEGANS_NETWORK_STALE=1
  fi
  if [ -f "$CELEGANS_TEMPLATE_FILE" ] && { [ ! -f "$CELEGANS_NETWORK_FILE" ] || [ "$CELEGANS_TEMPLATE_FILE" -nt "$CELEGANS_NETWORK_FILE" ]; }; then
    CELEGANS_NETWORK_STALE=1
  fi
  if [ -f "$CELEGANS_NETWORK_FILE" ] && ! celegans_network_valid "$CELEGANS_NETWORK_FILE"; then
    CELEGANS_NETWORK_STALE=1
  fi
  CELEGANS_ASSET_STALE=0
  if [ -f "$CELEGANS_ASSET_SCRIPT" ] && { [ ! -f "$CELEGANS_PROTO_FILE" ] || [ "$CELEGANS_ASSET_SCRIPT" -nt "$CELEGANS_PROTO_FILE" ]; }; then
    CELEGANS_ASSET_STALE=1
  fi
  if [ -f "$CELEGANS_ASSET_SCRIPT" ] && { [ ! -f "$CELEGANS_CONFIG_FILE" ] || [ "$CELEGANS_ASSET_SCRIPT" -nt "$CELEGANS_CONFIG_FILE" ]; }; then
    CELEGANS_ASSET_STALE=1
  fi
  if [ -f "$CELEGANS_NETWORK_FILE" ] && { [ ! -f "$CELEGANS_PROTO_FILE" ] || [ "$CELEGANS_NETWORK_FILE" -nt "$CELEGANS_PROTO_FILE" ]; }; then
    CELEGANS_ASSET_STALE=1
  fi
  if [ -f "$CELEGANS_NETWORK_FILE" ] && { [ ! -f "$CELEGANS_CONFIG_FILE" ] || [ "$CELEGANS_NETWORK_FILE" -nt "$CELEGANS_CONFIG_FILE" ]; }; then
    CELEGANS_ASSET_STALE=1
  fi
  if [ -f "$CELEGANS_CONFIG_FILE" ] && ! celegans_config_matches_sensory "$CELEGANS_CONFIG_FILE"; then
    CELEGANS_ASSET_STALE=1
  fi
  if [ -f "$CELEGANS_NETWORK_FILE" ] && [ -f "$CELEGANS_PROTO_FILE" ] && ! celegans_sensor_alignment_matches_proto "$CELEGANS_NETWORK_FILE" "$CELEGANS_PROTO_FILE"; then
    CELEGANS_ASSET_STALE=1
  fi

  if [ "$CELEGANS_REBUILD_NETWORK" = "1" ] || [ "$CELEGANS_NETWORK_STALE" = "1" ] || [ ! -f "$CELEGANS_NETWORK_FILE" ]; then
    python3 "$ROOT_DIR/scripts/build_celegans_network_json.py" \
      --connectome "$CELEGANS_CONNECTOME_FILE" \
      --template "$CELEGANS_TEMPLATE_FILE" \
      --output "$CELEGANS_NETWORK_FILE"
  fi

  if [ "$CELEGANS_REBUILD_ASSETS" = "1" ] || [ "$CELEGANS_ASSET_STALE" = "1" ] || [ ! -f "$CELEGANS_PROTO_FILE" ] || [ ! -f "$CELEGANS_CONFIG_FILE" ]; then
    python3 "$ROOT_DIR/scripts/build_webots_celegans_assets.py" \
      --network "$CELEGANS_NETWORK_FILE" \
      --proto "$CELEGANS_PROTO_FILE" \
      --config "$CELEGANS_CONFIG_FILE" \
      --world "$TMP_CELEGANS_WORLD"
  fi
fi

if [ "$COUNT_DROSOPHILA_BANC" -gt 0 ] || [ "$COUNT_DROSOPHILA_FAFB" -gt 0 ]; then
  DROSOPHILA_ASSET_SCRIPT="$ROOT_DIR/scripts/build_webots_drosophila_assets.py"
  export NM_DROS_CAMERA_RETINA_WIDTH="$DROSOPHILA_EYE_RETINA_WIDTH"
  export NM_DROS_CAMERA_RETINA_HEIGHT="$DROSOPHILA_EYE_RETINA_HEIGHT"
  DROSOPHILA_ASSET_STALE=0
  if [ -f "$DROSOPHILA_ASSET_SCRIPT" ] && { [ ! -f "$DROSOPHILA_BANC_PROTO_FILE" ] || [ "$DROSOPHILA_ASSET_SCRIPT" -nt "$DROSOPHILA_BANC_PROTO_FILE" ]; }; then
    DROSOPHILA_ASSET_STALE=1
  fi
  if [ -f "$DROSOPHILA_ASSET_SCRIPT" ] && { [ ! -f "$DROSOPHILA_FAFB_PROTO_FILE" ] || [ "$DROSOPHILA_ASSET_SCRIPT" -nt "$DROSOPHILA_FAFB_PROTO_FILE" ]; }; then
    DROSOPHILA_ASSET_STALE=1
  fi
  if [ -f "$DROSOPHILA_ASSET_SCRIPT" ] && { [ ! -f "$DROSOPHILA_BANC_CONFIG_FILE" ] || [ "$DROSOPHILA_ASSET_SCRIPT" -nt "$DROSOPHILA_BANC_CONFIG_FILE" ]; }; then
    DROSOPHILA_ASSET_STALE=1
  fi
  if [ -f "$DROSOPHILA_ASSET_SCRIPT" ] && { [ ! -f "$DROSOPHILA_FAFB_CONFIG_FILE" ] || [ "$DROSOPHILA_ASSET_SCRIPT" -nt "$DROSOPHILA_FAFB_CONFIG_FILE" ]; }; then
    DROSOPHILA_ASSET_STALE=1
  fi
  if [ -f "$DROSOPHILA_BANC_CONFIG_FILE" ] && ! drosophila_config_matches_sensory "$DROSOPHILA_BANC_CONFIG_FILE"; then
    DROSOPHILA_ASSET_STALE=1
  fi
  if [ -f "$DROSOPHILA_FAFB_CONFIG_FILE" ] && ! drosophila_config_matches_sensory "$DROSOPHILA_FAFB_CONFIG_FILE"; then
    DROSOPHILA_ASSET_STALE=1
  fi

  NEED_DROSO_REBUILD="$DROSOPHILA_REBUILD_NETWORK"
  if [ "$NEED_DROSO_REBUILD" != "1" ]; then
    if [ ! -f "$DROSOPHILA_BANC_NETWORK_FILE" ] || [ ! -f "$DROSOPHILA_FAFB_NETWORK_FILE" ]; then
      NEED_DROSO_REBUILD=1
    elif ! network_matches_selection "$DROSOPHILA_BANC_NETWORK_FILE" "BANC v626" "$DROSOPHILA_MAX_SENSORY"; then
      NEED_DROSO_REBUILD=1
    elif ! network_matches_selection "$DROSOPHILA_FAFB_NETWORK_FILE" "FAFB v783" "$DROSOPHILA_MAX_SENSORY"; then
      NEED_DROSO_REBUILD=1
    fi
  fi

  if [ "$NEED_DROSO_REBUILD" = "1" ]; then
    python3 "$ROOT_DIR/scripts/build_drosophila_network_json.py" \
      --dual \
      --banc-dir "$DROSOPHILA_BANC_DIR" \
      --fafb-dir "$DROSOPHILA_FAFB_DIR" \
      --template "$DROSOPHILA_TEMPLATE_FILE" \
      --output-banc "$DROSOPHILA_BANC_NETWORK_FILE" \
      --output-fafb "$DROSOPHILA_FAFB_NETWORK_FILE" \
      --max-sensory "$DROSOPHILA_MAX_SENSORY" \
      --max-hidden "$DROSOPHILA_MAX_HIDDEN" \
      --max-output "$DROSOPHILA_MAX_OUTPUT" \
      --min-syn-count "$DROSOPHILA_MIN_SYN_COUNT" \
      --weight-transform "$DROSOPHILA_WEIGHT_TRANSFORM" \
      --hidden-layer-width "$DROSOPHILA_HIDDEN_LAYER_WIDTH" \
      --long-range-policy "$DROSOPHILA_LONG_RANGE_POLICY"
  fi

  if [ "$DROSOPHILA_REBUILD_ASSETS" = "1" ] \
    || [ "$DROSOPHILA_ASSET_STALE" = "1" ] \
    || [ ! -f "$DROSOPHILA_BANC_PROTO_FILE" ] \
    || [ ! -f "$DROSOPHILA_FAFB_PROTO_FILE" ] \
    || [ ! -f "$DROSOPHILA_BANC_CONFIG_FILE" ] \
    || [ ! -f "$DROSOPHILA_FAFB_CONFIG_FILE" ]; then
    python3 "$ROOT_DIR/scripts/build_webots_drosophila_assets.py" \
      --network-a "$DROSOPHILA_BANC_NETWORK_FILE" \
      --network-b "$DROSOPHILA_FAFB_NETWORK_FILE" \
      --proto-a "$DROSOPHILA_BANC_PROTO_FILE" \
      --proto-b "$DROSOPHILA_FAFB_PROTO_FILE" \
      --config-a "$DROSOPHILA_BANC_CONFIG_FILE" \
      --config-b "$DROSOPHILA_FAFB_CONFIG_FILE" \
      --world "$TMP_DROSOPHILA_WORLD" \
      --compound-eyes "$DROSOPHILA_EYE_CAMERAS" \
      --eye-retina-width "$DROSOPHILA_EYE_RETINA_WIDTH" \
      --eye-retina-height "$DROSOPHILA_EYE_RETINA_HEIGHT" \
      --eye-camera-width "$DROSOPHILA_EYE_CAMERA_WIDTH" \
      --eye-camera-height "$DROSOPHILA_EYE_CAMERA_HEIGHT" \
      --brain-a banc \
      --brain-b fafb
  fi
fi

if [ "$COUNT_NAO" -gt 0 ]; then
  if [ ! -f "$NAO_NETWORK_SCRIPT" ]; then
    echo "Missing NAO network builder script: $NAO_NETWORK_SCRIPT"
    exit 1
  fi
  if [ ! -f "$NAO_TEMPLATE_FILE" ]; then
    echo "Missing NAO template snapshot: $NAO_TEMPLATE_FILE"
    exit 1
  fi

  NAO_NETWORK_STALE=0
  if [ ! -f "$NAO_NETWORK_FILE" ] || [ ! -f "$NAO_CONFIG_FILE" ]; then
    NAO_NETWORK_STALE=1
  fi
  if [ "$NAO_NETWORK_SCRIPT" -nt "$NAO_NETWORK_FILE" ] || [ "$NAO_NETWORK_SCRIPT" -nt "$NAO_CONFIG_FILE" ]; then
    NAO_NETWORK_STALE=1
  fi
  if [ "$NAO_TEMPLATE_FILE" -nt "$NAO_NETWORK_FILE" ] || [ "$NAO_TEMPLATE_FILE" -nt "$NAO_CONFIG_FILE" ]; then
    NAO_NETWORK_STALE=1
  fi
  if [ -n "$NAO_PROTO_FILE" ] && [ -f "$NAO_PROTO_FILE" ] && [ "$NAO_PROTO_FILE" -nt "$NAO_NETWORK_FILE" ]; then
    NAO_NETWORK_STALE=1
  fi
  if [ -f "$NAO_NETWORK_FILE" ] && [ -f "$NAO_CONFIG_FILE" ] && ! nao_network_valid "$NAO_NETWORK_FILE" "$NAO_CONFIG_FILE"; then
    NAO_NETWORK_STALE=1
  fi

  if [ "$NAO_REBUILD_NETWORK" = "1" ] || [ "$NAO_NETWORK_STALE" = "1" ]; then
    nao_build_cmd=(
      python3 "$NAO_NETWORK_SCRIPT"
      --template "$NAO_TEMPLATE_FILE"
      --output "$NAO_NETWORK_FILE"
      --config-output "$NAO_CONFIG_FILE"
      --expected-sensory "$NAO_EXPECTED_SENSORY"
      --expected-output "$NAO_EXPECTED_OUTPUT"
      --camera-retina-width "$NAO_CAMERA_RETINA_WIDTH"
      --camera-retina-height "$NAO_CAMERA_RETINA_HEIGHT"
      --hidden-layers "$NAO_HIDDEN_LAYERS"
      --hidden-per-layer "$NAO_HIDDEN_PER_LAYER"
      --aarnn-depth "$NAO_AARNN_DEPTH"
      --growth-headroom "$NAO_GROWTH_HEADROOM"
    )
    if [ -n "$NAO_PROTO_FILE" ]; then
      nao_build_cmd+=(--nao-proto "$NAO_PROTO_FILE")
    fi
    "${nao_build_cmd[@]}"
  fi

  if [ ! -f "$NAO_NETWORK_FILE" ]; then
    echo "Missing NAO network snapshot: $NAO_NETWORK_FILE"
    exit 1
  fi
  if [ ! -f "$NAO_CONFIG_FILE" ]; then
    echo "Missing NAO config file: $NAO_CONFIG_FILE"
    exit 1
  fi

  # Keep NAO camera event transport on compact AER packets with a larger
  # receive buffer for resilient non-blocking IPC (especially when retina
  # resolution is increased via overrides).
  NM_IPC_FORCE_AER="${NM_IPC_FORCE_AER:-1}"
  NM_IPC_MAX_RAW_BYTES="${NM_IPC_MAX_RAW_BYTES:-60000}"
  NM_IPC_AER_MAX_PACKET_BYTES="${NM_IPC_AER_MAX_PACKET_BYTES:-60000}"
  NM_IPC_UDS_RECV_BUF_BYTES="${NM_IPC_UDS_RECV_BUF_BYTES:-262144}"
  export NM_IPC_FORCE_AER NM_IPC_MAX_RAW_BYTES NM_IPC_AER_MAX_PACKET_BYTES NM_IPC_UDS_RECV_BUF_BYTES
fi

if [ "$COUNT_HEXAPOD" -gt 0 ]; then
  if [ ! -f "$HEXAPOD_PROTO_FILE" ]; then
    echo "Missing hexapod proto file: $HEXAPOD_PROTO_FILE"
    exit 1
  fi
  if [ ! -f "$HEXAPOD_NETWORK_SCRIPT" ]; then
    echo "Missing hexapod network builder script: $HEXAPOD_NETWORK_SCRIPT"
    exit 1
  fi
  if [ ! -f "$HEXAPOD_TEMPLATE_FILE" ]; then
    echo "Missing hexapod template snapshot: $HEXAPOD_TEMPLATE_FILE"
    exit 1
  fi

  # Keep runtime camera-event encoder dimensions aligned with generated
  # hexapod sensory channels for the front camera device.
  NM_CAMERA_RETINA_WIDTH_HEX_S_26_HEAD_CAMERA="${NM_CAMERA_RETINA_WIDTH_HEX_S_26_HEAD_CAMERA:-$HEXAPOD_CAMERA_RETINA_WIDTH}"
  NM_CAMERA_RETINA_HEIGHT_HEX_S_26_HEAD_CAMERA="${NM_CAMERA_RETINA_HEIGHT_HEX_S_26_HEAD_CAMERA:-$HEXAPOD_CAMERA_RETINA_HEIGHT}"
  export NM_CAMERA_RETINA_WIDTH_HEX_S_26_HEAD_CAMERA NM_CAMERA_RETINA_HEIGHT_HEX_S_26_HEAD_CAMERA

  HEXAPOD_NETWORK_STALE=0
  if [ ! -f "$HEXAPOD_NETWORK_FILE" ] || [ ! -f "$HEXAPOD_CONFIG_FILE" ]; then
    HEXAPOD_NETWORK_STALE=1
  fi
  if [ "$HEXAPOD_NETWORK_SCRIPT" -nt "$HEXAPOD_NETWORK_FILE" ] || [ "$HEXAPOD_NETWORK_SCRIPT" -nt "$HEXAPOD_CONFIG_FILE" ]; then
    HEXAPOD_NETWORK_STALE=1
  fi
  if [ "$HEXAPOD_TEMPLATE_FILE" -nt "$HEXAPOD_NETWORK_FILE" ] || [ "$HEXAPOD_TEMPLATE_FILE" -nt "$HEXAPOD_CONFIG_FILE" ]; then
    HEXAPOD_NETWORK_STALE=1
  fi
  if [ "$HEXAPOD_PROTO_FILE" -nt "$HEXAPOD_NETWORK_FILE" ] || [ "$HEXAPOD_PROTO_FILE" -nt "$HEXAPOD_CONFIG_FILE" ]; then
    HEXAPOD_NETWORK_STALE=1
  fi
  if [ -f "$HEXAPOD_NETWORK_FILE" ] && [ -f "$HEXAPOD_CONFIG_FILE" ] && ! hexapod_network_valid "$HEXAPOD_NETWORK_FILE" "$HEXAPOD_CONFIG_FILE" "$HEXAPOD_PROTO_FILE"; then
    HEXAPOD_NETWORK_STALE=1
  fi

  if [ "$HEXAPOD_REBUILD_NETWORK" = "1" ] || [ "$HEXAPOD_NETWORK_STALE" = "1" ]; then
    python3 "$HEXAPOD_NETWORK_SCRIPT" \
      --template "$HEXAPOD_TEMPLATE_FILE" \
      --output "$HEXAPOD_NETWORK_FILE" \
      --config-output "$HEXAPOD_CONFIG_FILE" \
      --hexapod-proto "$HEXAPOD_PROTO_FILE" \
      --expected-sensory "$HEXAPOD_EXPECTED_SENSORY" \
      --expected-output "$HEXAPOD_EXPECTED_OUTPUT" \
      --camera-retina-width "$HEXAPOD_CAMERA_RETINA_WIDTH" \
      --camera-retina-height "$HEXAPOD_CAMERA_RETINA_HEIGHT" \
      --hidden-layers "$HEXAPOD_HIDDEN_LAYERS" \
      --hidden-per-layer "$HEXAPOD_HIDDEN_PER_LAYER" \
      --aarnn-depth "$HEXAPOD_AARNN_DEPTH" \
      --growth-headroom "$HEXAPOD_GROWTH_HEADROOM"
  fi

  if [ ! -f "$HEXAPOD_NETWORK_FILE" ]; then
    echo "Missing hexapod network snapshot: $HEXAPOD_NETWORK_FILE"
    exit 1
  fi
  if [ ! -f "$HEXAPOD_CONFIG_FILE" ]; then
    echo "Missing hexapod config file: $HEXAPOD_CONFIG_FILE"
    exit 1
  fi
  if ! hexapod_network_valid "$HEXAPOD_NETWORK_FILE" "$HEXAPOD_CONFIG_FILE" "$HEXAPOD_PROTO_FILE"; then
    echo "Hexapod network/config validation failed for: $HEXAPOD_NETWORK_FILE / $HEXAPOD_CONFIG_FILE"
    exit 1
  fi

  # Keep hexapod sensory ingress on explicit event transport so sparse sensor
  # channels are visible in IPC diagnostics and spike raster views.
  NM_IPC_FORCE_AER="${NM_IPC_FORCE_AER:-1}"
  NM_IPC_MAX_RAW_BYTES="${NM_IPC_MAX_RAW_BYTES:-60000}"
  NM_IPC_AER_MAX_PACKET_BYTES="${NM_IPC_AER_MAX_PACKET_BYTES:-60000}"
  NM_IPC_AER_THRESHOLD="${NM_IPC_AER_THRESHOLD:-0.12}"
  NM_IPC_UDS_RECV_BUF_BYTES="${NM_IPC_UDS_RECV_BUF_BYTES:-262144}"
  export NM_IPC_FORCE_AER NM_IPC_MAX_RAW_BYTES NM_IPC_AER_MAX_PACKET_BYTES NM_IPC_AER_THRESHOLD NM_IPC_UDS_RECV_BUF_BYTES
fi

if [ "$COUNT_ZEBRAFISH" -gt 0 ]; then
  if [ ! -f "$ZEBRAFISH_DATA_FILE" ]; then
    echo "Missing zebrafish connectome data: $ZEBRAFISH_DATA_FILE"
    echo "  Download from https://seunglab.org/zebrafish/data/ and place at $ZEBRAFISH_DATA_FILE"
    exit 1
  fi
  if [ ! -f "$ZEBRAFISH_NETWORK_SCRIPT" ]; then
    echo "Missing zebrafish network builder: $ZEBRAFISH_NETWORK_SCRIPT"
    exit 1
  fi
  if [ ! -f "$ZEBRAFISH_ASSET_SCRIPT" ]; then
    echo "Missing zebrafish asset builder: $ZEBRAFISH_ASSET_SCRIPT"
    exit 1
  fi
  if [ ! -f "$ZEBRAFISH_TEMPLATE_FILE" ]; then
    echo "Missing network template: $ZEBRAFISH_TEMPLATE_FILE"
    exit 1
  fi

  ZEBRAFISH_NETWORK_STALE=0
  if [ ! -f "$ZEBRAFISH_NETWORK_FILE" ] || [ ! -f "$ZEBRAFISH_CONFIG_FILE" ]; then
    ZEBRAFISH_NETWORK_STALE=1
  fi
  if [ -f "$ZEBRAFISH_NETWORK_SCRIPT" ] && [ "$ZEBRAFISH_NETWORK_SCRIPT" -nt "$ZEBRAFISH_NETWORK_FILE" ]; then
    ZEBRAFISH_NETWORK_STALE=1
  fi
  if [ "$ZEBRAFISH_TEMPLATE_FILE" -nt "$ZEBRAFISH_NETWORK_FILE" ]; then
    ZEBRAFISH_NETWORK_STALE=1
  fi
  # Validate existing network has correct channel counts
  if [ -f "$ZEBRAFISH_NETWORK_FILE" ]; then
    if ! python3 - "$ZEBRAFISH_NETWORK_FILE" "$ZEBRAFISH_EXPECTED_SENSORY" "$ZEBRAFISH_EXPECTED_OUTPUT" <<'PY'
import json, sys
from pathlib import Path
try:
    data = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
except Exception:
    raise SystemExit(1)
net = data.get("net") or {}
labels = data.get("connectome_labels") or {}
bio = labels.get("bio_profile") or {}
if str(bio.get("species","")).strip() != "danio_rerio":
    raise SystemExit(1)
if str(net.get("clumping_design","")).strip() != "ZebraFish":
    raise SystemExit(1)
if int(net.get("num_sensory_neurons", -1)) != int(sys.argv[2]):
    raise SystemExit(1)
if int(net.get("num_output_neurons", -1)) != int(sys.argv[3]):
    raise SystemExit(1)
PY
    then
      ZEBRAFISH_NETWORK_STALE=1
    fi
  fi

  if [ "$ZEBRAFISH_REBUILD_NETWORK" = "1" ] || [ "$ZEBRAFISH_NETWORK_STALE" = "1" ]; then
    echo "Building zebrafish network (max_hidden=$ZEBRAFISH_MAX_HIDDEN) — this may take several minutes …"
    python3 "$ZEBRAFISH_NETWORK_SCRIPT" \
      --data      "$ZEBRAFISH_DATA_FILE" \
      --template  "$ZEBRAFISH_TEMPLATE_FILE" \
      --output    "$ZEBRAFISH_NETWORK_FILE" \
      --max-hidden "$ZEBRAFISH_MAX_HIDDEN"
  fi

  if [ ! -f "$ZEBRAFISH_NETWORK_FILE" ]; then
    echo "Missing zebrafish network snapshot: $ZEBRAFISH_NETWORK_FILE"
    exit 1
  fi

  ZEBRAFISH_ASSET_STALE=0
  if [ ! -f "$ZEBRAFISH_PROTO_FILE" ] || [ ! -f "$ZEBRAFISH_CONFIG_FILE" ]; then
    ZEBRAFISH_ASSET_STALE=1
  fi
  if [ "$ZEBRAFISH_ASSET_SCRIPT" -nt "$ZEBRAFISH_PROTO_FILE" ]; then
    ZEBRAFISH_ASSET_STALE=1
  fi
  if [ "$ZEBRAFISH_NETWORK_FILE" -nt "$ZEBRAFISH_CONFIG_FILE" ]; then
    ZEBRAFISH_ASSET_STALE=1
  fi

  if [ "$ZEBRAFISH_REBUILD_ASSETS" = "1" ] || [ "$ZEBRAFISH_ASSET_STALE" = "1" ]; then
    python3 "$ZEBRAFISH_ASSET_SCRIPT" \
      --network          "$ZEBRAFISH_NETWORK_FILE" \
      --proto-output     "$ZEBRAFISH_PROTO_FILE" \
      --world-output     /tmp/aarnn_tmp_zebrafish_assets_ignore.wbt \
      --config-output    "$ZEBRAFISH_CONFIG_FILE"
  fi

  if [ ! -f "$ZEBRAFISH_PROTO_FILE" ]; then
    echo "Missing zebrafish proto file: $ZEBRAFISH_PROTO_FILE"
    exit 1
  fi
  if [ ! -f "$ZEBRAFISH_CONFIG_FILE" ]; then
    echo "Missing zebrafish config file: $ZEBRAFISH_CONFIG_FILE"
    exit 1
  fi

  # Heal any stale JSON files that contain the wrong clumping_design casing
  # (workspace manifests, snapshots, and the generated config file).
  python3 - "$WEBOTS_RUNTIME_ROOT" "$ZEBRAFISH_CONFIG_FILE" <<'PY'
import sys
from pathlib import Path
targets = []
rt = Path(sys.argv[1])
targets.extend(rt.rglob("workspaces/webots-zebrafish-*/*.json"))
targets.append(Path(sys.argv[2]))
for f in targets:
    if not f.exists():
        continue
    text = f.read_text(encoding="utf-8")
    changed = False
    if '"clumping_design": "Zebrafish"' in text:
        text = text.replace('"clumping_design": "Zebrafish"', '"clumping_design": "ZebraFish"')
        changed = True
    # Fix stale c_elegans profile left by early workspace creation (before the
    # zebrafish spike_io profile existed).  "zebrafish" is now a valid variant.
    if '"profile": "c_elegans"' in text and "zebrafish" in str(f):
        text = text.replace('"profile": "c_elegans"', '"profile": "zebrafish"', 1)
        changed = True
    if changed:
        f.write_text(text, encoding="utf-8")
        print(f"Healed: {f}")
PY

  # Per-camera retina override: zebrafish eyes are 1×1-pixel cameras processed
  # by the DeviceMapper camera event encoder (2 channels per camera = 4 total,
  # mapping to sensory channels 16–17 (left eye ON/OFF) and 18–19 (right eye)).
  NM_CAMERA_RETINA_WIDTH_ZEBRAFISH_EYE_LEFT="${NM_CAMERA_RETINA_WIDTH_ZEBRAFISH_EYE_LEFT:-1}"
  NM_CAMERA_RETINA_HEIGHT_ZEBRAFISH_EYE_LEFT="${NM_CAMERA_RETINA_HEIGHT_ZEBRAFISH_EYE_LEFT:-1}"
  NM_CAMERA_RETINA_WIDTH_ZEBRAFISH_EYE_RIGHT="${NM_CAMERA_RETINA_WIDTH_ZEBRAFISH_EYE_RIGHT:-1}"
  NM_CAMERA_RETINA_HEIGHT_ZEBRAFISH_EYE_RIGHT="${NM_CAMERA_RETINA_HEIGHT_ZEBRAFISH_EYE_RIGHT:-1}"
  export NM_CAMERA_RETINA_WIDTH_ZEBRAFISH_EYE_LEFT NM_CAMERA_RETINA_HEIGHT_ZEBRAFISH_EYE_LEFT
  export NM_CAMERA_RETINA_WIDTH_ZEBRAFISH_EYE_RIGHT NM_CAMERA_RETINA_HEIGHT_ZEBRAFISH_EYE_RIGHT

  # AER transport recommended for the larger zebrafish network
  NM_IPC_FORCE_AER="${NM_IPC_FORCE_AER:-1}"
  NM_IPC_MAX_RAW_BYTES="${NM_IPC_MAX_RAW_BYTES:-131072}"
  NM_IPC_AER_MAX_PACKET_BYTES="${NM_IPC_AER_MAX_PACKET_BYTES:-131072}"
  NM_IPC_AER_THRESHOLD="${NM_IPC_AER_THRESHOLD:-0.10}"
  NM_IPC_UDS_RECV_BUF_BYTES="${NM_IPC_UDS_RECV_BUF_BYTES:-524288}"
  export NM_IPC_FORCE_AER NM_IPC_MAX_RAW_BYTES NM_IPC_AER_MAX_PACKET_BYTES NM_IPC_AER_THRESHOLD NM_IPC_UDS_RECV_BUF_BYTES
fi

# Webots controller IPC pacing/logging defaults:
# keep the UDS bridge non-blocking but less bursty so occasional reply jitter
# does not show as repeated timeout chatter.
NM_UDS_RECV_TIMEOUT_MS="${NM_UDS_RECV_TIMEOUT_MS:-150}"
NM_IPC_TIMEOUT_GRACE_MS="${NM_IPC_TIMEOUT_GRACE_MS:-1500}"
NM_IPC_TIMEOUT_LOG_INTERVAL_MS="${NM_IPC_TIMEOUT_LOG_INTERVAL_MS:-5000}"
NM_IPC_UDS_CTRL_BUF_BYTES="${NM_IPC_UDS_CTRL_BUF_BYTES:-524288}"
NM_IPC_WINDOW_MIN="${NM_IPC_WINDOW_MIN:-1}"
NM_IPC_WINDOW_INIT="${NM_IPC_WINDOW_INIT:-1}"
NM_IPC_WINDOW_MAX="${NM_IPC_WINDOW_MAX:-1}"
NM_IPC_SEND_BUDGET_MAX="${NM_IPC_SEND_BUDGET_MAX:-1}"
NM_IPC_STRICT_LOCKSTEP="${NM_IPC_STRICT_LOCKSTEP:-1}"
# Slow Webots progression intentionally so AARNN compute has maximum wall-time headroom.
# Set to 0 to disable.
NM_WEBOTS_STEP_SLEEP_MS="${NM_WEBOTS_STEP_SLEEP_MS:-0}"
export NM_UDS_RECV_TIMEOUT_MS \
  NM_IPC_TIMEOUT_GRACE_MS \
  NM_IPC_TIMEOUT_LOG_INTERVAL_MS \
  NM_IPC_UDS_CTRL_BUF_BYTES \
  NM_IPC_WINDOW_MIN \
  NM_IPC_WINDOW_INIT \
  NM_IPC_WINDOW_MAX \
  NM_IPC_SEND_BUDGET_MAX \
  NM_IPC_STRICT_LOCKSTEP \
  NM_WEBOTS_STEP_SLEEP_MS

declare -a BRAINS=()
declare -a CELEGANS_BRAINS=()
declare -a DROS_BANC_BRAINS=()
declare -a DROS_FAFB_BRAINS=()
declare -a HEXAPOD_BRAINS=()
declare -a NAO_BRAINS=()
declare -a ZEBRAFISH_BRAINS=()
declare -a NETWORK_MAP_ENTRIES=()
declare -a CONFIG_MAP_ENTRIES=()
declare -A BRAIN_NETWORK_FILES=()
declare -A BRAIN_CONFIG_FILES=()

PRIMARY_NETWORK_FILE=""
PRIMARY_CONFIG_FILE=""

add_brain_mapping() {
  local brain="$1"
  local network_file="$2"
  local config_file="$3"
  BRAINS+=("$brain")
  BRAIN_NETWORK_FILES["$brain"]="$network_file"
  BRAIN_CONFIG_FILES["$brain"]="$config_file"
  NETWORK_MAP_ENTRIES+=("$brain=$network_file")
  CONFIG_MAP_ENTRIES+=("$brain=$config_file")
  if [ -z "$PRIMARY_NETWORK_FILE" ]; then
    PRIMARY_NETWORK_FILE="$network_file"
    PRIMARY_CONFIG_FILE="$config_file"
  fi
}

rebuild_network_map_from_workspace_bindings() {
  local bindings_json="$1"
  while IFS=$'\t' read -r brain snapshot_path; do
    [ -n "$brain" ] || continue
    [ -n "$snapshot_path" ] || continue
    BRAIN_NETWORK_FILES["$brain"]="$snapshot_path"
  done < <(
    python3 - "$bindings_json" <<'PY'
import json
import sys

bindings = json.loads(sys.argv[1])
for brain in sorted(bindings):
    latest = str(bindings[brain].get("latest_snapshot_path") or "").strip()
    if latest:
        print(f"{brain}\t{latest}")
PY
  )

  NETWORK_MAP_ENTRIES=()
  PRIMARY_NETWORK_FILE=""
  local brain
  for brain in "${BRAINS[@]}"; do
    local network_file="${BRAIN_NETWORK_FILES[$brain]}"
    NETWORK_MAP_ENTRIES+=("$brain=$network_file")
    if [ -z "$PRIMARY_NETWORK_FILE" ]; then
      PRIMARY_NETWORK_FILE="$network_file"
    fi
  done
  NETWORK_MAP_CSV="$(IFS=','; echo "${NETWORK_MAP_ENTRIES[*]}")"
}

prepare_runtime_workspaces() {
  local helper="$ROOT_DIR/scripts/prepare_runtime_workspaces.py"
  if [ ! -f "$helper" ]; then
    echo "Missing runtime workspace helper: $helper"
    exit 1
  fi
  if ! [[ "$WEBOTS_WORKSPACE_AUTOSAVE_STEPS" =~ ^[0-9]+$ ]] || [ "$WEBOTS_WORKSPACE_AUTOSAVE_STEPS" -le 0 ]; then
    echo "Invalid WEBOTS_WORKSPACE_AUTOSAVE_STEPS='$WEBOTS_WORKSPACE_AUTOSAVE_STEPS' (must be a positive integer)."
    exit 1
  fi
  if ! [[ "$WEBOTS_WORKSPACE_RESUME_EXISTING" =~ ^[0-9]+$ ]]; then
    echo "Invalid WEBOTS_WORKSPACE_RESUME_EXISTING='$WEBOTS_WORKSPACE_RESUME_EXISTING' (use 0 or 1)."
    exit 1
  fi

  local -a triples=("$WEBOTS_WORKSPACE_PREFIX")
  local brain
  for brain in "${BRAINS[@]}"; do
    triples+=("$brain" "${BRAIN_NETWORK_FILES[$brain]}" "${BRAIN_CONFIG_FILES[$brain]}")
  done

  local specs_json
  specs_json="$(python3 - "${triples[@]}" <<'PY'
import json
import sys

prefix = sys.argv[1].strip() or "webots"
args = sys.argv[2:]
if len(args) % 3 != 0:
    raise SystemExit(2)

specs = []
for i in range(0, len(args), 3):
    brain_id, snapshot_path, config_path = args[i:i + 3]
    workspace_id = f"{prefix}-{brain_id.replace('_', '-')}"
    display_name = brain_id.replace("_", " ").upper()
    specs.append(
        {
            "brain_id": brain_id,
            "workspace_id": workspace_id,
            "name": display_name,
            "snapshot_path": snapshot_path,
            "config_path": config_path,
            "neuron_model": "aarnn",
            "learning_rule": "aarnn",
        }
    )

print(json.dumps(specs, separators=(",", ":")))
PY
)"

  local resume_existing_effective="$WEBOTS_WORKSPACE_RESUME_EXISTING"
  if [ "$WEBOTS_WORKSPACE_RESUME_EXISTING_SET_BY_USER" -eq 0 ] && [ "$resume_existing_effective" = "1" ]; then
    local repeated_kinds
    repeated_kinds="$(
      python3 - "${BRAINS[@]}" <<'PY'
import sys
from collections import defaultdict
brains = [b.strip() for b in sys.argv[1:] if b.strip()]

groups = defaultdict(list)
for brain in brains:
    kind = brain.split("_", 1)[0]
    groups[kind].append(brain)

repeated = []
for kind, members in groups.items():
    if len(members) >= 2:
        repeated.append(f"{kind}x{len(members)}")

print(",".join(sorted(repeated)))
PY
    )"
    if [ -n "$repeated_kinds" ]; then
      echo "Workspace resume auto-disabled for repeated robot kinds: $repeated_kinds"
      echo "  using WEBOTS_WORKSPACE_RESUME_EXISTING=0 so all instances start from the same seed snapshot."
      echo "  set WEBOTS_WORKSPACE_RESUME_EXISTING=1 explicitly if you want per-instance historical state."
      resume_existing_effective=0
    fi
  fi

  local bindings_json
  bindings_json="$(python3 "$helper" \
    --root "$WEBOTS_RUNTIME_ROOT" \
    --user "$WEBOTS_RUNTIME_USER" \
    --autosave-steps "$WEBOTS_WORKSPACE_AUTOSAVE_STEPS" \
    --resume-existing "$resume_existing_effective" \
    --spec-json "$specs_json")" || {
      echo "Failed to prepare runtime workspaces."
      exit 1
    }
  if [ -z "$bindings_json" ]; then
    echo "Runtime workspace helper returned an empty bindings payload."
    exit 1
  fi

  export NM_RUNTIME_WORKSPACE_BINDINGS="$bindings_json"
  export NM_WEB_UI_RUNTIME_ROOT="$WEBOTS_RUNTIME_ROOT"
  export NM_WEB_UI_DEFAULT_RUNTIME_USER="$WEBOTS_RUNTIME_USER"
  WEBOTS_WORKSPACE_RESUME_EFFECTIVE="$resume_existing_effective"
  rebuild_network_map_from_workspace_bindings "$bindings_json"
}

print_workspace_seed_summary() {
  local bindings="${NM_RUNTIME_WORKSPACE_BINDINGS:-}"
  if [ -z "$bindings" ]; then
    return
  fi
  python3 - "$bindings" <<'PY'
import json
import sys

try:
    data = json.loads(sys.argv[1])
except Exception:
    raise SystemExit(0)

if not isinstance(data, dict) or not data:
    raise SystemExit(0)

print("  workspace seeds:")
for brain in sorted(data):
    entry = data.get(brain)
    seed = str((entry or {}).get("seed_source", "")).strip() or "unknown"
    print(f"    {brain}: {seed}")
PY
}

for i in $(seq 1 "$COUNT_CELEGANS"); do
  brain_id="$(printf "celegans_%02d" "$i")"
  CELEGANS_BRAINS+=("$brain_id")
  add_brain_mapping "$brain_id" "$CELEGANS_NETWORK_FILE" "$CELEGANS_CONFIG_FILE"
done

for i in $(seq 1 "$COUNT_DROSOPHILA_BANC"); do
  brain_id="$(printf "banc_%02d" "$i")"
  DROS_BANC_BRAINS+=("$brain_id")
  add_brain_mapping "$brain_id" "$DROSOPHILA_BANC_NETWORK_FILE" "$DROSOPHILA_BANC_CONFIG_FILE"
done

for i in $(seq 1 "$COUNT_DROSOPHILA_FAFB"); do
  brain_id="$(printf "fafb_%02d" "$i")"
  DROS_FAFB_BRAINS+=("$brain_id")
  add_brain_mapping "$brain_id" "$DROSOPHILA_FAFB_NETWORK_FILE" "$DROSOPHILA_FAFB_CONFIG_FILE"
done

for i in $(seq 1 "$COUNT_HEXAPOD"); do
  brain_id="$(printf "hexapod_%02d" "$i")"
  HEXAPOD_BRAINS+=("$brain_id")
  add_brain_mapping "$brain_id" "$HEXAPOD_NETWORK_FILE" "$HEXAPOD_CONFIG_FILE"
done

for i in $(seq 1 "$COUNT_NAO"); do
  brain_id="$(printf "nao_%02d" "$i")"
  NAO_BRAINS+=("$brain_id")
  add_brain_mapping "$brain_id" "$NAO_NETWORK_FILE" "$NAO_CONFIG_FILE"
done

for i in $(seq 1 "$COUNT_ZEBRAFISH"); do
  brain_id="$(printf "zebrafish_%02d" "$i")"
  ZEBRAFISH_BRAINS+=("$brain_id")
  add_brain_mapping "$brain_id" "$ZEBRAFISH_NETWORK_FILE" "$ZEBRAFISH_CONFIG_FILE"
done

BRAINS_CSV="$(IFS=','; echo "${BRAINS[*]}")"
NETWORK_MAP_CSV="$(IFS=','; echo "${NETWORK_MAP_ENTRIES[*]}")"
CONFIG_MAP_CSV="$(IFS=','; echo "${CONFIG_MAP_ENTRIES[*]}")"
CELEGANS_BRAINS_CSV="$(IFS=','; echo "${CELEGANS_BRAINS[*]}")"
DROS_BANC_BRAINS_CSV="$(IFS=','; echo "${DROS_BANC_BRAINS[*]}")"
DROS_FAFB_BRAINS_CSV="$(IFS=','; echo "${DROS_FAFB_BRAINS[*]}")"
HEXAPOD_BRAINS_CSV="$(IFS=','; echo "${HEXAPOD_BRAINS[*]}")"
NAO_BRAINS_CSV="$(IFS=','; echo "${NAO_BRAINS[*]}")"
ZEBRAFISH_BRAINS_CSV="$(IFS=','; echo "${ZEBRAFISH_BRAINS[*]}")"

prepare_runtime_workspaces

python3 "$ROOT_DIR/scripts/build_webots_multi_world.py" \
  --world "$WORLD_FILE" \
  --celegans-proto "$CELEGANS_PROTO_FILE" \
  --drosophila-banc-proto "$DROSOPHILA_BANC_PROTO_FILE" \
  --drosophila-fafb-proto "$DROSOPHILA_FAFB_PROTO_FILE" \
  --hexapod-proto "$HEXAPOD_PROTO_FILE" \
  --zebrafish-proto "$ZEBRAFISH_PROTO_FILE" \
  --celegans-brains "$CELEGANS_BRAINS_CSV" \
  --drosophila-banc-brains "$DROS_BANC_BRAINS_CSV" \
  --drosophila-fafb-brains "$DROS_FAFB_BRAINS_CSV" \
  --hexapod-brains "$HEXAPOD_BRAINS_CSV" \
  --nao-brains "$NAO_BRAINS_CSV" \
  --zebrafish-brains "$ZEBRAFISH_BRAINS_CSV"

echo "Multi-robot launch composition:"
echo "  celegans: $COUNT_CELEGANS"
echo "  drosophila_banc: $COUNT_DROSOPHILA_BANC"
echo "  drosophila_fafb: $COUNT_DROSOPHILA_FAFB"
echo "  hexapod: $COUNT_HEXAPOD"
echo "  nao: $COUNT_NAO"
echo "  zebrafish: $COUNT_ZEBRAFISH"
echo "  total robots/brains: $TOTAL_ROBOTS"
echo "  world: $WORLD_FILE"
echo "  brains: $BRAINS_CSV"
echo "  runtime root: $WEBOTS_RUNTIME_ROOT"
echo "  runtime user: $WEBOTS_RUNTIME_USER"
echo "  workspace prefix: $WEBOTS_WORKSPACE_PREFIX"
echo "  workspace resume existing (requested/effective): $WEBOTS_WORKSPACE_RESUME_EXISTING/$WEBOTS_WORKSPACE_RESUME_EFFECTIVE"
echo "  webots extra step sleep (ms): $NM_WEBOTS_STEP_SLEEP_MS"
echo "  IPC strict lockstep/window/send: $NM_IPC_STRICT_LOCKSTEP $NM_IPC_WINDOW_MIN/$NM_IPC_WINDOW_INIT/$NM_IPC_WINDOW_MAX budget=$NM_IPC_SEND_BUDGET_MAX"
print_workspace_seed_summary

DEFAULT_SO_ARGS=()
if [ "$TOTAL_ROBOTS" -eq 1 ] && [ -n "$PRIMARY_CONFIG_FILE" ] && [ -f "$PRIMARY_CONFIG_FILE" ]; then
  config_defaults="$(single_brain_config_io_defaults "$PRIMARY_CONFIG_FILE" || true)"
  if [ -n "$config_defaults" ]; then
    config_default_s="${config_defaults%% *}"
    config_default_o="${config_defaults##* }"

    if [ -z "${NM_DEFAULT_SENSORY+x}" ] && ! pass_through_has_arg "--sensory"; then
      DEFAULT_SO_ARGS+=(--sensory "$config_default_s")
    fi
    if [ -z "${NM_DEFAULT_OUTPUT+x}" ] && ! pass_through_has_arg "--output"; then
      DEFAULT_SO_ARGS+=(--output "$config_default_o")
    fi

    if [ "${#DEFAULT_SO_ARGS[@]}" -gt 0 ]; then
      echo "  pre-handshake fallback S/O (from config): $config_default_s/$config_default_o"
    fi
  fi
fi

MAX_NETWORK_BYTES=0
for brain in "${BRAINS[@]}"; do
  network_path="${BRAIN_NETWORK_FILES[$brain]:-}"
  if [ -z "$network_path" ] || [ ! -f "$network_path" ]; then
    continue
  fi
  network_bytes="$(stat -c%s "$network_path" 2>/dev/null || echo 0)"
  if [[ "$network_bytes" =~ ^[0-9]+$ ]] && [ "$network_bytes" -gt "$MAX_NETWORK_BYTES" ]; then
    MAX_NETWORK_BYTES="$network_bytes"
  fi
done

MAX_NETWORK_MB=0
if [ "$MAX_NETWORK_BYTES" -gt 0 ]; then
  MAX_NETWORK_MB=$(( (MAX_NETWORK_BYTES + 1048575) / 1048576 ))
fi

AUTO_CONNECT_TIMEOUT=""
AUTO_CONNECT_REASON=""
if [ -z "${WEBOTS_CONNECT_TIMEOUT+x}" ] && ! pass_through_has_arg "--connect-timeout"; then
  if [ "$MAX_NETWORK_BYTES" -ge $((512 * 1024 * 1024)) ]; then
    AUTO_CONNECT_TIMEOUT=300
  elif [ "$MAX_NETWORK_BYTES" -ge $((256 * 1024 * 1024)) ]; then
    AUTO_CONNECT_TIMEOUT=240
  elif [ "$MAX_NETWORK_BYTES" -ge $((128 * 1024 * 1024)) ]; then
    AUTO_CONNECT_TIMEOUT=180
  elif [ "$MAX_NETWORK_BYTES" -ge $((32 * 1024 * 1024)) ]; then
    AUTO_CONNECT_TIMEOUT=120
  else
    AUTO_CONNECT_TIMEOUT=60
  fi

  if [ "$MAX_NETWORK_MB" -gt 0 ]; then
    AUTO_CONNECT_REASON="max snapshot ${MAX_NETWORK_MB}MB"
  fi
fi

AUTO_CLUSTER_DISTRIBUTION_TIMEOUT=""
AUTO_CLUSTER_DISTRIBUTION_REASON=""
if [ -z "${WEBOTS_CLUSTER_DISTRIBUTION_TIMEOUT+x}" ] && ! pass_through_has_arg "--cluster-distribution-timeout"; then
  if [ "$MAX_NETWORK_BYTES" -ge $((512 * 1024 * 1024)) ]; then
    AUTO_CLUSTER_DISTRIBUTION_TIMEOUT=1800
  elif [ "$MAX_NETWORK_BYTES" -ge $((256 * 1024 * 1024)) ]; then
    AUTO_CLUSTER_DISTRIBUTION_TIMEOUT=1200
  elif [ "$MAX_NETWORK_BYTES" -ge $((128 * 1024 * 1024)) ]; then
    AUTO_CLUSTER_DISTRIBUTION_TIMEOUT=900
  elif [ "$MAX_NETWORK_BYTES" -ge $((32 * 1024 * 1024)) ]; then
    AUTO_CLUSTER_DISTRIBUTION_TIMEOUT=600
  else
    AUTO_CLUSTER_DISTRIBUTION_TIMEOUT=300
  fi

  if [ "$MAX_NETWORK_MB" -gt 0 ]; then
    AUTO_CLUSTER_DISTRIBUTION_REASON="max snapshot ${MAX_NETWORK_MB}MB"
  fi
fi

AUTO_IPC_PROFILE=""
if [ "$MAX_NETWORK_BYTES" -ge $((128 * 1024 * 1024)) ]; then
  tuned=0

  # Heavy snapshots are usually coupled with expensive compute backends.
  # Keep controller-side transport stable by reducing in-flight burst pressure
  # and giving transient backend lag more grace before warning.
  if [ "$NM_UDS_RECV_TIMEOUT_MS" = "150" ]; then
    NM_UDS_RECV_TIMEOUT_MS=250
    tuned=1
  fi
  if [ "$NM_IPC_TIMEOUT_GRACE_MS" = "1500" ]; then
    NM_IPC_TIMEOUT_GRACE_MS=5000
    tuned=1
  fi
  if [ "$NM_IPC_TIMEOUT_LOG_INTERVAL_MS" = "5000" ]; then
    NM_IPC_TIMEOUT_LOG_INTERVAL_MS=10000
    tuned=1
  fi
  if [ "$NM_IPC_WINDOW_MAX" = "8" ]; then
    NM_IPC_WINDOW_MAX=4
    tuned=1
  fi
  if [ "$NM_IPC_SEND_BUDGET_MAX" = "4" ]; then
    NM_IPC_SEND_BUDGET_MAX=2
    tuned=1
  fi
  if [ -z "${NM_IPC_FORCE_AER+x}" ]; then
    NM_IPC_FORCE_AER=1
    tuned=1
  fi
  if [ -z "${NM_IPC_MAX_RAW_BYTES+x}" ]; then
    NM_IPC_MAX_RAW_BYTES=60000
    tuned=1
  fi
  if [ -z "${NM_IPC_AER_MAX_PACKET_BYTES+x}" ]; then
    NM_IPC_AER_MAX_PACKET_BYTES=60000
    tuned=1
  fi
  if [ -z "${NM_IPC_AER_THRESHOLD+x}" ]; then
    NM_IPC_AER_THRESHOLD=0.20
    tuned=1
  fi

  if [ "$tuned" -eq 1 ]; then
    export NM_UDS_RECV_TIMEOUT_MS \
      NM_IPC_TIMEOUT_GRACE_MS \
      NM_IPC_TIMEOUT_LOG_INTERVAL_MS \
      NM_IPC_WINDOW_MAX \
      NM_IPC_SEND_BUDGET_MAX \
      NM_IPC_FORCE_AER \
      NM_IPC_MAX_RAW_BYTES \
      NM_IPC_AER_MAX_PACKET_BYTES \
      NM_IPC_AER_THRESHOLD
    AUTO_IPC_PROFILE="heavy snapshot defaults"
  fi
fi

if [ -n "$AUTO_CONNECT_TIMEOUT" ]; then
  if [ -n "$AUTO_CONNECT_REASON" ]; then
    echo "  auto connect timeout (s): $AUTO_CONNECT_TIMEOUT ($AUTO_CONNECT_REASON)"
  else
    echo "  auto connect timeout (s): $AUTO_CONNECT_TIMEOUT"
  fi
fi

if [ -n "$AUTO_CLUSTER_DISTRIBUTION_TIMEOUT" ]; then
  if [ -n "$AUTO_CLUSTER_DISTRIBUTION_REASON" ]; then
    echo "  auto cluster distribution timeout (s): $AUTO_CLUSTER_DISTRIBUTION_TIMEOUT ($AUTO_CLUSTER_DISTRIBUTION_REASON)"
  else
    echo "  auto cluster distribution timeout (s): $AUTO_CLUSTER_DISTRIBUTION_TIMEOUT"
  fi
fi

if [ -n "$AUTO_IPC_PROFILE" ]; then
  echo "  auto IPC profile: $AUTO_IPC_PROFILE (window_max=$NM_IPC_WINDOW_MAX send_budget_max=$NM_IPC_SEND_BUDGET_MAX recv_timeout_ms=$NM_UDS_RECV_TIMEOUT_MS grace_ms=$NM_IPC_TIMEOUT_GRACE_MS)"
fi

RUN_WEBOT_BASE=(
  "$ROOT_DIR/run_webot.sh"
  --world "$WORLD_FILE"
  --brains "$BRAINS_CSV"
  --network "$PRIMARY_NETWORK_FILE"
  --config "$PRIMARY_CONFIG_FILE"
  --network-map "$NETWORK_MAP_CSV"
  --config-map "$CONFIG_MAP_CSV"
)
if [ -n "$AUTO_CONNECT_TIMEOUT" ]; then
  RUN_WEBOT_BASE+=(--connect-timeout "$AUTO_CONNECT_TIMEOUT")
fi
if [ -n "$AUTO_CLUSTER_DISTRIBUTION_TIMEOUT" ]; then
  RUN_WEBOT_BASE+=(--cluster-distribution-timeout "$AUTO_CLUSTER_DISTRIBUTION_TIMEOUT")
fi

REMOTE_ARGS=()
if [ "$REMOTE_COMPUTE" = "1" ] || [ "$REMOTE_COMPUTE" = "true" ]; then
  REMOTE_ARGS+=(--remote-compute)
  if [ -n "${REMOTE_HOSTS:-}" ]; then
    REMOTE_ARGS+=(--remote-hosts "$REMOTE_HOSTS")
  fi
  if [ -n "${REMOTE_HOST_WEIGHTS:-}" ]; then
    REMOTE_ARGS+=(--remote-host-weights "$REMOTE_HOST_WEIGHTS")
  fi
  if [ -n "${REMOTE_USER:-}" ]; then
    REMOTE_ARGS+=(--remote-user "$REMOTE_USER")
  fi
  if [ -n "${REMOTE_ROOT_DIR:-}" ]; then
    REMOTE_ARGS+=(--remote-root "$REMOTE_ROOT_DIR")
  fi
  if [ -n "${REMOTE_ORCHESTRATOR_HOST:-}" ]; then
    REMOTE_ARGS+=(--remote-orchestrator-host "$REMOTE_ORCHESTRATOR_HOST")
  fi
  if [ -n "${REMOTE_WEB_UI_HOST:-}" ]; then
    REMOTE_ARGS+=(--remote-web-ui-host "$REMOTE_WEB_UI_HOST")
  fi
  if [ -n "${REMOTE_WEB_UI_PORT:-}" ]; then
    REMOTE_ARGS+=(--remote-web-ui-port "$REMOTE_WEB_UI_PORT")
  fi
  if [ -n "${REMOTE_WEB_UI_API_PORT:-}" ]; then
    REMOTE_ARGS+=(--remote-web-ui-api-port "$REMOTE_WEB_UI_API_PORT")
  fi
  if [ -n "${REMOTE_UI_MODE:-}" ]; then
    REMOTE_ARGS+=(--remote-ui-mode "$REMOTE_UI_MODE")
  fi
  if [ -n "${REMOTE_WEBOTS_HOST:-}" ]; then
    REMOTE_ARGS+=(--remote-webots-host "$REMOTE_WEBOTS_HOST")
  fi
fi

if [ "$UI_MODE" = "rust" ]; then
  EXTRA_ARGS=(--runtime cluster --node-ui-hidden)
  if [ -n "${ORCHESTRATOR_PORT:-}" ]; then
    EXTRA_ARGS+=(--orchestrator-port "$ORCHESTRATOR_PORT")
  fi

  if [ "$REMOTE_COMPUTE" = "1" ] || [ "$REMOTE_COMPUTE" = "true" ]; then
    REMOTE_UI_MODE="${REMOTE_UI_MODE:-web}"
    LOCAL_RUST_UI="${LOCAL_RUST_UI:-1}"
    EXTRA_ARGS+=(--remote-ui-mode "$REMOTE_UI_MODE")
    case "$LOCAL_RUST_UI" in
      1|true|TRUE|yes|YES|on|ON) EXTRA_ARGS+=(--local-rust-ui) ;;
      0|false|FALSE|no|NO|off|OFF) EXTRA_ARGS+=(--no-local-rust-ui) ;;
      *)
        echo "Invalid LOCAL_RUST_UI='$LOCAL_RUST_UI' (use 0/1, true/false, yes/no)"
        exit 1
        ;;
    esac
  fi

  exec "${RUN_WEBOT_BASE[@]}" \
    "${EXTRA_ARGS[@]}" \
    "${REMOTE_ARGS[@]}" \
    "${DEFAULT_SO_ARGS[@]}" \
    "${PASS_THROUGH_ARGS[@]}"
fi

if [ "$UI_MODE" = "cli" ]; then
  EXTRA_ARGS=()
  if [ "$REMOTE_COMPUTE" = "1" ] || [ "$REMOTE_COMPUTE" = "true" ]; then
    EXTRA_ARGS+=(--runtime cluster --node-ui-hidden)
  else
    EXTRA_ARGS+=(--runtime uds)
  fi

  exec "${RUN_WEBOT_BASE[@]}" \
    "${EXTRA_ARGS[@]}" \
    "${REMOTE_ARGS[@]}" \
    "${DEFAULT_SO_ARGS[@]}" \
    "${PASS_THROUGH_ARGS[@]}"
fi

# web mode
if [ "$REMOTE_COMPUTE" = "1" ] || [ "$REMOTE_COMPUTE" = "true" ]; then
  exec "${RUN_WEBOT_BASE[@]}" \
    --runtime cluster \
    --node-ui-hidden \
    --orchestrator-port "$ORCHESTRATOR_PORT" \
    "${REMOTE_ARGS[@]}" \
    "${DEFAULT_SO_ARGS[@]}" \
    "${PASS_THROUGH_ARGS[@]}"
fi

BACKEND_PID=""
PREBUILT_LOCAL_WEB_RUNTIME=0
cleanup() {
  if [ -n "$BACKEND_PID" ] && kill -0 "$BACKEND_PID" 2>/dev/null; then
    kill -TERM "$BACKEND_PID" 2>/dev/null || true
    wait "$BACKEND_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

if ! pass_through_has_arg "--no-build"; then
  echo "Prebuilding local web runtime binaries..."
  cargo build --release --bin aarnn_rust --all-features
  cargo build --release --bin web_ui
  PREBUILT_LOCAL_WEB_RUNTIME=1
fi

RUN_WEBOT_ARGS=(
  "${RUN_WEBOT_BASE[@]}"
  --runtime cluster
  --node-ui-hidden
  --orchestrator-port "$ORCHESTRATOR_PORT"
  --no-orchestrator-ui
)
if [ "$PREBUILT_LOCAL_WEB_RUNTIME" -eq 1 ]; then
  RUN_WEBOT_ARGS+=(--no-build)
fi
RUN_WEBOT_ARGS+=("${DEFAULT_SO_ARGS[@]}")
RUN_WEBOT_ARGS+=("${PASS_THROUGH_ARGS[@]}")

"${RUN_WEBOT_ARGS[@]}" &
BACKEND_PID="$!"

echo "Waiting for orchestrator on port $ORCHESTRATOR_PORT..."
ORCH_READY=0
for _ in $(seq 1 120); do
  if command -v ss >/dev/null 2>&1; then
    if ss -H -ltn | awk '{print $4}' | awk -F: '{print $NF}' | grep -qx "$ORCHESTRATOR_PORT"; then
      ORCH_READY=1
      break
    fi
  fi
  if ! kill -0 "$BACKEND_PID" 2>/dev/null; then
    echo "Backend exited before orchestrator became reachable."
    exit 1
  fi
  sleep 0.5
done

if [ "$ORCH_READY" -ne 1 ]; then
  echo "Timed out waiting for orchestrator on port $ORCHESTRATOR_PORT."
  exit 1
fi

echo "Starting web_ui on $WEB_UI_LISTEN (orchestrator http://127.0.0.1:$ORCHESTRATOR_PORT)"
exec "$ROOT_DIR/target/release/web_ui" \
  --listen "$WEB_UI_LISTEN" \
  --orchestrator "http://127.0.0.1:$ORCHESTRATOR_PORT"
