#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Frame-by-frame replay video of a lock-step C. elegans MuJoCo trace.

The brain is slower than wall-clock time, so the body runs in lock-step
(``--lockstep``) and the native adapter records the full ``qpos`` and the
body-wall muscle activations every ``--trace-every`` physics steps
(``aarnn-mujoco-trace-v2``). This script restores each recorded state with
``mj_forward`` (no re-simulation) and renders it offscreen, so the video plays
at true simulation speed: frame ``k`` is shown at ``k * trace_every * 2 ms``.
Lag never appears in the video.

Muscle quadrants are tinted by activation (red = contracting), the head path is
drawn as a trail, and the overlay shows simulation time.

Run with the repository MuJoCo venv:
  MUJOCO_GL=egl target/robot-mujoco-venv/bin/python \\
    scripts/qa/render_celegans_trace_video.py TRACE.jsonl OUT.mp4
"""
import argparse
import json
import math
import os
import shutil
import subprocess
from pathlib import Path

os.environ.setdefault('MUJOCO_GL', 'egl')
import mujoco  # noqa: E402
import numpy as np  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
QUADRANT_TO_ACTIVATION = {0: 2, 1: 0, 2: 3, 3: 1}  # q MVL,MDL,MVR,MDR -> [MDL,MDR,MVL,MVR]


def load(path):
    lines = Path(path).read_text().splitlines()
    header = json.loads(lines[0])
    records = [json.loads(line) for line in lines[1:]]
    return header, records


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('trace')
    ap.add_argument('output')
    ap.add_argument('--model', default=str(ROOT / 'sim/content/embodiments/generated/celegans.xml'))
    ap.add_argument('--width', type=int, default=1280)
    ap.add_argument('--height', type=int, default=720)
    ap.add_argument('--view', choices=('follow', 'plate'), default='follow')
    ap.add_argument('--title', default='')
    args = ap.parse_args()
    header, records = load(args.trace)
    if not records or 'qpos' not in records[0]:
        raise SystemExit('trace has no qpos: record it with the aarnn-mujoco-trace-v2 adapter')
    every = int(header.get('trace_every_steps', 25))
    dt = 0.002
    fps = 1.0 / (every * dt)
    model = mujoco.MjModel.from_xml_path(args.model)
    data = mujoco.MjData(model)
    if len(records[0]['qpos']) != model.nq:
        raise SystemExit(f'trace nq {len(records[0]["qpos"])} != model nq {model.nq}')
    model.vis.global_.offwidth = max(model.vis.global_.offwidth, args.width)
    model.vis.global_.offheight = max(model.vis.global_.offheight, args.height)
    renderer = mujoco.Renderer(model, args.height, args.width)
    quadrants = []
    for g in range(model.ngeom):
        name = mujoco.mj_id2name(model, mujoco.mjtObj.mjOBJ_GEOM, g) or ''
        if '_muscle_quadrant_' in name:
            seg = int(name.split('_')[1])
            q = int(name.rsplit('_', 1)[1])
            quadrants.append((g, 24 - seg, QUADRANT_TO_ACTIVATION[q]))
    cuticle = {}
    for g in range(model.ngeom):
        name = mujoco.mj_id2name(model, mujoco.mjtObj.mjOBJ_GEOM, g) or ''
        if name.startswith('segment_') and name.endswith('_collision'):
            cuticle[g] = 24 - int(name.split('_')[1])
    base_rgba = model.geom_rgba.copy()
    head = mujoco.mj_name2id(model, mujoco.mjtObj.mjOBJ_BODY, 'segment_23')
    cam = mujoco.MjvCamera()
    cam.type = mujoco.mjtCamera.mjCAMERA_FREE
    opt = mujoco.MjvOption()
    ffmpeg = shutil.which('ffmpeg')
    if not ffmpeg:
        raise SystemExit('ffmpeg not found')
    out = Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)
    label = (args.title + '  ').replace(':', r'\:').replace("'", '')
    draw = (f"drawtext=fontcolor=white:fontsize=26:box=1:boxcolor=black@0.45:x=18:y=18:"
            f"text='{label}t = %{{pts\\:hms}} sim time (1\\:1)  red ventral / blue dorsal'")
    proc = subprocess.Popen([ffmpeg, '-hide_banner', '-loglevel', 'error', '-y', '-f', 'rawvideo', '-pix_fmt', 'rgb24',
                             '-s', f'{args.width}x{args.height}', '-r', f'{fps:.6f}', '-i', '-', '-vf', draw,
                             '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-crf', '20', str(out)], stdin=subprocess.PIPE)
    trail = []
    for rec in records:
        data.qpos[:] = rec['qpos']
        data.qvel[:] = 0
        mujoco.mj_forward(model, data)
        act = rec.get('muscle_activation')
        model.geom_rgba[:] = base_rgba
        if act:
            a = np.asarray(act).reshape(24, 4)
            for g, row, col in quadrants:
                x = float(np.clip(a[row - 1, col] * 2.2, 0, 1))
                model.geom_rgba[g] = [0.25 + 0.75 * x, 0.25 * (1 - x), 0.2 * (1 - x), 1]
            # Cuticle tint: ventral (+y, red) minus dorsal (-y, blue) drive.
            for g, row in cuticle.items():
                dv = float(np.clip((a[row - 1, 2:].mean() - a[row - 1, :2].mean()) * 3.0, -1, 1))
                base = base_rgba[g]
                if dv >= 0:
                    model.geom_rgba[g] = [base[0] + (0.95 - base[0]) * dv, base[1] * (1 - 0.7 * dv), base[2] * (1 - 0.7 * dv), 1]
                else:
                    k = -dv
                    model.geom_rgba[g] = [base[0] * (1 - 0.7 * k), base[1] * (1 - 0.3 * k), base[2] + (0.95 - base[2]) * k, 1]
        p = data.xpos[head].copy()
        trail.append(p)
        if args.view == 'follow':
            com = data.xpos[1:25].mean(axis=0)
            cam.lookat[:] = [com[0], com[1], 0.0]
            cam.distance, cam.azimuth, cam.elevation = 0.75, 90.0, -62.0
        else:
            cam.lookat[:] = [0, 0, 0]
            cam.distance, cam.azimuth, cam.elevation = 1.9, 90.0, -88.0
        renderer.update_scene(data, camera=cam, scene_option=opt)
        scn = renderer.scene
        for k, q in enumerate(trail[-400::4]):
            if scn.ngeom >= scn.maxgeom:
                break
            mujoco.mjv_initGeom(scn.geoms[scn.ngeom], mujoco.mjtGeom.mjGEOM_SPHERE, [0.004, 0, 0],
                                [q[0], q[1], 0.004], np.eye(3).flatten(), [0.15, 0.55, 0.95, 0.75])
            scn.ngeom += 1
        frame = renderer.render()
        proc.stdin.write(frame.tobytes())
    proc.stdin.close()
    code = proc.wait()
    if code:
        raise SystemExit(f'ffmpeg exit {code}')
    seconds = len(records) / fps
    print(json.dumps(dict(video=str(out), frames=len(records), fps=round(fps, 3), duration_s=round(seconds, 3),
                          trace_every_steps=every, view=args.view)))


if __name__ == '__main__':
    main()
