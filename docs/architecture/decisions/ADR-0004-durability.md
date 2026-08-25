# ADR-0004: Active, warm, and immutable checkpoint durability

Status: Accepted for the migration design; not implemented in Phase 0.

## Context

Current `src/runtime.rs` persistence is local JSON workspace state. It does not establish immutable brain-level checkpoints, a causal write-ahead log, or distributed recovery point/time guarantees.

## Decision

Later phases will separate active shard state, warm replicas, durable causal logs, and immutable content-addressed checkpoints. Publication creates a new immutable checkpoint identity; recovery replays only from compatible schema/configuration generations and never overwrites a published checkpoint.

## Consequences

This supports INV-006, INV-007, and INV-012 and requires explicit schema versions, checksums, retention, corruption handling, and rollback boundaries. Existing JSON snapshots remain a compatibility format until Phase 6 migration acceptance.

Authority: specification sections 7, 14, 15, and INV-006–INV-007, INV-012.
