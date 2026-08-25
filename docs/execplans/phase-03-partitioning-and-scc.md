# Build topology-aware virtual partitioning and SCC ownership

This ExecPlan is a living document maintained under `.agent/PLANS.md`. It implements Phase 3 of `docs/specifications/distributed-whole-brain-emulator-v1.1.md` without yet moving authoritative events between processes.

## Purpose and observable outcome

Replace layer-group allocation with a stable, topology-aware virtual-shard plan. At completion, every neuron, synapse, stateful model object and route has exactly one authoritative owner; conservative zero-delay strongly connected components (SCCs) are identified; growth produces atomic topology generations; and the same plan can be exercised by the Phase 2 local executor. The seven-node, three-layer fallback in which an anchor executes every layer is no longer a valid new-path plan.

## Specification authority and traceability

- Primary sections: 3.1, 5, 9.2, 11.2, 13, 19, 20.4, 21.2, 21.3 and Appendix A.
- Invariants: `INV-001`, `INV-002`, `INV-005`, `INV-010`, `INV-011` and `INV-015`.
- Tests: `UT-ID-001`, `UT-SCC-001`, `UT-PART-001`, `VT-CAUSAL-002`, `IT-DIST-001` in local-plan form and topology/ownership property tests.
- Phase gate: complete and deterministic ownership/routing, with every zero-delay cycle co-located or explicitly classified for the Phase 4 distributed-component protocol.

## Prerequisites and phase boundary

Phases 1 and 2 must be green. Phase 2 supplies stable event execution and a conservative component fallback; this phase supplies the actual graph analysis and virtual-shard plans. It creates and validates repartition plans but does not perform cross-process cutover. Phase 4 implements distributed event transport and oversized distributed SCC settlement; Phase 6 implements live migration of durable state.

## Scope

- Define an immutable, versioned topology snapshot and monotonically advancing `TopologyGeneration`.
- Construct the zero-delay graph from declared minimum model delays, treating unknown, dynamic or potentially zero lower bounds conservatively.
- Compute SCCs deterministically and contract them to a component DAG.
- Define weighted costs for neuron/synapse work, event rate, state bytes, route bytes, convergence depth, amplification and device suitability.
- Partition components into many stable virtual shards independent of physical node count.
- Emit complete ownership maps, route tables, component membership and placement constraints using stable IDs.
- Validate plans for uniqueness, referential completeness, capacity and zero-delay-cycle policy before activation.
- Express plasticity, morphology and growth as atomic topology transactions effective at safe logical tags.
- Produce online repartition proposals and state-transfer manifests without activating them.

## Non-goals

- Do not implement reliable network delivery, distributed termination, warm replicas or consensus.
- Do not split a zero-delay SCC merely to improve nominal balance.
- Do not let the optimiser alter biological delay, numerical profile or settling policy.
- Do not cut over a live plan or delete the existing layer path in this phase.

## Repository orientation

Locate the canonical `topology.rs`, `distributed.rs`, `network.rs`, morphology/growth code, serialised model schema and placement configuration. Record all uses of layer indices as ownership or routing identity, all dense-index persistence and any replication/redundancy flag that currently causes overlapping execution.

The intended modules are `topology/graph`, `topology/scc`, `topology/generation`, `partition/cost`, `partition/plan`, `partition/validate`, `placement/constraints` and `growth/transaction`. Biological modules propose stable-ID mutations; topology owns validation/generation; the scheduler later maps virtual shards to resources.

## Architecture and safety constraints

Stable biological identity is independent of dense storage location, virtual shard, replica and compute node. A partition plan may change placement but never identity. Each stateful object has one authority in a generation; redundancy is a replica role, not permission to compute and commit the same transition twice.

An edge belongs in the zero-delay graph if its minimum effective delay can be zero under any admitted configuration. Unknown lower bounds are zero until proven otherwise. Co-locate each SCC where feasible. If an SCC exceeds a resource boundary, mark it `DistributedComponentRequired` with explicit participants; Phase 4 must fence only those participants during same-tag settlement.

A topology transaction is validated against one base generation, reserves capacity, defines an effective safe tag and publishes one immutable next generation. Old-generation events drain or translate only by an explicit versioned rule. In-place partial mutation is forbidden. Growth admission fails or queues before exceeding RAM/VRAM/route/checkpoint budgets.

Cost estimates guide placement but do not affect authoritative results. Canonical ordering and tie-breaks make equal-input partition output reproducible. Security/tenant boundaries and failure-domain constraints are hard constraints, not soft costs.

## Milestones

### Milestone 3.1 — Versioned topology model

Introduce stable ownership records, immutable topology snapshots, generation checks and validation reports. Adapt the Phase 2 local executor to resolve all state/events through the active plan while keeping the legacy layer mapping behind its flag.

### Milestone 3.2 — Trusted zero-delay SCC pipeline

Build the conservative zero-delay graph, deterministic SCC algorithm and component DAG. Compare generated and adversarial graphs with a trusted test oracle, including self-loops, disconnected regions and delay lower bounds that change through configuration.

### Milestone 3.3 — Weighted virtual-shard planner

Implement deterministic cost aggregation and constrained partitioning over components. Produce many virtual shards for the small three-layer fixture and map them to a simulated seven-node inventory without an all-layer anchor or duplicate authority.

### Milestone 3.4 — Ownership and route compiler

Compile neuron, synapse, field, component and event-route ownership. Reject missing, duplicate or cross-tenant ownership, unhandled routes, stale generations and an unclassified split SCC. Property-test arbitrary valid graphs and plan serialisation.

### Milestone 3.5 — Growth and repartition transactions

Convert growth/morphology changes into proposed topology transactions with stable IDs, capacity reservation, SCC/route recomputation and a safe effective tag. Generate a deterministic state-transfer manifest and rollback-safe proposal; activation remains disabled.

### Milestone 3.6 — Local phase gate

Run the local superdense executor through topology generation changes. Prove one owner before/after, explicit old-event handling and reference digests. Publish partition metrics and hand the route/participant contracts to Phase 4.

## Progress

- [x] `2026-08-23 12:00Z` Audited layer-index assignment, dense-ID persistence,
  growth/morphology mutation and direct Runner consumers; remaining layer-range
  paths are recorded as compatibility paths.
- [x] `2026-08-23 12:00Z` Implemented immutable topology generations, stable
  ownership validation, conservative zero-delay SCC/component planning,
  deterministic weighted shard planning and route compilation in
  `src/topology_model.rs`.
- [x] `2026-08-23 12:00Z` Implemented generation-boundary proposals and local
  execution-plan validation; `phase2_to_phase8_gate` passed the SCC,
  ownership, route and planner cases.
- [!] `2026-08-23 12:00Z` Production biological state is still held by Runner
  vectors and distributed layer assignments. The `virtual_partitioning` gate
  remains blocked until every authoritative terminal/synapse/plasticity trace
  is shard-owned and integrated with causal apply/commit and recovery.

## Validation and acceptance

- `UT-SCC-001`: results match a trusted SCC oracle for generated graphs, self-loops and dynamic zero lower bounds.
- `UT-PART-001`: every biological/state object has exactly one authority and every required route exists.
- `UT-ID-001`: compaction, plan serialisation and proposed migration preserve stable event targets.
- `VT-CAUSAL-002`: a local D→A→D zero-delay loop is contained by one SCC and matches the reference digest.
- The local form of `IT-DIST-001` produces useful virtual-shard work for a seven-node inventory without an all-layer anchor.
- Property tests shrink and preserve the minimal graph for duplicate ownership, missing route, stale generation and illegal split-SCC failures.
- A growth transaction either publishes one complete generation at its declared tag or leaves the previous generation wholly authoritative.

## Rollout, compatibility and rollback

Use `virtual_partitioning` alongside the Phase 2 flag. Persist the plan schema/version and topology generation in diagnostics, but do not make an inactive plan authoritative across processes. Rollback selects the prior immutable plan before activation; it cannot reinterpret events already committed under a later generation. Preserve a versioned adapter for legacy layer definitions until Phase 8.

## Risks and mitigations

- Misclassifying a dynamic delay as positive permits false independent progress. Use conservative lower bounds and validation at configuration admission.
- Over-co-location can reduce parallelism. Preserve correctness, expose the cost, and allow Phase 4's explicit distributed-component mechanism for oversized SCCs.
- Cost weights can become hidden biological policy. Keep them scheduling-only, version inputs and explain output.
- Partial growth can orphan events. Publish one generation atomically and test old-generation draining/translation.
- Dense indices can leak into persistence/protocols. Make stable-ID conversion explicit at every boundary.

## Surprises & Discoveries

The topology model validates stable IDs and generation boundaries, while the
legacy distributed implementation still assigns layer ranges and uses dense
Runner indices. Growth/morphology ownership therefore remains to be migrated.

## Decision Log

- Initial decision: potentially zero-delay edges are zero-delay for SCC construction. Authority: Section 5.2 and `INV-010`.
- Initial decision: co-location is preferred; only an oversized SCC may request Phase 4 distributed component settlement. Authority: Sections 5.2 and 6.4.
- Initial decision: partition cost can change placement but cannot change model fidelity. Authority: Sections 5.3 and 11.

## Outcomes & Retrospective

Reference graph/ownership/planner tests pass. Production ownership counts,
partition evidence and migration of Runner/layer state are not complete; the
remaining contracts are handed to Phase 4 and the shard-state blocker.
