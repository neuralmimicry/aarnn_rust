# AARNN A0 poster (v2): provenance of every claim

Audit date: 13 Nov 2026. Source: `aarnn_rust` at commit `6d02f24` (v0.1.43). The
working tree differs from HEAD only by SPDX headers in the files cited here.
Generated network snapshots (`network_*.json`) are gitignored local build outputs.

Rebuild: `python3 -I audit_celegans_import.py`, then `build_poster.py`. Needs
matplotlib, Pillow and segno. Then export with `soffice --headless --convert-to pdf`.

## Meeting and author
| Claim | Evidence |
|---|---|
| Royal Society, "Innovating technologies for neuromorphic computing", 23–25 Nov 2026, poster session 23 Nov | royalsociety.org/science-events-and-lectures/2026/11/neuromorphic-computing/ |
| NeuralMimicry Ltd, Rugby, Warwickshire | neuralmimicry.ai news item on incorporation (Jan 2026) |
| AARNN = Auto-Asynchronous Recursive Neuromorphic Network | neuralmimicry.ai (team page), `REQUIREMENTS.md:25` |

## 01 Architecture
| Claim | Evidence |
|---|---|
| AARNN neurons are Izhikevich; LIF alternative | `src/runner.rs:2108-2113`, `src/neuron_kernels.rs:25,109`, `src/sim.rs:43` |
| RS default plus 8 presets (RS/IB/CH/FS/LTS/RZ/TC/P), adaptive threshold | `src/config.rs:324-350`, `src/neuron_kernels.rs:109` |
| AMPA/NMDA/GABA traces, voltage-gated NMDA | `src/aarnn/dynamics.rs:183`, `src/config.rs:466-469` |
| Tsodyks–Markram short-term plasticity | `src/aarnn/plasticity.rs:208` |
| Gap junctions, volume transmission, active dendrites | `src/aarnn/dynamics.rs:245,267,392,471` |
| Delay/attenuation, apical/basal compartments, bAP gain, myelination, ATP fatigue | `src/aarnn/transmission.rs:143,191`, `src/runner.rs:18231` |
| STDP/Hebb/Oja, homeostasis, synaptic scaling, soft Dale; neuromodulator gains default 0 | `src/aarnn/plasticity.rs:146,322,370`, `src/runner.rs:9274,16199-16200`, `src/config.rs:1833-1835` |
| Local, online learning; no backprop or gradient training of the SNN | `src/runner.rs:15282`; a grep of `src/` finds no optimiser/gradient code (GA hyper-parameter search in `src/ga.rs` is offline and gradient-free) |
| Recurrence: w_hh_rec, w_hh_bwd, output→sensory feedback | `src/runner.rs:1377-1381,8593` |
| Fixed dt clock; spike-sparse synaptic summation; slower decoupled growth/pruning/morphology/metabolism cadences | `src/runner.rs:8300,16840,10046-10060,16243,16478-16483,16509,16528`; test `growth_scheduler_interval_decouples_growth_from_spike_steps` |

## 02 Growth rules
| Claim | Evidence |
|---|---|
| Default network = 1 hidden neuron, no I/O | `NetworkConfig::default()` (`src/config.rs:2998ff`); `fresh_single_neuron_config` (`src/distributed.rs:626`) |
| Trigger 0.5; cooldowns 500 / 150 ms; one spawn per pass | `src/config.rs:3019,3027`; `collect_growth_candidates` (`src/runner.rs:20862`) |
| Layer split at 32, up to 6 layers; spontaneous neuron every 1,000 ms | `src/config.rs:3026,3152`; `src/runner.rs:16260` |
| Specification → migration → differentiation; five development stages | `src/runner.rs:20558,20620`; `DevelopmentStage` (`src/config.rs:223-234`) |
| Contact synaptogenesis, pruning, neuron removal (never the last) | `src/morphology.rs:4576,5864,6595`; `src/runner.rs:16578-16600` |
| 8–64 segments per neuron; 200–8,000 contact checks | `src/morphology.rs:5307-5310` |

## 03 Connectomes and embodiment
| Claim | Evidence |
|---|---|
| C. elegans 24/302/96, GoPiGo wiring (Busbice 2014), CANL/CANR added | `celegans.py:5-6`; `scripts/build_celegans_network_json.py:35-42`; `network_celegans.json` |
| Fly 418/20,000/48; FAFB 138,584 neurons, 50,666,648 synapses; BANC 112,885 / 22,243,310 | counted from `data/drosophila/*/connections_princeton.csv.gz`; `scripts/build_drosophila_network_json.py:1330,736,1988` |
| ≈63 % (FAFB, 62.8 %) / 64 % (BANC, 63.5 %) of kept edges remapped | computed from the generated fly snapshots |
| Zebrafish 32/2,000/32, Seung Lab table, synthetic sensory input | `scripts/build_zebrafish_network_json.py:7,523,731,861` |
| Rewire keep fraction 0.74 (worm), 0.78 (fly, fish) | `scripts/build_celegans_network_json.py:961-963`; `build_drosophila_network_json.py:1776`; `build_zebrafish_network_json.py:807`; `src/runner.rs:6299-6345` |
| Inhibitory fraction 0.20 human / 0.36 worm; myelination only in the human and fish profiles | `src/config.rs:1784,2013,2089,2164-2218,2242` |
| 28 embodiments (MJCF, USD, Webots); crow 31 joints, untrained | `sim/content/embodiments/generated/`; `docs/lec-crow-delivery.md:422-425` |

## 04 Audit (this directory)
`audit_celegans_import.py` writes `celegans_audit.json`. It finds 3,689 signed source pairs. All 3,564 positive pairs match the snapshot exactly, with 0 extra entries. The 125 negative neuron→muscle pairs (Σ −860, 27 presynaptic cells) are absent. The cause is that `EDGE_RE` (`scripts/build_celegans_network_json.py:36`) matches only `[0-9]+`.

## 05 Hardware interfaces
| Claim | Evidence |
|---|---|
| 7 FPAA candidate kernels, 6 software-only | `fpaa/algorithms.json` |
| Xcos, Okika Pi.Ka manifest and SPICE macromodel for each candidate | `fpaa/xcos/`, `fpaa/okika/`, `fpaa/spice/aarnn_kernels.lib` (`.subckt` per kernel) |
| Requested vs effective route; software fallback; no live analog path | `src/fpaa.rs:219-273,887`; `fpaa/README.md:86-94` |
| AER1, AARNN-AER/1 (CRC-16, ≤1 MiB), AER-CAN v1 | `crates/aarnn-aer-legacy/src/lib.rs`; `src/aer_transport.rs`; `src/aer_can.rs:28` |
| Fabric bridge: UDP, 32-byte events, ≤64 per packet | `aer_fabric_bridge/src/aer/packet.rs:5-24` |
| OpenCL parity tests (opt-in via `NM_ENABLE_OPENCL_IN_TESTS=1`) | `src/runner.rs:23440-23742` |

## 06 Scale
| Claim | Evidence |
|---|---|
| 2,101 neurons, five nodes, step 3,722,473 (10 Oct 2026) | `docs/execplans/live-neural-visualisation-and-fpv.md:155-160` |
| 1.38 M LIF neurons, 6.1 G neuron-steps, 40 shards, 1.84 % vs 3.74 % gate | `evelyn/docs/ARCHITECTURE.md:84` (Stage 3b) |
| Two hosts: +3.755 % ΔPPL, 98.44 % top-1, 1.311 vs 0.642 s/token | `evelyn/docs/ARCHITECTURE.md:86,125-129` (Stage 4c) |
| Distributed reference contracts and open gates | `src/hierarchical_sharding.rs`, `src/authoritative_shard.rs`, `src/consistent_cut.rs` with tests; `docs/architecture/distributed-hierarchy.md:97`; OpenMPI `src/openmpi_runtime.rs` has no tests |
| G3 target 100–400 M | author's roadmap (the v1 draft poster); not stated elsewhere in the repo |
| Goldcrest pallium ≈64 M, raven pallium ≈1.2 B | Olkowicz et al. 2016 [6] |
| Human brain ≈86 B | Azevedo et al. 2009 [7] |

## 07 Evidence today
| Claim | Evidence |
|---|---|
| MuJoCo worm 47 / hexapod 35 / NAO 17 exchanges in 1,500 steps | `docs/lec-crow-delivery.md:27-36` |
| 0 of 224 acceptance rows pass (24 fail, 56 blocked, 144 not run) | `qa/fixtures/lec-crow/CURRENT.md:5` |
| Bit-exact snapshot replay with growth and morphology off | `snapshot_roundtrip_preserves_runtime_state_and_rng_continuation` (`src/runner.rs:24915-24924`) |
| 16 QA scenarios with seeds and resource/latency bounds | `qa/scenarios/*.toml` |
| No energy or p95/p99 measurement | repo-wide search; spec targets only (`docs/specifications/live-neural-visualisation-and-fpv-v1.md:146-148`) |
