# Establish the baseline and safety net

This ExecPlan is a living document maintained under `.agent/PLANS.md`. It implements Phase 0 of `docs/specifications/distributed-whole-brain-emulator-v1.1.md` without changing runtime semantics.

## Purpose and observable outcome

Create a reproducible description of what the repository builds and does before the distributed architecture changes. At completion, a developer or Codex session can reproduce supported standalone and seven-node behaviour, including burst-to-persistent gRPC fallback, and can run an authoritative CI command set. Current behaviour is captured honestly: a baseline may document redundant layer computation or cross-node incoherence, but must not certify it as correct.

## Specification authority and traceability

- Primary sections: 1, 2, 19, 20.1, 21.1, 21.14, 22 and Appendix D.
- All `INV-001`–`INV-017` are future safety constraints; Phase 0 records current compliance/gaps without implementing them.
- Phase gate: supported builds/tests pass, baseline artefacts are reproducible and runtime semantics are unchanged.
- Required architecture decisions: logical time, synaptic event boundary, numerical profiles, durability and control-plane fencing.

## Prerequisites and phase boundary

There is no implementation-phase prerequisite. Work on a dedicated branch or worktree and preserve existing dirty changes. Phase 0 may add tests, fixtures, documentation, deterministic instrumentation and CI configuration, but must not alter biological transitions, distributed assignment, transport semantics, persistence compatibility or UI behaviour except to expose existing behaviour diagnostically.

## Scope

- Inventory workspace crates/packages, entry points, feature flags, generated artefacts, protocols, environment variables, deployment scripts and test suites.
- Resolve canonical paths corresponding to every candidate file in Section 19.1.
- Capture supported standalone and seven-node scenarios with deterministic input fixtures, state/event snapshots and resource/transport metrics.
- Reproduce the observed small-network layer-sharding fallback and 120 ms burst timeout/persistent gRPC fallback where the current code still contains them.
- Establish formatting, lint, unit, integration, web/browser, protocol/schema, security, licence and documentation checks.
- Create ADRs under `docs/architecture/decisions/` for the five locked design areas.

## Non-goals

- Do not fix layer sharding, logical time, transport reliability or failover yet.
- Do not introduce new IDs, schemas, executor paths or production feature flags.
- Do not rewrite modules merely to make the inventory cleaner.
- Do not treat nondeterministic current results as golden scientific truth; record variability and provenance.

## Repository orientation

Before implementation, replace this initial orientation with verified repository-relative paths. Locate manifests and canonical implementations for `distributed.rs`, `runner.rs`, `network.rs`, `topology.rs`, `transmission.rs`, `aer.rs`, `bridge.rs`, `dynamics.rs`, `morphology.rs`, `transport.rs`, `ui.rs` and the web files. Locate any USB/libusb/rusb, serial, AER hardware bridge, hot-plug and device-permission code, including whether it blocks a UI/runtime thread or bypasses causal admission. Locate `.proto`/OpenAPI sources, generated outputs, container/deployment definitions, CI workflows, existing golden data and runtime configuration.

Record the exact supported toolchains and commands rather than assuming a single Cargo workspace or JavaScript package manager. Identify how seven nodes are launched in CI or a controlled test environment and which hardware-dependent checks require a labelled runner.

## Architecture and safety constraints

Baseline instrumentation must be observational. Use stable fixture-local identifiers and canonical serialisation only for test artefacts; do not leak provisional formats into production APIs. Redact credentials, tokens and sensitive neural/peripheral payloads. Bound logs and captures by size and duration.

If a USB AER device or fixture is available, capture its descriptor/protocol version, endpoint modes, timestamp source, sequence/overflow behaviour, maximum measured event rate, hot-plug/reconnect behaviour and coexistence with existing workstation A/V/HID. Store no unredacted serial number or raw sensitive payload in ordinary logs. Where hardware is unavailable, create a deterministic USB AER emulator/fault fixture without claiming physical-device validation.

Golden artefacts must include code revision, configuration digest, seed/input digest, toolchain/platform, node topology and the exact command. If repeated current runs differ, preserve each result or an accepted variability envelope and mark the scenario nondeterministic.

## Milestones

### Milestone 0.1 — Repository and workflow inventory

Map workspace packages, binaries, services, protocols, generated sources, configuration precedence, deployments and test commands. Produce a concise repository architecture note and update this plan with canonical paths. The milestone is proven when a clean checkout can follow the recorded commands through dependency preparation, build and existing tests without undocumented manual steps.

### Milestone 0.2 — Reproducible current-behaviour fixtures

Add deterministic inputs and bounded capture tooling for at least one standalone brain and the current seven-node three-layer deployment. Capture layer assignments, emitted/received spike batches, transport selection/fallback, state snapshots/digests and resource metrics. Run each scenario repeatedly and document stability or variance. No assertion may describe current cross-node execution as causally correct unless independently proved.

### Milestone 0.3 — CI safety net

Encode the discovered formatting, lint, unit, integration, web, browser, schema, security, dependency/licence and documentation commands in CI with appropriate fast and scheduled tiers. Hardware, multi-node, chaos and performance jobs may use labelled/nightly runners but must produce durable evidence and explicit skip reasons when infrastructure is unavailable.

### Milestone 0.4 — Architecture decisions

Create ADRs for superdense logical time, precise event-stage ownership, per-variable numerical profiles/deterministic reference mode, active/warm/immutable-checkpoint durability and quorum lease/fencing. Each ADR records context, decision, alternatives, consequences and compatibility/migration effect, and cites the normative specification sections.

### Milestone 0.5 — Baseline gate

Repeat the supported scenarios from a clean environment, compare artefacts, inspect the diff for accidental runtime changes and record the accepted baseline version. The result becomes the reference used by later phases for behavioural comparison, without freezing known defects as required semantics.

## Progress

- [x] `2026-08-23 12:00Z` Recorded the workspace, manifests, canonical Rust,
  protobuf, UI, persistence and deployment paths with `cargo metadata`, source
  inspection and `rg`; the active cross-review records the dirty-worktree
  boundary.
- [x] `2026-08-23 12:00Z` Captured a reproducible standalone reference digest;
  `cargo test --locked --test phase0_baseline` and the corresponding unit test
  passed.
- [x] `2026-08-23 12:00Z` Replayed the seven-node compatibility fixture;
  `distributed::tests::phase0_seven_node_capture_matches_current_compatibility_behaviour`
  passed in the workspace suite.
- [!] `2026-08-23 12:00Z` Fast/scheduled multi-node, browser, security and
  documentation CI tiers plus the five ADRs are not present in this workspace.
  The Phase 0 gate is therefore not complete; these require repository CI and
  review ownership.

## Validation and acceptance

- A clean checkout executes every documented supported build/test command.
- The same fixture records its code/config/input/toolchain provenance and produces reproducible artefacts or an explicitly quantified nondeterministic envelope.
- The seven-node evidence includes node/layer assignments and transport fallback rather than relying on log anecdotes.
- CI fails on formatting, lint, test, stale generated schema/client, documentation-link or unsafe-code-policy violations according to the recorded policy.
- `git diff` and behavioural comparisons show no intentional runtime semantic change.
- Documentation examples used by the baseline are exercised where practical.

## Rollout, compatibility and rollback

This phase adds observation, tests and documentation only. Any instrumentation capable of affecting timing must be disabled by default or use bounded asynchronous export, and its measured overhead must be reported. Rollback removes the instrumentation/CI wiring without changing saved brain state or protocols. Baseline artefacts remain immutable and versioned for comparison.

## Risks and mitigations

- Current concurrency may prevent bitwise repeatability. Preserve inputs and event traces, quantify variance and avoid false golden assertions.
- Seven-node hardware may not be present in ordinary CI. Provide a deterministic process-level topology and a labelled real-cluster job.
- Generated outputs may be mistaken for sources. Record generators and add stale-output checks.
- Diagnostic logging may perturb the 120 ms fallback. Measure overhead and provide lower-volume counters/event sampling.

## Surprises & Discoveries

The repository is a single primary Rust package with an exporter dependency;
the generated protobuf Rust module is build-time output rather than a checked-in
file. Native mobile projects, browser automation and multi-process acceptance
infrastructure are absent and are carried forward as explicit blockers.

## Decision Log

- Initial decision: Phase 0 is observational and must not repair known distributed defects. Authority: Section 20.1.
- Initial decision: unstable current outputs are evidence, not normative golden biology. Authority: Sections 1 and 21.1.

## Outcomes & Retrospective

Host baseline and compatibility evidence are recorded. Reproducibility is
limited to the available host/reference fixtures; heterogeneous target,
browser/device and distributed CI evidence remains open and is carried into
the later phase gates.
