"""Bounded asynchronous AER1 reference adapter for simulated bodies only.

This does not implement AARNN-AER/1 leases or FPAA UDP. A binding pins the
model/ordered IO before connecting; existing networks must never be resized.
"""
from __future__ import annotations
import asyncio
from dataclasses import dataclass
import hashlib
import json
import math
import struct
import threading
import queue
from aer_legacy_codec import aer_frame, MAX_FRAME_BYTES


@dataclass(frozen=True)
class Binding:
    instance_id: str
    brain_id: str
    brain_generation: int
    binding_generation: int
    model_digest: str
    io_digest: str
    reset_epoch: int = 0
    protocol: str = 'AER1-reference'

    def validate(self, model, brain_manifest):
        if not self.instance_id or not self.brain_id or self.protocol != 'AER1-reference':
            raise ValueError('explicit reference instance/brain/protocol required')
        if self.model_digest != model['digest'] or self.io_digest != model['io']['digest']:
            raise ValueError('model/IO digest mismatch')
        if any(brain_manifest.get(k) != getattr(self, k) for k in ('brain_id','brain_generation','model_digest','io_digest')):
            raise ValueError('brain manifest mismatch; existing networks are never resized')


def decode(payload, base, count):
    if len(payload) < 12 or len(payload) > MAX_FRAME_BYTES or payload[:4] != b'AER1':
        raise ValueError('invalid AER1 payload')
    timestamp, = struct.unpack_from('<Q', payload, 4)
    pos = 12
    values = [0] * count
    events = 0
    def uint(bits):
        nonlocal pos
        value = 0
        for shift in range(0, bits, 7):
            if pos >= len(payload): raise ValueError('truncated varint')
            byte = payload[pos]; pos += 1
            value |= (byte & 127) << shift
            if value >= 1 << bits: raise ValueError('varint overflow')
            if not byte & 128: return value
        raise ValueError('oversized varint')
    while pos < len(payload):
        delta, address, value = uint(64), uint(32), uint(32)
        if timestamp + delta >= 1 << 64: raise ValueError('timestamp overflow')
        if not base <= address < base+count: raise ValueError('address outside pinned window')
        if value not in (0, 1): raise ValueError('reference transducer requires binary spikes')
        timestamp += delta
        values[address-base] += value
        events += 1
        if events > 65536: raise ValueError('event budget exceeded')
    return timestamp, values


class RateEncoder:
    """Deterministic pulse density, driven only by source simulation steps."""
    def __init__(self, size): self.phase = [0.] * size
    def encode(self, values, time_us, base):
        if len(values) != len(self.phase): raise ValueError('sensory shape mismatch')
        out=[]
        for i, value in enumerate(values):
            if not math.isfinite(value) or not 0 <= value <= 1: raise ValueError('invalid normalised sensor')
            self.phase[i] += value
            active = self.phase[i] >= 1
            if active: self.phase[i] -= 1
            out.append(1. if active else 0.)
        return aer_frame(time_us, base, out, .5)


class AsyncReferenceClient:
    """One worker per instance, two bounded pre-admission queues.

    A full capture mailbox rejects admission with a counted gap. A returned
    packet is never silently discarded: the worker awaits output credit.
    Fault/stop is local; no automatic reconnect or replay of ambiguous effects.
    """
    def __init__(self, model, binding, brain_manifest, host, port):
        binding.validate(model, brain_manifest)
        if model.get('controller_transfer','').startswith('blocked'):
            raise ValueError(model['controller_transfer'])
        # Legacy reference servers are local lab endpoints, never live devices.
        if host not in ('127.0.0.1', '::1', 'localhost'):
            raise ValueError('AER1 reference backend is loopback-only')
        self.model, self.binding = model, binding
        self.host, self.port = host, port
        self.brain_manifest = brain_manifest
        self.input = queue.Queue(maxsize=2); self.output = queue.Queue(maxsize=2)
        self.stop_event = threading.Event(); self.ready = threading.Event()
        self.fault = None; self.gaps = 0; self.exchanges = 0
        self.thread = threading.Thread(target=self._worker, name='aer-'+binding.instance_id, daemon=True)
        self.thread.start()

    def submit(self, step, timestamp_us, values):
        if self.fault or self.stop_event.is_set(): return False
        try: self.input.put_nowait((step, timestamp_us, tuple(values))); return True
        except queue.Full: self.gaps += 1; return False

    def receive(self):
        try: return self.output.get_nowait()
        except queue.Empty: return None

    def _worker(self):
        try: asyncio.run(self._run())
        except Exception as error: self.fault = f'{type(error).__name__}: {error}'
        finally: self.ready.set()

    async def _run(self):
        reader, writer = await asyncio.wait_for(asyncio.open_connection(self.host,self.port),5)
        io = self.model['io']; encoder = RateEncoder(len(io['sensors']))
        async def exchange(payload):
            if not payload or len(payload)>MAX_FRAME_BYTES: raise ValueError('invalid frame size')
            writer.write(struct.pack('<I',len(payload))+payload)
            await asyncio.wait_for(writer.drain(),5)
            size, = struct.unpack('<I',await asyncio.wait_for(reader.readexactly(4),5))
            if not 1 <= size <= MAX_FRAME_BYTES: raise ValueError('invalid response size')
            return await asyncio.wait_for(reader.readexactly(size),5)
        try:
            names = [s['name'] for s in io['sensors']]
            reply = json.loads(await exchange(json.dumps(dict(s_names=names,o_names=io['output_names'],sensory=len(names),output=len(io['output_names']),dt_ms=io['dt_us']/1000)).encode()))
            if (reply.get('expected_s'),reply.get('expected_o')) != (len(names),len(io['output_names'])):
                raise ValueError('handshake shape mismatch')
            if any(reply.get(k) != self.brain_manifest.get(k) for k in ('brain_id','brain_generation','model_digest','io_digest')):
                raise ValueError('server identity/contract mismatch; strict reference endpoint required')
            self.ready.set()
            while not self.stop_event.is_set():
                try: step, timestamp, values = self.input.get_nowait()
                except queue.Empty: await asyncio.sleep(.001); continue
                packet = await exchange(encoder.encode(values,timestamp,io['sensory_base']))
                output_timestamp, spikes = decode(packet,io['output_base'],len(io['output_names']))
                result = dict(step=step, source_timestamp_us=timestamp, output_timestamp_us=output_timestamp,
                              reset_epoch=self.binding.reset_epoch, binding_generation=self.binding.binding_generation,
                              brain_id=self.binding.brain_id, spikes=spikes, quality='legacy-reference-uncommitted')
                while not self.stop_event.is_set():
                    try: self.output.put_nowait(result); break
                    except queue.Full: await asyncio.sleep(.001)
                self.exchanges += 1
        finally:
            writer.close(); await writer.wait_closed()

    def close(self):
        self.stop_event.set(); self.thread.join(timeout=6)
        if self.thread.is_alive(): raise RuntimeError('AER worker did not cancel within IO deadline')
