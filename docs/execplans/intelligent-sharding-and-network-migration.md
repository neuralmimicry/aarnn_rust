# Deliver intelligent sharding, bounded placement, and whole-brain relocation

This ExecPlan is a living document maintained under `.agent/PLANS.md`. It is a
cross-phase implementation plan for the existing Phase 3–8 gates; it does not
replace their ordering or enable any migration feature early.

## Purpose and observable outcome

Give each brain two independently controlled capabilities:

1. a deterministic partition planner that chooses stable virtual shards and
   places them on eligible, authenticated resources; and
2. a fenced migration controller that can move, consolidate, evacuate or
   repartition a live brain through an operation visible to the orchestrator
   and CLI.

The system shall use available capacity when doing so improves useful causal
throughput, but shall refuse placement that would exhaust memory, violate
causal ownership, reduce configured durability, split an unsafe zero-delay
component, starve control traffic or change biological fidelity. A laptop can
therefore start a standalone brain, expand onto enrolled network nodes,
consolidate to one host before disconnection, hand authority to the network
while it shuts down, and later reclaim placement through an authenticated
reverse migration.

## Specification authority and traceability

Primary authority is `docs/specifications/distributed-whole-brain-emulator-v1.1.md`:

- Sections 5.1–5.4: virtual shards, SCC placement, weighted cost and immutable
  placement generations.
- Sections 10.1–10.5: causal delivery, ownership, acknowledgement, batching and
  backpressure.
- Sections 11.1–11.7: effective capacity, observations, explainable decisions,
  fairness, device selection and the prohibition on autonomous fidelity loss.
- Sections 13.2–13.4: atomic topology and growth transactions.
- Sections 14.1–14.6: WAL, immutable checkpoints and consistent brain export.
- Sections 15.1–15.9: leases, fencing, recovery, split-brain prevention, live
  migration and replica anti-affinity.
- Sections 16.1–16.9 and 17.3–17.8: authorised management operations, operation
  resources, protocol evolution and storage contracts.
- Sections 18.1–18.5, 20.4–20.8 and 21.5–21.10: telemetry, staged delivery and
  acceptance evidence.
- Sections 12.1–12.4 and the repository mobile requirements: independent brain
  domains, federation isolation, mobile lifecycle truthfulness and separate
  discovery, enrolment, compute and peripheral grants.

The work must preserve `INV-001`, `INV-002`, `INV-003`, `INV-004`, `INV-005`,
`INV-006`, `INV-007`, `INV-008`, `INV-009`, `INV-010`, `INV-011`, `INV-012`,
`INV-013`, `INV-014`, `INV-015`, `INV-016` and `INV-017`. In particular,
placement is not authority, a route watermark is not cyclic termination proof,
capture provenance must survive a path change, and a discovered node is not an
enrolled compute target.

Mobile delivery also carries `APP-INV-001` through `APP-INV-006`: standalone
mobile execution has host-equivalent semantics, discovery grants nothing,
permissions remain separate and revocable, transport paths do not alter
provenance or identity, federation does not merge brains, and opting out does
not corrupt local state or fabricate closure.

Mapped phase plans are:

- `phase-03-partitioning-and-scc.md`: stable shard identities, topology
  generations, SCC atoms and state-transfer manifests.
- `phase-04-distributed-data-plane.md`: reliable causal streams, credits,
  deduplication and distributed component closure.
- `phase-05-multi-brain-scheduler.md`: effective capacity, fair admission,
  observations, prediction and explainable placement decisions.
- `phase-06-durability-and-recovery.md`: shard-owned WAL/checkpoints, consistent
  cuts, warm replicas, failover and live migration.
- `phase-07-management-plane.md`: quorum authority, operations, fencing,
  authorisation and client contracts.
- `phase-08-workstation-io.md` and `mobile-cross-platform.md`: governed
  peripheral/AER continuity, workstation and mobile lifecycle adapters,
  product capability reporting and cross-product acceptance.

## Prerequisites and phase boundary

The current repository is a reference and compatibility implementation. The
following are useful seams but do not yet constitute production sharding or
migration:

- `src/distributed.rs` performs resource scoring and autonomous deployment
  transitions, but assigns layer ranges and may retain a full-network anchor.
- `src/deployment.rs` exposes `desired_shards`, live/autonomous transition
  policy, scope and infrastructure hints, but these settings do not yet drive a
  shard-owned biological executor.
- `src/topology_model.rs` provides the opt-in ownership/SCC plan boundary.
- `src/authoritative_shard.rs`, `src/data_plane.rs` and
  `src/causal_transport.rs` provide reference ownership and transport seams;
  the generated causal service still validates/echoes rather than applying the
  live runner state.
- `src/cluster_snapshot.rs` and `src/consistent_cut.rs` provide bounded
  reference snapshot/cut contracts. They do not replace quorum authority,
  asynchronous GVT integrated with live shard state, or cross-host recovery.
- `src/managed_durability.rs`, `src/recovery.rs` and `src/management.rs` contain
  local/reference durability and fencing paths; the production gate remains
  blocked as recorded in `docs/production-blocker-runbook.md`.
- `proto/management.proto` now exposes proposal-only `PlanPlacement` and the
  evidence-bearing `ApplyPlacement` reference RPC in addition to the existing
  lifecycle operations. The remaining brain-wide migration operation
  resources are still pending the mapped Phase 6–7 work.

The existing legacy layer/vector, `SpikeBatch`, JSON workspace and direct worker
paths remain rollback paths until all mapped gates pass. No phase in this plan
may make a compatibility projection authoritative by itself.

## Scope

- Maintain many stable virtual shards independently of physical node count.
- Partition biological graph components using weighted cost, SCC constraints,
  tenant/security boundaries, device eligibility and failure-domain policy.
- Discover resources, authenticate/enrol eligible nodes, benchmark them and
  publish bounded capability/health observations.
- Plan automatic scale-out, scale-in, co-location and physical relocation with
  hysteresis, minimum residence, benefit thresholds and migration budgets.
- Support explicit operations for placement, shard-count changes, consolidation,
  node evacuation, origin detachment and reclaim-to-node migration.
- Transfer complete shard state, queued events, dedupe windows, route progress,
  in-flight channel state, WAL position, checkpoint metadata and effect cursors.
- Make every decision, plan, cutover, abort, gap and discontinuity auditable,
  replayable and visible through the operation resource.
- Provide deterministic host/fault-injection scenarios before heterogeneous
  hardware and mobile product lanes.

## Non-goals

- Do not treat layer count as shard count or use duplicate layer views as
  replicated authority.
- Do not merge virtual shard identities merely because all shards are being
  co-located for an export. A true shard-count reduction is a separate topology
  generation transaction.
- Do not infer trust, compute admission, brain access or federation membership
  from discovery.
- Do not lower fidelity, alter biological delay, change numerical profile or
  advance biological time to make a migration easier.
- Do not use wall-clock timeout, silence, transport closure or a route watermark
  as proof that an SCC is complete.
- Do not allow an origin laptop to resume an old writer after authority has
  moved to the network. Reclaim means a new fenced placement generation or an
  explicit restore of a chosen immutable checkpoint.
- Do not accept new external input, arm an actuator or emit a committed effect
  merely because a migration is in progress. Peripheral sessions must have an
  explicit drain, buffer, gap or reconnect policy, and actuator leases must be
  fenced across cutover.
- Do not treat a topology-growth proposal, shard-count change, migration and
  checkpoint export as independent concurrent mutations. The orchestrator must
  serialise or explicitly compose them against the same resource/topology
  versions.

## Repository orientation

Verified on 2026-09-05 from `/home/pbisaacs/Developer/neuralmimicry/aarnn_rust`:

- `cargo metadata --no-deps --format-version 1` reports the root
  `aarnn_rust`, the path package `aarnn-biox6-exporter`, and `tools/xtask`; the
  illustrative `crates/` workspace has not yet been extracted.
- The active control loop is in `src/distributed.rs` (`rebalance_networks`,
  autonomous transition planning, resource scoring and layer assignment).
- Deployment policy is in `src/deployment.rs`; CLI wiring is in `src/main.rs`.
- Current topology, transport, durability, recovery, management and cut
  reference seams are in `src/topology_model.rs`, `src/data_plane.rs`,
  `src/causal_transport.rs`, `src/managed_durability.rs`, `src/recovery.rs`,
  `src/management.rs`, `src/cluster_snapshot.rs` and `src/consistent_cut.rs`.
- Protocol source is `proto/distributed.proto` and `proto/management.proto`;
  generation is performed by `build.rs` during Cargo builds.
- Existing focused evidence is in `tests/phase2_to_phase8_gate.rs`,
  `tests/causal_grpc.rs`, `tests/failover_rejoin.rs`,
  `tests/management_grpc.rs` and the module tests. These tests establish
  reference contracts, not production promotion.
- The working tree already contains unrelated and in-progress user changes
  across Rust, protocol, UI, Android, QA and plan files. This plan adds no
  reset, checkout, cleanup or broad reformat operation.

The first implementation session shall record exact canonical paths for the
extracted shard owner, placement registry, generated management clients and
node-enrolment adapter after the workspace split is measured with Cargo
metadata. No duplicate module shall be created to match an illustrative name.

## Architecture and safety constraints

### Separate the three decisions

The planner must keep these decisions distinct:

1. `PartitionPlan`: which stable virtual shards own which biological objects
   in a topology/partition generation.
2. `PlacementPlan`: where those shards and their active/warm roles run now.
3. `MigrationOperation`: how one placement or partition plan becomes the next
   plan, including data transfer and authority cutover.

Co-locating 32 virtual shards on one laptop is a placement change. Reducing
32 shards to 4 is a topology/partition transaction. The latter is optional for
single-host export and must never be used as a shortcut around state transfer.

### Hard admission constraints

Reject a candidate before reservation when any of the following is false:

- all objects have exactly one active writer and the active/warm arrangement
  satisfies the configured durability policy;
- the control plane remains available independently of the node being
  evacuated, or the operation is explicitly in a declared single-host/local
  authority profile with its reduced durability and recovery warning;
- every shard fits measured memory, queue, checkpoint and storage budgets with
  control/ack/recovery headroom reserved;
- every zero-delay SCC is co-located or has the approved distributed-component
  protocol and bounded participant set;
- CPU/NUMA/GPU/kernel profile, numerical determinism and target architecture are
  certified for the requested execution profile;
- network, metering, energy, thermal, failure-domain, tenant and peripheral
  policies permit the placement;
- the target has current credentials, compatible schema/generation and a valid
  lease path; discovery alone is insufficient;
- migration bandwidth, concurrent-operation quota and destination checkpoint
  capacity are reserved; and
- a consistent cut can include local state, queues, channel markers, dedupe,
  route progress and committed-effect cursors;
- all admitted peripheral samples retain capture sequence/time, mapping version
  and uncertainty, while all external effects retain stable `EffectId`, output
  commit evidence and actuator fencing; and
- the brain has an independent identity, quota, seed, timeline, audit chain and
  checkpoint lineage so another brain or federation link cannot be absorbed by
  placement.

When a hard constraint cannot be met, the brain remains on its last valid plan
or enters the explicitly configured paused/degraded state. It does not silently
drop committed events, reduce fidelity or declare a migration complete.

### Objective and self-restraint

Candidate scoring is deterministic and explainable. It combines causal compute
time, memory pressure, event/route bytes, SCC depth/amplification, critical-path
wait, device transfer cost, checkpoint/WAL cost, failure-domain value and the
measured cost of moving state. Automatic scale-out or movement requires a
sustained observation window, a material predicted benefit, minimum residence
and hysteresis. Scale-in requires a longer quiet window and a proof that the
destination can absorb the load without violating headroom or durability.

The scheduler may choose co-location, faster certified resources, more virtual
shards, fewer virtual shards, warm-replica work or migration. It may not change
the neural equations, biological delays, numerical profile or fidelity policy.
Growth forecasts are scheduler inputs: the planner may reserve capacity or
propose a repartition, but it may not silently create biological objects or
change the growth policy. A candidate using a different numerical or kernel
profile is rejected unless that profile is explicitly authorised and has its
own parity/tolerance evidence.
Every state-affecting choice records the observation window, predictor/config
versions, constraints, old/new plan digests, safe logical tag, benefit/cost
estimate, authorising policy and rollback condition.

### Authority and migration protocol

Every placement plan contains `BrainId`, `PartitionGeneration`,
`TopologyGeneration`, stable virtual shard IDs, active/warm node and device,
failure-domain labels, capacity reservations, lease term, fencing token,
effective logical tag and plan digest. Events and commands carrying stale
generation or fencing data receive a typed refresh response.

Each plan also records the source plan digest, parent operation, schema/profile
compatibility, reservation expiry and an explicit node lifecycle state. A
placement plan is not complete until the control plane can answer whether a
node is `Healthy`, `Draining`, `Suspect`, `Recovering` or `Quarantined` and why.

The control plane and data plane have separate relocation boundaries. Moving a
brain away from a laptop does not move the orchestrator automatically. The
operation must either retain an independent quorum/control endpoint or first
complete a separately authorised control-plane relocation. A laptop that is
the sole local authority may run in a declared single-host profile, but its
checkpoint/export must say that no second durable copy or quorum-backed
promotion exists.

The live migration protocol is:

1. Authorise and validate the requested target, mode, resource budget and
   operation idempotency key. Serialise the operation against topology growth,
   checkpoint export and other placement mutations.
2. Freeze the candidate plan, stop new topology mutations for its affected
   generation, and reserve destination resources and backup
   capacity without changing the active owner.
3. Establish the input/output policy for the operation: drain or pause new
   admission, preserve replayable peripheral samples and channel provenance,
   keep control traffic live, and prevent new effectful output unless the
   destination has a valid actuator lease.
4. Publish an immutable source checkpoint and begin causal-WAL/state transfer.
5. Stream post-checkpoint records and bounded channel state while the source
   continues under its current lease; apply and verify the destination.
6. Establish a consistent cut and catch the destination to the cut tag. For a
   cyclic component, use component termination evidence rather than route
   silence.
7. Request a short safe-boundary fence, drain remaining events, verify state,
   route, dedupe, WAL and output/effect cursors, then issue the new term and
   placement generation through the authoritative control plane.
8. Atomically redirect routes, transfer or rebind peripheral channels at their
   explicit binding generation, resume the destination and expose the cutover
   record. Retain the old source as a warm/cold recovery copy until policy says
   it may be released.
9. On failure before cutover, abort and release temporary destination state
   idempotently. On failure after cutover, fence the old source and recover from
   the new owner/checkpoint; never run both terms.

Graceful shutdown and unexpected loss are different paths. A graceful drain
must return a signed/authoritative `ShutdownReady` result proving that the node
has no active shard lease, no unacknowledged committed causal send, no
untransferred output commitment, and no local-only input needed by the current
policy. A lost node enters `Suspect` first; only quorum-observed lease expiry
may promote a replacement. A reconnecting node must recover under the current
plan before it can host work again.

### Explicit user workflows

`ConsolidateBrain(target=laptop)` moves all virtual shards to the laptop and
creates a complete single-host cut. The export artifact includes the BrainId,
topology/partition/placement generations, shard lineage, checkpoint/WAL
positions, route/channel state, peripheral limitations and control-plane
authority metadata. It may optionally request a later shard repartition, but a
snapshot does not depend on that reduction.

`EvacuateNode(node=laptop)` transfers every affected shard to eligible network
nodes, verifies remote durability and commits the new authority before the
laptop is allowed to shut down. The laptop becomes an enrolled but inactive
node; it cannot keep serving the old plan. If that laptop also hosts the only
orchestrator or quorum member, the operation first requires an independent
control-plane endpoint and verifies that remote management can fence and
recover the brain after laptop loss.

`ReclaimBrain(target=laptop)` first authenticates and benchmarks the returning
node, then migrates from the current authoritative network placement to the
laptop. It must not load an old laptop snapshot over newer remote state unless
the user explicitly selects a checkpoint restore/branch operation.

`ScaleShards(count=N)` creates a proposed topology/partition generation,
validates stable ownership and SCC treatment, transfers affected shards at a
safe tag and commits one generation. Physical scale-out/in uses the existing
virtual identities where possible.

When a true repartition retires or splits a shard, the committed generation
must contain an immutable lineage/successor map, object ownership map, route
translation rule and old-generation drain/replay policy. Unchanged biological
object IDs remain stable while shard IDs may change by generation. No event may
be guessed into a successor merely because its physical node is nearby.

`PrepareForShutdown(node=laptop)` is the explicit final step for a graceful
move. It performs or verifies `EvacuateNode`/`ConsolidateBrain`, reports the
exact checkpoint and logical tag that make recovery possible, and only then
returns shutdown readiness. It does not power off the device itself.

## Milestones

### Milestone A — Baseline model and policy contract

Define versioned DTOs for resource observations, partition/placement plans,
constraints, scheduling decisions, migration operations, cutover evidence and
typed refusal reasons. Include successor maps, checkpoint lineage, node
lifecycle/shutdown-readiness, peripheral admission/output cursors, path/mapping
epochs and encrypted-transfer metadata. Add policy fields for automatic
movement, minimum and maximum virtual shards, preferred/required nodes,
single-host consolidation,
origin evacuation, durability floor, thermal/energy/network budgets, minimum
residence, hysteresis, migration concurrency, input/output drain policy,
repartition allowance and minimum predicted benefit/confidence. Add golden
serialisation and replay fixtures. Keep all new behaviour disabled.

Before mobile implementation, add `APP-INV-001` through `APP-INV-006` to the
specification traceability table and map each one to its owning Phase 8
scenario, target products and rollback boundary. The plan cannot promote a
mobile migration path while those invariants exist only in repository prose.

Evidence: schema round-trip, unknown-field/version rejection, deterministic
decision digest and permission-denial tests.

### Milestone B — Stable virtual shard inventory

Complete Phase 3 ownership integration so stable biological state, queues,
routes and SCC membership belong to virtual shards rather than layer ranges.
Produce a deterministic plan with more virtual shards than physical nodes for a
small fixture, and validate single-owner and generation transitions. Integrate
growth/plasticity admission so topology changes reserve active/replica memory,
route and checkpoint capacity before publishing a new generation.

Evidence: `UT-ID-001`, `UT-SCC-001`, `UT-PART-001`, `VT-CAUSAL-002`, local
`IT-DIST-001` and a plan digest independent of node ordering.

### Milestone C — Reliable shard application and placement registry

Complete Phase 4 data-plane application: causal envelopes are applied once by
the authoritative shard actor, acknowledgements are durable/reconstructible,
credits reserve control traffic, and route/generation/fencing errors are
recoverable. Add a placement registry that maps stable virtual shards to active
and warm roles without changing biological state.

Evidence: reorder, duplicate, loss, reconnect, transport switch, stale-term,
backpressure and oversized-SCC tests with identical reference digests.

### Milestone D — Resource-aware automatic planner

Complete Phase 5 resource inventory, certification, benchmark and fair
admission integration. Replace the current layer-capacity heuristic with
weighted graph/shard observations and a deterministic candidate evaluator. Add
proposal-only mode first, then a guarded apply mode after placement decisions
are versioned and replayable.

Automatic actions must demonstrate scale-out, co-location and scale-in with
benefit thresholds, no thrashing, no oversubscription, preserved fairness and
unchanged deterministic neural output. Idle resources may perform warm-copy,
checkpoint or migration work only within reserved budgets. Growth forecasts,
resource reservations and the distinction between data-plane movement and
control-plane availability must be visible in each explanation.

Evidence: Section 21.8 scheduler/resource matrix, four-brain isolation and a
seven-node many-virtual-shard fixture.

### Milestone E — Durable live migration and whole-brain cuts

Complete Phase 6 live shard-owned WAL/checkpoint integration, asynchronous
consistent cuts, warm replicas, anti-affinity, recovery and operation crash
replay. Implement the migration state machine and make cutover idempotent.
Cover peripheral sample provenance, actuator leases, effect deduplication,
partial-transfer cleanup, coordinated brain migration groups, independent
control-plane availability and explicit `ShutdownReady` evidence.

Evidence: `CT-006`, `CT-007`, `CT-009`, `CT-010`, `IT-DIST-007`, failover during
pre-cutover and post-cutover, digest equality, no duplicate effects, measured
RPO/RTO and physical failure-domain evidence.

### Milestone F — Orchestrator and CLI control

Extend `proto/management.proto`, generated clients and `src/management.rs` with
operations equivalent to `PlanPlacement`, `ApplyPlacement`, `ScaleShards`,
`MigrateBrain`, `ConsolidateBrain`, `EvacuateNode`, `ReclaimBrain`,
`CancelMigration` and `GetMigrationStatus`. Each operation has request ID,
idempotency key, expected resource version, observed leader term, operation
state, progress, plan/cut digests, refusal code and audit record.

Add CLI commands through the existing command path and `cargo xtask` wrappers;
clients talk to the orchestrator and never directly mutate workers. Add
capabilities for placement planning, migration, consolidation, node evacuation,
checkpoint export and shard-count change, with dangerous operations requiring
explicit authorisation. Provide equivalent forms such as
`brain placement plan`, `brain migrate`, `brain consolidate`, `node evacuate`,
`brain reclaim` and `operation watch`, while keeping final spelling aligned
with the discovered CLI conventions.

Evidence: generated Rust/web/native contract freshness, concurrent-client
optimistic-concurrency tests, audit-chain tests and stale-leader/fencing tests.

### Milestone G — Scenario and product rollout

Add catalogued scenarios and thin QA wrappers for laptop growth, consolidation
before disconnect, origin evacuation, remote continuation, reconnect/reclaim,
scale-in/out, node loss and migration cancellation. Run each first against the
host reference and controlled multi-process lab, then add workstation, web,
iOS and Android capability lanes. Discovery, enrolment, compute lending,
peripheral access and federation remain separate grants. Mobile lanes must
exercise suspension, process termination, path changes and explicit opt-out
without advancing biological time or merging brain identities.

Evidence: result bundles contain plan/partition/topology generations, resource
observations, operation transitions, cut/effect digests, gap/discontinuity
records and an exact reproduction command.

## Progress

- [x] `2026-09-05` Repository and specification review completed. The current
  autonomous controller is a layer-range deployment heuristic; topology,
  causal, durability, cut and management modules are reference seams with
  production gates still blocked.
- [x] `2026-09-05` The design decision was recorded that physical consolidation
  should co-locate stable virtual shards, while virtual shard-count reduction
  is an independent topology transaction.
- [x] `2026-09-05` Cross-check added the missing `INV-015` and mobile
  application invariants, graceful shutdown readiness, abrupt disconnect and
  recovery paths, peripheral/effect provenance, topology successor mappings,
  growth serialisation and operation conflict rules.
- [x] `2026-09-05 08:09Z` Added the proposal-only placement contracts in
  `src/placement.rs`: device and failure-domain identity, control-plane lease
  and fencing context, deterministic preferred-node scoring, duplicate-input
  rejection, plan digest verification, and explicit shard lineage for true
  shard-count changes. Added unit and public integration evidence for these
  constraints; production application remains disabled.
- [x] `2026-09-05` Added the bounded offline `--placement-request-local-json`
  CLI path and `scripts/qa/ansible_placement_smoke.py`. The adapter gathers
  read-only CPU, memory, storage, load, network and thermal observations over
  the existing SwarmHPC Ansible SSH inventory, requires an explicit compute
  grant list, and delegates all placement decisions and digest creation to the
  Rust planner. Added the `run-ansible-placement.sh` wrapper for repeatable
  hardware QA.
- [x] `2026-09-05` Validated the adapter against the current laptop and the
  reachable QC/SM estate: eight shards were distributed across six explicitly
  granted nodes with warm replicas and no durability degradation. `qc05` was
  excluded because SSH reported no route; an enrolled grant was not inferred
  for any merely reachable node. The existing `sm_native_nodes_test.yml` also
  passed on `sm00` and `sm01` with GPU, CUDA, AARNN worker, Slurm and service
  checks all successful.
- [x] `2026-09-05` Closed the checkpoint-capacity gap in the proposal model.
  `ResourceObservation` now carries usable and reserved storage, each
  `ShardDemand` carries checkpoint/WAL bytes, and active/warm reservations
  charge that storage before admission. A focused unit test proves that a
  shard is refused when its immutable checkpoint cannot fit with headroom.
- [x] `2026-09-05` Exercised explicit laptop consolidation through the same
  adapter with `--consolidate-to localhost`: the single shard was accepted
  only under the explicitly selected single-host degraded-durability policy,
  and the result remained `applied: false` pending the later checkpoint and
  fenced migration phases.
- [x] `2026-09-05 08:39Z` Added `src/placement_registry.rs`, a stable-shard
  authority map with atomic apply, resource-version and term fencing,
  idempotent retry receipts, per-shard checkpoint/catch-up cutover evidence,
  repartition lineage checks and crash-safe persistence. Four focused registry
  tests pass, including restart and refusal-without-mutation cases.
- [x] `2026-09-05 08:39Z` Added the generated-contract `ApplyPlacement` RPC,
  optional cutover/repartition JSON DTOs, secured principal checks and remote
  CLI submission. `management_grpc` passes 11 tests. The secured service uses
  `NM_PLACEMENT_REGISTRY_DIR` for per-brain atomic registry files; absent that
  setting, the reference service keeps an in-memory registry for tests.
- [x] `2026-09-05 08:45Z` Added `src/migration_operation.rs`, a bounded durable
  brain-wide migration journal. It serialises one active migration per brain,
  fences submissions and transitions by leader term/resource version, makes
  progress monotonic, requires a complete cut tag before commit, hashes its
  audit chain, and marks in-flight work `RecoveryRequired` after takeover.
  The file-backed adapter uses locking, fsync and atomic replacement.
- [x] `2026-09-05 08:45Z` Added offline CLI submit/transition paths through
  `--migration-submit-local-json` and `--migration-transition-local-json`, plus
  black-box validation that the journal survives separate CLI processes and
  rejects unsafe state changes. This is a control-plane rehearsal; it does not
  claim that the CLI itself transfers biological state.
- [x] `2026-09-05 08:50Z` Added authenticated `SubmitMigration`,
  `AdvanceMigration` and `GetMigration` management RPCs, durable secured
  journal selection through `NM_MIGRATION_JOURNAL_DIR`, remote CLI submit and
  advance paths, and refreshed Rust/browser/Android management contract
  markers and migration client methods. `management_grpc` covers the full
  orchestrator round trip.
- [x] `2026-09-05 08:58Z` Added bounded, digest-verified shard checkpoint
  transfer in `src/migration_transfer.rs`. Transfers accept out-of-order and
  exact duplicate frames, reject conflicting or corrupted frames, reconstruct
  and verify the canonical `ShardState`, promote it under a newer destination
  term, and emit cutover evidence bound to the source placement-plan digest.
  The adapter can now materialise the verified state as a durable
  `AuthoritativeShard` with a warm checkpoint. `tests/migration_transfer.rs`
  covers the transfer, live owner materialisation, migration journal phases,
  and atomic registry owner change together. This is still a reference
  transfer adapter; quorum-backed promotion and multi-process cutover remain
  gated work.
- [x] `2026-09-05` Extended the transfer adapter with replay-provenance-bearing
  WAL records, channel-boundary state, post-checkpoint catch-up batches and
  destination replay. Out-of-order/duplicate catch-up, digest and channel
  parity, and tamper rejection are covered by `migration_transfer`; legacy WAL
  records remain restorable but cannot claim a live catch-up boundary.
- [x] `2026-09-05` Added fenced cancellation through the durable migration
  journal, `CancelMigration`, local and remote CLI paths, and generated browser
  and Android contract methods. Cancellation is bounded by a reason size and
  committed as `Aborting` followed by `Aborted`; committed operations reject
  cancellation. `management_grpc` now covers stale-term, stale-version,
  successful-abort and committed-operation cases, while `placement_cli` covers
  separate-process local cancellation.
- [x] `2026-09-05` Completed the browser gateway side of the migration
  contract. Submit, advance, lookup and cancellation routes now use the same
  authenticated brain-scoped policy and either the configured durable journal
  directory or an explicitly reference-only in-memory journal. Route tests
  cover control/read access separation; the browser client no longer points at
  an unimplemented cancellation endpoint.
- [ ] Integrate these contracts into the Phase 3–8 milestones and generated
  management schemas without bypassing the phase gates.
- [ ] Replace layer-range assignment with shard-owned placement after the Phase
  3 and Phase 4 gates pass.
- [ ] Enable guarded automatic movement only after Phase 5 evidence and enable
  live migration only after Phase 6–7 evidence.

## Validation and acceptance

The plan is complete only when all of the following pass:

- A laptop-only standalone run creates, checkpoints and exports one brain with
  one authoritative owner and a stable digest.
- Enrolled nodes join only after authenticated role/capability admission;
  discovery observations alone cannot receive shards.
- Sustained pressure moves eligible shards to useful capacity while respecting
  SCC, memory, queue, network, thermal, quota, fairness and durability limits.
- Scale-in and scale-out preserve event identity, logical tags, state digest,
  output/effect deduplication and replay results.
- Consolidation produces a complete immutable single-host cut, including
  queued/in-flight causal state and effect cursors, before remote owners are
  released.
- Origin evacuation commits remote authority before the origin can shut down;
  remote execution continues without the laptop.
- Reclaim migrates from the current authoritative placement and rejects stale
  laptop writers and old placement generations.
- A brain-wide migration group coordinates all affected shard operations and
  exports one manifest, while unrelated brains/components continue without an
  unnecessary global biological barrier.
- Faults before and after cutover yield one owner, recoverable operation state,
  explicit discontinuity/gap evidence and no duplicate committed effects.
- Automatic decisions are explainable/replayable and deterministic neural
  output is unchanged by resource order, packet order or wall-clock speed.
- Growth, export, migration and repartition operations cannot commit conflicting
  generations; a refused plan leaves the previous plan authoritative.
- Evacuating the host that runs the only control-plane authority is refused
  until an independent control endpoint is available, unless the operation is
  explicitly a single-host/local-authority export with its durability warning.
- A graceful origin shutdown produces an authoritative `ShutdownReady` record,
  while abrupt loss produces `Suspect`/lease-expiry/recovery evidence without
  claiming that missing uncommitted work executed.
- Peripheral input retains capture provenance across path migration, and no
  actuator emits an effect under an old lease or duplicate `EffectId`.
- iOS/Android standalone suspension, termination and reconnect preserve host
  semantics, do not advance biological time and do not merge local and remote
  brain identities.

Required scenario IDs to add or extend are `IT-DIST-007`, `CT-009`, `CT-010`,
the Section 21.8 placement/resource cases, Section 21.9 independent-brain
cases, and new stable IDs `IT-PLACEMENT-001` through `IT-PLACEMENT-011`:

1. automatic growth from one enrolled laptop to a heterogeneous fleet;
2. consolidation to laptop followed by snapshot and network loss;
3. evacuation of laptop with remote continuation;
4. authenticated reconnect and reclaim from the current remote authority;
5. scale-in/out and true repartition at a topology boundary;
6. refusal under SCC, capacity, durability, trust, thermal or migration-budget
   constraints;
7. graceful drain/shutdown readiness versus abrupt node loss and recovery;
8. concurrent growth/export/migration conflict and deterministic serialisation;
9. peripheral input path migration, effect fencing and duplicate-output
   suppression; and
10. mobile suspension, reconnect, opt-out and standalone/remote identity
    separation;
11. control-plane relocation/absence and brain-wide migration-group recovery.

Current reference-contract evidence (2026-09-05):

- `cargo test --locked --lib placement`: 10 passed;
- `cargo test --locked --test placement_migration`: 8 passed;
- `cargo test --locked --test management_grpc`: 13 passed;
- `cargo test --locked --test placement_cli`: 2 passed;
- `cargo test --locked --test migration_transfer`: 1 passed, including
  source framing, destination reconstruction, durable migration phases and
  fenced owner publication;
- `python3 -m py_compile scripts/qa/ansible_placement_smoke.py`;
- `scripts/qa/run-ansible-placement.sh --shards 8`: passed; active nodes were
  `qc00`, `qc02`, `qc03`, `qc04`, `sm00` and `sm01`, with `applied: false`;
- `scripts/qa/run-ansible-placement.sh --hosts localhost --grant-compute localhost --shards 1 --maximum-thermal-pressure 1000 --minimum-warm-replicas 1 --allow-single-host-degraded-durability --consolidate-to localhost`: passed with `degraded_durability: true` and `applied: false`;
- `ansible-playbook -i /home/pbisaacs/Developer/swarmhpc/swarmhpc/ansible/inventory/hosts.ini sm_native_nodes_test.yml --limit sm_amd64`: passed with zero changes;
- `rustfmt --edition 2024 --check src/placement.rs tests/placement_migration.rs`;
- `git diff --check -- src/placement.rs tests/placement_migration.rs`.

These commands validate deterministic proposal construction, the guarded
registry apply boundary and state-machine refusal paths. They do not claim
live multi-process migration, durable biological checkpoint transfer, or
quorum promotion; those remain owned by the mapped Phase 3–7 milestones.

Registry evidence added during the current implementation slice:

- cargo test --locked --test placement_registry: 4 passed;
- bootstrap, duplicate retry, stale leader/version protection, mandatory
  checkpoint cutover evidence, lineage-checked shard-count reduction and
  persisted-registry reopen/fence retention are covered by
  tests/placement_registry.rs;
- --placement-apply-local-json with --placement-registry-json now applies a
  bounded, evidence-bearing request through the atomic local registry. It
  remains a reference rehearsal path until the Phase 7 replicated management
  service owns the registry.
- `cargo test --locked --test management_grpc`: 12 passed after adding the
  SubmitMigration/AdvanceMigration/GetMigration orchestrator round trip;
- `cargo test --locked --lib migration_operation`: 4 passed;
- `cargo test --locked --test placement_cli`: 2 passed, including separate
  process submit/advance migration journal validation;
- `cargo xtask bindings check`: passed after refreshing the protocol digest;
- `cargo test --locked --test migration_transfer`: passed with live owner
  post-checkpoint WAL catch-up and tampered-batch rejection;
- `git diff --check`: passed after the cancellation and generated-contract
  changes;
- `cargo test --locked --bin web_ui`: 15 passed, including migration route
  authorisation checks;
- `cargo test --locked --all-targets --quiet`: 239 library tests and all
  integration/example targets passed after the transfer, cancellation and
  browser migration gateway changes. The current evidence still does not close
  quorum-backed promotion, brain-wide multi-process cutover, route/peripheral
  cursor handoff, or automatic executor adoption.
- `ansible-playbook -i /home/pbisaacs/Developer/swarmhpc/swarmhpc/ansible/inventory/hosts.ini /home/pbisaacs/Developer/swarmhpc/swarmhpc/ansible/sm_native_nodes_test.yml --limit sm_amd64`: passed on `sm00` and `sm01`, `changed=0`;
- `scripts/qa/run-ansible-placement.sh --shards 8`: passed on the final run;
  `localhost`, `qc00`, `qc01`, `qc02`, `qc03`, `qc04`, `sm00` and `sm01` were
  reachable, `qc05` was excluded as unreachable, and only the six explicitly
  granted compute nodes were admitted.

## Progress update — 2026-09-05

- [x] Added the brain-wide `MigrationGroup` barrier to the durable migration
  journal. Grouped requests bind their shard set to the journal-assigned
  operation ID; shard transfer, catch-up, fencing and publication evidence are
  resource-versioned journal updates; grouped commit is rejected until every
  shard is published and the group itself is committed.
- [x] Added fenced group takeover handling. A newer journal leader term is
  recorded in the group audit chain and the previous term is rejected for all
  subsequent shard updates.
- [x] Added route/channel and committed-effect cursor digests to cutover
  evidence. The placement registry rejects owner publication when either
  cursor boundary is absent; transfer evidence derives both from immutable
  shard state and the receipt ledger.
- [x] Added bounded local and remote CLI/management JSON paths for group specs,
  group evidence updates and `GetMigrationStatus`, while preserving legacy
  ungrouped requests.
- [x] Added migration-group takeover, persistence/barrier and management
  integration tests and refreshed Rust, browser and Android schema markers.
- [!] The implementation remains a validated reference control/data movement
  seam. Live runner adoption, network consensus, physical multi-process chaos
  and automatic executor-driven placement remain production gates.

## Final verification — 2026-09-05

- `cargo test --locked --all-targets`: passed with 249 library tests, all
  integration suites, web gateway tests, migration/placement/registry tests,
  failover/rejoin tests, mobile contract tests and example targets passing.
- `cargo test --locked --features replicated_durability --lib`: 259 passed;
  this includes durable runner publication, causal ingress idempotency,
  authoritative projection, migration-group and recovery coverage.
- `cargo check --locked --all-features --all-targets`: passed. The build still
  reports existing unused-code, optional-dependency and feature-profile
  warnings; these do not affect the successful validation result.
- `cargo fmt --all -- --check`, `git diff --check` and `cargo xtask bindings
  check`: passed.
- The Ansible placement smoke, using the existing SwarmHPC inventory, passed
  on the laptop plus reachable `qc00`–`qc04`, `sm00` and `sm01`; only
  explicitly granted compute nodes were admitted, unreachable `qc05` was
  excluded, and eight-shard consolidation to `qc00` passed in proposal-only
  mode. The existing `sm_native_nodes_test.yml` completed with `changed=0`.
- Focused runtime and coordinator tests passed: two stable-shard admission
  tests and three brain-wide parallel transfer/failure/term-consistency tests.

The implementation deliberately keeps production cutover disabled until the
stable-ID biological executor owns arbitrary multi-shard state, quorum-backed
network authority is exercised across physical failure domains, and complete
peripheral handoff plus measured chaos/RPO/RTO evidence is available.

## Rollout, compatibility and rollback

Roll out in proposal-only, shadow, canary and enabled stages. Persist the
previous placement plan, checkpoint and causal-log references before every
enabled cutover. A proposal or pre-cutover operation can be cancelled without
changing authority. After cutover, rollback means a new fenced migration to the
previous plan or restore from an explicitly selected immutable checkpoint; it
does not revive a stale process or rewrite committed history.

Keep `virtual_partitioning`, `causal_transport`, `multi_brain_scheduler`,
`replicated_durability` and `management_v1` as coarse gates until their owning
phase evidence passes. Mixed legacy/new execution is allowed only at an
explicit generation boundary with one authoritative path. Require rolling
upgrade compatibility for plan/checkpoint/WAL schemas, encrypted authenticated
state transfer, downgrade rejection where semantics differ, and a documented
restore path before enabling a new writer or deleting the old recovery copy.

## Risks and mitigations

- **Migration thrash:** hysteresis, minimum residence, benefit thresholds and a
  per-brain/fleet migration budget.
- **Quality dilution disguised as optimisation:** hard fidelity/profile rules,
  certified kernels and visible refusal rather than automatic approximation.
- **Layer-range illusion:** remove full-network anchors from the new path and
  require shard-owned biological state before promotion.
- **Split brain:** quorum terms, fencing at event/WAL/checkpoint/effect gates and
  stale-generation rejection at every receiver.
- **Incomplete exports:** consistent cuts include queues, channels, dedupe and
  effect cursors; compatibility runner snapshots are marked incomplete.
- **Resource starvation:** reserved control/recovery capacity, hierarchical
  fairness and admission before allocation.
- **Untrusted autodetection:** discovery, enrolment, compute lending,
  peripheral access and federation remain separate, revocable grants.
- **Laptop lifecycle loss:** checkpoint-before-suspend where possible, explicit
  lease expiry otherwise, and no biological time advancement during suspension.
- **Input/output discontinuity:** bind each peripheral session to BrainId,
  binding generation, path/mapping epoch and credit state; record gaps and
  suppress effects until a valid destination lease is active.
- **Concurrent topology and migration changes:** use resource-versioned
  operation conflicts and one orchestrated commit order; do not merge two
  independently planned generations.
- **Schema or credential mismatch during transfer:** preflight compatibility,
  encrypt/authenticate the stream, fail closed and retain the source authority
  until verification succeeds.
- **Control-plane loss mistaken for data-plane migration:** model control-plane
  placement and quorum membership separately, require remote fencing evidence,
  and reject shutdown readiness while the laptop is the sole authority.

## Surprises & Discoveries

The current automatic transition code can change `desired_shards` and placement
mode from telemetry, but the resulting distribution is still built from layer
ranges, with a full-network anchor for small networks. This is useful control
plane rehearsal but cannot claim intelligent biological sharding, exactly-once
state movement or migration safety.

The existing cluster snapshot and consistent-cut contracts correctly reject
incomplete/mixed projections, but the live runner is not yet fully represented
as a durable shard state with causal receipts and quorum authority. The plan
therefore treats those modules as prerequisites and does not promote their
current output to a whole-brain migration guarantee.

The first authoritative apply boundary is now present in
src/placement_registry.rs. It deliberately keeps a changed owner fenced
until a migration adapter supplies a source plan digest, source lease term,
verified checkpoint digest, catch-up confirmation and committed cut tag for
each retired or moved shard. This closes the previous gap where a valid plan
could be mistaken for proof that state had already arrived at its destination.

## Decision Log

- **D-ISM-001 — 2026-09-05:** Physical consolidation and virtual repartition are
  separate operations. Co-location preserves stable shard IDs and is sufficient
  for a single-host snapshot; reducing shard count requires a new topology and
  partition generation. Authority: Sections 5.1, 5.4, 13.2 and 15.8.
- **D-ISM-002 — 2026-09-05:** Automatic placement may optimise resource use and
  causal throughput but cannot autonomously lower biological fidelity, alter
  delays or fabricate quiescence. Authority: Sections 7.1–7.4, 11.4–11.5 and
  `INV-006`, `INV-008`, `INV-013`.
- **D-ISM-003 — 2026-09-05:** Reclaim targets the current authoritative remote
  placement. An old laptop snapshot is a restore/branch input, never an
  implicit merge source. Authority: Sections 15.6–15.8 and `INV-001`.
- **D-ISM-004 — 2026-09-05:** Auto-detected resources remain observations until
  authenticated enrolment grants the exact compute role and policy. Authority:
  repository discovery/enrolment requirements and `INV-011`, `INV-017`.
- **D-ISM-005 — 2026-09-05:** Graceful laptop shutdown requires an explicit
  `ShutdownReady` result; unexpected loss follows normal suspect/lease-expiry
  recovery and must not be represented as a completed migration. Authority:
  Sections 15.2–15.5 and `INV-001`, `INV-007`.
- **D-ISM-006 — 2026-09-05:** Migration preserves admitted peripheral
  provenance and effect fencing across path changes. Authority: Section 12.1,
  Sections 15.4–15.5, Sections 16.16–16.19 and `INV-015`–`INV-017`.
- **D-ISM-007 — 2026-09-05:** Growth, export, repartition and migration are
  serialised against explicit generation/resource versions. Authority:
  Sections 13.2–13.4, 14.5 and `INV-009`.
- **D-ISM-008 — 2026-09-05:** Placement authority is published through an
  atomic registry transaction separate from proposal generation. Same-key
  retries return the original receipt; owner changes require per-shard
  cutover evidence; shard-count changes require a verified lineage transaction.
  The local persisted adapter is a reference implementation and does not
  replace quorum-backed Phase 7 authority. Authority: `INV-001`, `INV-009`,
  `INV-011`, `INV-012` and Sections 13–15.

## Progress update — 2026-09-05

- [x] Added `src/managed_shard_runtime.rs`, a reusable stable-shard admission
  boundary. It requires matching brain/shard identity, topology and partition
  generations, lease term, fencing token and authoritative state digest before
  a step can be admitted. Placement generation adoption is atomic across all
  registered shard evidence, and stale or incomplete evidence leaves the
  directory unchanged.
- [x] Connected the gate to `ManagedNetwork`: durable managed owners construct
  a single-owner placement generation from their authoritative checkpoint,
  verify ownership before stepping and record the committed state digest after
  durable publication. The mutable `Runner` remains a compatibility projection
  and cannot bypass the gate when the durable profile is enabled.
- [x] Added `BrainMigrationCoordinator` to
  `src/migration_coordinator.rs`. Shard transfers run concurrently outside
  journal/registry locks; results are applied in stable shard-ID order; mixed
  cut tags or destination terms abort; placement publication is preflighted on
  clones and the brain-wide barrier is finalized only after publication.
- [x] Added focused fault coverage for plan-admission refusal, atomic runtime
  replacement, parallel transfer, failed transfer abort and mixed-term
  rejection. `cargo test --locked --lib managed_shard_runtime` and
  `cargo test --locked --lib brain_tests` pass.
- [x] Host and physical resource validation passed using the pre-existing
  SwarmHPC Ansible inventory. The proposal-only placement smoke admitted only
  explicit grants on `qc00`, `qc02`, `qc03`, `qc04`, `sm00` and `sm01`, excluded
  unreachable `qc05`, and separately consolidated all eight shards to `qc00`.
  The existing `sm_native_nodes_test.yml` passed on `sm00` and `sm01` with
  `changed=0`.
- [!] The full stable-ID biological executor is not yet the authoritative
  implementation for arbitrary multi-shard `Runner` models; the live network
  still uses the compatibility layer assignment path outside the durable
  single-owner gate. Network consensus, physical failure-domain chaos and
  complete peripheral handoff evidence remain required before production
  flags are enabled.

## Outcomes & Retrospective

This plan establishes the cross-phase workflow and the current implementation
boundary. No production sharding, live migration, automatic origin evacuation
or reclaim capability is claimed until the listed phase gates and scenario
evidence pass.

## Progress update — 2026-09-05: deterministic multi-shard execution seam

- [x] Added `StableShardExecutor` as a bounded transport-neutral reference
  fabric. It instantiates stable-ID biological state per virtual shard, checks
  route/target ownership, applies canonical event ordering, propagates
  same-time microsteps and positive delays, and derives stable child event IDs.
- [x] Added bounded admission and deduplication, exact conflicting-replay
  refusal, state digests, and whole-fabric transactional rollback when a
  transition or emitted route cannot be admitted.
- [x] Extended `StableBiologicalState` with generation-wide endpoint identity
  and shard-local neuron/synapse operations. Remote synapse endpoints survive
  serialisation; terminal, weight, release and plasticity field splitting is
  rejected until a field-granular ownership representation exists.
- [x] Added unit and public integration coverage for cross-shard routing,
  logical-time propagation, duplicate delivery, canonical admission, split
  ownership refusal, remote endpoint round-trip and queue overflow rollback.
- [!] The fabric is currently an in-memory/reference executor. It is not yet
  the authoritative `ManagedNetwork`/`Runner` replacement and is not wired
  into durable multi-process shard actors, quorum authority, peripheral cursor
  handoff or migration cutover. Production flags therefore remain disabled.

The next implementation boundary is to embed each stable shard state in the
durable authoritative actor, persist pending/dedupe/route state with the
checkpoint and WAL, and prove generation-boundary transfer against the live
runner on representative biological fixtures before enabling any production
executor-selection path.

## Progress update — 2026-09-05: durable stable-shard cut and transfer

- [x] Added deterministic `StableShardCheckpoint` DTOs carrying biological
  bytes, pending causal work, admitted deduplication entries, committed event
  history, capacities, plan digest, generations and the sibling `fabric_digest`.
  Restore rejects incomplete, tampered, mixed-plan and mixed-cut checkpoint
  sets before execution resumes.
- [x] Added conversion to and restoration from the existing durable
  `ShardState` envelope, preserving lease/generation/checkpoint integrity while
  keeping the stable executor payload versioned and opaque to generic storage.
- [x] Added an end-to-end migration test that exports every virtual shard,
  frames and transfers them out of order through `ShardTransferSource` and
  `ShardTransferReceiver`, promotes them under a newer lease term, and restores
  the complete fabric with an identical deterministic digest.
- [x] Added `StableExecutorCheckpointStore`, which publishes complete sibling
  checkpoint sets through the existing atomic no-replace filesystem store and
  reopens them only after bounded-size, manifest, set-digest and fabric-digest
  verification. Reopen/immutability coverage is in the public stable-shard
  integration suite.
- [!] The stable checkpoint is now migration-transfer compatible, but the live
  `AuthoritativeShard` actor still applies the existing single-shard stable
  event API and `ManagedNetwork` still uses the compatibility `Runner`. WAL
  append/output routing for the multi-shard executor and quorum-backed
  multi-process authority remain the next integration gates.

## Progress update — 2026-09-05: fenced complete-fabric checkpoint boundary

- [x] Added `StableExecutorCheckpointSet`, which binds every shard checkpoint
  to one immutable brain cut, compiled-plan digest, topology/partition
  generations and lease term. The set digest is canonical and tamper evident;
  incomplete, duplicate, mixed-plan and mixed-cut sets are rejected.
- [x] Added `StableExecutorCheckpointStore` on top of the existing atomic
  no-replace filesystem checkpoint primitive. Payload size is bounded before
  publication and verified again after reopen; the restored executor must
  reproduce the published whole-fabric digest.
- [x] Added `StableExecutorAuthority` with term/fencing validation and
  transactional admit/step/checkpoint publication. A duplicate is idempotent;
  execution or publication failure restores the exact prior executor state.
- [x] Added end-to-end tests for immutable reopen, tamper detection,
  out-of-order transfer, new-term promotion and failed-publication rollback.
  Repository-wide all-target tests, replicated-durability tests, all-feature
  checks, binding freshness, formatting and diff checks pass.
- [!] The implementation is deliberately not promoted to the live path. The
  next milestone must connect this boundary to durable shard actors, causal
  output/WAL receipts and generation-boundary migration while preserving
  peripheral provenance, effect fencing and complete-cut evidence. Filesystem
  persistence remains a local reference adapter until network consensus and
  physical multi-host failure evidence exist.

## Progress update — 2026-09-05: durable actor handoff

- [x] Added `StableExecutorDurableBridge`, which publishes the complete
  immutable fabric cut first and then mirrors every resulting stable shard
  checkpoint through `AuthoritativeShard`. Each mirror uses the same causal
  envelope, bounded channel projection, WAL, receipt and warm-replica path.
- [x] Mirror publication is resumable. If one actor fails after the complete
  cut is published, the pending envelope and expected pre-cut actor digests
  remain available for retry; already updated actors are accepted only when
  their biological bytes exactly match the requested checkpoint.
- [x] Added validation that existing actor files cannot silently be reused when
  they disagree with the initial complete cut. The stable executor integration
  suite now proves six cases including all-mirror publication, restart,
  duplicate delivery and digest preservation.
- [!] This is the durable handoff seam, not production distributed commit. The
  coordinator still needs network quorum fencing, crash-replay evidence for a
  partially mirrored cut, complete peripheral/effect cursor integration and
  generation-boundary migration before any live executor-selection flag is
  enabled.

## Verification update — 2026-09-05: resumable mirror failure and transfer adapter

- [x] Added `StableExecutorDurableBridge::prepare_transfer_sources`. It turns
  the verified durable `ShardState` views into bounded `ShardTransferSource`
  objects in stable shard order, with transfer IDs, source plan digest and
  consistent-cut evidence supplied by the caller. It performs no lease or
  placement publication.
- [x] Added an integration fault-injection scenario that makes one warm mirror
  unavailable after the complete fabric cut is published. The bridge retains
  the pending cut, leaves the successful sibling durable, retries the failed
  mirror without executing the neural event again, and reconstructs both
  destination actors under a newer lease term from out-of-order frames.
- [x] Revalidated the migration data-plane and management/CLI reference
  contracts, including the existing fenced migration journal. The transfer
  adapter remains separate from the journal: data movement produces evidence,
  while the orchestrator operation and placement registry authorize and
  publish cutover.
- [!] Network consensus, live stable-ID executor selection, peripheral/effect
  cursor handoff and physical multi-host failover/RPO/RTO evidence remain open.
  Production flags remain disabled.

## Progress update — 2026-09-05: complete reference migration session

- [x] Added `src/brain_migration_session.rs`. It consumes the durable bridge's
  real `ShardTransferSource` values, bounds and verifies every frame, accepts
  them out of order, reconstructs every destination `AuthoritativeShard` under
  a newer term, and derives route/effect cursor evidence from imported state.
- [x] Connected the session to `BrainMigrationCoordinator`, the placement
  registry and `MigrationJournal`. Registry publication remains the placement
  barrier; only after it succeeds does the journal accept the committed group
  and operation progress. A journal write failure leaves the placement
  recoverable as a committed registry state with journal recovery required.
- [x] Added `tests/brain_migration_session.rs`, which runs a complete source
  bridge → reversed-frame transfer → destination reconstruction → registry
  publication → journal commit path for two shards.
- [x] Added read-only operator aliases for proposal and migration workflows
  (`--brain-placement-plan`, `--brain-migrate`, `--brain-consolidate`,
  `--brain-reclaim`, `--node-evacuate`) and `--operation-watch` for bounded
  journal inspection. These aliases retain the existing authenticated and
  proposal-only boundaries.
- [!] The session is still a local/reference adapter. Destination lease
  issuance and actor fencing must be bound to the replicated network authority,
  and peripheral/effect cursor handoff plus physical multi-host chaos evidence
  remain required before production cutover.

## Progress update — 2026-09-05: parallel receive, quorum promotion and persisted recovery

- [x] Transfer reception now uses bounded scoped workers. Each source is
  verified and reassembled concurrently, then inserted into a stable shard-ID
  map so completion order cannot affect evidence or audit order.
- [x] Added an atomic brain-wide lease promotion API to the replicated quorum
  reference authority. It validates every source writer in one decision,
  fences all sources and issues one shared destination term. The session can
  bind each reconstructed destination actor to the returned replicated
  authority and revoke the complete lease set if materialisation fails.
- [x] Added crash-safe session finalisation through
  `PersistedPlacementRegistry` and `PersistedMigrationJournal`, including
  reopen verification after registry publication and journal commit. The
  journal remains the recovery record if the second publication fails.
- [x] Added true nested CLI forms for `brain placement plan`, `brain migrate`,
  `brain consolidate`, `brain reclaim`, `node evacuate` and `operation watch`.
  They dispatch to the same bounded handlers as the compatibility flags and
  select the requested migration kind explicitly.
- [!] The quorum implementation remains a local replicated reference adapter,
  not a network consensus/election implementation. The live orchestrator still
  requires an executor registration/dispatch adapter for real bridge-backed
  migration, physical failure testing, workload identity and scientific parity
  gates remain open.

## Progress update — 2026-09-05: explicit peripheral/effect cursor handoff

- [x] Added the versioned, bounded `PeripheralCursorState` DTO. It records
  per-channel device/mapping epochs, admitted capture sequences and queued
  samples, plus actuator lease terms, armed state and accepted `EffectId`
  deduplication entries. Validation rejects duplicate channels, unsorted or
  oversized cursor windows and payloads above the peripheral admission bound.
- [x] Included the cursor DTO in `ShardCheckpointPayload` and `ShardState`,
  sealed it into the checkpoint digest, and exposed an atomic safe-boundary
  update through `AuthoritativeShard`. Existing checkpoints without the new
  domain remain readable only when their legacy digest verifies; newly sealed
  checkpoints always carry the explicit state.
- [x] Migration promotion preserves queued/admitted peripheral state and
  re-fences actuator cursor terms under the destination lease. Cutover route
  and effect evidence now hashes the explicit cursor domains together with
  causal/channel state instead of relying only on derived projections.
- [x] Added regression coverage for cursor preservation and destination-term
  re-fencing in `migration_transfer`, plus legacy checkpoint compatibility in
  `durability`.
- [!] The management RPC still journals requests without owning a registered
  live `StableExecutorDurableBridge`; network consensus/election, physical
  multi-host RPO/RTO, deployed workload identity and scientific parity gates
  remain production blockers.

## Validation update — 2026-09-05: post-cursor broad verification

- [x] `cargo test --locked --all-targets --quiet`: 263 library tests and all
  integration/example targets passed.
- [x] `cargo check --locked --all-features --all-targets --quiet` passed;
  repository-wide pre-existing warnings remain non-fatal.
- [x] `cargo xtask bindings check`, `cargo fmt --all -- --check` and
  `git diff --check` passed.
- [x] `scripts/qa/run-ansible-placement.sh` passed against the current laptop,
  `qc00`–`qc04`, `sm00` and `sm01`; `qc05` remained unreachable and was
  excluded, while only the six explicitly granted compute nodes were admitted.
- [!] This validates the bounded reference implementation and deployment
  inventory adapter. It does not convert the local quorum files into network
  consensus or provide physical multi-host migration chaos/RPO/RTO evidence.

## Progress update — 2026-09-05: registered live migration executor seam

- [x] Added `src/migration_executor.rs`, a brain-scoped executor registry with
  explicit registration, one in-flight migration lease per brain, bounded
  `spawn_blocking` dispatch, duplicate/unknown executor rejection and
  fail-closed unregister behaviour.
- [x] Added `StableExecutorMigrationExecutor`, which connects the registry to
  the durable stable executor bridge: it validates the operation and target
  plan, prepares digest-verified transfer sources, performs the parallel
  brain-wide migration session with one quorum term, persists placement, and
  fences the source bridge after successful publication.
- [x] Extended both generated management service profiles with optional
  migration dispatch. A submitted grouped migration is dispatched only after
  an executor registration exists; journal finalisation consumes the returned
  committed group evidence and bounded byte/cut checks. Cancellation is
  rejected while the live executor lease is held, and a race with a prior
  cancellation is checked before worker dispatch.
- [x] Added `tests/migration_executor.rs` for management-to-registry dispatch,
  evidence-backed journal commit, registration/release and missing-executor
  behaviour, plus a bridge-backed end-to-end cutover test in
  `tests/brain_migration_session.rs`.
- [!] The production binary now owns an empty registry handler when management
  is enabled, but it does not yet discover or construct a live stable bridge
  from a deployed brain. Registration remains an embedding/orchestrator
  integration point, and production cutover stays disabled until durable
  network consensus, workload identity and physical multi-host evidence pass.

## Verification update — 2026-09-05: placement observability surfaces

- [x] Added a web Placement tab with a bounded canvas diagram of reported
  virtual nodes, host addresses, layer-owned shards and recent activity
  ratios. Workspace mode labels its node/layer arrangement as a runtime
  projection when only sandbox node metadata is available.
- [x] Added the matching native egui Placement tab. It reads the distributed
  registry, node address cache and lock-free activity buffers without issuing
  management commands or changing placement. Activity is presentation-only.
- [x] Added browser compatibility assertions for both surfaces and validated
  `node --check web_ui/app.js`, the six `web_ui_browser_compat` tests,
  formatting and diff checks. `cargo check --locked --features ui
  --all-targets --quiet` completes with warnings only after closing the
  existing non-robot UI portability guards.
- [!] The native and web views remain read-only telemetry projections. They do
  not replace the still-missing deployed stable-bridge registration, network
  consensus, or physical multi-host migration evidence.

## Cross-check update — 2026-09-05: lease-plan handoff validation

- [x] Cross-checking the observability work exposed an existing reference
  migration fixture whose destination placement plan used a term different
  from the next quorum lease. The single-shard cutover now rejects that
  mismatch before it can fence the source, preserving the one-writer safety
  boundary.
- [x] Corrected the fixture to derive its destination plan from the authority's
  next term and reran `cargo test --locked --test migration_transfer --quiet`,
  the web UI compatibility tests, JavaScript syntax validation, formatting and
  diff checks.

## Verification update — 2026-09-05: interactive placement movement telemetry

- [x] Extended `proto/distributed.proto` with explicit backup-layer ownership
  and bounded `ShardPlacementMovement` records. The orchestrator derives
  deterministic active/backup `moving` records from placement changes and
  `considering` records only while autonomous placement review is enabled.
  Progress is advisory and bounded; it is not a lease, cutover receipt or
  authority proof.
- [x] Exposed the new fields through the web status JSON and OpenAPI schema,
  preserving compatibility with older payloads by treating absent backup and
  movement arrays as empty.
- [x] Enhanced the web placement canvas with pan, wheel zoom, Ctrl/Meta-drag
  rotation, hit-tested click selection, Ctrl/Meta multi-selection and
  double-click detail/focus. Selected shard layers are highlighted on the
  dashboard neural graph. Active and backup cards use distinct styling and
  movement badges/arrows.
- [x] Enhanced the native egui placement surface with the same camera gestures,
  multi-selection, detail panel, active/backup cards, movement telemetry and
  selected-layer rings on the neural graph. The surface remains read-only.
- [x] Added deterministic backend tests for active versus backup movement,
  considering versus moving phases, progress bounds, stable ordering and the
  autonomous-review gate. Browser compatibility assertions cover the new
  interaction and highlighting hooks.
- [x] `node --check web_ui/app.js`, `cargo fmt --all -- --check`,
  `git diff --check`, `cargo test --locked --test web_ui_browser_compat --quiet`,
  the focused placement telemetry tests, `cargo test --locked --all-targets
  --quiet`, `cargo check --locked --features ui --all-targets --quiet`,
  `cargo check --locked --all-features --all-targets --quiet`, and
  `cargo xtask bindings check` pass. `scripts/qa/run-ansible-placement.sh`
  passes against localhost, `qc00`–`qc04`, `sm00` and `sm01`; `qc05` remains
  unreachable and is excluded by the existing validation policy.
- [!] Movement telemetry still describes queued placement intent in the legacy
  layer assignment path. It must not be promoted to authoritative transfer
  progress until live stable-shard acknowledgements, replicated authority and
  physical multi-host migration evidence are connected.

## Verification update — 2026-09-05: native shard metadata consistency

- [x] Corrected the native Placement surface so the click status and
  double-click detail card use the computed neuron count for the selected
  active or backup shard. Previously those messages could report the number of
  layers while labelling it as neurons.
- [x] Kept the placement surface read-only and preserved the existing
  layer-based highlighting contract: the current legacy placement telemetry
  identifies ownership by layer, so selection highlights the complete
  reported layer set until stable neuron ownership is available.
- [x] `cargo test --locked --test web_ui_browser_compat --quiet`,
  `cargo check --locked --features ui --all-targets --quiet`,
  `node --check web_ui/app.js`, `cargo fmt --all -- --check`, and
  `git diff --check` pass. Compiler output contains existing unused/deprecated
  warnings only.
- [x] Follow-up broad verification passed: `cargo test --locked
  --all-targets --quiet` (265 tests), `cargo check --locked --all-features
  --all-targets --quiet`, `cargo xtask bindings check`, and
  `scripts/qa/run-ansible-placement.sh`. The placement smoke run reached
  localhost, `qc00`–`qc04`, `sm00` and `sm01`; `qc05` was excluded because it
  remains unreachable under the script's existing policy. The planner reported
  six active enrolled compute nodes, a non-degraded placement, and
  `applied: false`.

## Progress update — 2026-09-05: bounded stable executor managed-loop seam

- [x] Added `src/managed_stable_executor.rs` behind the explicit
  `stable_executor_live` feature. It accepts only an already opened durable
  stable-ID bridge, allocates monotonic checkpoint operation IDs, processes
  external and queued causal work through the same fenced publication path,
  and reports remaining pending work when a bounded poll budget is exhausted.
- [x] Added `StableExecutorDurableBridge::step_pending`, sharing mirror
  preparation and retry logic with external admission. Same-tick child work
  therefore cannot bypass immutable complete-fabric publication.
- [x] Added explicit `ManagedNetwork` registration and polling methods plus a
  fail-closed guard against accidentally running the compatibility Runner once
  a stable executor is registered. Registration is paused, brain-scoped, and
  rejects a second legacy durable authority.
- [x] Added five integration tests in `tests/managed_stable_executor.rs` for
  durable transition publication, queued causal draining, duplicate input
  idempotence, stale fencing rejection, and deferred work reporting.
- [x] `cargo test --locked --features stable_executor_live --test
  managed_stable_executor -- --test-threads=1` passed all five tests;
  `cargo check --locked --features stable_executor_live --all-targets` passed
  with the repository's existing warnings.
- [x] Final verification also passed `cargo test --locked --all-targets
  --quiet`, `cargo test --locked --test web_ui_browser_compat --quiet`,
  `cargo check --locked --all-features --all-targets --quiet`, `cargo xtask
  bindings check`, `node --check web_ui/app.js`, `cargo fmt --all --
  --check`, and `git diff --check`.
- [x] `scripts/qa/run-ansible-placement.sh` passed using the existing Ansible
  inventory and SSH access. `localhost`, `qc00`–`qc04`, `sm00`, and `sm01`
  were reachable and enrolled; `qc05` remained excluded as unreachable. The
  run was a dry placement validation (`applied: false`) with degraded
  durability disabled.
- [!] Stable runtime construction from deployed topology/placement state,
  network quorum/election, physical multi-host cutover, and production RPO/RTO
  evidence remain disabled. The new registration method is an explicit
  embedding seam and does not infer authority from discovery or telemetry.

## Verification update — 2026-09-05: restart recovery and example launcher

- [x] Added `StableExecutorDurableBridge::open_existing`, which reopens a
  restored immutable complete-fabric cut without republishing it, verifies each
  durable actor byte-for-byte against the restored biological checkpoint, and
  derives the next mirror sequence from durable actor metadata.
- [x] Added a restart integration test proving digest equality, no pending
  mirror after reopen, and continuation at the next durable mirror sequence.
  `cargo test --locked --features stable_executor_live --test
  managed_stable_executor -- --test-threads=1` now passes all six tests.
- [x] Corrected `run_examples.sh` and `run_webcluster.sh` so local examples do
  not enable the production-only `management_v1` service through
  `--all-features`. The example launcher now selects free gRPC/HTTP ports,
  isolates the web runtime workspace, verifies `/api/config` readiness, fails
  fast when a node exits during startup, prints the exact dashboard URL and
  bounds child-process cleanup. `AARNN_BIN_DIR`, `AARNN_SKIP_BUILD`, and
  `AARNN_NATIVE_UI` make the launcher smoke-testable without changing its
  default release/native-UI behaviour.
- [x] Exact launcher smoke test passed with the existing debug binaries using
  `AARNN_SKIP_BUILD=1 AARNN_BIN_DIR=target/debug AARNN_NATIVE_UI=0`: the
  orchestrator, both nodes, and web dashboard started; the printed dashboard
  URL was reachable; and the bounded timeout left no launcher-owned processes.
  A direct isolated web profile probe reached `/api/config` after two seconds.
- [x] Broad verification passed: `cargo test --locked --all-targets --quiet`
  (265 library tests plus all integration targets),
  `cargo check --locked --all-features --all-targets --quiet`, the six web UI
  browser compatibility tests, `node --check web_ui/app.js`,
  `cargo xtask bindings check`, `cargo fmt --all -- --check`, `git diff
  --check`, and shell syntax checks. The Ansible placement QA also passed on
  localhost, `qc00`–`qc04`, `sm00`, and `sm01`; `qc05` remained excluded as
  unreachable, with six active enrolled compute nodes and `applied: false`.
- [x] The earlier bounded desktop-UI build window was superseded by a fresh
  release build of the explicit local profile. The release launcher smoke test
  and dashboard readiness probe now pass on this laptop.

## Verification update — 2026-09-05: local profile regression

- [x] Tightened both launchers to build with `--no-default-features` and only
  their documented local workload features. This prevents an example launch
  from inheriting the production-only `management_v1` service and failing on
  its required bearer token/mTLS configuration.
- [x] Built fresh binaries in an isolated target directory with
  `cargo build --locked --target-dir /tmp/aarnn-local-example-target --bin
  aarnn_rust --no-default-features --features 'engine_runtime,ui'` and the
  corresponding `web_ui` engine profile. The end-to-end launcher smoke test
  started the orchestrator, both nodes and the dashboard, fetched
  `/api/config` from the printed URL, and left no launcher-owned processes
  after shutdown.
- [x] `run_examples.sh` now prints the selected gRPC and dashboard ports after
  the dashboard readiness check. `run_webcluster.sh` uses the same dynamic
  dashboard port and readiness check for consistent operator output.

## Verification update — 2026-09-05: release launcher smoke test

- [x] Added `tests/run_examples_launcher.rs` to prevent the example launcher
  from regressing to `--all-features` and to require a successful `/api/config`
  readiness probe before printing the dashboard URL.
- [x] Rebuilt the release example profile with
  `cargo build --release --no-default-features --bin aarnn_rust
  --features 'engine_runtime,ui'` and the matching `web_ui` profile. The
  end-to-end `run_examples.sh` smoke test then started the orchestrator, both
  nodes and the dashboard without `NM_MANAGEMENT_BEARER_TOKEN`; it printed
  `http://127.0.0.1:8080` and exited cleanly under a bounded timeout.

## Verification update — 2026-09-05: stable bootstrap test and live URL confirmation

- [x] Added `ManagedNetwork::new` as the encapsulated constructor for a paused
  network with no durable authority. The stable-runtime integration test now
  uses this constructor instead of duplicating private executor fields.
- [x] `cargo test --locked --features stable_executor_live --test
  stable_runtime_bootstrap -- --test-threads=1` passed all four tests,
  including stable registration, bounded sensory polling, authority locking,
  and checkpoint/topology mismatch rejection.
- [x] A fresh binary built without `stable_executor_live` rejects
  `--stable-runtime-manifest` with the expected fail-closed error.
- [x] A bounded `run_examples.sh` smoke run on the current laptop started the
  orchestrator, both nodes and the web dashboard, passed the `/api/config`
  readiness probe, printed `Web dashboard: http://127.0.0.1:8080`, and cleaned
  up its child processes. The orchestrator log contained no bearer-token
  startup error.
- [!] Stable manifest execution remains local-process only. Physical
  multi-host stable routing, quorum/election, deployed executor registration,
  and production RPO/RTO evidence remain blocked by the gates recorded above.

## Verification update — 2026-09-05: placement-controller review binding

- [x] Corrected the controller fixture to state its intended full-capacity
  synthetic workload explicitly; the planner correctly retains its production
  headroom constraint for other callers.
- [x] Added source-plan and proposed-plan digests to `PlacementReview` and
  require matching schema, digest and exact canonical moved-shard evidence at
  commit. A stale review or a forged/duplicated moved-shard list is rejected
  before residence state or authority changes.
- [x] `cargo test --locked --test placement_controller -- --test-threads=1`
  passed all five controller tests, including stale-review and evidence
  tampering cases.
- [x] `cargo test --locked --test run_examples_launcher -- --test-threads=1`
  passed both launcher contract tests. The bounded laptop smoke run started
  the orchestrator, both nodes and web UI, printed the selected ports and
  `Web dashboard: http://127.0.0.1:8080`, and found no bearer-token startup
  error; launcher-owned processes were cleaned up after interruption.
- [x] `cargo test --locked --test web_ui_browser_compat -- --test-threads=1`
  passed all six web placement/UI compatibility tests.
- [!] The controller remains a reusable proposal gate. It is not yet wired to
  the deployed automatic-placement loop because stable physical routing,
  quorum/election and live executor registration remain production gates.

## Verification update — 2026-09-05: management placement admission

- [x] Added brain-scoped `PlacementController` state to both the reference and
  secured management services. `PlanPlacement` now runs the candidate through
  residence hysteresis, deterministic improvement, transfer-budget,
  concurrent-migration and emergency eligibility checks after the stateless
  planner succeeds.
- [x] Successful `ApplyPlacement` registry publication records a committed
  residence boundary only after the registry accepts the plan and cutover
  evidence. The controller restarts residence hysteresis for all shards at
  that boundary.
- [x] When `NM_PLACEMENT_REGISTRY_DIR` is configured, management reloads the
  authoritative persisted placement before reviewing a proposal and uses the
  same persisted registry in the secured apply profile. A process restart
  therefore cannot treat an existing brain as an initial placement.
- [x] Added an RPC-level regression: after the initial placement is applied,
  an otherwise valid automatic relocation at tick 10 is rejected with a
  failed-precondition residence error. `management_grpc` passed all 14 tests;
  `placement_controller` passed all 6 tests; all-feature all-target checking
  passed.
- [!] Management admission now enforces the controller contract, but the
  legacy layer assignment loop remains a compatibility projection. Stable
  physical routing and deployed stable-executor registration are still needed
  before it can become the authoritative automatic execution path.

## Verification update — 2026-09-05: cluster QA after management integration

- [x] `scripts/qa/run-ansible-placement.sh` passed using the existing
  `/home/pbisaacs/Developer/swarmhpc/swarmhpc/ansible/` deployment inventory.
  Reachable hosts were `localhost`, `qc00`–`qc04`, `sm00` and `sm01`; `qc05`
  was excluded as unreachable. Six explicitly granted compute nodes were
  enrolled, placement was non-degraded, and the QA run remained proposal-only
  (`applied: false`).
- [x] `RUSTFLAGS=-Awarnings cargo test --locked --all-targets --quiet` passed:
  265 library tests and every integration/example target passed.
- [x] All-feature all-target checking, formatting, diff validation, JavaScript
  syntax and launcher shell syntax passed. A bounded launcher run again
  started both distributed nodes and the dashboard, printed the selected gRPC
  ports and verified `http://127.0.0.1:8080`; no bearer-token startup error was
  present and no launcher-owned process remained after shutdown.
- [!] Stable worker registration is now connected to join/heartbeat, but the
  complete stable fabric remains local to one worker process. Physical virtual
  shard routing, remote causal admission, quorum/election, live migration
  dispatch and physical RPO/RTO evidence remain open gates.

## Progress update — 2026-09-05: stable worker registration boundary

- [x] Added the versioned `StableExecutorRegistration` wire contract to join,
  heartbeat and node status. It reports stable topology/partition and
  virtual-shard identity, current logical frontier, fenced lease observation,
  bounded poll budgets and state digest. Registration is explicitly an
  observation and never an authority grant.
- [x] Added the reusable `stable_worker` validation module. It rejects unknown
  profiles/schema versions, empty or malformed identities/digests, duplicate
  or unordered shard IDs, zero fencing/lease values, zero poll budgets and
  non-authoritative claims before they enter orchestrator state. A worker
  cannot change plan identity during a session without a migration boundary.
- [x] Stable executor nodes now send their registration during initial join
  and every heartbeat. The orchestrator records one stable worker per network,
  preserves a stable-network fence after worker loss, and refuses to fall back
  to legacy layer placement without an explicit migration transaction.
- [x] The compatibility rebalancer skips stable networks and stable workers
  cannot be changed or unloaded by legacy layer commands. This prevents a
  stable executor and the legacy Runner from becoming simultaneous writers.
- [x] Added unit and RPC-level tests for registration validation, duplicate
  ownership rejection, stable-network rebalancing fences and plan identity
  stability. `cargo check --locked --features stable_executor_live
  --all-targets`, the four stable bootstrap tests and the focused stable-worker
  tests pass.
- [!] This closes registration and legacy-scheduler isolation only. The
  registered executor still owns its complete fabric in one process: physical
  virtual-shard routing, remote causal admission, quorum/election, live
  migration dispatch and physical RPO/RTO evidence remain open gates.

## Verification update — 2026-09-05: launcher checkout and authentication regression

- [x] Rebuilt the release example binaries with the explicit local profiles
  (`aarnn_rust`: `engine_runtime,ui`; `web_ui`: `engine_runtime`) and ran a
  bounded `run_examples.sh` smoke test. The orchestrator, both nodes and web
  dashboard became ready without `NM_MANAGEMENT_BEARER_TOKEN`; `/api/config`
  succeeded before the launcher printed `Web dashboard URL (port 8080):
  http://127.0.0.1:8080`.
- [x] Added repository-directory anchoring to both launchers and a regression
  assertion in `tests/run_examples_launcher.rs`, so invoking a launcher by
  absolute path cannot accidentally use another checkout's binaries or
  snapshots. The launcher contract suite passes all three tests.
- [!] The exact old banner and bearer-token failure came from the separate
  `neuromorphic_demo/run_examples.sh` checkout, which still builds with
  `--all-features`. That checkout is outside this repository's implementation
  scope; this AARNN launcher now identifies and uses its own local profile.

## Progress update — 2026-09-05: complete-plan and worker-ownership contract

- [x] Versioned `StableExecutorRegistration` to schema/profile v2 and added
  `owned_shard_ids`. `shard_ids` now means the complete immutable plan
  inventory, while `owned_shard_ids` records the non-empty sorted subset
  materialised by the reporting worker.
- [x] Added fail-closed validation for zero, unordered, duplicate and
  out-of-plan owned shard IDs. Plan identity comparisons deliberately ignore
  ownership telemetry so a migration handoff can update the worker subset
  without fabricating a topology or partition change; an ownership update
  nevertheless requires a newer lease term and fencing token.
- [x] Added protobuf conversion coverage and an RPC-level heartbeat test that
  changes ownership from `[1,2]` to `[2]` across a newer fenced boundary while
  preserving complete plan inventory `[1,2]`. The current admission policy
  still permits only one
  stable worker per network, so partial execution is not enabled by this
  contract change.
- [x] `cargo test --locked --features stable_executor_live --lib
  stable_worker -- --test-threads=1` and the stable distributed admission
  tests passed (6 tests); stable managed-executor, bootstrap and shard
  executor integration tests passed (6, 4 and 6 tests respectively).
- [!] The current managed executor reports the complete inventory as its owned
  subset. Remote causal application, durable ownership handoff, quorum
  fencing and physical multi-host execution remain required before a worker
  may materialise only a subset in production.

## Verification update — 2026-09-05: local-profile feature-gate repair

- [x] Fixed the distributed causal stream handler so stable-executor
  admission methods are referenced only under `stable_executor_live`. The
  no-management local profile therefore compiles and retains its established
  causal ingress path; the stable profile continues to use managed causal
  admission.
- [x] Rebuilt both release binaries with the explicit local profiles used by
  `run_examples.sh`: `aarnn_rust` with `engine_runtime,ui` and `web_ui` with
  `engine_runtime`.
- [x] Ran a bounded `run_examples.sh` smoke test with the native window
  disabled. The orchestrator, two nodes and web UI became ready;
  `/api/config` succeeded before the launcher reported
  `Web dashboard URL (port 8080): http://127.0.0.1:8080`; shutdown left no
  launcher-owned release processes and the orchestrator log contained no
  `NM_MANAGEMENT_BEARER_TOKEN` error.
- [x] `cargo fmt --all -- --check`, `git diff --check`, the three launcher
  contract tests, the default all-target test suite, and the all-feature
  all-target check passed.
- [!] The separate `neuromorphic_demo` checkout remains outside this
  repository. Its launcher still needs the same explicit local feature
  profile, or a management token/TLS configuration when it intentionally
  enables `management_v1`.

## Progress update — 2026-09-05: stable causal stream cursor and gRPC admission

- [x] Added a bounded per-runtime `ReliableReceiver` cursor for stable causal
  ingress. It validates contiguous producer sequences, brain/stream/term/
  generation identity, payload limits, and event/payload identity on replay.
  A duplicate returns an empty poll and therefore cannot drain unrelated
  queued biological work; a gap or conflicting replay fails closed.
- [x] Preserved external producer stream sequence space separately from the
  stable bridge's internal mirror sequence. External envelopes are mirrored
  into every authoritative shard's receipt ledger, and a reopened bridge
  reconstructs the next cursor only when all shard receipt prefixes agree.
- [x] Stable stream selection now routes a known stable brain or authenticated
  link stream into the stable authority. A valid brain with the wrong stream
  therefore receives a fenced precondition failure rather than falling into
  compatibility JSON ingress.
- [x] Added an end-to-end `DistributedNode` gRPC test covering enrolled sender
  acceptance, unknown sender denial, duplicate replay, same-event payload
  conflict, sequence gap, stale lease, brain mismatch, stream mismatch, and
  successful continuation after rejected frames. The test also verifies the
  durable external receipt progress.
- [x] Stable-profile all-target tests passed (281 library and 270 integration
  tests); default all-target tests passed (268 library and 257 integration
  tests); all-feature all-target checking and formatting passed.
- [!] This closes stream admission and cursor recovery for the local stable
  bridge. It does not yet provide deployed partial-shard ownership, networked
  quorum/election, physical multi-host causal routing, or chaos RPO/RTO proof;
  those remain the production migration gates.

## Verification update — 2026-09-05 15:58Z: durable application acknowledgement gate

- [x] Upgraded the stable worker registration contract to schema/profile v3.
  Every materialised shard must report one committed durable application
  acknowledgement bound to brain identity, topology/partition generation,
  plan digest, lease term, fencing token, logical frontier and state digest.
  The acknowledgement set is sorted, complete and fail-closed; it is an
  observation and never grants writer authority.
- [x] Added protobuf conversion and managed-executor reporting from sealed
  durable actor checkpoints. If actor evidence cannot be read, the worker
  reports no acknowledgements and orchestrator admission rejects it rather
  than synthesising state.
- [x] Added unit/RPC coverage for missing acknowledgements, stale plan/fence
  evidence, uncommitted evidence, ownership updates across a newer fenced
  boundary, and accepted acknowledgement updates. Added a restart assertion
  proving that acknowledgement records are reconstructed identically from
  durable actor checkpoints.
- [x] Empty ownership is now valid for an enrolled worker after a fenced
  drain. This is required for the source side of whole-brain migration to
  detach cleanly while the immutable plan inventory remains present.
- [x] Focused verification passed:
  `cargo test --locked --features stable_executor_live --lib stable_worker`,
  `cargo test --locked --features stable_executor_live --test managed_stable_executor`,
  `cargo test --locked --features stable_executor_live --lib distributed`,
  `cargo test --locked --features stable_executor_live --test placement_cli`,
  `cargo test --locked --features stable_executor_live --test causal_grpc`,
  and `cargo test --locked --test run_examples_launcher`.
- [!] This closes durable application evidence at the registration seam only.
  The worker still materialises the complete fabric locally; remote causal
  routing, quorum/election, physical ownership handoff and multi-host
  RPO/RTO evidence remain required before partial execution is enabled.

## Verification update — 2026-09-05 16:02Z: launcher end-to-end confirmation

- [x] Ran `AARNN_NATIVE_UI=0 AARNN_SKIP_BUILD=1 timeout --signal=INT
  --kill-after=5s 12s ./run_examples.sh`. The local orchestrator, two nodes
  and dashboard reached readiness; the launcher printed
  `Web dashboard URL (port 8080): http://127.0.0.1:8080` only after
  `/api/config` succeeded, and cleanup completed on interruption.
- [x] The resulting orchestrator log contained no
  `NM_MANAGEMENT_BEARER_TOKEN` startup error and no launcher-owned release
  process remained after shutdown.

## Verification update — 2026-09-05: partial worker and durable outbound seam

- [x] Added `src/partial_shard_executor.rs`. A worker now materialises only
  its assigned virtual-shard checkpoints, retains immutable topology
  descriptors, routes cross-shard causal work as typed messages and permits a
  zero-owned-shard drained worker. It rejects split mutable synapse ownership,
  incomplete or over/under-populated checkpoint state, stale plan messages,
  wrong declared destinations and conflicting same-key control replays.
- [x] Added `src/stable_outbound.rs`. The bounded per-destination log uses
  independent sequences, atomic fsync publication, process locking, digest
  validation, crash reopen, retry-safe acknowledgements, authority fencing,
  logical-tag/event/shard/plan metadata and corrupt-storage rejection. It also
  exposes a placement-registry route helper that resolves the active node and
  fence before append.
- [x] Added `tests/partial_shard_executor.rs` and
  `tests/stable_outbound.rs`. The partial-worker parity test exchanges work
  between two workers and matches the complete reference shard bytes; the
  outbound tests cover restart/replay, per-destination sequence spaces,
  conflicting acknowledgements, stale fences and corruption.
- [x] `cargo fmt --all`, the focused partial-worker and outbound suites,
  `cargo test --locked --test run_examples_launcher -- --test-threads=1`,
  `git diff --check`, `bash -n run_examples.sh` and a bounded local launcher
  smoke passed. The launcher reported the exact dashboard URL and port only
  after `/api/config` succeeded, and the resulting orchestrator log contained
  no missing bearer-token startup error.
- [!] The durable log and partial executor remain transport-neutral reference
  seams. The stable runtime still mirrors a complete fabric locally; wiring
  these records to authenticated causal gRPC, placement authority, durable
  receiver application and quorum-backed promotion remains a production gate.
- [x] Added `src/stable_shard_transport.rs` and the versioned
  `StableShardDataPlane` protobuf service. Frames carry source/destination
  node identity, brain, shard, topology/partition generations, plan digest,
  lease/fence, stream sequence, logical tag, event identity, typed message kind,
  bounded payload and record digest. Metadata is checked against the sealed
  `PartialShardOutbound` payload before application.
- [x] Added a durable receiver that applies contiguous frames to a partial
  worker and atomically publishes its receipt frontier with shard checkpoints.
  It rejects unauthorised sources, gaps, conflicting replays, stale fences,
  invalid digests and receipt-window exhaustion; duplicate frames return a
  durable idempotent acknowledgement. `flush_pending` asynchronously retries
  pending sender records and removes them only after a matching durable ack.
- [x] Added `tests/stable_shard_transport.rs` covering two-worker durable
  application, restart reconstruction, duplicate replay, sequence gaps, stale
  authority, the generated tonic stream and sender outbox acknowledgement.
  The focused suite passed with two tests.
- [!] The transport and receiver now form a usable reference data-plane
  boundary, but the stable runtime still does not dispatch physical worker
  ownership from the orchestrator. Quorum-backed authority, authenticated
  session binding, network fault/RPO/RTO evidence and production cutover remain
  required before enabling live partial execution.
- [x] Generation binding was strengthened: sender records and protobuf frames
  now carry topology and partition generations, receiver admission compares
  both with its active execution plan, and the transport test covers a validly
  sealed wrong-generation frame being rejected before application.
- [x] Final validation after the transport changes passed `cargo check
  --locked --all-features --all-targets`, `cargo test --locked --all-targets
  --quiet` (270 library and 258 integration/target tests), formatting, diff
  checks and launcher syntax checks. Existing repository warnings remain
  non-fatal and unrelated to this seam.
- [x] Reused `/home/pbisaacs/Developer/swarmhpc/swarmhpc/ansible/` and its
  inventory for a read-only placement probe. `localhost`, `qc00`–`qc04`,
  `sm00` and `sm01` were reachable; `qc05` was excluded as unreachable. The
  Rust placement smoke selected six enrolled active nodes, produced a stable
  digest and kept `applied: false`, so no live placement was changed.

## Verification update — 2026-09-05: placement-authorised physical dispatch

- [x] Added `StableShardDispatcher` as the reusable asynchronous join between
  the authoritative placement registry, bounded durable per-destination
  outbox and typed stable-shard transport. Independent destination streams are
  flushed concurrently; failed streams remain pending for reconnect retry.
- [x] Sealed outbound records now carry a separate physical placement-plan
  digest in addition to the compiled biological execution-plan digest. The
  dispatcher validates destination authority, placement digest, lease/fence and
  topology/partition generations before any network send.
- [x] Added `tests/stable_shard_dispatch.rs` covering planner/registry setup,
  digest separation, missing and invalid endpoints, failed-connection retry
  retention, stale placement/fence/generation rejection and the bounded batch
  admission limit. All five tests pass.
- [x] Re-ran the current `run_examples.sh` with the local non-management
  profile. The orchestrator, both nodes and dashboard became ready and the
  launcher printed `Web dashboard URL (port 8080):
  http://127.0.0.1:8080` after the `/api/config` readiness probe. Logs contain
  no bearer-token startup error.
- [!] This dispatcher is an explicit physical data-plane seam. It is not yet
  connected to the live deployed worker loop, quorum-backed authority,
  authenticated node-session identity or production migration cutover. The
  complete stable fabric therefore remains local until those gates and
  multi-host RPO/RTO evidence pass.

## Progress update — 2026-09-05: staged partial-worker commit boundary

- [x] Added `ManagedPartialShardRuntime`, a reusable bounded worker-loop
  adapter that stages `PartialShardExecutor` transitions, atomically seals all
  typed cross-shard output through `StableShardDispatcher`, and publishes the
  staged biological state only after durable outbox success.
- [x] Made placement-resolved outbound batch append atomic. Queue, size,
  validation or fencing failure leaves both the durable outbox and staged
  worker state unchanged; a successful network flush may still be retried
  independently from the committed worker state.
- [x] Added integration coverage proving a partial worker produces durable
  cross-node output and that a later queue-bound failure rolls the entire
  outbound batch back. The focused dispatcher and partial-runtime suites pass.
- [!] This closes the local worker commit seam. It does not claim deployed
  partial execution: receiver ownership, authenticated worker sessions,
  quorum-backed authority, runtime registration and multi-host failure/RPO/RTO
  evidence remain required before enabling automatic production sharding.

## Verification update — 2026-09-05: fresh release launcher validation

- [x] Rebuilt the exact local profiles used by `run_examples.sh`: `aarnn_rust`
  with `engine_runtime,ui` and `web_ui` with `engine_runtime`, both with
  `--no-default-features`. This prevents the authenticated `management_v1`
  endpoint from being linked into the local example orchestrator.
- [x] `cargo fmt --all -- --check`, `git diff --check`,
  `cargo test --locked --all-targets --quiet` (270 library and 259 integration
  tests), `cargo check --locked --all-features --all-targets`, launcher
  contract tests, shell syntax checks and JavaScript syntax checks passed.
- [x] A bounded run of the freshly rebuilt launcher reached `/api/config`,
  printed `Web dashboard URL (port 8080):
  http://127.0.0.1:8080`, and shut down without leaving launcher-owned
  processes. Orchestrator, node and web logs contained no
  `NM_MANAGEMENT_BEARER_TOKEN` startup diagnostic.
- [!] The current implementation remains a validated local/reference seam for
  partial execution. Live receiver ownership, quorum/election, authenticated
  node-session fencing, physical multi-host migration and chaos RPO/RTO proof
  remain required before production sharding is enabled.

## Verification update — 2026-09-05: receiver registry and launcher regression

- [x] Added explicit node-scoped receiver registration to
  `StableShardReceiverRegistry`. A distributed node now exposes the stable-shard
  service backed by the registry on both orchestrator and worker gRPC listeners;
  registration remains an explicit bootstrap/migration action and cannot be
  created by discovery or by an incoming frame.
- [x] Added coverage for duplicate brain registration, receiver/node identity
  mismatch, idempotent unregister cleanup and gRPC `NotFound` after a brain is
  removed. The focused `stable_shard_transport` suite passes all four tests.
- [x] Re-ran `cargo test --locked --test run_examples_launcher --
  --test-threads=1`, `git diff --check`, `bash -n run_examples.sh
  run_webcluster.sh` and `cargo fmt --all` successfully.
- [x] Ran `AARNN_SKIP_BUILD=1 AARNN_NATIVE_UI=0 timeout --signal=INT
  --kill-after=5s 15s ./run_examples.sh` using the current release binaries.
  The orchestrator, both nodes and web dashboard became ready; `/api/config`
  passed before the launcher printed `Web dashboard URL (port 8080):
  http://127.0.0.1:8080`; bounded shutdown completed and the generated
  orchestrator log contained no `NM_MANAGEMENT_BEARER_TOKEN` startup error.
- [!] The registry is a local ownership binding, not quorum election or
  authenticated session proof. Runtime manifest bootstrap, authenticated
  source-node binding, receiver-side outbound dispatch and physical multi-host
  migration/RPO/RTO evidence remain required before production promotion.
- [x] Repeated the bounded launcher smoke with the default native UI enabled
  (`AARNN_SKIP_BUILD=1 timeout --signal=INT --kill-after=5s 12s
  ./run_examples.sh`). The native UI, orchestrator, both nodes and dashboard
  all reached readiness, the same URL/port was printed, and the orchestrator
  log reported a ready node without the previous management-token error.

## Progress update — 2026-09-05: partial worker bootstrap and durable fan-out

- [x] Extended the stable transport receiver with a bounded durable pending
  outbound journal. Applying a remote event now settles bounded local work and
  persists every generated cross-shard message before the input can be
  acknowledged. A restart replays the pending generated work, and the
  placement-aware outbound log reuses an identical pending record's sequence
  instead of allocating a duplicate.
- [x] The stable gRPC service now requires source-node session metadata and
  rejects a frame whose declared source differs from that session identity.
  When generated output exists, an explicitly registered dispatcher must seal
  it into the local durable outbox before the inbound acknowledgement is
  emitted. Missing dispatchers therefore fail closed while retaining the
  receiver journal for retry.
- [x] Added `StablePartialWorkerBootstrapManifest` and the
  `--stable-worker-manifest` CLI path behind `stable_executor_live`. Bootstrap
  verifies the complete immutable checkpoint and compiled plan, materialises
  only the declared shard subset, checks active-node placement/term/fence
  identity, binds source allow-lists, creates the receiver and durable
  dispatcher, and registers both explicitly with the distributed node.
- [x] Added regression coverage for admitted-event digest serialisation,
  generated-output restart/replay, missing/mismatched session identity,
  no-ack without a dispatcher, idempotent outbound append, partial-worker
  placement admission and node mismatch rejection.
- [!] The dispatcher binding and session metadata are still reference control
  adapters until the deployment supplies authenticated mTLS/session identity
  and a replicated placement authority. Physical multi-host migration,
  receiver-side quorum promotion and chaos RPO/RTO evidence remain blocked by
  the existing production gates; the bounded receiver admission and outbox
  flush lifecycle is recorded in the verification update below.

## Verification update — 2026-09-05: partial-worker lifecycle telemetry and retry pass

- [x] Added a durable receiver snapshot boundary that reports the active
  topology/partition and plan digests, owned virtual shards, logical frontier,
  whole-worker digest and per-shard checkpoint acknowledgements from the same
  persisted cut used for data-plane application.
- [x] Bound manifest-bootstrapped partial workers to their network identity
  and bounded poll budgets before registration. The regular join/heartbeat
  path now publishes those stable-worker observations alongside complete
  stable executors; invalid snapshot evidence is omitted rather than
  fabricated or promoted.
- [x] Added a bounded lifecycle pass that concurrently flushes every
  registered stable outbox. Destination failure, timeout or missing endpoint
  leaves the sealed records durable for a later heartbeat; it cannot delay
  the control-plane heartbeat beyond the 500 ms pass budget.
- [x] Verification passed `cargo fmt --all`,
  `cargo test --locked --test stable_shard_transport -- --test-threads=1`,
  `cargo check --locked --features stable_executor_live --bin aarnn_rust`,
  `cargo test --locked --test run_examples_launcher -- --test-threads=1`,
  `cargo check --locked --all-targets`, the focused outbound/dispatcher/
  managed-worker suites, and `git diff --check`. The bounded laptop launcher smoke reached
  `/api/config`, printed `http://127.0.0.1:8080`, and shut down cleanly with
  no `NM_MANAGEMENT_BEARER_TOKEN` startup error.
- [x] Rebuilt the exact release binaries used by the launcher with
  `cargo build --release --no-default-features --bin aarnn_rust
  --features 'engine_runtime,ui'` and the matching `web_ui` profile, then
  repeated the bounded smoke against those fresh binaries. No launcher-owned
  process remained after the timeout-driven shutdown.
- [!] This closes the local registration/flush lifecycle seam. It still does
  not claim authenticated mTLS identity, replicated quorum/fencing, a
  networked partial executor poll/admission path, physical migration
  cutover, or multi-host chaos/RPO/RTO evidence; those remain mandatory
  promotion gates.

## Verification update — 2026-09-05: authenticated session boundary and example launcher

- [x] Added the shared `src/node_auth.rs` boundary for inter-node session
  claims. Live causal and stable-shard streams now require the claimed node
  ID, a node-scoped enrolled credential, and the SHA-256 fingerprint of the
  authenticated mTLS leaf certificate. Missing metadata is rejected before
  credential configuration is read, and malformed credential/fingerprint
  configuration fails closed.
- [x] Routed stable-shard client connections through the shared
  `management::grpc_client_endpoint` policy. Enabling the live profile now
  selects the configured client certificate/CA/domain and cannot silently
  fall back to plaintext. Stable-shard credentials are selected from
  `NM_CAUSAL_NODE_TOKENS` for the claimed source node rather than from an
  unscoped shared token.
- [x] Added denial coverage for missing session metadata, wrong node token,
  missing certificate fingerprint and mismatched certificate fingerprint in
  the `node_auth` tests. The stable-shard gRPC suite continues to pass its
  metadata/session identity and durable replay cases.
- [x] Fixed and validated `run_examples.sh`: it builds only the local
  `engine_runtime,ui` and `web_ui` profiles, waits for `/api/config`, prints
  the exact dashboard URL and port, and performs bounded cleanup. A fresh
  release-binary smoke reached `http://127.0.0.1:8080`; orchestrator, node and
  web logs contained no `NM_MANAGEMENT_BEARER_TOKEN` startup error, and the
  launcher-owned processes and ports were released after shutdown.
- [x] Post-change evidence: `cargo fmt --all`,
  `cargo test --locked --all-targets --quiet` (274 library and 263
  integration/target tests), `cargo test --locked --test
  run_examples_launcher --quiet`, `cargo build --locked --release
  --no-default-features --bin aarnn_rust --features 'engine_runtime,ui'`,
  the matching `web_ui` release build, `bash -n run_examples.sh
  run_webcluster.sh`, `cargo check --locked --features
  stable_executor_live --bin aarnn_rust` and `git diff --check` all passed.
- [!] The new authentication boundary is a prerequisite for live promotion,
  not proof of production identity by itself. Physical mTLS deployment,
  replicated quorum/fencing, orchestrator-controlled receiver ownership,
  multi-host migration cutover, and fault-injected RPO/RTO evidence remain
  mandatory before production sharding is enabled.

## Verification update — 2026-09-05: placement activation dispatch and launcher confirmation

- [x] Added the bounded `stable_worker_activation_json` field to the
  management placement request. The management layer decodes, verifies and
  brain-binds the `StableWorkerActivationCommand`; an activation target must
  be an active node in the applied plan and an activation dispatcher must be
  configured before the registry mutation is attempted.
- [x] Added an orchestrator-owned placement activation dispatcher in
  `src/main.rs`. After successful fenced placement publication it queues the
  activation through `DistributedNode`, so management clients never contact a
  worker directly. Queue admission requires an enrolled stable-worker
  activation capability observation, a known peer address and bounded command
  capacity; an existing shard registration is not required for first
  activation. Identical retries are idempotent and conflicting activation
  commands for one network are rejected.
- [x] Added `--placement-stable-worker-activation-json` for the remote apply
  CLI path and regression coverage for management dispatch and worker queue
  replay. The focused management suite passed 15 tests; the stable queue test
  passed with `stable_executor_live` enabled.
- [x] Verified `cargo check --locked --features management_v1 --bin aarnn_rust`,
  `cargo check --locked --all-targets`, `cargo fmt --all`, `git diff --check`
  and shell syntax checks. A bounded `run_examples.sh` smoke reached
  `http://127.0.0.1:8080`, printed the URL and port, found no management-token
  startup error, and released ports 50051, 50075, 50087 and 8080 on shutdown.
- [!] The activation seam remains a reference control path until physical
  mTLS identity, replicated quorum/fencing, a networked partial-executor poll
  loop, physical migration cutover and multi-host RPO/RTO evidence pass. The
  launcher profile intentionally remains free of `management_v1`, so local
  examples do not require a production bearer token.

## Verification update — 2026-09-05: final release rebuild

- [x] Rebuilt the final launcher binaries with the exact `run_examples.sh`
  profiles after the active-session admission tightening:
  `cargo build --locked --release --no-default-features --bin aarnn_rust
  --features 'engine_runtime,ui'` and the matching `web_ui` build.
- [x] Repeated the bounded release launcher smoke. It reached
  `http://127.0.0.1:8080`, printed the dashboard port, found no management
  bearer-token startup error, and released all selected ports on shutdown.
- [x] Final `cargo fmt --all -- --check` and `git diff --check` passed.

## Verification update — 2026-09-05: idle stable-worker activation admission

- [x] Added versioned `StableExecutorCapability` observations to the
  distributed join, heartbeat and node-status contracts. The observation is
  separate from shard registration and carries only the supported stable
  profile, activation schema and bounded poll budgets.
- [x] Stable-enabled workers now advertise this capability before owning a
  network or shard. The orchestrator validates schema/profile/budget bounds,
  stores the observation with the enrolled node session, and admits an
  activation command using that capability plus the existing active session,
  peer-address, command verification and later target-side manifest/fence
  checks.
- [x] Added regression coverage for first activation of an idle enrolled
  worker, denial when only resource/network telemetry is present, and
  malformed or oversized capability budgets. This preserves the rule that
  capability and telemetry observations never grant placement or writer
  authority.
- [x] Validation passed `cargo check --locked --all-targets`,
  `cargo check --locked --all-targets --features stable_executor_live`,
  `cargo test --locked --lib --features stable_executor_live
  idle_enrolled_worker_can_receive_its_first_stable_activation`,
  `cargo test --locked --lib --features stable_executor_live
  resource_or_network_observation_without_activation_capability_is_denied`,
  `cargo fmt --all` and `git diff --check`.
- [!] This removes the first-activation deadlock in the reference control
  path. It does not by itself provide physical mTLS enrolment, replicated
  placement authority, manifest construction from live checkpoint state,
  networked executor polling, or production migration cutover evidence.

## Verification update — 2026-09-05: durable placement activation outcome

- [x] Added persisted `PlacementActivationStatus` records keyed by the
  placement idempotency key. A published placement paired with a worker
  activation now records `pending`, then `queued` after dispatch, or `failed`
  with a bounded error. The status is bound to the applied request ID and plan
  digest and cannot be attached to another placement.
- [x] Wired both generated management service implementations and the
  persisted/in-memory registry adapters through this lifecycle. Dispatch
  failure is returned as an unavailable operation while the durable registry
  retains the failed outcome for retry and inspection; an error persisting the
  failure is surfaced as an internal error rather than hidden.
- [x] Added registry retry/binding coverage and asserted that successful
  management activation responses contain a `queued` status without creating a
  second placement resource version. `placement_registry` (5 tests) and
  `management_grpc` (15 tests) passed.
- [x] The reference profile now persists the verified activation command with
  the lifecycle record and reconstructs eligible commands after restart. The
  remaining production gate is replicated authority and quorum commit rather
  than single-process recovery.

## Verification update — 2026-09-05: validated activation-manifest construction

- [x] Added `StablePartialWorkerBootstrapManifest::from_authoritative_state`.
  It builds the partial-worker DTO from the complete verified runtime
  manifest, immutable placement plan, target node and selected shard set,
  then validates generation/brain/term/fence identity, active ownership,
  bounded paths and explicit source/endpoint allowlists before returning it.
- [x] Converted the orchestrator activation fixture to use the factory and
  added rejection coverage for selecting a shard that is not active on the
  declared target. The stable bootstrap suite passed all 8 tests.
- [!] The factory is now reusable by an orchestrator or CLI adapter, but live
  manifest acquisition from an authoritative checkpoint catalogue and the
  networked worker activation/recovery loop remain separate production-gated
  work. No caller may infer those inputs from discovery or telemetry.

## Verification update — 2026-09-05: read-only physical inventory QA

- [x] Ran the repository Ansible adapter against the existing inventory at
  `/home/pbisaacs/Developer/swarmhpc/swarmhpc/ansible/` using the laptop,
  `qc00`–`qc05` and `sm00`/`sm01`. The read-only probe reached `localhost`,
  `qc00`–`qc04`, `sm00` and `sm01`; `qc05` was excluded as unreachable.
- [x] With explicit compute grants for `qc00`, `qc02`, `qc03`, `qc04`, `sm00`
  and `sm01`, the Rust planner admitted six shards across `qc02`, `qc03`,
  `qc04`, `sm00` and `sm01`, with `degraded_durability: false` and
  `applied: false`. Evidence is retained in
  `qa/artifacts/ansible-placement-current.json`.
- [x] The same inventory probe with an explicit single-host consolidation to
  `qc02`, zero warm replicas and an explicit degraded-durability policy
  admitted all six shards only on `qc02`, still without mutating the cluster.
  Evidence is retained in
  `qa/artifacts/ansible-placement-consolidate-current.json`.
- [!] This is hardware/resource and planner evidence only. It does not prove
  that the existing hosts are enrolled for stable execution, possess matching
  mTLS identities, or can safely accept a production shard activation.

## Progress update — 2026-09-05: at-least-once stable-worker activation delivery

- [x] Extended the distributed heartbeat contract with bounded
  `NetworkCommandResult` acknowledgements for stable-worker activation. Each
  result is bound to the network, activation request and manifest digest; an
  accepted result cannot carry diagnostic text, and rejected results require a
  bounded error.
- [x] The orchestrator now retains activation commands after delivering a
  heartbeat response and removes them only after a matching result arrives.
  A lost response therefore causes a safe retry after reconnect rather than
  silently losing the activation. Identical result retries are idempotent;
  conflicting or forged results fail closed.
- [x] The node heartbeat loop retains results across reconnect attempts and
  emits them again when an RPC response is lost. Activation execution still
  occurs through the existing bounded manifest/checkpoint bootstrap path, and
  legacy command delivery remains unchanged until it receives its own versioned
  acknowledgement contract.
- [x] Added a regression scenario covering command replay, manifest-digest
  forgery rejection, successful acknowledgement replay and terminal worker
  failure. The focused stable-executor test passes, along with default and
  stable-feature compilation.
- [!] The acknowledgement delivery itself remains an in-process orchestrator
  callback and is not backed by a replicated authority across orchestrator
  restart. Placement-bound failure persistence was added in the follow-up
  update below. Physical mTLS session identity, replicated quorum/fencing,
  durable command recovery and multi-host migration evidence remain mandatory
  before production promotion.

## Verification update — 2026-09-05: placement-bound activation outcomes

- [x] Added a placement idempotency binding to activation commands. The
  management decoder stamps the immutable placement key before dispatch; the
  worker result carries that key, brain identity and manifest digest, so a
  result cannot update an unrelated placement record.
- [x] Added `PlacementRegistry::record_activation_outcome` and the persisted
  `open_existing` path. The orchestrator-side result hook updates a failed
  worker activation durably while preserving the original request and plan
  binding; successful results remain `queued` until complete durable worker
  registration evidence is observed.
- [x] Installed the hook only on the orchestrator management service and run
  registry persistence in bounded blocking tasks outside the heartbeat state
  lock. Workers never receive a management service or registry handle.
- [x] Verified the focused placement, management, stable-activation, format,
  diff and feature compilation checks after regenerating the protobuf bindings.
- [x] The result hook remains process-local, while the placement journal now
  reconstructs and retries eligible activation commands after orchestrator
  restart. Production mTLS identity, replicated quorum/fencing and physical
  multi-host migration evidence remain open gates.

- [x] Worker outcomes cannot resurrect a terminal failed activation through a
  delayed or replayed acknowledgement. A new management retry must first
  create the explicit retry lifecycle, while identical duplicate outcomes
  remain idempotent.
- [x] Added `tests/stable_activation_heartbeat.rs`, which exercises the
  join/heartbeat/acknowledgement flow through a live tonic gRPC server. It
  proves command replay and removal over the wire in both the default and
  `stable_executor_live` test profiles.

## Verification update — 2026-09-05: restart-safe activation reconstruction

- [x] Extended the placement activation record with a bounded, immutable JSON
  copy of the verified worker activation command. The command and `pending`
  lifecycle state are published in one crash-safe registry mutation, so an
  orchestrator loss cannot leave a retryable activation without its payload.
- [x] Added restart recovery on the secured orchestrator service. A bounded
  polling task scans only the configured placement registry directory,
  validates brain identity, plan digest, placement target, command schema and
  manifest digest, then requeues only `pending`/`queued` activations after the
  existing enrollment, capability and live-session admission checks succeed.
  Terminal `failed` records are excluded and require a new management retry.
- [x] Added process-local retry suppression after successful requeue so a
  healthy node does not receive the same durable retry every poll. A process
  crash clears only this suppression set; the durable idempotency key and
  worker-side idempotence remain authoritative.
- [x] Added persistence/reopen and terminal-failure regression coverage in
  `tests/placement_registry.rs`; the focused test passes. Management-profile
  compilation and formatting checks pass.
- [x] Rebuilt the exact local release profiles used by `run_examples.sh`, ran
  `cargo test --locked --test run_examples_launcher -- --test-threads=1`
  (3 passed), and ran a bounded launcher smoke. It reached the dashboard at
  `http://127.0.0.1:8080`, started the orchestrator and both nodes without
  `NM_MANAGEMENT_BEARER_TOKEN`, and cleaned up all launcher-owned processes.
- [!] The recovery journal is crash-safe on one orchestrator and remains a
  reference control path until replicated quorum authority, physical mTLS
  session identity, physical receiver ownership and multi-host chaos/RPO/RTO
  evidence are complete.

## Verification update — 2026-09-05 19:48Z: example launcher authentication and URL regression

- [x] Kept `run_examples.sh` on the explicit local `engine_runtime,ui`
  profile and `web_ui` on `engine_runtime`; the launcher no longer enables the
  authenticated `management_v1` endpoint implicitly through `--all-features`.
  This preserves fail-closed bearer authentication for deployments that
  intentionally enable management while keeping the local example runnable.
- [x] The launcher selects free gRPC and HTTP ports, waits for
  `/api/config` to succeed, and then prints the exact dashboard URL and port.
  Cleanup is bounded and idempotent under timeout or Ctrl+C interruption.
- [x] `cargo test --locked --test run_examples_launcher -- --test-threads=1`
  passed all 3 launcher contract tests; `git diff --check` and shell syntax
  checks passed.
- [x] Rebuilt the exact release profiles used by the launcher and ran a
  bounded smoke with `AARNN_NATIVE_UI=0 AARNN_SKIP_BUILD=1 timeout -k 3s 11s
  ./run_examples.sh`. The orchestrator, both nodes and dashboard reached
  readiness, the output included `Web dashboard URL (port 8080):
  http://127.0.0.1:8080`, no service log contained the bearer-token startup
  error, and no launcher-owned process remained after cleanup.

## Verification update — 2026-09-05: automatic placement coordinator coverage

- [x] Added `tests/placement_automation.rs` with a verified stable runtime
  and immutable checkpoint fixture. The suite proves initial placement can
  publish independent activation commands for multiple target nodes, and
  each command contains only that target's shard subset.
- [x] The suite reopens the same persisted registry and reconstructs both
  retryable activation commands. This covers the multi-target activation
  journal shape introduced for distributed placement and prevents one
  target's retry state from overwriting another target's state.
- [x] The suite rejects observations outside the explicit compute grant,
  unenrolled nodes and nodes without compute authorisation. It also rejects
  movement without checkpoint/cutover evidence, then accepts the same
  consolidation after complete per-shard checkpoint, route-cursor and effect
  cursor evidence is supplied.
- [x] `cargo test --locked --features stable_executor_live --test
  placement_automation -- --test-threads=1` passed all 3 tests after
  `cargo fmt --all`.
- [!] These tests validate the durable orchestrator boundary and worker
  command shape. They do not claim physical migration success; replicated
  quorum authority, mTLS session identity, live checkpoint transfer,
  multi-host cutover, recovery/rejoin and chaos/RPO/RTO evidence remain open
  production gates.

## Verification update — 2026-09-05 20:16Z: bounded partial-worker lifecycle

- [x] Added a node-owned partial-worker service loop. It snapshots the
  explicitly registered worker handles, polls each worker with its configured
  bounded step budget, and flushes independent durable outboxes concurrently.
  Discovery, resource observations and placement plans still cannot create a
  worker or grant writer authority.
- [x] Changed partial-worker dispatch to clone the shared dispatcher under the
  worker mutex and release that mutex before network I/O. A slow destination
  therefore cannot block local biological progress or another network's worker
  loop.
- [x] Started the lifecycle beside the compatibility simulation loop with a
  bounded `NM_STABLE_PARTIAL_WORKER_INTERVAL_MS` interval. The interval is
  scheduling metadata only and never changes logical biological time.
- [x] `cargo check --locked --features stable_executor_live,management_v1
  --bin aarnn_rust`, `cargo test --locked --test stable_shard_transport --
  --test-threads=1` (6 passed), `cargo test --locked --test
  managed_partial_shard_runtime -- --test-threads=1` (1 passed), formatting,
  diff checks and the launcher smoke all pass.
- [!] This lifecycle services already registered partial workers; it does not
  make the legacy layer scheduler authoritative and does not claim physical
  migration success. Live checkpoint transfer, mTLS session identity,
  replicated quorum/fencing, worker input routing and multi-host cutover
  remain production gates.

- [x] Extended the managed partial-worker integration test with a tonic
  receiver. It now proves that a locally generated cross-shard record is
  durably delivered and acknowledged by the registered remote node through
  the automatic service pass; the sender outbox is empty only after the
  receiver's durable acknowledgement.
- [x] Rebuilt both release binaries with the exact launcher profiles and ran
  `timeout --signal=INT --kill-after=5s 15s env AARNN_SKIP_BUILD=1
  AARNN_NATIVE_UI=0 NM_STABLE_PARTIAL_WORKER_INTERVAL_MS=25
  ./run_examples.sh`. The dashboard URL was printed after `/api/config`
  readiness, the orchestrator/nodes stayed free of the bearer-token startup
  error, and the launcher-owned processes were cleaned up. The timeout exit
  was the intentional bounded-stop result.
- [x] `cargo check --locked --all-features --all-targets`, `cargo fmt --all
  -- --check`, `git diff --check` and both launcher shell syntax checks pass.

## Verification update — 2026-09-05 20:28Z: launcher and UI profile recheck

- [x] Re-ran the bounded local launcher smoke with
  `AARNN_SKIP_BUILD=1 AARNN_NATIVE_UI=0 timeout --signal=INT
  --kill-after=5s 12s ./run_examples.sh`. It selected free ports, reached the
  web dashboard readiness endpoint, printed the exact dashboard URL and port,
  and shut down all launcher-owned processes cleanly.
- [x] Confirmed `orchestrator.log`, `node_1.log`, `node_2.log` and
  `webui.log` contain no `NM_MANAGEMENT_BEARER_TOKEN` startup failure.
- [x] Re-ran `cargo check --locked --all-features --all-targets`, the
  `stable_executor_live,management_v1` binary check, the stable transport,
  managed partial-worker and launcher integration tests, `cargo fmt --all --
  --check`, `git diff --check` and both launcher syntax checks. All passed;
  compilation emitted existing dead-code/deprecation warnings only.

## Verification update — 2026-09-05: target-local checkpoint activation boundary

- [x] Added a versioned `StableWorkerCheckpointTransferReference` to stable
  activation commands. It carries transfer, checkpoint, brain, lease,
  partition, plan, payload and manifest identities, and rejects malformed or
  cross-brain references before bootstrap.
- [x] The checkpoint transfer service now publishes an immutable bounded
  manifest receipt beside the checkpoint. Target activation verifies that
  receipt and the immutable checkpoint payload below the target-configured
  `NM_CHECKPOINT_TRANSFER_ROOT`; no source-provided filesystem path is
  accepted.
- [x] Target activation rebases checkpoint, owner, warm, receiver and
  outbound paths to target-local roots. The default worker state root is
  `data/stable-workers/<node-id>` and can be explicitly configured with
  `NM_STABLE_WORKER_STATE_ROOT`. Existing commands without a transfer
  reference retain the reference bootstrap path for compatibility.
- [x] `cargo check --locked --features stable_executor_live --lib`,
  `cargo check --locked --features stable_executor_live,management_v1 --bin
  aarnn_rust`, `cargo check --locked --all-features --all-targets`,
  `cargo test --locked --test checkpoint_transfer -- --test-threads=1`,
  `cargo test --locked --test run_examples_launcher -- --test-threads=1`,
  `cargo fmt --all`, `git diff --check` and `bash -n run_examples.sh` pass.
- [x] A bounded local launcher run with
  `AARNN_NATIVE_UI=0 AARNN_SKIP_BUILD=1 timeout --signal=INT
  --kill-after=5s 15s ./run_examples.sh` reached readiness, printed
  `Web dashboard URL (port 8080): http://127.0.0.1:8080`, avoided the
  `NM_MANAGEMENT_BEARER_TOKEN` startup error and cleaned up its processes.
- [!] The stable migration executor is still not wired to the live production
  executor registry or to a networked transfer/cutover session. The new
  activation reference is the safe target materialisation boundary; physical
  multi-host migration, source pause/drain, live WAL catch-up, mTLS identity,
  replicated quorum authority and chaos/RPO/RTO evidence remain open gates.

## Verification update — 2026-09-05 21:09Z: live checkpoint-transfer admission

- [x] Added `StableExecutorAuthority::checkpoint_store` and
  `StableExecutorDurableBridge::prepare_checkpoint_transfer_source`. These
  expose only the last immutable complete-fabric checkpoint through the
  bounded checkpoint-transfer source; they do not create a second writer or
  alter the source term.
- [x] Extended `StableExecutorMigrationSettings` with an explicit
  `destination_endpoints` map. When configured, it must exactly cover the
  distinct enrolled destination nodes and contain bounded HTTP(S) gRPC
  endpoints. An empty map remains the deterministic in-process reference
  profile and is accepted only for local tests/rehearsals.
- [x] Wired the node-owned live migration executor to transfer one complete
  checkpoint concurrently to each distinct destination before quorum lease
  promotion or placement publication. The sender uses bounded per-target
  channels, verifies the target acknowledgement against the immutable
  activation reference, and fails while leaving the source paused if any
  target rejects or cannot durably publish the checkpoint.
- [x] Added registry tests for duplicate registration, unregistered brains and
  concurrent same-brain migration rejection. Existing brain migration,
  checkpoint-transfer and node-owned live-registration tests remain green.
- [!] This milestone proves target checkpoint materialisation admission, not
  full physical worker activation. The target must still consume the verified
  reference through its explicit activation command and report durable
  registration/heartbeat evidence before production placement publication is
  enabled. Source pause/drain and WAL catch-up, mTLS session identity,
  replicated quorum across hosts, remote runtime activation, rejoin and
  multi-host chaos/RPO/RTO evidence remain required gates.

- [x] `cargo check --locked --all-features --all-targets` passed after the
  live transfer wiring; the build emitted the repository's existing warning
  set only.
- [x] `cargo test --locked --features stable_executor_live,management_v1
  --test checkpoint_transfer -- --test-threads=1` passed all 3 bounded
  transfer tests, `cargo test --locked --test run_examples_launcher --
  --test-threads=1` passed all 3 launcher contract tests, and the live
  migration registration test passed with an actual target tonic service.
- [x] `AARNN_SKIP_BUILD=1 AARNN_NATIVE_UI=0 timeout --signal=INT
  --kill-after=5s 12s ./run_examples.sh` reached readiness, printed the
  selected gRPC ports and dashboard URL, showed no bearer-token startup error,
  and cleaned up launcher-owned processes. `bash -n`, `cargo fmt --all --
  --check` and `git diff --check` passed.

## Verification update — 2026-09-05: Ansible estate reachability

- [x] Reused `/home/pbisaacs/Developer/swarmhpc/swarmhpc/ansible/inventory/hosts.ini`
  and ran `ansible-inventory --graph`; the inventory contains qc00–qc05 and
  sm00/sm01 in the existing Slurm/native-node groups.
- [x] `ansible-playbook -i inventory/hosts.ini --syntax-check
  continuum_tenant_aarnn_site.yml` completed without a syntax error.
- [x] The existing Ansible SSH credentials reached qc00, qc01, qc02, qc03,
  qc04, sm00 and sm01 successfully with the `ping` module. qc05 was
  unreachable (`192.168.1.65`, no route to host), so no deployment or
  destructive action was attempted there.

## Verification update — 2026-09-05 21:40Z: activation persistence and final launcher validation

- [x] `PlacementActivationState::Active` is now a durable terminal state for
  an activation idempotency key. Delayed or duplicate worker outcomes cannot
  regress an active activation, and a terminal `Failed` activation cannot be
  resurrected under the same key. A retry therefore requires a new explicit
  activation request.
- [x] Stable-worker registration is accepted only after validation of the
  brain, network, plan digest, lease/fence, complete plan shard set, exact
  target ownership and committed per-shard application acknowledgements. The
  management callback promotes matching pending/queued activations to
  `Active`; it runs outside heartbeat locks and the persisted registry path
  publishes the transition atomically.
- [x] The persisted registry regression now reopens after promotion and
  verifies that the activation remains `Active` and is absent from the
  retryable command set. The stable heartbeat test filters activation
  commands from unrelated queued commands, so acknowledgement coverage is
  deterministic when multiple control messages coexist.
- [x] `cargo test --locked --all-targets` passed, including the placement,
  management, migration, checkpoint, failover/rejoin, UI and launcher suites.
  `cargo check --locked --all-features --all-targets`, `cargo fmt --all`,
  `git diff --check`, and both launcher shell syntax checks passed.
- [x] A bounded current-laptop run of `run_examples.sh` reached readiness and
  printed the selected endpoints, including `Web dashboard URL (port 8080):
  http://127.0.0.1:8080`. The orchestrator and node logs contained no
  `NM_MANAGEMENT_BEARER_TOKEN` startup failure, and the intentional timeout
  cleanup left no launcher-owned release process.
- [!] The Ansible placement checks remain proposal/admission evidence. The
  target estate currently includes the laptop, qc01–qc04, sm00 and sm01;
  qc00 and qc05 were unavailable in the latest bounded probe. Physical
  multi-process activation, source drain/WAL catch-up, replicated quorum
  fencing, automatic deployed executor registration and chaos/RPO/RTO remain
  required before production placement publication.
- [x] The final Ansible smoke returned `passed` for an eight-shard proposal
  across qc02, qc03, qc04, sm00 and sm01, and for consolidation of all eight
  shards onto sm00 with the expected `degraded_durability: true` admission
  signal. `sm_native_nodes_test.yml` also passed on sm00 and sm01 with
  `changed=0`, confirming the existing GPU/native service lane without
  changing deployment state.

## Verification update — 2026-09-05 21:55Z: prepared placement activation barrier

- [x] Closed the ordering gap in the placement registry. A placement that
  carries stable-worker activation commands is now persisted as a prepared
  record; `active_plan`, shard authorities and the resource version remain on
  the previous committed plan until every attached activation reaches
  `Active` through validated worker registration evidence.
- [x] Added durable `prepare`, `commit_prepared` and `abort_prepared` paths for
  both in-memory and crash-safe registries. Prepared state survives restart,
  failed activation retains audit bindings, and a retry requires a new
  idempotency key. Existing no-activation applies retain immediate committed
  behavior.
- [x] Updated both management service implementations, activation-result
  handling, worker-registration callbacks and automatic placement
  reconciliation to use the same barrier. Failed dispatch aborts the
  prepared intent; competing automatic moves are blocked while activation is
  pending.
- [x] Added registry and management regression coverage for partial target
  activation, atomic final commit, failed activation/abort, retry-key
  handling, and the prepared response contract. Focused placement,
  automation, management, live-registration and heartbeat suites pass.
- [x] Re-ran `cargo fmt --all -- --check`, `cargo test --locked --test
  placement_registry -- --test-threads=1`, the stable automation suite,
  `cargo test --locked --features stable_executor_live,management_v1 --test
  live_migration_registration -- --test-threads=1`, and the stable activation
  heartbeat suite. All passed; `git diff --check` passed.
- [x] Re-ran `AARNN_SKIP_BUILD=1 AARNN_NATIVE_UI=0 timeout --signal=INT
  --kill-after=5s 12s ./run_examples.sh`. It printed
  `Web dashboard URL (port 8080): http://127.0.0.1:8080`, reached dashboard
  readiness without `NM_MANAGEMENT_BEARER_TOKEN` errors, and left no
  launcher-owned release process.
- [!] The production gate remains unchanged: this is a safe reference
  publication barrier, not evidence of replicated quorum fencing, mTLS
  session identity, source drain/WAL catch-up, automatic deployed executor
  registration or multi-host chaos/RPO/RTO success.

## Verification update — 2026-09-05: remote activation is now an explicit migration gate

- [x] Closed the remote-transfer ordering gap in
  `src/migration_executor.rs`. When destination checkpoint-transfer endpoints
  are configured, the executor now collects the verified target-local
  checkpoint references and requires an explicit activation gate before it
  allocates destination leases, publishes placement, or fences the source.
  The gate contract requires the caller to wait for digest-bound target
  activation and durable registration evidence; checkpoint-transfer
  acknowledgement alone cannot claim a live worker.
- [x] The live migration registration test now exercises the gate with the
  actual tonic checkpoint-transfer service and verifies that the request is
  bound to the operation, brain, target plan and target reference.
  In-process migrations continue to use the reference path without a gate.
- [x] `cargo fmt --all -- --check`, the migration executor unit tests, the
  brain migration integration suite and the live migration registration suite
  all pass.
- [!] The activation gate is deliberately an adapter boundary. The remaining
  production work is to bind it to authenticated remote worker registration,
  source drain/WAL catch-up, network quorum fencing, rejoin handling and
  multi-host chaos/RPO/RTO evidence; no production migration flag is enabled
  by this change.

## Verification update — 2026-09-05: launcher and complete validation rerun

- [x] Rebuilt both release binaries with the launcher profiles:
  `aarnn_rust --no-default-features --features 'engine_runtime,ui'` and
  `web_ui --no-default-features --features engine_runtime`. This keeps the
  local example outside the authenticated `management_v1` profile.
- [x] `cargo test --locked --all-targets` passed all library and integration
  suites, including placement, migration, failover/rejoin, UI and launcher
  coverage. `cargo check --locked --all-features --all-targets`, formatting,
  `git diff --check` and both launcher shell syntax checks also passed.
- [x] A fresh-binary launcher smoke run reached dashboard readiness, printed
  the selected gRPC endpoints and `Web dashboard URL (port 8080):
  http://127.0.0.1:8080`, and left no launcher-owned process after bounded
  interrupt cleanup. The orchestrator reached `distributed node ready` and
  contained no `NM_MANAGEMENT_BEARER_TOKEN` startup error.

## Verification update — 2026-09-05: distributed activation gate wiring

- [x] Extended `StableMigrationActivationRequest` and
  `StableExecutorMigrationSettings` with one explicit, digest-bound
  `StableWorkerActivationCommand` per target node. Remote checkpoint transfer
  now binds each immutable target-local reference to its activation command
  before the gate is invoked; missing, extra, conflicting, or wrong-operation
  commands fail closed before destination authority or placement publication.
- [x] Added the orchestrator-owned
  `DistributedNode::stable_migration_activation_gate` adapter. It queues every
  command through the enrolled-node heartbeat path, waits with a bounded
  timeout, validates accepted command results, and requires an authoritative
  stable registration with the target brain/plan generations, lease/fence,
  complete shard inventory, exact active ownership and committed per-shard
  acknowledgements. `register_stable_network_migration_executor` installs
  this gate automatically for endpoint-backed migrations unless an explicit
  adapter is supplied.
- [x] Changed stable registration admission to support multiple workers for
  one immutable plan when ownership is disjoint. Overlapping ownership is
  admitted only with a newer lease and fencing token at an explicitly queued
  activation boundary; incompatible plan identities remain rejected.
- [x] Added `distributed_activation_gate_waits_for_command_and_registration_evidence`,
  which drives the real `spawn_blocking` gate through heartbeat command delivery,
  an accepted digest-bound result and durable registration evidence. Added
  `stable_registration_allows_disjoint_workers_after_queued_activation` for
  multi-worker ownership admission.
- [x] Validation passed:
  `cargo test --locked --features stable_executor_live --test
  live_migration_registration --test brain_migration_session --test
  stable_activation_heartbeat --test placement_registry -- --test-threads=1`,
  `cargo test --locked --features stable_executor_live --all-targets`, and
  `cargo test --locked --all-targets`.
- [!] This binds the migration executor to the in-process authenticated
  heartbeat/session adapter and proves the ordering contract. Physical
  mTLS identity, cross-host causal routing, quorum-issued destination lease
  before target writer activation, source drain/WAL catch-up, automatic
  deployment-time registration of settings/manifests, rejoin handling and
  multi-host chaos/RPO/RTO evidence remain production gates. No production
  migration flag is enabled by this change.

## Verification update — 2026-09-05: aligned both local example launchers

- [x] Updated `run_webcluster.sh` to match the verified local launcher profile:
  it builds with `--no-default-features --features "engine_runtime,ui"`,
  avoids the authenticated `management_v1` service, uses a dynamic web port,
  waits for `/api/config`, and prints the exact dashboard URL and port.
- [x] Added launcher contract assertions for the webcluster profile and
  readiness check in `tests/run_examples_launcher.rs`.
- [x] Rebuilt the exact release profiles with `cargo build --release
  --locked --no-default-features --bin aarnn_rust --features
  'engine_runtime,ui'` and the matching `web_ui` command. Both completed
  successfully with the repository's existing warnings.
- [x] A bounded `AARNN_SKIP_BUILD=1 timeout --kill-after=3s 14s
  ./run_webcluster.sh` run reached dashboard readiness, printed
  `http://127.0.0.1:8080`, reported no management-token/auth startup error,
  and left no launcher-owned process. `bash -n`, `git diff --check` and all
  three `run_examples_launcher` tests passed.
- [!] The exact old banner in the reported log belongs to the separate
  sibling checkout `/home/pbisaacs/Developer/neuralmimicry/neuromorphic_demo`,
  whose legacy launcher still builds `--all-features`; that checkout was not
  modified because this ExecPlan governs `aarnn_rust`. Invoke the launcher
  from this checkout to use the validated profile.

## Verification update — 2026-09-06: current launcher and activation regression rerun

- [x] Re-ran `cargo fmt --all -- --check`, `git diff --check`, `bash -n
  run_examples.sh` and `bash -n run_webcluster.sh` after cleaning the live
  migration fixture warnings.
- [x] Re-ran `cargo test --locked --features stable_executor_live --test
  live_migration_registration -- --test-threads=1`; all four tests passed,
  including the multi-target activation barrier and failed-activation
  preservation of source authority and placement.
- [x] Ran the current release launcher with
  `AARNN_NATIVE_UI=0 AARNN_SKIP_BUILD=1 timeout --kill-after=5s 15s
  ./run_examples.sh`. The orchestrator, both nodes and web dashboard reached
  readiness; the output included `Web dashboard URL (port 8080):
  http://127.0.0.1:8080`; the orchestrator reached `distributed node ready`
  without an `NM_MANAGEMENT_BEARER_TOKEN` error, and bounded cleanup removed
  the launcher-owned processes.
- [x] Repeated the same bounded smoke run with the default native UI enabled
  on the current laptop (`DISPLAY=:0`, `WAYLAND_DISPLAY=wayland-0`). It
  reached the same dashboard banner and reported `Native Rust UI: active
  onscreen`; no management-token startup error or launcher-owned process
  remained after cleanup. Non-fatal snapshot transport warnings can appear
  during the intentional shutdown race.
- [x] Used the existing Ansible inventory at
  `/home/pbisaacs/Developer/swarmhpc/swarmhpc/ansible/inventory/hosts.ini`
  for a read-only SSH capability probe. `qc00`–`qc04`, `sm00` and `sm01`
  accepted `pbisaacs` access; `qc05` was unreachable (`No route to host`).
  This establishes reachable validation targets but does not claim the
  remaining production migration gates.
- [x] Read-only service verification found `aarnn-node` active and enabled,
  listening on `0.0.0.0:50051`, on every reachable native host. `qc00`–`qc04`
  reported version `0.1.27`; `sm00` and `sm01` reported `0.1.25`. This
  confirms the existing Ansible worker estate is live, while the version
  skew and unavailable `qc05` remain deployment concerns for a later
  production cutover.
- [!] The production gate remains: deployment-controlled executor/manifest
  registration, authenticated physical mTLS session identity, cross-host
  causal routing, quorum-issued destination authority before writer
  activation, source drain/WAL catch-up, rejoin/reclaim handling and
  multi-host chaos/RPO/RTO evidence still require implementation and
  validation before production migration is enabled.

## Progress update — 2026-09-06: deployment-controlled migration registration

- [x] Added the bounded, versioned `StableMigrationDeploymentManifest` in
  `src/migration_executor.rs`. It validates the consistent cut, target plan,
  quorum membership and distinct replica paths, exact destination-node/
  endpoint/activation-command sets, source fencing tokens, payload bounds and
  command identity before opening any authority or placement files.
- [x] Added `--stable-migration-spec` and allowed an orchestrator to open an
  explicit `--stable-runtime-manifest`. Startup loads the deployment manifest
  on a blocking task, constructs the replicated authority and persisted
  placement registry, then registers the existing hosted runtime with the
  node-owned migration dispatcher. Missing features, wrong role, missing
  runtime, network mismatch and invalid manifests fail closed.
- [x] Added `tests/migration_deployment_manifest.rs` covering successful
  persistent settings construction, target-set mismatch before filesystem
  creation, bounded versioned loading, and schema rejection. The suite passes
  under `stable_executor_live`; the management-enabled binary also passes
  `cargo check`.
- [x] Extended the existing SwarmHPC Ansible role with an opt-in
  `continuum_tenant_aarnn_stable_migration_enable` profile. It builds the
  `stable-orchestrator` and `stable-node` workload images with the
  `stable_runtime_workload` feature, mounts deployment-supplied runtime and
  migration JSON plus durable state, passes both stable manifest flags, and
  requires a bearer token, management principal, existing mTLS Secret and
  mTLS domain before rendering the profile. The default role path remains
  unchanged.
- [x] Added stable workload metadata to `scripts/container_workloads.sh`, the
  `stable_runtime_workload` Cargo feature, and the container entrypoint. The
  existing Ansible playbook passes syntax validation and the workload profile
  compiles with `cargo check --locked --no-default-features --features
  stable_runtime_workload --bin aarnn_rust`.
- [!] The opt-in profile remains a reference deployment boundary until the
  supplied manifests and mTLS credentials are populated and exercised on the
  cluster. Physical mTLS session identity, remote causal routing, source
  drain/WAL catch-up, quorum network consensus and multi-host RPO/RTO evidence
  remain open.
- [x] Documented the Ansible profile boundary and its fail-closed prerequisites
  in `docs/production-blocker-runbook.md` so operators cannot mistake the
  local launcher or default role deployment for production migration evidence.
- [x] Final validation after the profile ordering/path checks: Ansible
  playbook syntax check, Jinja template parse, shell syntax checks, Cargo
  formatting, `git diff --check`, stable workload profile tests, launcher
  contract tests, stable migration manifest tests and the stable workload
  feature check all passed. Existing compiler dead-code warnings remain.

## Verification update — 2026-09-06: target-term activation and example dashboard

- [x] Corrected partial-worker migration bootstrap so the immutable checkpoint
  retains its source lease term as provenance while the target runtime binds
  execution, receiver fencing and registration to the newer target placement
  term. A target term regression is rejected before any worker state opens.
- [x] Added target-local activation roots through
  `DistributedNode::activate_stable_worker_with_roots`; transferred manifests
  no longer depend on source filesystem paths. The real target activation test
  transfers a checkpoint over the tonic service, delivers activation by
  heartbeat, verifies durable registration and shard acknowledgements, then
  reopens and retries the same activation after a simulated target restart.
- [x] The focused migration suite passed 21 tests with
  `cargo test --locked --features stable_executor_live --test
  live_migration_registration --test migration_transfer --test
  brain_migration_session --test migration_executor --test
  managed_stable_executor -- --test-threads=1`. The bootstrap suite passed 8
  tests, including persisted partial-worker reopen and activation idempotency.
- [x] The current laptop smoke-tested `run_examples.sh` using the release
  binaries. The local launcher excludes authenticated `management_v1`, waits
  for `/api/config`, prints the selected gRPC and web ports, and reached
  `http://127.0.0.1:8080` without an `NM_MANAGEMENT_BEARER_TOKEN` error. The
  intentional timeout cleanup removed all launcher-owned processes and ports.
- [!] These results remain reference and local-process evidence. Network
  quorum/election, authenticated physical identity, deployed worker
  registration, source drain/WAL catch-up over physical hosts, reverse
  reclaim, and measured multi-host RPO/RTO remain open promotion gates.

## Progress update — 2026-09-06 00:30Z: authenticated control-plane node sessions

- [x] Live worker join and heartbeat requests now carry and validate the
  node-ID/token claim against the mTLS leaf certificate fingerprint before the
  orchestrator accepts membership, resource observations, stable registrations
  or migration command results.
- [x] Client reconnect and heartbeat requests use the same deployment-managed
  credential binding. Missing, mismatched or malformed live credentials fail
  closed; the local reference profile remains plaintext-compatible behind its
  explicit disabled-live flag.
- [x] Added a reusable metadata attachment helper and regression coverage while
  retaining separate authentication boundaries for causal streams, checkpoint
  transfer and stable-shard transport.
- [!] Physical certificate provisioning, deployed identity rotation/revocation,
  network consensus/election, and multi-host migration/failure evidence remain
  open gates.

## Verification update — 2026-09-06: peer RPC identity binding

- [x] Extended live node-session metadata to all remaining outgoing peer paths
  used by shard snapshots, consistent-cut assembly, GA forwarding and legacy
  spike streams. The same local node identity is attached to every request;
  credential lookup failure aborts the request before network transmission.
- [x] Added live receiver checks for the snapshot and GA methods so a valid
  protobuf payload alone cannot bypass per-node token and certificate
  fingerprint validation. Reference-mode tests continue to exercise the
  compatibility path with live transport disabled.
- [x] Five node-auth tests, 33 distributed tests and the stable-executor live
  feature check passed.
- [!] SwarmHPC deployment identity remains an external gate: the current
  Kubernetes template gives worker pods generated identities and does not yet
  project a per-pod credential/fingerprint binding. It must be solved through
  an explicit identity provider or stable secret projection before live causal
  transport or production migration cutover is enabled.

## Verification update — 2026-09-06: Ansible resource and consolidation probes

- [x] The existing Ansible read-only probe collected current host capacity and
  produced a bounded seven-shard placement over five explicitly authorised
  enrolled hosts. It excluded unreachable inventory entries and did not grant
  compute authority from reachability alone.
- [x] The same live inventory rejected a request to co-locate all shards on
  `sm00` while requiring a distinct warm replica. With the explicit
  single-host degraded-durability allowance, the proposal co-located every
  shard on `sm00` and remained unapplied. This verifies the requested
  laptop-style consolidation tradeoff is explicit and auditable.
- [x] The existing Ansible AARNN site passed syntax validation. These are
  proposal and read-only host observations; they do not constitute physical
  shard execution, migration, failover or RPO/RTO evidence.

## Verification update — 2026-09-06 00:54Z: stable deployment identity wiring

- [x] The canonical SwarmHPC role now gives the orchestrator an explicit
  stable node ID and derives each daemonset worker ID from its Kubernetes host
  name. This preserves the writer identity across a pod restart on the same
  host and avoids using ephemeral pod names for fencing.
- [x] The stable migration role now rejects deployment-mode workers and
  missing identity-source/orchestrator settings before rendering. The local
  container entrypoint forwards an explicitly supplied `AARNN_NODE_ID`
  without inventing a second identity source.
- [x] `scripts/qa/validate-ansible-stable-profile.py` passed the canonical
  role contract and the read-only
  `continuum_tenant_aarnn_site.yml --syntax-check`; the wrapper identity
  smoke passed as well.
- [!] This is a deployment identity prerequisite, not production cutover
  evidence. Per-node credential and mTLS leaf projection/rotation, quorum
  authority, physical causal routing, source drain/catch-up and failure/RPO/RTO
  scenarios remain gated.

## Verification update — 2026-09-06: final launcher and provider-contract validation

- [x] Revalidated the canonical external SwarmHPC role after adding the
  node-local identity-provider projection. The stable profile validator and
  `continuum_tenant_aarnn_site.yml --syntax-check` both passed. The
  orchestrator loads an explicit node identity, while both engine workload
  variants derive their node identity from `spec.nodeName` and load their
  provider-mounted token and certificate-fingerprint files.
- [x] Passed `cargo fmt --all -- --check`, the repository `git diff --check`,
  shell syntax checks, the stable workload/container contract tests and the
  stable migration deployment-manifest tests.
- [x] Passed `cargo test --locked --all-targets`, including 281 library tests,
  migration, placement, failover, management, mobile, browser and launcher
  suites. Compiler dead-code warnings remain in the existing broad workspace,
  but no test failed.
- [x] Bounded `run_examples.sh` smoke validation started the orchestrator,
  both distributed nodes and the web dashboard, served `/api/config`, printed
  the selected gRPC and dashboard URL/port, and cleaned up all launcher-owned
  processes after shutdown. The local launcher no longer enables the
  authenticated `management_v1` profile, so it does not require
  `NM_MANAGEMENT_BEARER_TOKEN`.
- [x] Re-ran the read-only Ansible placement probe. Reachable hosts were
  `localhost`, `qc01`–`qc04`, `sm00` and `sm01`; `qc00` and `qc05` were
  excluded. The proposal used only explicitly granted enrolled compute hosts,
  returned `degraded_durability: false`, and remained unapplied.
- [!] Production migration remains gated. The provider files and unique
  per-host certificates/tokens still require real provisioning and rotation;
  quorum/election, physical cross-host causal routing, source drain/WAL
  catch-up, reclaim/reverse consolidation and measured multi-host RPO/RTO
  evidence are still required before enabling the stable migration profile.

## Verification update — 2026-09-06: target-local checkpoint binding

- [x] Extended `PlacementAutomationSpec` with an optional, explicitly supplied
  target-node map of `StableWorkerCheckpointTransferReference` values. Every
  receipt is bounded and validated against the runtime brain, immutable
  checkpoint, partition generation and source plan digest before an activation
  command is created.
- [x] Activation commands now carry the target-local receipt when one is
  available, so remote workers resolve checkpoint bytes below their own
  transfer root instead of inheriting source filesystem paths. The target
  still verifies the receipt and checkpoint before opening the worker.
- [x] Added `activation_binds_a_validated_target_local_checkpoint_receipt` to
  `tests/placement_automation.rs`; the focused automation suite passes all
  four tests. This closes the command/schema seam while leaving the actual
  physical transfer, credential provisioning and cutover authority behind
  their documented production gates.
