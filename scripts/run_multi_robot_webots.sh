#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

UI_MODE="${UI_MODE:-rust}"  # rust|web|cli
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
WEBOTS_WORKSPACE_RESUME_EXISTING="${WEBOTS_WORKSPACE_RESUME_EXISTING:-1}"

COUNT_CELEGANS_OVERRIDE=""
COUNT_DROSOPHILA_BANC_OVERRIDE=""
COUNT_DROSOPHILA_FAFB_OVERRIDE=""
COUNT_NAO_OVERRIDE=""

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

CELEGANS_TEMPLATE_FILE="${CELEGANS_TEMPLATE_FILE:-$ROOT_DIR/network.json}"
CELEGANS_CONNECTOME_FILE="${CELEGANS_CONNECTOME_FILE:-$ROOT_DIR/celegans.py}"
CELEGANS_REBUILD_NETWORK="${CELEGANS_REBUILD_NETWORK:-0}"
CELEGANS_REBUILD_ASSETS="${CELEGANS_REBUILD_ASSETS:-0}"
DROSOPHILA_REBUILD_ASSETS="${DROSOPHILA_REBUILD_ASSETS:-0}"

NAO_TEMPLATE_FILE="${NAO_TEMPLATE_FILE:-$ROOT_DIR/network.json}"
NAO_NETWORK_SCRIPT="${NAO_NETWORK_SCRIPT:-$ROOT_DIR/scripts/build_nao_network_json.py}"
NAO_PROTO_FILE="${NAO_PROTO_FILE:-}"
NAO_CAMERA_RETINA_WIDTH="${NAO_CAMERA_RETINA_WIDTH:-160}"
NAO_CAMERA_RETINA_HEIGHT="${NAO_CAMERA_RETINA_HEIGHT:-120}"
NAO_EXPECTED_SENSORY="${NAO_EXPECTED_SENSORY:-$((58 + 4 * NAO_CAMERA_RETINA_WIDTH * NAO_CAMERA_RETINA_HEIGHT))}"
NAO_EXPECTED_OUTPUT="${NAO_EXPECTED_OUTPUT:-40}"
NAO_HIDDEN_LAYERS="${NAO_HIDDEN_LAYERS:-6}"
NAO_HIDDEN_PER_LAYER="${NAO_HIDDEN_PER_LAYER:-${NAO_HIDDEN_NEURONS:-96}}"
NAO_AARNN_DEPTH="${NAO_AARNN_DEPTH:-5}"
NAO_GROWTH_HEADROOM="${NAO_GROWTH_HEADROOM:-1.8}"
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
    if [ "$arg" = "$needle" ]; then
      return 0
    fi
  done
  return 1
}

usage() {
  cat <<'USAGE'
Usage: scripts/run_multi_robot_webots.sh [options] [run_webot passthrough args]

Options:
  --ui-mode <rust|web|cli>   Frontend mode (default: rust).
  --robots <spec>            Robot count spec, e.g.
                             "drosophila_fafb=1,drosophila_banc=3,celegans=2,nao=3"
  --celegans <n>             Override celegans count.
  --drosophila-banc <n>      Override BANC drosophila count.
  --drosophila-fafb <n>      Override FAFB drosophila count.
  --nao <n>                  Override Nao count.
  --world <path>             Output mixed world path.
  --help                     Show this help.

Environment:
  UI_MODE, ROBOT_SPEC, REMOTE_COMPUTE, ORCHESTRATOR_PORT, WEB_UI_LISTEN,
  WEBOTS_RUNTIME_ROOT, WEBOTS_RUNTIME_USER, WEBOTS_WORKSPACE_PREFIX,
  WEBOTS_WORKSPACE_AUTOSAVE_STEPS, WEBOTS_WORKSPACE_RESUME_EXISTING,
  CELEGANS_* / DROSOPHILA_* / NAO_* path and build variables.

Notes:
  - All robot instances are placed into a single Webots world.
  - Each instance gets a unique brain ID.
  - A single cluster runtime is launched with per-brain network/config mapping.
USAGE
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --ui-mode)
      shift
      UI_MODE="${1:-$UI_MODE}"
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
    --nao)
      shift
      COUNT_NAO_OVERRIDE="${1:-}"
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
counts = {"celegans": 0, "drosophila_banc": 0, "drosophila_fafb": 0, "nao": 0}
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
    "nao": "nao",
    "naos": "nao",
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
apply_override_count COUNT_NAO "$COUNT_NAO_OVERRIDE"

TOTAL_ROBOTS=$((COUNT_CELEGANS + COUNT_DROSOPHILA_BANC + COUNT_DROSOPHILA_FAFB + COUNT_NAO))
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

  # Full-resolution NAO camera event streams must use compact AER packets and
  # a larger UDS receive buffer to remain non-blocking and resilient.
  NM_IPC_FORCE_AER="${NM_IPC_FORCE_AER:-1}"
  NM_IPC_MAX_RAW_BYTES="${NM_IPC_MAX_RAW_BYTES:-60000}"
  NM_IPC_AER_MAX_PACKET_BYTES="${NM_IPC_AER_MAX_PACKET_BYTES:-60000}"
  NM_IPC_UDS_RECV_BUF_BYTES="${NM_IPC_UDS_RECV_BUF_BYTES:-262144}"
  export NM_IPC_FORCE_AER NM_IPC_MAX_RAW_BYTES NM_IPC_AER_MAX_PACKET_BYTES NM_IPC_UDS_RECV_BUF_BYTES
fi

declare -a BRAINS=()
declare -a CELEGANS_BRAINS=()
declare -a DROS_BANC_BRAINS=()
declare -a DROS_FAFB_BRAINS=()
declare -a NAO_BRAINS=()
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

  local bindings_json
  bindings_json="$(python3 "$helper" \
    --root "$WEBOTS_RUNTIME_ROOT" \
    --user "$WEBOTS_RUNTIME_USER" \
    --autosave-steps "$WEBOTS_WORKSPACE_AUTOSAVE_STEPS" \
    --resume-existing "$WEBOTS_WORKSPACE_RESUME_EXISTING" \
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
  rebuild_network_map_from_workspace_bindings "$bindings_json"
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

for i in $(seq 1 "$COUNT_NAO"); do
  brain_id="$(printf "nao_%02d" "$i")"
  NAO_BRAINS+=("$brain_id")
  add_brain_mapping "$brain_id" "$NAO_NETWORK_FILE" "$NAO_CONFIG_FILE"
done

BRAINS_CSV="$(IFS=','; echo "${BRAINS[*]}")"
NETWORK_MAP_CSV="$(IFS=','; echo "${NETWORK_MAP_ENTRIES[*]}")"
CONFIG_MAP_CSV="$(IFS=','; echo "${CONFIG_MAP_ENTRIES[*]}")"
CELEGANS_BRAINS_CSV="$(IFS=','; echo "${CELEGANS_BRAINS[*]}")"
DROS_BANC_BRAINS_CSV="$(IFS=','; echo "${DROS_BANC_BRAINS[*]}")"
DROS_FAFB_BRAINS_CSV="$(IFS=','; echo "${DROS_FAFB_BRAINS[*]}")"
NAO_BRAINS_CSV="$(IFS=','; echo "${NAO_BRAINS[*]}")"

prepare_runtime_workspaces

python3 "$ROOT_DIR/scripts/build_webots_multi_world.py" \
  --world "$WORLD_FILE" \
  --celegans-proto "$CELEGANS_PROTO_FILE" \
  --drosophila-banc-proto "$DROSOPHILA_BANC_PROTO_FILE" \
  --drosophila-fafb-proto "$DROSOPHILA_FAFB_PROTO_FILE" \
  --celegans-brains "$CELEGANS_BRAINS_CSV" \
  --drosophila-banc-brains "$DROS_BANC_BRAINS_CSV" \
  --drosophila-fafb-brains "$DROS_FAFB_BRAINS_CSV" \
  --nao-brains "$NAO_BRAINS_CSV"

echo "Multi-robot launch composition:"
echo "  celegans: $COUNT_CELEGANS"
echo "  drosophila_banc: $COUNT_DROSOPHILA_BANC"
echo "  drosophila_fafb: $COUNT_DROSOPHILA_FAFB"
echo "  nao: $COUNT_NAO"
echo "  total robots/brains: $TOTAL_ROBOTS"
echo "  world: $WORLD_FILE"
echo "  brains: $BRAINS_CSV"
echo "  runtime root: $WEBOTS_RUNTIME_ROOT"
echo "  runtime user: $WEBOTS_RUNTIME_USER"
echo "  workspace prefix: $WEBOTS_WORKSPACE_PREFIX"

RUN_WEBOT_BASE=(
  "$ROOT_DIR/run_webot.sh"
  --world "$WORLD_FILE"
  --brains "$BRAINS_CSV"
  --network "$PRIMARY_NETWORK_FILE"
  --config "$PRIMARY_CONFIG_FILE"
  --network-map "$NETWORK_MAP_CSV"
  --config-map "$CONFIG_MAP_CSV"
)

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
    "${PASS_THROUGH_ARGS[@]}"
fi

# web mode
if [ "$REMOTE_COMPUTE" = "1" ] || [ "$REMOTE_COMPUTE" = "true" ]; then
  exec "${RUN_WEBOT_BASE[@]}" \
    --runtime cluster \
    --node-ui-hidden \
    --orchestrator-port "$ORCHESTRATOR_PORT" \
    "${REMOTE_ARGS[@]}" \
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
