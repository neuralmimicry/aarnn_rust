"""Data-derived figures for the AARNN A0 poster (v2).

Figure 1: C. elegans import audit (from celegans_audit.json, produced by
          audit_celegans_import.py).
Figure 2: neuron counts on a log scale - measured AARNN runs, the G3 target
          and published biological references (values and sources listed in
          SCALE_ROWS below).

Palette validated with the dataviz skill's validator (light mode, all pairs):
#0A9396 / #3A5BC7 / #E2572B.  Identity never relies on colour alone: marker
shape differs per category and every category is direct-labelled.
"""
from __future__ import annotations

import io
import json
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
from matplotlib.ticker import FixedLocator, NullLocator  # noqa: E402

TEAL = "#0A9396"
INDIGO = "#3A5BC7"
ORANGE = "#E2572B"
INK = "#1D2B36"
INK2 = "#4A5A66"
RULE = "#C9D6DB"
CM = 1 / 2.54

plt.rcParams.update({
    "font.family": "Liberation Sans",
    "svg.fonttype": "path",
    "axes.edgecolor": RULE,
    "axes.labelcolor": INK2,
    "xtick.color": INK2,
    "ytick.color": INK2,
    "axes.linewidth": 1.0,
})


def _export(fig) -> tuple[bytes, bytes]:
    svg = io.BytesIO()
    fig.savefig(svg, format="svg", transparent=False, facecolor="white")
    png = io.BytesIO()
    fig.savefig(png, format="png", dpi=150, facecolor="white")
    plt.close(fig)
    return svg.getvalue(), png.getvalue()


def celegans_audit_figure(audit_path: Path, w_cm: float, h_cm: float):
    a = json.loads(audit_path.read_text(encoding="utf-8"))
    pts = a["points"]
    n_hidden = a["hidden_labels"]
    n_total = n_hidden + a["muscle_readouts"]
    fig = plt.figure(figsize=(w_cm * CM, h_cm * CM))
    ax = fig.add_axes([0.13, 0.13, 0.765, 0.70])
    fs = 17

    def xy(key):
        p = pts[key]
        return [q[0] for q in p], [q[1] for q in p]

    x, y = xy("nn")
    ax.scatter(x, y, s=5.5, marker="o", color=TEAL, linewidths=0, zorder=2)
    x, y = xy("nm_kept")
    ax.scatter(x, y, s=7, marker="s", color=INDIGO, linewidths=0, zorder=3)
    x, y = xy("nm_dropped")
    ax.scatter(x, y, s=26, marker="x", color=ORANGE, linewidths=1.3, zorder=4)

    ax.axhline(n_hidden - 0.5, color=INK2, lw=1.0, ls=(0, (4, 3)), zorder=1)
    ax.set_xlim(-3, n_hidden + 2)
    ax.set_ylim(n_total + 2, -3)
    ax.xaxis.set_major_locator(FixedLocator([0, 100, 200, n_hidden - 1]))
    ax.yaxis.set_major_locator(FixedLocator([0, 100, 200, n_hidden - 1, n_total - 1]))
    ax.yaxis.set_minor_locator(NullLocator())
    ax.tick_params(labelsize=fs - 2, length=4)
    ax.set_xlabel("Presynaptic neuron (alphabetical index)", fontsize=fs)
    ax.set_ylabel("Postsynaptic target index", fontsize=fs)
    for s in ("top", "right"):
        ax.spines[s].set_visible(False)
    ax.text(n_hidden + 9, (n_hidden - 1) / 2, "302 neurons", rotation=90, va="center", ha="left",
            fontsize=fs - 2, color=INK2, clip_on=False)
    ax.text(n_hidden + 9, n_hidden + (n_total - n_hidden) / 2, "96\nmuscles", rotation=0,
            va="center", ha="left", fontsize=fs - 3, color=INK2, clip_on=False, linespacing=1.0)

    # Legend as direct labels above the plot (identity = shape + colour + words).
    items = [
        ("o", TEAL, f"neuron → neuron, kept ({a['nn_pairs_source']:,})", 5.5 * 3),
        ("s", INDIGO, f"neuron → muscle, kept ({a['nm_pairs_source_positive']:,})", 7 * 3),
        ("x", ORANGE, f"neuron → muscle, inhibitory, not imported ({a['negative_pairs_dropped']:,})", 60),
    ]
    for i, (m, c, label, s) in enumerate(items):
        yy = 0.965 - i * 0.052
        fig.lines  # noqa: B018
        fig.add_artist(plt.Line2D([0.03], [yy], transform=fig.transFigure, marker=m, color=c,
                                  markersize=8 if m != "x" else 9, markeredgewidth=1.6, linestyle="none"))
        fig.text(0.055, yy, label, fontsize=fs, color=INK, va="center", ha="left")
    return _export(fig)


# label, value (neurons), kind, note.  kind: measured | imported | target | biology
SCALE_ROWS = [
    ("Default seed network", 1, "measured", "1"),
    ("C. elegans snapshot (MuJoCo loop)", 302, "measured", "302"),
    ("Live shared workspace, 5 nodes", 2_101, "measured", "2,101"),
    ("Fly projection (imported)", 20_466, "imported", "20,466"),
    ("Largest run: 40-shard LIF mesh", 1_380_000, "measured", "1.38 M"),
    ("G3 target (roadmap)", (100e6, 400e6), "target", "100–400 M (≥72× largest run)"),
    ("Goldcrest pallium [6]", 64e6, "biology", "≈64 M"),
    ("Raven pallium [6]", 1.2e9, "biology", "≈1.2 B"),
    ("Human brain [7]", 86e9, "biology", "≈86 B"),
]


def scale_figure(w_cm: float, h_cm: float):
    fig = plt.figure(figsize=(w_cm * CM, h_cm * CM))
    ax = fig.add_axes([0.385, 0.16, 0.585, 0.80])
    fs = 16
    n = len(SCALE_ROWS)
    ys = list(range(n))[::-1]
    for (label, val, kind, note), y in zip(SCALE_ROWS, ys):
        if kind == "target":
            lo, hi = val
            ax.plot([lo, hi], [y, y], color=ORANGE, lw=9, solid_capstyle="butt", zorder=3)
            ax.text(lo / 1.6, y, note, va="center", ha="right", fontsize=fs, color=INK, weight="bold")
            continue
        if kind == "measured":
            ax.plot([val], [y], marker="o", ms=11, color=TEAL, mec="white", mew=1.5, zorder=4)
        elif kind == "imported":
            ax.plot([val], [y], marker="o", ms=11, mfc="white", color=TEAL, mew=2.2, zorder=4)
        else:
            ax.plot([val], [y], marker="D", ms=10, color=INDIGO, mec="white", mew=1.2, zorder=4)
        ax.plot([1, val], [y, y], color=RULE, lw=1.2, zorder=1)
        ax.text(val * 1.9, y, note, va="center", ha="left", fontsize=fs, color=INK)
    ax.set_xscale("log")
    ax.set_xlim(0.5, 1e12)
    ax.set_ylim(-0.7, n - 0.3)
    ax.set_yticks(ys)
    ax.set_yticklabels([r[0] for r in SCALE_ROWS], fontsize=fs)
    ax.xaxis.set_major_locator(FixedLocator([1, 1e3, 1e6, 1e9, 1e12]))
    ax.set_xticklabels(["1", "10³", "10⁶", "10⁹", "10¹²"], fontsize=fs)
    ax.xaxis.set_minor_locator(NullLocator())
    ax.set_xlabel("Neurons (log scale)", fontsize=fs)
    ax.tick_params(axis="y", length=0)
    ax.grid(axis="x", color=RULE, lw=0.8, zorder=0)
    for s in ("top", "right", "left"):
        ax.spines[s].set_visible(False)
    # separator between AARNN rows and references
    ax.axhline(ys[5] - 0.5, color=RULE, lw=1.0)
    ax.axhline(ys[4] - 0.5, color=RULE, lw=1.0)
    return _export(fig)
