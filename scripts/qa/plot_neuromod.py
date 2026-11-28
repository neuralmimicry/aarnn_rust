#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
"""Plot a network's global neuromodulator levels against stimuli and synapse energy.

Reads the runtime's neuromodulator telemetry (``NM_NEUROMOD_TRACE`` file, or the
``<network>.neuromod.jsonl`` files ``run_sim.sh`` writes under
``NM_NEUROMOD_TRACE_DIR``) and draws, per file:

- top: dopamine, serotonin, noradrenaline and acetylcholine levels (baseline 1);
- middle: external stimuli (fraction of the sensory population driven) and
  synapse energy (chemical drive delivered across synapses per cell after
  modulation, normalised to its maximum; mean axon/dendrite ATP too when it
  varies), with the postsynaptic efficacy and presynaptic release factor;
- bottom: the environmental drives (food, light, heat, noxious, energy,
  activity, novelty, reward).

  python3 scripts/qa/plot_neuromod.py TRACE.jsonl [TRACE2.jsonl ...] --out neuromod.png
  python3 scripts/qa/plot_neuromod.py logs/sim_cluster_*/neuromod --out neuromod.png
"""
import argparse
import json
from pathlib import Path


def load(path: Path):
    rows = []
    for line in path.read_text().splitlines():
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError:
            break
    return rows


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("traces", nargs="+", help="jsonl files or directories of *.neuromod.jsonl")
    ap.add_argument("--out", default="neuromod.png")
    ap.add_argument("--summary", action="store_true", help="also print per-file means")
    a = ap.parse_args()
    files = []
    for t in map(Path, a.traces):
        files += sorted(t.glob("*.jsonl")) if t.is_dir() else [t]
    files = [f for f in files if f.exists()]
    if not files:
        raise SystemExit("no neuromodulator traces found")
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    fig, axes = plt.subplots(3, len(files), figsize=(7 * len(files), 8), sharex="col", squeeze=False)
    colours = {"dopamine": "#c9971c", "serotonin": "#8e5fa8", "noradrenaline": "#c44e3b", "acetylcholine": "#2f8f8f"}
    for col, f in enumerate(files):
        rows = load(f)
        if not rows:
            continue
        t = [r["t_ms"] / 1000.0 for r in rows]
        ax = axes[0][col]
        for k, c in colours.items():
            ax.plot(t, [r[k] for r in rows], color=c, label=k, lw=1.4)
        ax.axhline(1.0, color="#999", lw=0.6, ls=":")
        ax.set_ylabel("level (baseline 1)")
        ax.set_title(f"{f.name}  (depth {rows[-1].get('depth')})", fontsize=10)
        ax.legend(fontsize=8, loc="upper left", ncol=2)
        ax = axes[1][col]
        drive = [r["synapse_drive"] for r in rows]
        d_max = max(drive) or 1.0
        ax.plot(t, [r["stimuli"] for r in rows], color="#5a9b2f", label="stimuli", lw=1.2)
        ax.plot(t, [d / d_max for d in drive], color="#555", label="synapse energy (drive / max)", lw=1.0, ls="--")
        atp = [r.get("synapse_energy") for r in rows]
        if all(x is not None for x in atp) and max(atp) - min(atp) > 1e-6:
            ax.plot(t, atp, color="#999", label="mean axon/dendrite ATP", lw=0.8, ls=":")
        ax.plot(t, [r["efficacy"] for r in rows], color="#c9971c", label="efficacy", lw=0.8, alpha=0.7)
        ax.plot(t, [r["release_factor"] for r in rows], color="#8e5fa8", label="release factor", lw=0.8, alpha=0.7)
        ax.set_ylabel("0..1 / factor")
        ax.legend(fontsize=8, loc="upper left", ncol=2)
        ax = axes[2][col]
        for k in ("food", "light", "heat", "noxious", "energy", "activity", "novelty", "reward"):
            v = [r["drives"].get(k, 0.0) for r in rows]
            if max(v) > 0:
                ax.plot(t, v, label=k, lw=1.0)
        ax.set_ylabel("drive 0..1")
        ax.set_xlabel("simulated time (s)")
        ax.legend(fontsize=8, loc="upper left", ncol=4)
        if a.summary:
            n = len(rows)
            mean = lambda k: sum(r[k] for r in rows) / n
            print(f"{f}: {n} samples, {t[-1]:.1f} s; DA {mean('dopamine'):.3f} 5-HT {mean('serotonin'):.3f} "
                  f"NA {mean('noradrenaline'):.3f} ACh {mean('acetylcholine'):.3f} stimuli {mean('stimuli'):.3f} "
                  f"efficacy {mean('efficacy'):.3f} release {mean('release_factor'):.3f}")
    fig.tight_layout()
    fig.savefig(a.out, dpi=110)
    print(f"wrote {a.out}")


if __name__ == "__main__":
    main()
