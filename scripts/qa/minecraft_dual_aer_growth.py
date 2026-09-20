#!/usr/bin/env python3
"""Bounded two-brain AER1 proxy for the Minecraft hexapod sandbox.

The Java companion accepts one Rust TCP route for a profile. This proxy makes
that route a small causal fabric: Minecraft -> A -> B -> A -> Minecraft.
It deliberately uses the repository's legacy AER1 loopback adapter.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import signal
import socket
import struct
import sys
import threading
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from aer_legacy_codec import aer_frame, read_exact, write_frame
from nao_social_server import decode_aer

SENSORY = 34
OUTPUT = 18
INPUT_BASE = 4096
OUTPUT_BASE = 16384
MAX_FRAME = 4 * 1024 * 1024


def read_frame(stream: socket.socket) -> bytes:
    length = struct.unpack("<I", read_exact(stream, 4))[0]
    if not 0 < length <= MAX_FRAME:
        raise ValueError(f"invalid frame length {length}")
    return read_exact(stream, length)


def send_handshake(stream: socket.socket) -> None:
    write_frame(stream, json.dumps({"sensory": SENSORY, "output": OUTPUT, "dt_ms": 1}).encode())
    reply = json.loads(read_frame(stream))
    if reply != {"expected_s": SENSORY, "expected_o": OUTPUT}:
        raise RuntimeError(f"unexpected brain handshake: {reply}")


class Brain:
    def __init__(self, address: tuple[str, int], name: str):
        self.name = name
        self.stream = socket.create_connection(address, timeout=10)
        self.stream.settimeout(60)
        self.stream.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        send_handshake(self.stream)

    def step(self, timestamp_us: int, values: list[float]) -> tuple[int, list[int]]:
        if len(values) != SENSORY:
            raise ValueError(f"{self.name} expected {SENSORY} inputs")
        write_frame(self.stream, aer_frame(timestamp_us, INPUT_BASE, values, 0.5))
        payload = read_frame(self.stream)
        returned_timestamp, indices = decode_aer(payload, OUTPUT_BASE, OUTPUT)
        return returned_timestamp, indices

    def close(self) -> None:
        try:
            self.stream.close()
        except OSError:
            pass


def cross_input(environment: list[float], spikes: list[int], offset: int) -> list[float]:
    result = list(environment)
    for index in spikes:
        result[(offset + index) % SENSORY] = 1.0
    return result


def motion(spikes: list[int]) -> tuple[float, float]:
    left = sum(index in spikes for index in range(0, 9)) / 9.0
    right = sum(index in spikes for index in range(9, 18)) / 9.0
    return (left + right) / 2.0, (right - left) / 2.0


def target_motion(values: list[float]) -> tuple[float, float]:
    return motion([index for index, value in enumerate(values) if value > 0.5])


def control_score(spikes: list[int], target: tuple[float, float]) -> float:
    drive, turn = motion(spikes)
    return max(0.0, 1.0 - (abs(drive - target[0]) + abs(turn - target[1])) / 2.0)


class Fabric:
    def __init__(self, brain_a: Brain, brain_b: Brain, stats_path: Path):
        self.a = brain_a
        self.b = brain_b
        self.stats_path = stats_path
        self.lock = threading.Lock()
        self.stats: dict[str, Any] = {
            "protocol": "legacy-AER1-loopback",
            "logical_delay_ms": [1, 1],
            "frontend_frames": 0,
            "a_to_b_frames": 0,
            "b_to_a_frames": 0,
            "a_isolated_spikes": 0,
            "b_spikes": 0,
            "coupled_spikes": 0,
            "cross_brain": {"a_to_b": 0, "b_to_a": 0},
            "baseline_scores": [],
            "coupled_scores": [],
            "output_trace": [],
            "errors": [],
        }

    def persist(self) -> None:
        self.stats_path.parent.mkdir(parents=True, exist_ok=True)
        self.stats_path.write_text(json.dumps(self.stats, indent=2) + "\n")

    def exchange(
        self,
        timestamp_us: int,
        environment: list[float],
        target: tuple[float, float] = (0.0, 0.0),
    ) -> tuple[int, list[int]]:
        with self.lock:
            isolated_timestamp, isolated_a = self.a.step(timestamp_us, environment)
            b_input = cross_input(environment, isolated_a, 0)
            b_timestamp, b_spikes = self.b.step(isolated_timestamp + 1000, b_input)
            final_a_input = cross_input(environment, b_spikes, 17)
            final_timestamp, final_a = self.a.step(b_timestamp + 1000, final_a_input)
            # Brain B is an independently mounted inhibitory co-controller. Its
            # delayed AER output is rotated onto the complementary motor bank
            # and gates matching A spikes before the final hexapod actuator map.
            # This makes the AER return path causal and observable without
            # treating two saturated spike sets as a stronger motor command.
            b_actuator = [(index + 9) % OUTPUT for index in b_spikes]
            coupled = sorted(set(final_a).difference(b_actuator))

            self.stats["frontend_frames"] += 1
            self.stats["a_to_b_frames"] += 1
            self.stats["b_to_a_frames"] += 1
            self.stats["a_isolated_spikes"] += len(isolated_a)
            self.stats["b_spikes"] += len(b_spikes)
            self.stats["coupled_spikes"] += len(coupled)
            self.stats["cross_brain"]["a_to_b"] += len(isolated_a)
            self.stats["cross_brain"]["b_to_a"] += len(b_spikes)
            self.stats["baseline_scores"].append(control_score(isolated_a, target))
            self.stats["coupled_scores"].append(control_score(coupled, target))
            self.stats["output_trace"].append(
                {"isolated_a": isolated_a, "b": b_spikes, "final_a": final_a, "coupled": coupled}
            )
            self.persist()
            return final_timestamp, coupled


def serve(args: argparse.Namespace) -> int:
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind((args.listen_host, args.listen_port))
    listener.listen(1)
    listener.settimeout(0.5)
    print(f"dual AER fabric listening on {args.listen_host}:{args.listen_port}", flush=True)

    stopping = threading.Event()
    fabric: Fabric | None = None
    fixture_frames: list[dict[str, Any]] = []
    fixture_outputs: list[float] = []
    if args.fixture:
        fixture = json.loads(Path(args.fixture).read_text())
        fixture_case = next(item for item in fixture["cases"] if item["profile"] == "hexapod")
        fixture_frames = fixture_case["frames"]
        fixture_outputs = fixture_case["outputs"]

    def stop(_signum: int, _frame: Any) -> None:
        stopping.set()
        if fabric is not None:
            fabric.persist()
        try:
            listener.close()
        except OSError:
            pass

    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)

    while not stopping.is_set():
        try:
            frontend, peer = listener.accept()
        except socket.timeout:
            continue
        except OSError:
            break
        print(f"Minecraft route connected from {peer}", flush=True)
        try:
            frontend.settimeout(60)
            hello = json.loads(read_frame(frontend))
            if int(hello.get("sensory", 0)) != SENSORY or int(hello.get("output", 0)) != OUTPUT:
                raise ValueError(f"hexapod handshake mismatch: {hello}")
            a = Brain((args.brain_host, args.brain_a), "brain-a")
            b = Brain((args.brain_host, args.brain_b), "brain-b")
            fabric = Fabric(a, b, Path(args.stats))
            write_frame(frontend, json.dumps({"expected_s": SENSORY, "expected_o": OUTPUT}).encode())
            while not stopping.is_set():
                payload = read_frame(frontend)
                if payload.startswith(b"{"):
                    write_frame(frontend, json.dumps({"expected_s": SENSORY, "expected_o": OUTPUT}).encode())
                    continue
                timestamp_us, indices = decode_aer(payload, INPUT_BASE, SENSORY, input_amplitude=True)
                environment = [0.0] * SENSORY
                for index in indices:
                    environment[index] = 1.0
                frame_index = fabric.stats["frontend_frames"]
                target = (
                    target_motion(fixture_outputs)
                    if fixture_outputs
                    else (0.0, 0.0)
                )
                response_timestamp, coupled = fabric.exchange(timestamp_us, environment, target)
                write_frame(
                    frontend,
                    aer_frame(
                        response_timestamp,
                        OUTPUT_BASE,
                        [float(i in coupled) for i in range(OUTPUT)],
                        0.5,
                    ),
                )
        except (ConnectionError, EOFError, OSError, ValueError, RuntimeError) as error:
            if fabric is not None:
                fabric.stats["errors"].append(str(error))
                fabric.persist()
            print(f"dual AER frontend stopped: {error}", flush=True)
        finally:
            try:
                frontend.close()
            except OSError:
                pass
            if fabric is not None:
                fabric.a.close()
                fabric.b.close()
        if args.once:
            break
    listener.close()
    return 0


def drive_http(args: argparse.Namespace) -> int:
    import urllib.request

    fixture = json.loads(Path(args.fixture).read_text())
    case = next(item for item in fixture["cases"] if item["profile"] == "hexapod")
    content_digest = fixture["digest"]
    for step in range(args.iterations):
        frame = case["frames"][step % len(case["frames"])]
        body = {
            "network_id": "hexapod",
            "node_id": None,
            "step_index": step,
            "time_ms": step,
            "dt_ms": 1,
            "content_digest": content_digest,
            "capture_sequence": step,
            "capture_nanos": step * 1_000_000,
            "input_values": frame["values"],
        }
        request = urllib.request.Request(
            f"http://127.0.0.1:{args.bridge_port}/api/aer/infer",
            data=json.dumps(body).encode(),
            headers={"Authorization": f"Bearer {args.token}", "Content-Type": "application/json"},
        )
        with urllib.request.urlopen(request, timeout=65) as response:
            reply = json.loads(response.read(65536))
        if reply.get("network_id") != "hexapod":
            raise RuntimeError(f"unexpected bridge reply: {reply}")
    print(f"drove {args.iterations} fixture frames through Java bridge and dual fabric", flush=True)
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)

    serve_parser = subparsers.add_parser("serve")
    serve_parser.add_argument("--listen-host", default="127.0.0.1")
    serve_parser.add_argument("--listen-port", type=int, required=True)
    serve_parser.add_argument("--brain-host", default="127.0.0.1")
    serve_parser.add_argument("--brain-a", type=int, required=True)
    serve_parser.add_argument("--brain-b", type=int, required=True)
    serve_parser.add_argument("--stats", required=True)
    serve_parser.add_argument("--fixture")
    serve_parser.add_argument("--once", action="store_true")
    serve_parser.set_defaults(function=serve)

    drive_parser = subparsers.add_parser("drive-http")
    drive_parser.add_argument("--bridge-port", type=int, required=True)
    drive_parser.add_argument("--token", required=True)
    drive_parser.add_argument("--fixture", required=True)
    drive_parser.add_argument("--iterations", type=int, default=24)
    drive_parser.set_defaults(function=drive_http)

    args = parser.parse_args()
    return args.function(args)


if __name__ == "__main__":
    raise SystemExit(main())
