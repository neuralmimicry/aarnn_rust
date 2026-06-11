#!/usr/bin/env python3
"""
Launch a mixed local NAO Webots cluster:

- 1 orchestrator with the web UI
- 1 worker node with the native Rust UI and IPC socket for Webots
- 2 worker nodes in CLI/headless mode

All workers advertise the same shared network id so the orchestrator shards one
NAO network across all three nodes. The orchestrator/web UI therefore shows
the combined network, while the Rust UI node auto-selects its local managed
shard view.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import IO


ROOT_DIR = Path(__file__).resolve().parents[1]
LOCAL_BIND_HOST = "127.0.0.1"
DEFAULT_NETWORK_ID = "nao_01"
DEFAULT_ORCHESTRATOR_PORT = 50051
DEFAULT_WEB_UI_PORT = 8080
DEFAULT_NODE_PORT_START = 50070
DEFAULT_CONNECT_TIMEOUT = 60
DEFAULT_DISTRIBUTION_TIMEOUT = 300
# Small-primate NAO profile:
# - Keep Webots NAO base non-camera channels (58)
# - Compress both camera event streams to a low-resolution retina
DEFAULT_NAO_CAMERA_RETINA_WIDTH = 8
DEFAULT_NAO_CAMERA_RETINA_HEIGHT = 6
DEFAULT_NAO_SENSORY_COUNT = 58 + 4 * DEFAULT_NAO_CAMERA_RETINA_WIDTH * DEFAULT_NAO_CAMERA_RETINA_HEIGHT
DEFAULT_NAO_OUTPUT_COUNT = 40
DEFAULT_NAO_HIDDEN_LAYERS = 4
DEFAULT_NAO_HIDDEN_PER_LAYER = 64
DEFAULT_NAO_AARNN_DEPTH = 3
DEFAULT_NAO_GROWTH_HEADROOM = 1.6
NAO_CAMERA_EXCLUSION_REGEX = r"^(?!Camera(?:Top|Bottom)[.]).*"

RUNTIME_ENV_DEFAULTS = {
    "NM_REALTIME_IPC": "auto",
    "NM_REALTIME_DISABLE_GROWTH": "auto",
    "NM_REALTIME_DISABLE_MORPHO": "auto",
    "NM_REALTIME_DISABLE_METABOLIC": "auto",
    "NM_REALTIME_DISABLE_PRUNING": "auto",
    "NM_MORPHO_ASYNC": "auto",
    # Keep controller-side IPC defaults aligned with run_webot.sh wrappers.
    "NM_UDS_RECV_TIMEOUT_MS": "150",
    "NM_IPC_TIMEOUT_GRACE_MS": "1500",
    "NM_IPC_TIMEOUT_LOG_INTERVAL_MS": "5000",
    "NM_IPC_UDS_CTRL_BUF_BYTES": "524288",
    "NM_IPC_WINDOW_MIN": "1",
    "NM_IPC_WINDOW_INIT": "1",
    "NM_IPC_WINDOW_MAX": "8",
    "NM_IPC_SEND_BUDGET_MAX": "4",
    # NAO camera-heavy transport should stay on compact AER packets by default.
    "NM_IPC_FORCE_AER": "1",
    "NM_IPC_MAX_RAW_BYTES": "60000",
    "NM_IPC_AER_MAX_PACKET_BYTES": "60000",
    "NM_IPC_UDS_RECV_BUF_BYTES": "262144",
    # Match runtime camera event encoding to generated NAO sensory dimensions.
    "NM_CAMERA_RETINA_WIDTH": str(DEFAULT_NAO_CAMERA_RETINA_WIDTH),
    "NM_CAMERA_RETINA_HEIGHT": str(DEFAULT_NAO_CAMERA_RETINA_HEIGHT),
}
RUNTIME_ENV_PASSTHROUGH = (
    "NM_REALTIME_MORPHO_INTERVAL_MS",
    "NM_REALTIME_METABOLIC_INTERVAL_MS",
    "NM_REALTIME_MORPHO_MAX_SYNAPSES",
    "NM_GRPC_MAX_MESSAGE_BYTES",
    "NM_LOG_PATH",
    "OMP_NUM_THREADS",
    "OMP_PROC_BIND",
    "OMP_PLACES",
)


class LaunchError(RuntimeError):
    pass


@dataclass
class ManagedProcess:
    name: str
    command: list[str]
    log_path: Path
    handle: IO[str]
    proc: subprocess.Popen[str]

    def poll(self) -> int | None:
        return self.proc.poll()

    def is_running(self) -> bool:
        return self.poll() is None

    def terminate(self) -> None:
        if self.proc.poll() is not None:
            return
        try:
            os.killpg(self.proc.pid, signal.SIGTERM)
        except ProcessLookupError:
            return
        except Exception:
            self.proc.terminate()

    def kill(self) -> None:
        if self.proc.poll() is not None:
            return
        try:
            os.killpg(self.proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            return
        except Exception:
            self.proc.kill()

    def close(self) -> None:
        try:
            self.handle.close()
        except Exception:
            pass


class RuntimeSession:
    def __init__(self) -> None:
        self.processes: list[ManagedProcess] = []
        self.temp_paths: list[Path] = []
        self.cleaned = False

    def spawn(
        self,
        *,
        name: str,
        command: list[str],
        env: dict[str, str],
        cwd: Path,
        log_path: Path,
    ) -> ManagedProcess:
        log_path.parent.mkdir(parents=True, exist_ok=True)
        handle = log_path.open("w", encoding="utf-8")
        handle.write(f"$ {shell_join(command)}\n\n")
        handle.flush()
        proc = subprocess.Popen(
            command,
            cwd=str(cwd),
            env=env,
            stdout=handle,
            stderr=subprocess.STDOUT,
            text=True,
            start_new_session=True,
        )
        managed = ManagedProcess(
            name=name,
            command=command,
            log_path=log_path,
            handle=handle,
            proc=proc,
        )
        self.processes.append(managed)
        return managed

    def cleanup(self, *, keep_temp_world: bool = False) -> None:
        if self.cleaned:
            return
        self.cleaned = True
        for process in reversed(self.processes):
            process.terminate()
        deadline = time.time() + 5.0
        for process in reversed(self.processes):
            while process.poll() is None and time.time() < deadline:
                time.sleep(0.1)
            if process.poll() is None:
                process.kill()
        for process in reversed(self.processes):
            process.close()
        if not keep_temp_world:
            for path in self.temp_paths:
                try:
                    path.unlink()
                except FileNotFoundError:
                    pass


def shell_join(parts: list[str]) -> str:
    return " ".join(shlex.quote(part) for part in parts)


def build_runtime_env(overrides: dict[str, str] | None = None) -> dict[str, str]:
    env = os.environ.copy()
    for key, default in RUNTIME_ENV_DEFAULTS.items():
        env.setdefault(key, default)
    for key in RUNTIME_ENV_PASSTHROUGH:
        if key in os.environ:
            env[key] = os.environ[key]
    if overrides:
        env.update(overrides)
    return env


def find_free_port(start_port: int, reserved: set[int]) -> int:
    port = max(1, start_port)
    while port <= 65535:
        if port not in reserved:
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
                sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                try:
                    sock.bind(("127.0.0.1", port))
                except OSError:
                    port += 1
                    continue
                reserved.add(port)
                return port
        port += 1
    raise LaunchError(f"Failed to allocate a free TCP port starting at {start_port}.")


def reserve_port(start_port: int, reserved: set[int]) -> tuple[int, socket.socket]:
    port = max(1, start_port)
    while port <= 65535:
        if port in reserved:
            port += 1
            continue
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            sock.bind((LOCAL_BIND_HOST, port))
        except OSError:
            sock.close()
            port += 1
            continue
        reserved.add(port)
        return port, sock
    raise LaunchError(f"Failed to reserve a free TCP port starting at {start_port}.")


def reserve_specific_port(port: int, reserved: set[int]) -> tuple[int, socket.socket]:
    if port in reserved:
        raise LaunchError(f"TCP port {port} was requested more than once.")
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    try:
        sock.bind((LOCAL_BIND_HOST, port))
    except OSError as exc:
        sock.close()
        raise LaunchError(f"Requested TCP port {port} is not available: {exc}") from exc
    reserved.add(port)
    return port, sock


def release_port_reservation(sock: socket.socket | None) -> None:
    if sock is None:
        return
    try:
        sock.close()
    except OSError:
        pass


def wait_for_tcp_port(
    host: str,
    port: int,
    timeout_s: int,
    *,
    watched: list[ManagedProcess] | None = None,
) -> None:
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        ensure_processes_alive(watched or [], f"waiting for {host}:{port}")
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.settimeout(0.5)
            try:
                sock.connect((host, port))
                return
            except OSError:
                time.sleep(0.2)
    raise LaunchError(f"Timed out waiting for {host}:{port} to accept connections.")


def wait_for_socket(
    socket_path: Path,
    timeout_s: int,
    *,
    watched: list[ManagedProcess] | None = None,
) -> None:
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        ensure_processes_alive(watched or [], f"waiting for socket {socket_path}")
        if socket_path.exists() and socket_path.is_socket():
            return
        time.sleep(0.1)
    raise LaunchError(f"Timed out waiting for IPC socket {socket_path}.")


def http_get_json(url: str, timeout_s: float = 2.5) -> dict:
    request = urllib.request.Request(url)
    with urllib.request.urlopen(request, timeout=timeout_s) as response:
        payload = response.read().decode("utf-8", errors="replace")
    if not payload:
        return {}
    data = json.loads(payload)
    if isinstance(data, dict):
        return data
    raise LaunchError(f"Expected JSON object from {url}, got {type(data).__name__}.")


def wait_for_http_json(
    url: str,
    timeout_s: int,
    *,
    watched: list[ManagedProcess] | None = None,
) -> dict:
    deadline = time.time() + timeout_s
    last_error = ""
    while time.time() < deadline:
        ensure_processes_alive(watched or [], f"waiting for {url}")
        try:
            return http_get_json(url)
        except (LaunchError, urllib.error.URLError, urllib.error.HTTPError, TimeoutError) as exc:
            last_error = str(exc)
            time.sleep(0.3)
    detail = f" Last error: {last_error}" if last_error else ""
    raise LaunchError(f"Timed out waiting for HTTP endpoint {url}.{detail}")


def tail_log(path: Path, *, lines: int = 80) -> str:
    try:
        content = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except FileNotFoundError:
        return "(log file not found)"
    if len(content) <= lines:
        return "\n".join(content)
    return "\n".join(content[-lines:])


def ensure_processes_alive(processes: list[ManagedProcess], stage: str) -> None:
    for process in processes:
        code = process.poll()
        if code is None:
            continue
        log_tail = tail_log(process.log_path)
        raise LaunchError(
            f"Process '{process.name}' exited early while {stage} (code {code}). "
            f"See {process.log_path}.\n\n{log_tail}"
        )


def run_checked(command: list[str], *, cwd: Path, log_path: Path) -> None:
    log_path.parent.mkdir(parents=True, exist_ok=True)
    with log_path.open("w", encoding="utf-8") as handle:
        handle.write(f"$ {shell_join(command)}\n\n")
        handle.flush()
        result = subprocess.run(
            command,
            cwd=str(cwd),
            stdout=handle,
            stderr=subprocess.STDOUT,
            text=True,
            check=False,
        )
    if result.returncode != 0:
        raise LaunchError(
            f"Command failed with exit code {result.returncode}: {shell_join(command)}\n"
            f"See {log_path}.\n\n{tail_log(log_path)}"
        )


def read_json_object(path: Path) -> dict:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise LaunchError(f"JSON file not found: {path}") from exc
    except json.JSONDecodeError as exc:
        raise LaunchError(f"Failed to parse JSON file {path}: {exc}") from exc
    if not isinstance(data, dict):
        raise LaunchError(f"Expected JSON object in {path}, got {type(data).__name__}.")
    return data


def network_counts(path: Path) -> dict[str, int]:
    with path.open("r", encoding="utf-8", errors="ignore") as handle:
        prefix = handle.read(131072)
    keys = (
        "num_sensory_neurons",
        "num_hidden_layers",
        "num_hidden_per_layer_initial",
        "num_output_neurons",
    )
    matches = {
        key: re.search(rf'"{re.escape(key)}"\s*:\s*(\d+)', prefix) for key in keys
    }
    if all(matches.values()):
        return {key: int(matches[key].group(1)) for key in keys}

    data = read_json_object(path)
    net = data.get("net")
    if not isinstance(net, dict):
        raise LaunchError(f"Snapshot JSON does not contain an object-valued 'net' field: {path}")
    return {
        "num_sensory_neurons": int(net.get("num_sensory_neurons") or 0),
        "num_hidden_layers": int(net.get("num_hidden_layers") or 0),
        "num_hidden_per_layer_initial": int(net.get("num_hidden_per_layer_initial") or 0),
        "num_output_neurons": int(net.get("num_output_neurons") or 0),
    }


def aligned_runtime_config(
    *,
    network_path: Path,
    config_path: Path,
    logs_dir: Path,
) -> Path:
    counts = network_counts(network_path)
    config = read_json_object(config_path)
    mismatch = any(int(config.get(key) or 0) != value for key, value in counts.items())
    if not mismatch:
        return config_path

    patched = dict(config)
    patched.update(counts)
    runtime_config = logs_dir / f"{config_path.stem}.runtime.json"
    runtime_config.write_text(json.dumps(patched, indent=2) + "\n", encoding="utf-8")
    return runtime_config


def resolve_runtime_binaries(args: argparse.Namespace, logs_dir: Path) -> tuple[Path, Path]:
    aarnn_bin = ROOT_DIR / "target/release/aarnn_rust"
    web_ui_bin = ROOT_DIR / "target/release/web_ui"
    need_build = args.build or not aarnn_bin.is_file() or not web_ui_bin.is_file()
    if need_build:
        print("Building release binaries...", flush=True)
        run_checked(
            [
                "cargo",
                "build",
                "--release",
                "--bin",
                "aarnn_rust",
                "--bin",
                "web_ui",
                "--all-features",
            ],
            cwd=ROOT_DIR,
            log_path=logs_dir / "cargo_build.log",
        )
    if not aarnn_bin.is_file():
        raise LaunchError(f"Missing runtime binary: {aarnn_bin}")
    if not web_ui_bin.is_file():
        raise LaunchError(f"Missing web UI binary: {web_ui_bin}")
    return aarnn_bin, web_ui_bin


def ensure_controller_binary(args: argparse.Namespace, logs_dir: Path) -> None:
    if args.skip_controller_build or args.no_webots:
        return
    controller_dir = ROOT_DIR / "webots_world/controllers/nao_nn_controller_uds"
    source = controller_dir / "nao_nn_controller_uds.cpp"
    binary = controller_dir / "nao_nn_controller_uds"
    if not source.is_file():
        raise LaunchError(f"Webots controller source not found: {source}")
    rebuild_needed = not binary.is_file() or source.stat().st_mtime > binary.stat().st_mtime
    strings_bin = shutil.which("strings")
    if not rebuild_needed and strings_bin and binary.is_file():
        probe = subprocess.run(
            [strings_bin, str(binary)],
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            check=False,
        )
        if "neuromorphic_demo." in probe.stdout:
            rebuild_needed = True
    if not rebuild_needed:
        return
    if shutil.which("make") is None:
        raise LaunchError("Controller rebuild required but 'make' is not available.")
    print("Building Webots controller...", flush=True)
    run_checked(
        ["make", "-C", str(controller_dir)],
        cwd=ROOT_DIR,
        log_path=logs_dir / "webots_controller_build.log",
    )


def resolve_webots_bin(explicit: str | None) -> Path:
    if explicit:
        path = Path(explicit).expanduser()
        if not path.is_file():
            raise LaunchError(f"Configured Webots binary does not exist: {path}")
        return path
    env_candidates = [
        os.environ.get("WEBOTS_EXECUTABLE"),
        os.environ.get("WEBOTS_BINARY"),
    ]
    for candidate in env_candidates:
        if candidate:
            path = Path(candidate).expanduser()
            if path.is_file():
                return path
    which_path = shutil.which("webots")
    if which_path:
        return Path(which_path)
    webots_home = os.environ.get("WEBOTS_HOME")
    if webots_home:
        candidate = Path(webots_home).expanduser() / "webots"
        if candidate.is_file():
            return candidate
    candidate = Path("/usr/local/webots/webots")
    if candidate.is_file():
        return candidate
    raise LaunchError("Unable to locate a Webots executable. Set --webots-bin or WEBOTS_HOME.")


def create_runtime_world(
    *,
    network_id: str,
    base_world: Path,
    target_world: Path,
) -> None:
    text = base_world.read_text(encoding="utf-8")
    replacements = [
        ('"NM_BRAINS=default"', f'"NM_BRAINS={network_id}"'),
        ('"NM_SENSORS_default=', f'"NM_SENSORS_{network_id}='),
        ('"NM_ACTUATORS_default=', f'"NM_ACTUATORS_{network_id}='),
    ]
    for old, new in replacements:
        if old not in text:
            raise LaunchError(f"Expected token {old!r} was not found in {base_world}")
        text = text.replace(old, new)
    target_world.write_text(text, encoding="utf-8")


def find_node_block_ranges(text: str, node_name: str) -> list[tuple[int, int]]:
    token = f"{node_name} {{"
    ranges: list[tuple[int, int]] = []
    offset = 0
    while True:
        start = text.find(token, offset)
        if start < 0:
            return ranges
        brace_start = text.find("{", start)
        if brace_start < 0:
            raise LaunchError(f"Malformed {node_name} block in world file.")
        depth = 0
        in_string = False
        escaped = False
        end = -1
        for idx in range(brace_start, len(text)):
            ch = text[idx]
            if in_string:
                if escaped:
                    escaped = False
                elif ch == "\\":
                    escaped = True
                elif ch == '"':
                    in_string = False
                continue
            if ch == '"':
                in_string = True
            elif ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    end = idx + 1
                    break
        if end < 0:
            raise LaunchError(f"Unterminated {node_name} block in world file.")
        ranges.append((start, end))
        offset = end


def create_nao_runtime_world(
    *,
    network_id: str,
    base_world: Path,
    target_world: Path,
) -> None:
    text = base_world.read_text(encoding="utf-8")
    nao_blocks = find_node_block_ranges(text, "Nao")
    if not nao_blocks:
        raise LaunchError(f"No Nao blocks were found in {base_world}")
    for start, end in reversed(nao_blocks[1:]):
        text = text[:start] + text[end:]

    old = (
        '  controllerArgs [\n'
        '    "NM_INTERCONNECT=vision->motor:8"\n'
        '    "NM_ACTUATORS_motor=.*Shoulder.*"\n'
        '    "NM_ACTUATORS_vision=Eye.*"\n'
        '    "NM_BRAINS=vision,motor"\n'
        '  ]'
    )
    new = (
        "  controllerArgs [\n"
        f'    "NM_BRAINS={network_id}"\n'
        f'    "NM_SENSORS_{network_id}={NAO_CAMERA_EXCLUSION_REGEX}"\n'
        "  ]"
    )
    if old not in text:
        raise LaunchError(f"Expected NAO controllerArgs block was not found in {base_world}")
    text = text.replace(old, new, 1)
    target_world.write_text(text, encoding="utf-8")


def network_specs_json(network_id: str, network_path: Path, config_path: Path) -> str:
    payload = [
        {
            "network_id": network_id,
            "network_path": str(network_path),
            "config_path": str(config_path),
            "execution_modes": ["distributed", "sharded"],
            "execution_scope": "cluster",
            "desired_shards": 3,
            "autodetect_infrastructure": False,
        }
    ]
    return json.dumps(payload, separators=(",", ":"))


def active_display_available() -> bool:
    return bool(os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY"))


def maybe_wrap_with_xvfb(command: list[str], *, enabled: bool) -> list[str]:
    if not enabled:
        return command
    xvfb_run = shutil.which("xvfb-run")
    if xvfb_run is None:
        raise LaunchError(
            "Rust UI node requires a display. No DISPLAY/WAYLAND_DISPLAY is available and "
            "'xvfb-run' was not found."
        )
    return [xvfb_run, "-a"] + command


def wait_for_cluster_distribution(
    *,
    web_ui_base: str,
    orchestrator_addr: str,
    network_id: str,
    expected_nodes: int,
    timeout_s: int,
    watched: list[ManagedProcess],
) -> dict:
    deadline = time.time() + timeout_s
    last_summary = ""
    while time.time() < deadline:
        ensure_processes_alive(watched, "waiting for cluster distribution")
        url = (
            f"{web_ui_base}/api/status?"
            f"{urllib.parse.urlencode({'addr': orchestrator_addr})}"
        )
        try:
            status = http_get_json(url)
        except (LaunchError, urllib.error.URLError, urllib.error.HTTPError, TimeoutError) as exc:
            last_summary = str(exc)
            time.sleep(0.5)
            continue

        nodes = status.get("nodes") or []
        networks = status.get("networks") or []
        if not isinstance(nodes, list) or not isinstance(networks, list):
            last_summary = "status payload did not contain list-valued nodes/networks fields"
            time.sleep(0.5)
            continue

        target = next(
            (
                network
                for network in networks
                if str(network.get("network_id", "")).strip() == network_id
            ),
            None,
        )
        if target is None:
            last_summary = f"network '{network_id}' not yet visible"
            time.sleep(0.5)
            continue

        dist_entries = target.get("distribution") or []
        if not isinstance(dist_entries, list):
            dist_entries = []
        dist_node_ids = {str(entry.get("node_id", "")).strip() for entry in dist_entries}
        dist_node_ids.discard("")

        nodes_with_network = {
            str(node.get("node_id", "")).strip()
            for node in nodes
            if network_id in (node.get("active_networks") or [])
        }
        nodes_with_network.discard("")

        num_layers = int(target.get("num_layers") or 0)
        layer_cover = {
            int(layer)
            for entry in dist_entries
            for layer in (entry.get("layers") or [])
            if isinstance(layer, int)
        }
        full_cover = num_layers <= 0 or layer_cover == set(range(num_layers))
        has_partial_view = num_layers <= 1 or any(
            len(entry.get("layers") or []) < num_layers for entry in dist_entries
        )

        if (
            len(nodes_with_network) >= expected_nodes
            and len(dist_node_ids) == expected_nodes
            and dist_node_ids.issubset(nodes_with_network)
            and full_cover
            and has_partial_view
        ):
            return status

        distribution_layers = [
            list(entry.get("layers") or [])
            for entry in dist_entries
            if isinstance(entry, dict)
        ]
        last_summary = (
            f"nodes_with_network={len(nodes_with_network)}/{expected_nodes}, "
            f"distribution={len(dist_node_ids)}/{expected_nodes}, "
            f"layers={distribution_layers}, "
            f"layer_cover={sorted(layer_cover) if layer_cover else []}, "
            f"num_layers={num_layers}, "
            f"has_partial_view={has_partial_view}"
        )
        time.sleep(0.5)

    raise LaunchError(
        f"Timed out waiting for orchestrator to shard '{network_id}' across {expected_nodes} nodes. "
        f"Last status: {last_summary}"
    )


def wait_for_webots_connection(
    *,
    log_path: Path,
    network_id: str,
    timeout_s: int,
    watched: list[ManagedProcess],
) -> None:
    deadline = time.time() + timeout_s
    success_line = f"[nao_nn_controller_uds] Brain '{network_id}': Connected."
    while time.time() < deadline:
        ensure_processes_alive(watched, "waiting for Webots controller handshake")
        text = log_path.read_text(encoding="utf-8", errors="replace") if log_path.exists() else ""
        if success_line in text:
            return
        time.sleep(1.0)
    raise LaunchError(
        f"Timed out waiting for Webots to connect brain '{network_id}'. "
        f"See {log_path}.\n\n{tail_log(log_path, lines=120)}"
    )


def ensure_nao_assets(
    args: argparse.Namespace,
    logs_dir: Path,
) -> tuple[Path, Path, Path]:
    network_path = Path(args.network_file).expanduser().resolve()
    config_path = Path(args.config_file).expanduser().resolve()
    world_path = Path(args.world_file).expanduser().resolve()

    template_path = (ROOT_DIR / "network.json").resolve()
    if args.rebuild_network or not network_path.is_file() or not config_path.is_file():
        if not template_path.is_file():
            raise LaunchError(f"Missing NAO template snapshot: {template_path}")
        print("Building NAO network/config artifacts...", flush=True)
        run_checked(
            [
                "python3",
                str(ROOT_DIR / "scripts/build_nao_network_json.py"),
                "--template",
                str(template_path),
                "--output",
                str(network_path),
                "--config-output",
                str(config_path),
                "--expected-sensory",
                str(DEFAULT_NAO_SENSORY_COUNT),
                "--expected-output",
                str(DEFAULT_NAO_OUTPUT_COUNT),
                "--camera-retina-width",
                str(DEFAULT_NAO_CAMERA_RETINA_WIDTH),
                "--camera-retina-height",
                str(DEFAULT_NAO_CAMERA_RETINA_HEIGHT),
                "--hidden-layers",
                str(DEFAULT_NAO_HIDDEN_LAYERS),
                "--hidden-per-layer",
                str(DEFAULT_NAO_HIDDEN_PER_LAYER),
                "--aarnn-depth",
                str(DEFAULT_NAO_AARNN_DEPTH),
                "--growth-headroom",
                str(DEFAULT_NAO_GROWTH_HEADROOM),
            ],
            cwd=ROOT_DIR,
            log_path=logs_dir / "build_nao_network.log",
        )

    missing = [path for path in (network_path, config_path, world_path) if not path.is_file()]
    if missing:
        formatted = "\n".join(f"  - {path}" for path in missing)
        raise LaunchError(f"Required NAO assets are missing:\n{formatted}")
    return network_path, config_path, world_path


def monitor_runtime(
    *,
    session: RuntimeSession,
    webots_proc: ManagedProcess | None,
) -> int:
    try:
        while True:
            for process in session.processes:
                code = process.poll()
                if code is None:
                    continue
                if webots_proc and process is webots_proc:
                    return code
                raise LaunchError(
                    f"Process '{process.name}' exited unexpectedly with code {code}. "
                    f"See {process.log_path}.\n\n{tail_log(process.log_path)}"
                )
            time.sleep(1.0)
    except KeyboardInterrupt:
        return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Run one orchestrator web UI, one Rust UI worker node, and two CLI worker "
            "nodes against a single sharded NAO Webots network."
        )
    )
    parser.add_argument("--build", action="store_true", help="Force a fresh release build first.")
    parser.add_argument(
        "--no-webots",
        action="store_true",
        help="Start the orchestrator and nodes but do not launch Webots.",
    )
    parser.add_argument(
        "--webots-bin",
        default=os.environ.get("WEBOTS_BIN", ""),
        help="Path to the Webots executable. Defaults to auto-detection.",
    )
    parser.add_argument(
        "--webots-mode",
        default=os.environ.get("WEBOTS_MODE", "realtime"),
        choices=("pause", "realtime", "fast"),
        help="Webots execution mode.",
    )
    parser.add_argument(
        "--webots-headless",
        action="store_true",
        help="Launch Webots with --no-rendering --minimize.",
    )
    parser.add_argument(
        "--skip-controller-build",
        action="store_true",
        help="Skip rebuilding the Webots controller binary.",
    )
    parser.add_argument(
        "--network-id",
        default=DEFAULT_NETWORK_ID,
        help="Shared network id to register on the orchestrator and all nodes.",
    )
    parser.add_argument(
        "--network-file",
        default=str(ROOT_DIR / "nao_network.json"),
        help="Path to the NAO snapshot JSON.",
    )
    parser.add_argument(
        "--config-file",
        default=str(ROOT_DIR / "webots_world/configs/config_nao_webots.json"),
        help="Path to the NAO Webots NetworkConfig JSON.",
    )
    parser.add_argument(
        "--world-file",
        default=str(ROOT_DIR / "webots_world/worlds/neuroworld.wbt"),
        help="Base Webots world file used to create the runtime world copy.",
    )
    parser.add_argument(
        "--rebuild-network",
        action="store_true",
        help=(
            "Force rebuilding the NAO snapshot/config pair with the small-primate profile "
            "(58 base sensory + compressed camera event channels)."
        ),
    )
    parser.add_argument(
        "--orchestrator-port",
        type=int,
        default=0,
        help="Explicit orchestrator gRPC port. Default: first free port from 50051.",
    )
    parser.add_argument(
        "--web-ui-port",
        type=int,
        default=0,
        help="Explicit web UI port. Default: first free port from 8080.",
    )
    parser.add_argument(
        "--node-port-start",
        type=int,
        default=DEFAULT_NODE_PORT_START,
        help="First port to try for worker node gRPC listeners.",
    )
    parser.add_argument(
        "--connect-timeout",
        type=int,
        default=int(os.environ.get("WEBOTS_CONNECT_TIMEOUT", DEFAULT_CONNECT_TIMEOUT)),
        help="Seconds to wait for ports, sockets, and Webots handshakes.",
    )
    parser.add_argument(
        "--distribution-timeout",
        type=int,
        default=int(
            os.environ.get(
                "WEBOTS_CLUSTER_DISTRIBUTION_TIMEOUT",
                DEFAULT_DISTRIBUTION_TIMEOUT,
            )
        ),
        help="Seconds to wait for the orchestrator to shard the network across all nodes.",
    )
    parser.add_argument(
        "--logs-dir",
        default="",
        help="Directory for runtime logs. Default: logs/nao_combo_webots-<timestamp>",
    )
    parser.add_argument(
        "--rust-ui-hidden",
        action="store_true",
        help="Hide the Rust UI window after launch.",
    )
    parser.add_argument(
        "--keep-runtime-world",
        action="store_true",
        help="Keep the generated runtime world file on exit.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if not args.no_webots and not active_display_available():
        args.webots_headless = True

    timestamp = time.strftime("%Y%m%d-%H%M%S")
    logs_dir = (
        Path(args.logs_dir).expanduser().resolve()
        if args.logs_dir
        else (ROOT_DIR / "logs" / f"nao_combo_webots-{timestamp}").resolve()
    )
    logs_dir.mkdir(parents=True, exist_ok=True)

    network_path, config_path, base_world = ensure_nao_assets(args, logs_dir)
    runtime_config_path = aligned_runtime_config(
        network_path=network_path,
        config_path=config_path,
        logs_dir=logs_dir,
    )

    session = RuntimeSession()
    keep_temp_world = args.keep_runtime_world
    port_reservations: list[socket.socket] = []

    def _signal_handler(signum: int, _frame: object) -> None:
        raise KeyboardInterrupt()

    for sig in (signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, _signal_handler)

    try:
        aarnn_bin, web_ui_bin = resolve_runtime_binaries(args, logs_dir)
        ensure_controller_binary(args, logs_dir)

        reserved_ports: set[int] = set()
        orchestrator_port, orchestrator_reservation = (
            reserve_specific_port(args.orchestrator_port, reserved_ports)
            if args.orchestrator_port > 0
            else reserve_port(DEFAULT_ORCHESTRATOR_PORT, reserved_ports)
        )
        port_reservations.append(orchestrator_reservation)
        web_ui_port, web_ui_reservation = (
            reserve_specific_port(args.web_ui_port, reserved_ports)
            if args.web_ui_port > 0
            else reserve_port(DEFAULT_WEB_UI_PORT, reserved_ports)
        )
        port_reservations.append(web_ui_reservation)
        rust_node_port, rust_reservation = reserve_port(args.node_port_start, reserved_ports)
        port_reservations.append(rust_reservation)
        cli_node_1_port, cli1_reservation = reserve_port(rust_node_port + 7, reserved_ports)
        port_reservations.append(cli1_reservation)
        cli_node_2_port, cli2_reservation = reserve_port(cli_node_1_port + 7, reserved_ports)
        port_reservations.append(cli2_reservation)

        orchestrator_addr = f"http://127.0.0.1:{orchestrator_port}"
        web_ui_base = f"http://127.0.0.1:{web_ui_port}"
        socket_path = Path(os.environ.get("HOME", "/tmp")) / f"aarnn_rust.{args.network_id}.nn"
        if socket_path.exists():
            socket_path.unlink()

        runtime_world = base_world.parent / f"{base_world.stem}.combo-{timestamp}.wbt"
        create_nao_runtime_world(
            network_id=args.network_id,
            base_world=base_world,
            target_world=runtime_world,
        )
        session.temp_paths.append(runtime_world)

        specs_json = network_specs_json(args.network_id, network_path, runtime_config_path)

        common_execution_args = [
            "--execution-mode",
            "distributed,sharded",
            "--execution-scope",
            "cluster",
            "--execution-desired-shards",
            "3",
        ]

        orchestrator_env = build_runtime_env(
            {
                "NM_ORCHESTRATOR_NETWORK_SPECS": specs_json,
                "NM_DISTRIBUTE_STARTUP_SNAPSHOT": "1",
                "NM_DISTRIBUTED_AUTOSTART": "1",
            }
        )
        release_port_reservation(orchestrator_reservation)
        orchestrator = session.spawn(
            name="orchestrator",
            command=[
                str(aarnn_bin),
                "--orchestrator",
                "--brain-id",
                "orchestrator",
                "--grpc-addr",
                f"{LOCAL_BIND_HOST}:{orchestrator_port}",
                *common_execution_args,
            ],
            env=orchestrator_env,
            cwd=ROOT_DIR,
            log_path=logs_dir / "orchestrator.log",
        )
        print(f"Started orchestrator on {orchestrator_addr}", flush=True)
        wait_for_tcp_port(
            LOCAL_BIND_HOST,
            orchestrator_port,
            args.connect_timeout,
            watched=[orchestrator],
        )

        release_port_reservation(web_ui_reservation)
        web_ui = session.spawn(
            name="web_ui",
            command=[
                str(web_ui_bin),
                "--listen",
                f"{LOCAL_BIND_HOST}:{web_ui_port}",
                "--orchestrator",
                orchestrator_addr,
                "--auth-mode",
                "none",
            ],
            env=build_runtime_env(),
            cwd=ROOT_DIR,
            log_path=logs_dir / "web_ui.log",
        )
        wait_for_http_json(
            f"{web_ui_base}/api/config",
            args.connect_timeout,
            watched=[orchestrator, web_ui],
        )
        print(f"Started web UI on {web_ui_base}", flush=True)

        node_common = [
            "--node",
            "--brain-id",
            args.network_id,
            "--orchestrator-addr",
            orchestrator_addr,
            "--config",
            str(runtime_config_path),
            "--network",
            str(network_path),
            *common_execution_args,
        ]

        rust_ui_cmd = [
            str(aarnn_bin),
            *node_common,
            "--grpc-addr",
            f"{LOCAL_BIND_HOST}:{rust_node_port}",
            "--ui",
            "--ipc",
        ]
        rust_ui_env = build_runtime_env(
            {
                "NM_PRELOAD_NODE_NETWORK": "1",
                "NM_CAPACITY_MULTIPLIER": "1.35",
            }
        )
        if args.rust_ui_hidden:
            rust_ui_env["NM_UI_HIDDEN"] = "1"
        rust_ui_cmd = maybe_wrap_with_xvfb(
            rust_ui_cmd,
            enabled=not active_display_available(),
        )
        release_port_reservation(rust_reservation)
        rust_ui_node = session.spawn(
            name="rust_ui_node",
            command=rust_ui_cmd,
            env=rust_ui_env,
            cwd=ROOT_DIR,
            log_path=logs_dir / "node_rust_ui.log",
        )

        release_port_reservation(cli1_reservation)
        cli_node_1 = session.spawn(
            name="cli_node_1",
            command=[
                str(aarnn_bin),
                *node_common,
                "--grpc-addr",
                f"{LOCAL_BIND_HOST}:{cli_node_1_port}",
            ],
            env=build_runtime_env(
                {
                    "NM_PRELOAD_NODE_NETWORK": "1",
                    "NM_CAPACITY_MULTIPLIER": "0.85",
                }
            ),
            cwd=ROOT_DIR,
            log_path=logs_dir / "node_cli_1.log",
        )
        release_port_reservation(cli2_reservation)
        cli_node_2 = session.spawn(
            name="cli_node_2",
            command=[
                str(aarnn_bin),
                *node_common,
                "--grpc-addr",
                f"{LOCAL_BIND_HOST}:{cli_node_2_port}",
            ],
            env=build_runtime_env(
                {
                    "NM_PRELOAD_NODE_NETWORK": "1",
                    "NM_CAPACITY_MULTIPLIER": "0.85",
                }
            ),
            cwd=ROOT_DIR,
            log_path=logs_dir / "node_cli_2.log",
        )
        critical = [orchestrator, web_ui, rust_ui_node, cli_node_1, cli_node_2]
        wait_for_tcp_port(
            LOCAL_BIND_HOST,
            rust_node_port,
            args.connect_timeout,
            watched=critical,
        )
        wait_for_tcp_port(
            LOCAL_BIND_HOST,
            cli_node_1_port,
            args.connect_timeout,
            watched=critical,
        )
        wait_for_tcp_port(
            LOCAL_BIND_HOST,
            cli_node_2_port,
            args.connect_timeout,
            watched=critical,
        )
        wait_for_socket(socket_path, args.connect_timeout, watched=critical)

        status = wait_for_cluster_distribution(
            web_ui_base=web_ui_base,
            orchestrator_addr=orchestrator_addr,
            network_id=args.network_id,
            expected_nodes=3,
            timeout_s=args.distribution_timeout,
            watched=critical,
        )
        network_status = next(
            network
            for network in status.get("networks", [])
            if str(network.get("network_id", "")).strip() == args.network_id
        )
        distribution = network_status.get("distribution") or []
        print(
            f"Cluster distribution ready: {args.network_id} -> {len(distribution)} nodes",
            flush=True,
        )

        webots_proc: ManagedProcess | None = None
        if not args.no_webots:
            webots_bin = resolve_webots_bin(args.webots_bin or None)
            webots_cmd = [
                str(webots_bin),
                "--batch",
                "--stdout",
                "--stderr",
                f"--mode={args.webots_mode}",
            ]
            if args.webots_headless:
                webots_cmd.extend(["--no-rendering", "--minimize"])
            webots_cmd.append(str(runtime_world))
            webots_proc = session.spawn(
                name="webots",
                command=webots_cmd,
                env=build_runtime_env(),
                cwd=ROOT_DIR,
                log_path=logs_dir / "webots.log",
            )
            wait_for_webots_connection(
                log_path=webots_proc.log_path,
                network_id=args.network_id,
                timeout_s=args.connect_timeout,
                watched=critical + [webots_proc],
            )
            print("Webots controller connected to the Rust UI node.", flush=True)

        print("", flush=True)
        print("Runtime ready:", flush=True)
        print(f"  logs: {logs_dir}", flush=True)
        print(f"  web UI: {web_ui_base}", flush=True)
        print(f"  orchestrator: {orchestrator_addr}", flush=True)
        print(f"  shared network: {args.network_id}", flush=True)
        print(f"  network snapshot: {network_path}", flush=True)
        print(f"  runtime config: {runtime_config_path}", flush=True)
        print(f"  rust UI node gRPC: 127.0.0.1:{rust_node_port}", flush=True)
        print(f"  cli node 1 gRPC: 127.0.0.1:{cli_node_1_port}", flush=True)
        print(f"  cli node 2 gRPC: 127.0.0.1:{cli_node_2_port}", flush=True)
        print(f"  rust UI IPC socket: {socket_path}", flush=True)
        if webots_proc:
            print(f"  runtime world: {runtime_world}", flush=True)
        print("", flush=True)
        print(
            "The orchestrator web UI should show the combined NAO network. "
            "The Rust UI node should auto-select its local managed shard view.",
            flush=True,
        )
        if args.no_webots:
            print("Webots launch disabled. Press Ctrl+C to stop the cluster.", flush=True)
        else:
            print("Press Ctrl+C to stop everything.", flush=True)

        return monitor_runtime(session=session, webots_proc=webots_proc)
    except LaunchError as exc:
        print(str(exc), file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        return 0
    finally:
        for reservation in port_reservations:
            release_port_reservation(reservation)
        for sig in (signal.SIGINT, signal.SIGTERM):
            signal.signal(sig, signal.SIG_IGN)
        session.cleanup(keep_temp_world=keep_temp_world)


if __name__ == "__main__":
    raise SystemExit(main())
