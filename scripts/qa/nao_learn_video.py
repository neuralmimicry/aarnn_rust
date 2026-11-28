#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Learning-by-experience video: chosen time windows of one long lock-step
closed-loop run, robot (left) and the live AARNN network (right).

The network pane is the real ``aarnn_rust --ui`` (ui_screenshot build)
replaying the run's whole sensory stream, so its synapses learn exactly as
they did in the run; ``NM_UI_REPLAY_WINDOWS`` saves screenshots only inside
the windows (``frames.tsv`` maps each frame to simulated ms). The robot pane
replays the recorded ``qpos`` of the same windows. Each window becomes one
captioned segment; the segments are joined in order.

Needs a run recorded with ``--trace-every 1``:

  python3 scripts/qa/nao_learn_video.py RUN_DIR NETWORK CONFIG OUT.mp4 \\
      --window 15-75:"Encounter 1 - innate reflex: walks in, backs off" \\
      --window 600-640:"Encounter 9 - learned: turns before the wall"
"""
import argparse
import importlib.util
import json
import os
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("sv", ROOT / "scripts/qa/showcase_video.py")
sv = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sv)


def window_trace(trace, out, t0, t1, every=10):
    """Rows of [t0, t1) seconds every `every` physics steps for the robot pane
    (the trace may itself be sampled every N steps: keep every `every / N`)."""
    with open(trace) as f, open(out, "w") as g:
        header = json.loads(f.readline())
        sampled = max(1, int(header.get("trace_every_steps", 1)))
        if every % sampled:
            raise SystemExit(f"frame interval {every} steps is not a multiple of the trace's {sampled}")
        header["trace_every_steps"] = every
        g.write(json.dumps(header) + "\n")
        every //= sampled
        k = 0
        for line in f:
            r = json.loads(line)
            t = r.get("time_s", 0.0)
            if t < t0:
                continue
            if t >= t1:
                break
            if k % every == 0:
                g.write(line)
            k += 1


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("run_dir", type=Path)
    ap.add_argument("network", type=Path)
    ap.add_argument("config", type=Path)
    ap.add_argument("output", type=Path)
    ap.add_argument("--window", action="append", required=True, help='"T0-T1:caption" in seconds')
    ap.add_argument("--title", default="NAO learns by experience - AARNN network in its 3D human-equivalent layout")
    ap.add_argument("--height", type=int, default=720)
    ap.add_argument("--timeout", type=float, default=20000)
    ap.add_argument("--skip-ui", action="store_true", help="reuse captured UI frames")
    ap.add_argument("--display", default=":97", help="Xvfb display for the UI capture (one per parallel capture)")
    ap.add_argument("--fps", type=int, default=25, choices=(25, 50), help="video frame rate (simulated time, 1:1)")
    ap.add_argument("--robot-args", default="--fixed 0.6,-0.4,0 --distance 3.0 --elevation -70 --azimuth 200 --trail-radius 0.02",
                    help="render_trace_video.py camera arguments (default: elevated view of the whole room, so "
                         "the path trail shows where the body went)")
    ap.add_argument("--ui-crop", default="1632:1080:0:0")
    ap.add_argument("--cam", default="NM_UI_CAMERA_YAW_DEG=60 NM_UI_CAMERA_ROLL_DEG=-90 NM_UI_CAMERA_ZOOM=2.8 "
                                     "NM_UI_CAMERA_STRETCH=3.4", help="UI camera environment (as the showcase)")
    a = ap.parse_args()
    run = a.run_dir.resolve()
    trace = next((run / "trace").glob("*.jsonl"))
    work = run / "learn_video"
    work.mkdir(exist_ok=True)
    every = 20 if a.fps == 25 else 10
    step_ms = 2 * every
    wins = []
    for w in a.window:
        span, _, cap = w.partition(":")
        t0, t1 = (float(x) for x in span.split("-"))
        wins.append((t0, t1, cap))
    frames = work / "ui_frames"
    if not a.skip_ui:
        display = sv.ensure_xvfb(a.display)
        extra = dict(kv.split("=", 1) for kv in a.cam.split())
        # OpenGL renderer: wgpu loses its (software) device under Xvfb.
        extra.update(NM_UI_RENDERER="glow", NM_UI_REPLAY_EVERY=str(every), NM_UI_VISUALIZATION_STAGE="6",
                     NM_UI_VISUALIZATION_AUTO="0",
                     NM_UI_WINDOW_SIZE="1920x1080",
                     NM_UI_REPLAY_WARMUP_FRAMES="300", NM_UI_REPLAY_WINDOWS=",".join(f"{t0}-{t1}" for t0, t1, _ in wins))
        n = sv.capture_ui(trace, a.network.resolve(), a.config.resolve(), frames, display, a.timeout, extra)
        print(f"ui frames: {n}")
    index = [tuple(int(x) for x in line.split("\t")) for line in (frames / "frames.tsv").read_text().splitlines()]
    segs = []
    for i, (t0, t1, cap) in enumerate(wins):
        sel = [k for k, ms in index if t0 * 1000 <= ms - step_ms < t1 * 1000]
        missing = [k for k in sel if not (frames / f"frame_{k:06d}.png").exists()]
        if missing:
            raise SystemExit(f"window {t0}-{t1}: {len(missing)} UI frames missing (capture incomplete?)")
        if not sel:
            print(f"window {t0}-{t1}: no UI frames")
            continue
        wdir = work / f"w{i}"
        shutil.rmtree(wdir, ignore_errors=True)
        wdir.mkdir()
        for j, k in enumerate(sel):
            os.link(frames / f"frame_{k:06d}.png", wdir / f"f_{j:06d}.png")
        wt = wdir / "trace.jsonl"
        window_trace(trace, wt, t0, t1, every)
        robot = wdir / "robot.mp4"
        subprocess.run([str(sv.VENV_PY), "scripts/qa/render_trace_video.py", str(wt), str(robot), "--width",
                        str(a.height * 16 // 9), "--height", str(a.height), "--title", "NAO (MuJoCo)",
                        "--time-offset", f"{t0:.3f}"] + a.robot_args.split(), cwd=ROOT, env=dict(os.environ, MUJOCO_GL="egl"),
                       check=True, capture_output=True)
        ui = wdir / "ui.mp4"
        crop = f"crop={a.ui_crop}," if a.ui_crop else ""
        subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-framerate", str(a.fps), "-i", str(wdir / "f_%06d.png"),
                        "-vf", f"{crop}scale=-2:{a.height}", "-pix_fmt", "yuv420p", "-c:v", "libx264", "-crf", "20",
                        str(ui)], check=True)
        seg = wdir / "segment.mp4"
        # Captions from text files (no filter-string escaping).
        (wdir / "caption.txt").write_text(cap)
        (wdir / "footer.txt").write_text(f"{a.title}   |   left: MuJoCo body   right: live AARNN network, lock-step")
        top = (f"drawtext=textfile={wdir / 'caption.txt'}:expansion=none:x=(w-text_w)/2:y=64:fontsize=30:"
               f"fontcolor=white:box=1:boxcolor=black@0.6:boxborderw=10")
        bottom = (f"drawtext=textfile={wdir / 'footer.txt'}:expansion=none:x=20:y=h-38:fontsize=20:fontcolor=white:"
                  f"box=1:boxcolor=black@0.55:boxborderw=6")
        filt = (f"[0:v]trim=start_frame=1,setpts=PTS-STARTPTS,scale=-2:{a.height}[l];[1:v]scale=-2:{a.height}[r];"
                f"[l][r]hstack=inputs=2:shortest=1,{top},{bottom},fps={a.fps}[v]")
        subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", str(robot), "-i", str(ui), "-filter_complex", filt,
                        "-map", "[v]", "-pix_fmt", "yuv420p", "-c:v", "libx264", "-crf", "20", str(seg)], check=True)
        segs.append(seg)
        print(f"window {t0}-{t1}: {len(sel)} frames -> {seg}")
    lst = work / "segments.txt"
    lst.write_text("".join(f"file '{s}'\n" for s in segs))
    a.output.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", str(lst), "-c:v", "libx264",
                    "-crf", "20", "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(a.output)], check=True)
    print(json.dumps(dict(video=str(a.output), segments=len(segs))))


if __name__ == "__main__":
    main()
