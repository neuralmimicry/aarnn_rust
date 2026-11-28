# AARNN connectome explorer

`aarnn-explorer` is a separate Rust binary (package `crates/aarnn-explorer`) for working with AARNN networks:

- inspect existing networks;
- record complete activity by executing the real AARNN `Runner`;
- extract Graphical Neural Activity Threads (GNATs) and recurring subthreads;
- run lesions and other interventions on isolated copies;
- evaluate circuits against declared functional and resilience contracts;
- search for physically smaller circuits that keep the declared functions;
- export an executable candidate with its evidence.

The software proves its own mechanisms (tests) and measures the authored or simulated tasks it is given. It does not establish biological equivalence, and it does not compress a fly brain by itself. See [functional-validation.md](functional-validation.md) for the claim boundaries.

## Build

From the repository root:

```bash
cargo build --locked --release -p aarnn-explorer                  # headless: every workflow
cargo build --locked --release -p aarnn-explorer --features web-ui # adds the loopback browser UI
```

The package depends on the root `aarnn_rust` crate (features `parallel`, `growth3d`, `morpho`). That crate's `build.rs` compiles protobuf definitions, so `protoc` is required. `doctor` reports what it finds. No BLAS library is linked at runtime.

## Quick start

```bash
# Capabilities, backends and resource enforcement (read-only)
cargo run --locked --release -p aarnn-explorer -- doctor --json

# Authored offline end-to-end pipeline through real AARNN execution
cargo run --locked --release -p aarnn-explorer -- demo --project target/explorer-demo

# Open the saved project in the browser (allocated loopback port)
cargo run --locked --release -p aarnn-explorer --features web-ui -- serve --project target/explorer-demo --open

# Through the repository's QA orchestration
cargo xtask examples run --id EX-EXPLORER-001 --product host
cargo xtask qa run --suite explorer
```

The demo:

1. imports `examples/explorer/models/synthetic_sensorimotor.sparse.json` through the sparse importer;
2. runs the intact baseline;
3. captures complete traces and extracts GNATs;
4. runs the lesion plan `examples/explorer/interventions/single_ablation.json` with intact and sham controls;
5. optimises under two profiles (`configs/explorer/default.toml` compact, `configs/explorer/resilient.toml` resilient);
6. validates on the locked split;
7. exports the compact candidate and re-validates the bundle in a clean worker process.

The report is written to `target/explorer-demo/demo-report.json`.

## Staged operation

```bash
P=target/explorer-demo
cargo run --locked --release -p aarnn-explorer -- project init --project $P --name my-project
cargo run --locked --release -p aarnn-explorer -- import --project $P --format sparse --input examples/explorer/models/synthetic_sensorimotor.sparse.json --name synthetic-sensorimotor
cargo run --locked --release -p aarnn-explorer -- baseline --project $P --split development --label intact-development --reuse-compatible
cargo run --locked --release -p aarnn-explorer -- analyse --project $P --run intact-development
cargo run --locked --release -p aarnn-explorer -- experiment --project $P --plan examples/explorer/interventions/single_ablation.json
cargo run --locked --release -p aarnn-explorer -- optimise --project $P --config configs/explorer/default.toml --select min-state-bytes
cargo run --locked --release -p aarnn-explorer -- validate --project $P --candidate selected --split locked-validation
cargo run --locked --release -p aarnn-explorer -- export --project $P --candidate selected --output target/explorer-candidate
cargo run --locked --release -p aarnn-explorer -- verify-bundle --bundle target/explorer-candidate
```

Command behaviour:

- **Output and exit codes:** every command prints a JSON summary and exits with a stable code: 0 ok, 2 invalid input, 3 unsupported capability, 4 budget exhausted, 5 cancelled, 6 failed acceptance, 7 inconclusive, 8 conflict.
- **`selected`:** a project alias for an immutable candidate hash. Commands print the resolved hash.
- **`--reuse-compatible`:** reuses a baseline only when its source, suite, split and policy hashes all match.

To import a native AARNN network snapshot (designed circuit or imported connectome), use `--format aarnn-snapshot --input network.json`.

`derive` makes a new source from an existing one without modifying it. It records its provenance (`derived_from`):
- `--preset learned-cap`: the learned state of a learning network (see functional-validation.md, "Learned-state contracts");
- `--interventions plan.json`: persistent interventions (weight changes, ablations; transient silencing is refused);
- `--lower-like CANDIDATE`: physically remove a candidate's removal set, by stable ID. `--write net.json` also writes the result for a simulator. Sync that network's config from its own `net`, because per-cell masks change length.

## Further documents

| Document | Contents |
|---|---|
| [architecture.md](architecture.md) | Package and process boundaries, worker isolation, model reuse, capability table |
| [methods.md](methods.md) | GNAT scoring as implemented, recurrence, calibration, interventions |
| [data-formats.md](data-formats.md) | Sparse interchange, trace segments, records, bundle layout, IDs |
| [functional-validation.md](functional-validation.md) | Task contracts, splits, oracles, claim boundaries |
| [resilience.md](resilience.md) | Presets, fault campaigns, confidence bounds |
| [optimisation-and-hardware.md](optimisation-and-hardware.md) | Search, Pareto frontier, physical lowering, cost model, targets |
| [drosophila.md](drosophila.md) | Fly workflow, VX-001 comparison, real-data prerequisites |
| [g2-salamander.md](g2-salamander.md) | G2: motif library across five networks, the salamander crawler body and network, its reductions |
| [operations.md](operations.md) | Budgets, cancellation, restart, storage, Slurm, troubleshooting |
| [validation-report.md](validation-report.md) | Checks run, results, scale, not-run items |
