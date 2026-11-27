#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Open-loop check of NAO learning by experience: scripted wall approaches
(alternating sides) through the depth probe; reports, per approach, the
distance at which the learning-turn cell LT on the obstacle side switches on.

  python3 scripts/qa/nao_learn_openloop.py NETWORK OUT_DIR [--episodes 12]
"""
import argparse
import json
import os
import subprocess
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
PROBE = ROOT / "target/release/examples/celegans_depth_probe"
_IO = json.loads((ROOT / "sim/content/embodiments/generated/nao.json").read_text())["io"]
_SONAR = next(x for x in _IO["sensors"] if x["name"] == "Sonar/Left")
REACH, MIN_M = _SONAR["angle_rad"], _SONAR.get("min_m") or 0.0   # NAO V6: 0.8 m, 0.2 m


def sonar(d):
    return 1.0 - min(max(d, MIN_M), REACH) / REACH


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("network")
    ap.add_argument("out")
    ap.add_argument("--episodes", type=int, default=12)
    ap.add_argument("--approach-s", type=float, default=20.0)
    ap.add_argument("--hold-s", type=float, default=4.0)
    ap.add_argument("--retreat-s", type=float, default=4.0)
    ap.add_argument("--pause-s", type=float, default=2.0)
    ap.add_argument("--far-m", type=float, default=2.2)
    ap.add_argument("--near-m", type=float, default=0.35)
    ap.add_argument("--set", action="append", default=[], help="probe parameter override key=value")
    ap.add_argument("--export-after", help="write the network with its learned weights after the run")
    ap.add_argument("--seed", type=int, default=1)
    a = ap.parse_args()
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    io = json.loads((ROOT / "sim/content/embodiments/generated/nao.json").read_text())["io"]
    names = [s["name"] for s in io["sensors"]]
    trace = next((ROOT / "target/qa/showcase/nao/trace").glob("*.jsonl"))
    rows = [json.loads(l) for l in open(trace)][1:]
    rows = [r for r in rows if r.get("sensors")]
    base = np.median(np.array([r["sensors"] for r in rows[len(rows) // 3:]], float), axis=0)
    # The baseline trace may predate optional sensor groups (hearing): pad
    # them silent, interaural time differences at 0.5 (straight ahead).
    if len(base) < len(names):
        pad = [0.5 if n.startswith("Micro/ITD") else 0.0 for n in names[len(base):]]
        base = np.concatenate([base, pad])
    # The baseline trace predates the toe bumpers (its "bumpers" were half-
    # sole contacts with the floor): standing and walking press none.
    for i, n in enumerate(names):
        if "Bumper" in n:
            base[i] = 0.0
    il, ir = names.index("Sonar/Left"), names.index("Sonar/Right")
    ep_s = a.approach_s + a.hold_s + a.retreat_s + a.pause_s
    total_s = 3.0 + a.episodes * ep_s
    sched = []   # (t, d_left, d_right, episode, side)
    t = 0.0
    dt = 0.02
    while t < total_s:
        e, d_obs, side = -1, a.far_m + 0.3, None
        if t >= 3.0:
            e = int((t - 3.0) // ep_s)
            u = (t - 3.0) - e * ep_s
            side = "l" if e % 2 == 0 else "r"
            if u < a.approach_s:
                d_obs = a.far_m - (a.far_m - a.near_m) * u / a.approach_s
            elif u < a.approach_s + a.hold_s:
                d_obs = a.near_m
            elif u < a.approach_s + a.hold_s + a.retreat_s:
                d_obs = a.near_m + (a.far_m - a.near_m) * (u - a.approach_s - a.hold_s) / a.retreat_s
            else:
                d_obs = a.far_m + 0.3
        dl = d_obs if side == "l" else REACH
        dr = d_obs if side == "r" else REACH
        sched.append((t, dl, dr, e, side))
        t += dt
    inp = out / "input.jsonl"
    with open(inp, "w") as f:
        f.write(json.dumps({"profile": "nao", "sensor_names": names, "output_names": io["output_names"]}) + "\n")
        for k, (t, dl, dr, _, _) in enumerate(sched):
            s = base.copy()
            s[il] = sonar(dl)
            s[ir] = sonar(dr)
            f.write(json.dumps({"time_s": round(t, 3), "step": k * 10, "sensors": [round(float(x), 5) for x in s]})
                    + "\n")
    ms = int(total_s * 1000)
    cmd = [str(PROBE), "--network", a.network, "--ms", str(ms), "--input", str(inp), "--out", str(out / "o.json"),
           "--series", str(out / "s.jsonl"), "--series-ms", "50", "--no-raster", "--seed", str(a.seed), "--watch-rows", "LT_l,LT_r"]
    for kv in a.set:
        cmd += ["--set", kv]
    if a.export_after:
        cmd += ["--export-after", str(Path(a.export_after).resolve())]
    env = dict(os.environ, NM_DISABLE_OPENCL="1", RAYON_NUM_THREADS="1")
    p = subprocess.run(cmd, cwd=ROOT, env=env, capture_output=True, text=True)
    if p.returncode:
        raise SystemExit(p.stderr[-1500:])
    o = json.loads((out / "o.json").read_text())
    hn = o["hidden_names"]
    ser = [json.loads(l) for l in open(out / "s.jsonl")]
    T = np.array([r["t_ms"] for r in ser]) / 1000.0
    # With growth on the hidden layer grows during the run: the designed
    # cells keep their indices, newborns are appended after them.
    n_h = min(len(r["hidden_hz"]) for r in ser)
    H = np.array([r["hidden_hz"][:n_h] for r in ser], float)
    grown = max(len(r["hidden_hz"]) for r in ser) - len(hn)
    lt = {s: H[:, hn.index(f"LT_{s}")] for s in "lr"}
    # interpolate the obstacle distance on the series clock
    st = np.array([x[0] for x in sched])
    dist = {"l": np.interp(T, st, [x[1] for x in sched]), "r": np.interp(T, st, [x[2] for x in sched])}
    epi = np.interp(T, st, [x[3] for x in sched]).round().astype(int)
    peak = max(lt["l"].max(), lt["r"].max(), 1e-9)
    res = []
    for e in range(a.episodes):
        side = "l" if e % 2 == 0 else "r"
        m = (epi == e)
        u = (T - 3.0) - e * (a.approach_s + a.hold_s + a.retreat_s + a.pause_s)
        app = m & (u < a.approach_s)
        on = np.flatnonzero(app & (lt[side] > 0.25 * peak))
        onset_d = float(dist[side][on[0]]) if on.size else None
        res.append(dict(episode=e, side=side, onset_m=onset_d,
                        lt_mean_approach=float(lt[side][app].mean()) if app.any() else 0.0,
                        lt_peak=float(lt[side][m].max()) if m.any() else 0.0))
    summary = dict(network=a.network, plastic=json.loads(Path(a.network).read_text())["net"].get(
        "aarnn_plasticity_enabled"), episodes=res, ms_per_frame=o.get("ms_per_frame"), peak_hz=peak,
        hidden_initial=len(hn), hidden_grown=grown, overrides=a.set)
    (out / "summary.json").write_text(json.dumps(summary, indent=1))
    for r in res:
        print(f"episode {r['episode']:2d} {r['side']}: LT onset at "
              f"{('%.2f m' % r['onset_m']) if r['onset_m'] else 'near reflex only'}  "
              f"(mean LT during approach {r['lt_mean_approach']:.2f} Hz)")


if __name__ == "__main__":
    main()
