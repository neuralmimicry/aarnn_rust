# Build the local superdense event executor

This ExecPlan is a living document maintained under `.agent/PLANS.md`. It implements Phase 2 of `docs/specifications/distributed-whole-brain-emulator-v1.1.md` inside one process before distributed transport is introduced.

## Purpose and observable outcome

Replace the monolithic conceptual `step` with explicit causal phases while preserving a legacy facade. At completion, one complete brain can run locally as multiple virtual shards through typed event queues, produce the same deterministic-reference result regardless of local parallel schedule, prove component-scoped quiescence, and preserve/checkpoint/defer unresolved work when the settling limit is reached. No process-wide per-tick barrier is introduced.

## Specification authority and traceability

- Primary sections: 3.3, 6, 7, 8, 9, 10.5, 13.2, 19, 20.3, 21.3–21.5, Appendix A and Appendix E.
- Invariants: `INV-002`–`INV-008`, `INV-010` and `INV-014`.
- Tests: `UT-SETTLE-001`, `UT-SETTLE-002`, `UT-DEF-001`, `UT-SYN-001`; local forms of `VT-CAUSAL-001`, `003`, `004`, `007`, `009`, `010`; all assertions in Section 21.4; deterministic-reference cases in Section 21.5.
- Phase gate: exact local deterministic replay, no event loss on non-convergence and no global barrier inside one process.

## Prerequisites and phase boundary

Phase 1's stable IDs, `LogicalTag`, canonical ordering, deterministic arithmetic/RNG and digests must be green. Phase 2 uses local in-memory/reconstructible queues and a local reference settlement mechanism. Formal topology-derived zero-delay SCC construction arrives in Phase 3; reliable cross-process streams and distributed termination arrive in Phase 4.

Until Phase 3 supplies conservative components, use an explicit safe local component definition (including a whole-brain local fallback if necessary) rather than guessing independence. This may reduce parallelism but must not alter causality.

## Scope

- Refactor `Runner::step` into explicit input collection, synaptic/model transition, deterministic accumulation, neuron update, plasticity/homeostasis/growth proposal, output staging and commit phases.
- Represent model stages precisely: `SpikeDecision`, `AxonalDeparture`, `AxonalArrival`, `SynapticTransition`, `PostsynapticEffect` and `PlasticityUpdate`, with one authoritative owner per stateful transition.
- Run local virtual shards as actors/tasks with bounded queues and work-conserving CPU execution.
- Implement same-tick microstep progression and positive-delay future-tick queues.
- Implement microstep/tick closure and exact local quiescence; separate wall-clock pacing.
- Implement `Converged`, `DeferredNonConvergent`, `Blocked` and `Failed` outcomes.
- Implement provisional immutable non-convergence state, complete current-microstep proof, event-set digest, marked deferral to the configured next biological quantum and `NonConvergenceDiscontinuity`.
- Convert global AARNN effects into explicit scheduled field events.
- Preserve a temporary legacy facade and compare both paths using Phase 0 fixtures.

## Non-goals

- Do not build final SCC/topology generation algorithms or live repartitioning.
- Do not send events across processes or use network watermarks.
- Do not implement control-plane leases, warm replication or external workstation effects.
- Do not tune GPU kernels before the deterministic reference executor is correct.
- Do not treat the local fallback component scope as the final partitioning policy.

## Repository orientation

Verify canonical locations for `runner.rs`, `network.rs`, `transmission.rs`, `dynamics.rs`, `bridge.rs`, `aer.rs`, morphology/growth code and tests. Record every place that advances `self.t`, `self.t_ms`, nested runner time or wall-clock pacing independently. Record every place that mutates synaptic/release/plasticity state and assign the model-defined authoritative owner.

The intended responsibility split is `executor/` for shard actors, phases, queues and settlement; `event/` for envelopes/payloads/order; `time/` for tags/pacing; and biological modules for pure state transitions. Keep transport and UI dependencies outside the executor.

## Architecture and safety constraints

An event's tag denotes the selected model-stage transition, not simply presynaptic spike emission or packet arrival. A zero-delay consequence from `(t, μ)` is eligible only at `(t, μ + 1)`; a positive delay `δ` is eligible at `(t + δ, 0)`. Consume all admissible input in canonical order before committing a microstep.

Quiescence requires completed deterministic local transition, empty ready/output staging for the tag, accounted emitted events and stable expected producers/component membership. Queue emptiness alone is insufficient. Unknown in-flight/local ownership evidence produces `Blocked`/`Failed`, not non-convergence.

At `settling_limit`, finish and prove the current microstep before recording provisional state. Preserve unresolved event count, payload, original tags, canonical digest and deferral root/count. Retag to the configured next quantum with `deferred_from_nonconvergence`; this is an explicitly trajectory-changing approximation. High-risk effects are not present in this phase.

Queues, worker pools and staging memory are bounded. Backpressure occurs before causal admission where rejection/drop is policy-permitted; committed events cannot be discarded. Parallel kernels write exclusive slices or private buffers followed by deterministic segmented reduction.

## Milestones

### Milestone 2.1 — Reference interpreter and phase contracts

Extract pure phase interfaces and build a small single-thread reference interpreter using authoritative primitives. Define event-stage ownership and transition inputs/outputs in types and tests. Keep `Runner::step` as a compatibility facade calling the selected legacy/new path without changing default behaviour.

### Milestone 2.2 — Local virtual shards and event queues

Represent one brain as stable local virtual shards with bounded ready/future queues and work-conserving dispatch. Implement canonical dequeue, same-tick microsteps and future ticks. Demonstrate slow and fast local shard tasks do not change the final digest and unrelated safe components can progress independently.

### Milestone 2.3 — Component closure and quiescence

Implement local activity epochs/producer accounting sufficient to prove microstep and tick closure. Introduce explicit outcome types and evidence. Prove that queue emptiness, silence and settling-limit exhaustion cannot produce `Quiescent`.

### Milestone 2.4 — Non-convergence checkpoint and deferral

Construct a known amplifying zero-delay loop. On cap, complete the current microstep proof, publish/reference immutable provisional state, record `NonConvergenceRecord` and discontinuity, preserve the event-set digest and defer every unresolved event to the configured next quantum with provenance. Continue an unrelated component/brain.

### Milestone 2.5 — Explicit global field events

Convert resonance, homeostasis, ambient drive and other configured global effects into versioned scheduled events with declared scope, cadence, reduction and effective tag. No hidden field implementation may force every shard to wait at every tick.

### Milestone 2.6 — Legacy comparison and local gate

Run Phase 0 fixtures through legacy and new paths. For deterministic-reference cases, compare against the single-thread interpreter and explain intentional semantic corrections separately from accidental regressions. Keep the new path behind `superdense_executor` until the phase gate and rollback evidence pass.

## Progress

- [x] `2026-08-23 12:00Z` Audited `Runner::step`, engine dispatch and new causal
  ownership boundaries; the audit confirms Runner remains the production
  biological-state owner and is retained as a compatibility facade.
- [x] `2026-08-23 12:00Z` Implemented the local causal reference interpreter,
  bounded event queue, deterministic phase contracts and explicit settlement
  outcomes in `src/causal.rs`; causal unit tests passed.
- [x] `2026-08-23 12:00Z` Implemented component-scoped closure and the complete
  non-convergence/deferred-event procedure; `phase2_to_phase8_gate` passed the
  non-convergence, failed-transition and event-preservation cases.
- [x] `2026-08-23 12:00Z` Implemented explicit future-tag field events and the
  feature-gated `superdense` adapter; field-event tests passed.
- [!] `2026-08-23 12:00Z` A complete shard-owned local brain is not wired into
  Runner replacement, and the `superdense_executor` gate cannot be promoted
  until Phase 3 ownership and Phase 6 durability are complete.

## Validation and acceptance

- `UT-SYN-001`: every terminal/release/weight/plasticity state has one owner and each transition is emitted once.
- `UT-SETTLE-001`: removing any closure condition prevents quiescence.
- `UT-SETTLE-002`: the cap returns `DeferredNonConvergent`, never `Quiescent`.
- `UT-DEF-001`: event count, payload, original tag and digest survive retagging.
- `VT-CAUSAL-001`: a local zero-delay edge yields one transition at `(t, μ)` and its consequence at `(t, μ + 1)`.
- `VT-CAUSAL-003`: positive-delay feedback does not block the current tick.
- `VT-CAUSAL-004`: slow and fast shard execution changes wall-clock latency, not logical result.
- `VT-CAUSAL-007`: field cadence is explicit and introduces no per-tick whole-brain barrier.
- `VT-CAUSAL-009`/`010`: event-stage ownership, delay/failure provenance and oscillatory/travelling-wave phase relationships match the reference model.
- Section 21.4's full non-convergence assertions pass, including unrelated progress, traceable repeated deferral and quantified divergence from a higher-limit reference.
- Section 21.5 repeats the deterministic fixture across one/max threads, work-stealing schedules and batching with exactly equal committed digests/event sequence.

## Rollout, compatibility and rollback

Use the coarse `superdense_executor` flag. Persist which path created any diagnostic/provisional state. Do not make new-only checkpoints the sole recovery source until Phase 6 defines migration. Rollback selects the legacy facade only while no new semantics/state have crossed the documented compatibility boundary. Remove the legacy facade in Phase 8 after full acceptance.

## Risks and mitigations

- Ambiguous biological event boundary can double-apply or omit release effects. Resolve ownership from the model/schema and test each stage explicitly.
- A local closure shortcut can become an invalid distributed assumption. Keep closure interfaces capable of Phase 4 termination evidence and avoid using silence.
- Non-convergence deferral can be mislabelled as equivalence. Propagate provisional quality/discontinuity and quantify divergence.
- Parallel reductions can change results. Compare every optimised path with the single-thread deterministic interpreter.
- Compatibility facade can advance time twice. Centralise time in `ExecutionContext` and search for all legacy increments.

## Surprises & Discoveries

The new executor is a reference adapter around Runner; Runner still owns the
biological vectors and remains reachable from UI, GA and distributed paths.
That ownership gap is the prerequisite blocker for production promotion.

## Decision Log

- Initial decision: one precise model-stage `SynapticTransition` is the cross-shard synchronisation boundary; presynaptic departure, arrival and postsynaptic effect retain separate tags/owners. Authority: Section 3.3.
- Initial decision: cap exhaustion follows the complete non-convergence procedure and changes the trajectory explicitly. Authority: Sections 7.3–7.5.
- Initial decision: local conservative component fallback may reduce parallelism but cannot guess causal independence before Phase 3. Authority: `INV-010` and Phase 3 boundary.

## Outcomes & Retrospective

The local reference/non-convergence evidence passes. Production execution still
uses the legacy Runner facade, so new/legacy equivalence, performance limits and
the shard-owned hand-off remain open for Phases 3 and 6.
