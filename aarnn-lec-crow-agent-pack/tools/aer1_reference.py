"""Strict AER1 fixture codec, not a production AARNN bridge or governed envelope."""
from __future__ import annotations
import struct

MAGIC = b'AER1'
MAX_PAYLOAD = 1024 * 1024  # local fixture-tool budget, not a negotiated wire field

def varint(value):
    if not isinstance(value, int) or not 0 <= value < 2**64:
        raise ValueError('Unsigned 64-bit varint required')
    result = bytearray()
    while value >= 128:
        result.append((value & 127) | 128)
        value >>= 7
    result.append(value)
    return bytes(result)

def read_varint(payload, offset):
    result = 0
    for number in range(10):
        if offset >= len(payload):
            raise ValueError('Truncated varint')
        byte = payload[offset]
        offset += 1
        if number == 9 and byte > 1:
            raise ValueError('Varint overflow')
        result |= (byte & 127) << (7 * number)
        if byte < 128:
            return result, offset
    raise ValueError('Varint overflow')

def encode(events, empty_timestamp_us=0):
    events = sorted(events, key=lambda e:e[0])
    base = events[0][0] if events else empty_timestamp_us
    if not 0 <= base < 2**64:
        raise ValueError('Invalid timestamp')
    result = bytearray(MAGIC + struct.pack('<Q', base))
    previous = base
    for timestamp, address, value in events:
        if not 0 <= address < 2**32 or not 0 <= value < 256 or not previous <= timestamp < 2**64:
            raise ValueError('Event outside AER1 field bounds')
        result += varint(timestamp-previous) + varint(address) + varint(value)
        previous = timestamp
    if len(result) > MAX_PAYLOAD:
        raise ValueError('Fixture payload budget exceeded')
    return bytes(result)

def decode(payload, address_base=None, address_count=None):
    if len(payload) < 12 or len(payload) > MAX_PAYLOAD or payload[:4] != MAGIC:
        raise ValueError('Invalid AER1 header or size')
    timestamp = struct.unpack_from('<Q', payload, 4)[0]
    offset = 12
    events = []
    while offset < len(payload):
        delta, offset = read_varint(payload, offset)
        address, offset = read_varint(payload, offset)
        value, offset = read_varint(payload, offset)
        timestamp += delta
        if timestamp >= 2**64 or address >= 2**32 or value >= 256:
            raise ValueError('Overflow in AER1 event')
        if address_base is not None and not address_base <= address < address_base + address_count:
            raise ValueError('Address outside binding window')
        events.append((timestamp,address,value))
    return events

def frame(payload):
    if not 0 < len(payload) <= MAX_PAYLOAD:
        raise ValueError('Invalid framed payload length')
    return struct.pack('<I',len(payload)) + payload
