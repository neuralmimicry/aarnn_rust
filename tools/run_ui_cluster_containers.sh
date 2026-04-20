#!/usr/bin/env bash
set -euo pipefail

# Run a distributed cluster (orchestrator + nodes) in containers with Rust UI on orchestrator.

if ! command -v podman >/dev/null 2>&1; then
  echo "podman not found." >&2
  exit 1
fi
if ! command -v xauth >/dev/null 2>&1; then
  echo "xauth not found. Install xauth and try again." >&2
  exit 1
fi
if ! command -v ss >/dev/null 2>&1; then
  echo "ss not found. Install iproute2 and try again." >&2
  exit 1
fi

if [ -z "${DISPLAY:-}" ]; then
  echo "DISPLAY is not set. Use ssh -X/-Y to connect with X11 forwarding." >&2
  exit 1
fi

ARCH_RAW="$(uname -m)"
case "${ARCH_RAW}" in
  x86_64|amd64) IMAGE_ARCH="amd64" ;;
  aarch64|arm64) IMAGE_ARCH="arm64" ;;
  *) IMAGE_ARCH="${ARCH_RAW}" ;;
esac

IMAGE_NAME="${IMAGE_NAME:-ghcr.io/neuralmimicry/aarnn_rust:engine-${IMAGE_ARCH}}"
BRAIN_ID_ORCH="${BRAIN_ID_ORCH:-cluster_master}"
NODE_COUNT="${NODE_COUNT:-2}"

CONFIG_PATH="${CONFIG_PATH:-config.json}"
NETWORK_PATH="${NETWORK_PATH:-network_aarnn_6layer.json}"

OUTPUT_DIR="${OUTPUT_DIR:-$PWD/outputs}"
LOG_DIR="${LOG_DIR:-$PWD/logs}"

mkdir -p "$OUTPUT_DIR" "$LOG_DIR" "$HOME/.cache"

# ----- X11 auth setup -----
XAUTH=/tmp/.podman.xauth
rm -f "$XAUTH"
touch "$XAUTH"

XAUTHORITY_SRC="${XAUTHORITY:-$HOME/.Xauthority}"
if [ ! -f "$XAUTHORITY_SRC" ]; then
  echo "Source Xauthority file not found: $XAUTHORITY_SRC" >&2
  exit 1
fi

if ! xauth -f "$XAUTHORITY_SRC" extract "$XAUTH" "$DISPLAY" >/dev/null 2>&1; then
  echo "Failed to extract Xauthority for DISPLAY=$DISPLAY from $XAUTHORITY_SRC" >&2
  echo "Available entries:" >&2
  xauth -f "$XAUTHORITY_SRC" list >&2 || true
  exit 1
fi

if ! xauth -f "$XAUTH" list >/dev/null 2>&1; then
  echo "Failed to create Xauthority file at $XAUTH" >&2
  exit 1
fi

# ----- Dynamic port selection helpers -----
declare -A USED_PORTS=()
reserve_port() { USED_PORTS[$1]=1; }

is_port_free() {
  local port="$1"
  if ss -H -ltn | awk '{print $4}' | awk -F: '{print $NF}' | grep -qx "$port"; then
    return 1
  fi
  if ss -H -lun | awk '{print $4}' | awk -F: '{print $NF}' | grep -qx "$port"; then
    return 1
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
    p=$((p+1))
  done
  echo ""; return 1
}

ORCH_PORT="$(find_free_port 50051)"; reserve_port "$ORCH_PORT"
NODE_BASE_PORT="${NODE_BASE_PORT:-50075}"

CONFIG_ARG=()
if [ -f "$CONFIG_PATH" ]; then
  CONFIG_ARG=(--config "$CONFIG_PATH")
  echo "Using config: $CONFIG_PATH"
else
  echo "Config file '$CONFIG_PATH' not found; using defaults"
fi

NETWORK_ARG=()
if [ -f "$NETWORK_PATH" ]; then
  NETWORK_ARG=(--network "$NETWORK_PATH")
  echo "Using network snapshot: $NETWORK_PATH"
else
  echo "Network snapshot '$NETWORK_PATH' not found; skipping --network"
fi

COMMON_OPTS=(
  --network=host
  --cpus=8
  --user "$(id -u):$(id -g)"
  -e DISPLAY="$DISPLAY"
  -e XAUTHORITY=/tmp/.Xauthority
  -e XDG_CACHE_HOME=/tmp/cache
  -e FONTCONFIG_PATH=/etc/fonts
  -e LIBGL_ALWAYS_SOFTWARE=1
  -e MESA_GL_VERSION_OVERRIDE=3.3
  -e MESA_LOADER_DRIVER_OVERRIDE=llvmpipe
  -e NMD_TFLITE_ALLOW_LARGE=1
  -v "$XAUTH:/tmp/.Xauthority:ro"
  -v "$HOME/.cache:/tmp/cache:Z"
  -v "$OUTPUT_DIR:/app/outputs:Z"
  -v "$LOG_DIR:/app/logs:Z"
)

RUN_ID="$(date +%s)"
ORCH_NAME="nm-orch-${RUN_ID}"

PIDS=()
CONTAINERS=()

cleanup() {
  echo "Shutting down containers..."
  for name in "${CONTAINERS[@]}"; do
    if [ -n "$name" ]; then
      podman stop "$name" >/dev/null 2>&1 || true
    fi
  done
  for pid in "${PIDS[@]}"; do
    if [ -n "$pid" ]; then
      kill "$pid" >/dev/null 2>&1 || true
    fi
  done
}
trap cleanup SIGINT SIGTERM EXIT

echo "Selected ports -> Orchestrator gRPC: $ORCH_PORT"

# Orchestrator
ORCH_LOG="$LOG_DIR/orchestrator.log"
CONTAINERS+=("$ORCH_NAME")

podman run --rm --name "$ORCH_NAME" \
  "${COMMON_OPTS[@]}" \
  "$IMAGE_NAME" \
  --orchestrator --brain-id "$BRAIN_ID_ORCH" \
  --grpc-addr "0.0.0.0:$ORCH_PORT" \
  "${CONFIG_ARG[@]}" "${NETWORK_ARG[@]}" \
  --ui --quiet \
  > "$ORCH_LOG" 2>&1 &
PIDS+=("$!")

echo "Orchestrator started (log: $ORCH_LOG)"

# Give orchestrator time to start
sleep 2

# Nodes
for i in $(seq 1 "$NODE_COUNT"); do
  NODE_PORT="$(find_free_port $((NODE_BASE_PORT + i - 1)))"; reserve_port "$NODE_PORT"
  NODE_NAME="nm-node-${i}-${RUN_ID}"
  NODE_LOG="$LOG_DIR/node_${i}.log"
  CONTAINERS+=("$NODE_NAME")

  podman run --rm --name "$NODE_NAME" \
    "${COMMON_OPTS[@]}" \
    "$IMAGE_NAME" \
    --node --brain-id "node_${i}" \
    --grpc-addr "0.0.0.0:$NODE_PORT" \
    --orchestrator-addr "http://127.0.0.1:$ORCH_PORT" --quiet \
    > "$NODE_LOG" 2>&1 &
  PIDS+=("$!")

  echo "Node ${i} started (log: $NODE_LOG)"
  sleep 1
 done

echo "----------------------------------------------------------------"
echo "Cluster running in containers."
echo "Orchestrator UI should be visible on your X11 display."
echo "Orchestrator log: $ORCH_LOG"
echo "Nodes logs: $LOG_DIR/node_1.log, $LOG_DIR/node_2.log"
echo "Press Ctrl+C to stop all containers."
echo "----------------------------------------------------------------"

wait
