#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Public showcase video: the robot (left) and the AARNN Rust UI showing its
network (right), in lock-step.

Input is a lock-step closed-loop run recorded with ``--trace-every 1`` (the
body's sensory frame of every 2 ms step). The robot pane replays the
recorded ``qpos`` (``render_trace_video.py``); the network pane is the real
``aarnn_rust --ui`` (built with ``ui_screenshot``) loading the same network
and stepping it through the same sensory stream with the embodiment
encoder, saving one screenshot every 10 frames (20 ms) - so both panes show
the same simulated instant at 50 frames per second (real time).

  python3 scripts/qa/showcase_video.py RUN_DIR NETWORK CONFIG OUT.mp4 --title "..."

Needs an X display for the UI (a private Xvfb is started on :97 if
``DISPLAY`` is unset or ``--xvfb`` is given).
"""
import argparse
import json
import os
import shutil
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
UI_BIN = ROOT / "target/ui-capture/release/aarnn_rust"
XVFB = ROOT / "target/xvfb/root/usr/bin/Xvfb"
VENV_PY = ROOT / "target/robot-mujoco-venv/bin/python"


def subsample(trace, out, every=10):
    with open(trace) as f, open(out, "w") as g:
        header = json.loads(f.readline())
        header["trace_every_steps"] = every
        g.write(json.dumps(header) + "\n")
        for k, line in enumerate(f):
            if k % every == 0:
                g.write(line)
    return header


def ensure_xvfb(display=":97"):
    probe = subprocess.run(["bash", "-c", f"DISPLAY={display} xdpyinfo >/dev/null 2>&1"])
    if probe.returncode == 0:
        return display
    # -noreset: without it the server resets (pointer back to the centre,
    # where it hovers the graph and pops tooltips) whenever the last client
    # disconnects - e.g. right after the pointer was parked.
    subprocess.Popen([str(XVFB), display, "-screen", "0", "1920x1080x24", "-nolisten", "tcp", "-noreset"],
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    for _ in range(50):
        time.sleep(0.2)
        if subprocess.run(["bash", "-c", f"DISPLAY={display} xdpyinfo >/dev/null 2>&1"]).returncode == 0:
            return display
    raise RuntimeError("Xvfb did not start")


def capture_ui(trace, network, config, frames_dir, display, timeout_s, extra_env=None):
    shutil.rmtree(frames_dir, ignore_errors=True)
    frames_dir.mkdir(parents=True)
    env = dict(os.environ, DISPLAY=display, NM_UI_REPLAY_INPUT=str(trace), NM_UI_REPLAY_CAPTURE_DIR=str(frames_dir),
               NM_UI_REPLAY_EVERY="10", NM_DISABLE_OPENCL="1", RAYON_NUM_THREADS="2", NM_AUTO_AARNN_DEPTH="0")
    env.pop("WAYLAND_DISPLAY", None)
    env.update(extra_env or {})
    cmd = [str(UI_BIN), "--ui", "--network", str(network), "--config", str(config)]
    with open(frames_dir.parent / "ui_capture.log", "w") as log:
        p = subprocess.Popen(cmd, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        t0 = time.time()
        while p.poll() is None:
            if (frames_dir / "DONE").exists() or time.time() - t0 > timeout_s:
                time.sleep(2)
                if p.poll() is None:
                    p.terminate()
                    try:
                        p.wait(10)
                    except subprocess.TimeoutExpired:
                        p.kill()
                break
            time.sleep(1)
    return len(list(frames_dir.glob("frame_*.png")))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("run_dir", type=Path)
    ap.add_argument("network", type=Path)
    ap.add_argument("config", type=Path)
    ap.add_argument("output", type=Path)
    ap.add_argument("--title", default="")
    ap.add_argument("--robot-args", default="", help="extra render_trace_video.py arguments")
    ap.add_argument("--height", type=int, default=1080)
    ap.add_argument("--timeout", type=float, default=5400)
    ap.add_argument("--skip-robot", action="store_true")
    ap.add_argument("--skip-ui", action="store_true")
    ap.add_argument("--ui-crop", default="1632:1080:0:0",
                    help="ffmpeg crop W:H:X:Y of the UI frames (default drops the Controls side panel)")
    args = ap.parse_args()
    run = args.run_dir.resolve()
    trace = next((run / "trace").glob("*.jsonl"))
    work = run / "showcase"
    work.mkdir(exist_ok=True)
    sub = work / "trace_20ms.jsonl"
    header = subsample(trace, sub)
    robot_mp4 = work / "robot.mp4"
    if not args.skip_robot:
        cmd = [str(VENV_PY), "scripts/qa/render_trace_video.py", str(sub), str(robot_mp4), "--width",
               str(args.height * 16 // 9), "--height", str(args.height)] + (args.robot_args.split() if args.robot_args else [])
        subprocess.run(cmd, cwd=ROOT, env=dict(os.environ, MUJOCO_GL="egl"), check=True, capture_output=True)
    frames = work / "ui_frames"
    if not args.skip_ui:
        display = ensure_xvfb()
        n = capture_ui(trace, args.network.resolve(), args.config.resolve(), frames, display, args.timeout)
        print(f"ui frames: {n}")
    ui_mp4 = work / "ui.mp4"
    crop = f"crop={args.ui_crop}," if args.ui_crop else ""
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-framerate", "50", "-i", str(frames / "frame_%06d.png"),
                    "-vf", f"{crop}scale=-2:{args.height}", "-pix_fmt", "yuv420p", "-c:v", "libx264", "-crf", "20",
                    str(ui_mp4)], check=True)
    # Side by side; the network pane is captured after its first 20 ms step,
    # so drop the robot's first frame to align simulated time.
    title = args.title.replace(":", r"\:").replace("'", "")
    draw = (f"drawtext=text='{title}':x=20:y=h-40:fontsize=26:fontcolor=white:box=1:boxcolor=black@0.55:boxborderw=8"
            if title else "null")
    filt = (f"[0:v]trim=start_frame=1,setpts=PTS-STARTPTS,scale=-2:{args.height}[l];"
            f"[1:v]scale=-2:{args.height}[r];[l][r]hstack=inputs=2:shortest=1,{draw}[v]")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", str(robot_mp4), "-i", str(ui_mp4), "-filter_complex", filt,
                    "-map", "[v]", "-pix_fmt", "yuv420p", "-c:v", "libx264", "-crf", "20", "-movflags", "+faststart",
                    str(args.output)], check=True)
    print(json.dumps(dict(video=str(args.output), profile=header.get("profile"))))


if __name__ == "__main__":
    main()
