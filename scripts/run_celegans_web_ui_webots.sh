#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

NETWORK_FILE="${NETWORK_FILE:-$ROOT_DIR/network_celegans.json}"
CONFIG_FILE="${CONFIG_FILE:-$ROOT_DIR/webots_world/configs/config_celegans_webots.json}"
WORLD_FILE="${WORLD_FILE:-$ROOT_DIR/webots_world/worlds/celegans_neuroworld.wbt}"
ORCHESTRATOR_PORT="${ORCHESTRATOR_PORT:-50051}"
WEB_UI_LISTEN="${WEB_UI_LISTEN:-0.0.0.0:8080}"
REMOTE_COMPUTE="${REMOTE_COMPUTE:-0}"

if [ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ]; then
  exec "$ROOT_DIR/run_webot.sh" --help
fi

python3 "$ROOT_DIR/scripts/build_webots_celegans_assets.py" \
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
