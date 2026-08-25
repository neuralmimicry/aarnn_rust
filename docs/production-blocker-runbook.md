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

The production consumers still include `Runner::step`, layer-range assignment,
legacy `SpikeBatch`, JSON workspace persistence, direct UI/runtime management,
and non-consensus file leases. These are rollback paths until the gates below
are met. No live device, browser automation, multi-process cluster, quorum
deployment, or scientific reference dataset is available in this workspace.

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

`proto/distributed.proto` now has an additive envelope and
`CausalValidationService` provides a bounded generated-tonic validation/echo
seam. It does not apply events to shard state or publish durable receipts. The
legacy `StreamSpikes(SpikeBatch)` service remains the production exchange and
no causal gRPC cutover evidence exists.

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

`src/durability.rs` currently supplies fenced in-memory/filesystem reference
primitives. Runtime JSON snapshots do not contain the complete causal cut and
there is no active/warm orchestration or measured failure evidence.

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

The management reference now fences operation transitions by leader term, but
`src/management.rs` is not a replicated consensus authority.

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

The current policy/orchestrator is a reference Rust type; browser and runtime
paths still use legacy management contracts and no generated management client
is consumed end to end.

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

No federation service or acceptance evidence currently exists; the positive
delay rule is only recorded as a design decision.

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
