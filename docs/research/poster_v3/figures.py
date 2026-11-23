"""Data-derived figures for the AARNN A0 poster (v3; v2 figures kept).

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



# ----------------------------------------------------------------------- v3
GREEN = "#2E8540"


def _mat(m):
    import numpy as np
    return np.array(m["data"], dtype=float).reshape(m["rows"], m["cols"])


def layouts_figure(panels, w_cm: float, h_cm: float):
    """Soma positions of each network in its biologically equivalent body
    frame (side view: rostral left, dorsal up), with its synapses.

    panels: list of (title, network_json_path, subtitle)."""
    import numpy as np
    fig = plt.figure(figsize=(w_cm * CM, h_cm * CM))
    fs = 19
    n = len(panels)
    cols = 2
    rows = (n + 1) // cols
    for k, (title, path, sub) in enumerate(panels):
        snap = json.loads(Path(path).read_text())
        t = snap["topo"]
        H = np.array([[d["y"], d["z"]] for d in t["layers"][0]])
        S = np.array([[d["y"], d["z"]] for d in t["sensory_nodes"]])
        O = np.array([[d["y"], d["z"]] for d in t["output_nodes"]])
        r, c = divmod(k, cols)
        x0 = 0.01 + c * 0.5
        top = 0.97 - r * (0.80 / rows)
        hgt = 0.80 / rows - 0.13
        ax = fig.add_axes([x0, top - hgt - 0.10, 0.47, hgt])
        # synapses (hidden->hidden, sensory->hidden, hidden->motor), faint
        segs = []
        rec = _mat(snap["w_hh_rec"][0])
        for j, i in zip(*np.nonzero(rec)):
            segs.append((H[i], H[j]))
        win = _mat(snap["w_in"])
        for j, i in zip(*np.nonzero(win)):
            if i < len(S):
                segs.append((S[i], H[j]))
        wout = _mat(snap["w_out"])
        for kk, j in zip(*np.nonzero(wout)):
            if kk < len(O):
                segs.append((H[j], O[kk]))
        from matplotlib.collections import LineCollection
        alpha = 0.08 if len(segs) > 1500 else 0.25
        ax.add_collection(LineCollection([[(-a[0], a[1]), (-b[0], b[1])] for a, b in segs],
                                         colors=INK2, linewidths=0.35, alpha=alpha, zorder=1))
        ax.scatter(-S[:, 0], S[:, 1], s=14, marker="^", color=INDIGO, linewidths=0, zorder=3)
        ax.scatter(-H[:, 0], H[:, 1], s=16, marker="o", color=TEAL, linewidths=0, zorder=4)
        ax.scatter(-O[:, 0], O[:, 1], s=14, marker="s", color=ORANGE, linewidths=0, zorder=3)
        allp = np.vstack([S, H, O])
        ylo, yhi = allp[:, 0].min(), allp[:, 0].max()
        zlo, zhi = allp[:, 1].min(), allp[:, 1].max()
        pad_y = 0.05 * (yhi - ylo + 1e-3)
        pad_z = 0.12 * (zhi - zlo + 1e-3) + 0.01
        ax.set_xlim(-yhi - pad_y, -ylo + pad_y)
        ax.set_ylim(zlo - pad_z, zhi + pad_z)
        ax.set_xticks([])
        ax.set_yticks([])
        for s_ in ax.spines.values():
            s_.set_visible(False)
        fig.text(x0, top, title, fontsize=fs, color=INK, weight="bold", va="top")
        fig.text(x0, top - 0.05, sub, fontsize=fs - 5, color=INK2, va="top")
    # legend
    for i, (m, col, lab) in enumerate([("^", INDIGO, "sensory"), ("o", TEAL, "interneurons"),
                                         ("s", ORANGE, "motor"), ("_", INK2, "synapses")]):
        fig.add_artist(plt.Line2D([0.02 + i * 0.25], [0.075], transform=fig.transFigure, marker=m, color=col,
                                  markersize=9, markeredgewidth=2 if m == "_" else 1, linestyle="none"))
        fig.text(0.04 + i * 0.25, 0.075, lab, fontsize=fs - 3, color=INK, va="center")
    fig.text(0.01, 0.02, "Side view: head left, dorsal up (vertical axis stretched)",
             fontsize=fs - 4, color=INK2, va="center")
    return _export(fig)


def learning_figure(worm_new: Path, worm_old: Path, nao_new: Path, nao_old: Path, w_cm: float, h_cm: float):
    """Embodied behaviour under learning: none / every cell (before and after
    the structural fix) / a specialised subset. Worm: crawl speed; NAO: path
    walked in 30 s, x = fell."""
    import numpy as np
    wn, wo = json.loads(Path(worm_new).read_text())["tiers"], json.loads(Path(worm_old).read_text())["tiers"]
    nn, no = json.loads(Path(nao_new).read_text())["tiers"], json.loads(Path(nao_old).read_text())["tiers"]
    beh = lambda t, k: t.get(k, {}).get("closed", {}).get("behaviour", {})
    fig = plt.figure(figsize=(w_cm * CM, h_cm * CM))
    fs = 18
    series = [("no learning", "", RULE, None, "old"), ("every cell learns, before fix", "+L", "#F4B9A3", None, "old"),
              ("every cell learns", "+L", ORANGE, None, "new"), ("specialised cells learn", "+Ls", TEAL, None, "new")]
    panels = [("Worm crawl speed (body lengths/s)", ["T1/S0", "T1/S1", "T1/S1g"], ["point\ncells", "3D\nanatomy",
               "3D + gap\ncoupling"], wn, wo, "speed_body_lengths_s"),
              ("NAO path in 30 s (body lengths)", ["T0/S0", "T0/S1"], ["point\ncells", "3D\nanatomy"], nn, no,
               "path_length_L")]
    for p, (title, levels, labels, tn, to, key) in enumerate(panels):
        ax = fig.add_axes([0.08 + p * 0.5, 0.34, 0.40, 0.54])
        bw = 0.2
        for si, (lab, suf, col, _, src) in enumerate(series):
            xs, ys, fell = [], [], []
            for li, lev in enumerate(levels):
                b = beh(to if src == "old" else tn, lev + suf)
                if not b:
                    continue
                xs.append(li + (si - 1.5) * bw)
                ys.append(b.get(key, 0.0))
                fell.append(bool(b.get("fell")))
            ax.bar(xs, ys, width=bw * 0.92, color=col, zorder=2, label=lab if p == 0 else None)
            for x, y, f in zip(xs, ys, fell):
                if f:
                    ax.text(x, y + 0.01 * (ax.get_ylim()[1] or 1), "fell", ha="center", va="bottom", fontsize=fs - 4,
                            color=INK, rotation=90)
        ax.set_xticks(range(len(levels)))
        ax.set_xticklabels(labels, fontsize=fs - 2)
        ax.tick_params(axis="y", labelsize=fs - 2)
        ax.set_title(title, fontsize=fs, color=INK, loc="left", pad=8)
        ax.grid(axis="y", color=RULE, lw=0.7, zorder=0)
        for s_ in ("top", "right"):
            ax.spines[s_].set_visible(False)
    handles = [plt.Rectangle((0, 0), 1, 1, color=c) for _, _, c, _, _ in series]
    fig.legend(handles, [s[0] for s in series], loc="lower center", ncol=2, fontsize=fs - 3, frameon=False,
               bbox_to_anchor=(0.5, -0.01), columnspacing=1.5, handlelength=1.4)
    return _export(fig)


def scaling_figure(summary_path: Path, w_cm: float, h_cm: float):
    """Pattern decoding accuracy vs neuron count (fixed in-degree recurrent
    AARNN networks; mean ± s.d. over seeds) with chance levels."""
    import numpy as np
    s = json.loads(Path(summary_path).read_text())
    ns = sorted(int(k) for k in s)
    fig = plt.figure(figsize=(w_cm * CM, h_cm * CM))
    ax = fig.add_axes([0.13, 0.17, 0.62, 0.74])
    fs = 18
    lines = [("evoked_8", "8 patterns", TEAL, "o", 1 / 8), ("evoked_16", "16 patterns", INDIGO, "s", 1 / 16),
             ("evoked_32", "32 patterns", ORANGE, "D", 1 / 32)]
    for key, lab, col, m, chance in lines:
        xs = [n for n in ns if key in s[str(n)]]
        if not xs:
            continue
        mu = np.array([s[str(n)][key]["mean"] for n in xs])
        sd = np.array([s[str(n)][key]["sd"] for n in xs])
        ls = "--" if key.startswith("memory") else "-"
        ax.fill_between(xs, mu - sd, mu + sd, color=col, alpha=0.12, lw=0)
        ax.plot(xs, mu, ls=ls, marker=m, color=col, ms=6, lw=1.8, mec="white", mew=0.8)
        ax.text(xs[-1] * 1.12, mu[-1], lab, color=col, fontsize=fs - 3, va="center", ha="left")
        if chance:
            ax.axhline(chance, color=col, lw=0.8, ls=":", alpha=0.8)
    ax.set_xscale("log", base=2)
    ax.set_xticks(ns)
    ax.set_xticklabels([str(n) for n in ns], fontsize=fs - 3)
    ax.xaxis.set_minor_locator(NullLocator())
    ax.set_ylim(0, 1.02)
    ax.tick_params(axis="y", labelsize=fs - 3)
    ax.set_xlabel("Neurons in the recurrent network (log scale)", fontsize=fs - 1)
    ax.set_ylabel("Pattern decoded correctly", fontsize=fs - 1)
    ax.grid(color=RULE, lw=0.6)
    for s_ in ("top", "right"):
        ax.spines[s_].set_visible(False)
    return _export(fig)


def network_vs_parts_figure(summary_path: Path, w_cm: float, h_cm: float, key: str = "acc"):
    """Neuron-by-neuron: decoding accuracy (32 patterns) of the connected
    network vs the same cells disconnected, mean over seeds; shaded gap =
    what the connections add."""
    import numpy as np
    s = json.loads(Path(summary_path).read_text())
    ns = sorted(int(k) for k in s if "network" in s[k] and "parts" in s[k])
    net = np.array([s[str(n)]["network"][key]["mean"] for n in ns])
    par = np.array([s[str(n)]["parts"][key]["mean"] for n in ns])
    fig = plt.figure(figsize=(w_cm * CM, h_cm * CM))
    ax = fig.add_axes([0.13, 0.17, 0.62, 0.74])
    fs = 18
    ax.fill_between(ns, par, net, where=net >= par, color=TEAL, alpha=0.18, lw=0, interpolate=True)
    ax.plot(ns, par, color=INK2, lw=1.6, marker="o", ms=3, label="cells disconnected")
    ax.plot(ns, net, color=TEAL, lw=2.0, marker="o", ms=3.5, label="connected network")
    ax.axhline(1 / 32, color=INK2, lw=0.8, ls=":")
    ax.text(ns[-1] * 1.02, net[-1], "connected\nnetwork", color=TEAL, fontsize=fs - 4, va="center", ha="left")
    ax.text(ns[-1] * 1.02, par[-1], "same cells,\ndisconnected", color=INK2, fontsize=fs - 4, va="center",
            ha="left")
    ax.set_xlim(0, ns[-1] + 1)
    ax.set_ylim(0, max(net.max(), par.max()) * 1.15)
    ax.tick_params(labelsize=fs - 3)
    ax.set_xlabel("Neurons, added one at a time", fontsize=fs - 1)
    ax.set_ylabel("32 patterns told apart", fontsize=fs - 1)
    ax.grid(color=RULE, lw=0.6)
    for s_ in ("top", "right"):
        ax.spines[s_].set_visible(False)
    return _export(fig)


def scaling_two_panel_figure(one_by_one: Path, scaling: Path, w_cm: float, h_cm: float):
    """Left: neurons added one at a time (1-64), connected network vs the
    same cells disconnected (32-pattern decoding, mean of 3 seeds, 3-point
    running mean). Right: 8-2,048 neurons, 8/16/32 patterns (mean ± s.d.)."""
    import numpy as np
    a = json.loads(Path(one_by_one).read_text())
    ns = sorted(int(k) for k in a if "network" in a[k] and "parts" in a[k])
    net = np.array([a[str(n)]["network"]["acc"]["mean"] for n in ns])
    par = np.array([a[str(n)]["parts"]["acc"]["mean"] for n in ns])
    sm = lambda v: np.convolve(np.pad(v, 1, mode="edge"), np.ones(3) / 3, mode="valid")
    fig = plt.figure(figsize=(w_cm * CM, h_cm * CM))
    fs = 17
    ax = fig.add_axes([0.10, 0.20, 0.36, 0.66])
    ax.fill_between(ns, sm(par), sm(net), where=sm(net) >= sm(par), color=TEAL, alpha=0.22, lw=0,
                    interpolate=True)
    ax.plot(ns, sm(par), color=INK2, lw=1.8)
    ax.plot(ns, sm(net), color=TEAL, lw=2.2)
    ax.scatter(ns, net, s=6, color=TEAL, alpha=0.5, lw=0)
    ax.scatter(ns, par, s=6, color=INK2, alpha=0.5, lw=0)
    ax.axhline(1 / 32, color=INK2, lw=0.8, ls=":")
    ax.text(3, net.max() * 1.02, "connected network", color=TEAL, fontsize=fs - 4, va="bottom")
    ax.text(30, sm(par)[ns.index(30)] - 0.045 if 30 in ns else 0.08, "same cells,\ndisconnected", color=INK2,
            fontsize=fs - 4, va="top")
    ax.set_xlim(0, ns[-1] + 1)
    ax.set_ylim(0, net.max() * 1.18)
    ax.tick_params(labelsize=fs - 4)
    ax.set_xlabel("Neurons, added one at a time", fontsize=fs - 3)
    ax.set_ylabel("32 patterns told apart", fontsize=fs - 3)
    ax.set_title("The whole beats its parts", fontsize=fs - 1, color=INK, loc="left")
    ax.grid(color=RULE, lw=0.6)
    for s_ in ("top", "right"):
        ax.spines[s_].set_visible(False)
    s = json.loads(Path(scaling).read_text())
    ms = sorted(int(k) for k in s)
    bx = fig.add_axes([0.58, 0.20, 0.27, 0.66])
    for key, lab, col in (("evoked_8", "8", TEAL), ("evoked_16", "16", INDIGO), ("evoked_32", "32", ORANGE)):
        mu = np.array([s[str(n)][key]["mean"] for n in ms])
        sd = np.array([s[str(n)][key]["sd"] for n in ms])
        bx.fill_between(ms, mu - sd, mu + sd, color=col, alpha=0.12, lw=0)
        bx.plot(ms, mu, color=col, lw=1.8, marker="o", ms=4, mec="white", mew=0.6)
        bx.text(ms[-1] * 1.25, mu[-1], f"{lab} patterns", color=col, fontsize=fs - 5, va="center")
        bx.axhline(1 / int(lab), color=col, lw=0.7, ls=":", alpha=0.8)
    bx.set_xscale("log", base=2)
    bx.set_xticks([8, 64, 512, 2048])
    bx.set_xticklabels(["8", "64", "512", "2,048"], fontsize=fs - 4)
    bx.xaxis.set_minor_locator(NullLocator())
    bx.set_ylim(0, 1.0)
    bx.tick_params(axis="y", labelsize=fs - 4)
    bx.set_xlabel("Neurons (log scale)", fontsize=fs - 3)
    bx.set_title("…and keeps growing", fontsize=fs - 1, color=INK, loc="left")
    bx.grid(color=RULE, lw=0.6)
    for s_ in ("top", "right"):
        bx.spines[s_].set_visible(False)
    return _export(fig)


def storage_figure(report_path: Path, modular_dir: Path, w_cm: float, h_cm: float):
    """Left: experiences recalled (>= 90 % from half cues) vs excitatory
    neurons, log-log, with a linear reference; right: information stored per
    neuron. Open network (any pair may connect) and, where measured, groups
    (experiences inside groups, sparse wiring)."""
    import numpy as np
    rep = json.loads(Path(report_path).read_text())["storage"]
    rows = {int(k): v for k, v in rep["rows"].items() if v["capacity"] > 0}
    ns = sorted(rows)
    cap = np.array([rows[n]["capacity"] for n in ns])
    sd = np.array([rows[n]["capacity_sd"] for n in ns])
    bpn = np.array([rows[n]["bits_per_neuron"] for n in ns])
    mod = defaultdict_list()
    for f in Path(modular_dir).glob("*.json"):
        d = json.loads(f.read_text())
        mod[(d["module_size"], d["within_p"])].append(d)
    fig = plt.figure(figsize=(w_cm * CM, h_cm * CM))
    fs = 17
    ax = fig.add_axes([0.10, 0.20, 0.36, 0.64])
    ax.errorbar(ns, cap, yerr=sd, color=TEAL, marker="o", ms=5, lw=2, capsize=2, label="open network")
    ref = cap[0] * np.array(ns) / ns[0]
    ax.plot(ns, ref, color=INK2, lw=1.2, ls="--")
    ax.text(ns[-1], ref[-1], "  linear", color=INK2, fontsize=fs - 5, va="center")
    k = rep["fit"].get("capacity", {}).get("exponent")
    if k:
        ax.text(0.03, 0.95, f"capacity ∝ N^{k:.2f}", transform=ax.transAxes, fontsize=fs - 3, color=TEAL, va="top")
    markers = {(40, 0.5): ("s", ORANGE, "groups of 40, 50 % wired"), (20, 1.0): ("D", INDIGO, "groups of 20, fully wired")}
    for key, runs in sorted(mod.items()):
        m, col, lab = markers.get(key, ("^", INK2, f"groups of {key[0]}"))
        by = defaultdict_list()
        for d in runs:
            by[d["neurons"]].append(d)
        xs = sorted(by)
        ys = [np.mean([d["capacity"] for d in by[x]]) for x in xs]
        ax.plot(xs, ys, color=col, marker=m, ms=5, lw=1.6, label=lab)
    ax.set_xscale("log")
    ax.set_yscale("log")
    ax.set_xlabel("Excitatory neurons (log)", fontsize=fs - 3)
    ax.set_ylabel("Experiences recalled", fontsize=fs - 3)
    ax.tick_params(labelsize=fs - 4)
    ax.grid(color=RULE, lw=0.6, which="both")
    ax.set_title("Memories grow faster than neurons", fontsize=fs - 1, loc="left", color=INK)
    for s_ in ("top", "right"):
        ax.spines[s_].set_visible(False)
    bx = fig.add_axes([0.60, 0.20, 0.33, 0.64])
    bx.plot(ns, bpn, color=TEAL, marker="o", ms=5, lw=2)
    for key, runs in sorted(mod.items()):
        m, col, lab = markers.get(key, ("^", INK2, f"groups of {key[0]}"))
        by = defaultdict_list()
        for d in runs:
            by[d["neurons"]].append(d)
        xs = sorted(by)
        ys = [np.mean([d["bits_per_neuron"] for d in by[x]]) for x in xs]
        bx.plot(xs, ys, color=col, marker=m, ms=5, lw=1.6)
    bx.set_xlabel("Excitatory neurons", fontsize=fs - 3)
    bx.set_ylabel("Bits stored per neuron", fontsize=fs - 3)
    bx.tick_params(labelsize=fs - 4)
    bx.grid(color=RULE, lw=0.6)
    bx.set_title("Density rises with size", fontsize=fs - 1, loc="left", color=INK)
    for s_ in ("top", "right"):
        bx.spines[s_].set_visible(False)
    if mod:
        ax.legend(fontsize=fs - 6, frameon=False, loc="lower right")
    return _export(fig)


def defaultdict_list():
    from collections import defaultdict
    return defaultdict(list)


def whole_and_storage_figure(one_by_one: Path, report_path: Path, w_cm: float, h_cm: float):
    """Left: neurons added one at a time, connected vs disconnected (as in
    scaling_two_panel_figure). Right: experiences stored and recalled vs
    excitatory neurons (log-log, mean ± s.d. over seeds) with a linear
    reference through the smallest size, and the fitted exponent."""
    import numpy as np
    a = json.loads(Path(one_by_one).read_text())
    ns = sorted(int(k) for k in a if "network" in a[k] and "parts" in a[k])
    net = np.array([a[str(n)]["network"]["acc"]["mean"] for n in ns])
    par = np.array([a[str(n)]["parts"]["acc"]["mean"] for n in ns])
    sm = lambda v: np.convolve(np.pad(v, 1, mode="edge"), np.ones(3) / 3, mode="valid")
    fig = plt.figure(figsize=(w_cm * CM, h_cm * CM))
    fs = 17
    ax = fig.add_axes([0.10, 0.20, 0.36, 0.66])
    ax.fill_between(ns, sm(par), sm(net), where=sm(net) >= sm(par), color=TEAL, alpha=0.22, lw=0,
                    interpolate=True)
    ax.plot(ns, sm(par), color=INK2, lw=1.8)
    ax.plot(ns, sm(net), color=TEAL, lw=2.2)
    ax.scatter(ns, net, s=6, color=TEAL, alpha=0.5, lw=0)
    ax.scatter(ns, par, s=6, color=INK2, alpha=0.5, lw=0)
    ax.axhline(1 / 32, color=INK2, lw=0.8, ls=":")
    ax.text(3, net.max() * 1.02, "connected network", color=TEAL, fontsize=fs - 4, va="bottom")
    ax.text(30, 0.07, "same cells,\ndisconnected", color=INK2, fontsize=fs - 4, va="top")
    ax.set_xlim(0, ns[-1] + 1)
    ax.set_ylim(0, net.max() * 1.18)
    ax.tick_params(labelsize=fs - 4)
    ax.set_xlabel("Neurons, added one at a time", fontsize=fs - 3)
    ax.set_ylabel("32 patterns told apart", fontsize=fs - 3)
    ax.set_title("The whole beats its parts", fontsize=fs - 1, color=INK, loc="left")
    ax.grid(color=RULE, lw=0.6)
    for s_ in ("top", "right"):
        ax.spines[s_].set_visible(False)
    rep = json.loads(Path(report_path).read_text())["storage"]
    rows = {int(k): v for k, v in rep["rows"].items() if v["capacity"] > 0}
    ms = sorted(rows)
    cap = np.array([rows[n]["capacity"] for n in ms])
    sd = np.array([rows[n]["capacity_sd"] for n in ms])
    bx = fig.add_axes([0.60, 0.20, 0.33, 0.66])
    bx.errorbar(ms, cap, yerr=sd, color=ORANGE, marker="o", ms=5, lw=2, capsize=2)
    ref = cap[0] * np.array(ms) / ms[0]
    bx.plot(ms, ref, color=INK2, lw=1.2, ls="--")
    bx.text(ms[-1] * 1.05, ref[-1], "linear", color=INK2, fontsize=fs - 5, va="center")
    k = rep["fit"].get("capacity", {}).get("exponent")
    if k:
        bx.text(0.04, 0.96, f"≈ N^{k:.1f}", transform=bx.transAxes, fontsize=fs - 2, color=ORANGE, va="top",
                weight="bold")
    bx.set_xscale("log")
    bx.set_yscale("log")
    from matplotlib.ticker import NullFormatter
    xt = [t for t in (16, 32, 64, 128, 256) if ms[0] <= t <= ms[-1] * 1.01]
    bx.set_xticks(xt)
    bx.set_xticklabels([str(t) for t in xt])
    yt = [t for t in (2, 5, 10, 20, 50, 100) if cap.min() * 0.7 <= t <= cap.max() * 1.4]
    bx.set_yticks(yt)
    bx.set_yticklabels([str(t) for t in yt])
    bx.xaxis.set_minor_formatter(NullFormatter())
    bx.yaxis.set_minor_formatter(NullFormatter())
    bx.set_xlabel("Neurons (log)", fontsize=fs - 3)
    bx.set_ylabel("Experiences stored and recalled", fontsize=fs - 3)
    bx.tick_params(labelsize=fs - 4)
    bx.grid(color=RULE, lw=0.6, which="both")
    bx.set_title("Memories grow faster than neurons", fontsize=fs - 1, color=INK, loc="left")
    for s_ in ("top", "right"):
        bx.spines[s_].set_visible(False)
    return _export(fig)
