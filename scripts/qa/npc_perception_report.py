#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""What does the NAO perceive of a programmed NPC actor sharing its MuJoCo
world (NM_MUJOCO_NPC_ACTOR)? From a closed-loop trace: the actor's bearing
and distance from the learner's chest, the chest sonars, and retinal ON/OFF
activity per camera column, binned by the actor's bearing.

  python3 scripts/qa/npc_perception_report.py RUN_DIR [--json OUT]
"""
import argparse
import json
import math
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
REACH = 2.55


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("run_dir", type=Path)
    ap.add_argument("--json")
    a = ap.parse_args()
    io = json.loads((ROOT / "sim/content/embodiments/generated/nao.json").read_text())["io"]
    names = [s["name"] for s in io["sensors"]]
    il, ir = names.index("Sonar/Left"), names.index("Sonar/Right")
    cams = {}
    for i, s in enumerate(io["sensors"]):
        if s["kind"] == "legacy-camera":
            cams.setdefault((s["camera"], s["cell"][1]), []).append(i)
    rows = []
    for line in open(next((a.run_dir / "trace").glob("*.jsonl"))):
        r = json.loads(line)
        if not r.get("qpos") or not r.get("sensors") or not r.get("npc_actor"):
            continue
        q = r["qpos"]
        w, x, y, z = q[3:7]
        yaw = math.atan2(2 * (w * z + x * y), 1 - 2 * (y * y + z * z))
        ax, ay = r["npc_actor"]["pos"][:2]
        bearing = math.degrees(math.atan2(ay - q[1], ax - q[0]) - yaw)
        bearing = (bearing + 180) % 360 - 180
        rows.append(dict(t=r["time_s"], dist=math.hypot(ax - q[0], ay - q[1]), bearing=bearing,
                         dl=(1 - r["sensors"][il]) * REACH, dr=(1 - r["sensors"][ir]) * REACH,
                         cols={k: float(np.mean([r["sensors"][i] for i in v])) for k, v in cams.items()}))
    if not rows:
        raise SystemExit("no trace rows with npc_actor")
    out = dict(rows=len(rows), bins=[])
    print("bearing  dist   sonarL  sonarR   CameraTop columns 0..7 (mean ON/OFF activity)")
    for lo in range(-60, 60, 10):
        sel = [r for r in rows if lo <= r["bearing"] < lo + 10]
        if not sel:
            continue
        top = [float(np.mean([r["cols"][("CameraTop", c)] for r in sel])) for c in range(8)]
        b = dict(bearing=lo + 5, n=len(sel), dist=float(np.mean([r["dist"] for r in sel])),
                 sonar_left=float(np.mean([r["dl"] for r in sel])), sonar_right=float(np.mean([r["dr"] for r in sel])),
                 camera_top_columns=top)
        out["bins"].append(b)
        print(f"{lo + 5:+4d}  {b['dist']:5.2f}  {b['sonar_left']:6.2f}  {b['sonar_right']:6.2f}   "
              + " ".join(f"{v:.3f}" for v in top))
    if a.json:
        Path(a.json).write_text(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
