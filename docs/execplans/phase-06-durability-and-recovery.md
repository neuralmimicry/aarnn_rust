# Build durable shard replication, recovery and live migration

This ExecPlan is a living document maintained under `.agent/PLANS.md`. It implements Phase 6 of `docs/specifications/distributed-whole-brain-emulator-v1.1.md` while preserving the replicated-control-plane boundary owned by Phase 7.

## Purpose and observable outcome

Ensure that losing a compute node does not destroy a brain whose configured durability state was healthy. At completion, each protected shard has one active authority, a synchronous warm log replica, periodic immutable checkpoints and optional colder copies; stale replicas fast-forward and validate before rejoining; live migration preserves one owner; and failover/replay never fabricates seamlessness or duplicate committed output.

## Specification authority and traceability

- Primary sections: 8.5, 10.3, 14, 15.3–15.9, 17.6–17.8, 18.4–18.5, 19, 20.7, 21.5, 21.7 and Appendices A–B.
- Invariants: `INV-001`, `INV-004`, `INV-005`, `INV-008`, `INV-012`, `INV-015` and `INV-016`.
- Tests: `UT-LOG-001`, `UT-CHK-001`, `IT-DIST-007`, `CT-001`–`004`, `CT-006`, `CT-007`, `CT-009`–`011`; controlled-harness portions of `CT-005` and `CT-008` are finalised under Phase 7 quorum authority.
- Phase gate: configured RPO/RTO are measured; one compute-node loss does not destroy a healthy protected brain; no stale or unvalidated replica becomes active; failover/migration produces one authoritative event/output history.

## Prerequisites and phase boundary

Phases 1–5 must be green. Reliable event streams, stable generations, digests, isolation, placement and background priority classes already exist. This phase implements the storage/recovery mechanisms and a test authority with explicit terms. Production promotion, membership, lease renewal and split-brain fencing depend on the replicated control plane in Phase 7; do not claim production-safe automatic failover before that gate.

## Scope

- Define causal write-ahead log (WAL) records/segments with checksums/hash chains, logical tags, topology/authority versions and event/application metadata.
- Synchronously replicate the configured commit boundary to a warm backup on an anti-affine failure domain.
- Produce periodic immutable content-addressed shard checkpoints with atomic manifest publication, plus configurable colder copies.
- Capture consistent cuts containing state, queues, dedupe windows, watermarks, component settlement, stream/WAL offsets, topology, numerical/RNG versions, scheduler decisions and in-transit-channel evidence.
- Implement retention, quota, verification, repair and secure deletion policy.
- Implement restore, fast-forward, digest comparison, divergence quarantine and safe rejoin.
- Implement failover discontinuity/quality propagation and suppression/deduplication of external effects.
- Implement live migration through prepare, snapshot/log catch-up, digest validation, authority cutover and cleanup.
- Enforce active/warm/checkpoint anti-affinity and explicit degraded-durability states.
- Implement consistent brain export coordination without a whole-brain per-tick barrier.

## Non-goals

- Do not implement quorum membership, production lease issuance or end-user restore/export operations; Phase 7 owns them.
- Do not promote a backup merely because the active is unreachable.
- Do not call an asynchronous stale copy “synchronous warm” or overwrite immutable checkpoints.
- Do not hide failover/non-convergence quality from dependent output.

## Repository orientation

Locate current model serialisation, checkpoint/export/import paths, filesystem/object-store adapters, event buffering, delivery acknowledgement, output staging and node-health handling. Record whether current snapshots include in-flight events, RNG/scheduler versions, dedupe state and topology generation; record atomicity/fsync/object-publish assumptions.

The intended modules are `durability/wal`, `durability/checkpoint`, `durability/manifest`, `durability/store`, `durability/retention`, `replica/state_machine`, `recovery/replay`, `recovery/fast_forward`, `migration/protocol` and `export/consistent_cut`. Storage backends implement narrow traits and cannot decide promotion.

## Architecture and safety constraints

The recommended arrangement is one active writer, one synchronously updated warm WAL replica on an anti-affine node, periodic immutable checkpoints in durable storage and optional additional cold copies. Durability policy is per brain/shard and status must distinguish healthy, catching up, degraded and unprotected.

The authoritative commit boundary is explicit. State/events/outputs beyond the last acknowledged durable boundary are not advertised as committed. WAL replay is idempotent and validated by stable IDs, sequence/dedupe state and hierarchical digests. A checkpoint becomes discoverable only after every object verifies and its immutable manifest is atomically published.

A backup/recovered node remains `Recovering` until it holds the current topology/term, replays from a valid checkpoint/WAL horizon, accounts for streams/in-transit work and matches the expected digest. Mismatch quarantines it. Promotion requires Phase 7 quorum authority; local reachability never suffices.

Failover may create a bounded biological/output discontinuity. Record it, propagate quality and suppress high-risk external effects by default. Committed effects use stable `EffectId` dedupe; uncertain-after-disconnect remains explicit. Fast-forward can use spare compute and reduced presentation but cannot skip authoritative transitions or change numerical/fidelity profiles.

## Milestones

### Milestone 6.1 — Versioned causal WAL

Specify and implement checksummed/hash-chained records and immutable segments. Integrate the durable commit boundary with causal send/application and restore. Corruption, truncation and incompatible versions fail explicitly.

### Milestone 6.2 — Immutable consistent-cut checkpoints

Implement snapshot preparation, content addressing, object verification and atomic manifest publication. Include queues, channel/progress/termination evidence and every version needed for deterministic replay. Exercise partial upload and corrupt-newest fallback.

### Milestone 6.3 — Warm replication and anti-affinity

Replicate WAL synchronously to a reserved warm target and expose durability health/RPO. Schedule checkpoint/copy/verification work in Phase 5 background classes and yield to causal traffic. Report insufficient failure domains rather than co-locating silently.

### Milestone 6.4 — Restore, fast-forward and rejoin

Restore from the newest valid compatible cut, resume logs/streams, suppress duplicate side effects, fast-forward to the active horizon and compare digests. Only validated replicas become eligible; divergent copies remain quarantined with evidence.

### Milestone 6.5 — Controlled failover and discontinuity

Under a deterministic test authority, fail the active at each commit boundary, issue a new term, fence the old term at event/log/checkpoint/output adapters and recover exactly once. Record RPO/RTO and discontinuity. Mark production promotion pending Phase 7.

### Milestone 6.6 — Live migration and consistent export

Reserve the target, transfer checkpoint/state, stream delta WAL, verify digest, commit one authority cutover and clean up idempotently. Coordinate brain-level exports as consistent cuts without synchronising every biological timestamp.

## Progress

- [x] `2026-08-23 12:00Z` Audited serialisation, filesystem atomicity, snapshot
  restore and output-commit boundaries; JSON workspace persistence is recorded
  as incomplete for a distributed causal cut.
- [x] `2026-08-23 12:00Z` Implemented fenced, versioned WAL append/reopen and
  immutable verified checkpoint reference primitives in `src/durability.rs`;
  durability unit tests and `phase2_to_phase8_gate` passed.
- [!] `2026-08-23 12:00Z` Complete shard cuts, synchronous warm replication,
  anti-affinity, active/backup orchestration, deterministic failover,
  quarantine, migration/export and measured RPO/RTO are not implemented.
  `replicated_durability` remains disabled and the Phase 6 gate is blocked.

## Validation and acceptance

- `UT-LOG-001`: any record/segment mutation is detected; valid chains replay exactly.
- `UT-CHK-001`: partial objects are undiscoverable and published manifests cannot be overwritten.
- `CT-001`–`004`: failures around durable acknowledgement/checkpoint publication and corruption recover from the correct boundary with no false commit.
- `CT-006`/`007`: a stale recovered active cannot resume and loss of active plus one store endpoint meets configured RPO or pauses safely.
- `CT-009`/`010`: migration failures before/after cutover retain exactly one owner and clean temporary state idempotently.
- `CT-011`: quota exhaustion preserves old checkpoints and invokes admission/retention policy.
- `IT-DIST-007`: live repartition has one owner before/after, no event/state loss or duplication and expected digest.
- Phase 7 must rerun `CT-005` and `CT-008` under real quorum/lease authority before production promotion is enabled.

## Rollout, compatibility and rollback

Use `replicated_durability` per brain with explicit checkpoint/WAL schema writers and backward-compatible readers for the stated window. Never disable the last known-good restore source during rollout. A persisted-state migration requires a verified pre-migration checkpoint and tested downgrade path; after emitting a non-downgradable format, rollback is restore/migrate, not binary-only downgrade.

## Risks and mitigations

- A local snapshot can omit in-flight causal state. Use a consistent-cut protocol and test delayed channels.
- Synchronous replication can stall causal work. Admit based on policy, reserve resources and expose degraded/paused state; do not acknowledge undurable commits.
- Checkpoint retention can delete the only compatible recovery point. Pin manifests required by replicas/exports/upgrades and enforce reference-aware deletion.
- Fast-forward can duplicate outputs. Separate authoritative neural replay from external presentation and dedupe committed effects.
- Test-term fencing can be mistaken for quorum safety. Keep production promotion disabled until Phase 7 acceptance.

## Surprises & Discoveries

Filesystem WAL/checkpoint primitives are tested for atomic publication and
immutability, but runtime JSON snapshots omit the complete causal cut and no
warm replica or measured RPO/RTO harness exists.

## Decision Log

- Initial decision: recommended durability is synchronous warm WAL plus periodic immutable checkpoints and optional cold copies. Authority: Section 14.1.
- Initial decision: a consistent cut includes in-transit/progress/dedupe evidence, not only neuron arrays. Authority: Section 14.3.
- Initial decision: recovery quality/discontinuity is explicit and high-risk output defaults to suppression. Authority: Sections 15.4–15.5 and `INV-016`.

## Outcomes & Retrospective

Reference WAL/checkpoint immutability and fencing tests pass. Store guarantees,
distributed recovery/chaos evidence, RPO/RTO and migration timings remain open
and are prerequisites for Phase 7 authority.
