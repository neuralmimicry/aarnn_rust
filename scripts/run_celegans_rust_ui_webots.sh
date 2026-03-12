#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

NETWORK_FILE="${NETWORK_FILE:-$ROOT_DIR/network_celegans.json}"
CONFIG_FILE="${CONFIG_FILE:-$ROOT_DIR/webots_world/configs/config_celegans_webots.json}"
WORLD_FILE="${WORLD_FILE:-$ROOT_DIR/webots_world/worlds/celegans_neuroworld.wbt}"
REMOTE_COMPUTE="${REMOTE_COMPUTE:-0}"

if [ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ]; then
  exec "$ROOT_DIR/run_webot.sh" --help
fi

python3 "$ROOT_DIR/scripts/build_webots_celegans_assets.py" \
  --network "$NETWORK_FILE" \
  --config "$CONFIG_FILE" \
  --world "$WORLD_FILE"

EXTRA_ARGS=()
if [ -n "${ORCHESTRATOR_PORT:-}" ]; then
  EXTRA_ARGS+=(--orchestrator-port "$ORCHESTRATOR_PORT")
fi

if [ "$REMOTE_COMPUTE" = "1" ] || [ "$REMOTE_COMPUTE" = "true" ]; then
  REMOTE_UI_MODE="${REMOTE_UI_MODE:-web}"
  LOCAL_RUST_UI="${LOCAL_RUST_UI:-1}"
  EXTRA_ARGS+=(--remote-compute)
  EXTRA_ARGS+=(--remote-ui-mode "$REMOTE_UI_MODE")
  case "$LOCAL_RUST_UI" in
    1|true|TRUE|yes|YES|on|ON)
      EXTRA_ARGS+=(--local-rust-ui)
      ;;
    0|false|FALSE|no|NO|off|OFF)
      EXTRA_ARGS+=(--no-local-rust-ui)
      ;;
    *)
      echo "Invalid LOCAL_RUST_UI='$LOCAL_RUST_UI' (use 0/1, true/false, yes/no)"
      exit 1
      ;;
  esac
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
  if [ -n "${REMOTE_WEBOTS_HOST:-}" ]; then
    EXTRA_ARGS+=(--remote-webots-host "$REMOTE_WEBOTS_HOST")
  fi
  exec "$ROOT_DIR/run_webot.sh" \
    --runtime cluster \
    --world "$WORLD_FILE" \
    --brains default \
    --network "$NETWORK_FILE" \
    --config "$CONFIG_FILE" \
    "${EXTRA_ARGS[@]}" \
    "$@"
fi

exec "$ROOT_DIR/run_webot.sh" \
  --runtime cluster \
  --single-orchestrator-ui \
  --world "$WORLD_FILE" \
  --brains default \
  --network "$NETWORK_FILE" \
  --config "$CONFIG_FILE" \
  "${EXTRA_ARGS[@]}" \
  "$@"
