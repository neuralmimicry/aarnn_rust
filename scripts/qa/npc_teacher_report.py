#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""NPC-teacher experiment, analysis: a standing NAO learner while a
programmed NPC actor (NM_MUJOCO_NPC_ACTOR) walks up to it repeatedly.

1. The closed-loop run's recorded sensory stream (the body's own sonars,
   cameras, joints) is replayed through the learner network twice with
   ``celegans_depth_probe``: plastic (``--export-after`` saves the trained
   network) and frozen (same experience, no plasticity).
2. Per NPC approach (from the actor script's protocol): the distance at which
   the obstacle-side learning-turn cell LT switches on (same absolute
   threshold for both replays: half the frozen reflex peak), LT activity while
   the NPC is 0.6-1.2 m away, and the summed sonar-group -> LT efficacy.
3. ``--walking-out``: the trained network with the walk command restored
   (``tonic_current_walking``), for the closed-loop walking test without NPC.

  python3 scripts/qa/npc_teacher_report.py TRAIN_RUN_DIR ACTOR_SCRIPT STAND_NETWORK FROZEN_NETWORK OUT_DIR \\
      [--walking-out NET.json]
"""
import argparse
import json
import math
import os
import subprocess
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
PROBE = ROOT / "target/release/examples/celegans_depth_probe"


def replay_input(trace: Path, out: Path):
    io = json.loads((ROOT / "sim/content/embodiments/generated/nao.json").read_text())["io"]
    names = [s["name"] for s in io["sensors"]]
    actor = []
    with open(trace) as f, open(out, "w") as g:
        f.readline()
        g.write(json.dumps({"profile": "nao", "sensor_names": names, "output_names": io["output_names"]}) + "\n")
        for line in f:
            r = json.loads(line)
            if not r.get("sensors") or not r.get("qpos"):
                continue
            g.write(json.dumps({"time_s": r["time_s"], "step": r["step"], "sensors": r["sensors"]}) + "\n")
            q = r["qpos"]
            w, x, y, z = q[3:7]
            yaw = math.atan2(2 * (w * z + x * y), 1 - 2 * (y * y + z * z))
            chest = (q[0] + 0.05 * math.cos(yaw), q[1] + 0.05 * math.sin(yaw))
            a = (r.get("npc_actor") or {}).get("pos")
            actor.append((r["time_s"], math.hypot(a[0] - chest[0], a[1] - chest[1]) if a else float("nan")))
    return np.array(actor)


def run_probe(network, inp, ms, out_dir, tag, export=None):
    cmd = [str(PROBE), "--network", str(network), "--ms", str(ms), "--input", str(inp), "--out", str(out_dir / f"{tag}_o.json"),
           "--series", str(out_dir / f"{tag}_s.jsonl"), "--series-ms", "50", "--no-raster", "--seed", "1",
           "--watch-rows", "LT_l,LT_r"]
    if export:
        cmd += ["--export-after", str(export)]
    env = dict(os.environ, NM_DISABLE_OPENCL="1", RAYON_NUM_THREADS="1")
    p = subprocess.run(cmd, cwd=ROOT, env=env, capture_output=True, text=True)
    if p.returncode:
        raise SystemExit(p.stderr[-1500:])
    o = json.loads((out_dir / f"{tag}_o.json").read_text())
    ser = [json.loads(l) for l in open(out_dir / f"{tag}_s.jsonl")]
    return o["hidden_names"], np.array([r["t_ms"] for r in ser]) / 1000.0, np.array([r["hidden_hz"] for r in ser]), \
        [r.get("weights") or {} for r in ser]


def episodes(hn, T, H, W, dist_t, dist, proto, thr):
    out = []
    period = proto["approach"] + proto["hold"] + proto["retreat"] + proto["pause"]
    d = np.interp(T, dist_t, dist)
    for e in range(proto["episodes"]):
        side = "l" if e % 2 == 0 else "r"
        lt = H[:, hn.index(f"LT_{side}")]
        u = T - proto["t0"] - e * period
        app = (u >= 0) & (u < proto["approach"])
        on = np.flatnonzero(app & (lt > thr))
        far = app & (d >= 0.6) & (d <= 1.2)
        end = np.flatnonzero(app)
        w = W[end[-1]].get(f"LT_{side}", {}) if end.size else {}
        cs = [v for k, v in w.items() if k.startswith("CS_")]
        out.append(dict(episode=e, side=side, onset_m=round(float(d[on[0]]), 3) if on.size else None,
                        lt_far_hz=round(float(lt[far].mean()), 3) if far.any() else 0.0,
                        w_sum=round(float(sum(cs)), 4)))
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("train_run", type=Path)
    ap.add_argument("actor_script", type=Path)
    ap.add_argument("stand_network", type=Path)
    ap.add_argument("frozen_network", type=Path)
    ap.add_argument("out", type=Path)
    ap.add_argument("--walking-out", type=Path)
    a = ap.parse_args()
    a.out.mkdir(parents=True, exist_ok=True)
    proto = json.loads(a.actor_script.read_text())["protocol"]
    inp = a.out / "replay_input.jsonl"
    actor = replay_input(next((a.train_run / "trace").glob("*.jsonl")), inp)
    ms = int(actor[-1, 0] * 1000)
    trained = a.out / "network_trained_stand.json"
    res = {}
    for tag, net, export in (("frozen", a.frozen_network, None), ("plastic", a.stand_network, trained)):
        res[tag] = run_probe(net, inp, ms, a.out, tag, export)
    hn, T, H, _ = res["frozen"]
    peak = max(H[:, hn.index("LT_l")].max(), H[:, hn.index("LT_r")].max())
    thr = 0.5 * peak
    report = dict(threshold_hz=thr, frozen_peak_hz=peak, protocol=proto, conditions={})
    for tag in ("plastic", "frozen"):
        report["conditions"][tag] = episodes(*res[tag], actor[:, 0], actor[:, 1], proto, thr)
    (a.out / "report.json").write_text(json.dumps(report, indent=1))
    print(f"threshold {thr:.2f} Hz (half the frozen reflex peak {peak:.2f} Hz)")
    for tag, eps in report["conditions"].items():
        print(f"\n## {tag}\nep side onset_m  LT@0.6-1.2m  w_sum")
        for r in eps:
            print(f"{r['episode']:2d}  {r['side']}   {str(r['onset_m']):6s}  {r['lt_far_hz']:7.2f}    {r['w_sum']:.4f}")
    if a.walking_out:
        net = json.loads(trained.read_text())
        meta = (net.get("connectome_labels") or {}).get("npc_teacher") or {}
        restored = 0
        for t in net["net"].get("neuron_types") or []:
            if t.get("name") == meta.get("walk_type"):
                t["bio_params"]["tonic_current"] = meta["walk_tonic"]
                restored += 1
        if restored != 1:
            raise SystemExit(f"expected one walk command to restore, found {restored}")
        net["connectome_labels"]["npc_teacher"] = dict(net["connectome_labels"].get("npc_teacher") or {},
                                                       trained_by=str(a.train_run), walking_restored=True)
        a.walking_out.write_text(json.dumps(net))
        print(f"\nwalking network with the NPC-trained synapses: {a.walking_out}")


if __name__ == "__main__":
    main()
