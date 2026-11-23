<!--
SPDX-FileCopyrightText: masterkiga
SPDX-License-Identifier: MIT
-->
# AARNN A0 poster (v3): provenance of every claim

Evidence date: 23 Nov 2026. Source: `aarnn_rust` at commit `6d02f24` (v0.1.43)
plus the uncommitted working tree of 23 Nov 2026. That tree contains the
user's SPDX/licensing work and the changes listed under "Code changed for
this poster". Study outputs live under `target/qa/` (gitignored local build
outputs) and are named per claim below. Claims carried over unchanged from v2
keep the v2 audit (`docs/research/poster_v2/FACTS.md`). The literature was
verified against Crossref and PubMed on 23 Nov 2026 (DOIs resolve; claims were
checked against the abstracts or main text, with the wording adjusted where a
paper says less than first drafted).

Rebuild, with Python and matplotlib, Pillow and segno:

    PYTHONPATH=target/poster-venv/lib/python3.13/site-packages python3 docs/research/poster_v3/build_poster.py
    soffice --headless --convert-to pdf --outdir docs/research docs/research/AARNN-Royal-Society-A0-Portrait-v3-Editable.odg

## Meeting and author
| Claim | Evidence |
|---|---|
| Royal Society, "Innovating technologies for neuromorphic computing", London, 23–25 Nov 2026 | royalsociety.org/science-events-and-lectures/2026/11/neuromorphic-computing/ (rechecked 23 Nov 2026) |
| Author, company, AARNN expansion | v2 FACTS |

## Hypothesis panel
| Claim | Evidence |
|---|---|
| Human brain ≈86 billion neurons | Azevedo et al. 2009 [2]: 86.1 ± 8.1 billion (adult males) |
| Absolute cortical or pallial neuron numbers are a good correlate of cognitive capability across species | Herculano-Houzel 2017 [3]. The publisher's highlights say "good correlates"; the poster does not claim "best" |
| Raven pallium ≈1.2 billion; goldcrest ≈64 million | Olkowicz et al. 2016 [4], Results text and Table S1 |
| Status: 9 moving bodies, layouts, stable learning, specialised learning, capacity from 8 to 1,024 neurons | sections 02–05 below |
| 2,101 neurons on 5 nodes; 1.38 M-neuron layer on 40 shards | v2 FACTS (section 06) |
| 2–7 ms median brain reply per exchange | `docs/lec-crow-delivery.md` results table: per-robot median latency 1.9–6.8 ms (worm ~2, Keenon 1.9–2.6, NAO 2.6–3.7, fish 3.3, balancer 3.5, crow 4.4, UGOT quadruped 2.8–6.8) |

## 01 AARNN in one loop
| Claim | Evidence |
|---|---|
| Rust runtime; growth, wiring and adaptation online; no backpropagation | v2 FACTS (section 01) |
| Izhikevich or graded cells; AMPA/NMDA/GABA; conduction delays; gap junctions | v2 FACTS; graded cells: `docs/lec-crow-delivery.md` "Common algorithm" (graded AARNN cells) |
| Plasticity acts on existing synapses, plus homeostatic scaling | `src/aarnn/plasticity.rs` `apply_weight_delta` and `homeostatic_scale_rows`; `src/runner.rs` `matrix_plasticity_proposals` and `apply_rate_homeostasis` |
| Lock-step with the body every 2 ms | `scripts/run_sim.sh --lockstep`; showcase README |

## 02 Nine bodies
Every row: MuJoCo, lock-step, closed loop.

| Body | Cells | Behaviour | Evidence |
|---|---|---|---|
| C. elegans | 302 | connectome with 909 gap-junction pairs; head-first crawl, 0.8 Hz body wave | 909 = nonzero upper-triangle entries of `w_gap` in the showcase network `target/qa/celegans-depth-study-forward-crawl/network_grafted.json` (neuron-neuron pairs; source Cook et al. 2019 [7] via the import v2; the import notes quote 1,080 pairs from the full Cook edge list); 0.8 Hz: `target/qa/complexity/celegans/spatial_iso.json`, T1/S0 closed `undulation_hz` |
| Zebrafish larva | 55 | wave, 4.5 Hz, 0.64 L/s, wall avoidance | delivery results table (zebrafish v2 row) |
| Fruit fly, BANC body | 29 | lift-off at 4.19 s, 20.8 s of continuous flight, 40.0 Hz wingbeat | delivery "Drosophila flight", closed-loop paragraph |
| Hexapod | 24 | Cruse-rule stepping 1.9 Hz; tilt ≤ 4° | delivery table; showcase README (max tilt 4°) |
| NAO (OEM) | 31 | walk with weight shift; sonar avoidance; never falls in 3 × 200 s | delivery table (NAO walk row) |
| UGOT quadruped | 105 | 1.9 Hz trot, run-and-turn; never falls in 40 s | delivery table |
| UGOT balancer | 20 | 30 s balance, max tilt 7.6° | delivery table (re-tuned row) and UGOT re-tune paragraph |
| Carrion crow | 71 | 97 m in 10.6 s, 9.9 m/s, 4.3 Hz | delivery table (crow flight v2 row) |
| Keenon T8 | 22 | crosses a room, turns 0.3–0.8 m before obstacles | delivery table (Keenon T8 row); cell count from the network's `connectome_labels.hidden_nodes` |

Cell counts are `len(connectome_labels.hidden_nodes)` of the showcase networks (`target/qa/showcase/make_all.sh` NET map). The showcase README lists the same values. Circuits are designed (`scripts/aarnn_circuit.py`, `build_*_network_json.py`), not trained.

Stills are frames of `target/showcase-videos/*.mp4`, recaptured on 23 Nov 2026 with the anatomical UI view. Commands: `target/qa/showcase/make_all.sh`, the camera settings listed there, and `NM_UI_VISUALIZATION_STAGE=6`.

## 03 Neurons where biology puts them
| Claim | Evidence |
|---|---|
| Worm positions from WormAtlas soma maps (ganglion, A–P site per class) | `scripts/celegans_anatomy.py` docstring; `scripts/build_celegans_network_json.py` `anatomical_position` |
| Fly cells at their BANC-annotated neuropil and soma side | `scripts/layout_designed_circuit.py` `drosophila` (`data/drosophila/BANC v626/neurons.csv.gz`: "Top in/out region", "Soma side"; Bates et al. 2026 [8]) |
| NAO human-equivalent cord; mammal-equivalent cords for quadruped and wheeled robots; avian layout for the crow | `scripts/layout_designed_circuit.py` profiles `nao`, `ugot_quadruped`, `ugot_balance`, `keenon_t8`, `crow` |
| Placement alone leaves dynamics bit-identical | 23 Nov 2026: `celegans_depth_probe` hidden-rate series, original vs layout network, 4 s on each robot's recorded showcase trace. All eight designed circuits (zebrafish, nao, crow, drosophila, hexapod, ugot_quadruped, ugot_balance, keenon_t8) give identical 400-row series (`cmp`). These nets have gap-junction strength 0 and morphology and delays off |
| With morphology and delays on: zebrafish 0.69 vs 0.63 L/s, hexapod 0.073 vs 0.059, NAO 0.586 vs 0.582 m in 30 s | `docs/lec-crow-delivery.md` "Showcase videos, 3D layouts and the spatial ladder" |
| Figure | `figures.layouts_figure`: soma positions and nonzero `w_hh_rec`, `w_in` and `w_out` of the four networks |

## 04 Learning that keeps innate skills
| Claim | Evidence |
|---|---|
| STDP was applied to every dense-matrix entry and created absent synapses; worm input synapses 246 → 12,381 in 8 s; 88 of 96 inhibitory outputs lost; all 302 cells at ≈320 Hz | `data/celegans_learning_runaway.json` (before-fix runs, 23 Nov 2026; code at 6d02f24). Inhibitory outputs: `w_out_negative` 96 → 8 |
| Learning now changes only existing synapses; growth makes new ones | `apply_weight_delta` (zero stays zero) and `matrix_plasticity_proposals`; unit test `plasticity_never_creates_a_synapse` |
| Each cell keeps its own set point; multiplicative scaling, with excitation down and inhibition up | `RateHomeostat` and `homeostatic_scale_rows` (unit tests); Hengen et al. 2016 [13] (cell-autonomous set point); Turrigiano et al. 1998 [10] (multiplicative); Kilman et al. 2002 [11] (inhibition scales the opposite way) |
| The compensation Hebbian learning needs | Zenke & Gerstner 2017 [12] (Hebbian plasticity requires compensatory processes) |
| Worm with learning 9.8–9.9 Hz over 40 s; without learning 9.5–10.1 Hz | `data/celegans_learning_runaway.json` (`after_fix_with_homeostat`, `no_learning`) |
| Coherence between ganglia 0.07 with learning (before the fix 0.98), as without learning (0.07) | resonance test, mean of 2 seeds: `target/qa/complexity/celegans/spatial_learn.json` T1/S0+L coherence 0.071 (after); `spatial_iso.json` T1/S0+L 0.981 (before) and T1/S0 0.072 (no learning) |
| Worm locomotion runs on motor-circuit rhythm generators | Wen et al. 2012 [16] (proprioceptive coupling); Fouad et al. 2018 [17] (distributed rhythm generators) |
| Memory at sensory→interneuron synapses | Kunitomo et al. 2013 [15] (ASER→AIB plasticity, salt-concentration memory) |
| AIA, AIB, AIY and AIZ are the amphid's first interneuron layer | Gray, Hill & Bargmann 2005 [14] (main text: half of amphid output goes to AIA, AIB, AIY and AIZ, "layer 1") |
| Subset learning keeps the crawl: 0.125–0.146 L/s, as without learning (0.126–0.146) | `target/qa/complexity/celegans/spatial_learn.json` T1/{S0,S1,S1g}+Ls `speed_body_lengths_s` (0.125, 0.135, 0.146); `spatial_iso.json` T1/{S0,S1,S1g} (0.126, 0.146, 0.146) |
| Learning in every cell: 0.034, 0.015 and 0.011 L/s (figure) | `spatial_learn.json` T1/*+L |
| NAO subset learning: 15.7–16.0 body lengths without falling; every-cell learning falls | `target/qa/complexity/nao/spatial_learn.json` T0/S0+Ls 15.73 and T0/S1+Ls 15.98 (`fell` false); T0/*+L `fell` true |
| Newborn stepping has two primitives that are kept, with new ones added in toddlers | Dominici et al. 2011 [18] |
| Predictive gait adaptation depends on the cerebellum | Morton & Bastian 2006 [19] (title "…locomotor adaptations…") |
| Figure | `figures.learning_figure`: closed-loop behaviour from `spatial_learn.json` (after the fix) and `spatial_iso.json` (no learning, and "before fix" +L) |

## 05 More neurons, more room to compute
| Claim | Evidence |
|---|---|
| Scaling study design: fixed in-degree (20 recurrent, 4 input synapses per cell, 20 % inhibitory), worm cell physiology, only N varies; 32 patterns × 6 noisy presentations; leave-one-out nearest-centroid decoding | `examples/aarnn_scaling_probe.rs`; `scripts/qa/scaling_study.py` (probe arguments in `data/scaling_text.json`) |
| Accuracy values (8–2,048 neurons) | `target/qa/scaling/summary.json` (mean ± s.d., 3 seeds; 2,048 added 23 Nov 11:27); text in `data/scaling_text.json` |
| Not yet: no readout after the input ends | `summary.json` `memory_8`: decoding from 0–100 ms after offset stays near chance (0.125) at every size; the figure therefore shows only the stimulus window |
| "About N points per doubling" | least-squares slope of 32-pattern accuracy against log2(N), `data/scaling_text.json` `slope_per_doubling` |
| Largest jumps in brain neuron number came with birds and mammals | Kverková et al. 2022 [5] (convergent increases in the two endotherm lineages) |
| Goldcrest ≈64 M, raven ≈1.2 B pallial neurons | Olkowicz et al. 2016 [4] |
| More dimensions make more patterns linearly separable | Cover 1965 [20] (function-counting theorem). Paraphrased, not quoted |
| 2,101 neurons on five nodes; 1.38 M LIF neurons on 40 shards (converted language-model layer) | v2 FACTS (section 06; Evelyn stage-gate record [21]) |

### Neuron by neuron: the whole vs its parts (added 23 Nov, after the user's request)
| Claim | Evidence |
|---|---|
| Design: N = 1..64, 3 seeds; the same cells, input wiring, patterns and noise with recurrent synapses on (`--w-exc 1 --w-inh 4`, all-to-all below 21 cells, then 20 per cell) or off (`--w-exc 0 --w-inh 0`) | `target/qa/resume/run_1by1.sh`; outputs `target/qa/scaling_1by1/{network,parts}/run_N*_s*.json`. The same seed gives identical `w_in`, patterns and input-flip noise; only the recurrent weights differ. The runtime's own stochastic terms (e.g. synaptic release) are not seed-reproducible: two identical runs differ by <1 % in spike count |
| Connected beats disconnected by 14–20 % above 16 neurons; better in 106 of 134 matched runs; sign test p ≤ 0.002 per range; no difference below 16 (p = 0.87) | `target/qa/scaling_1by1/stats.json` (ranges 17–32, 33–48, 49–64: accuracy 0.151 vs 0.125, 0.191 vs 0.161, 0.228 vs 0.200; exact two-sided sign tests; sign-flip permutation p < 1e-4). Gap grows with N: +0.005 per 10 neurons, permutation p = 0.001 |
| Information per neuron ×1.8 (connected) vs ×1.15 (disconnected) from 1–16 to 17–32 neurons | `stats.json` `*_bits_per_neuron`: Fano lower bound on information from decoding accuracy, I ≥ log2 32 − H(e) − e·log2 31, divided by N. This is a lower bound and conservative near chance; both conditions use the same bound |
| Figure | `figures.scaling_two_panel_figure` (left `scaling_1by1/summary.json`, right `scaling/summary.json`) |
| N(N−1) possible synapses among N neurons | arithmetic (ordered pairs, no self-synapses) |
| Sparse memories in binary synapses approach ≈0.69 bits per synapse; capacity grows with the number of storage elements | Palm 1980 [23], abstract: 0.688 bits per element for a 1,000×1,000 matrix, "increases proportionally to the number of storage elements"; Willshaw et al. 1969 [22] (69 % efficiency, via Graham & Willshaw and Knoblauch, Palm & Sommer 2010). Both verified 23 Nov 2026 |
| AARNN recalls a stored experience from half a cue | `examples/aarnn_storage_probe.rs` (clipped Hebbian storage, global inhibitory reset, best-match retrieval). N = 80, assembly 8, `--w-assoc 20 --w-inhib 4`: load 1 recalled in every test. Higher loads are not yet reliable (overlapping assemblies ignite each other), so storage scaling is NOT claimed |

### Storage with an inhibitory interneuron pool, and groups of groups (added 23 Nov, user request)
| Claim | Evidence |
|---|---|
| Storage probe | `examples/aarnn_storage_probe.rs`. Settings:<br>- 8-cell sparse experiences stored as clipped Hebbian synapses between excitatory cells (`--w-assoc 3`);<br>- inhibitory interneuron pool of 20 % of all cells: every excitatory cell excites every interneuron (`--w-ei 3`) and every interneuron inhibits every excitatory cell (`--w-ie 2`), weights scaled 1/N relative to 80 cells (`--pool-ref 80`), Dale's law holds;<br>- recall from a 12 ms cue volley to half an experience, read over 40 ms;<br>- best match among stored experiences, ≥ 90 % of 20 tested;<br>- reset by driving the pool;<br>- deterministic release (`--release-p 1`).<br>Driver `target/qa/resume/run_storage.sh` / `run_storage_small.sh`; outputs `target/qa/storage/run_N*_s*.json` |
| "Memories grow faster than neurons": capacity ≈ N^k, bits stored per neuron rising | `target/qa/modular_storage_report.json` `storage.rows` and `storage.fit` (log-log least squares over seed means; 3 seeds per N). Generated into `data/scaling_text.json` |
| Why the pool needed balancing and a brief volley | Calibration on 23 Nov:<br>- fixed pool weights made capacity fall with N (14–17 at N = 80, ~2 at N = 256–320);<br>- with a long 100 ms cue, one active partner fired a cell (no coincidence threshold), so overlapping assemblies ignited each other (capacity ≤ 2 at every setting tried). |
| Theory: memories live in synapses; capacity grows with the number of storage elements | Palm 1980 [23] abstract; Willshaw et al. 1969 [22] |
| Groups of groups (representation) | `examples/aarnn_scaling_probe.rs` `--module-size G --within-p p --branching B --attach 2`: groups wired inside (all-to-all or with p), every pair of groups in a collection joined by 2 excitatory synapses each way, recursively. Outputs `target/qa/scaling_modular/{clump,mod20,mod12,parts}` (N = 20–640, 3 seeds) and `target/qa/scaling_hierarchy` (N = 320) |
| Groups of 20 match the interwoven network; groups of 12 lose ~15 % | `modular_storage_report.json` `modular.tests`:<br>- `mod20_vs_clump` +3 %, better in 26/41, sign p = 0.12 (not significant);<br>- `mod20_vs_mod12` +15 %, 34/37, exact sign p = 1.2e-7. |
| Beyond the threshold, partners per neuron matter more than grouping | `hierarchy`, N = 320, 3 seeds:<br>- sparse groups G40/G60/G80 at p 0.5: 0.39–0.41 / 0.42 / 0.47;<br>- clump with 20/30/40 partners: 0.35 / 0.46 / 0.49.<br>Collection size B = 2/4/8 made no detectable difference |
| Storage inside groups is lower than in the open network | `target/qa/storage_modular` and the 23 Nov tests at N = 160:<br>- open network 43 (8-cell) and 22 (16-cell) experiences;<br>- full groups of 80: 28 (8-cell) and 12 (16-cell);<br>- sparse groups of 80 at p 0.5: 7 (16-cell).<br>Experiences were assigned to random groups, not group-by-group as in the proposed growth rule |
| Brain: modules within modules, economical wiring | Meunier, Lambiotte & Bullmore 2010 [24] (full text: "hierarchical modularity … an economical wiring diagram"), verified 23 Nov 2026 |

## 06 Toward hardware
| Claim | Evidence |
|---|---|
| Minimum tier keeping behaviour: worm T0, NAO T0, zebrafish T4, hexapod T4 | `target/qa/complexity/{celegans,nao,zebrafish,hexapod}/REPORT.md`, line "Lowest tier with each capacity" |
| Tier definitions (T0 spikes or graded release only; T4 adds active dendrites and NMDA gating) | `scripts/qa/complexity_ladder.py` docstring and `TIERS` |
| Seven FPAA candidate kernels; Xcos, Okika Pi.Ka and SPICE models; route gate with software fallback; AER formats | v2 FACTS (section 05); `fpaa/algorithms.json` |

## 07 Falsification table
| Row | Evidence |
|---|---|
| Capacity vs neurons | section 05 |
| Worm import signed (26 GABAergic cells) and includes gap junctions | `docs/lec-crow-delivery.md` import v2 defect table (`--polarity transmitter`; McIntire et al. GABA set via `scripts/celegans_anatomy.py`); Cook et al. 2019 [7] |
| Fly projections remap ≈63 % of kept edges | v2 FACTS (unchanged builder) |
| Nine bodies; designed circuits; no held-out task | section 02 |
| Specialised learning keeps gait; no controlled conditioning in any robot | section 04; `target/qa/complexity/*/REPORT.md` ("conditioning none") and `SPATIAL_LEARN.md` |
| Median reply 1.9–6.8 ms; fly flight p95 3.7 ms; no energy measurement | delivery results table; Drosophila flight paragraph; a repo-wide search finds no energy measurement |
| Christensen et al. 2022 [6] for energy and latency metrics | v2 reference |

## 08 Roadmap
| Claim | Evidence |
|---|---|
| Growth: sprouting unbounded in designed circuits | `docs/lec-crow-delivery.md` "Growth is unbounded and its cost explodes" |
| G3 target 100–400 M | author's roadmap (v1/v2) |

## Code changed for this poster (working tree, 23 Nov 2026)
- `src/aarnn/plasticity.rs`: `apply_weight_delta` (no synapse creation, sign kept, silent floor 1e-6, no truncation above `w_max`); `homeostatic_scale_rows`; `RateHomeostat`; unit tests.
- `src/runner.rs`:
  - `matrix_plasticity_proposals`;
  - recurrent learning for every hidden layer;
  - `apply_rate_homeostasis`;
  - plastic-cell mask;
  - homeostat state in snapshots.
- `src/config.rs`: `aarnn_rate_homeostasis_*` and `aarnn_plastic_cells`.
- `src/cl_compute.rs`: OpenCL plasticity kernel mirrors `apply_weight_delta`.
- `src/ui.rs` and `src/ui/anatomy.rs`:
  - replay publishes the topology (anatomical stages);
  - camera roll and stretch, with environment variables.
- `scripts/layout_designed_circuit.py`: fly, quadruped, balancer, Keenon and crow profiles.
- `scripts/qa/complexity_ladder.py`: `+Ls` specialised-subset learning.
- New: `examples/aarnn_scaling_probe.rs` and `scripts/qa/scaling_study.py`.
