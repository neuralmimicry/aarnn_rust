# ADR-0001: Superdense logical time

Status: Accepted for the migration design; not implemented in Phase 0.

## Context

The current runner advances a monolithic step and distributed messages carry a step index. This cannot distinguish same-time causal work from positive-delay work.

## Decision

The new reference execution model will represent biological time as an explicit typed tag `(t, μ)`, where `t` is the configured integer base-quantum and `μ` is the microstep. Zero-delay causal output advances `μ`; positive delay advances `t` and resets `μ` to zero. Packet arrival and wall-clock time are never biological time.

## Consequences

This directly supports INV-002–INV-004 and deterministic replay, but requires an additive protocol/schema boundary and explicit compatibility translation from current step/vector messages. Phase 1 owns the canonical type; Phase 2 owns execution semantics.

Authority: specification sections 3.2–3.4, 6, 7, and INV-002–INV-004.
