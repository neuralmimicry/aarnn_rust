# Introduce deterministic identity, time and numerical primitives

This ExecPlan is a living document maintained under `.agent/PLANS.md`. It implements Phase 1 of `docs/specifications/distributed-whole-brain-emulator-v1.1.md`.

## Purpose and observable outcome

Create the small, authoritative primitives on which every later executor, route, checkpoint and API depends. At completion, stable biological/event identities survive dense remapping, `LogicalTag` has exact superdense ordering, deterministic arithmetic/RNG/order/digest operations are independent of iteration and thread order, and versioned serialisation is fixed by golden fixtures. Existing execution continues through compatibility adapters.

## Specification authority and traceability

- Primary sections: 3, 8, 9.2, 13.1, 17.1, 17.8, 19, 20.2, 21.2, 21.5, Appendix A and Appendix E.
- Invariants: `INV-002`, `INV-003`, `INV-004`, `INV-008`, `INV-009` and `INV-014`.
- Tests: `UT-TIME-001`–`003`, `UT-ID-001`, `UT-NUM-001`–`002`, `UT-RNG-001`, `UT-ORDER-001`; schema/digest golden fixtures; deterministic portions of Section 21.5.
- Phase gate: primitive tests pass on at least two CPU architectures where CI permits and serialisation compatibility is fixed by golden fixtures.

## Prerequisites and phase boundary

Phase 0 must have a reproducible baseline, canonical paths, command set and reviewed ADRs. Phase 1 introduces primitives and adapters only. It does not replace `Runner::step`, perform distributed routing, repartition graphs, change checkpoint ownership or expose new management behaviour.

## Scope

- Define stable newtypes for brains, shards, neurons, synapses, terminals, routes, streams, events, components, topology/partition generations, lease terms and schema versions as required by the model.
- Define lexicographically ordered `(tick, microstep)` `LogicalTag`, checked progression and overflow behaviour.
- Add stable-to-dense maps scoped by generation while retaining legacy index adapters during migration.
- Implement canonical event sort keys and deterministic hierarchical state/event digests.
- Implement per-variable dimensioned fixed-point support, checked conversion/rounding and deterministic accumulation with widened intermediates.
- Implement counter-based deterministic randomness using stable coordinates rather than mutable traversal-order state.
- Add versioned DTOs/golden fixtures without serialising raw in-memory layouts.

## Non-goals

- Do not claim one universal fixed-point format is suitable for every biological variable.
- Do not change biological event progression in the legacy runner.
- Do not introduce transport, persistence or scheduler policy into primitive crates.
- Do not delete legacy vector-index fields until later migration gates prove all consumers moved.

## Repository orientation

Verify and record the canonical locations for `network.rs`, `transmission.rs`, `dynamics.rs`, topology/model DTOs, RNG utilities, serialisation schemas, generated protocol code and tests. The target responsibility split is approximately `identity/`, `time/`, `numeric/` and `event/`, but exact crate boundaries must follow the discovered workspace. Keep these primitives independent of gRPC, HTTP, UI and consensus.

Document all existing implicit IDs, floating-point accumulations, mutable RNGs and hash functions, plus every persisted or wire-visible use that needs a compatibility adapter.

## Architecture and safety constraints

Stable IDs are never derived solely from mutable vector positions or physical node IDs. Allocation must be deterministic or durably authoritative as appropriate, collision-checked and generation-aware.

`LogicalTag` ordering is `(t, μ)` lexicographic. Zero-delay progression is checked `μ + 1`; positive delay is checked `(t + δ, 0)`; backwards tags and overflow fail before state mutation.

The specification's Q32.32 test is a required candidate conversion profile, not permission to force all variables into Q32.32. Record physical/model units, expected ranges, precision/error budgets, rounding (round-to-nearest ties-to-even where specified), overflow policy and high-precision oracle comparison for each authoritative quantity. Use widened intermediates such as `i128` where the selected representation requires them.

Canonical ordering and digests must not depend on hash-map iteration, allocation address, thread scheduling, physical placement or transport batching. RNG coordinates include stable brain/entity/event coordinates and named draw purpose.

## Milestones

### Milestone 1.1 — Stable types and version envelopes

Introduce non-interchangeable ID/generation/version newtypes with validated parsing, explicit serialisation and boundary tests. Add compatibility conversions at legacy edges and schema golden fixtures. Demonstrate that accidental cross-type assignment is a compile-time error and unknown/new schema values fail safely or follow the documented compatibility rule.

### Milestone 1.2 — Superdense logical tag

Implement `LogicalTag`, checked order/progression and display/audit conversions. Replace duplicate time-tag definitions at new interfaces while leaving legacy runner pacing unchanged. Prove boundary, overflow, zero-delay and positive-delay rules through `UT-TIME-001`–`003`.

### Milestone 1.3 — Stable biological mapping

Introduce generation-scoped stable-to-dense and dense-to-stable maps. Migrate one bounded model path and its tests to stable neuron/synapse/terminal identity while keeping dense arrays for kernels. Prove compaction/reordering preserves targets and rejects stale generation mappings.

### Milestone 1.4 — Deterministic numerical foundation

Implement dimensioned fixed-point types/profiles, checked conversions, widened accumulation, rounding and overflow handling. Build a high-precision reference oracle and range/property tests, including Q32.32 golden conversions. Separate authoritative deterministic operations from any fast floating-point profile.

### Milestone 1.5 — RNG, canonical order and digest

Implement counter-based RNG with stable named coordinates, canonical event sort keys and versioned hierarchical digests. Exercise permutations, thread/work-stealing orders and repeated runs. Add digest domains so incompatible schema/profile versions cannot be compared accidentally.

### Milestone 1.6 — Compatibility gate

Integrate primitives behind legacy adapters in a baseline scenario. Confirm no unintended runtime semantic change, serialisation fixtures remain stable and the deterministic primitive suite passes on available heterogeneous architectures.

## Progress

- [x] `2026-08-23 12:00Z` Verified the canonical consumers in `src/deterministic.rs`,
  `src/causal.rs`, `src/topology_model.rs`, `src/durability.rs`, the protobuf
  conversion and the compatibility runner paths.
- [x] `2026-08-23 12:00Z` Implemented stable IDs, schema/version envelopes,
  logical tags, generation-scoped dense maps, canonical ordering, counter RNG,
  digests and fixed-point helpers; `cargo test --locked --test
  phase1_deterministic_primitives` passed (4 tests).
- [x] `2026-08-23 12:00Z` Adopted `LogicalTag` at the new causal/topology,
  transport, field-event and persistence reference boundaries; progression
  boundary tests passed in the workspace suite.
- [!] `2026-08-23 12:00Z` The proposed standalone primitive crates and
  heterogeneous CPU-target architecture/serialisation matrix are not yet in
  the workspace. Per-variable scientific error analysis is also not published;
  the Phase 1 production gate remains blocked.

## Validation and acceptance

- `UT-TIME-001`: exact lexicographic order, serialisation and boundary values.
- `UT-TIME-002`: zero delay yields `μ + 1`; positive delay yields `(t + δ, 0)`; backwards tags reject.
- `UT-TIME-003`: tick/microstep overflow is detected before mutation.
- `UT-ID-001`: dense compaction/reordering and migration fixtures preserve stable targets.
- `UT-NUM-001`: Q32.32 conversion and representable limits match golden ties-to-even values.
- `UT-NUM-002`: widened arithmetic, negatives and configured overflow policy satisfy property/oracle tests.
- `UT-RNG-001`: draws are identical across iteration/thread order and distinct for different coordinates.
- `UT-ORDER-001`: every permutation of an input multiset produces the same canonical sequence and digest.
- Golden serialisation detects incompatible field/order changes and supports the documented rolling version window.
- Run deterministic tests on at least two CPU architectures when runners permit; record any architecture not yet available.

## Rollout, compatibility and rollback

Adopt primitives additively. Persist/wire new versions only through explicit version envelopes and dual-read/controlled-write migration where required. A coarse compatibility flag may select legacy or typed edges, but both must call the same primitive implementation. Rollback remains possible while no new-only checkpoint/protocol version has crossed the recorded point of no return.

## Risks and mitigations

- ID retrofitting can accidentally change model connectivity. Use bidirectional map validation and golden topology fixtures.
- Fixed-point range mistakes can silently saturate. Require per-variable range proofs, oracle comparisons and explicit counters/errors.
- Digest changes can masquerade as state divergence. Domain-separate by schema/numerical profile and version fixtures.
- RNG coordinates can omit a causal dimension. Review every draw purpose and test collision/independence cases.

## Surprises & Discoveries

The reference primitives currently coexist with legacy in-memory/JSON paths;
the proposed crate extraction has not happened. This is a compatibility
boundary, not evidence that the duplicate paths are production-equivalent.

## Decision Log

- Initial decision: stable IDs and dense kernel indices coexist; dense indices are never sole persisted identity. Authority: Sections 3.1, 9.2 and 13.1.
- Initial decision: numerical formats are dimensioned per variable/profile; Q32.32 is a tested candidate, not a universal mandate. Authority: Section 8.2.
- Initial decision: deterministic randomness is counter-based and coordinate-addressed. Authority: Section 8.4.

## Outcomes & Retrospective

The host reference primitive suite and golden serialisation checks pass. The
required extracted crate architecture, target matrix and measured numerical
error report remain open and are carried into the mobile and device-equivalence
gates.
