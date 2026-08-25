# ADR-0002: Explicit event-stage ownership

Status: Accepted for the migration design; not implemented in Phase 0.

## Context

`Runner::step` currently combines input delivery, accumulation, neuron updates, output generation, learning, and time advancement. Distributed layer batches do not identify an authoritative synaptic transition owner.

## Decision

The reference executor will expose explicit prepare, deliver, accumulate, update, emit, and commit stages. Each synapse/terminal/release/plasticity state has one owner in a topology generation, and routes carry typed causal events to that owner. Optimised kernels may implement the stages internally only after reference tests establish equivalence.

## Consequences

This preserves INV-007, INV-008, and INV-014 and makes replay/fencing testable. The compatibility facade may continue to call the old runner until the new executor gate passes, but it must not be presented as equivalent.

Authority: specification sections 3.3, 5, 8, 9, and INV-007–INV-008, INV-014.
