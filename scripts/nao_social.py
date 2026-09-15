"""Versioned, bounded simulator transducer/decoder; neural execution stays in Rust.

Time here is a named legacy reference-frame mapping. It is not a production
peripheral clock mapper, durable admission gateway or exactly-once effect store.
"""
from __future__ import annotations
from dataclasses import dataclass, field
from collections import deque
import json
import math
from pathlib import Path
import re
import secrets
import threading
import time
import unicodedata

ROOT = Path(__file__).resolve().parents[1]
CONTRACT = json.loads((ROOT/'sim/nao/interaction.json').read_text())


class SocialError(ValueError):
    """A bounded, safe-to-display interaction denial."""


def text_sample(text):
    if not isinstance(text, str) or not text.strip():
        raise SocialError('Enter a message for NAO')
    if any(unicodedata.category(c) in ('Cc', 'Cs') for c in text):
        raise SocialError('Control characters are not accepted')
    raw = text.encode('utf-8')
    if len(raw) > CONTRACT['max_utf8_bytes']:
        raise SocialError('Message exceeds 256 UTF-8 bytes')
    return text, raw


def lexical_features(text):
    words = set(re.findall(r'\w+', unicodedata.normalize('NFKC', text).casefold()))
    result = [int(bool(words.intersection(act['words']))) for act in CONTRACT['acts'][:8]]
    result[-1] = int(not any(result))
    return result


@dataclass
class Turn:
    id: str
    player: str
    generation: str
    sequence: int
    capture_ns: int
    modality: str
    text: str
    raw: bytes
    lexical: list
    submitted: float
    position: list
    confidence: float
    state: str = 'queued'
    frames: int = 0
    counts: list = field(default_factory=lambda: [0]*len(CONTRACT['acts']))
    first_step: int | None = None
    last_step: int | None = None
    reply: dict | None = None

    def public(self):
        return dict(schema=CONTRACT['schema'], id=self.id, state=self.state,
                    source_sequence=self.sequence, capture_timestamp_ns=self.capture_ns,
                    modality=self.modality, mapping='legacy-reference-frame-v1',
                    first_step=self.first_step, last_step=self.last_step,
                    frames=self.frames, reply=self.reply, model_note=CONTRACT['model_note'])


class Hub:
    """One sandbox body, at most sixteen players, one pending turn per player.

    HTTP threads can cancel during a slow neural step. The body thread snapshots
    input under a short lock; results are accepted only for that turn/generation.
    The post-turn guard is a presentation isolation policy, never quiescence.
    """
    def __init__(self, clock=time.monotonic):
        self.clock = clock
        self.lock = threading.RLock()
        self.generation = secrets.token_hex(16)
        self.players = {}
        self.turns = {}
        self.queue = deque()
        self.current = None
        self.quiet = 0
        self.body = None
        self.last_body = -math.inf
        self.last_step = -1

    def attach(self, owner):
        with self.lock:
            if self.body is not None:
                raise SocialError('This NAO already has a simulation writer')
            self.body = owner
            self.last_body = self.clock()

    def detach(self, owner):
        with self.lock:
            if self.body != owner:
                return
            for t in self.turns.values():
                if t.state in ('queued', 'active'):
                    t.state = 'cancelled: simulation disconnected'
            self.queue.clear()
            self.current = None
            self.quiet = CONTRACT['quiet_frames']
            self.body = None
            self.players.clear()
            self.generation = secrets.token_hex(16)

    def available(self):
        return self.body is not None and self.clock()-self.last_body < 10

    def register(self, principal=None):
        with self.lock:
            self.prune()
            if not self.available():
                raise SocialError('NAO is not receiving simulation frames')
            if principal is not None and (not isinstance(principal, str) or
                    not re.fullmatch(r'[A-Za-z0-9_.:-]{1,100}', principal)):
                raise SocialError('Invalid player identity')
            if principal in self.players:
                return principal
            if len(self.players) >= CONTRACT['max_players']:
                raise SocialError('Player capacity reached')
            principal = principal or secrets.token_urlsafe(32)
            self.players[principal] = dict(sequence=-1, capture_ns=-1, seen=self.clock(), turn=None, encountered=False)
            return principal

    def prune(self):
        now = self.clock()
        for key, player in list(self.players.items()):
            if now-player['seen'] > 300:
                self.stop(key)
                del self.players[key]
        for key, turn in list(self.turns.items()):
            if now-turn.submitted > 120 and turn.state not in ('queued', 'active'):
                del self.turns[key]

    def submit(self, principal, data):
        with self.lock:
            self.prune()
            if principal not in self.players or not self.available():
                raise SocialError('Conversation expired or simulation unavailable; join again')
            text, raw = text_sample(data.get('text'))
            sequence, capture_ns = data.get('sequence'), data.get('capture_ns')
            if type(sequence) is not int or not 0 <= sequence < 2**53 or type(capture_ns) is not int or not 0 <= capture_ns < 2**63:
                raise SocialError('Invalid source sequence or capture timestamp')
            if data.get('modality') not in ('typed', 'speech_transcript'):
                raise SocialError('Use typed text or an explicitly submitted speech transcript')
            position = data.get('position', [0, 0, 0])
            confidence = data.get('confidence', 1)
            if not isinstance(position, list) or len(position) != 3 or any(type(v) not in (float, int) or not math.isfinite(v) or abs(v)>1 for v in position):
                raise SocialError('Player position must be a normalised relative vector')
            if type(confidence) not in (float, int) or not math.isfinite(confidence) or not 0 <= confidence <= 1:
                raise SocialError('Invalid transcript confidence')
            p = self.players[principal]
            old = self.turns.get(p['turn'])
            if sequence == p['sequence']:
                if old and (old.text, old.capture_ns, old.modality, old.position, old.confidence) == (text, capture_ns, data['modality'], position, confidence):
                    return old.public()
                raise SocialError('Conflicting duplicate source sequence')
            if sequence < p['sequence'] or capture_ns < p['capture_ns']:
                raise SocialError('Stale source sequence or capture clock')
            if old and old.state in ('queued', 'active'):
                raise SocialError('Wait for your current turn or stop it')
            if len(self.queue)+(self.current is not None) >= CONTRACT['max_pending_turns']:
                raise SocialError('Conversation queue full; input was not admitted')
            if len(self.turns) >= 128:
                raise SocialError('Recent turn retention is full; wait before sending again')
            turn = Turn(secrets.token_urlsafe(24), principal, self.generation, sequence,
                        capture_ns, data['modality'], text, raw, lexical_features(text),
                        self.clock(), position, confidence)
            self.turns[turn.id] = turn
            self.queue.append(turn.id)
            p.update(sequence=sequence, capture_ns=capture_ns, seen=self.clock(), turn=turn.id)
            return turn.public()

    def encounter(self, principal, data):
        """One neural question per registered encounter; no fabricated utterance.

        Text-capable participants can hear an invitation. Other species receive
        motion/contact through scene sensors, never a fabricated text receptor.
        This observation grants no control or cross-world forwarding capability.
        """
        with self.lock:
            if principal not in self.players or not self.available():
                raise SocialError('Conversation expired or simulation unavailable')
            kind = data.get('kind', 'player')
            if kind not in ('player', 'npc', 'nao', 'celegans', 'drosophila_banc', 'drosophila_fafb', 'hexapod', 'zebrafish'):
                raise SocialError('Unknown participant capability profile')
            position = data.get('position', [0, 0, 0])
            if not isinstance(position, list) or len(position)!=3 or any(type(v) not in (float,int) or not math.isfinite(v) for v in position) or sum(v*v for v in position)>1:
                raise SocialError('Encounter is outside the local interaction range')
            sequence, capture_ns = data.get('sequence'), data.get('capture_ns')
            if type(sequence) is not int or not 0 <= sequence < 2**53 or type(capture_ns) is not int or not 0 <= capture_ns < 2**63:
                raise SocialError('Encounter requires source sequence and capture timestamp')
            p = self.players[principal]
            if p['encountered'] or kind not in ('player', 'npc', 'nao'):
                return dict(schema=CONTRACT['schema'], state='observed', id=None)
            if len(self.queue)+(self.current is not None)>=CONTRACT['max_pending_turns'] or len(self.turns)>=128:
                raise SocialError('Conversation queue full; encounter was not admitted')
            t = Turn(secrets.token_urlsafe(24), principal, self.generation, sequence, capture_ns,
                     'scene_encounter', '', b'', [0]*8, self.clock(), position, 1)
            self.turns[t.id] = t
            self.queue.append(t.id)
            p.update(encountered=True, turn=t.id, seen=self.clock())
            return t.public()

    def get(self, principal, turn_id):
        with self.lock:
            t = self.turns.get(turn_id)
            if not t or t.player != principal or principal not in self.players:
                raise SocialError('Turn unavailable for this player')
            self.players[principal]['seen'] = self.clock()
            if not self.available() and t.state in ('queued', 'active'):
                t.state = 'cancelled: simulation stalled'
            return t.public()

    def stop(self, principal):
        with self.lock:
            for turn in self.turns.values():
                if turn.player == principal and turn.state in ('queued', 'active'):
                    turn.state = 'cancelled by player'
            self.queue = deque(i for i in self.queue if self.turns[i].player != principal)
            if principal in self.players:
                self.players[principal]['encountered'] = True

    def leave(self, principal):
        with self.lock:
            self.stop(principal)
            self.players.pop(principal, None)

    def before_frame(self, owner):
        with self.lock:
            if owner != self.body:
                raise SocialError('Stale simulation writer')
            self.last_body = self.clock()
            if self.current and self.turns[self.current].state != 'active':
                self.current = None
                self.quiet = CONTRACT['quiet_frames']
            if self.quiet:
                self.quiet -= 1
                return None, [0]*32
            if not self.current:
                while self.queue:
                    key = self.queue.popleft()
                    t = self.turns[key]
                    if t.state != 'queued':
                        continue
                    if self.clock()-t.submitted > 30:
                        t.state = 'expired before admission'
                        continue
                    self.current = key
                    t.state = 'active'
                    break
            if not self.current:
                return None, [0]*32
            t = self.turns[self.current]
            i = t.frames
            features = t.lexical[:] if i < max(16, len(t.raw)) else [0]*8
            byte = t.raw[i] if i < len(t.raw) else 0
            features += [(byte>>b)&1 for b in range(8)]
            x, y, z = t.position
            features += [int(i==0), int(i==len(t.raw)-1), int(t.modality=='typed'),
                         int(t.modality=='speech_transcript'), 1, int(t.confidence>=.5),
                         int(sum(v*v for v in t.position)<.25), int(x<-.1), int(x>.1),
                         int(y>.1), int(y<-.1), int(i<len(t.raw)), 1,
                         int(t.modality=='scene_encounter' and i<16), 0, 0]
            return t.id, features

    def after_frame(self, owner, turn_id, step, spikes):
        with self.lock:
            if owner != self.body or type(step) is not int or step <= self.last_step:
                raise SocialError('Stale neural response')
            self.last_step = step
            if len(set(spikes)) != len(spikes) or any(type(i) is not int or not 0 <= i < CONTRACT['output'] for i in spikes):
                raise SocialError('Invalid neural output channels')
            if not turn_id or self.current != turn_id:
                return
            t = self.turns[turn_id]
            if t.state != 'active' or t.generation != self.generation:
                return
            if t.first_step is None:
                t.first_step = step
            t.last_step = step
            t.frames += 1
            for channel in spikes:
                if channel >= 40:
                    t.counts[channel-40] += 1
            ranked = sorted(range(len(CONTRACT['acts'])), key=lambda i: (-t.counts[i], i))
            winner = ranked[0]
            if t.frames >= max(16, len(t.raw)) and t.counts[winner]>=2 and t.counts[winner]>t.counts[ranked[1]]:
                act = CONTRACT['acts'][winner]
                t.reply = dict(text=act['text'], act=act['id'], gesture=act['gesture'],
                    output_channel=40+winner, observed_spikes=t.counts[winner],
                    presentation_id=self.generation+':'+t.id, source_step=step,
                    quality='legacy-reference-output; no production commit certificate')
                t.state = 'replied'
            elif t.frames >= CONTRACT['turn_frames']:
                t.state = 'no unambiguous neural reply'
