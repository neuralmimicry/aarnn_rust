#!/usr/bin/env python3
"""Bridge one length-prefixed TCP brain client to one AARNN IPC socket.

The distributed runtime's IPC endpoint deliberately remains a Unix datagram
protocol.  Unreal and Unity use the existing TCP framing, so this process
translates only the transport boundary and keeps one isolated IPC peer per
TCP listener.  AER input is forwarded unchanged; the distributed endpoint
returns bounded raw output floats, which are converted back to AER for AER
clients.
"""

from __future__ import annotations

import argparse
import json
import os
import socket
import struct
import signal
import sys
import tempfile
from pathlib import Path


MAX_FRAME_BYTES = 4 * 1024 * 1024
MAX_DATAGRAM_BYTES = 4 * 1024 * 1024
AER_MAGIC = b"AER1"


class BridgeError(Exception):
    pass


def read_exact(stream: socket.socket, size: int) -> bytes:
    chunks = bytearray()
    while len(chunks) < size:
        part = stream.recv(size - len(chunks))
        if not part:
            raise EOFError("TCP peer disconnected")
        chunks.extend(part)
    return bytes(chunks)


def read_frame(stream: socket.socket) -> bytes:
    header = read_exact(stream, 4)
    size = struct.unpack("<I", header)[0]
    if size == 0 or size > MAX_FRAME_BYTES:
        raise BridgeError(f"invalid TCP frame length {size}")
    return read_exact(stream, size)


def write_frame(stream: socket.socket, payload: bytes) -> None:
    if not payload or len(payload) > MAX_FRAME_BYTES:
        raise BridgeError(f"invalid TCP response length {len(payload)}")
    stream.sendall(struct.pack("<I", len(payload)) + payload)


def varint(value: int) -> bytes:
    output = bytearray()
    while value >= 0x80:
        output.append((value & 0x7F) | 0x80)
        value >>= 7
    output.append(value)
    return bytes(output)


def aer_frame(timestamp_us: int, output_base: int, values: list[float], threshold: float) -> bytes:
    events = [index for index, value in enumerate(values) if value > threshold]
    result = bytearray(AER_MAGIC)
    result.extend(struct.pack("<Q", timestamp_us))
    for index in events:
        result.extend(varint(0))
        result.extend(varint(output_base + index))
        result.extend(varint(1))
    # The valid AER1 header is required even when a frame has no output spikes.
    return bytes(result)


def aer_timestamp(payload: bytes) -> int:
    if len(payload) >= 12 and payload[:4] == AER_MAGIC:
        return struct.unpack_from("<Q", payload, 4)[0]
    return 0


def handshake_dimensions(payload: bytes, sensory: int, output: int) -> tuple[int, int, float]:
    try:
        document = json.loads(payload.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError):
        return sensory, output, 1.0
    s_names = document.get("s_names") or []
    o_names = document.get("o_names") or []
    requested_s = len(s_names) or document.get("sensory") or document.get("expected_s") or sensory
    requested_o = len(o_names) or document.get("output") or document.get("expected_o") or output
    dt_ms = document.get("dt_ms", 1.0)
    if not isinstance(dt_ms, (int, float)) or not (0.0 < float(dt_ms) <= 1000.0):
        dt_ms = 1.0
    return max(1, int(requested_s)), max(1, int(requested_o)), float(dt_ms)


def output_values(payload: bytes, output: int) -> list[float] | None:
    if payload.startswith(b"{"):
        return None
    if len(payload) != output * 4:
        raise BridgeError(f"IPC output length {len(payload)} does not match O={output}")
    return list(struct.unpack(f"<{output}f", payload))


def parse_host_port(value: str) -> tuple[str, int]:
    if value.startswith("["):
        host, separator, port = value[1:].partition("]:")
    else:
        host, separator, port = value.rpartition(":")
    if not separator or not host or not port.isdigit():
        raise argparse.ArgumentTypeError(f"expected HOST:PORT, got {value!r}")
    number = int(port)
    if not 1 <= number <= 65535:
        raise argparse.ArgumentTypeError(f"port out of range: {number}")
    return host, number


class Bridge:
    def __init__(self, args: argparse.Namespace):
        self.listen_host, self.listen_port = parse_host_port(args.listen)
        self.ipc_path = str(Path(args.ipc))
        self.sensory = max(1, args.sensory)
        self.output = max(1, args.output)
        self.aer_output_base = args.aer_output_base
        self.aer_threshold = args.aer_threshold
        self.timeout = args.timeout
        self.stop = False

    def run(self) -> None:
        listener = socket.socket(socket.AF_INET6 if ":" in self.listen_host else socket.AF_INET, socket.SOCK_STREAM)
        if listener.family == socket.AF_INET6:
            listener.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 0)
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind((self.listen_host, self.listen_port))
        listener.listen(1)
        listener.settimeout(1.0)
        print(
            f"[tcp_aer_ipc_bridge] listening on {self.listen_host}:{self.listen_port} "
            f"-> {self.ipc_path}",
            flush=True,
        )
        while not self.stop:
            try:
                stream, peer = listener.accept()
            except socket.timeout:
                continue
            try:
                print(f"[tcp_aer_ipc_bridge] client connected: {peer}", flush=True)
                self.handle_client(stream)
            except (BridgeError, EOFError, OSError) as error:
                print(f"[tcp_aer_ipc_bridge] client stopped: {error}", file=sys.stderr, flush=True)
            finally:
                stream.close()
        listener.close()

    def handle_client(self, stream: socket.socket) -> None:
        stream.settimeout(self.timeout)
        local_dir = tempfile.mkdtemp(prefix="aarnn-tcp-bridge-")
        local_path = os.path.join(local_dir, "peer.sock")
        ipc = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        try:
            ipc.bind(local_path)
            ipc.settimeout(self.timeout)
            sensory, output, dt_ms = self.sensory, self.output, 1.0
            while not self.stop:
                payload = read_frame(stream)
                if payload.startswith(b"{"):
                    sensory, output, dt_ms = handshake_dimensions(payload, sensory, output)

                if len(payload) > MAX_DATAGRAM_BYTES:
                    raise BridgeError("payload exceeds IPC datagram bound")
                ipc.sendto(payload, self.ipc_path)
                response, _ = ipc.recvfrom(MAX_DATAGRAM_BYTES)
                if response.startswith(b"{"):
                    try:
                        hint = json.loads(response.decode("utf-8"))
                        sensory = max(1, int(hint.get("expected_s", sensory)))
                        output = max(1, int(hint.get("expected_o", output)))
                    except (ValueError, TypeError, UnicodeDecodeError, json.JSONDecodeError) as error:
                        raise BridgeError(f"invalid IPC size hint: {error}") from error
                    write_frame(stream, response)
                    continue

                values = output_values(response, output)
                if values is None:
                    raise BridgeError("unexpected non-JSON IPC response")
                if payload.startswith(AER_MAGIC):
                    # The IPC response does not carry a timestamp. Advance from
                    # the input logical timestamp by the negotiated frame dt;
                    # never use wall-clock arrival time for biological time.
                    timestamp = aer_timestamp(payload) + max(1, round(dt_ms * 1000.0))
                    response = aer_frame(timestamp, self.aer_output_base, values, self.aer_threshold)
                write_frame(stream, response)
        finally:
            ipc.close()
            try:
                os.unlink(local_path)
            except FileNotFoundError:
                pass
            os.rmdir(local_dir)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--listen", required=True, help="TCP HOST:PORT")
    parser.add_argument("--ipc", required=True, help="distributed brain Unix datagram socket")
    parser.add_argument("--sensory", type=int, default=1)
    parser.add_argument("--output", type=int, default=1)
    parser.add_argument("--aer-output-base", type=int, default=16384)
    parser.add_argument("--aer-threshold", type=float, default=0.5)
    parser.add_argument("--timeout", type=float, default=300.0)
    args = parser.parse_args()
    bridge = Bridge(args)

    def stop(_signum: int, _frame: object) -> None:
        bridge.stop = True

    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)
    try:
        bridge.run()
    except OSError as error:
        print(f"[tcp_aer_ipc_bridge] fatal: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
