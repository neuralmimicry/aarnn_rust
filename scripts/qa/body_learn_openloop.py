#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Teacher-gated anticipation learning, open loop, for any body built with
``aarnn_circuit.add_learned_anticipation``.

The learner's ``connectome_labels["learning"]`` names the profile, the range
channels per side, the code (``encoding``: "inverse" = 1 - d / reach,
"direct" = d / reach) and the teacher / group distances. An obstacle
approaches one side per episode (sides alternate): far -> near -> hold ->
retreat -> pause; every other sensor sits at 0.5. Per episode the side's
learning-turn cell LT_side onset distance is measured; output matches
``nao_learn_openloop.py`` (summary.json, ``hidden_grown``), so
``growth_learning_nao.py --body generic`` drives it.

  python3 scripts/qa/body_learn_openloop.py NET OUTDIR [--episodes 24] [--set k=v ...] [--export-after F] [--seed S]
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
    ap.add_argument("--set", action="append", default=[])
    ap.add_argument("--export-after")
    ap.add_argument("--seed", type=int, default=1)
    a = ap.parse_args()
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    snap = json.loads(Path(a.network).read_text())
    meta = snap["connectome_labels"]["learning"]
    profile, enc, reach = meta["profile"], meta["encoding"], float(meta["reach_m"])
    io = json.loads((ROOT / f"sim/content/embodiments/generated/{profile}.json").read_text())["io"]
    names = [s["name"] for s in io["sensors"]]
    sides = meta["channels"]
    idx = {s: [names.index(ch) for ch in chans] for s, chans in sides.items()}
    code = (lambda d: 1.0 - min(d, reach) / reach) if enc == "inverse" else (lambda d: min(d, reach) / reach)
    far, near = float(meta["protocol_far_m"]), float(meta["protocol_near_m"])
    ep_s = a.approach_s + a.hold_s + a.retreat_s + a.pause_s
    total_s = 3.0 + a.episodes * ep_s
    order = sorted(sides)
    sched = []
    t, dt = 0.0, 0.02
    while t < total_s:
        e, d, side = -1, reach * 1.2, None
        if t >= 3.0:
            e = int((t - 3.0) // ep_s)
            u = (t - 3.0) - e * ep_s
            side = order[e % len(order)]
            if u < a.approach_s:
                d = far - (far - near) * u / a.approach_s
            elif u < a.approach_s + a.hold_s:
                d = near
            elif u < a.approach_s + a.hold_s + a.retreat_s:
                d = near + (far - near) * (u - a.approach_s - a.hold_s) / a.retreat_s
            else:
                d = reach * 1.2
        sched.append((t, d, e, side))
        t += dt
    inp = out / "input.jsonl"
    with open(inp, "w") as f:
        f.write(json.dumps({"profile": profile, "sensor_names": names, "output_names": io["output_names"]}) + "\n")
        for k, (t, d, _, side) in enumerate(sched):
            # A channel shared by two sides (the front ray) sees the obstacle
            # when it belongs to the approached side; it reads far only when
            # it belongs to no approached side (it was last-side-wins, so
            # left approaches lost their front teacher, 25 Nov).
            s = [0.5] * len(names)
            for ii in idx.values():
                for i in ii:
                    s[i] = code(reach * 1.2)
            if side is not None:
                for i in idx[side]:
                    s[i] = code(d)
            f.write(json.dumps({"time_s": round(t, 3), "step": k * 10, "sensors": s}) + "\n")
    cmd = [str(PROBE), "--network", str(Path(a.network).resolve()), "--ms", str(int(total_s * 1000)), "--input", str(inp),
           "--out", str(out / "o.json"), "--series", str(out / "s.jsonl"), "--series-ms", "50", "--no-raster",
           "--seed", str(a.seed)]
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
    T = np.array([r["t_ms"] for r in ser]) / 1000.0
    lt = {s: np.array([r["hidden_hz"][hn.index(f"LT_{s}")] for r in ser]) for s in sides}
    st_ = np.array([x[0] for x in sched])
    dist = np.interp(T, st_, [x[1] for x in sched])
    epi = np.interp(T, st_, [x[2] for x in sched]).round().astype(int)
    peak = max(max(v.max() for v in lt.values()), 1e-9)
    res = []
    for e in range(a.episodes):
        side = order[e % len(order)]
        m = epi == e
        u = (T - 3.0) - e * ep_s
        app = m & (u < a.approach_s)
        on = np.flatnonzero(app & (lt[side] > 0.25 * peak))
        res.append(dict(episode=e, side=side, onset_m=float(dist[on[0]]) if on.size else None,
                        lt_mean_approach=float(lt[side][app].mean()) if app.any() else 0.0,
                        lt_peak=float(lt[side][m].max()) if m.any() else 0.0))
    grown = max(len(r["hidden_hz"]) for r in ser) - len(hn)
    summary = dict(network=a.network, profile=profile, episodes=res, peak_hz=float(peak), hidden_initial=len(hn),
                   hidden_grown=grown, overrides=a.set, protocol=dict(far_m=far, near_m=near, approach_s=a.approach_s))
    (out / "summary.json").write_text(json.dumps(summary, indent=1))
    for r in res:
        print(f"episode {r['episode']:2d} {r['side']}: LT onset at {('%.3f m' % r['onset_m']) if r['onset_m'] else 'near reflex only'}")


if __name__ == "__main__":
    main()
