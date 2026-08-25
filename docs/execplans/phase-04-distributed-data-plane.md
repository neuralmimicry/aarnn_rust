# Build the reliable causal distributed data plane

This ExecPlan is a living document maintained under `.agent/PLANS.md`. It implements Phase 4 of `docs/specifications/distributed-whole-brain-emulator-v1.1.md` and connects topology-owned virtual shards across processes.

## Purpose and observable outcome

Make remote execution semantically equivalent to the deterministic local reference. At completion, causal envelopes survive reordering, duplication, loss, reconnect and transport changes; receivers know when no earlier event can arrive through explicit progress evidence; distributed zero-delay components close exactly once; and unrelated components/brains continue without a whole-network timestamp barrier.

## Specification authority and traceability

- Primary sections: 3.2–3.4, 6, 7.1, 8.1, 10, 17.1–17.2, 19, 20.5, 21.3, 21.5 and 21.6.
- Invariants: `INV-002`–`INV-007`, `INV-009`, `INV-010`, `INV-012` and `INV-014`.
- Tests: `UT-DEDUPE-001`, `UT-WM-001`, `UT-TERM-001`, `IT-DIST-001`–`005`, `IT-DIST-009`, `IT-DIST-010` and `VT-CAUSAL-001`–`006`.
- Phase gate: identical deterministic-reference digest and event sequence under transport reordering, duplication, loss, timeout, reconnect and gRPC/burst switching, with bounded resources and no false closure.

## Prerequisites and phase boundary

Phases 1–3 must be green. The active topology generation defines owners, routes and component participants. This phase supplies reliable data-plane semantics but not replicated control-plane authority or production failover: a controlled test authority issues terms/tokens until Phases 6–7 provide durable replicas and quorum fencing.

## Scope

- Define a versioned transport-independent `CausalEnvelope` containing brain, topology generation, ownership/lease term, source/destination shard, event ID, logical tag, stream sequence, stage/payload version, trace and quality provenance.
- Implement ordered logical streams, durable/reconstructible send state, acknowledgements, duplicate suppression, gap repair, retransmission and reconnect/resume.
- Implement monotonic route watermarks/safe horizons and reject late events after accepted closure evidence.
- Implement distributed component-tag termination using membership, activity epochs, passivity, durable sent/received balance, report invalidation and reconstructible coordination.
- Implement bounded batching, credits and backpressure with reserved control/ack/progress capacity.
- Place gRPC, MPI/burst and in-memory transports behind one semantic interface; transport changes resume sequences rather than opening a new logical stream.
- Replace new-path layer-wide `SpikeBatch` broadcasts with route-targeted authoritative events.
- Preserve local and distributed deterministic comparisons and causal observability.

## Non-goals

- Do not implement active/warm replica recovery, live migration, quorum membership or end-user management.
- Do not infer completion from timeout, silence, TCP connection state or empty queues.
- Do not promise exactly-once network delivery; provide at-least-once transfer with exactly-once authoritative application.
- Do not add a global timestamp barrier or block positive-delay feedback on the current tick.

## Repository orientation

Locate canonical transport implementations in `transport.rs`, event packing in `transmission.rs`/`aer.rs`, gRPC/protobuf definitions, MPI/burst adapters, `bridge.rs`, `distributed.rs`, queue ownership and all timeout fallbacks. Capture the current 120 ms burst-timeout path and whether it discards, restarts or duplicates logical-stream state.

The intended split is `protocol/envelope`, `transport/session`, `transport/stream`, `transport/credits`, `progress/watermark`, `settlement/distributed`, `routing/dispatch` and generated wire types. Network adapters carry bytes/frames; they do not decide biological eligibility, closure or ownership.

## Architecture and safety constraints

Wall-clock arrival never assigns logical time. Events are admitted only against their envelope tag, active generation and valid authority. A stream sequence is scoped by brain, generation, source/destination route and stream incarnation. Duplicate application state survives restart where required for recovery compatibility.

A watermark is a promise about a route horizon and solves the receiver's “no earlier event is expected” problem, but route watermarks alone cannot prove closure of a cyclic component. Distributed termination additionally requires a stable component/tag epoch, every participant passive after its latest activity epoch, durable sent/received balance zero and no invalidated report. A D→A message emitted after D's earlier passive report invalidates that report. Coordinator failure reconstructs the epoch; it never creates a second closure decision.

Positive-delay events enter future queues and do not hold the current component tag open. Zero-delay consequences advance the microstep. Only participants in an oversized distributed SCC fence its microstep; unrelated components and brains remain work-conserving.

Credits bound data admission. Ack, retransmission, watermark, lease/fence and emergency control traffic have reserved capacity so data saturation cannot deadlock progress. Once an event is causally committed to a send, overload cannot silently drop it. Batching changes frames only, not event identity/order/application.

## Milestones

### Milestone 4.1 — Golden protocol and in-memory fault transport

Define versioned envelope/frame schemas and golden fixtures. Implement an in-memory adapter that deterministically injects reorder, duplication, loss, disconnect, delay and constrained credit. Compare every path with the single-process reference.

### Milestone 4.2 — Reliable logical streams

Implement sequences, durable/reconstructible outbound state, acknowledgements, dedupe, gap requests, retry and resume. Prove restart/reconnect and adapter switching preserve one logical stream and exactly-once application.

### Milestone 4.3 — Watermarks and protocol-violation handling

Emit/accept monotonic safe horizons only after route evidence is durable. A missing watermark causes waiting/progress request, not guessing. A late event behind an accepted horizon quarantines the stream/component and invokes explicit recovery.

### Milestone 4.4 — Distributed component termination

Implement participant activity epochs, passivity reports, durable balance, invalidation and one reconstructible closure decision. Exercise an oversized D→A→D SCC and coordinator failure while unrelated work progresses.

### Milestone 4.5 — Bounded multi-transport data plane

Integrate persistent gRPC and available burst/MPI transport behind the shared interface. Add per-brain/route credits, fair queues, reserved progress traffic and adaptive batching with deterministic canonical event contents.

### Milestone 4.6 — Layer-broadcast migration and distributed gate

Route only events implied by the ownership/route plan. Run local vs seven-process fixtures, transport fallback and full fault matrix. Keep legacy layer broadcasts behind `causal_transport` rollback until all new-path streams drain.

## Progress

- [x] `2026-08-23 12:00Z` Audited the existing `SpikeBatch` envelope, layer
  assignment, timeout/fallback, batching and receive-buffer paths; they remain
  legacy production compatibility paths.
- [x] `2026-08-23 12:00Z` Implemented versioned causal envelopes, bounded
  reliable streams, resume/dedupe/credits, watermark validation and explicit
  component-termination reference semantics in `src/data_plane.rs` and
  `src/causal_transport.rs`; the phase gate tests passed.
- [x] `2026-08-23 12:00Z` Added additive protobuf generation for the causal
  validation service and verified generated tonic/prost consumers through the
  all-feature build.
- [!] `2026-08-23 12:00Z` The generated causal service only validates/echoes
  frames; it does not apply events to shard state or publish durable receipts.
  No reorder/duplication/reconnect multi-process cutover evidence exists, so
  `causal_transport` remains disabled.

## Validation and acceptance

- `UT-DEDUPE-001`: duplicates apply once and acknowledge consistently across stream restart.
- `UT-WM-001`: regressing/contradictory watermarks are rejected with evidence.
- `UT-TERM-001`: active participants, unreceived sends and invalidated passivity each prevent closure; one decision exists per epoch.
- `VT-CAUSAL-001`–`006`: correct zero/positive-delay tags, cyclic closure, slow-shard independence, missing-progress waiting and late-event quarantine.
- `IT-DIST-001`–`005`: seven-node distribution, transport switching, reorder/duplication, bounded backpressure and participant-scoped oversized-SCC fencing pass.
- `IT-DIST-009`: coordinator failure reconstructs one epoch without early/duplicate closure.
- `IT-DIST-010`: advanced route horizons cannot hide an in-flight cyclic event.
- Section 21.5 deterministic fixtures produce identical hierarchical digests/event output under packet batching, jitter and supported transport placement.

## Rollout, compatibility and rollback

Use `causal_transport` per brain and record protocol/stream versions. Mixed mode may use an explicit bridge only at a generation boundary; never feed one authoritative object through both legacy broadcast and new route paths. Rollback requires quiescing/draining new streams and selecting a compatible topology/checkpoint boundary. Protocol readers tolerate the documented rolling-upgrade window; writers emit one configured version.

## Risks and mitigations

- Treating a watermark as cyclic termination can close early. Keep route progress and component termination as separate typed evidence.
- Backpressure can starve the acknowledgements needed to release it. Reserve control capacity and test saturation.
- Transport fallback can reset sequences. Bind sequences to logical streams, not connections.
- Dedupe eviction can reapply old events. Tie retention to checkpoint/replay horizons and fail safely when evidence is unavailable.
- A distributed SCC can become a hidden global barrier. Scope membership to the topology component/generation/tag and measure affected participants.

## Surprises & Discoveries

The additive causal service is a generated validation/echo seam only. Existing
`SpikeBatch` peer streams still carry production traffic, and causal receipts
are not restart-durable; this prevents cutover claims.

## Decision Log

- Initial decision: watermarks express per-route safe horizons; component termination proves cyclic closure. They are complementary, not interchangeable. Authority: Sections 6.3–6.4.
- Initial decision: delivery is retried and deduplicated at authoritative application. Authority: Section 10.3.
- Initial decision: transport switching resumes one logical stream. Authority: Sections 10.1–10.2 and `IT-DIST-002`.

## Outcomes & Retrospective

Protocol/reference bounds and deterministic in-memory tests pass. Durable shard
application, multi-process fault measurements and production gRPC cutover remain
open and are handed to Phases 6–7.
