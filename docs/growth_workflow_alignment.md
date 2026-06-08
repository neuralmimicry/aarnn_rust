# Growth Workflow Alignment

This document maps the runtime workflow to `GROWTH.md` and tracks incremental alignment work.

## Runtime Workflow (Current)

1. Fast spike path (every `Runner::step`):
   - sensory spike ingress
   - hidden/output traversal across synapses
   - motor/intermediate spike emission
2. Slow biology path (cadenced):
   - structural growth scan/spawn (`development_growth_interval_ms`)
   - morphology evolution + metabolic updates (already interval-gated)
   - pruning/shrinkage scan (`development_pruning_interval_ms`)
   - sensory/output interface formation (`development_io_formation_interval_ms`)

The objective is to keep sensory->intermediate/motor latency minimal while preserving biological dynamics on a slower schedule.

## GROWTH.md Mapping

- Sections 1-8 (origin, patterning, progenitors, migration, layering, cell classes):
  represented by topology + region/type configuration (`src/config.rs`, `src/topology.rs`).
- Sections 9-11 (axon/dendrite growth, synaptogenesis):
  represented by `Runner` fast traversal and morphology connectivity (`src/runner.rs`, `src/morphology.rs`).
- Sections 12-13 (pruning/refinement, myelination):
  represented by pruning/shrinkage logic + myelination dynamics (`src/runner.rs`).
- Sections 14-16 (regional maturation, architecture-level principles):
  represented by clumping profiles, layered defaults, and staged biological cadence (`src/config.rs`).

## Incremental Alignment Chunks

- Chunk 1 (implemented): decouple fast spike traversal from slower growth/pruning cadence.
- Chunk 2 (implemented): stage-specific policy tuning per biomimicry profile against GROWTH sections 9-13.
- Chunk 2 extension (implemented): pre-differentiation early-cell staging with explicit xyz migration before neuron finalization.
- Chunk 3 (implemented): explicit verification checks comparing observed runtime workflow against GROWTH section groups.

## Chunk 2 Implementation Notes (Sections 9-13)

`src/config.rs` now carries explicit developmental stage controls:
- `development_stage_mode`: `auto` or `manual`
- `development_stage`: fixed stage when manual
- auto transition boundaries:
  - section 9 -> 10 (`development_stage_dendrite_start_ms`)
  - section 10 -> 11 (`development_stage_synaptogenesis_start_ms`)
  - section 11 -> 12 (`development_stage_refinement_start_ms`)
  - section 12 -> 13 (`development_stage_myelination_start_ms`)

Profile defaults are now stage-tuned:
- Human: slower transition windows and stage-13 myelination enabled.
- C. elegans: compressed transition windows and unmyelinated policy.
- Drosophila: intermediate transition windows and unmyelinated policy.

`src/runner.rs` now applies stage/profile policy without changing fast spike traversal order:
- section 9-11 bias: faster growth/io cadence, pruning disabled.
- section 12 bias: pruning enabled and strengthened stabilization.
- section 13 bias: myelination only when biologically valid for profile (human enabled, c. elegans/drosophila gated off).
- before section 13: no myelination effect is applied to conduction timing/attenuation.
- morphology/metabolic update cadence is scaled per stage/profile so biological transformation can run slower than the spike path.

## Early-Cell Workflow Alignment

To align with the biological ordering in `GROWTH.md` (cells appear before full specialization), growth now stages an explicit early-cell lifecycle in AARNN mode:

1. Growth candidate creation (`collect_growth_candidates`) still runs on the slow growth cadence.
2. `apply_growth_queue` creates `topo.early_cells` entries instead of immediately inserting fully formed neurons.
3. Early cells carry current xyz, start xyz, target xyz, source layer/parent, target layer, intended region/type, age, maturation time, and phase (`specification`, `migration`, `differentiation`).
4. `advance_early_cells` runs on the slow biology path, updates trajectory and phase progression, and only on maturation finalizes into a real hidden neuron via spawn override placement.
5. Fast spike traversal order remains unchanged; early-cell updates do not run in the per-spike hot path.

Transition algorithm (biologically aligned):
- `specification` phase keeps the cell near progenitor origin while committing identity.
- `migration` phase performs target-directed motion with local crowding repulsion against occupied tissue.
- `differentiation` phase settles near final layer position and only then finalizes as a differentiated neuron.
- Stage/profile policy modulates phase boundaries, migration speed, and settling strength so human/drosophila/c. elegans profiles retain distinct developmental tempos.

UI alignment:
- Rust UI (`src/ui.rs`) snapshots and renders `topo_early` with distinct colors/tooltip metadata.
- Web UI (`web_ui/app.js`) renders `snapshot.topo.early_cells` as distinct early nodes with progress/phase cues, and exposes early-cell xyz/target xyz/source->target layer/type metadata in graph context details while keeping spike probes limited to differentiated sensory/hidden/output neurons.

Verification coverage:
- `aarnn_growth_creates_early_cell_before_differentiation`
- `aarnn_early_cell_matures_into_neuron_with_target_position_and_type`
- `aarnn_max_total_neurons_counts_pending_early_cells`
- `test_myelin_level_does_not_modulate_delay_before_stage_13`

## Chunk 3 Verification Checks (Implemented)

Observed-runtime checks mapped to `GROWTH.md` section groups:

- Sections 1-8 (progenitor staging, migration before final identity):
  - `chunk3_workflow_alignment_sections_1_to_11_observed_runtime` (first half)
  - verifies early-cell staging, xyz migration toward target, and no immediate hidden-neuron insertion.
- Sections 9-11 (axon/dendrite/synaptogenesis cadence and low-latency spike traversal):
  - `chunk3_workflow_alignment_sections_1_to_11_observed_runtime` (second half)
  - verifies per-step spike traversal continues while structural growth remains interval-gated and only stages early cells after cadence elapses.
- Sections 12-13 (refinement/pruning vs myelination gating):
  - `chunk3_workflow_alignment_sections_12_to_13_observed_runtime`
  - verifies section-12 pruning policy with no myelin conduction effect, section-13 human myelin conduction acceleration, and section-13 c. elegans unmyelinated behavior.
