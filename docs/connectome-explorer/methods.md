# Methods as implemented

Reference: Theilman, Wang, Rothganger and Aimone, *Decomposing spiking neural networks with Graphical Neural Activity Threads* (2023), arXiv:2306.16684 v1, checked against the HTML full text. The differences from that paper are listed in the last section.

## Time and events

- **Ticks.** One tick is one runner step (dt = 1 ms).
- **Microsteps.** The legacy runner has no superdense microsteps, so the microstep records an event's stage within its step: sensory 0, hidden 1, output 2. Microsteps order events but add no duration to any decay term.
- **Effective delays** (measured on the runner, `legacy-runner-stage-delay-v1`):
  - sensory→hidden: 0 ticks;
  - hidden→hidden: 1 tick;
  - hidden→output: 0 ticks.
  
  A relay shows S 10.0, H1 10.1, H2 11.1, O 11.2 (tick.stage).
- **Event identity.** An event's ID is its sequence number in its trace, and the trace is scoped to a source hash and topology generation.

## Causal influence score (`effective-delivery-v1`)

```text
delta = t_post - (t_pre + d_ab)                         ticks
score = (w_ab / norm_a) * exp(-delta * dt_ms / tau_ms)
edge  pre -> post  iff  w_ab > 0, delta >= 0, score >= threshold
```

- **Delay conditions.**
  - `d_ab = 0`: the edge exists only when `micro_pre < micro_post` at the same tick.
  - `d_ab >= 1`: same-tick arrival is always valid.
- **Norm.** `norm_a` is the L1 norm of the absolute weights of all chemical inputs onto `a`, sensory inputs included (`norm = "l1-abs"`). `"l2-abs"` reproduces an L2 reading.
  - The paper says only "the norm of all synaptic weights onto neuron α".
  - A zero norm means the cell has no edges.
- **Ties.** A score equal to the threshold is an edge (θ(0) = 1).
- **Parallel synapses.** For a pair of events, the best-scoring synapse defines the edge.
- **Defaults.**
  - τ = 5 ms, as in the paper's Fig. 1c.
  - Threshold 0.0067 ≈ e^-5, the paper's −log Ω ≈ 5 read as a rule.
- **Eligibility horizon.** Because w/‖w‖₁ ≤ 1, an edge requires delta ≤ τ·ln((w/norm)/threshold). The search window is therefore exact, not truncated.
- **Inhibition and gap junctions.** They never create edges, but they count in the norm and stay fully active in the simulation. Their absence from GNATs is never evidence that they are dispensable. The demo's inhibitor `I` has no positive edge yet is essential to the no-go task, and the lesion tests show it.
- **Graded cells.** Designed circuits are mostly graded (non-spiking) cells, which the runner never lets spike. Method `effective-delivery-graded-v1` adds spike-equivalent events: a phase accumulator integrates each graded cell's release rate, so a cell releasing at r Hz emits r events per second. These events are marked `graded-equivalent` in traces, and analyses state which method was used.

## GNATs

- **Definition.** GNATs are the weakly connected components of the event graph, found with union-find in which the smaller root wins (deterministic). Events with no edge are counted as isolated and are not reported as GNATs.
- **Component ID.** `gnat-t<tick>-n<neuron>-e<event>`, taken from the earliest member.
- **Depth.** The longest causal chain in edges, computed in one pass because edges always point to a later (tick, micro).
- **Depth per neuron.** Depth divided by the number of distinct cells in the GNAT.
- **Boundary flags.**
  - `open_start`: a member lies within the boundary reach (max delay + horizon) of the window start, unless the window starts at the run's cold start.
  - `open_end`: the same test at the window end.
- **Two algorithms.**
  - Reference: tests every event pair against the synapse map. Bounded to 20 000 events.
  - Indexed: binary search over per-neuron sorted ticks within the horizon.
  - They agree exactly on 40 random adversarial fixtures, which include parallel synapses, mixed signs, zero delays, stages and recurrent firing (`gnat::tests::reference_and_indexed_agree_on_adversarial_fixtures`).

## Threshold calibration and sensitivity

- **Calibration** (`threshold_mode = development-calibrated`):
  1. Histogram −ln score over all candidate edges (score floor 1e-6, bin 0.25), for the real trace and for circularly time-shifted controls (each neuron shifted by a seeded random offset, keeping its rate and intervals).
  2. Walking outward from the strongest bin, keep the deepest boundary where the cumulative chance count stays ≤ 10 % of the real count.
  3. The status is `calibrated`, `no-transition` (every bin passes) or `uncertain` (the first bin already fails), with the false-edge fraction recorded.
  4. Calibration runs on development traces only.
- **Sensitivity.** GNAT and edge counts, depth and size are reported over τ × {0.5, 1, 2} and threshold × {0.5, 1, 2}.

## Recurrence (analogous subthreads)

- **Construction.** This is the second-order activity graph of the paper's Eq. 4-5:
  - vertices are pairs of same-neuron events, one from graph A and one from graph B;
  - an edge exists when both underlying edges exist;
  - the weakly connected components are the analogous subthreads.
- **Sparse indexing.** B's edges are indexed by their (pre neuron, post neuron) pair, so only matching edges are paired and no spike-pair product is formed.
- **Budget.** Second-order edges are capped by `max_motif_candidates`. Exceeding the cap sets `approximate`.
- **Motifs.** A motif groups subthreads by a fingerprint (hash of the sorted neuron-pair edge multiset), then verifies the grouping by exact multiset comparison. Collisions are separated and counted, because hash equality is not proof.
- **Reported per motif:** occurrences, neurons, edges and timing jitter. The analysis compares the first trial of each task with its later trials (same neuron identities), never different circuits.

## Interventions

All interventions apply to a copy of an immutable source at a declared cold-start boundary (tick 0). No event is in flight (`cold-start-empty`).

| Action | Classification | Semantics |
|---|---|---|
| `neuron-ablation` | structural mask | every incident synapse zeroed in `w_in`, `w_hh_rec`, `w_out`, `w_gap` and the presence patterns; the cell keeps its state and cost |
| `synapse-ablation` | structural mask | one matrix slot zeroed (gap junctions symmetrically) |
| `weight-scale`, `weight-perturb` | parameter change | bounded multiplicative changes of the selected synapses' native weights |
| `weight-set` | parameter change | sets each selected synapse to `sign(w) × value` (finite, ≥ 0); used by `derive --preset learned-cap` to put plastic synapses at the plasticity cap |
| `silence` | transient mask | outgoing weights zeroed for [start, end) ms, then restored; incoming integration continues; frozen plasticity only; gap coupling not silenced (private runner routes) |
| `sham` | sham | identical bookkeeping, no change; must reproduce the intact verdict |

- **Selections:** explicit neuron or synapse IDs, region, cell type, or a deterministic random fraction. A random fraction rounds half up, takes at least one cell, and can be restricted to regions.
- **Protection:** sensory and output neurons are protected under `protect_input_output_mapping`.
- **Rejection:** unknown IDs and protected cells are rejected.

## Differences from the paper

- The paper simulates Izhikevich networks in STACS with 1-20 ms delays. Here the model is the AARNN runner, with stage delays of 0/1/0 ticks and microstep ordering.
- The norm is unspecified in the paper. The default here is L1 over absolute weights, and L2 is available.
- θ(0) = 1 is a choice here; the paper does not state it.
- The paper's threshold comes from inspecting a histogram. The calibration rule here is explicit and records its uncertainty.
- Graded spike-equivalent events (D-005) are an extension, labelled with their own method name.
- In recurrence, matching neurons is the paper's criterion. Timing differences are reported, not thresholded.
