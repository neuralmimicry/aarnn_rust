"""Local, opt-in NAO reference I/O proxy. No neural implementation or public listener."""
from __future__ import annotations
import hmac
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import math
from pathlib import Path
import secrets
import select
import socket
import struct
import threading
from urllib.parse import urlsplit

from nao_social import CONTRACT, Hub, ROOT, SocialError
from tcp_aer_ipc_bridge import aer_frame, read_exact, write_frame

MAX_FRAME = 65536


def read_frame(stream):
    length = struct.unpack('<I', read_exact(stream, 4))[0]
    if not 0 < length <= MAX_FRAME:
        raise SocialError('Frame exceeds the 64 KiB allocation bound')
    return read_exact(stream, length)



def decode_aer(data, base, count, input_amplitude=False):
    if len(data)<12 or len(data)>MAX_FRAME or data[:4]!=b'AER1':
        raise SocialError('Invalid AER1 frame')
    values, value, shift = [], 0, 0
    for byte in data[12:]:
        value |= (byte & 127) << shift
        if byte & 128:
            shift += 7
            if shift >= 64:
                raise SocialError('AER varint overflow')
        else:
            values.append(value)
            value = shift = 0
    if shift or len(values)%3 or len(values)>count*3:
        raise SocialError('Malformed AER events')
    indices = []
    for i in range(0, len(values), 3):
        delta, address, polarity = values[i:i+3]
        # Unity/Unreal encode above-threshold sensory amplitude as 1..255.
        # The existing Rust AER1 codec treats its nonzero low byte as a spike.
        if delta or not base <= address < base+count or not 1 <= polarity <= (255 if input_amplitude else 1):
            raise SocialError('Unexpected AER address, time or polarity')
        indices.append(address-base)
    if len(set(indices)) != len(indices):
        raise SocialError('Duplicate AER event')
    return struct.unpack_from('<Q', data, 4)[0], indices


class Body:
    def __init__(self, backend, hub, substeps=8):
        self.backend = backend
        self.hub = hub
        self.substeps = substeps
        self.lock = threading.Lock()
        self.failed = False
        self.socket = socket.create_connection(backend, timeout=60)
        self.socket.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        self.socket.settimeout(10)
        write_frame(self.socket, json.dumps(dict(sensory=CONTRACT['sensory'], output=CONTRACT['output'], dt_ms=1)).encode())
        if json.loads(read_frame(self.socket)) != dict(expected_s=CONTRACT['sensory'], expected_o=CONTRACT['output']):
            self.socket.close()
            raise SocialError('Rust snapshot does not expose the social profile')

    def step(self, owner, values):
        if not isinstance(values, list) or len(values)!=250 or any(type(v) not in (int, float) or not math.isfinite(v) or not 0 <= v <= 1 for v in values):
            raise SocialError('Expected 250 finite normalised body inputs')
        if not self.lock.acquire(blocking=False):
            raise SocialError('A body frame is already outstanding')
        try:
            if self.failed:
                raise SocialError('Neural exchange failed; restart this reference session')
            active = set()
            for _ in range(self.substeps):
                turn, social = self.hub.before_frame(owner)
                timestamp = (self.hub.last_step+1)*1000
                write_frame(self.socket, aer_frame(timestamp, 4096, values+social, .5))
                response = read_frame(self.socket)
                timestamp, indices = decode_aer(response, 16384, CONTRACT['output'])
                if timestamp%1000:
                    raise SocialError('Rust output differs from pinned 1 ms reference step')
                step = timestamp//1000
                self.hub.after_frame(owner, turn, step, indices)
                active.update(i for i in indices if i<40)
            return dict(network_id='nao', output_step_index=step,
                        output_spike_indices=sorted(active), reference_substeps=self.substeps)
        except (OSError, EOFError, ValueError):
            self.failed = True
            self.hub.detach(owner)
            self.socket.close()
            raise SocialError('Neural exchange failed; no automatic retry') from None
        finally:
            self.lock.release()

    def close(self):
        self.socket.close()


def frame_values(payload):
    if payload.startswith(b'AER1'):
        _, indices = decode_aer(payload, 4096, 250, input_amplitude=True)
        values = [0]*250
        for i in indices:
            values[i] = 1
        return values
    if len(payload)!=1004:
        raise SocialError('Body frame must retain the 250-channel NAO contract')
    values = struct.unpack('<251f', payload)
    if not math.isfinite(values[0]):
        raise SocialError('Invalid body time field')
    return list(values[1:])


def handshake(payload):
    data = json.loads(payload)
    s = len(data.get('s_names', [])) or data.get('sensory', data.get('expected_s', 250))
    o = len(data.get('o_names', [])) or data.get('output', data.get('expected_o', 40))
    if (s, o)!=(250, 40):
        raise SocialError('This proxy accepts only the original NAO 250/40 body')
    return b'{"expected_s":250,"expected_o":40}'


def tcp_loop(listener, body, stop):
    listener.settimeout(.5)
    while not stop.is_set():
        try:
            stream, _ = listener.accept()
        except socket.timeout:
            continue
        owner = 'tcp:'+secrets.token_hex(12)
        try:
            # The local listener owns one body, so no per-client unbounded threads.
            body.hub.attach(owner)
            stream.settimeout(10)
            stream.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
            with stream:
                negotiated = False
                while not stop.is_set():
                    # Companions negotiate before a game is loaded/armed. Idle
                    # transport must not consume a neural step or break that
                    # cached connection; partial frames still have a deadline.
                    if not select.select([stream], [], [], .5)[0]:
                        continue
                    payload = read_frame(stream)
                    if payload.startswith(b'{'):
                        write_frame(stream, handshake(payload))
                        negotiated = True
                        continue
                    if not negotiated:
                        raise SocialError('Handshake required')
                    result = body.step(owner, frame_values(payload))
                    values = [int(i in result['output_spike_indices']) for i in range(40)]
                    response = aer_frame(result['output_step_index']*1000, 16384, values, .5) if payload.startswith(b'AER1') else struct.pack('<40f', *values)
                    write_frame(stream, response)
        except (OSError, EOFError, ValueError):
            stream.close()
        finally:
            body.hub.detach(owner)


def uds_loop(listener, body, stop):
    listener.settimeout(.5)
    owner = None
    try:
        while not stop.is_set():
            try:
                payload, peer = listener.recvfrom(MAX_FRAME+1)
            except socket.timeout:
                if owner and not body.hub.available():
                    body.hub.detach(owner)
                    owner = None
                continue
            try:
                if not peer or len(payload)>MAX_FRAME:
                    continue
                if owner is None:
                    body.hub.attach(peer)
                    owner = peer
                if peer != owner:
                    continue
                if payload.startswith(b'{'):
                    response = handshake(payload)
                else:
                    result = body.step(owner, frame_values(payload))
                    # The existing Webots datagram controller expects raw motor floats.
                    response = struct.pack('<40f', *[int(i in result['output_spike_indices']) for i in range(40)])
                listener.sendto(response, peer)
            except (OSError, EOFError, ValueError):
                if owner:
                    body.hub.detach(owner)
                    owner = None
    finally:
        if owner:
            body.hub.detach(owner)


class BoundedHttp(ThreadingHTTPServer):
    daemon_threads = True
    def __init__(self, address, handler):
        self.slots = threading.BoundedSemaphore(8)
        super().__init__(address, handler)

    def process_request(self, request, address):
        if not self.slots.acquire(blocking=False):
            request.close()
            return
        try:
            super().process_request(request, address)
        except BaseException:
            self.slots.release()
            raise

    def process_request_thread(self, request, address):
        try:
            super().process_request_thread(request, address)
        finally:
            self.slots.release()


def handler(body, adapter_token, join_token):
    hub = body.hub
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass  # Never log player text, credentials, URLs or response payloads.

        def setup(self):
            super().setup()
            self.connection.settimeout(5)

        def send(self, status, data, kind='application/json'):
            raw = json.dumps(data, ensure_ascii=True).encode() if kind=='application/json' else data
            self.send_response(status)
            self.send_header('Content-Type', kind)
            self.send_header('Content-Length', str(len(raw)))
            self.send_header('Cache-Control', 'no-store')
            self.send_header('X-Content-Type-Options', 'nosniff')
            self.send_header('Referrer-Policy', 'no-referrer')
            self.send_header('Content-Security-Policy', "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'self' http://127.0.0.1:* http://localhost:*; object-src 'none'; base-uri 'none'")
            self.end_headers()
            self.wfile.write(raw)

        def authorised(self, token):
            value = self.headers.get('Authorization', '')
            return len(value)<=4096 and hmac.compare_digest(value, 'Bearer '+token)

        def check_origin(self):
            host = self.headers.get('Host', '')
            permitted = {f'{h}:{self.server.server_port}' for h in ('127.0.0.1', 'localhost')}
            if host not in permitted:
                raise SocialError('Unrecognised local host')
            origin = self.headers.get('Origin')
            if origin and origin != 'http://'+host:
                raise SocialError('Cross-origin interaction is denied')

        def principal(self, data):
            if self.authorised(adapter_token):
                principal = data.get('player')
                if not isinstance(principal, str) or not principal.startswith(('java:', 'bedrock:', 'unity:', 'unreal:', 'webots:')):
                    raise SocialError('Trusted simulator player identity required')
                return hub.register(principal)
            token = self.headers.get('Authorization', '')
            principal = token[7:] if token.startswith('Bearer ') else ''
            with hub.lock:
                if principal not in hub.players or ':' in principal:
                    raise SocialError('Join this conversation first')
            return principal

        def do_GET(self):
            try:
                self.check_origin()
                path = urlsplit(self.path).path
                if path=='/api/status':
                    self.send(200, dict(schema=CONTRACT['schema'], available=hub.available(),
                                        output_step_index=hub.last_step,
                                        model_note=CONTRACT['model_note'], voice='browser speech recognition if available; editable transcript'))
                    return
                if path=='/api/config':
                    self.send(200, dict(default_network='nao'))
                    return
                assets = {'/':'sim/nao/chat.html', '/chat.js':'sim/nao/chat.js',
                          '/world':'web_ui/webgl-sim.html', '/style.css':'web_ui/style.css',
                          '/webgl-sim.js':'web_ui/webgl-sim.js', '/webgl-world.js':'web_ui/webgl-world.js',
                          '/aer-transport.js':'web_ui/aer-transport.js',
                          '/sim-content.generated.js':'web_ui/sim-content.generated.js'}
                if path not in assets:
                    self.send(404, dict(error='Not found'))
                    return
                file = ROOT/assets[path]
                kind = 'text/html; charset=utf-8' if path in ('/', '/world') else 'text/css' if path.endswith('.css') else 'text/javascript'
                self.send(200, file.read_bytes(), kind)
            except (OSError, ValueError):
                self.send(403, dict(error='Page unavailable for this origin'))

        def do_POST(self):
            try:
                self.check_origin()
                if self.headers.get('Transfer-Encoding') or self.headers.get('Content-Type', '').split(';')[0]!='application/json':
                    raise SocialError('Bounded JSON request required')
                length = int(self.headers.get('Content-Length', '0'))
                if not 0 < length <= 16384:
                    raise SocialError('Request exceeds bounds')
                data = json.loads(self.rfile.read(length))
                if not isinstance(data, dict):
                    raise SocialError('JSON object required')
                path = urlsplit(self.path).path
                if path=='/api/join':
                    if not self.authorised(join_token):
                        raise SocialError('Invalid world invitation')
                    result = dict(player_token=hub.register(), schema=CONTRACT['schema'])
                elif path.startswith('/api/body'):
                    if not self.authorised(adapter_token):
                        raise SocialError('Body control requires the local simulator credential')
                    if path=='/api/body/open':
                        owner = 'http:'+secrets.token_hex(16)
                        hub.attach(owner)
                        result = dict(body_session=owner)
                    elif path=='/api/body/close':
                        hub.detach(data.get('body_session'))
                        result = dict(state='disarmed')
                    elif path=='/api/body':
                        result = body.step(data.get('body_session'), data.get('input_values'))
                    else:
                        raise SocialError('Unknown body action')
                else:
                    principal = self.principal(data)
                    if path=='/api/turn':
                        result = hub.submit(principal, data)
                    elif path=='/api/encounter':
                        result = hub.encounter(principal, data)
                    elif path=='/api/leave':
                        hub.leave(principal)
                        result = dict(state='left')
                    elif path=='/api/poll':
                        result = hub.get(principal, data.get('id'))
                    elif path=='/api/stop':
                        hub.stop(principal)
                        result = dict(state='stopped')
                    else:
                        raise SocialError('Unknown interaction action')
                self.send(200, result)
            except SocialError as error:
                self.send(409, dict(error=str(error)))
            except (ValueError, TypeError, KeyError, OSError):
                self.send(400, dict(error='Invalid or incomplete request'))
    return Handler
