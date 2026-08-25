# ADR-0003: Numerical profiles and deterministic reference mode

Status: Accepted for the migration design; not implemented in Phase 0.

## Context

The current biological kernels use floating-point arrays and optional parallel/GPU paths. Thread and device execution can therefore vary in rounding and ordering.

## Decision

The project will define named numerical profiles, including a deterministic reference profile with canonical event order, fixed-point accumulation where specified, deterministic RNG streams, and a state digest. Fast CPU/GPU profiles remain valid execution choices only with a declared error envelope and explicit validation against the reference profile.

## Consequences

Exact digest equality will mean reproducibility of the declared reference profile, not biological adequacy. Existing floating-point behaviour remains the Phase 0 baseline and is not silently reinterpreted. Numerical profiles and configuration digests become part of later persisted/protocol metadata.

Authority: specification section 8, Section 21.5, and INV-008.
