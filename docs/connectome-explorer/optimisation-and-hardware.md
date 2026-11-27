# Optimisation and hardware

## Search (`optimise`)

1. **Revalidate the intact reference** on the development split. If it fails its own contract, the search returns a negative result (exit code 6) and stops.
2. **Remove verified-inert cells.** A cell is verified inert when all of these hold:
   - it has no directed chemical path, and no electrical path, to any output;
   - it is not a modulatory, plastic or growth-parent cell;
   - the network declares neither environmental neuromodulator drive nor growth. Either would let any cell's activity, or newborns, matter.
   
   The removal is still validated by the tasks.
3. **Exclude protected cells.** Excluded from proposals are:
   - the learning substrate (plastic, presynaptic-plastic and modulatory cells, plus their direct excitatory hidden readouts), unless `learning_required`;
   - the profile's declared out-of-contract cells (`protect_cell_types`, `protect_neurons`).
4. **Screen single ablations** and rank each cell by importance: 1000 × (required tasks its ablation fails) + (events inside GNATs, from the analysis). Cells whose single ablation fails a task are not proposed for removal, and are recorded as individually essential.
5. **Beam search** (`beam_width`, `proposal_budget`):
   - a child is its parent plus one more removal;
   - every child is physically lowered and re-evaluated in full, which catches individually dispensable but jointly essential cells (e.g. `G1a` + `G1b`);
   - the beam keeps the cheapest passing children, sorted by (cost key, removal set), so the search does not depend on job completion order.
6. **Synapse pass.** Greedy removal of single synapses on the best state, weakest first.
7. **Frontier and selection.**
   - The Pareto frontier is computed over the structural cost dimensions. Measured latency is reported, but it never decides dominance alone, because it is noisy.
   - `--select min-state-bytes | min-neurons | min-synapses` orders the frontier, followed by other passing candidates.
   - Under a resilience profile, candidates are checked in that order and the first that passes the single-fault claim is selected.
   - Selection is locked before validation.

Every proposal is logged in the optimisation report: removals, pass/fail, failed tasks, cost and job. The search may find no acceptable reduction. It never relaxes requirements to produce a smaller brain.

## Masks versus physical reduction

- **Masks** (`experiment`) zero synapses but keep the cell, so they test causality, not cost.
- **Physical lowering** (`lesion::lower`) removes:
  - the cell's rows and columns from `w_in`, `w_hh_rec`, `w_out`, `w_gap` and every presence pattern;
  - its entry in `topo.layers[0]` and in `hidden_nodes`;
  - its entry in every per-cell `[[...]]` network array.
  
  It updates `num_hidden_per_layer_initial` and `max_total_neurons`, and publishes the source-to-candidate `Mapping`.
- **What is measured.** The lowered network is what is evaluated, validated and exported.

## Cost model

| Dimension | Basis |
|---|---|
| neurons, hidden, synapses | counted from the lowered snapshot |
| memory_bytes | estimate: the native runner's dense matrix allocation, 8 bytes per slot |
| state_bytes | estimate: 12 state words per cell + 2 words per matrix slot |
| latency_ns_p50 | measured: median runner step time on this host (CPU, single thread) |
| energy, area | not available: needs a target tool; never inferred |

The cost report always states which values are measured and which are estimated.

## Hardware targets

- **Supported target:** `aarnn-cpu-native` only, the same f64 reference runner. The exported `candidate.network.json` loads with `Runner::import_network_json`.
- **Not provided:**
  - fixed-point or other silicon profiles need a conversion and validation step through the canonical numeric implementation;
  - FPAA, ASIC and FPGA mapping needs target-specific placement and calibration.

A hardware-neutral JSON network is not a fabricated or validated chip.

## Microcircuit replacement

Relay-chain replacement (`allow_verified_relay_replacement`) is rejected as unsupported. No rule has been established under which collapsing AARNN graded or spiking relays preserves delay and efficacy semantics, and the demo's memory loop shows naive chain collapse destroying function. Training-assisted reduction is an extension contract and is also rejected as unsupported.
