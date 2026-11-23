#!/usr/bin/env python3
"""Build the AARNN Royal Society A0 portrait poster (v2) as an editable .odg.

    python3 -I docs/research/poster_v2/audit_celegans_import.py      # refresh audit data
    <python with matplotlib, Pillow, segno> docs/research/poster_v2/build_poster.py
    soffice --headless --convert-to pdf --outdir docs/research docs/research/<name>.odg

Every statement on the poster traces to the audited source tree
(aarnn_rust @ 6d02f24, v0.1.43), the Evelyn stage-gate record, the
neuralmimicry.ai site, or the cited literature.  See FACTS.md alongside.
"""
from __future__ import annotations

import io
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import segno  # noqa: E402

from figures import celegans_audit_figure, scale_figure  # noqa: E402
from odg import Doc, P, PStyle, Run, block_height, text_width_cm  # noqa: E402

OUT = HERE.parent / "AARNN-Royal-Society-A0-Portrait-v2-Editable.odg"

# ---------------------------------------------------------------- palette
NAVY = "#122A3A"
TEAL = "#007D83"
TEAL_L = "#5FC4CA"
TEAL_XL = "#A9DEE1"
PANEL = "#EDF5F6"
PANEL2 = "#F4F6F7"
INK = "#1D2B36"
INK2 = "#4A5A66"
RULE = "#C9D6DB"
WHITE = "#FFFFFF"
GOOD = "#0CA30C"
WARN = "#FAB219"
CRIT = "#D03B3B"
ORANGE = "#E2572B"

W, H = 84.1, 118.9
M = 2.2
CW = W - 2 * M
G = 1.4
COLW = (CW - 2 * G) / 3
COLX = [M, M + COLW + G, M + 2 * (COLW + G)]
SEC_GAP = 1.0

# ---------------------------------------------------------------- type scale
BODY = PStyle(size=22.0, leading=1.28, color=INK, after=0.22)
SMALL = PStyle(size=19.0, leading=1.27, color=INK2, after=0.0)
SMALL_INK = PStyle(size=19.0, leading=1.27, color=INK, after=0.0)
CAPTION = PStyle(size=18.0, leading=1.27, color=INK2, after=0.0)
LABEL = PStyle(size=18.5, leading=1.2, color=TEAL, bold=True, letter_spacing=1.5)
H_SEC = PStyle(size=40.0, leading=1.1, color=NAVY, bold=True)
H_NUM = PStyle(size=40.0, leading=1.1, color=TEAL, bold=True)
H_SUB = PStyle(size=22.0, leading=1.2, color=NAVY, bold=True, after=0.1)


def bullet_style(base: PStyle, after=0.16) -> PStyle:
    hang = text_width_cm("• ", base.size, bold=True)
    return PStyle(size=base.size, leading=base.leading, color=base.color, after=after, hang=hang)


BUL = bullet_style(BODY)
BUL_S = bullet_style(SMALL_INK, after=0.12)


def B(text, style=BUL):
    return P(text, style, bullet="• ")


doc = Doc(W, H,
          title="Growing Embodied Intelligence from a Single Neuron: an evidence-gated, connectome-guided AARNN roadmap",
          creator="Paul B. Isaac’s",
          description=("A0 portrait poster (84.1 x 118.9 cm), Royal Society 'Innovating technologies for "
                       "neuromorphic computing', 23-25 Nov 2026. Native editable text and shapes; figures are "
                       "data-derived SVG. Source audit 13 Nov 2026 at aarnn_rust 6d02f24 (v0.1.43)."))


def section(x, y, w, num, title):
    nw = text_width_cm(num + "  ", H_NUM.size, bold=True)
    doc.text(x, y, nw + 0.2, [P(num, H_NUM)], name=f"Section {num} number")
    doc.text(x + nw, y, w - nw, [P(title, H_SEC)], name=f"Section {num} title")
    hh = H_SEC.line_cm()
    doc.line(x, y + hh + 0.2, x + w, y + hh + 0.2, color=TEAL, sw=0.07)
    return y + hh + 0.65


def text(x, y, w, paras, gap=0.0, **kw):
    h = doc.text(x, y, w, paras, **kw)
    return y + h + gap


# ===================================================================== HEADER
HEADER_H = 14.9
doc.rect(0, 0, W, HEADER_H, fill=NAVY, name="Header band")
doc.rect(0, HEADER_H, W, 0.3, fill=TEAL, name="Header rule")
doc.text(M, 1.1, 30, [P("NEURALMIMICRY", PStyle(size=29, leading=1.1, color=TEAL_L, bold=True,
                                                    letter_spacing=4))], name="Brand")
right = PStyle(size=20, leading=1.25, color=WHITE, align="end", letter_spacing=1)
right_b = PStyle(size=20, leading=1.25, color=TEAL_XL, align="end", letter_spacing=1, bold=True)
doc.text(W - M - 54, 1.05, 54, [P("ROYAL SOCIETY  ·  INNOVATING TECHNOLOGIES FOR NEUROMORPHIC COMPUTING", right_b),
                                P("LONDON  ·  23–25 NOVEMBER 2026  ·  POSTER SESSION 23 NOVEMBER", right)],
         name="Meeting")
TITLE = PStyle(size=94, leading=1.0, color=WHITE, bold=True)
y = 2.95 + doc.text(M, 2.95, CW, [P("Growing Embodied Intelligence\nfrom a Single Neuron", TITLE)], name="Title")
SUB = PStyle(size=37, leading=1.15, color=TEAL_XL)
y = text(M, y + 0.2, CW, [P("A connectome-guided AARNN roadmap that advances only through falsifiable evidence gates",
                            SUB)], name="Subtitle")
AUTH = PStyle(size=27, leading=1.2, color=WHITE)
text(M, y + 0.4, CW, [P([Run("Paul B. Isaac’s", bold=True),
                         Run("   ·   NeuralMimicry Ltd, Rugby, Warwickshire, United Kingdom   ·   "),
                         Run("paul@neuralmimicry.ai   ·   neuralmimicry.ai", color=TEAL_XL)], AUTH)], name="Author")

# ===================================================================== HYPOTHESIS + STATUS
y0 = HEADER_H + 1.3
hyp_w = 2 * COLW + G
pad = 0.85
hyp_paras = [
    P("RESEARCH HYPOTHESIS", LABEL),
    P("Generalisable problem-solving may emerge from recursive, embodied spiking dynamics that grow and "
      "adapt under local rules, at neuron counts far below the human brain’s ≈86 billion [7].",
      PStyle(size=31, leading=1.2, color=NAVY, bold=True, before=0.25, after=0.3)),
    P("AARNN (Auto-Asynchronous Recursive Neuromorphic Network) is the open, inspectable test-bed for this claim. "
      "This poster separates what is **implemented**, what has been **measured**, and the **experiments that "
      "could reject it**. Every number traces to the audited source [1], its engineering records, or the "
      "cited literature.", PStyle(size=22, leading=1.28, color=INK)),
]
hyp_h = block_height(hyp_paras, hyp_w - 2 * pad - 0.45) + 2 * pad

st_x = COLX[2]
st_pad = 0.75
status_rows = [
    (GOOD, "BUILT AND TESTED",
     "Growth from one neuron · local plasticity · 3 connectome importers · 28 robot bodies · AER and FPAA "
     "interfaces · seeded replay"),
    (WARN, "MEASURED AT SMALL SCALE",
     "2,101 neurons on 5 nodes · a 1.38 M-neuron layer on 40 shards · closed sensor-motor I/O loops for "
     "worm, hexapod and NAO"),
    (CRIT, "NOT YET SHOWN",
     "Behavioural task success · retention under continual learning · energy per task · live analog FPAA "
     "execution"),
]
dot = 0.7
row_w = COLW - 2 * st_pad - dot - 0.4
st_label = PStyle(size=18.5, leading=1.2, color=NAVY, bold=True, letter_spacing=1)
st_body = PStyle(size=19.5, leading=1.26, color=INK)
rows_h = [block_height([P(lab, st_label), P(body, st_body)], row_w) for _, lab, body in status_rows]
st_h = 2 * st_pad + 1.15 + sum(rows_h) + 0.38 * (len(rows_h) - 1)
box_h = max(hyp_h, st_h)
doc.rect(M, y0, hyp_w, box_h, fill=PANEL, name="Hypothesis panel")
doc.rect(M, y0, 0.45, box_h, fill=TEAL, name="Hypothesis accent")
doc.text(M + 0.45 + pad, y0 + pad, hyp_w - 2 * pad - 0.45, hyp_paras, name="Hypothesis text")
doc.rect(st_x, y0, COLW, box_h, fill=WHITE, stroke=RULE, sw=0.06, name="Status panel")
doc.text(st_x + st_pad, y0 + st_pad, COLW - 2 * st_pad, [P("STATUS AT A GLANCE  ·  13 NOV 2026", LABEL)],
         name="Status label")
yy = y0 + st_pad + 1.15
for (col, lab, body), rh in zip(status_rows, rows_h):
    doc.ellipse(st_x + st_pad, yy + 0.08, dot, dot, fill=col, name=f"Status dot {lab}")
    doc.text(st_x + st_pad + dot + 0.4, yy, row_w, [P(lab, st_label), P(body, st_body)], name=f"Status {lab}")
    yy += rh + 0.38

ytop = y0 + box_h + 1.3
col_bottom = []

# ===================================================================== COLUMN 1
x, w = COLX[0], COLW
y = section(x, ytop, w, "01", "AARNN in one loop")
y = text(x, y, w, [P("An open-source Rust runtime in which a spiking network grows, wires and adapts while it runs. "
                     "Learning is local and online: the spiking network receives no backpropagation or gradient "
                     "training [1].", BODY)])
dy = y + 0.2
lm = 1.15
bw = w - 2 * lm
node = PStyle(size=19.5, leading=1.18, color=NAVY, bold=True, align="center")
node_s = PStyle(size=16.5, leading=1.2, color=INK2, align="center")
node_w = PStyle(size=20, leading=1.18, color=WHITE, bold=True, align="center")
node_ws = PStyle(size=16.5, leading=1.2, color=TEAL_XL, align="center")
half = (bw - 0.7) / 2
gap_v = 1.05
sh = 2.35
doc.rect(x + lm, dy, half, sh, fill=PANEL, stroke=TEAL, sw=0.06, name="Seed: single neuron",
         paras=[P("Single-neuron seed", node), P("default: 1 hidden neuron, no I/O", node_s)], pad=0.2)
doc.rect(x + lm + half + 0.7, dy, half, sh, fill=PANEL, stroke=TEAL, sw=0.06, name="Seed: connectome",
         paras=[P("Connectome seed", node), P("imported topology prior", node_s)], pad=0.2)
cy = dy + sh + gap_v
ch = 2.7
c1x = x + lm + half / 2
c2x = x + lm + half * 1.5 + 0.7
doc.line(c1x, dy + sh, c1x, cy, color=NAVY, sw=0.07, end=True, head=0.5)
doc.line(c2x, dy + sh, c2x, cy, color=NAVY, sw=0.07, end=True, head=0.5)
doc.rect(x + lm, cy, bw, ch, fill=NAVY, name="Spiking core",
         paras=[P("Spiking core  ·  fixed time-step", node_w),
                P("Izhikevich neurons [8] · AMPA/NMDA/GABA · short-term plasticity [11] · conduction delays",
                  node_ws)], pad=0.25)
ry = cy + ch + gap_v
rh2 = 2.7
doc.rect(x + lm, ry, half, rh2, fill=WHITE, stroke=TEAL, sw=0.06, name="Local plasticity",
         paras=[P("Local plasticity", node), P("every step: STDP · homeostasis · scaling · Dale’s law", node_s)],
         pad=0.2)
doc.rect(x + lm + half + 0.7, ry, half, rh2, fill=WHITE, stroke=TEAL, sw=0.06, name="Structural loop",
         paras=[P("Structural loop", node), P("slower cadences: growth · contact synapses · pruning · myelin",
                                               node_s)], pad=0.2)
for cx_ in (c1x, c2x):
    doc.line(cx_ - 0.35, cy + ch, cx_ - 0.35, ry, color=TEAL, sw=0.07, end=True, head=0.42)
    doc.line(cx_ + 0.35, ry, cx_ + 0.35, cy + ch, color=TEAL, sw=0.07, end=True, head=0.42)
by = ry + rh2 + gap_v
bh = 2.35
doc.rect(x + lm, by, bw, bh, fill=PANEL, stroke=TEAL, sw=0.06, name="Embodiment",
         paras=[P("Embodiment  ·  Webots / MuJoCo robots", node),
                P("sensors → spike inputs   ·   motor readouts → actuators", node_s)], pad=0.2)
xr = x + w - 0.4
xl = x + 0.4
doc.polyline([(x + lm + bw, cy + ch * 0.5), (xr, cy + ch * 0.5), (xr, by + bh * 0.5), (x + lm + bw, by + bh * 0.5)],
             color=NAVY, sw=0.07, end=True, name="Motor path", head=0.5)
doc.polyline([(x + lm, by + bh * 0.5), (xl, by + bh * 0.5), (xl, cy + ch * 0.5), (x + lm, cy + ch * 0.5)],
             color=NAVY, sw=0.07, end=True, name="Sensory path", head=0.5)
y = by + bh + 0.55
y = text(x, y, w, [
    B("**Neurons:** Izhikevich (RS default; 8 presets) with adaptive threshold, or LIF; plus gap junctions, "
      "volume transmission and active dendrites."),
    B("**Conduction:** delay and attenuation from path length; apical/basal compartments; back-propagating spikes; "
      "activity-dependent myelination; ATP fatigue."),
    B("**Recursive:** within-layer recurrence, feedback from layer l+1 to l, and output-to-sensory feedback."),
    B("**Asynchronous = multi-rate:** spikes advance every dt; growth, pruning, morphology and metabolism run on "
      "slower, decoupled cadences. The core is clock-driven; synaptic summation is spike-sparse."),
], gap=SEC_GAP)

y = section(x, y, w, "02", "Growth rules, as coded")
kv_k = PStyle(size=19.5, leading=1.25, color=NAVY, bold=True)
kv_v = PStyle(size=19.5, leading=1.25, color=INK)
kv = [
    ("Spawn trigger", "firing-rate average ≥ 0.5; 500 ms own and 150 ms global cooldown; ≤ 1 spawn per pass"),
    ("Layers", "a layer of 32 neurons opens the next, up to 6"),
    ("Spontaneous", "one new neuron per 1,000 ms if nothing else grew"),
    ("Development", "specification → migration → differentiation in 3-D; axon pathfinding → dendrites → "
                    "synaptogenesis → refinement → myelination"),
    ("Synapses", "trunks, branches and boutons grow toward an activity-energy field; synapses form on contact"),
    ("Pruning", "weak components retract; a hidden neuron without boutons is removed after a delay, never the last"),
    ("Bounds", "neuron cap; per-neuron I/O and degree caps; 8–64 segments per neuron; 200–8,000 contact checks "
               "per pass"),
]
kw_ = 6.0
for i, (k, v) in enumerate(kv):
    vh = block_height([P(v, kv_v)], w - kw_ - 0.5)
    rh = vh + 0.36
    if i % 2 == 0:
        doc.rect(x, y, w, rh, fill=PANEL2, name=f"Rule row {k}")
    doc.text(x + 0.3, y + 0.18, kw_ - 0.3, [P(k, kv_k)], name=f"Rule {k}")
    doc.text(x + kw_, y + 0.18, w - kw_ - 0.3, [P(v, kv_v)], name=f"Rule value {k}")
    y += rh
y = text(x, y + 0.3, w, [P("CI tests (all features) cover growth-cadence decoupling, early-cell stages, neuron caps and "
                           "staged development [1].", CAPTION)])
col_bottom.append(y)

# ===================================================================== COLUMN 2
x, w = COLX[1], COLW
y = section(x, ytop, w, "03", "Connectome-guided embodiment")
y = text(x, y, w, [P("Three builders turn published wiring into runnable snapshots paired with generated robot "
                     "bodies [1]. S/H/O: sensory / hidden / output neurons.", BODY)])
species = [
    ("C. elegans", "24 / 302 / 96",
     "GoPiGo wiring [2] (reference [3]): positive integer weights exact; no delays; heuristic geometry."),
    ("Drosophila · FAFB v783 [4]", "418 / 20,000 / 48",
     "From 138,584 neurons, 50.7 M synapses: region-balanced top-k; 63 % of kept edges remapped to "
     "adjacent layers; no transmitter signs."),
    ("Drosophila · BANC v626 [5]", "418 / 20,000 / 48",
     "From 112,885 neurons, 22.2 M synapses: same projection, 64 % of kept edges remapped."),
    ("Larval zebrafish", "32 / 2,000 / 32",
     "Seung Lab synapse table: top 2,000 cells by degree, region-stratified; synthetic sensory input."),
]
sp_name = PStyle(size=21, leading=1.18, color=NAVY, bold=True)
sp_sho = PStyle(size=21, leading=1.18, color=TEAL, bold=True, align="end")
sp_txt = PStyle(size=18.5, leading=1.26, color=INK)
cp = 0.42
for name, sho, detail in species:
    paras = [P(detail, sp_txt)]
    hh = 0.95 + block_height(paras, w - 2 * cp - 0.2) + 2 * cp - 0.05
    doc.rect(x, y, w, hh, fill=PANEL2, name=f"Species card {name}")
    doc.rect(x, y, 0.2, hh, fill=TEAL, name=f"Species accent {name}")
    doc.text(x + cp + 0.2, y + cp - 0.05, w - 2 * cp - 7.5, [P(name, sp_name)], name=f"Species {name}")
    doc.text(x + w - cp - 7.0, y + cp - 0.05, 7.0, [P(sho, sp_sho)], name=f"Species S/H/O {name}")
    doc.text(x + cp + 0.2, y + cp + 0.9, w - 2 * cp - 0.2, paras, name=f"Species detail {name}")
    y += hh + 0.28
y = text(x, y + 0.1, w, [
    P("Fresh imports keep the top-scored 74 % (worm) or 78 % (fly, fish) of edges, so running weights can differ "
      "from the file.", SMALL),
], gap=0.3)
y = text(x, y, w, [
    B("**Species profiles change physiology too:** inhibitory fraction 0.20 human vs 0.36 worm; myelination on "
      "for human and fish only."),
    B("**28 embodiments** (MuJoCo, USD, Webots): worm, two flies, hexapod, NAO, zebrafish, a reduced Corvus corone "
      "crow (31 joints, untrained) and 21 commercial robot models."),
], gap=SEC_GAP)

y = section(x, y, w, "04", "Import audit: C. elegans")
audit_stats = [("302", "neuron labels"), ("3,126", "neuron → neuron"), ("438", "neuron → muscle"),
               ("0", "mismatches"), ("125", "inhibitory missing")]
sw_ = w / len(audit_stats)
an = PStyle(size=34, leading=1.05, color=TEAL, bold=True, align="center")
an_o = PStyle(size=34, leading=1.05, color=ORANGE, bold=True, align="center")
al = PStyle(size=16, leading=1.2, color=INK2, align="center")
ah = 0
for i, (n, l) in enumerate(audit_stats):
    ah = max(ah, doc.text(x + i * sw_, y, sw_, [P(n, an_o if i == 4 else an), P(l, al)], name=f"Audit stat {l}"))
y += ah + 0.2
fig_h = 13.6
svg, png = celegans_audit_figure(HERE / "celegans_audit.json", w, fig_h)
doc.image(x, y, w, fig_h, "fig_celegans_audit", svg, png)
y += fig_h + 0.25
y = text(x, y, w, [
    P("Independent re-parse of the signed source against the snapshot: all 3,564 positive pairs match, with no "
      "extras. The builder accepts only non-negative weights, so 125 inhibitory neuron → muscle pairs (Σw = −860; "
      "27 cells incl. DD, VD) are absent; 96.6 % of signed pairs retained. Scope: structure, not behaviour.",
      CAPTION),
])
col_bottom.append(y)

# ===================================================================== COLUMN 3
x, w = COLX[2], COLW
y = section(x, ytop, w, "05", "Hardware-facing interfaces")
y = text(x, y, w, [P("Kernels and spike traffic are exposed in forms hardware can consume, and the runtime does not "
                     "claim an offload it cannot verify [1].", BODY)])
fp_pad = 0.4
hd = PStyle(size=19.5, leading=1.2, color=WHITE, bold=True)
li = PStyle(size=17.5, leading=1.28, color=INK)
li_b = bullet_style(li, after=0.0)
analog = ["Synaptic filter (AMPA/NMDA/GABA)", "Short-term plasticity", "Adaptive threshold + homeostasis",
          "Active dendrite", "Gap-junction + volume field", "Morphology transmission", "Triplet / scaling / Dale"]
softw = ["Growth, pruning, migration", "Sleep replay", "Prediction-error loop", "World-model state",
         "Import rewiring", "Distributed orchestration"]
half3 = (w - 0.5) / 2
la = [P(t, li_b, bullet="• ") for t in analog]
ls = [P(t, li_b, bullet="• ") for t in softw]
lh_ = max(block_height(la, half3 - 2 * fp_pad), block_height(ls, half3 - 2 * fp_pad))
head_h = 1.15
doc.rect(x, y, half3, head_h, fill=TEAL, name="FPAA candidates header",
         paras=[P("FPAA candidates (7)", hd)], pad=0.25)
doc.rect(x + half3 + 0.5, y, half3, head_h, fill=INK2, name="Software-only header",
         paras=[P("Software-only (6)", hd)], pad=0.25)
doc.rect(x, y + head_h, half3, lh_ + 2 * fp_pad, fill=PANEL, name="FPAA candidates box")
doc.rect(x + half3 + 0.5, y + head_h, half3, lh_ + 2 * fp_pad, fill=PANEL2, name="Software-only box")
doc.text(x + fp_pad, y + head_h + fp_pad, half3 - 2 * fp_pad, la, name="FPAA candidates list")
doc.text(x + half3 + 0.5 + fp_pad, y + head_h + fp_pad, half3 - 2 * fp_pad, ls, name="Software-only list")
y += head_h + lh_ + 2 * fp_pad + 0.35
rt = PStyle(size=18.5, leading=1.25, color=WHITE, bold=True)
rt2 = PStyle(size=17.5, leading=1.25, color=TEAL_XL)
route = [P("Per kernel: requested route vs effective route", rt),
         P("Effective route is FPAA only if transport is ready, the manifest validates, programming state is "
           "persisted and sample tests pass; otherwise automatic software fallback.", rt2)]
rth = block_height(route, w - 0.9) + 0.9
doc.rect(x, y, w, rth, fill=NAVY, name="Route gate", paras=route, pad=0.45, valign="top")
y += rth + 0.35
y = text(x, y, w, [P("Each candidate ships an Xcos/Scilab model, an Okika Pi.Ka manifest and a SPICE macromodel. "
                     "**Current limit:** no live analog execution path from the runtime into the FPAA exists yet; "
                     "the Rust kernels remain the source of truth.", SMALL_INK)], gap=0.35)
y = text(x, y, w, [
    P("Spike-event formats", H_SUB),
    B("**AER1** varint events; **AARNN-AER/1** with epochs, sequence numbers, ns capture time, clock uncertainty, "
      "gap records and CRC-16; **AER-CAN v1** 8-byte frames.", BUL_S),
    B("**Fabric bridge** (Raspberry Pi 4 + Okika Pi.Ka): UDP, 32-byte synapse events, ≤ 64 per packet. Opt-in "
      "OpenCL parity tests compare GPU and CPU stepping.", BUL_S),
], gap=SEC_GAP)

y = section(x, y, w, "06", "Measured so far: scale")
sf_h = 11.6
svg, png = scale_figure(w, sf_h)
doc.image(x, y, w, sf_h, "fig_scale", svg, png)
y += sf_h + 0.15
y = text(x, y, w, [P("Filled: measured runs. Open: imported; its closed-loop placement exceeded the 300 s budget. "
                     "Diamonds: published counts [6, 7]. The largest run is 72× below the G3 floor.", CAPTION)],
         gap=0.35)
y = text(x, y, w, [
    B("**Largest run:** 1.38 M LIF neurons, 6.1 G neuron-steps on 40 shards of one host, executing a converted "
      "Qwen3.5-9B feed-forward layer at 1.84 % error (gate 3.74 %) [10].", BUL_S),
    B("**Two hosts:** Qwen3-8B layers 18–21 as LIF meshes: +3.755 % perplexity (gate < 5 %), 98.44 % top-1 "
      "agreement, but 1.311 s/token versus 0.642 s/token dense [10].", BUL_S),
    B("**Distributed runtime:** gRPC spike streams and OpenMPI exchange are implemented; shard planning, "
      "quorum-fenced ownership and consistent cuts are tested reference contracts; production cut-over is open.",
      BUL_S),
])
col_bottom.append(y)

# ===================================================================== FALSIFICATION TABLE
y = max(col_bottom) + 1.4
print(f"table starts {y:.2f}")
y = section(M, y, CW, "07", "Experiments that can falsify it")
y = text(M, y, CW, [P("Pre-register tasks, seeds, budgets and success criteria; test held-out environments after "
                      "adaptation. Controls: degree-preserving shuffle; frozen plasticity and growth; recurrence and "
                      "timing ablations; a conventional controller.", BODY)], gap=0.1)
cols = [12.4, 29.6, CW - 12.4 - 29.6]
hdr = PStyle(size=18, leading=1.2, color=WHITE, bold=True, letter_spacing=1)
q_st = PStyle(size=20, leading=1.22, color=NAVY, bold=True)
c_st = PStyle(size=19, leading=1.25, color=INK)
rows = [
    ("Topology fidelity", "Retained nodes and edges; declared signs, delays, missing data and projection changes.",
     (CRIT, "OPEN", "Worm keeps 3,564 of 3,689 signed pairs; fly projections remap ≈63 % of kept edges; "
                    "no synaptic signs yet.")),
    ("Behavioural transfer", "Task success and perturbation recovery on unseen sensorimotor tasks versus silent, "
                             "random and conventional controllers.",
     (CRIT, "OPEN", "I/O loops only: MuJoCo worm 47, hexapod 35, NAO 17 neural exchanges in 1,500 steps. "
                    "Acceptance matrix: 0 of 224 rows pass.")),
    ("Continual adaptation", "Retention of earlier tasks after new exposure; plastic versus frozen networks.",
     (CRIT, "OPEN", "No retention benchmark exists yet.")),
    ("Timing and energy", "p95/p99 sensor-to-actuator latency, event throughput and whole-system joules per "
                          "task [9].",
     (CRIT, "OPEN", "Targets specified; no latency distribution or energy measurement yet.")),
    ("Causality and safety", "Seeded replay divergence, event loss, resource bounds and actuator-limit violations.",
     (WARN, "PARTIAL", "Bit-exact seeded snapshot replay (growth off); 16 seeded QA scenarios with resource and "
                       "latency bounds.")),
]
tp = 0.35
heads = ["QUESTION", "MEASUREMENT / ACCEPTANCE GATE", "EVIDENCE TODAY  ·  13 NOV 2026"]
hh = 1.15
cx_ = M
for c, hname in zip(cols, heads):
    doc.rect(cx_, y, c, hh, fill=NAVY, name=f"Table header {hname}", paras=[P(hname, hdr)], pad=tp)
    cx_ += c
y += hh
chip_w = 3.2
for ri, (q, gate, (col, tag, ev)) in enumerate(rows):
    ev_w = cols[2] - 2 * tp - chip_w - 0.4
    rh = max(block_height([P(q, q_st)], cols[0] - 2 * tp), block_height([P(gate, c_st)], cols[1] - 2 * tp),
             block_height([P(ev, c_st)], ev_w), 0.95) + 2 * tp
    doc.rect(M, y, CW, rh, fill=PANEL2 if ri % 2 == 0 else WHITE, name=f"Table row {q}")
    doc.text(M + tp, y + tp, cols[0] - 2 * tp, [P(q, q_st)], name=f"Question {q}")
    doc.text(M + cols[0] + tp, y + tp, cols[1] - 2 * tp, [P(gate, c_st)], name=f"Gate {q}")
    chip_st = PStyle(size=15.5, leading=1.1, color=WHITE if col == CRIT else NAVY, bold=True, align="center",
                     letter_spacing=1)
    doc.rect(M + cols[0] + cols[1] + tp, y + tp + 0.02, chip_w, 0.9, fill=col, name=f"Status chip {q}",
             paras=[P(tag, chip_st)], pad=0.05)
    doc.text(M + cols[0] + cols[1] + tp + chip_w + 0.4, y + tp, ev_w, [P(ev, c_st)], name=f"Evidence {q}")
    y += rh
doc.line(M, y, M + CW, y, color=RULE, sw=0.05)
y = text(M, y + 0.3, CW, [P("Reject the hypothesis if transfer gains vanish under matched controls or fail to "
                            "replicate across seeds and tasks.", PStyle(size=23, leading=1.25, color=NAVY,
                                                                        bold=True))], gap=SEC_GAP)

# ===================================================================== ROADMAP
print(f"roadmap starts {y:.2f}")
y = section(M, y, CW, "08", "Roadmap: advance only after each evidence gate")
gates = [
    ("G1  DEVELOPMENT", "Single-neuron growth", "Gate: bounded growth, stable dynamics, reproducible replay.",
     "growth implemented and CI-tested; replay verified with growth off."),
    ("G2  EMBODIMENT", "Connectome + robot", "Gate: audited projections, unseen tasks, retained adaptation.",
     "3 importers, 28 bodies, closed I/O loops; no task passes yet."),
    ("G3  DISTRIBUTED SCALE", "100–400 million neurons",
     "Gate: partition sparse events; measure memory, causal lag and recovery.",
     "1.38 M neurons on 40 shards; 2,101 on 5 nodes."),
    ("G4  SUBSTRATES", "FPAA / neuromorphic", "Gate: compare numerical fidelity, I/O latency and joules per task [9].",
     "verified routing with software fallback; no live analog path; energy unmeasured."),
]
gg = 0.9
gw4 = (CW - 3 * gg) / 4
g_lab = PStyle(size=17.5, leading=1.2, color=TEAL, bold=True, letter_spacing=1.5)
g_ttl = PStyle(size=25, leading=1.18, color=NAVY, bold=True, after=0.12)
g_txt = PStyle(size=19, leading=1.25, color=INK, after=0.15)
g_now = PStyle(size=19, leading=1.25, color=INK2)
gpad = 0.55


def gate_paras(a, b, c, d):
    return [P(a, g_lab), P(b, g_ttl), P(c, g_txt), P([Run("Now: ", bold=True, color=INK), Run(d)], g_now)]


g_h = max(block_height(gate_paras(*g), gw4 - 2 * gpad) for g in gates) + 2 * gpad + 0.2
for i, g in enumerate(gates):
    gx = M + i * (gw4 + gg)
    doc.rect(gx, y, gw4, g_h, fill=PANEL, name=f"Gate card {g[0]}")
    doc.rect(gx, y, gw4, 0.22, fill=TEAL, name=f"Gate card rule {g[0]}")
    doc.text(gx + gpad, y + gpad + 0.12, gw4 - 2 * gpad, gate_paras(*g), name=f"Gate {g[0]}")
    if i < 3:
        doc.line(gx + gw4 + 0.08, y + g_h / 2, gx + gw4 + gg - 0.08, y + g_h / 2, color=TEAL, sw=0.08, end=True,
                 head=0.45)
y += g_h + 0.3
y = text(M, y, CW, [P("Neuron count alone does not establish intelligence, and hardware efficiency must be measured, "
                      "not assumed. Company roadmap: software now, then programmable hardware, then dedicated "
                      "neuromorphic hardware when the evidence justifies it.", SMALL)], gap=SEC_GAP)

# ===================================================================== REFERENCES + COLLAB + QR
refs = [
    "[1] NeuralMimicry Ltd. AARNN Rust source v0.1.43, commit 6d02f24 (23 Oct 2026), audited 13 Nov 2026. "
    "github.com/neuralmimicry/aarnn_rust",
    "[2] T. Busbice. GoPiGo Connectome (2014): C. elegans postsynaptic wiring, vendored as celegans.py in [1].",
    "[3] S. J. Cook et al. Whole-animal connectomes of both Caenorhabditis elegans sexes. Nature 571, 63–71 (2019). "
    "doi:10.1038/s41586-019-1352-7",
    "[4] S. Dorkenwald et al. Neuronal wiring diagram of an adult brain. Nature 634, 124–138 (2024). "
    "doi:10.1038/s41586-024-07558-y",
    "[5] A. S. Bates et al. Distributed control circuits across a brain-and-cord connectome. Nature (2026). "
    "doi:10.1038/s41586-026-10735-w",
    "[6] S. Olkowicz et al. Birds have primate-like numbers of neurons in the forebrain. PNAS 113, 7255–7260 (2016). "
    "doi:10.1073/pnas.1517131113",
    "[7] F. A. C. Azevedo et al. Equal numbers of neuronal and nonneuronal cells make the human brain an isometrically "
    "scaled-up primate brain. J. Comp. Neurol. 513, 532–541 (2009). doi:10.1002/cne.21974",
    "[8] E. M. Izhikevich. Simple model of spiking neurons. IEEE Trans. Neural Netw. 14, 1569–1572 (2003). "
    "doi:10.1109/TNN.2003.820440",
    "[9] D. V. Christensen et al. 2022 roadmap on neuromorphic computing and engineering. Neuromorph. Comput. Eng. 2, "
    "022501 (2022). doi:10.1088/2634-4386/ac4a83",
    "[10] NeuralMimicry Ltd. Evelyn stage-gate record, docs/ARCHITECTURE.md, github.com/neuralmimicry/evelyn; "
    "summary at neuralmimicry.ai/news (Oct 2026).",
    "[11] M. Tsodyks, H. Markram. The neural code between neocortical pyramidal neurons depends on neurotransmitter "
    "release probability. PNAS 94, 719–723 (1997). doi:10.1073/pnas.94.2.719",
]
ref_st = PStyle(size=15.5, leading=1.22, color=INK2, after=0.07)
qr_w = 6.4
collab_w = 20.5
ref_w = CW - collab_w - qr_w - 2 * 1.0
half_refs = (len(refs) + 1) // 2
rcol_w = (ref_w - 0.7) / 2
print(f"refs start {y:.2f}")
text(M, y, ref_w, [P("SELECTED EVIDENCE AND REFERENCES", LABEL)])
ry0 = y + 0.95
h1 = doc.text(M, ry0, rcol_w, [P(r, ref_st) for r in refs[:half_refs]], name="References A")
h2 = doc.text(M + rcol_w + 0.7, ry0, rcol_w, [P(r, ref_st) for r in refs[half_refs:]], name="References B")
cx2 = M + ref_w + 1.0
cb = bullet_style(PStyle(size=17.5, leading=1.25, color=INK), after=0.06)
collab = [
    P("COLLABORATION", LABEL),
    P("NeuralMimicry welcomes collaboration with academia, industry and public-sector partners. The open gates "
      "above define where help moves the evidence:", PStyle(size=17.5, leading=1.25, color=INK, before=0.2,
                                                             after=0.12)),
    B("FPAA and neuromorphic hardware groups: joules per task and latency for the 7 candidate kernels.", cb),
    B("Connectomics groups: sign- and transmitter-aware import.", cb),
    B("Robotics labs: pre-registered, held-out sensorimotor tasks.", cb),
]
ch_ = block_height(collab, collab_w - 1.1) + 1.1
doc.rect(cx2, y, collab_w, ch_, fill=PANEL, name="Collaboration panel")
doc.text(cx2 + 0.55, y + 0.55, collab_w - 1.1, collab, name="Collaboration text")
qx = W - M - qr_w
qr = segno.make("https://github.com/neuralmimicry/aarnn_rust", error="m")
qbuf = io.BytesIO()
qr.save(qbuf, kind="svg", scale=10, border=2, dark=NAVY, xmldecl=True, svgns=True)
pbuf = io.BytesIO()
qr.save(pbuf, kind="png", scale=12, border=2, dark=NAVY)
doc.image(qx, y, qr_w, qr_w, "qr_source", qbuf.getvalue(), pbuf.getvalue())
qy = y + qr_w + 0.15
doc.text(qx - 1.5, qy, qr_w + 1.5, [P("Audited source", PStyle(size=17, leading=1.2, color=NAVY, bold=True,
                                                                align="end")),
                                     P("github.com/neuralmimicry/aarnn_rust",
                                       PStyle(size=13, leading=1.2, color=INK2, align="end"))], name="QR caption")
y_end = max(ry0 + max(h1, h2), y + ch_, qy + 1.9)
print(f"refs h {max(h1, h2):.2f} collab h {ch_:.2f} qr bottom {qy + 1.9 - y:.2f}")

# ===================================================================== FOOTER
fy = H - 1.7
doc.rect(0, fy, W, 1.7, fill=NAVY, name="Footer band")
foot = PStyle(size=17.5, leading=1.2, color=WHITE)
doc.text(M, fy + 0.5, 55, [P([Run("NEURALMIMICRY", bold=True, color=TEAL_L),
                              Run("   ·   Innovating Technologies for Neuromorphic Computing   ·   Royal Society, "
                                  "London, 2026")], foot)], name="Footer left")
doc.text(W - M - 26, fy + 0.5, 26, [P("neuralmimicry.ai   ·   paul@neuralmimicry.ai",
                                      PStyle(size=17.5, leading=1.2, color=WHITE, align="end"))], name="Footer right")

print(f"columns start {ytop:.2f}; bottoms {[round(v, 2) for v in col_bottom]}")
print(f"content bottom {y_end:.2f} cm; footer top {fy:.2f} cm; slack {fy - y_end:.2f} cm")
doc.save(OUT)
print(f"wrote {OUT}")
