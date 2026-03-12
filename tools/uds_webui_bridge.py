#!/usr/bin/env python3
"""
Bridge local Webots UDS frames to a remote distributed cluster via web_ui HTTP APIs.

Protocol expectations (matches nao_nn_controller_uds):
- Handshake datagram: JSON object with optional "s_names" and "o_names".
- Frame datagram: float32 little-endian array: [dt_ms, sensory_0, sensory_1, ...].
- Reply datagram: float32 little-endian actuator vector with O values.
"""

from __future__ import annotations

import argparse
import json
import os
import socket
import struct
import time
import urllib.error
import urllib.parse
import urllib.request
from typing import Any


def _as_bool_env(value: str | None, default: bool) -> bool:
    if value is None:
        return default
    return value.lower() in {"1", "true", "yes", "on"}


class WebUiBridge:
    def __init__(
        self,
        socket_path: str,
        web_ui_url: str,
        orchestrator_addr: str,
        network_id: str,
        threshold: float,
        default_s: int,
        default_o: int,
        http_timeout: float,
        status_refresh_secs: float,
        verbose: bool,
    ) -> None:
        self.socket_path = socket_path
        self.web_ui_url = web_ui_url.rstrip("/")
        self.orchestrator_addr = orchestrator_addr
        self.network_id = network_id
        self.threshold = threshold
        self.http_timeout = max(0.05, http_timeout)
        self.status_refresh_secs = max(0.5, status_refresh_secs)
        self.verbose = verbose

        self.s_count = max(1, default_s)
        self.o_count = max(1, default_o)
        self.step_index = 0
        self.output_node_id: str | None = None
        self.last_status_refresh = 0.0
        self.last_error_log = 0.0

        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        self.sock.settimeout(0.2)

    def _log(self, msg: str) -> None:
        if self.verbose:
            print(f"[uds_webui_bridge:{self.network_id}] {msg}", flush=True)

    def _log_error_throttled(self, msg: str) -> None:
        now = time.time()
        if now - self.last_error_log >= 2.0:
            print(f"[uds_webui_bridge:{self.network_id}] {msg}", flush=True)
            self.last_error_log = now

    def _json_post(self, path: str, payload: dict[str, Any]) -> dict[str, Any]:
        body = json.dumps(payload).encode("utf-8")
        req = urllib.request.Request(
            f"{self.web_ui_url}{path}",
            data=body,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        with urllib.request.urlopen(req, timeout=self.http_timeout) as resp:
            raw = resp.read().decode("utf-8", errors="replace")
            return json.loads(raw) if raw else {}

    def _json_get(self, path: str, params: dict[str, Any]) -> dict[str, Any]:
        query_params = {k: str(v) for k, v in params.items() if v is not None}
        query = urllib.parse.urlencode(query_params)
        url = f"{self.web_ui_url}{path}"
        if query:
            url = f"{url}?{query}"
        req = urllib.request.Request(url, method="GET")
        with urllib.request.urlopen(req, timeout=self.http_timeout) as resp:
            raw = resp.read().decode("utf-8", errors="replace")
            return json.loads(raw) if raw else {}

    def _refresh_output_owner(self) -> None:
        now = time.time()
        if now - self.last_status_refresh < self.status_refresh_secs:
            return
        self.last_status_refresh = now
        try:
            status = self._json_get("/api/status", {"addr": self.orchestrator_addr})
            networks = status.get("networks", [])
            best_node = None
            best_layer = -1
            for net in networks:
                if not isinstance(net, dict):
                    continue
                if str(net.get("network_id", "")) != self.network_id:
                    continue
                distribution = net.get("distribution", [])
                if not isinstance(distribution, list):
                    continue
                for entry in distribution:
                    if not isinstance(entry, dict):
                        continue
                    node_id = entry.get("node_id")
                    layers = entry.get("layers", [])
                    if not node_id or not isinstance(layers, list) or not layers:
                        continue
                    try:
                        max_layer = max(int(x) for x in layers)
                    except Exception:
                        continue
                    if max_layer > best_layer:
                        best_layer = max_layer
                        best_node = str(node_id)
            if best_node and best_node != self.output_node_id:
                self.output_node_id = best_node
                self._log(f"output owner node set to '{best_node}' (max layer={best_layer})")
        except Exception as exc:
            self._log_error_throttled(f"status refresh failed: {exc}")

    def _handle_handshake(self, msg: bytes) -> bool:
        try:
            obj = json.loads(msg.decode("utf-8"))
        except Exception:
            return False
        if not isinstance(obj, dict):
            return False
        s_names = obj.get("s_names")
        o_names = obj.get("o_names")
        if isinstance(s_names, list) and s_names:
            self.s_count = len(s_names)
        if isinstance(o_names, list) and o_names:
            self.o_count = len(o_names)
        self._log(f"handshake accepted S={self.s_count} O={self.o_count}")
        return True

    def _decode_frame(self, msg: bytes) -> list[float] | None:
        if len(msg) < 4 or len(msg) % 4 != 0:
            return None
        count = len(msg) // 4
        try:
            values = struct.unpack(f"<{count}f", msg)
        except struct.error:
            return None
        return list(values)

    def _indices_from_sensory(self, sensory: list[float]) -> list[int]:
        return [idx for idx, value in enumerate(sensory) if value >= self.threshold]

    def _fetch_output_indices(self) -> list[int]:
        params: dict[str, Any] = {
            "addr": self.orchestrator_addr,
            "network_id": self.network_id,
        }
        if self.output_node_id:
            params["node_id"] = self.output_node_id
        payload = self._json_get("/api/activity", params)
        output = payload.get("output", {})
        if isinstance(output, dict):
            indices = output.get("indices", [])
            if isinstance(indices, list):
                out = []
                for idx in indices:
                    try:
                        out.append(int(idx))
                    except Exception:
                        continue
                return out
        return []

    def _send_injection(self, spike_indices: list[int]) -> None:
        # web_ui requires either spike_indices or aer_payload_hex; skip if empty.
        if not spike_indices:
            return
        payload = {
            "addr": self.orchestrator_addr,
            "network_id": self.network_id,
            "step_index": self.step_index,
            "spike_indices": spike_indices,
        }
        self._json_post("/api/aer/inject", payload)

    def _build_reply(self, output_indices: list[int]) -> bytes:
        out = [0.0] * self.o_count
        for idx in output_indices:
            if 0 <= idx < self.o_count:
                out[idx] = 1.0
        return struct.pack(f"<{self.o_count}f", *out)

    def run(self) -> None:
        os.makedirs(os.path.dirname(self.socket_path) or ".", exist_ok=True)
        try:
            os.unlink(self.socket_path)
        except FileNotFoundError:
            pass
        except OSError as exc:
            raise SystemExit(f"failed to remove stale socket '{self.socket_path}': {exc}") from exc

        self.sock.bind(self.socket_path)
        self._log(
            "listening on "
            + f"{self.socket_path} -> {self.web_ui_url} (orchestrator={self.orchestrator_addr})"
        )

        while True:
            try:
                msg, peer = self.sock.recvfrom(65536)
            except socket.timeout:
                continue
            except KeyboardInterrupt:
                break
            except Exception as exc:
                self._log_error_throttled(f"recv failed: {exc}")
                continue

            if self._handle_handshake(msg):
                continue

            frame = self._decode_frame(msg)
            if frame is None:
                self._log_error_throttled(f"ignoring malformed frame ({len(msg)} bytes)")
                continue

            sensory = frame[1:] if len(frame) > 1 else []
            if self.s_count > 0 and len(sensory) != self.s_count:
                if len(sensory) > self.s_count:
                    sensory = sensory[: self.s_count]
                else:
                    sensory = sensory + [0.0] * (self.s_count - len(sensory))

            self._refresh_output_owner()

            try:
                spikes = self._indices_from_sensory(sensory)
                self._send_injection(spikes)
                out_indices = self._fetch_output_indices()
                reply = self._build_reply(out_indices)
            except urllib.error.HTTPError as exc:
                self._log_error_throttled(f"http error: {exc}")
                reply = self._build_reply([])
            except urllib.error.URLError as exc:
                self._log_error_throttled(f"url error: {exc}")
                reply = self._build_reply([])
            except Exception as exc:
                self._log_error_throttled(f"bridge step failed: {exc}")
                reply = self._build_reply([])

            try:
                self.sock.sendto(reply, peer)
            except Exception as exc:
                self._log_error_throttled(f"send failed: {exc}")

            self.step_index += 1

        try:
            self.sock.close()
        finally:
            try:
                os.unlink(self.socket_path)
            except OSError:
                pass


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Bridge local UDS controller frames to web_ui APIs.")
    parser.add_argument("--socket", required=True, help="Local Unix datagram socket path to bind.")
    parser.add_argument(
        "--web-ui-url",
        required=True,
        help="Base web_ui URL, e.g. http://192.168.1.72:8080",
    )
    parser.add_argument(
        "--orchestrator",
        required=True,
        help="Orchestrator gRPC address, e.g. http://192.168.1.72:50051",
    )
    parser.add_argument("--network-id", default="default", help="Target cluster network ID.")
    parser.add_argument("--threshold", type=float, default=0.5, help="Sensory spike threshold.")
    parser.add_argument("--default-s", type=int, default=20, help="Fallback sensory width.")
    parser.add_argument("--default-o", type=int, default=96, help="Fallback output width.")
    parser.add_argument(
        "--http-timeout",
        type=float,
        default=0.8,
        help="HTTP timeout in seconds for web_ui calls.",
    )
    parser.add_argument(
        "--status-refresh",
        type=float,
        default=2.0,
        help="Seconds between status refreshes for output-node tracking.",
    )
    parser.add_argument(
        "--verbose",
        action="store_true",
        default=_as_bool_env(os.environ.get("NM_UDS_WEBUI_BRIDGE_VERBOSE"), False),
        help="Enable verbose bridge logs.",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    bridge = WebUiBridge(
        socket_path=args.socket,
        web_ui_url=args.web_ui_url,
        orchestrator_addr=args.orchestrator,
        network_id=args.network_id,
        threshold=args.threshold,
        default_s=args.default_s,
        default_o=args.default_o,
        http_timeout=args.http_timeout,
        status_refresh_secs=args.status_refresh,
        verbose=args.verbose,
    )
    bridge.run()


if __name__ == "__main__":
    main()

