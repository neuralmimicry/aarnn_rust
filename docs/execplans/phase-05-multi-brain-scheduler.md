# Build isolated multi-brain execution and the adaptive scheduler

This ExecPlan is a living document maintained under `.agent/PLANS.md`. It implements Phase 5 of `docs/specifications/distributed-whole-brain-emulator-v1.1.md` on the reliable virtual-shard data plane.

## Purpose and observable outcome

Run several federated-capable whole-brain domains concurrently across heterogeneous resources without cross-contamination or starvation. At completion, dispatch is hierarchical, fair and work-conserving; idle capacity is borrowed; CPU/GPU/NUMA/RAM/VRAM/storage/network constraints are honoured; and versioned, explainable predictions learn which shards/components exhibit deep or amplifying cascades without silently changing biological fidelity.

## Specification authority and traceability

- Primary sections: 4.2–4.3, 7.2, 7.6, 11, 12.1, 18.1–18.3, 19, 20.6, 21.5, 21.8, 21.9 and Appendix C.
- Invariants: `INV-001`, `INV-006`, `INV-007`, `INV-013`–`INV-015` and `INV-017`.
- Tests: `VT-CAUSAL-004`, `VT-CAUSAL-008`, all scheduler/resource cases in Section 21.8 and the independent-brain portion of Section 21.9.
- Phase gate: at least four heterogeneous brains remain isolated and meet configured shares/starvation bounds while faster/idle resources receive proportionate causal work and deterministic-reference digests remain unchanged.

## Prerequisites and phase boundary

Phases 1–4 must be green. Their virtual shards, route costs, causal readiness and component observations form the schedulable work. This phase does not yet add federation links, durable replica promotion or end-user control. Backup/catch-up priority classes are modelled so Phase 6 can use them.

## Scope

- Make `BrainId` the root of execution context, queues, clocks, seeds, topology, quotas, logs/metrics labels and output namespaces.
- Discover and periodically benchmark CPU cores, NUMA domains, GPUs, deterministic kernel capabilities, RAM/VRAM, storage/network capacity and failure domains.
- Implement hierarchical admission and scheduling across tenant → brain → priority class → component/shard task.
- Enforce minimum shares, weighted fairness, starvation bounds, quotas and work-conserving idle borrowing.
- Capture versioned workload observations: ready work, event rate, state bytes, microstep depth, convergence residual, amplification, causal critical path, queue pressure, transfer cost and device utilisation.
- Implement a deterministic reference predictor and an explainable predictor interface with versioned features/models.
- Make placement/migration recommendations effective only at safe logical tags with hysteresis and deterministic tie-breaks.
- Select certified deterministic CPU/GPU kernels according to profile, transfer cost and capacity.
- Separate model depth, settling depth and scheduler prediction depth in types, metrics and UI contracts.

## Non-goals

- Do not let the scheduler change neuron/synapse equations, biological delays, numerical profile, tick quantum or `settling_limit` autonomously.
- Do not train from payloads across tenants or share identity/seed/state between brains.
- Do not activate live migration or replicas before Phase 6.
- Do not impose a shared global brain clock or global ready queue.

## Repository orientation

Locate current worker/thread-pool creation, Rayon/Tokio runtimes, GPU feature gates, global runner/network singletons, configuration, metrics and any unbounded task spawning. Record how many pools each brain/node creates and whether CPU count, NUMA, GPU or memory pressure currently influences placement.

The intended modules are `resource/inventory`, `resource/benchmark`, `scheduler/admission`, `scheduler/fairness`, `scheduler/observation`, `scheduler/predictor`, `scheduler/decision`, `scheduler/device` and `brain/context`. One bounded process runtime should serve many brains; blocking CPU/GPU/storage work uses explicit pools rather than the async control executor.

## Architecture and safety constraints

Each brain owns independent `ExecutionContext`, logical time domain, seed domain, topology generation, queues, quotas and digests. Scheduler metadata can be fleet-global, but an allocation always carries tenant/brain identity and cannot expose another brain's state. Explicit future federation is the only cross-brain causal dependency.

Fairness is hierarchical and work-conserving: guarantees and admission bounds apply before opportunistic borrowing. Causal/control/lease traffic cannot be starved by checkpoint, prediction training or catch-up. Total worker configuration respects available cores; per-brain pools must not oversubscribe the node.

Observations describe execution, not scientific truth. Non-convergence may raise predicted depth/amplification and recommend placement or an operator-reviewed fidelity policy, but it cannot redefine quiescence or automatically lower fidelity. Every state-affecting decision records feature/schema/model/config versions, inputs, explanation, safe effective tag and deterministic tie-break.

GPU selection requires a certified deterministic implementation for the active numerical profile. Unsupported operations remain on CPU or use an explicitly non-reference profile with tolerances. Capacity admission precedes allocation; OOM is not flow control.

## Milestones

### Milestone 5.1 — Brain-domain isolation

Remove process-global runner/time/seed/queue assumptions. Create multiple brain contexts in one process and across processes; namespace telemetry/state/output. Prove pause/failure/reset of one test context cannot affect another.

### Milestone 5.2 — Resource inventory and bounded executors

Publish versioned CPU/NUMA/GPU/memory/storage/network inventory and deterministic capability reports. Consolidate bounded worker pools and benchmark representative kernels/transfers without blocking async progress.

### Milestone 5.3 — Hierarchical fair dispatch

Implement admission, minimum shares, weights, starvation bounds and idle borrowing across at least two tenants and three brains. Reserve causal/control classes and make background work yield predictably.

### Milestone 5.4 — Observation and reference prediction

Emit Appendix C observations and implement a deterministic rule-based predictor. Learn recurring deep/amplifying SCC behaviour from versioned histories; expose why a resource/placement recommendation was made and confidence/uncertainty.

### Milestone 5.5 — Safe placement and device selection

Plan virtual-shard placement using effective capacity, data locality, failure domains, hysteresis and deterministic CPU/GPU capability. Apply non-migrating scheduling decisions at safe tags; emit future live-migration requests but do not execute them.

### Milestone 5.6 — Multi-brain gate

Run four brains with different sizes, tick rates, profiles and owners under load. Verify isolation, fairness, deterministic output, resource bounds and useful contribution by slow/busy and fast/idle resources.

## Progress

- [x] `2026-08-23 12:00Z` Audited global Runner/runtime/thread-pool creation and
  resource assumptions; the legacy runtime and JSON workspace manager remain
  reachable and are not the production scheduler authority.
- [x] `2026-08-23 12:00Z` Implemented independent brain contexts, bounded fair
  dispatch, resource/capability reference types and deterministic placement
  decisions in `src/multi_brain.rs`; the four-brain isolation/dispatch case in
  `phase2_to_phase8_gate` passed.
- [x] `2026-08-23 12:00Z` Added safe device/resource decision seams without
  allowing a device profile to alter biological semantics; mobile remains CPU
  reference-only until parity and lifecycle evidence exists.
- [!] `2026-08-23 12:00Z` No heterogeneous four-brain live fleet, device
  benchmark matrix or production admission integration exists. The
  `multi_brain_scheduler` flag remains disabled pending shard ownership,
  durable recovery, consensus fencing and device equivalence.
- [x] `2026-08-23 12:07Z` Final cross-review confirms the Android reference
  ABI build does not alter this gate: both mobile ABIs compile, but no device
  profile is admitted to scheduling until CPU/device equivalence, thermal and
  measured resource evidence exist. The scheduler flag remains disabled.
- [x] `2026-08-23 12:17Z` Final cross-review reran the all-feature Rust build,
  workspace tests and stable Clippy successfully, and confirmed the Android
  emulator ABI smoke test. This validates reference compilation and packaging
  only; CPU/OpenCL equivalence, thermal/resource measurements, shard-owned state,
  durable recovery and consensus fencing remain explicit blockers. The
  `multi_brain_scheduler` flag and static-placement production cutover remain
  unchanged.
- [x] `2026-08-23 13:35Z` Final Android review confirms that the Rust-enabled
  emulator ABI is packaging evidence only and contributes no admitted device
  profile. The live gateway probe also does not establish heterogeneous-fleet
  scheduling or CPU/device equivalence. Shard-owned biological state, durable
  recovery, quorum fencing, thermal/resource measurements and the hardware
  matrix remain explicit blockers; `multi_brain_scheduler` remains disabled.
- [x] `2026-08-23 14:55Z` Final cross-review after the Webots/UI run confirms
  that the Graph Explorer and live IPC evidence do not change scheduler
  admission: the captured local reference view is read-only, while the
  controller completed repeated `tx/rx` exchanges against the Rust runner.
  No heterogeneous fleet, certified OpenCL profile, thermal/resource matrix,
  shard-owned biological state, durable recovery or quorum fencing evidence
  exists; `multi_brain_scheduler` and production placement remain disabled.
- [x] `2026-08-23 17:13Z` Cross-review of the bounded workspace topology
  endpoint confirms it is read-only and sourced from the current local runner;
  it does not expose scheduler placement, shard ownership or cluster-global
  state. The Android Graph Explorer consumes the projection without changing
  scheduler admission.
- [!] `2026-08-23 17:13Z` Production blockers remain: certified CPU/OpenCL
  equivalence and thermal/resource measurements, shard-owned biological state,
  durable recovery and quorum fencing are not evidenced. The scheduler and
  production placement flags remain disabled; the endpoint is reference/local
  observation only.
- [x] `2026-08-23 17:39Z` Final sequential Rust verification and the Android
  JBR/SDK-scoped package tests passed. The bounded topology projection is
  confirmed read-only and does not admit a device, expose placement authority
  or claim cluster-global state.
- [!] `2026-08-23 17:39Z` Explicit scheduler blockers remain: certified
  CPU/OpenCL/device equivalence with thermal/resource measurements, serialised
  shard-owned biological state, durable distributed recovery and quorum
  fencing. `multi_brain_scheduler` and production placement remain disabled;
  rollback remains the existing static/legacy path.

## Validation and acceptance

- `VT-CAUSAL-004`: CPU speed/load changes wall-clock completion, not logical result; unrelated components progress.
- `VT-CAUSAL-008`: independent brains at different rates never wait on or reuse one another's tags/queues.
- Section 21.8 verifies stable prediction explanations, deep-SCC placement benefit, effective-capacity contribution, CPU/GPU selection, no core oversubscription, RAM/VRAM admission, fairness, idle borrowing, background yielding and anti-affinity recommendations.
- Section 21.9 runs at least four brains and proves identity, seed, state, quota, log, checkpoint placeholder, output and permission namespaces cannot cross-contaminate.
- Repeated non-convergence changes only a versioned recommendation effective at a safe tag; deterministic replay sees the same decision stream.
- Performance evidence reports p50/p95/p99 event/settlement latency, causal wait, utilisation, queue/memory/network bounds and agreed regression thresholds.

## Rollout, compatibility and rollback

Use `multi_brain_scheduler` with a deterministic static-placement fallback. Scheduler decisions are append-only/replayable; rollback can select the prior predictor/config at the next safe decision boundary, not rewrite decisions already effective. Persist capability/profile identifiers so restore cannot choose an uncertified kernel silently.

## Risks and mitigations

- Nested runtimes oversubscribe cores. Centralise bounded executors and test thread counts under multiple brains.
- Predictor instability causes migration thrash. Use hysteresis, minimum residence, uncertainty and explicit benefit/cost.
- Fairness can block causal critical work. Use priority classes inside guaranteed shares and reserved control capacity.
- Learned amplification may be mistaken for permission to lower fidelity. Keep fidelity controls outside scheduler authority.
- GPU nondeterminism can corrupt reference equivalence. Require capability certification and CPU fallback.

## Surprises & Discoveries

The scheduler types are reference-only and have no measured heterogeneous fleet
or device profile evidence. OpenCL equivalence is intermittent on the available
host and remains excluded from production admission.

## Decision Log

- Initial decision: scheduling learns causal workload depth/amplification, not biological model depth. Authority: Sections 7.2 and 11.4.
- Initial decision: fairness is tenant/brain hierarchical and work-conserving. Authority: Section 11.6.
- Initial decision: state-affecting decisions are versioned and effective at safe tags. Authority: Section 11.5 and `INV-013`.

## Outcomes & Retrospective

Reference isolation and dispatch tests pass. Fleet fairness/starvation,
heterogeneous device measurements and durable background-work contracts remain
open; no scheduler production promotion is claimed.
