#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Freenove hexapod obstacle learning, open loop (the NAO protocol on the hexapod).

Repeated approaches of an obstacle in front of the HC-SR04 ranger (far ->
near -> hold -> retreat -> pause) are fed to the hexapod learning network
(``build_hexapod_cpg_network_json.py --learn-groups G``) through the probe;
the learning-turn cell LT's onset distance is measured per approach. The
innate near turn (P_sonar at 0.40 m) teaches LT through the LC cell.
Output and summary match ``nao_learn_openloop.py`` (summary.json with
per-episode ``onset_m``, ``hidden_grown``), so the growth-learning harness
can drive either body.

  python3 scripts/qa/hexapod_learn_openloop.py NET OUTDIR [--episodes 24] [--set k=v ...] [--export-after F]
"""
import argparse
import json
import os
import subprocess
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
PROBE = ROOT / "target/release/examples/celegans_depth_probe"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("network")
    ap.add_argument("out")
    ap.add_argument("--episodes", type=int, default=24)
    ap.add_argument("--approach-s", type=float, default=20.0)
    ap.add_argument("--hold-s", type=float, default=4.0)
    ap.add_argument("--retreat-s", type=float, default=4.0)
    ap.add_argument("--pause-s", type=float, default=2.0)
    ap.add_argument("--far-m", type=float, default=2.2)
    ap.add_argument("--near-m", type=float, default=0.30)
    ap.add_argument("--set", action="append", default=[])
    ap.add_argument("--export-after")
    ap.add_argument("--seed", type=int, default=1)
    a = ap.parse_args()
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    io = json.loads((ROOT / "sim/content/embodiments/generated/hexapod.json").read_text())["io"]
    names = [s["name"] for s in io["sensors"]]
    sonar = [i for i, s in enumerate(io["sensors"]) if s["kind"] == "legacy-range"]
    reach = io["sensors"][sonar[0]].get("angle_rad") or 3.0
    ep_s = a.approach_s + a.hold_s + a.retreat_s + a.pause_s
    total_s = 3.0 + a.episodes * ep_s
    sched = []
    t, dt = 0.0, 0.02
    while t < total_s:
        e, d = -1, a.far_m + 0.3
        if t >= 3.0:
            e = int((t - 3.0) // ep_s)
            u = (t - 3.0) - e * ep_s
            if u < a.approach_s:
                d = a.far_m - (a.far_m - a.near_m) * u / a.approach_s
            elif u < a.approach_s + a.hold_s:
                d = a.near_m
            elif u < a.approach_s + a.hold_s + a.retreat_s:
                d = a.near_m + (a.far_m - a.near_m) * (u - a.approach_s - a.hold_s) / a.retreat_s
        sched.append((t, min(d, reach), e))
        t += dt
    inp = out / "input.jsonl"
    with open(inp, "w") as f:
        f.write(json.dumps({"profile": "hexapod", "sensor_names": names, "output_names": io["output_names"]}) + "\n")
        for k, (t, d, _) in enumerate(sched):
            s = [0.5] * len(names)
            for i in sonar:
                s[i] = 1.0 - d / reach
            f.write(json.dumps({"time_s": round(t, 3), "step": k * 10, "sensors": s}) + "\n")
    cmd = [str(PROBE), "--network", str(Path(a.network).resolve()), "--ms", str(int(total_s * 1000)), "--input", str(inp),
           "--out", str(out / "o.json"), "--series", str(out / "s.jsonl"), "--series-ms", "50", "--no-raster", "--seed", str(a.seed)]
    for kv in a.set:
        cmd += ["--set", kv]
    if a.export_after:
        cmd += ["--export-after", str(Path(a.export_after).resolve())]
    p = subprocess.run(cmd, cwd=ROOT, env=dict(os.environ, NM_DISABLE_OPENCL="1", RAYON_NUM_THREADS="1"),
                       capture_output=True, text=True)
    if p.returncode:
        raise SystemExit(p.stderr[-1500:])
    o = json.loads((out / "o.json").read_text())
    hn = o["hidden_names"]
    ser = [json.loads(line) for line in open(out / "s.jsonl")]
    n_h = min(len(r["hidden_hz"]) for r in ser)
    T = np.array([r["t_ms"] for r in ser]) / 1000.0
    lt = np.array([r["hidden_hz"][hn.index("LT")] for r in ser])
    st_ = np.array([x[0] for x in sched])
    dist = np.interp(T, st_, [x[1] for x in sched])
    epi = np.interp(T, st_, [x[2] for x in sched]).round().astype(int)
    peak = max(lt.max(), 1e-9)
    res = []
    for e in range(a.episodes):
        m = epi == e
        u = (T - 3.0) - e * ep_s
        app = m & (u < a.approach_s)
        on = np.flatnonzero(app & (lt > 0.25 * peak))
        res.append(dict(episode=e, side="f", onset_m=float(dist[on[0]]) if on.size else None,
                        lt_mean_approach=float(lt[app].mean()) if app.any() else 0.0,
                        lt_peak=float(lt[m].max()) if m.any() else 0.0))
    grown = max(len(r["hidden_hz"]) for r in ser) - len(hn)
    summary = dict(network=a.network, episodes=res, peak_hz=float(peak), hidden_initial=len(hn), hidden_grown=grown,
                   overrides=a.set, hidden_min=n_h)
    (out / "summary.json").write_text(json.dumps(summary, indent=1))
    for r in res:
        print(f"episode {r['episode']:2d}: LT onset at {('%.2f m' % r['onset_m']) if r['onset_m'] else 'near reflex only'}")


if __name__ == "__main__":
    main()
