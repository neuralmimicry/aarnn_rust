"""Small legacy AER1/TCP codec retained by the NAO reference proxy.

The simulator TCP-to-IPC bridge is implemented by the Rust binary. These
helpers remain Python because ``nao_social_server.py`` is itself a Python-only
reference HTTP/session adapter and uses the same compatibility wire format.
"""

from __future__ import annotations

import socket
import struct


AER_MAGIC = b"AER1"
MAX_FRAME_BYTES = 4 * 1024 * 1024


def read_exact(stream: socket.socket, size: int) -> bytes:
    chunks = bytearray()
    while len(chunks) < size:
        part = stream.recv(size - len(chunks))
        if not part:
            raise EOFError("TCP peer disconnected")
        chunks.extend(part)
    return bytes(chunks)


def write_frame(stream: socket.socket, payload: bytes) -> None:
    if not payload or len(payload) > MAX_FRAME_BYTES:
        raise ValueError(f"invalid TCP response length {len(payload)}")
    stream.sendall(struct.pack("<I", len(payload)) + payload)


def _varint(value: int) -> bytes:
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
        result.extend(_varint(0))
        result.extend(_varint(output_base + index))
        result.extend(_varint(1))
    return bytes(result)
