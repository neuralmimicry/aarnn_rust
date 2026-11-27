#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Open-loop check of neuromodulated transmission on any AARNN network.

Drives a network's sensory channels in blocks: baseline, then food, light,
heat and noxious, with a baseline between each. In a block, the channels
whose IO-contract modality (``aarnn_circuit.sensory_modality``) matches are
held at ``--level`` and every other channel is held at ``--rest``.

The probe (``target/release/examples/celegans_depth_probe``) runs twice:
- with neuromodulated transmission;
- without it (``aarnn_neuromod_transmission_enabled=false``).

Each run records the runtime's neuromodulator trace. The script reports
per-block mean dopamine / serotonin / noradrenaline / acetylcholine levels
and how much the output activation differs between the two runs. It also
writes ``neuromod.png`` (``plot_neuromod.py``).

  python3 scripts/qa/neuromod_stimulus_probe.py NETWORK PROFILE OUTDIR [--block-s 8] [--depth D]
"""
import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from aarnn_circuit import sensory_modality  # noqa: E402

PROBE = ROOT / "target/release/examples/celegans_depth_probe"
BLOCKS = ["baseline", "food", "baseline", "light", "baseline", "heat", "baseline", "noxious", "baseline"]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("network")
    ap.add_argument("profile", help="generated embodiment profile (sim/content/embodiments/generated/<profile>.json)")
    ap.add_argument("out")
    ap.add_argument("--block-s", type=float, default=8.0)
    ap.add_argument("--level", type=float, default=0.9)
    ap.add_argument("--rest", type=float, default=0.0)
    ap.add_argument("--depth", type=int, default=None)
    ap.add_argument("--set", action="append", default=[])
    a = ap.parse_args()
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    io = json.loads((ROOT / f"sim/content/embodiments/generated/{a.profile}.json").read_text())["io"]
    sensors = io["sensors"]
    names = [s["name"] for s in sensors]
    tags = [sensory_modality(s) for s in sensors]
    snap = json.loads(Path(a.network).read_text())
    ns = int(snap["net"]["num_sensory_neurons"])
    if ns != len(names):
        raise SystemExit(f"network has {ns} sensory cells, profile {a.profile} has {len(names)} channels")
    present = {t for t in tags if t != "other"}
    print("modalities:", {t: tags.count(t) for t in sorted(set(tags))})
    dt = 0.02
    total = a.block_s * len(BLOCKS)
    with open(out / "input.jsonl", "w") as f:
        f.write(json.dumps({"profile": a.profile, "sensor_names": names, "output_names": io["output_names"]}) + "\n")
        k, t = 0, 0.0
        while t < total:
            block = BLOCKS[min(int(t // a.block_s), len(BLOCKS) - 1)]
            v = [a.level if tag == block else a.rest for tag in tags]
            f.write(json.dumps({"time_s": round(t, 3), "step": k * 10, "sensors": v}) + "\n")
            k += 1
            t += dt
    runs = {}
    for cond, extra in (("modulated", []), ("unmodulated", ["aarnn_neuromod_transmission_enabled=false"])):
        trace = out / f"{cond}.neuromod.jsonl"
        trace.unlink(missing_ok=True)
        cmd = [str(PROBE), "--network", str(Path(a.network).resolve()), "--ms", str(int(total * 1000)),
               "--input", str(out / "input.jsonl"), "--out", str(out / f"{cond}.json"),
               "--series", str(out / f"{cond}.series.jsonl"), "--series-ms", "100", "--no-raster",
               "--set", "aarnn_sensory_modalities=" + json.dumps(tags)]
        if a.depth is not None:
            cmd += ["--depth", str(a.depth)]
        for kv in a.set + extra:
            cmd += ["--set", kv]
        env = dict(os.environ, NM_DISABLE_OPENCL="1", RAYON_NUM_THREADS="2", NM_NEUROMOD_TRACE=str(trace),
                   NM_NEUROMOD_SAMPLE_EVERY="20")
        p = subprocess.run(cmd, cwd=ROOT, env=env, capture_output=True, text=True)
        if p.returncode:
            raise SystemExit(p.stderr[-2000:])
        nm = [json.loads(line) for line in trace.read_text().splitlines() if line.strip()]
        ser = [json.loads(line) for line in (out / f"{cond}.series.jsonl").read_text().splitlines() if line.strip()]
        runs[cond] = (nm, ser)

    def block_of(t_s):
        return int(t_s // a.block_s)

    nm, _ = runs["modulated"]
    print(f"\n{'block':9s} {'DA':>6s} {'5-HT':>6s} {'NA':>6s} {'ACh':>6s} {'stimuli':>8s} {'efficacy':>8s} {'release':>8s}"
          f"  hidden Hz mod / unmod   |out mod - unmod|")
    summary = []
    for b, name in enumerate(BLOCKS):
        rows = [r for r in nm if block_of(r["t_ms"] / 1000.0) == b and (r["t_ms"] / 1000.0) % a.block_s > a.block_s * 0.5]
        if not rows:
            continue
        mean = lambda k: sum(r[k] for r in rows) / len(rows)
        s_mod = [r for r in runs["modulated"][1] if block_of(r["t_ms"] / 1000.0) == b]
        s_un = [r for r in runs["unmodulated"][1] if block_of(r["t_ms"] / 1000.0) == b]
        diff, h_mod, h_un = 0.0, 0.0, 0.0
        if s_mod and s_un:
            n = min(len(s_mod), len(s_un))
            diff = sum(sum(abs(x - y) for x, y in zip(s_mod[i]["output_activation"], s_un[i]["output_activation"]))
                       / max(1, len(s_mod[i]["output_activation"])) for i in range(n)) / n
            h_mod = sum(sum(r["hidden_hz"]) / max(1, len(r["hidden_hz"])) for r in s_mod[:n]) / n
            h_un = sum(sum(r["hidden_hz"]) / max(1, len(r["hidden_hz"])) for r in s_un[:n]) / n
        row = dict(block=name, dopamine=mean("dopamine"), serotonin=mean("serotonin"), noradrenaline=mean("noradrenaline"),
                   acetylcholine=mean("acetylcholine"), stimuli=mean("stimuli"), efficacy=mean("efficacy"),
                   release=mean("release_factor"), output_diff=diff, hidden_hz_modulated=h_mod, hidden_hz_unmodulated=h_un,
                   present=name in present or name == "baseline")
        summary.append(row)
        print(f"{name:9s} {row['dopamine']:6.3f} {row['serotonin']:6.3f} {row['noradrenaline']:6.3f} {row['acetylcholine']:6.3f} "
              f"{row['stimuli']:8.3f} {row['efficacy']:8.3f} {row['release']:8.3f}  {h_mod:7.2f} / {h_un:7.2f}       {diff:.4f}"
              + ("" if row["present"] else "  (no such channels)"))
    (out / "summary.json").write_text(json.dumps(dict(network=a.network, profile=a.profile, depth=nm[-1]["depth"] if nm else None,
                                                     blocks=summary, modalities={t: tags.count(t) for t in set(tags)}), indent=1))
    subprocess.run([sys.executable, str(ROOT / "scripts/qa/plot_neuromod.py"), str(out / "modulated.neuromod.jsonl"),
                    "--out", str(out / "neuromod.png")], cwd=ROOT)


if __name__ == "__main__":
    main()
