#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
"""Build the AARNN Royal Society A0 portrait poster (v3) as an editable .odg.

    <python with matplotlib, Pillow, segno> docs/research/poster_v3/build_poster.py
    soffice --headless --convert-to pdf --outdir docs/research docs/research/<name>.odg

Every statement traces to the aarnn_rust working tree of 23 Nov 2026 (v0.1.43
+ changes listed in FACTS.md), its study outputs under target/qa/, the v2
audit (docs/research/poster_v2/FACTS.md) or the cited literature (verified
against Crossref/PubMed on 23 Nov 2026). See FACTS.md alongside.
"""
from __future__ import annotations

import io
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, str(HERE))

import segno  # noqa: E402

from figures import (layouts_figure, learning_figure, scaling_two_panel_figure,  # noqa: E402
                     whole_and_storage_figure)
from odg import Doc, P, PStyle, Run, block_height, text_width_cm  # noqa: E402

OUT = HERE.parent / "AARNN-Royal-Society-A0-Portrait-v3-Editable.odg"
Q = ROOT / "target/qa"

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
SEC_GAP = 0.9

# ---------------------------------------------------------------- type scale
BODY = PStyle(size=21.0, leading=1.27, color=INK, after=0.2)
SMALL = PStyle(size=18.5, leading=1.26, color=INK2, after=0.0)
SMALL_INK = PStyle(size=18.5, leading=1.26, color=INK, after=0.0)
CAPTION = PStyle(size=17.0, leading=1.26, color=INK2, after=0.0)
LABEL = PStyle(size=18.0, leading=1.2, color=TEAL, bold=True, letter_spacing=1.5)
H_SEC = PStyle(size=38.0, leading=1.1, color=NAVY, bold=True)
H_NUM = PStyle(size=38.0, leading=1.1, color=TEAL, bold=True)
H_SUB = PStyle(size=21.0, leading=1.2, color=NAVY, bold=True, after=0.1)


def bullet_style(base: PStyle, after=0.15) -> PStyle:
    hang = text_width_cm("• ", base.size, bold=True)
    return PStyle(size=base.size, leading=base.leading, color=base.color, after=after, hang=hang)


BUL = bullet_style(BODY)
BUL_S = bullet_style(SMALL_INK, after=0.12)


def B(text, style=BUL):
    return P(text, style, bullet="• ")


doc = Doc(W, H,
          title="Growing Embodied Intelligence from a Single Neuron (v3)",
          creator="Paul B. Isaac’s",
          description=("A0 portrait poster (84.1 x 118.9 cm), Royal Society 'Innovating technologies for "
                       "neuromorphic computing', 23-25 Nov 2026. Native editable text and shapes; figures are "
                       "data-derived SVG; video stills are frames of the showcase videos. Evidence as of 23 Nov 2026."))


def section(x, y, w, num, title):
    nw = text_width_cm(num + "  ", H_NUM.size, bold=True)
    doc.text(x, y, nw + 0.2, [P(num, H_NUM)], name=f"Section {num} number")
    hh = doc.text(x + nw, y, w - nw, [P(title, H_SEC)], name=f"Section {num} title")
    doc.line(x, y + hh + 0.2, x + w, y + hh + 0.2, color=TEAL, sw=0.07)
    return y + hh + 0.6


def text(x, y, w, paras, gap=0.0, **kw):
    h = doc.text(x, y, w, paras, **kw)
    return y + h + gap


# ===================================================================== HEADER
HEADER_H = 14.6
doc.rect(0, 0, W, HEADER_H, fill=NAVY, name="Header band")
doc.rect(0, HEADER_H, W, 0.3, fill=TEAL, name="Header rule")
doc.text(M, 1.1, 30, [P("NEURALMIMICRY", PStyle(size=29, leading=1.1, color=TEAL_L, bold=True,
                                                    letter_spacing=4))], name="Brand")
right = PStyle(size=20, leading=1.25, color=WHITE, align="end", letter_spacing=1)
right_b = PStyle(size=20, leading=1.25, color=TEAL_XL, align="end", letter_spacing=1, bold=True)
doc.text(W - M - 54, 1.05, 54, [P("ROYAL SOCIETY  ·  INNOVATING TECHNOLOGIES FOR NEUROMORPHIC COMPUTING", right_b),
                                P("LONDON  ·  23–25 NOVEMBER 2026", right)], name="Meeting")
TITLE = PStyle(size=92, leading=1.0, color=WHITE, bold=True)
y = 2.95 + doc.text(M, 2.95, CW, [P("Growing Embodied Intelligence\nfrom a Single Neuron", TITLE)], name="Title")
SUB = PStyle(size=35, leading=1.15, color=TEAL_XL)
y = text(M, y + 0.2, CW, [P("More neurons, more room to compute: biomimetic AARNN circuits now move nine bodies, "
                            "sit where biology puts them, and keep innate gait while specialised cells stay plastic", SUB)],
         name="Subtitle")
AUTH = PStyle(size=26, leading=1.2, color=WHITE)
text(M, y + 0.35, CW, [P([Run("Paul B. Isaac’s", bold=True),
                          Run("   ·   NeuralMimicry Ltd, Rugby, Warwickshire, United Kingdom   ·   "),
                          Run("paul@neuralmimicry.ai   ·   neuralmimicry.ai", color=TEAL_XL)], AUTH)], name="Author")

# ===================================================================== HYPOTHESIS + STATUS
y0 = HEADER_H + 1.2
hyp_w = 2 * COLW + G
pad = 0.8
hyp_paras = [
    P("RESEARCH HYPOTHESIS", LABEL),
    P("Intelligence becomes possible as neurons accumulate: recursive, embodied spiking circuits that grow and "
      "adapt under local rules gain room to represent more with every neuron added – long before the "
      "human brain’s ≈86 billion [2].",
      PStyle(size=30, leading=1.2, color=NAVY, bold=True, before=0.25, after=0.3)),
    P("Across mammals and birds, the absolute number of cortical or pallial neurons is a good correlate of "
      "cognitive capability [3]; a raven’s pallium holds ≈1.2 billion neurons [4]. AARNN (Auto-Asynchronous "
      "Recursive Neuromorphic Network) is the open, inspectable test-bed for the claim. Since our 13 Nov poster it "
      "has gained **nine moving bodies**, **3D biological layouts**, **stable plasticity** and a **measured "
      "neuron-count scaling curve**. Every number traces to the source [1], its study outputs or the cited "
      "literature.", PStyle(size=21, leading=1.28, color=INK)),
]
hyp_h = block_height(hyp_paras, hyp_w - 2 * pad - 0.45) + 2 * pad

st_x = COLX[2]
st_pad = 0.7
status_rows = [
    (GOOD, "NEW SINCE 13 NOVEMBER",
     "9 robot bodies move by AARNN circuits · neurons placed as in biology · plasticity stabilised · plasticity "
     "confined to specialised cells keeps innate gait · " + (json.loads((HERE / "data/scaling_text.json").read_text())["status"]
                                                if (HERE / "data/scaling_text.json").exists()
                                                else "neuron-count scaling study running")),
    (WARN, "MEASURED AT SMALL SCALE",
     "2,101 neurons on 5 nodes · a 1.38 M-neuron layer on 40 shards · 2–7 ms median brain reply per exchange"),
    (CRIT, "NOT YET SHOWN",
     "Controlled conditioning · bounded growth · held-out task transfer · energy per task · live analog FPAA"),
]
dot = 0.7
row_w = COLW - 2 * st_pad - dot - 0.4
st_label = PStyle(size=18, leading=1.2, color=NAVY, bold=True, letter_spacing=1)
st_body = PStyle(size=18.5, leading=1.25, color=INK)
rows_h = [block_height([P(lab, st_label), P(body, st_body)], row_w) for _, lab, body in status_rows]
st_h = 2 * st_pad + 1.1 + sum(rows_h) + 0.34 * (len(rows_h) - 1)
box_h = max(hyp_h, st_h)
doc.rect(M, y0, hyp_w, box_h, fill=PANEL, name="Hypothesis panel")
doc.rect(M, y0, 0.45, box_h, fill=TEAL, name="Hypothesis accent")
doc.text(M + 0.45 + pad, y0 + pad, hyp_w - 2 * pad - 0.45, hyp_paras, name="Hypothesis text")
doc.rect(st_x, y0, COLW, box_h, fill=WHITE, stroke=RULE, sw=0.06, name="Status panel")
doc.text(st_x + st_pad, y0 + st_pad, COLW - 2 * st_pad, [P("STATUS AT A GLANCE  ·  23 NOV 2026", LABEL)],
         name="Status label")
yy = y0 + st_pad + 1.1
for (col, lab, body), rh in zip(status_rows, rows_h):
    doc.ellipse(st_x + st_pad, yy + 0.08, dot, dot, fill=col, name=f"Status dot {lab}")
    doc.text(st_x + st_pad + dot + 0.4, yy, row_w, [P(lab, st_label), P(body, st_body)], name=f"Status {lab}")
    yy += rh + 0.34

ytop = y0 + box_h + 1.2
col_bottom = []

# ===================================================================== COLUMN 1
x, w = COLX[0], COLW
y = section(x, ytop, w, "01", "AARNN in one loop")
y = text(x, y, w, [P("An open-source Rust runtime in which a spiking network grows, wires and adapts while it runs, "
                     "closed in a loop with a simulated body. Learning is local and online: no backpropagation [1].",
                     BODY)])
dy = y + 0.15
lm = 1.0
bw = w - 2 * lm
node = PStyle(size=18.5, leading=1.18, color=NAVY, bold=True, align="center")
node_s = PStyle(size=16, leading=1.2, color=INK2, align="center")
node_w = PStyle(size=19, leading=1.18, color=WHITE, bold=True, align="center")
node_ws = PStyle(size=16, leading=1.2, color=TEAL_XL, align="center")
half = (bw - 0.7) / 2
gap_v = 0.9
sh = 2.2
doc.rect(x + lm, dy, half, sh, fill=PANEL, stroke=TEAL, sw=0.06, name="Seed: single neuron",
         paras=[P("Single-neuron seed", node), P("default: 1 hidden neuron", node_s)], pad=0.2)
doc.rect(x + lm + half + 0.7, dy, half, sh, fill=PANEL, stroke=TEAL, sw=0.06, name="Seed: connectome",
         paras=[P("Connectome or circuit", node), P("placed in 3D as in biology", node_s)], pad=0.2)
cy = dy + sh + gap_v
ch = 2.5
c1x = x + lm + half / 2
c2x = x + lm + half * 1.5 + 0.7
doc.line(c1x, dy + sh, c1x, cy, color=NAVY, sw=0.07, end=True, head=0.5)
doc.line(c2x, dy + sh, c2x, cy, color=NAVY, sw=0.07, end=True, head=0.5)
doc.rect(x + lm, cy, bw, ch, fill=NAVY, name="Spiking core",
         paras=[P("Spiking core  ·  fixed time-step", node_w),
                P("Izhikevich or graded cells [9] · AMPA/NMDA/GABA · conduction delays · gap junctions", node_ws)],
         pad=0.25)
ry = cy + ch + gap_v
rh2 = 2.5
doc.rect(x + lm, ry, half, rh2, fill=WHITE, stroke=TEAL, sw=0.06, name="Local plasticity",
         paras=[P("Local plasticity", node), P("STDP on existing synapses · homeostatic scaling", node_s)],
         pad=0.2)
doc.rect(x + lm + half + 0.7, ry, half, rh2, fill=WHITE, stroke=TEAL, sw=0.06, name="Structural loop",
         paras=[P("Structural loop", node), P("growth · contact synapses · pruning · myelin", node_s)], pad=0.2)
for cx_ in (c1x, c2x):
    doc.line(cx_ - 0.35, cy + ch, cx_ - 0.35, ry, color=TEAL, sw=0.07, end=True, head=0.42)
    doc.line(cx_ + 0.35, ry, cx_ + 0.35, cy + ch, color=TEAL, sw=0.07, end=True, head=0.42)
by = ry + rh2 + gap_v
bh = 2.2
doc.rect(x + lm, by, bw, bh, fill=PANEL, stroke=TEAL, sw=0.06, name="Embodiment",
         paras=[P("Body  ·  MuJoCo / Webots, lock-step every 2 ms", node),
                P("sensors → spike trains   ·   motor cells → actuators", node_s)], pad=0.2)
xr = x + w - 0.4
xl = x + 0.4
doc.polyline([(x + lm + bw, cy + ch * 0.5), (xr, cy + ch * 0.5), (xr, by + bh * 0.5), (x + lm + bw, by + bh * 0.5)],
             color=NAVY, sw=0.07, end=True, name="Motor path", head=0.5)
doc.polyline([(x + lm, by + bh * 0.5), (xl, by + bh * 0.5), (xl, cy + ch * 0.5), (x + lm, cy + ch * 0.5)],
             color=NAVY, sw=0.07, end=True, name="Sensory path", head=0.5)
y = by + bh + 0.5
y = text(x, y, w, [
    B("**Recursive and multi-rate:** within-layer recurrence and feedback; spikes advance every step while growth, "
      "pruning and morphology run on slower cadences [1]."),
], gap=SEC_GAP)

y = section(x, y, w, "02", "Nine bodies, moving by biomimicry")
y = text(x, y, w, [P("Each body runs closed-loop in MuJoCo with a circuit designed after its animal’s nervous system: "
                     "half-centre rhythm generators, reflexes, a hindbrain that commands, turns and backs off [1].",
                     BODY)], gap=0.1)
robots = [
    ("C. elegans", "302", "connectome with 909 gap-junction pairs [7]; head-first crawl, 0.8 Hz body wave"),
    ("Zebrafish larva", "55", "head-to-tail swimming wave, 4.5 Hz, 0.64 body lengths/s; avoids walls"),
    ("Fruit fly (BANC body)", "29", "takes off at 4.2 s, then 20.8 s of continuous flight; 40 Hz wingbeat"),
    ("Hexapod", "24", "reflex stepping (Cruse rules), 1.9 Hz; tilt ≤ 4°"),
    ("NAO humanoid (OEM)", "31", "walks with weight shift; sonar: turns from walls; never falls in 3 × 200 s"),
    ("UGOT quadruped (OEM)", "105", "1.9 Hz trot, run-and-turn exploring; never falls in 40 s"),
    ("UGOT balancer (OEM)", "20", "inverted-pendulum balance for 30 s, tilt ≤ 7.6°"),
    ("Carrion crow", "71", "flapping flight: 97 m in 10.6 s at 9.9 m/s, 4.3 Hz"),
    ("Keenon T8", "22", "explores a room, turns 0.3–0.8 m before obstacles"),
]
rb_n = PStyle(size=17.5, leading=1.2, color=NAVY, bold=True)
rb_c = PStyle(size=17.5, leading=1.2, color=TEAL, bold=True, align="end")
rb_t = PStyle(size=16.5, leading=1.23, color=INK)
nw_ = 8.1
cw_ = 3.4
for i, (name, cells, beh) in enumerate(robots):
    th = max(block_height([P(beh, rb_t)], w - nw_ - cw_ - 0.8), block_height([P(name, rb_n)], nw_ - 0.3)) + 0.3
    if i % 2 == 0:
        doc.rect(x, y, w, th, fill=PANEL2, name=f"Robot row {name}")
    doc.text(x + 0.25, y + 0.15, nw_ - 0.3, [P(name, rb_n)], name=f"Robot {name}")
    doc.text(x + nw_, y + 0.15, cw_, [P(cells, rb_c)], name=f"Robot cells {name}")
    doc.text(x + nw_ + cw_ + 0.45, y + 0.15, w - nw_ - cw_ - 0.6, [P(beh, rb_t)], name=f"Robot behaviour {name}")
    y += th
y = text(x, y + 0.25, w, [P("Cells = neurons in the circuit. Median brain reply 1.9–6.8 ms per 2 ms body frame on one "
                            "16-core host. Circuits are designed, not trained [1].", CAPTION)], gap=0.35)
STILLS = HERE / "data/stills"


def place_still(x, y, w, stem, caption):
    path = STILLS / f"{stem}.jpg"
    if not path.exists():
        return y
    from PIL import Image
    im = Image.open(path)
    sh_ = w * im.height / im.width
    doc.raster(x, y, w, sh_, f"still_{stem}", path.read_bytes())
    return text(x, y + sh_ + 0.12, w, [P(caption, CAPTION)])


def still_height(w, stem):
    path = STILLS / f"{stem}.jpg"
    if not path.exists():
        return 0.0
    from PIL import Image
    im = Image.open(path)
    return w * im.height / im.width + 1.7


y = place_still(x, y, w, "celegans", "Showcase video still: the worm crawls (left) while its 302-neuron connectome, "
                                     "drawn at the cells’ anatomical sites, fires in lock-step (right).")
col_bottom.append(y)

# ===================================================================== COLUMN 2
x, w = COLX[1], COLW
y = section(x, ytop, w, "03", "Neurons where biology puts them")
y = text(x, y, w, [P("Space matters for timing and growth, so every soma now sits at a biologically equivalent "
                     "site – for synthetic bodies, in the nervous system of the animal the body resembles [1].",
                     BODY)])
lf_h = 13.8
panels = [("C. elegans · 302 neurons", ROOT / "target/qa/celegans-depth-study-forward-crawl/network_grafted.json",
           "WormAtlas soma map"),
          ("Zebrafish larva · 55 cells", Q / "spatial/showcase/network_zebrafish_layout.json",
           "hindbrain + 8 spinal segments"),
          ("NAO humanoid · 31 cells", Q / "spatial/showcase/network_nao_layout.json",
           "human-equivalent brainstem and cord"),
          ("Carrion crow · 71 cells", Q / "spatial/showcase/network_crow_layout.json",
           "tectum, brachial and lumbar cord")]
svg, png = layouts_figure(panels, w, lf_h)
doc.image(x, y, w, lf_h, "fig_layouts", svg, png)
y += lf_h + 0.25
y = text(x, y, w, [
    B("**Fly** cells sit at their BANC-annotated neuropil [8]; **NAO** gets a human cord with cervical and "
      "lumbar enlargements; wheeled and four-legged robots a mammalian cord; the **crow** an avian one.", BUL_S),
    B("Placement alone leaves dynamics bit-identical; with morphology and conduction delays on, circuits still "
      "work (zebrafish 0.69 vs 0.63 body lengths/s; NAO 0.586 vs 0.582 m in 30 s) [1].", BUL_S),
], gap=SEC_GAP)

y = section(x, y, w, "04", "Plasticity that keeps innate skills")
lfig_h = 11.0
svg, png = learning_figure(Q / "complexity/celegans/spatial_learn.json", Q / "complexity/celegans/spatial_iso.json",
                           Q / "complexity/nao/spatial_learn.json", Q / "complexity/nao/spatial_iso.json", w, lfig_h)
doc.image(x, y, w, lfig_h, "fig_learning", svg, png)
y += lfig_h + 0.2
y = text(x, y, w, [
    B("**Found and fixed:** STDP was applied to every entry of dense weight matrices, so learning wired up absent "
      "synapses. In 8 s the worm’s input synapses went from 246 to 12,381 and 88 of 96 inhibitory outputs vanished; "
      "all 302 cells fired at ≈320 Hz in lock-step. Learning now changes only synapses that exist; making new ones is "
      "growth’s job [1].", BUL_S),
    B("**Homeostatic counterweight:** each cell defends its own firing-rate set point [13] by multiplicative scaling "
      "– excitation down and inhibition up when over-active [10, 11], the compensation Hebbian learning needs [12]. "
      "Worm with learning: 9.8–9.9 Hz over 40 s and coherence between ganglia 0.07 (before the fix 0.98), as "
      "without learning (9.5–10.1 Hz; 0.07).", BUL_S),
    B("**Specialised learning:** in C. elegans, locomotion runs on motor-circuit rhythm generators [16, 17] while "
      "memory sits at sensory→interneuron synapses [15]. Letting only the amphid interneurons AIA, AIB, AIY and AIZ "
      "learn [14] keeps the crawl (0.125–0.146 body lengths/s, as without learning); NAO learning only in its sonar "
      "and decision cells walks 15.7–16.0 body lengths without falling.", BUL_S),
    B("**Like babies:** newborn stepping already holds two locomotor primitives kept for life, with new ones added "
      "in toddlers [18]; adults adapt gait through the cerebellum [19]. AARNN now follows this design: an innate "
      "scaffold plus specialised plastic circuits.", BUL_S),
])
col_bottom.append(y)

# ===================================================================== COLUMN 3
x, w = COLX[2], COLW
y = section(x, ytop, w, "05", "More neurons, more room to compute")
summary = Q / "scaling/summary.json"
SCALE_TEXT = HERE / "data/scaling_text.json"
y = text(x, y, w, [P("Does a network hold more than its neurons would alone? Same cells, same inputs – only the "
                     "number of neurons, or their connections, change [1].", BODY)])
one_by_one = Q / "scaling_1by1/summary.json"
storage_report = Q / "modular_storage_report.json"
if one_by_one.exists() and storage_report.exists():
    sf_h = 11.0
    svg, png = whole_and_storage_figure(one_by_one, storage_report, w, sf_h)
    doc.image(x, y, w, sf_h, "fig_scaling", svg, png)
    y += sf_h + 0.15
elif summary.exists() and one_by_one.exists():
    sf_h = 11.0
    svg, png = scaling_two_panel_figure(one_by_one, summary, w, sf_h)
    doc.image(x, y, w, sf_h, "fig_scaling", svg, png)
    y += sf_h + 0.15
st = json.loads(SCALE_TEXT.read_text()) if SCALE_TEXT.exists() else {}
y = text(x, y, w, [P(st.get("caption", "Scaling study pending."), CAPTION)], gap=0.3)
y = text(x, y, w, [B(b, BUL_S) for b in st.get("bullets", [])], gap=0.2)
y = text(x, y, w, [
    B("**Beyond the bench:** 2,101 neurons ran on five nodes, a 1.38 M-neuron layer on 40 shards [21]; in "
      "nature, neuron numbers jumped with birds and mammals [5].", BUL_S),
], gap=SEC_GAP)

y = section(x, y, w, "06", "Toward hardware: the minimum kernel")
y = text(x, y, w, [P("A complexity ladder strips each robot’s circuit down until behaviour breaks; the smallest "
                     "working tier is the kernel to program into a field-programmable analog array (FPAA) [1].",
                     BODY)])
tiers = [("C. elegans", "T0: spikes / graded release only"), ("NAO", "T0"),
         ("Zebrafish", "T4: + active dendrites, NMDA gating"), ("Hexapod", "T4")]
kv_k = PStyle(size=18, leading=1.25, color=NAVY, bold=True)
kv_v = PStyle(size=18, leading=1.25, color=INK)
for i, (k, v) in enumerate(tiers):
    rh = block_height([P(v, kv_v)], w - 6.2) + 0.3
    if i % 2 == 0:
        doc.rect(x, y, w, rh, fill=PANEL2, name=f"Tier row {k}")
    doc.text(x + 0.3, y + 0.15, 5.6, [P(k, kv_k)], name=f"Tier {k}")
    doc.text(x + 6.0, y + 0.15, w - 6.2, [P(v, kv_v)], name=f"Tier value {k}")
    y += rh
y = text(x, y + 0.25, w, [
    B("Seven kernels have FPAA candidates (synaptic filter, short-term plasticity, adaptive threshold and "
      "homeostasis, active dendrite, gap-junction field, transmission, triplet/scaling/Dale), each with Xcos, "
      "Okika Pi.Ka and SPICE models; a kernel routes to the FPAA only after its checks pass, otherwise to "
      "software [1].", BUL_S),
    B("Spikes travel as AER events (AARNN-AER/1 with CRC-16; AER-CAN; UDP fabric bridge) [1].", BUL_S),
], gap=0.4)
if y + still_height(w, "nao") <= max(col_bottom) + 0.2:
    y = place_still(x, y, w, "nao", "NAO walks and avoids walls with 31 cells in a human-equivalent brainstem and "
                                    "cord; the gait rhythm runs in the lumbar cells (right).")
col_bottom.append(y)

# ===================================================================== FALSIFICATION TABLE
y = max(col_bottom) + 1.2
print(f"table starts {y:.2f}")
y = section(M, y, CW, "07", "Experiments that can falsify it – evidence today")
cols = [12.2, 27.8, CW - 12.2 - 27.8]
hdr = PStyle(size=17.5, leading=1.2, color=WHITE, bold=True, letter_spacing=1)
q_st = PStyle(size=19, leading=1.22, color=NAVY, bold=True)
c_st = PStyle(size=18, leading=1.25, color=INK)
rows = [
    ("Capacity vs neurons", "Decoding accuracy rises with N at fixed in-degree; separate seeds and loads.",
     (GOOD, "SHOWN", None)),
    ("Topology fidelity", "Retained nodes, edges, signs, delays; declared projection changes.",
     (WARN, "PARTIAL", "Worm import now signed (26 GABAergic cells) and includes gap junctions [7]; fly "
                       "projections still remap ≈63 % of kept edges.")),
    ("Behaviour", "Task success and recovery on unseen tasks versus silent, random and conventional controllers.",
     (WARN, "PARTIAL", "Nine bodies locomote closed-loop with designed circuits; no held-out task or matched "
                       "controller yet.")),
    ("Learning and retention", "Controlled (paired vs unpaired) learning; innate skills retained while learning.",
     (WARN, "PARTIAL", "Specialised learning keeps gait in worm and NAO; no controlled conditioning in any "
                       "robot yet.")),
    ("Timing and energy", "p95/p99 sensor-to-actuator latency; whole-system joules per task [6].",
     (CRIT, "OPEN", "Median reply 1.9–6.8 ms per exchange (fly flight p95 3.7 ms); no energy measurement yet.")),
]
cap_text = json.loads(SCALE_TEXT.read_text()).get("table") if SCALE_TEXT.exists() else None
rows[0] = (rows[0][0], rows[0][1], (GOOD, "SHOWN", cap_text) if cap_text else (WARN, "RUNNING",
                                                                                 "Scaling study in progress."))
tp = 0.32
heads = ["QUESTION", "MEASUREMENT / ACCEPTANCE GATE", "EVIDENCE TODAY  ·  23 NOV 2026"]
hh = 1.1
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
    chip_st = PStyle(size=15, leading=1.1, color=WHITE if col in (CRIT, GOOD) else NAVY, bold=True, align="center",
                     letter_spacing=1)
    doc.rect(M + cols[0] + cols[1] + tp, y + tp + 0.02, chip_w, 0.9, fill=col, name=f"Status chip {q}",
             paras=[P(tag, chip_st)], pad=0.05)
    doc.text(M + cols[0] + cols[1] + tp + chip_w + 0.4, y + tp, ev_w, [P(ev, c_st)], name=f"Evidence {q}")
    y += rh
doc.line(M, y, M + CW, y, color=RULE, sw=0.05)
y = text(M, y + 0.3, CW, [P("Reject the hypothesis if capacity stops growing with neurons, or if learning gains vanish "
                            "under matched controls and fail to replicate across seeds and tasks.",
                            PStyle(size=22, leading=1.25, color=NAVY, bold=True))], gap=SEC_GAP)

# ===================================================================== ROADMAP
print(f"roadmap starts {y:.2f}")
y = section(M, y, CW, "08", "Roadmap: advance only after each evidence gate")
gates = [
    ("G1  DEVELOPMENT", "Single-neuron growth", "Gate: bounded growth, stable dynamics, reproducible replay.",
     "growth implemented; sprouting still unbounded in designed circuits; stable learning shown."),
    ("G2  EMBODIMENT", "Connectome + robot", "Gate: unseen tasks, retained adaptation, controlled learning.",
     "nine bodies move; innate gait kept while learning; no controlled conditioning yet."),
    ("G3  DISTRIBUTED SCALE", "100–400 million neurons",
     "Gate: capacity keeps rising; memory, causal lag and recovery measured.",
     "1.38 M neurons on 40 shards; 2,101 on 5 nodes."),
    ("G4  SUBSTRATES", "FPAA / neuromorphic", "Gate: fidelity, I/O latency and joules per task [6].",
     "minimum kernels known per robot; no live analog path; energy unmeasured."),
]
gg = 0.9
gw4 = (CW - 3 * gg) / 4
g_lab = PStyle(size=17, leading=1.2, color=TEAL, bold=True, letter_spacing=1.5)
g_ttl = PStyle(size=24, leading=1.18, color=NAVY, bold=True, after=0.12)
g_txt = PStyle(size=18, leading=1.25, color=INK, after=0.15)
g_now = PStyle(size=18, leading=1.25, color=INK2)
gpad = 0.5


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
y += g_h + 0.9

# ===================================================================== REFERENCES + COLLAB + QR
refs = [
    "[1] NeuralMimicry Ltd. AARNN Rust source v0.1.43 (6d02f24) + working tree of 23 Nov 2026; study outputs and "
    "provenance in docs/research/poster_v3/FACTS.md. github.com/neuralmimicry/aarnn_rust",
    "[2] F. A. C. Azevedo et al. J. Comp. Neurol. 513, 532–541 (2009). doi:10.1002/cne.21974",
    "[3] S. Herculano-Houzel. Curr. Opin. Behav. Sci. 16, 1–7 (2017). doi:10.1016/j.cobeha.2017.02.004",
    "[4] S. Olkowicz et al. PNAS 113, 7255–7260 (2016). doi:10.1073/pnas.1517131113",
    "[5] K. Kverková et al. PNAS 119, e2121624119 (2022). doi:10.1073/pnas.2121624119",
    "[6] D. V. Christensen et al. Neuromorph. Comput. Eng. 2, 022501 (2022). doi:10.1088/2634-4386/ac4a83",
    "[7] S. J. Cook et al. Nature 571, 63–71 (2019). doi:10.1038/s41586-019-1352-7",
    "[8] A. S. Bates et al. Nature (2026). doi:10.1038/s41586-026-10735-w",
    "[9] E. M. Izhikevich. IEEE Trans. Neural Netw. 14, 1569–1572 (2003). doi:10.1109/TNN.2003.820440",
    "[10] G. G. Turrigiano et al. Nature 391, 892–896 (1998). doi:10.1038/36103",
    "[11] V. Kilman, M. C. W. van Rossum, G. G. Turrigiano. J. Neurosci. 22, 1328–1337 (2002). "
    "doi:10.1523/JNEUROSCI.22-04-01328.2002",
    "[12] F. Zenke, W. Gerstner. Phil. Trans. R. Soc. B 372, 20160259 (2017). doi:10.1098/rstb.2016.0259",
    "[13] K. B. Hengen et al. Cell 165, 180–191 (2016). doi:10.1016/j.cell.2016.01.046",
    "[14] J. M. Gray, J. J. Hill, C. I. Bargmann. PNAS 102, 3184–3191 (2005). doi:10.1073/pnas.0409009101",
    "[15] H. Kunitomo et al. Nat. Commun. 4, 2210 (2013). doi:10.1038/ncomms3210",
    "[16] Q. Wen et al. Neuron 76, 750–761 (2012). doi:10.1016/j.neuron.2012.08.039",
    "[17] A. D. Fouad et al. eLife 7, e29913 (2018). doi:10.7554/eLife.29913",
    "[18] N. Dominici et al. Science 334, 997–999 (2011). doi:10.1126/science.1210617",
    "[19] S. M. Morton, A. J. Bastian. J. Neurosci. 26, 9107–9116 (2006). doi:10.1523/JNEUROSCI.2622-06.2006",
    "[20] T. M. Cover. IEEE Trans. Electron. Comput. EC-14, 326–334 (1965). doi:10.1109/PGEC.1965.264137",
    "[21] NeuralMimicry Ltd. Evelyn stage-gate record, docs/ARCHITECTURE.md, github.com/neuralmimicry/evelyn.",
    "[22] D. J. Willshaw, O. P. Buneman, H. C. Longuet-Higgins. Nature 222, 960–962 (1969). doi:10.1038/222960a0",
    "[23] G. Palm. Biol. Cybern. 36, 19–31 (1980). doi:10.1007/BF00337019",
    "[24] D. Meunier, R. Lambiotte, E. T. Bullmore. Front. Neurosci. 4, 200 (2010). doi:10.3389/fnins.2010.00200",
]
ref_st = PStyle(size=14.5, leading=1.2, color=INK2, after=0.06)
qr_w = 6.0
collab_w = 19.0
ref_w = CW - collab_w - qr_w - 2 * 0.9
ncols = 3
per = (len(refs) + ncols - 1) // ncols
rcol_w = (ref_w - (ncols - 1) * 0.6) / ncols
print(f"refs start {y:.2f}")
text(M, y, ref_w, [P("REFERENCES", LABEL)])
ry0 = y + 0.9
hs = []
for c in range(ncols):
    hs.append(doc.text(M + c * (rcol_w + 0.6), ry0, rcol_w, [P(r, ref_st) for r in refs[c * per:(c + 1) * per]],
                       name=f"References {c + 1}"))
cx2 = M + ref_w + 0.9
cb = bullet_style(PStyle(size=17, leading=1.25, color=INK), after=0.06)
collab = [
    P("COLLABORATION", LABEL),
    P("We welcome partners from academia, industry and the public sector. Where help moves the evidence:",
      PStyle(size=17, leading=1.25, color=INK, before=0.2, after=0.12)),
    B("FPAA and neuromorphic hardware: joules and latency for the minimum kernels.", cb),
    B("Neuroscience: learning sites and set points for specialised plasticity.", cb),
    B("Robotics: pre-registered, held-out sensorimotor tasks.", cb),
]
ch_ = block_height(collab, collab_w - 1.0) + 1.0
doc.rect(cx2, y, collab_w, ch_, fill=PANEL, name="Collaboration panel")
doc.text(cx2 + 0.5, y + 0.5, collab_w - 1.0, collab, name="Collaboration text")
qx = W - M - qr_w
qr = segno.make("https://github.com/neuralmimicry/aarnn_rust", error="m")
qbuf = io.BytesIO()
qr.save(qbuf, kind="svg", scale=10, border=2, dark=NAVY, xmldecl=True, svgns=True)
pbuf = io.BytesIO()
qr.save(pbuf, kind="png", scale=12, border=2, dark=NAVY)
doc.image(qx, y, qr_w, qr_w, "qr_source", qbuf.getvalue(), pbuf.getvalue())
qy = y + qr_w + 0.15
doc.text(qx - 1.5, qy, qr_w + 1.5, [P("Open source", PStyle(size=16.5, leading=1.2, color=NAVY, bold=True,
                                                             align="end")),
                                     P("github.com/neuralmimicry/aarnn_rust",
                                       PStyle(size=12.5, leading=1.2, color=INK2, align="end"))], name="QR caption")
y_end = max(ry0 + max(hs), y + ch_, qy + 1.8)

# ===================================================================== FOOTER
fy = H - 1.7
doc.rect(0, fy, W, 1.7, fill=NAVY, name="Footer band")
foot = PStyle(size=17, leading=1.2, color=WHITE)
doc.text(M, fy + 0.5, 55, [P([Run("NEURALMIMICRY", bold=True, color=TEAL_L),
                              Run("   ·   Innovating Technologies for Neuromorphic Computing   ·   Royal Society, "
                                  "London, 2026")], foot)], name="Footer left")
doc.text(W - M - 26, fy + 0.5, 26, [P("neuralmimicry.ai   ·   paul@neuralmimicry.ai",
                                      PStyle(size=17, leading=1.2, color=WHITE, align="end"))], name="Footer right")

print(f"columns start {ytop:.2f}; bottoms {[round(v, 2) for v in col_bottom]}")
print(f"content bottom {y_end:.2f} cm; footer top {fy:.2f} cm; slack {fy - y_end:.2f} cm")
doc.save(OUT)
print(f"wrote {OUT}")
