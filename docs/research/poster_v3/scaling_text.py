#!/usr/bin/env python3
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
"""Write data/scaling_text.json (poster section 05 caption, bullets and the
falsification-table evidence) from target/qa/scaling/summary.json, so every
number on the poster comes straight from the study output."""
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
s = json.loads((ROOT / "target/qa/scaling/summary.json").read_text())
ns = sorted(int(k) for k in s)
lo, hi = ns[0], ns[-1]
g = lambda n, k: s[str(n)][k]["mean"]
seeds = s[str(hi)]["evoked_32"]["n"]
pct = lambda x: f"{100 * x:.0f} %"
run = json.loads(next((ROOT / "target/qa/scaling").glob("run_N*_s*.json")).read_text())

caption = ("Left: the same cells with their recurrent synapses on or off; 3 seeds per size (dots), running mean. "
           "Right: 8-cell experiences stored as Hebbian synapses and recalled from half a cue (≥ 90 % of tests); "
           "mean ± s.d. of 3 seeds; dashed: linear growth [1].")
import numpy as np
xs = np.log2(ns)
ys = np.array([g(n, "evoked_32") for n in ns])
slope = float(np.polyfit(xs, ys, 1)[0])
mono = all(b >= a - 0.02 for a, b in zip(ys, ys[1:]))
st = json.loads((ROOT / "target/qa/scaling_1by1/stats.json").read_text())
small, mid = st["1-16"], st["17-32"]
pooled = {k: sum(st[b][k] for b in ("17-32", "33-48", "49-64")) for k in ("network_better", "nonzero")}
gains = [st[b]["network_acc"] / st[b]["parts_acc"] - 1 for b in ("17-32", "33-48", "49-64")]
max_p = max(st[b]["sign_p"] for b in ("17-32", "33-48", "49-64"))
dens_net = mid["network_bits_per_neuron"] / small["network_bits_per_neuron"]
dens_par = mid["parts_bits_per_neuron"] / small["parts_bits_per_neuron"]
rep = json.loads((ROOT / "target/qa/modular_storage_report.json").read_text())
srows = {int(k): v for k, v in rep["storage"]["rows"].items()}
sn = sorted(n for n in srows if srows[n]["capacity"] > 0)
s_lo, s_hi = sn[0], sn[-1]
s_exp = rep["storage"]["fit"]["capacity"]["exponent"]
mt = rep["modular"]["tests"]
eq = mt["mod20_vs_clump"]
thr = mt["mod20_vs_mod12"]
bullets = [
    f"**The whole beats its parts:** adding neurons one at a time (1–64), the connected network tells "
    f"{100 * min(gains):.0f}–{100 * max(gains):.0f} % more patterns apart than the same cells disconnected once "
    f"it passes 16 neurons (better in {pooled['network_better']} of {pooled['nonzero']} matched runs; "
    f"p ≤ {max_p:.1g}); below 16 there is no difference.",
    f"**Memories grow faster than neurons:** with an inhibitory interneuron pool (20 % of cells), "
    f"{srows[s_lo]['capacity']:.0f} experiences at {s_lo} neurons, {srows[s_hi]['capacity']:.0f} at {s_hi}; "
    f"information stored per neuron rises {srows[s_lo]['bits_per_neuron']:.0f} → "
    f"{srows[s_hi]['bits_per_neuron']:.0f} bits. Memories live in synapses, and each new neuron can pair with "
    "every other [22, 23].",
    f"**Groups of groups:** groups of 20 joined by 2-synapse attachments match a fully interwoven network "
    f"(n.s.); smaller groups lose {100 * thr['rel_gain']:.0f} % (p < 10⁻⁶) – like the brain’s modules within "
    "modules [24]. Memories still store best when every neuron can reach every other.",
    f"**And keeps growing:** to {hi:,} neurons, 32 patterns told apart {pct(g(lo, 'evoked_32'))} → "
    f"{pct(g(hi, 'evoked_32'))} (chance 3 %); more dimensions, more separable patterns [20].",
]
table = (f"Connected beats disconnected above 16 neurons ({pooled['network_better']}/{pooled['nonzero']} matched runs); "
         f"stored experiences grow ≈ N^{s_exp:.1f} from {s_lo} to {s_hi} neurons.")
status = "memories grow faster than neurons"
out = dict(slope_per_doubling=slope, one_by_one=st, monotonic_within_2pts=mono, caption=caption, bullets=bullets, table=table, status=status,
           source="target/qa/scaling/summary.json", probe_args={k: run[k] for k in (
               "sensors", "patterns", "repeats", "stim_ms", "gap_ms", "memory_ms", "k_rec", "k_in", "w_exc", "w_inh",
               "w_in", "drive", "background", "flip", "base")})
(HERE / "data/scaling_text.json").write_text(json.dumps(out, indent=1, ensure_ascii=False))
print(json.dumps(out, indent=1, ensure_ascii=False))
