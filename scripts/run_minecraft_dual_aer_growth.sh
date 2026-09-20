#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
ITERATIONS="${AARNN_DUAL_ITERATIONS:-24}"
MINECRAFT_WORLD="${AARNN_MINECRAFT_WORLD:-New World}"
NO_ENGINE=0
KEEP_ALIVE=1
while [ "$#" -gt 0 ]; do
  case "$1" in
    --no-engine) NO_ENGINE=1 ;;
    --iterations) shift; ITERATIONS="${1:?missing iteration count}" ;;
    --help|-h)
      cat <<'EOF'
Usage: scripts/run_minecraft_dual_aer_growth.sh [--no-engine] [--iterations N]
Creates two independent generated AARNN brains and joins them through the
existing Minecraft hexapod companion using a bidirectional legacy AER1 fabric.
With the engine enabled, the companion is bound to the Minecraft mod's default
loopback endpoint, 127.0.0.1:62620. --no-engine runs the Java bridge and
bounded fixture driver without launching a Minecraft client. Live mode keeps
the brains and bridge running until Ctrl-C; use --no-keep-alive for a bounded
engine launch.
EOF
      exit 0 ;;
    --keep-alive) KEEP_ALIVE=1 ;;
    --no-keep-alive) KEEP_ALIVE=0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done
if ! [[ "$ITERATIONS" =~ ^[1-9][0-9]*$ ]]; then
  echo "iterations must be a positive integer" >&2; exit 2
fi
if [ "$NO_ENGINE" -eq 1 ]; then
  KEEP_ALIVE=0
fi

RESULT_DIR="$(mktemp -d "$ROOT_DIR/target/qa/minecraft-dual-aer-XXXXXX")"
CONFIG_A="$RESULT_DIR/brain_a.json"
CONFIG_B="$RESULT_DIR/brain_b.json"
STATS="$RESULT_DIR/aer_stats.json"
MINECRAFT_CONFIG_BACKUP="$RESULT_DIR/aarnn.json.before"
MINECRAFT_CONFIG_PATH=""
TOKEN="$(python3 -c 'import secrets; print(secrets.token_urlsafe(32))')"
PIDS=()
FAILURE_COMMAND=""

free_port() {
  python3 - <<'PY'
import socket
with socket.socket() as s:
    s.bind(("127.0.0.1", 0))
    print(s.getsockname()[1])
PY
}
wait_port() {
  local port="$1" name="$2" pid="${3:-}" deadline="$((SECONDS + 60))"
  while [ "$SECONDS" -lt "$deadline" ]; do
    if [ -n "$pid" ] && ! kill -0 "$pid" 2>/dev/null; then
      echo "$name exited before port $port became ready" >&2; return 1
    fi
    if python3 - "$port" <<'PY'
import socket, sys
try:
    with socket.create_connection(("127.0.0.1", int(sys.argv[1])), .2): pass
except OSError: raise SystemExit(1)
PY
    then return 0; fi
    sleep .2
  done
  echo "timed out waiting for $name on port $port" >&2; return 1
}
wait_bridge_health() {
  local port="$1" name="$2" pid="$3" deadline="$((SECONDS + 60))"
  while [ "$SECONDS" -lt "$deadline" ]; do
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "$name exited before authenticated health became ready" >&2
      return 1
    fi
    if AARNN_MINECRAFT_TOKEN="$TOKEN" python3 - "$port" <<'PY'
import json
import os
import sys
import urllib.error
import urllib.request

request = urllib.request.Request(
    f"http://127.0.0.1:{int(sys.argv[1])}/api/aarnn/health",
    headers={"Authorization": "Bearer " + os.environ["AARNN_MINECRAFT_TOKEN"]},
)
try:
    with urllib.request.urlopen(request, timeout=0.5) as response:
        data = json.loads(response.read(8192))
    if response.status == 200 and "content_digest" in data and "hexapod" in data.get("profiles", []):
        raise SystemExit(0)
except (OSError, ValueError, urllib.error.HTTPError, urllib.error.URLError):
    pass
raise SystemExit(1)
PY
    then return 0; fi
    sleep .2
  done
  echo "timed out waiting for authenticated $name on $port" >&2
  return 1
}
wait_live_frames() {
  local stats="$1" expected="$2" deadline="$((SECONDS + 180))"
  while [ "$SECONDS" -lt "$deadline" ]; do
    if [ -f "$stats" ] && python3 - "$stats" "$expected" <<'PY'
import json
import pathlib
import sys

data = json.loads(pathlib.Path(sys.argv[1]).read_text())
raise SystemExit(0 if int(data.get("frontend_frames", 0)) >= int(sys.argv[2]) else 1)
PY
    then return 0; fi
    sleep 1
  done
  echo "Minecraft did not deliver $expected live hexapod frames within 180 seconds" >&2
  return 1
}
cleanup() {
  local code=$?
  trap - EXIT INT TERM ERR
  for pid in "${PIDS[@]}"; do kill "$pid" 2>/dev/null || true; done
  for pid in "${PIDS[@]}"; do wait "$pid" 2>/dev/null || true; done
  if [ -n "$MINECRAFT_CONFIG_PATH" ] && [ -f "$MINECRAFT_CONFIG_BACKUP" ]; then
    if [ "$(cat "$MINECRAFT_CONFIG_BACKUP")" = "<absent>" ]; then
      rm -f "$MINECRAFT_CONFIG_PATH"
    else
      cp "$MINECRAFT_CONFIG_BACKUP" "$MINECRAFT_CONFIG_PATH"
    fi
    echo "[aarnn] restored Minecraft config: $MINECRAFT_CONFIG_PATH" >&2
  fi
  [ -f "$STATS" ] && cp "$STATS" "$RESULT_DIR/aer_stats.final.json"
  if [ ! -f "$RESULT_DIR/result.json" ]; then
    python3 - "$RESULT_DIR" "$CONFIG_A" "$CONFIG_B" "$ITERATIONS" "$code" "$FAILURE_COMMAND" <<'PY'
import hashlib
import json
import pathlib
import re
import sys

result_dir = pathlib.Path(sys.argv[1])
config_a = pathlib.Path(sys.argv[2])
config_b = pathlib.Path(sys.argv[3])
iterations, exit_code, failed_command = sys.argv[4], int(sys.argv[5]), sys.argv[6]

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest() if path.exists() else None

def growth(path):
    if not path.exists():
        return 0
    text = path.read_text(errors="replace")
    return sum(int(value) for value in re.findall(r"growth_spawn\s+\|\s+hits=([0-9]+)", text)) \
        + len(re.findall(r"^\[growth\]", text, re.M))

stats_path = result_dir / "aer_stats.final.json"
stats = json.loads(stats_path.read_text()) if stats_path.exists() else {}
report = {
    "status": "failed",
    "exit_code": exit_code,
    "failed_command": failed_command or None,
    "compatibility": "legacy AER1 loopback; bounded Minecraft hexapod fixture",
    "two_independent_configs": digest(config_a) != digest(config_b),
    "brain_a_config_sha256": digest(config_a),
    "brain_b_config_sha256": digest(config_b),
    "iterations_requested": int(iterations),
    "aer": stats,
    "startup": {
        "brain_a_log_present": (result_dir / "brain_a.log").exists(),
        "brain_b_log_present": (result_dir / "brain_b.log").exists(),
        "brain_a_ready": "listening on" in (result_dir / "brain_a.log").read_text(errors="replace")
            if (result_dir / "brain_a.log").exists() else False,
        "brain_b_ready": "listening on" in (result_dir / "brain_b.log").read_text(errors="replace")
            if (result_dir / "brain_b.log").exists() else False,
        "dual_fabric_log_present": (result_dir / "fabric.log").exists(),
        "java_bridge_log_present": (result_dir / "bridge.log").exists(),
    },
    "growth": {
        "brain_a_log_events": growth(result_dir / "brain_a.log"),
        "brain_b_log_events": growth(result_dir / "brain_b.log"),
    },
    "limitations": [
        "fixture score is reference evidence, not Minecraft physics telemetry",
        "production AARNN-AER/1 admission, committed effects and EffectId fencing are not exercised",
    ],
}
(result_dir / "result.json").write_text(json.dumps(report, indent=2) + "\n")
PY
  fi
  echo "dual AER receipt: $RESULT_DIR/result.json" >&2
  if [ "$code" -ne 0 ]; then
    echo "dual AER run failed after startup; inspect the retained brain and bridge logs above" >&2
  fi
  exit "$code"
}
trap cleanup EXIT INT TERM
trap 'FAILURE_COMMAND=$BASH_COMMAND' ERR

python3 - "$ROOT_DIR/webots_world/configs/config_hexapod_webots.json" "$CONFIG_A" "$CONFIG_B" <<'PY'
import json, pathlib, sys
source, out_a, out_b = map(pathlib.Path, sys.argv[1:])
base = json.loads(source.read_text())
common = {
    "num_sensory_neurons": 34, "num_output_neurons": 18,
    "num_hidden_layers": 2, "num_hidden_per_layer_initial": 24,
    "max_total_neurons": 150, "growth_enabled": True,
    "use_morphology": False, "morpho_growth_enabled": False,
    "saturation_threshold": 0.0, "saturation_window_ms": 1.0,
    "growth_cooldown_ms": 0.0, "global_growth_cooldown_ms": 0.0,
    "development_growth_interval_ms": 1.0, "spontaneous_neuron_interval_ms": 5.0,
    "development_io_formation_interval_ms": 1.0, "max_layers": 3,
    "layer_split_threshold": 10000,
}
def make(overrides):
    value = dict(base); value.update(common); value.update(overrides); return value
out_a.write_text(json.dumps(make({"p_in": .18, "p_hidden": .11, "p_out": .24}), indent=2) + "\n")
out_b.write_text(json.dumps(make({"p_in": .31, "p_hidden": .07, "p_out": .16}), indent=2) + "\n")
PY

if [ ! -x "$ROOT_DIR/target/release/examples/nn_tcp_server" ]; then
  cargo build --release --locked --all-features --example nn_tcp_server
fi
BRIDGE_JAR="$(python3 "$ROOT_DIR/scripts/minecraft.py" artifact --kind bridge --edition java)"
JAVA_BIN="$(python3 "$ROOT_DIR/scripts/minecraft.py" java)"
PORT_A="$(free_port)" ; PORT_B="$(free_port)"
PORT_PROXY="$(free_port)"
if [ -n "${AARNN_MINECRAFT_HTTP_PORT:-}" ]; then
  PORT_HTTP="$AARNN_MINECRAFT_HTTP_PORT"
elif [ "$NO_ENGINE" -eq 0 ]; then
  # The generated Minecraft config below pins the client to this endpoint.
  # Pick a free port so a previous live run cannot be mistaken for this bridge.
  PORT_HTTP="$(free_port)"
else
  PORT_HTTP="$(free_port)"
fi
if ! [[ "$PORT_HTTP" =~ ^[1-9][0-9]*$ ]] || [ "$PORT_HTTP" -gt 65535 ]; then
  echo "AARNN_MINECRAFT_HTTP_PORT must be a valid TCP port" >&2
  exit 2
fi
unset NM_REALTIME_IPC
export AARNN_MINECRAFT_TOKEN="$TOKEN"
export RAYON_NUM_THREADS="${RAYON_NUM_THREADS:-2}"

"$ROOT_DIR/target/release/examples/nn_tcp_server" --tcp "127.0.0.1:$PORT_A" \
  --sensory 34 --output 18 --config "$CONFIG_A" --model aarnn --learning aarnn \
  >"$RESULT_DIR/brain_a.log" 2>&1 & PIDS+=("$!")
"$ROOT_DIR/target/release/examples/nn_tcp_server" --tcp "127.0.0.1:$PORT_B" \
  --sensory 34 --output 18 --config "$CONFIG_B" --model aarnn --learning aarnn \
  >"$RESULT_DIR/brain_b.log" 2>&1 & PIDS+=("$!")
wait_port "$PORT_A" brain-a "${PIDS[0]}"
echo "[aarnn] brain A ready (pid=${PIDS[0]}, tcp=127.0.0.1:$PORT_A)" >&2
wait_port "$PORT_B" brain-b "${PIDS[1]}"
echo "[aarnn] brain B ready (pid=${PIDS[1]}, tcp=127.0.0.1:$PORT_B)" >&2

python3 "$ROOT_DIR/scripts/qa/minecraft_dual_aer_growth.py" serve \
  --listen-port "$PORT_PROXY" --brain-a "$PORT_A" --brain-b "$PORT_B" \
  --stats "$STATS" --fixture "$ROOT_DIR/sim/minecraft/build/reference.json" \
  >"$RESULT_DIR/fabric.log" 2>&1 & PIDS+=("$!")
wait_port "$PORT_PROXY" dual-fabric "${PIDS[2]}"
echo "[aarnn] dual AER fabric ready (tcp=127.0.0.1:$PORT_PROXY; hops=A->B->A)" >&2

"$JAVA_BIN" -jar "$BRIDGE_JAR" --port "$PORT_HTTP" --base-port "$PORT_PROXY" --profiles hexapod \
  >"$RESULT_DIR/bridge.log" 2>&1 & PIDS+=("$!")
wait_bridge_health "$PORT_HTTP" java-bridge "${PIDS[3]}"
echo "[aarnn] Java bridge ready (http=127.0.0.1:$PORT_HTTP; profile=hexapod)" >&2
echo "[aarnn] receipt directory: $RESULT_DIR" >&2

if [ "$NO_ENGINE" -eq 0 ]; then
  MINECRAFT_DOCTOR="$RESULT_DIR/minecraft-doctor.json"
  python3 "$ROOT_DIR/scripts/minecraft.py" doctor --require engine --edition java >"$MINECRAFT_DOCTOR"
  MINECRAFT_GAME_DIR="$(python3 - "$MINECRAFT_DOCTOR" <<'PY'
import json
import pathlib
import sys

data = json.loads(pathlib.Path(sys.argv[1]).read_text())
print(data["game_directory"])
PY
)"
  CONTENT_DIGEST="$(python3 - "$ROOT_DIR/sim/content/compiled.generated.json" <<'PY'
import json
import pathlib
import sys

print(json.loads(pathlib.Path(sys.argv[1]).read_text())["digest"])
PY
)"
MINECRAFT_CONFIG_PATH="$(python3 - "$MINECRAFT_GAME_DIR" "$MINECRAFT_CONFIG_BACKUP" "$PORT_HTTP" "$CONTENT_DIGEST" "$ITERATIONS" <<'PY'
import json
import os
import pathlib
import shutil
import sys
import tempfile

game_dir = pathlib.Path(sys.argv[1])
backup = pathlib.Path(sys.argv[2])
http_port = sys.argv[3]
digest = sys.argv[4]
first_step = int(sys.argv[5])
config = game_dir / "config" / "aarnn.json"
config.parent.mkdir(parents=True, exist_ok=True)
if config.exists():
    shutil.copy2(config, backup)
else:
    backup.write_text("<absent>\n")

data = json.loads(config.read_text()) if config.exists() else {}
data["schemaVersion"] = 1
data["allowLegacySandboxInference"] = True
data["endpoint"] = f"http://127.0.0.1:{http_port}/api/aer/infer"
data["tokenEnvironment"] = "AARNN_MINECRAFT_TOKEN"
data["contentDigest"] = digest
data["autoConnectOnStart"] = True
data["autoConnectProfile"] = "hexapod"
data["autoVisitProfile"] = True
bindings = data.setdefault("bindings", {})
bindings["hexapod"] = {
    "networkId": "hexapod",
    "nodeId": None,
    "sensory": 34,
    "output": 18,
    "firstStep": first_step,
}
fd, temporary = tempfile.mkstemp(prefix="aarnn.json.", dir=config.parent)
try:
    with os.fdopen(fd, "w") as stream:
        json.dump(data, stream, indent=2)
        stream.write("\n")
    os.chmod(temporary, config.stat().st_mode & 0o777 if config.exists() else 0o644)
    os.replace(temporary, config)
finally:
    if os.path.exists(temporary):
        os.unlink(temporary)
print(config)
PY
)"
echo "[aarnn] Minecraft hexapod binding prepared in $MINECRAFT_GAME_DIR/config/aarnn.json" >&2
  echo "[aarnn] previous config retained at $MINECRAFT_CONFIG_BACKUP" >&2
  python3 "$ROOT_DIR/scripts/minecraft.py" launch --edition java --direct --world "$MINECRAFT_WORLD" >"$RESULT_DIR/minecraft.log" 2>&1 & PIDS+=("$!")
  echo "Fabric Minecraft started directly with quick-play world '$MINECRAFT_WORLD'; the lab will build, arm and visit the hexapod automatically." >&2
  echo "Waiting for $ITERATIONS live hexapod frames; /aarnn status remains a compact diagnostic." >&2
  wait_live_frames "$STATS" "$ITERATIONS"
else
  python3 "$ROOT_DIR/scripts/qa/minecraft_dual_aer_growth.py" drive-http \
    --bridge-port "$PORT_HTTP" --token="$TOKEN" \
    --fixture "$ROOT_DIR/sim/minecraft/build/reference.json" --iterations "$ITERATIONS"
fi

sleep 1
python3 - "$RESULT_DIR" "$CONFIG_A" "$CONFIG_B" "$ITERATIONS" "$PORT_HTTP" <<'PY'
import hashlib, json, pathlib, re, sys
result_dir, config_a, config_b = map(pathlib.Path, sys.argv[1:4])
iterations = int(sys.argv[4])
http_port = int(sys.argv[5])
def digest(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def growth(path):
    if not path.exists():
        return 0
    text = path.read_text(errors="replace")
    marker = re.findall(r"growth_spawn\s+\|\s+hits=([0-9]+)", text)
    return sum(int(value) for value in marker) + len(re.findall(r"^\[growth\]", text, re.M))
stats = json.loads((result_dir / "aer_stats.json").read_text())
a = stats.get("baseline_scores", []); c = stats.get("coupled_scores", [])
if not a or not c: raise SystemExit("control score evidence was not collected")
report = {
  "status": "pass",
  "compatibility": "legacy AER1 loopback; bounded Minecraft hexapod fixture",
  "startup": {"brain_a_ready": True, "brain_b_ready": True,
               "dual_fabric_ready": True, "java_bridge_ready": True,
               "minecraft_http_endpoint": f"http://127.0.0.1:{http_port}/api/aer/infer"},
  "two_independent_configs": digest(config_a) != digest(config_b),
  "brain_a_config_sha256": digest(config_a), "brain_b_config_sha256": digest(config_b),
  "iterations": iterations, "aer": stats,
  "growth": {"brain_a_log_events": growth(result_dir/"brain_a.log"),
             "brain_b_log_events": growth(result_dir/"brain_b.log")},
  "control": {"isolated_a_mean": sum(a)/len(a), "coupled_mean": sum(c)/len(c),
              "improvement": sum(c)/len(c)-sum(a)/len(a)},
  "limitations": ["fixture score is reference evidence, not Minecraft physics telemetry",
                  "production AARNN-AER/1 admission, committed effects and EffectId fencing are not exercised"],
}
if not report["two_independent_configs"] or stats["frontend_frames"] < iterations:
    raise SystemExit("dual fabric did not process the requested frames")
if stats["a_to_b_frames"] == 0 or stats["b_to_a_frames"] == 0:
    raise SystemExit("bidirectional AER frame evidence is missing")
if report["growth"]["brain_a_log_events"] + report["growth"]["brain_b_log_events"] == 0:
    raise SystemExit("no growth evidence was emitted by either brain")
if report["control"]["improvement"] <= 0:
    raise SystemExit("coupled fixture control did not improve over isolated brain A")
(result_dir/"result.json").write_text(json.dumps(report, indent=2)+"\n")
print(json.dumps(report, indent=2))
PY

if [ "$KEEP_ALIVE" -eq 1 ]; then
  echo "[aarnn] live stack is running; the configured Fabric client auto-builds and arms hexapod." >&2
  echo "        Use /aarnn status after the world loads to confirm frames in/out." >&2
  echo "        Manual recovery commands: /aarnn world; /aarnn visit hexapod; /aarnn connect hexapod" >&2
  echo "[aarnn] brains, AER fabric and bridge remain active; press Ctrl-C here to stop them" >&2
  while :; do sleep 1; done
fi
