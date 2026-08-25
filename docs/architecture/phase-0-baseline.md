# Phase 0 repository architecture baseline

This note records the verified pre-migration architecture. It is descriptive, not a claim that the current distributed execution satisfies the normative invariants.

## Runtime composition

`src/main.rs` starts standalone, node, orchestrator, MPI-bootstrap, or related compatibility modes. `src/bin/web_ui.rs` serves the web management and visualisation surface. Both expose the library modules through the root package. `src/runtime.rs` owns local workspace scheduling and JSON snapshot persistence. `src/runner.rs` remains the monolithic biological step owner; `src/network.rs` builds layer/matrix topology and `src/morphology.rs` mutates growth state.

`src/distributed.rs` manages cluster membership, network assignment, heartbeat/control RPCs, snapshot/activity RPCs, and spike transport selection. Its current sharding representation is network/layer based and can retain overlapping full network views. The current transport choices include MPI when available, persistent gRPC streams, and short-lived burst gRPC streams with a 120 ms timeout and failure-streak fallback.

`src/aer.rs` and `src/spike_io/transport.rs` provide AER1 and vector compatibility helpers. `src/bridge.rs` is an optional robot/simulator bridge behind `robot_io`; `aer_fabric_bridge/` is a separate synapse-addressed UDP/FPAA service. The current workstation paths do not provide the specification's independently governed concurrent peripheral session model.

## Contracts and generated code

`proto/distributed.proto` is compiled by `build.rs` using tonic/prost into Cargo's generated `OUT_DIR`. The protocol currently carries layer indices, spike vectors/AER bytes, JSON snapshots, and operational status. There is no checked-in generated output and no OpenAPI source discovered. Web assets are static files under `web_ui/` and are served by Rust; there is no `package.json` or Node build step.

## Verification baseline

The pinned toolchain is Rust 1.92.0 minimal with rustfmt locally; CI now installs clippy explicitly. CI runs formatting, clippy, locked all-feature/all-target checking, selected all-feature tests with OpenCL disabled, doctests, the standalone Phase 0 fixture, release binary builds, CLI help smoke tests, and a shell syntax check. Local default-profile formatting, workspace tests, and doctests pass as recorded in `docs/execplans/phase-00-baseline.md`. All-feature verification needs the native dependency set installed by CI.

The accepted fixture artefacts are immutable inputs for later comparisons:

- `tests/fixtures/phase0_standalone.json`: SHA-256 `fa5b0d460a2c80fb4b609b8e20167978b71a3bbdf0a425cb5c0d8486ba9efa84`; seeded output digest `e60b83dc0209bfc4`.
- `docs/architecture/baseline/phase0-seven-node-layer.json`: SHA-256 `f5cf46f36d1e5b7ad696b441de93c318dd118e4b3746002ed388391fae3adac0`; process-level compatibility evidence at source revision `92b525d`.

## Known migration gaps

The current implementation does not yet provide typed superdense logical time, stable biological ownership across topology generations, reliable causal envelopes with durable receipt, immutable distributed checkpoints, quorum fencing, orchestrator-only management, or independently authorised peripheral admission/effect delivery. These gaps are carried into Phases 1–8 and must not be hidden by compatibility terminology.
