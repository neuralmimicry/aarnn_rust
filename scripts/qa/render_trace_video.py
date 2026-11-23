#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Frame-by-frame replay video of any native MuJoCo trace.

Profile-agnostic counterpart of ``render_celegans_trace_video.py``: each
recorded ``qpos`` is restored with ``mj_forward`` (no re-simulation) and
rendered offscreen with a camera that follows the root body, so a
slower-than-real-time lock-step run replays at true simulation speed. The
root's path is drawn as a trail on the ground and the overlay shows
simulation time.

  MUJOCO_GL=egl target/robot-mujoco-venv/bin/python \\
    scripts/qa/render_trace_video.py TRACE.jsonl OUT.mp4 [--model generated/<profile>.xml]
"""
import argparse
import json
import os
import shutil
import subprocess
from pathlib import Path

os.environ.setdefault('MUJOCO_GL', 'egl')
import mujoco  # noqa: E402
import numpy as np  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('trace')
    ap.add_argument('output')
    ap.add_argument('--model', help='MJCF (default: sim/content/embodiments/generated/<profile>.xml)')
    ap.add_argument('--width', type=int, default=1280)
    ap.add_argument('--height', type=int, default=720)
    ap.add_argument('--distance', type=float, default=None, help='camera distance (default: 3 body lengths)')
    ap.add_argument('--elevation', type=float, default=-30.0)
    ap.add_argument('--azimuth', type=float, default=135.0)
    ap.add_argument('--title', default='')
    ap.add_argument('--fixed', metavar='X,Y,Z', help='fixed camera look-at point instead of following the root body')
    ap.add_argument('--lookat-height', type=float, default=0.5,
                    help='followed look-at height as a fraction of the root height (1 keeps a flier centred)')
    args = ap.parse_args()
    lines = Path(args.trace).read_text().splitlines()
    header = json.loads(lines[0])
    records = [json.loads(x) for x in lines[1:]]
    records = [r for r in records if r.get('qpos')]
    if not records:
        raise SystemExit('trace has no qpos (record with aarnn-mujoco --trace, schema v2)')
    model_path = args.model or str(ROOT / f"sim/content/embodiments/generated/{header['profile']}.xml")
    model = mujoco.MjModel.from_xml_path(model_path)
    data = mujoco.MjData(model)
    if len(records[0]['qpos']) != model.nq:
        raise SystemExit(f"trace nq {len(records[0]['qpos'])} != model nq {model.nq}")
    model.vis.global_.offwidth = max(model.vis.global_.offwidth, args.width)
    model.vis.global_.offheight = max(model.vis.global_.offheight, args.height)
    renderer = mujoco.Renderer(model, args.height, args.width)
    every = int(header.get('trace_every_steps', 10))
    fps = 1.0 / (every * 0.002)
    data.qpos[:] = records[0]['qpos']
    mujoco.mj_forward(model, data)
    xyz = np.array([data.xpos[b] for b in range(1, model.nbody)])
    length = float(max(np.ptp(xyz[:, 0]), np.ptp(xyz[:, 1]), 0.05))
    cam = mujoco.MjvCamera()
    cam.type = mujoco.mjtCamera.mjCAMERA_FREE
    cam.distance = args.distance or 3.0 * length
    cam.elevation, cam.azimuth = args.elevation, args.azimuth
    ffmpeg = shutil.which('ffmpeg')
    if not ffmpeg:
        raise SystemExit('ffmpeg not found')
    out = Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)
    label = (args.title or header['profile']).replace(':', r'\:').replace("'", '')
    draw = (f"drawtext=fontcolor=white:fontsize=26:box=1:boxcolor=black@0.45:x=18:y=18:"
            f"text='{label}  t = %{{pts\\:hms}} sim time (1\\:1)'")
    proc = subprocess.Popen([ffmpeg, '-hide_banner', '-loglevel', 'error', '-y', '-f', 'rawvideo', '-pix_fmt', 'rgb24',
                             '-s', f'{args.width}x{args.height}', '-r', f'{fps:.6f}', '-i', '-', '-vf', draw,
                             '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-crf', '20', str(out)], stdin=subprocess.PIPE)
    trail = []
    for rec in records:
        data.qpos[:] = rec['qpos']
        data.qvel[:] = 0
        mujoco.mj_forward(model, data)
        root = data.xpos[1].copy()
        trail.append(root)
        if args.fixed:
            cam.lookat[:] = [float(v) for v in args.fixed.split(',')]
        else:
            cam.lookat[:] = [root[0], root[1], max(root[2] * args.lookat_height, 0.0)]
        renderer.update_scene(data, camera=cam)
        scn = renderer.scene
        for q in trail[-600::6]:
            if scn.ngeom >= scn.maxgeom:
                break
            mujoco.mjv_initGeom(scn.geoms[scn.ngeom], mujoco.mjtGeom.mjGEOM_SPHERE, [0.012 * length, 0, 0],
                                [q[0], q[1], 0.003], np.eye(3).flatten(), [0.15, 0.55, 0.95, 0.75])
            scn.ngeom += 1
        proc.stdin.write(renderer.render().tobytes())
    proc.stdin.close()
    if proc.wait():
        raise SystemExit('ffmpeg failed')
    print(json.dumps(dict(video=str(out), frames=len(records), fps=round(fps, 3),
                          duration_s=round(len(records) / fps, 3), profile=header['profile'])))


if __name__ == '__main__':
    main()
