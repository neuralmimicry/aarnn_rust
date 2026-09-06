# Production blocker runbook

This document is a living cross-review of the implementation against
`docs/specifications/distributed-whole-brain-emulator-v1.1.md`. It describes the
work and evidence required before any migration flag can become a production
default. Reference and opt-in tests are not production acceptance evidence.

## Current boundary

The following reference slices are present and tested: deterministic identities
and logical time, local causal settlement, SCC/ownership planning, bounded
causal stream semantics, independent-brain dispatch, fenced WAL/checkpoint
primitives, default-deny management policy, and governed peripheral/effect
state. The migration flags remain disabled by default:

`superdense_executor`, `virtual_partitioning`, `causal_transport`,
`multi_brain_scheduler`, `replicated_durability`, `management_v1` and
`workstation_io`.

The existing SwarmHPC Ansible role now has a separate, disabled-by-default
`continuum_tenant_aarnn_stable_migration_enable` profile. When explicitly
enabled, it builds `stable-orchestrator` and `stable-node` images with
`stable_runtime_workload`, mounts operator-supplied runtime and migration
manifests plus the durable PVC, and requires an existing gRPC mTLS Secret,
management principal and bearer token before rendering. The manifests are
deployment input and are never synthesized from discovery. This profile is a
reference deployment boundary until its credentials, manifests, physical
worker registration and multi-host acceptance evidence are complete; the
normal Ansible deployment and local launchers do not enable it.

The production consumers still include `Runner::step`, layer-range assignment,
legacy `SpikeBatch`, JSON workspace persistence, direct UI/runtime management,
and non-consensus file leases. These are rollback paths until the gates below
are met. The workspace has deterministic local process/restart tests, but no
physical multi-host chaos run, network consensus deployment, live device,
browser automation run, or scientific reference dataset.

As of 2026-08-30 19:06Z, host verification, the available QA matrix, all
catalogued host examples, generated-management freshness checks, and persisted
consistent-cut epoch/restart tests pass. `NM_CONSISTENT_CUT_ROOT` is an
explicit opt-in for durable cut evidence. The `replicated_durability` profile
also wraps the live managed-network step with a durable owner, but this is
still a compatibility-Runner integration seam, not the complete shard-owned
causal executor required for production. The external production gates below
remain open.

As of 2026-08-31, durable live snapshot projections now read from the durable
owner rather than a read-ahead `Runner` projection, and a warm replica repairs
an uncommitted WAL suffix after a crash between record fsync and active
publication. Replicated authority configuration now accepts an explicit
`NM_AUTHORITY_REPLICAS=member=path,...` set and rejects member-set drift. These
changes strengthen the local failure boundary; they do not turn filesystem
replication into network consensus or complete the live causal-stream cutover.

The live causal profile is now fail-closed at startup unless it has a durable
owner root, a distinct warm-replica root, at least three explicitly configured
authority replicas, mutual TLS, per-node credentials, and a SHA-256 allow-list
binding each declared sender ID to its mTLS leaf certificate. Once enabled, the
legacy `SpikeBatch` RPC and MPI receiver are rejected so one boundary cannot be
admitted through two ordering/deduplication domains. The certificate and
authority checks are deployment guards; they do not make the filesystem
authority a network consensus implementation.

The latest verification also closes the managed commit-intent replay gap: the
biological snapshot, channel projection and destination outbox frontiers are
now recovered from one durable intent, and recovery requires an exact WAL and
outbox frontier match. A missing outbox suffix is appended exactly once rather
than being treated as an empty successful prefix. Default and all-feature
builds, the workspace tests, the available QA matrix, EX-012 failover/rejoin,
all catalogued host examples and generated binding checks pass. This remains
repository-local evidence; it does not upgrade the filesystem adapter to a
network consensus system or supply physical multi-host evidence.

The 2026-08-31 verification pass also rejects recovery evidence that does not
prove stale-writer fencing, and verifies that duplicate live causal ingress
does not mutate biological bytes, create a second receipt or rewrite the
durable channel projection. The all-feature workspace, generated bindings,
available QA matrix, catalogued host examples and warning-tolerant Clippy
remain green; the iOS lane is explicitly `not-run` because no Xcode project is
present. These tests strengthen the local proof boundary but do not close the
external production gates below.

## Blocker closure sequence

### 1. Device/OpenCL equivalence

1. Make the pure CPU transition the versioned numerical oracle, including all
   supported neuron models, refractory/adaptive state, STP, synaptic
   accumulation, plasticity, morphology and output staging.
2. Add bounded vectors for normal, boundary, non-finite, overflow and empty
   inputs. Compare every returned state, spike, proposal and digest, not only a
   selected voltage vector.
3. Run the vectors on every supported OpenCL/CUDA architecture and driver
   profile, with declared tolerances and compiler/profile identifiers. A mismatch
   must reject that profile and retain CPU execution.
4. Run long deterministic replays with one/max CPU threads, batching and device
   execution; compare committed event sequence and digest.
5. Publish the hardware matrix, tolerances and performance results. Only then
   may a device profile be admitted to `multi_brain_scheduler` or workstation
   production.

Current code closes only the bounded LIF, sanitised Izhikevich and STP
initialisation check in `src/cl_compute.rs`. The feature-only OpenCL build was
fixed after three sparse-map fallback locals were found with inconsistent
names. The opt-in local probe passed three repeated runs, but one preceding
same-host run rejected STP (`actual=0`, `expected=0.2774`), so a stable profile
certification is not claimed. Hardware matrix, long replay, remaining kernels
and scientific adequacy remain open.

### 2. Shard-owned biological state

1. Define a serialisable `ShardState` containing every authoritative terminal,
   synapse, weight, delay, release, plasticity trace, neuron state, future-event
   queue, dedupe window, watermark and topology/partition generation.
2. Bind each object to exactly one stable-ID owner in the compiled execution
   plan. Dense indices may be local caches only and must never be protocol or
   persistence identity.
3. Replace direct `Runner` mutation with prepare/apply/commit deltas. A shard
   actor owns the state and emits causal events/proposals; another shard or
   worker cannot mutate its vectors.
4. Make growth and morphology produce validated topology transactions. Publish
   one immutable generation at a safe logical boundary and drain, translate or
   replay old-generation events explicitly.
5. Integrate shard state with causal settlement, WAL, checkpoints and recovery,
   then prove duplicate/missing/stale-owner rejection under replay.
6. Re-run the direct-runner search. The gate is not met while production
   execution still owns biological state in the monolithic runner.

The current `src/topology_model.rs` is a planner/reference contract; it is not
yet the owner of the biological state used by `src/runner.rs`.

### 3. Causal gRPC cutover

1. Extend the authoritative protobuf with a versioned causal stream RPC whose
   request/response carries schema version, brain/stream identity, sequence,
   term, route, generations, logical tag, stage, event ID and payload.
2. Regenerate Rust client/server bindings from the `.proto` source in the build
   and fail CI if generated output or golden wire fixtures drift.
3. Implement a transport adapter that maps generated messages to the causal
   data-plane types without losing stage, identity or logical time. Validate
   terms, generations, sequence, size and credits before queue mutation.
4. Add a server/client integration test with reorder, duplication, reconnect,
   credit exhaustion, stale term/generation and corruption cases.
5. Switch one brain at a generation boundary under `causal_transport`; do not
   send the same authoritative event through both `SpikeBatch` and causal gRPC.
6. Measure multi-process fault behaviour and retain the legacy bridge only for
   the declared rollback window.

`proto/distributed.proto` now has an additive envelope plus generated durable
and authoritative causal services. The authoritative service applies through
the shard owner before acknowledging, and integration tests cover replay and
fencing. It is not yet wired as the live `ManagedNetwork` inter-process
exchange: production still uses `StreamSpikes(SpikeBatch)`, so no
generation-boundary causal gRPC cutover evidence exists.

The live node now also exposes the generated `CausalDataPlane` service. With
`NM_CAUSAL_TRANSPORT_LIVE=1`, layer-boundary batches are encoded as bounded
versioned causal ingress records and are sent exclusively through that service;
the receiver admits them to the durable receipt/WAL/warm-replica boundary
before acknowledging. The sender now persists a bounded, digest-verified
per-peer outbox and link-scoped sequence/event identity, so reconnect and
process restart retransmit the unacknowledged prefix without reusing another
peer's cursor. This closes the repository retry/replay seam for a configured
single-process sender. The flag must remain disabled for production until the
outbox is included in the same distributed commit transaction as the
biological step, TLS/mTLS transport identity, network quorum authority and a
generation-boundary multi-process fault run are accepted.

The live profile also rejects legacy `StreamSpikes` ingress and validates the
presented mTLS leaf certificate against
`NM_CAUSAL_NODE_CERT_SHA256=node=sha256hex` entries. Required live settings are
`NM_DURABLE_SHARD_ROOT`, a distinct `NM_WARM_REPLICA_ROOT`,
`NM_AUTHORITY_MEMBERS`, `NM_AUTHORITY_REPLICAS`, `NM_GRPC_TLS_CERT`,
`NM_GRPC_TLS_KEY`, `NM_GRPC_TLS_CA`, `NM_CAUSAL_NODE_TOKEN`,
`NM_CAUSAL_NODE_TOKENS`, and `NM_CAUSAL_NODE_CERT_SHA256`. This prevents
accidental startup in a partially secured mixed-transport mode; it is still
not a substitute for a mature network consensus provider.

`NM_PRODUCTION_CUTOVER=1` is now a single fail-closed startup gate for both
distributed nodes and the web gateway. It requires the live causal profile,
compiled migration profiles, mTLS/domain, OIDC JWT verification with a
revocation source, durable management state, a stable node identity and
secure web session cookies. This prevents an operator from enabling a
production label while the process is still using the legacy transport or
local/none web authentication. It does not manufacture the missing
consensus, shard-owned executor, audit sink, physical failure-domain or
provider/device evidence.

Management startup also has an explicit `NM_PRODUCTION_CUTOVER=1` guard. In
that mode static bearer authentication is rejected; OIDC JWT settings and a
durable revocation-list path (`NM_OIDC_REVOCATION_FILE`) are mandatory, and a
revoked subject or JWT ID is denied on every request. This improves the local
cutover boundary. Startup now also requires the JWKS and revocation paths to
be readable regular files, and rejects malformed or empty JWKS documents before
the listener is exposed. PKCE issuance/refresh, workload identity
provisioning, replicated audit delivery and live operator evidence still
belong to the deployment integration gate.

### 4. Durable distributed recovery

1. Define WAL records for accepted inputs, transitions/proposals, outgoing
   commitments, acknowledgements, watermarks, topology changes, scheduler and
   numerical decisions, non-convergence, leases, migrations and external
   effect dedupe.
2. Add checksums/hash-chain or manifest verification and fail closed on
   truncation, corruption or unsupported schema.
3. Checkpoint the complete shard cut: state, queues, in-transit channel
   evidence, dedupe, producer horizons, generations, profile and resume
   positions. Publish immutable manifests atomically.
4. Synchronously replicate the configured WAL commit boundary to an anti-affine
   warm backup. Expose durable-log and applied tags separately; pause commits
   when the policy cannot meet its RPO.
5. Recover by selecting a valid cut, replaying deterministically, suppressing
   already committed effects and quarantining digest/stream mismatches.
6. Exercise active loss, backup loss, corrupt newest checkpoint, partial upload,
   migration before/after cutover and quota exhaustion in a controlled
   multi-process harness. Record measured RPO/RTO.

`src/durability.rs` now also supplies a staged `DurableShard` apply/commit
boundary, verified checkpoint restore and process-shared warm-replica
publication. `src/managed_durability.rs` connects an opt-in durable owner to
the live managed-network step and its snapshot projection; replacement-owner
tests verify stable identity, newer-term promotion, stale-writer rejection and
machine-verifiable local RPO/RTO evidence. Live snapshot and workspace
projections use that owner when the durable profile is enabled, and the warm
repair test covers the pre-publication crash window. The live sender also has a
digest-verified durable causal outbox with per-peer acknowledgement cursors.
The managed commit-intent record now covers the biological snapshot, channel
state and outgoing outbox reservation/replay boundary, including crash recovery
of a missing outbox suffix. The complete stable-ID shard-owned biological
state, networked active/warm orchestration and physical failure evidence remain
absent; the intent is a crash-recovery transaction marker, not distributed
consensus.

### 5. Consensus fencing

1. Select and approve a mature quorum implementation and failure-domain
   membership; do not promote based on reachability or a heartbeat alone.
2. Store membership, placement/topology generations, operation state and
   monotonically increasing lease terms/fencing tokens in the quorum.
3. Validate the token at event admission, causal send/receive, WAL append,
   checkpoint publication, command handling and effect gateway. Reject stale
   terms even when an old process is still running.
4. Implement Joining/Benchmarking/Healthy/Draining/Suspect/Failed/Recovering/
   Quarantined lifecycle states and quorum-observed expiry.
5. Test leader loss, partition, delayed old leader, quorum loss and restart;
   prove one owner and no duplicate external effect.
6. Enable `management_v1` or automatic failover only after the chaos and
   security evidence is archived.

`QuorumLeaseAuthority` and `ReplicatedQuorumLeaseAuthority` now provide
deterministic majority tests, durable local authority documents, monotonic
replacement terms and stale-token rejection. The latter is a filesystem-local
replication adapter, not a network consensus protocol; production promotion
remains forbidden until mature quorum implementation and multi-process
partition/fencing evidence exist. Its explicit-availability constructor now
fails closed on reopen when fewer than a majority of declared members is
available; this is test/deployment input, not failure detection or leader
election.

### 6. Generated management clients

1. Create one authoritative versioned management schema covering resources,
   operations, status, errors, permissions, idempotency and optimistic
   concurrency.
2. Generate the Rust client/server and browser client from that schema; keep
   OpenAPI/JSON only as a generated compatibility projection if required.
3. Route web UI, Rust UI, CLI and native workstation control through the
   generated client. Remove direct worker/runtime mutation and verify server
   authorisation independently of UI visibility.
4. Integrate OIDC/PKCE for users, mTLS/workload identity for workers, CSRF and
   token refresh/revocation, redaction and audit correlation.
5. Run API concurrency, retry/idempotency, forged-worker, tenant isolation,
   stale leader, export expiry and audit-failure tests against a live server.

The versioned management schema now generates the Rust service boundary and
checked browser and Android path clients, and integration tests exercise the
Rust client/server contract, persisted operations, authentication failure,
read authorisation and principal scoping. Rust UI/runtime legacy paths,
OIDC/PKCE, workload identity/mTLS, durable audit and a complete end-to-end
management cutover remain open.

### 7. Live browser/native USB AER I/O

1. Define and maintain USB AER framing, length/checksum/CRC, descriptor and
   capability negotiation, device epoch and hot-plug state machine.
2. Implement a native asynchronous adapter with bounded transfer pools,
   cancellation-safe shutdown, watchdog, allow-list and local-device binding.
3. Implement browser-safe capture/presentation using supported browser APIs;
   browser code must not claim global HID actuation. Keep native HID output as a
   separately reviewed capability.
4. Give USB AER input/output, audio, video and HID independent identity,
   sequence, clock mapping, credit budget and failure state. Test saturation and
   reconnect without starving another modality.
5. Admit samples through the peripheral service with capture time, mapping
   version and uncertainty; never write raw frames directly to a shard buffer.
6. Run browser automation, native emulator, device removal/reconnect and
   physical hardware timing tests before enabling `workstation_io`.

`src/peripheral.rs` provides governance and safety references only. There is no
maintained live USB implementation, browser automation runner or physical AER
device evidence in this workspace.

### 8. Federation

1. Add a separate federation link contract with source identity, authorisation,
   clock mapping, positive minimum delay, sequence/credits and bounded payloads.
2. Admit remote inputs as peripheral/federation samples with capture provenance;
   keep them separate from internal shard transport.
3. Reject zero-delay cross-brain cycles unless a separately approved component
   design proves closure and ownership. Apply revocation and dual authorisation
   at both brains.
4. Test backpressure, source failure, clock discontinuity, replay/dedupe,
   revocation and independent-brain progress in a multi-process harness.

`src/federation.rs` now provides a bounded positive-delay offer/consent/link
reference with deduplication, credits and revocation tests. No live federation
directory/service or multi-process acceptance evidence currently exists.

### 9. Scientific validation

1. Name the biological and transducer reference datasets, provenance, units,
   parameter versions, preprocessing and licensing.
2. Define numerical metrics, tolerances, confidence intervals and failure
   interpretation separately for CPU/device equivalence and biological adequacy.
3. Compare neuron, synapse, plasticity, growth and transducer outputs on fixed
   replay traces, including perturbation sensitivity and clock/jitter effects.
4. Publish reproducible reports and raw result hashes. A matching digest proves
   determinism only; it does not prove biological validity.

No approved scientific reference dataset or validation report is present.

### 10. Migration evidence

1. Freeze a pre-migration immutable checkpoint and record protocol, topology,
   partition, numerical and transducer versions.
2. Rehearse restore and rollback with representative workloads, delayed/lost
   messages, node failure, schema mismatch and external-effect suppression.
3. Canary one isolated brain in recorded-input/sandbox-output mode; compare
   committed digest/event sequence and operational SLOs against the legacy
   reference.
4. Promote by brain/generation boundary with an explicit operator decision,
   retain the rollback source for the stated window and document the point of no
   return.
5. Repeat for live input, failover, federation and every supported hardware
   profile. Record signed evidence before changing defaults.

No migration rehearsal or production deployment evidence is available here.

### 11. Legacy-path removal

Remove `Runner::step`, layer ownership, `SpikeBatch`, JSON-only persistence,
direct worker management and temporary flags only after blockers 1–10 pass and
all persisted brains are migrated. Before deletion, run repository-wide symbol
search, compatibility restore, rollback and acceptance suites. Removal is
irreversible at the code level, so the last supported rollback must be the new
durable path, not a legacy binary.

## Cross-review conclusion

The current repository is suitable for continued reference/opt-in development
and deterministic local verification. It is not eligible for production
cutover. The blocking items requiring external evidence are hardware/driver
matrices, quorum and multi-process chaos, browser/native USB execution, physical
device timing, scientific datasets/reports and migration rehearsal. Code-only
work can close the ownership, schema, adapter and integration seams, but those
seams must not be labelled production-complete until the corresponding evidence
exists.

## Latest repository-only ownership slice — 2026-08-31

`src/authoritative_shard.rs` now contains a versioned `StableBiologicalState`
reference kernel with stable neuron/synapse ownership, explicit release and
plasticity state, logically tagged future events, deterministic serialisation
and digest verification. `AuthoritativeShard::apply_stable_event` and the
explicit stable authoritative gRPC constructor publish transitions only after
the existing durable receipt/WAL/warm boundary succeeds. Focused authoritative
and generated-causal tests pass.

This does not authorise cutover. The live `ManagedNetwork` model remains on its
compatibility `Runner` path until the full supported biology and topology are
mapped to stable shard state and parity-tested. The quorum adapter is still
filesystem-local, and physical failure-domain, OIDC/PKCE, workload identity,
durable audit and operator evidence are still required.

## Stable multi-shard reference fabric — 2026-09-05

`src/shard_executor.rs` now provides a transport-neutral, deterministic
multi-shard reference executor. It creates one stable-ID biological state per
planned virtual shard, validates cross-shard routes and generations, orders
events canonically, advances same-time and positive-delay logical tags, keeps
bounded pending and deduplication windows, and emits deterministic child event
IDs. A failed transition or output admission restores the complete executor
state transactionally. `StableBiologicalState` retains generation-wide neuron
identity so a shard-owned synapse can safely reference a remote neuron, and
split ownership of terminal/weight/release/plasticity fields is rejected before
execution.

Unit and integration tests cover cross-shard routing, duplicate/conflicting
delivery, canonical admission order, split ownership refusal, remote-endpoint
state round-trip and queue-overflow rollback. This closes a local deterministic
execution seam; it does not make the compatibility `Runner` path authoritative,
provide network consensus, or prove physical migration, RPO/RTO, peripheral
handoff or scientific parity. Wiring the fabric into durable
`AuthoritativeShard` actors and generation-boundary migration remains the next
shard-owned-state gate.

The stable executor can now emit per-shard checkpoint envelopes compatible with
the existing `ShardState` and transfer protocol. A migration integration test
transfers all shard envelopes out of order, promotes them under a new lease
term, and restores the fabric with the original digest. This is repository-local
reference evidence; it does not yet prove that live WAL append, output routing,
quorum authority or the compatibility `ManagedNetwork` path use those stable
executor envelopes.

`StableExecutorCheckpointStore` also publishes a bounded complete sibling set
through the existing atomic immutable filesystem checkpoint primitive and
verifies it after reopen. It remains a durable reference adapter until the
multi-shard executor is attached to live WAL/output actors and quorum-backed
authority.

As of 2026-09-05, the stable executor also has a fenced transactional authority
and an immutable complete-fabric checkpoint store. Every admitted step is
published only after the complete sibling checkpoint set has been sealed; a
publication failure restores the executor, including pending work and
deduplication state. Reopen verifies the outer manifest, set digest, plan and
generation identity, every shard digest, and the restored whole-fabric digest.
The focused suites and all-target workspace tests pass, including the
out-of-order transfer, new-term promotion, immutable-reopen and rollback
cases. This remains a reference/migration adapter: `AuthoritativeShard` and
the live `ManagedNetwork` path do not yet use the stable executor as their
multi-shard biological owner, and the filesystem checkpoint is not network
consensus.

The next gate is a durable actor bridge that commits stable executor state,
causal output and per-shard WAL/receipt state at one generation boundary. It
must prove restart, duplicate delivery, failed publication rollback, source
fencing and destination catch-up with the same digest before any executor
selection flag is enabled. The bridge must also retain peripheral provenance,
effect fencing and complete-cut evidence; a stable biological checkpoint alone
cannot satisfy those requirements.

The first reusable handoff for that gate is now present in
`src/stable_executor_durable.rs`. It publishes one complete immutable cut,
then records the resulting stable checkpoint through every
`AuthoritativeShard` actor. A failed mirror leaves a resumable pending
operation; retry reuses the same causal sequence and exact expected pre-cut
digests, so already published actors are durable duplicates rather than a
second neural execution. The public bridge test proves that all shard mirrors
contain the same post-step checkpoint and one durable receipt. This remains a
local reference coordinator: it intentionally does not claim an atomic
network transaction across actors or replace quorum consensus.

The bridge now also exposes `prepare_transfer_sources`, which converts the
verified durable actor state into bounded `ShardTransferSource` values in
stable shard order. The accompanying fault-injection test removes one warm
mirror after cut publication, verifies that the bridge retains the pending
operation and the successful sibling, retries without a second neural
execution, and reconstructs all destinations under a newer lease term. The
orchestrator migration journal and placement registry remain the authority for
operation progress and cutover publication.

The repository now also contains `brain_migration_session.rs`, which composes
that source boundary into a complete two-phase reference handoff: bounded
frames are reassembled and verified, all destination actors are materialised
under a newer term, the brain-wide group is prepared, and the target placement
registry is published before the migration journal records `Committed`. The
focused `brain_migration_session` test covers this path with two shards and
real stable-bridge sources. This evidence remains local/reference evidence;
destination leases still require replicated authority, and peripheral/effect
cursor handoff plus physical multi-host RPO/RTO evidence remain blockers.

## 2026-09-05 stable worker registration status

Stable executor workers now advertise a versioned, bounded registration during
join and heartbeat. The orchestrator validates the topology/partition digest,
stable shard set, logical frontier, lease/fencing observation and local poll
budgets, and rejects duplicate network owners or plan identity changes outside
an explicit migration transaction. Stable networks remain fenced from the
legacy layer rebalancer and legacy load/unload commands after registration.

This closes the registration and legacy-scheduler isolation seam only. It does
not establish authenticated workload identity, network consensus, quorum lease
authority, remote causal shard routing, deployed migration executor dispatch,
physical failover, or measured RPO/RTO. Those gates remain required before
enabling stable multi-host execution or production cutover.

The registration contract is schema/profile v2 and carries both the complete
immutable `shard_ids` plan inventory and the worker-local `owned_shard_ids`
subset. The subset is bounded, sorted, non-empty and must belong to the plan;
it is excluded from plan identity so an authorised migration boundary can
update ownership telemetry. The deployed stable executor still reports the
complete fabric in both fields, and the orchestrator still admits only one
stable worker per network. A change to the subset is rejected unless the
observation also carries both a newer lease term and a newer fencing token.
