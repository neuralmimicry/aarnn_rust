# Data formats

All records are JSON, published as immutable content-addressed objects (`objects/sha256/ab/<rest>`). A named record binds once; a name bound to a different hash is a conflict. Every record carries a schema or `schema_version`.

## Identity

- **Stable IDs.** Neuron IDs are the source's names: `connectome_labels.{sensory,hidden,output}_nodes`, or the `id` of the sparse interchange. They are always carried as strings, so integer dataset IDs such as FlyWire root IDs (around 7.2e17) never pass through a float. The sparse importer rejects float IDs.
- **Dense index.** A neuron's dense index (`role`, `dense`) is generation-bound and is never used as identity across a lowering. Lowering publishes an explicit `Mapping`.
- **Synapse IDs.** `pre->post`, with `:gap` appended for electrical synapses.
- **Trace neurons.** Trace events carry the global index into the source topology's neuron list, in the order sensory, hidden, output, for topology generation 1.

## Native AARNN snapshot (`--format aarnn-snapshot`)

This is the runtime's own format (`aarnn_rust::runner::Snapshot`). The explorer reads:

- **Matrices**, all row-major `{rows, cols, data}`:
  - `w_in` (hidden × sensory);
  - `w_hh_rec[0]` (post × pre);
  - `w_out` (output × hidden);
  - `w_gap[0]` (symmetric);
  - `p_*`, the u32 presence patterns.
- **Topology:** `topo.layers[0][i].{type_name, region_name, x, y, z}`.
- **Network settings and types:** `net` (authoritative network settings) and `net.neuron_types[].bio_params`.

Only single-hidden-layer snapshots are lowered. A snapshot with `runtime_state` imports as `declared-cold-start`, and lowering drops `runtime_state`.

## Sparse interchange `aarnn-explorer-sparse-v1` (`--format sparse`)

```json
{
  "schema": "aarnn-explorer-sparse-v1",
  "dataset": {"name": "...", "version": "...", "specimen": "...", "coverage": "...", "licence": "...",
              "source_files": [{"path": "...", "sha256": "..."}]},
  "units": {"weight": "aarnn-native | synapse-count", "delay": "ticks"},
  "neurons": [{"id": "720575940612345678", "role": "sensory|hidden|output", "type": "Exc",
               "region": "...", "position": [x, y, z], "position_kind": "measured|inferred|procedural|unknown"}],
  "edges": [{"pre": "id", "post": "id", "weight": 1.0, "count": 12, "sign": 1, "class": "chemical|electrical", "delay_ticks": 1}],
  "dynamics": {
    "cell_types": {"Exc": {"izh_preset": "RS", "graded_enabled": false}},
    "weight_conversion": "explicit-weight | count-sign-scale", "scale": 0.1,
    "assumption_version": "my-assumptions-v1",
    "net_overrides": {"aarnn_neuromod_env_enabled": false}
  }
}
```

- **Unknown fields.** Unknown top-level and manifest fields are rejected.
- **Allocation budget.** Sizes are checked against the import budget before anything is allocated. There is no dense all-pairs matrix until a network is compiled for execution, and compilation is refused when it would exceed the budget.
- **Unknown values.** Unknown weights or signs are kept as unknown (NaN): the source is structural-only, and those edges create no GNAT edges.
- **Executable sources.** With `dynamics`, and no unknown weights, the document is compiled to a native snapshot and is executable (`declared-cold-start`). Compilation applies:
  - the designed-circuit network overrides of `scripts/aarnn_circuit.py::_net`;
  - reliable release (`p_release_default` = 1.0);
  - neuromodulator transmission and environmental drive off, unless `net_overrides` says otherwise.
  
  Every one of these is recorded in the source's `assumptions`.
- **Delays.** Compiled networks use the legacy runner's stage delays (hidden→hidden 1 tick). The declared per-edge delays are therefore replaced, which is recorded in `limitations`.

## Trace (schema 1)

- **Segments.** Each segment is an object of fixed 24-byte little-endian records: `id u64 | tick u64 | neuron u32 | micro u16 | kind u16`, where kind 0 is a spike and 1 a graded equivalent.
- **Manifest.** The manifest records:
  - source and topology hashes, generation, task and seed;
  - policy, `dt_ms`, ticks, mode and time semantics;
  - coverage per population (sensory, hidden-spiking, hidden-graded, outputs, newborn);
  - segments with their hash, first/last sequence number and count, plus gaps and exclusions;
  - completeness, the output-series hash, and the output and event digests;
  - the trial's measures.
- **Load-time checks.** Loading verifies every segment hash, the segment's manifest entry and global sequence continuity.

## Job and result (`aarnn-explorer-job-v1`)

- **Job.** A job names the immutable snapshot object (path + sha256), the suite, the plasticity policy, interventions, the lowering set, I/O protection and the job kind: `evaluate`, `capture`, or `fault` (tests only).
- **Result.** The result holds the process status, any error, `source_unchanged`, the output and the wall time.

## Derived source (`aarnn-explorer-source-v1`, `format = "derived"`)

A source record whose snapshot was produced by `derive`. `derived_from` holds:
- the parent source name, record and snapshot hash;
- the preset;
- the applied interventions (with resolved synapse IDs);
- for `--lower-like`, the candidate, its hash, its source, the removed neurons and synapses, and the mapping.

`parent_capability` keeps the parent's capability.

## Candidate (`aarnn-explorer-candidate-v1`)

- **Provenance:** the source snapshot, the suite and config hashes, the profile and the selection rule.
- **Reduction:** the removed neurons and synapses, the lowered snapshot object and the mapping.
- **Evidence:** the development verdict and cost, the intact cost, the development resilience, the classification (`physical-reduction`) and `validated: false`.
- **Validation** is a separate record (`aarnn-explorer-validation-v1`).

## Mapping (lowering)

- `hidden`: source hidden ID → candidate dense index.
- `retired_neurons` and `retired_synapses`.
- `io_identity`: true.
- `filtered_config_arrays`: per-cell `[[...]]` settings filtered with the layer, such as plastic-cell or growth-parent masks.
- `notes`.

## Export bundle (`aarnn-explorer-bundle-v1`)

```text
candidate.network.json   native snapshot; loads with Runner::import_network_json
mapping.json             biological-to-candidate IDs, retired IDs
contract.json            expanded profile + task suite
validation.json          locked validation record (null when exploratory)
cost.json                intact vs candidate; measured vs estimated vs unavailable
manifest.json            status, classification, capabilities, hashes, licences, reproduction
sha256sums.txt           checksums of every file
```

`status` is one of `validated`, `validation-failed` or `exploratory-unvalidated`.

## Compatibility

All schemas are version 1. Readers reject other versions with `unsupported-capability`. Additive fields use serde defaults; removing a field needs a version bump.
