#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Closed-loop NAO learning by experience in the MuJoCo room (3 m x 3 m,
walls on all sides): the learning network (scripts/build_nao_learn_network_json.py)
against the same network frozen.

  run    - one lock-step MuJoCo run through scripts/run_sim.sh
           python3 scripts/qa/nao_learn_closedloop.py run OUT NETWORK CONFIG [--sim-s 800]
  report - wall encounters from the traces, in time order
           python3 scripts/qa/nao_learn_closedloop.py report DIR [DIR ...] [--json OUT]

An encounter is an approach that brings the nearer chest sonar below 0.6 m;
it starts where that sonar last read >= 1.2 m and ends when it reads
> 0.9 m (turned or backed away). Per encounter: the closest approach, the
time spent inside the innate reflex range (< 0.4 m), whether the body backed
off (> 2 cm of reverse travel along the heading), and the sonar distance at
which it had turned 10 deg since the approach began (turn onset). Learning predicts later
encounters with earlier turns, larger closest approaches and fewer back-offs;
the frozen network cannot change.
"""
import argparse
import json
import math
import os
import socket
import subprocess
import time
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
REACH = 2.55          # v1 single-ray sonar (traces recorded before the V6 body)
DT_STEP = 0.002
_RECORD = json.loads((ROOT / "sim/content/embodiments/generated/nao.json").read_text())


# Earlier NAO V6 IO contracts (0.8 m sonar), e.g. before the hearing group.
V6_IO_DIGESTS = {"7c6e767d01140c1819d444c3734560ea47a4e84c93f02a48c67e1ec5b6327a91"}


def sonar_reach(header):
    """Sonar reach of the body that recorded a trace: the current record's
    (NAO V6: 0.8 m) when the IO digests match or the digest is a known V6
    contract, else the v1 2.55 m."""
    if header.get("io_digest") in V6_IO_DIGESTS:
        return 0.8
    if header.get("io_digest") == _RECORD["io"]["digest"]:
        return next(s for s in _RECORD["io"]["sensors"] if s["name"] == "Sonar/Left")["angle_rad"]
    return REACH


def target_ball():
    o = next((o for o in _RECORD["scene"]["objects"] if o["id"] == "target_ball"), None)
    return (o["position"][:2], o["size"][0] / 2) if o else None


def run(a):
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    s = socket.socket()
    s.bind(("", 0))
    port = s.getsockname()[1]
    s.close()
    sdk = Path.home() / "Developer/mujoco-3.17.0"
    env = dict(os.environ, NM_DISABLE_OPENCL="1", RAYON_NUM_THREADS="2", NM_AUTO_AARNN_DEPTH="0",
               NM_AER_REFERENCE_DATA_TIMEOUT_MS="60000", NM_CLUSTER_NODES="1", AARNN_MUJOCO_DIR=str(sdk),
               LD_LIBRARY_PATH=str(sdk / "lib"))
    steps = int(round(a.sim_s / DT_STEP))
    cmd = ["bash", "scripts/run_sim.sh", "--sim", "mujoco", "--robots", "nao=1", "--no-build", "--tcp-base-port",
           str(port), "--tcp-ready-timeout", "300", "--headless", "--steps", str(steps), "--lockstep", "--output",
           str(out / "run.json"), "--trace", str(out / "trace"), "--trace-every", str(a.trace_every),
           "--network-map", f"nao_0={Path(a.network).resolve()}", "--config-map", f"nao_0={Path(a.config).resolve()}"]
    t0 = time.time()
    with open(out / "run.log", "w") as log:
        rc = subprocess.run(cmd, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT).returncode
    print(json.dumps(dict(rc=rc, wall_s=round(time.time() - t0, 1), network=a.network)))


def load(d):
    io = json.loads((ROOT / "sim/content/embodiments/generated/nao.json").read_text())["io"]
    names = [s["name"] for s in io["sensors"]]
    il, ir = names.index("Sonar/Left"), names.index("Sonar/Right")
    f = next((Path(d) / "trace").glob("*.jsonl"))
    bumpers = [i for i, n in enumerate(names) if "Bumper" in n]
    T, X, Y, Z, YAW, DL, DR, BU = [], [], [], [], [], [], [], []
    with open(f) as fh:
        reach = sonar_reach(json.loads(fh.readline()))
        for line in fh:
            try:
                r = json.loads(line)
            except json.JSONDecodeError:
                break      # a run still writing its trace: stop at the partial last line

            if not r.get("sensors") or not r.get("qpos"):
                continue
            q = r["qpos"]
            w, x, y, z = q[3:7]
            T.append(r["time_s"])
            X.append(q[0])
            Y.append(q[1])
            Z.append(q[2])
            YAW.append(math.atan2(2 * (w * z + x * y), 1 - 2 * (y * y + z * z)))
            DL.append((1 - r["sensors"][il]) * reach)
            DR.append((1 - r["sensors"][ir]) * reach)
            BU.append(max(r["sensors"][i] for i in bumpers) if bumpers else 0.0)
    out = {k: np.array(v) for k, v in dict(t=T, x=X, y=Y, z=Z, yaw=np.unwrap(YAW), dl=DL, dr=DR, bump=BU).items()}
    out["reach"] = reach
    return out


def encounters(tr, near_m=0.6, approach_m=1.2, clear_m=0.9):
    """Wall encounters: the nearer sonar dips below `near_m`; the approach
    starts where it last read >= `approach_m` (or where the previous
    encounter ended); the encounter ends when it reads > `clear_m` again.
    With the V6 sonar (nothing beyond 0.8 m) the approach starts at the last
    "nothing" reading and the encounter clears 5 cm inside the reach."""
    approach_m = min(approach_m, tr["reach"] - 1e-3)
    clear_m = min(clear_m, tr["reach"] - 0.05)
    dmin = np.minimum(tr["dl"], tr["dr"])
    out, i, n, prev_end = [], 0, len(dmin), 0
    while i < n:
        if dmin[i] >= near_m:
            i += 1
            continue
        a0 = i
        while a0 > prev_end and dmin[a0 - 1] < approach_m:
            a0 -= 1
        j = i
        while j < n:
            if dmin[j] > clear_m:
                k = j
                while k < n and dmin[k] > clear_m and tr["t"][k] - tr["t"][j] < 2.0:
                    k += 1
                if k >= n or dmin[k] > clear_m:
                    break   # clear for 2 s (or to the end of the run)
                j = k
            j += 1
        seg = slice(a0, j)
        yaw0 = tr["yaw"][a0]
        turned = np.flatnonzero(np.abs(tr["yaw"][seg] - yaw0) > math.radians(10.0))
        dx, dy = np.diff(tr["x"][seg]), np.diff(tr["y"][seg])
        fwd = dx * np.cos(tr["yaw"][seg][:-1]) + dy * np.sin(tr["yaw"][seg][:-1])
        cs = np.cumsum(fwd)
        rev = float(-(cs - np.maximum.accumulate(cs)).min()) if fwd.size else 0.0   # longest reverse run
        near = np.flatnonzero(dmin[seg] < 0.4)
        side = "l" if tr["dl"][seg].min() < tr["dr"][seg].min() else "r"
        out.append(dict(t_start=round(float(tr["t"][a0]), 1), t_near=round(float(tr["t"][i]), 1),
                        t_end=round(float(tr["t"][min(j, n - 1)]), 1), side=side,
                        closest_m=round(float(dmin[seg].min()), 3),
                        turn_onset_m=round(float(dmin[a0 + turned[0]]), 3) if turned.size else None,
                        s_in_reflex_zone=round(float(near.size * np.median(np.diff(tr["t"]))), 1),
                        backed_off=bool(rev > 0.02), reverse_m=round(rev, 3), complete=bool(j < n)))
        prev_end = j
        i = j + 1
    return out


def report(a):
    res = {}
    for d in a.dirs:
        tr = load(d)
        enc = encounters(tr)
        fell = bool((tr["z"] < 0.2).any())
        t, x, y = tr["t"], tr["x"], tr["y"]
        dt = float(np.median(np.diff(t)))
        w = max(1, int(round(20.0 / dt)))
        # stalled: net displacement over the next 20 s below 0.10 m
        stalled = float((np.hypot(x[w:] - x[:-w], y[w:] - y[:-w]) < 0.10).mean() * 100) if len(t) > w else None
        dmin = np.minimum(tr["dl"], tr["dr"])
        ball = target_ball()
        at_ball = (float(((np.hypot(x - ball[0][0], y - ball[0][1]) - ball[1]) < 0.15).mean() * 100)
                   if ball else None)
        res[str(d)] = dict(sim_s=float(t[-1]), fell=fell, path_m=float(np.sum(np.hypot(np.diff(x), np.diff(y)))),
                           sonar_reach_m=tr["reach"], in_reflex_pct=float((dmin < 0.4).mean() * 100),
                           stalled_pct=stalled, at_target_ball_pct=at_ball,
                           bumper_pressed_pct=float((tr["bump"] > 0.5).mean() * 100),
                           cells_visited=int(len({(int(np.floor(a / 0.25)), int(np.floor(b / 0.25)))
                                                  for a, b in zip(x, y)})),
                           encounters=enc)
        r = res[str(d)]
        print(f"\n## {d}  ({t[-1]:.0f} s simulated, fell={fell}, {len(enc)} encounters; "
              f"reflex zone {r['in_reflex_pct']:.1f} %, stalled {r['stalled_pct'] or 0:.1f} %, at target ball "
              f"{r['at_target_ball_pct'] or 0:.1f} %, bumper pressed {r['bumper_pressed_pct']:.1f} %, "
              f"{r['cells_visited']} 0.25 m cells)")
        print(" #  t(s)    side closest  turn@   s<0.4m  back-off")
        for k, e in enumerate(enc):
            print(f"{k:2d} {e['t_start']:6.1f}   {e['side']}   {e['closest_m']:.2f}   "
                  f"{str(e['turn_onset_m']):5s}  {e['s_in_reflex_zone']:6.1f}  "
                  f"{'yes' if e['backed_off'] else 'no '} ({e['reverse_m']:.2f} m)"
                  + ("" if e["complete"] else "  [run ended]"))
    if a.json:
        Path(a.json).write_text(json.dumps(res, indent=1))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("out")
    r.add_argument("network")
    r.add_argument("config")
    r.add_argument("--sim-s", type=float, default=800.0)
    r.add_argument("--trace-every", type=int, default=50)
    p = sub.add_parser("report")
    p.add_argument("dirs", nargs="+")
    p.add_argument("--json")
    a = ap.parse_args()
    run(a) if a.cmd == "run" else report(a)


if __name__ == "__main__":
    main()
