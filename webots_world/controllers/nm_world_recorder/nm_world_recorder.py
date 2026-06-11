#!/usr/bin/env python3
"""Optional Webots Supervisor movie recorder.

The controller is intentionally inert unless NM_WEBOTS_RECORD is enabled.
"""

from __future__ import annotations

import os
import signal
import sys
from pathlib import Path

from controller import Supervisor


def env_bool(name: str, default: bool = False) -> bool:
    raw = os.environ.get(name)
    if raw is None or raw == "":
        return default
    return raw.strip().lower() in {"1", "true", "yes", "on"}


def env_int(name: str, default: int, minimum: int, maximum: int) -> int:
    raw = os.environ.get(name)
    if raw is None or raw == "":
        return default
    try:
        value = int(raw)
    except ValueError:
        return default
    if value < minimum or value > maximum:
        return default
    return value


def default_output_path() -> str:
    root = Path(os.environ.get("NM_WEBOTS_RECORD_DIR", "logs"))
    return str(root / "webots_recording.mp4")


def main() -> int:
    supervisor = Supervisor()
    timestep = int(supervisor.getBasicTimeStep())

    if not env_bool("NM_WEBOTS_RECORD", False):
        return 0

    output = os.environ.get("NM_WEBOTS_RECORD_FILE", "").strip() or default_output_path()
    output_path = Path(output).expanduser()
    output_path.parent.mkdir(parents=True, exist_ok=True)

    width = env_int("NM_WEBOTS_RECORD_WIDTH", 1280, 16, 16384)
    height = env_int("NM_WEBOTS_RECORD_HEIGHT", 720, 16, 16384)
    codec = env_int("NM_WEBOTS_RECORD_CODEC", 0, 0, 64)
    quality = env_int("NM_WEBOTS_RECORD_QUALITY", 85, 1, 100)
    acceleration = env_int("NM_WEBOTS_RECORD_ACCELERATION", 1, 1, 512)
    duration_ms = env_int("NM_WEBOTS_RECORD_DURATION_MS", 0, 0, 86_400_000)
    caption = env_bool("NM_WEBOTS_RECORD_CAPTION", False)
    quit_on_done = env_bool("NM_WEBOTS_RECORD_QUIT_ON_DONE", duration_ms > 0)
    progress_enabled = env_bool("NM_WEBOTS_RECORD_PROGRESS", False) and duration_ms > 0
    progress_interval_ms = env_int("NM_WEBOTS_RECORD_PROGRESS_INTERVAL_MS", 500, 100, 600_000)

    stop_requested = False

    def request_stop(_signum, _frame) -> None:
        nonlocal stop_requested
        stop_requested = True

    signal.signal(signal.SIGTERM, request_stop)
    signal.signal(signal.SIGINT, request_stop)

    print(
        "[nm_world_recorder] recording "
        f"{output_path} {width}x{height} codec={codec} quality={quality} "
        f"acceleration={acceleration} duration_ms={duration_ms or 'until-stop'}",
        flush=True,
    )
    supervisor.movieStartRecording(
        str(output_path),
        width,
        height,
        codec,
        quality,
        acceleration,
        caption,
    )

    def emit_progress(elapsed_ms: float) -> None:
        if not progress_enabled:
            return
        clamped_ms = max(0, min(int(elapsed_ms), duration_ms))
        pct = int(round((clamped_ms / duration_ms) * 100.0)) if duration_ms > 0 else 0
        pct = max(0, min(pct, 100))
        print(
            f"[nm_world_recorder] progress elapsed_ms={clamped_ms} "
            f"duration_ms={duration_ms} pct={pct}",
            flush=True,
        )

    start_ms = supervisor.getTime() * 1000.0
    next_progress_ms = 0.0
    recording = True
    while supervisor.step(timestep) != -1:
        elapsed_ms = supervisor.getTime() * 1000.0 - start_ms
        if progress_enabled and recording and elapsed_ms >= next_progress_ms:
            emit_progress(elapsed_ms)
            next_progress_ms = elapsed_ms + progress_interval_ms
        duration_reached = duration_ms > 0 and elapsed_ms >= duration_ms
        if recording and (stop_requested or duration_reached):
            emit_progress(duration_ms if duration_reached else elapsed_ms)
            print("[nm_world_recorder] stopping recording", flush=True)
            supervisor.movieStopRecording()
            recording = False
        if not recording and supervisor.movieIsReady():
            if supervisor.movieFailed():
                print(f"[nm_world_recorder] recording failed: {output_path}", file=sys.stderr, flush=True)
                if quit_on_done:
                    supervisor.simulationQuit(1)
                return 1
            print(f"[nm_world_recorder] recording ready: {output_path}", flush=True)
            if quit_on_done:
                supervisor.simulationQuit(0)
            return 0

    if recording:
        supervisor.movieStopRecording()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
