#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

NETWORK_FILE="${NETWORK_FILE:-$ROOT_DIR/network_drosophila.json}"
CONFIG_FILE="${CONFIG_FILE:-$ROOT_DIR/webots_world/configs/config_drosophila_webots.json}"
WORLD_FILE="${WORLD_FILE:-$ROOT_DIR/webots_world/worlds/drosophila_neuroworld.wbt}"
ORCHESTRATOR_PORT="${ORCHESTRATOR_PORT:-50051}"
WEB_UI_LISTEN="${WEB_UI_LISTEN:-0.0.0.0:8080}"
REMOTE_COMPUTE="${REMOTE_COMPUTE:-0}"

DROSOPHILA_NEURONS_FILE="${DROSOPHILA_NEURONS_FILE:-$ROOT_DIR/data/drosophila/BANC v626/neurons.csv.gz}"
DROSOPHILA_CONNECTIONS_FILE="${DROSOPHILA_CONNECTIONS_FILE:-$ROOT_DIR/data/drosophila/BANC v626/connections_princeton.csv.gz}"
DROSOPHILA_TEMPLATE_FILE="${DROSOPHILA_TEMPLATE_FILE:-$ROOT_DIR/network.json}"
DROSOPHILA_MAX_SENSORY="${DROSOPHILA_MAX_SENSORY:-34}"
DROSOPHILA_MAX_HIDDEN="${DROSOPHILA_MAX_HIDDEN:-20000}"
DROSOPHILA_MAX_OUTPUT="${DROSOPHILA_MAX_OUTPUT:-48}"
DROSOPHILA_MIN_SYN_COUNT="${DROSOPHILA_MIN_SYN_COUNT:-1}"
DROSOPHILA_WEIGHT_TRANSFORM="${DROSOPHILA_WEIGHT_TRANSFORM:-sqrt}"
DROSOPHILA_HIDDEN_LAYER_WIDTH="${DROSOPHILA_HIDDEN_LAYER_WIDTH:-512}"
DROSOPHILA_LONG_RANGE_POLICY="${DROSOPHILA_LONG_RANGE_POLICY:-fold}"
DROSOPHILA_REBUILD_NETWORK="${DROSOPHILA_REBUILD_NETWORK:-0}"

if [ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ]; then
  exec "$ROOT_DIR/run_webot.sh" --help
fi

NEED_REBUILD="$DROSOPHILA_REBUILD_NETWORK"
if [ "$NEED_REBUILD" != "1" ] && [ -f "$NETWORK_FILE" ]; then
  if ! python3 - "$NETWORK_FILE" "$DROSOPHILA_MAX_HIDDEN" "$DROSOPHILA_HIDDEN_LAYER_WIDTH" "$DROSOPHILA_LONG_RANGE_POLICY" <<'PY'
import json
import sys
from pathlib import Path

net_path = Path(sys.argv[1])
want_hidden = int(sys.argv[2])
want_width = int(sys.argv[3])
want_policy = sys.argv[4].strip().lower()
try:
    data = json.loads(net_path.read_text(encoding="utf-8"))
except Exception:
    raise SystemExit(1)
sel = ((data.get("connectome_labels") or {}).get("selection") or {})
if int(sel.get("max_hidden", -1)) != want_hidden:
    raise SystemExit(1)
if int(sel.get("hidden_layer_width", -1)) != want_width:
    raise SystemExit(1)
if str(sel.get("long_range_policy", "")).strip().lower() != want_policy:
    raise SystemExit(1)
raise SystemExit(0)
PY
  then
    NEED_REBUILD=1
  fi
fi

if [ "$NEED_REBUILD" = "1" ] || [ ! -f "$NETWORK_FILE" ]; then
  python3 "$ROOT_DIR/scripts/build_drosophila_network_json.py" \
    --neurons "$DROSOPHILA_NEURONS_FILE" \
    --connections "$DROSOPHILA_CONNECTIONS_FILE" \
    --template "$DROSOPHILA_TEMPLATE_FILE" \
    --output "$NETWORK_FILE" \
    --max-sensory "$DROSOPHILA_MAX_SENSORY" \
    --max-hidden "$DROSOPHILA_MAX_HIDDEN" \
    --max-output "$DROSOPHILA_MAX_OUTPUT" \
    --min-syn-count "$DROSOPHILA_MIN_SYN_COUNT" \
    --weight-transform "$DROSOPHILA_WEIGHT_TRANSFORM" \
    --hidden-layer-width "$DROSOPHILA_HIDDEN_LAYER_WIDTH" \
    --long-range-policy "$DROSOPHILA_LONG_RANGE_POLICY"
fi

python3 "$ROOT_DIR/scripts/build_webots_drosophila_assets.py" \
  --network "$NETWORK_FILE" \
  --config "$CONFIG_FILE" \
  --world "$WORLD_FILE"

if [ "$REMOTE_COMPUTE" = "1" ] || [ "$REMOTE_COMPUTE" = "true" ]; then
  EXTRA_ARGS=(--remote-compute)
  if [ -n "${REMOTE_HOSTS:-}" ]; then
    EXTRA_ARGS+=(--remote-hosts "$REMOTE_HOSTS")
  fi
  if [ -n "${REMOTE_HOST_WEIGHTS:-}" ]; then
    EXTRA_ARGS+=(--remote-host-weights "$REMOTE_HOST_WEIGHTS")
  fi
  if [ -n "${REMOTE_USER:-}" ]; then
    EXTRA_ARGS+=(--remote-user "$REMOTE_USER")
  fi
  if [ -n "${REMOTE_ROOT_DIR:-}" ]; then
    EXTRA_ARGS+=(--remote-root "$REMOTE_ROOT_DIR")
  fi
  if [ -n "${REMOTE_ORCHESTRATOR_HOST:-}" ]; then
    EXTRA_ARGS+=(--remote-orchestrator-host "$REMOTE_ORCHESTRATOR_HOST")
  fi
  if [ -n "${REMOTE_WEB_UI_HOST:-}" ]; then
    EXTRA_ARGS+=(--remote-web-ui-host "$REMOTE_WEB_UI_HOST")
  fi
  if [ -n "${REMOTE_WEB_UI_PORT:-}" ]; then
    EXTRA_ARGS+=(--remote-web-ui-port "$REMOTE_WEB_UI_PORT")
  fi
  if [ -n "${REMOTE_WEB_UI_API_PORT:-}" ]; then
    EXTRA_ARGS+=(--remote-web-ui-api-port "$REMOTE_WEB_UI_API_PORT")
  fi
  if [ -n "${REMOTE_UI_MODE:-}" ]; then
    EXTRA_ARGS+=(--remote-ui-mode "$REMOTE_UI_MODE")
  fi
  if [ -n "${REMOTE_WEBOTS_HOST:-}" ]; then
    EXTRA_ARGS+=(--remote-webots-host "$REMOTE_WEBOTS_HOST")
  fi
  exec "$ROOT_DIR/run_webot.sh" \
    --runtime cluster \
    --world "$WORLD_FILE" \
    --brains default \
    --network "$NETWORK_FILE" \
    --config "$CONFIG_FILE" \
    --orchestrator-port "$ORCHESTRATOR_PORT" \
    "${EXTRA_ARGS[@]}" \
    "$@"
fi

BACKEND_PID=""
cleanup() {
  if [ -n "$BACKEND_PID" ] && kill -0 "$BACKEND_PID" 2>/dev/null; then
    kill -TERM "$BACKEND_PID" 2>/dev/null || true
    wait "$BACKEND_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

"$ROOT_DIR/run_webot.sh" \
  --runtime cluster \
  --world "$WORLD_FILE" \
  --brains default \
  --network "$NETWORK_FILE" \
  --config "$CONFIG_FILE" \
  --orchestrator-port "$ORCHESTRATOR_PORT" \
  --no-orchestrator-ui \
  "$@" &
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
  echo "Hint: if this is a headless session, run with a display or use CLI mode (UDS runtime)."
  exit 1
fi

if [ ! -x "$ROOT_DIR/target/release/web_ui" ]; then
  cargo build --release --bin web_ui
fi

echo "Starting web_ui on $WEB_UI_LISTEN (orchestrator http://127.0.0.1:$ORCHESTRATOR_PORT)"
exec "$ROOT_DIR/target/release/web_ui" \
  --listen "$WEB_UI_LISTEN" \
  --orchestrator "http://127.0.0.1:$ORCHESTRATOR_PORT"
