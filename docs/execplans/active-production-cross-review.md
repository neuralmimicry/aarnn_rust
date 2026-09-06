# Close the remaining production blockers

This ExecPlan is the active cross-phase review maintained under `.agent/PLANS.md`.
It does not promote any migration flag or claim production readiness without the
external evidence required by the normative specification.

## Purpose and observable outcome

Cross-check the nine phase plans and implementation, close safe repository-only
defects, and leave an auditable checklist for the remaining production gates:
device equivalence, shard ownership, causal transport, recovery, fencing,
generated management clients, workstation I/O, federation, scientific
validation, migration and legacy removal.

## Specification authority and traceability

Authority is `docs/specifications/distributed-whole-brain-emulator-v1.1.md`,
the nine plans in `docs/execplans/`, and invariants `INV-001`–`INV-017`.
The detailed closure procedure is `docs/production-blocker-runbook.md`.

## Repository orientation

The verified workspace root is `/home/pbisaacs/Developer/neuralmimicry/aarnn_rust`.
`cargo metadata --no-deps --format-version 1` reports one root package,
`aarnn_rust`, plus the `tools/xtask` workspace member; the illustrative
`crates/` tree has not yet been extracted. The current distributed authority
and compatibility paths are `src/distributed.rs` (`DistributedNode`, heartbeat
registry and legacy `GetNetworkSnapshot`), `src/runner.rs` (`Snapshot` and
mutable `Runner`), `src/durability.rs` (reference WAL/checkpoint primitives),
`proto/distributed.proto` (generated tonic service source), and
`src/bin/web_ui.rs` (authenticated HTTP gateway/OpenAPI). New cluster-cut
assembly is isolated in `src/cluster_snapshot.rs` and is also declared by
`src/main.rs` because the binary currently has a parallel module graph.

The baseline working tree already contained modifications to `Cargo.lock` and
the two Webots world files; these are unrelated and are preserved. The new
RPC/REST path is additive, does not enable a migration feature, and does not
claim durable shard ownership or quorum authority.

## Current status

- [x] `2026-09-05` Completed a focused review of automatic sharding and
  whole-brain relocation. `src/distributed.rs` currently makes telemetry-driven
  layer-range deployment changes and may retain a full-network anchor; it does
  not yet move shard-owned biological state through the causal/WAL/fencing
  boundary. The cross-phase implementation plan is now recorded in
  `docs/execplans/intelligent-sharding-and-network-migration.md`. Physical
  consolidation is specified as co-location of stable virtual shards; true
  shard-count reduction remains a separate topology transaction.

- [x] `2026-08-24` Made distributed burst forwarding's deadline configurable
  through `NM_SPIKE_BURST_TIMEOUT_MS`, retaining the 120 ms compatibility
  default. Production Ansible config sets 500 ms for the orchestrator,
  Kubernetes engines and native nodes because observed distributed steps are
  approximately 140 ms; this preserves transport failover while allowing a
  healthy production step to complete.

- [x] `2026-08-22 22:00Z` Re-read repository instructions and cross-reviewed the
  phase plans, canonical runner/distributed/runtime/UI/persistence paths and
  generated-protocol source.
- [x] `2026-08-22 22:00Z` Fixed feature-only OpenCL sparse-map fallback names in
  `src/runner.rs`; all-feature compilation remains valid.
- [x] `2026-08-22 22:00Z` Preserved optional causal source/target neuron IDs in
  `CausalEnvelope` and added a bounded generated-tonic validation service. It
  validates and echoes frames only; shard application and durable acknowledgement
  remain deliberately absent.
- [x] `2026-08-22 22:00Z` Ran the opt-in local OpenCL certification three times
  successfully after one earlier same-host STP rejection. The intermittent
  observation is retained as a device-profile blocker.
- [!] `2026-08-22 22:00Z` Production cutover remains blocked. The exact owners,
  required evidence, safety gates and rollback procedure are in the runbook.
- [x] `2026-08-22 22:08Z` Final sequential verification completed without Cargo
  lock contention: `git diff --check`, `cargo fmt --all --check`, `cargo check
  --locked --all-features --all-targets`, `cargo test --locked --workspace`,
  `cargo test --locked --workspace --doc`, and `cargo +stable clippy
  --workspace --all-targets --all-features` all passed. The all-feature check
  regenerated and compiled the tonic/prost consumers from `proto/distributed.proto`;
  no checked-in generated output exists. Stable Clippy passes with no errors but
  reports the repository's existing non-fatal warning set. Final path/plan
  review still finds the production blockers listed in the
  runbook, so no migration flag or legacy path was promoted.
- [x] `2026-08-22 22:10Z` Re-ran the opt-in OpenCL probe
  `NM_ENABLE_OPENCL_IN_TESTS=1 cargo test --locked --features opencl --lib
  cl_compute::tests::hardware_reference_gate_is_opt_in -- --nocapture`;
  one bounded hardware-reference test passed. This does not erase the earlier
  same-host STP rejection or provide the required architecture/driver matrix,
  long replay, full-kernel comparison or scientific validation.
- [x] `2026-08-22 22:10Z` Final `rg` audit covered direct runner/layer assignment,
  `SpikeBatch`, UI and runtime management, JSON persistence, causal/generated
  protobuf paths, WAL/checkpoints, USB/AER/peripheral and federation markers.
  It confirms the new contracts are additive: `StreamSpikes(SpikeBatch)`,
  `Runner::step`, layer-range assignment, JSON snapshots and direct management
  consumers remain reachable compatibility paths; no generated management
  client, shard-owner application, live USB adapter, federation service or
  complete distributed recovery path is present.
- [x] `2026-08-23 12:00Z` Re-read the updated mobile requirements and verified
  the portable mobile contract with `cargo test --locked --test
  mobile_contract`; all four host contract tests passed. The Android project,
  Quail 3 toolchain, NDK r27d and emulator are now also recorded in the mobile
  plan; iOS, generated production bindings, physical-device and signed-package
  evidence remain unavailable.
- [x] `2026-08-23 12:00Z` Re-ran the required sequential verification after the
  mobile changes: `git diff --check`, `cargo fmt --all --check`, `cargo check
  --locked --all-features --all-targets`, `cargo test --locked --workspace`,
  `cargo test --locked --workspace --doc`, and `cargo +stable clippy
  --workspace --all-targets --all-features` all passed. Clippy emitted
  warnings only under the repository policy.
- [x] `2026-08-23 12:00Z` Re-reviewed all nine phase plans and the mobile plan
  against the implementation, tests and `rg` path audit. Reference slices are
  recorded as complete only where evidence exists; phase gates and production
  cutover remain explicitly blocked by the runbook.
- [x] `2026-08-23 12:00Z` Closed a reference peripheral admission defect found
  during the audit: `src/peripheral.rs` now bounds payloads and rejects
  duplicate capture sequences within a bounded device-epoch window while
  accepting reordered unique samples. Two focused peripheral unit tests and
  the Phase 2–8 gate passed.
- [x] `2026-08-23 10:54Z` Re-ran the required final verification sequentially
  to avoid Cargo lock contention: `git diff --check`, `cargo fmt --all --check`,
  `cargo check --locked --all-features --all-targets`,
  `cargo test --locked --workspace`, `cargo test --locked --workspace --doc`,
  and `cargo +stable clippy --workspace --all-targets --all-features` all
  passed. The all-feature build regenerated and compiled the protobuf
  consumers; Clippy emitted warnings only. No source, schema or flag cutover
  was authorised by this verification run.
- [x] `2026-08-23 11:58Z` Completed the Android Quail 3 reference lane:
  installed NDK r27d, added the checked-in Gradle 9.1 wrapper and host-only
  `cargo xtask` Android build orchestration, compiled both Android ABIs,
  packaged the Rust-enabled debug APK and installed/launched it on the
  configured API 34 emulator. Android unit tests passed; native management,
  peripheral, federation, physical-device and signed-release gates remain
  blocked.
- [x] `2026-08-23 12:07Z` Final review completed after the Android changes:
  the six requested Rust commands passed sequentially, generated protobuf
  output was refreshed by the all-feature build, the Android wrapper/unit/
  ABI/emulator evidence is recorded, and the direct-runner/layer/SpikeBatch/
  UI/persistence/management/AER path audit still identifies only additive
  reference seams. No migration flag was enabled and no legacy path was
  removed.
- [x] `2026-08-23 12:17Z` Repeated the final verification after the latest
  Android source and plan review: `git diff --check`, `cargo fmt --all --check`,
  `cargo check --locked --all-features --all-targets`, `cargo test --locked
  --workspace`, `cargo test --locked --workspace --doc`, and `cargo +stable
  clippy --workspace --all-targets --all-features` passed sequentially.
  `./gradlew testDebugUnitTest --no-daemon` passed, and the Rust-enabled
  `assembleDebug -PwithRust=true` APK reinstalled and launched on
  `emulator-5554`; UI automation observed Rust ABI 1 and 9/10 unavailable or
  gated capabilities. Generated protobuf output remained fresh through the
  all-feature build. No source migration, production flag, or legacy-path
  removal is authorised by this evidence.
- [x] `2026-08-23 13:35Z` Reinstalled the Rust-enabled debug APK on
  `emulator-5554` after the Android safety changes. UI automation confirmed the
  remote observation form and Rust ABI availability. A deliberate invalid-login
  probe reached `192.168.1.2` with `aarnn.neuralmimicry.ai` and returned
  `HTTP 401 invalid_credentials`, confirming ingress routing and fail-closed
  auth. Live neural display could not be claimed because no authorised runtime
  credential was available to this lane. The release manifest rejects cleartext;
  only the debug manifest permits the documented emulator HTTP endpoint.
- [x] `2026-08-23` Android UI review moved connection/account controls to a
  separate Account destination and made Dashboard the visual-first landing
  screen. Emulator UI automation confirmed both destinations, accessible
  Material navigation icons, graphical empty state and bounded read-only
  capability/session presentation. This remains reference UI evidence; it does
  not close generated management, live neural authorisation, peripheral,
  federation or other production gates.
- [x] `2026-08-23 14:55Z` Final Webots/UI cross-review completed with no stale
  runtime processes left behind. The opt-in Rust framebuffer capture now
  supports `NM_UI_CAPTURE_CLOSE=0` for a live multi-process evidence run. With
  distributed auto-selection disabled, `logs/rust-ui-celegans-graph-live-connected-final.png`
  contains the zoomable/rotatable/pannable Graph Explorer with visible
  weighted connections; the same run recorded Webots `Connected`, repeated
  `tx/rx` pairs and Rust IPC frames 1/100/200/300. This proves local
  Rust-runner/Webots connectivity and presentation only. Cluster-global
  snapshot resets, shard ownership, causal durable application, quorum
  fencing, generated management clients, live browser/native I/O, federation,
  scientific and migration evidence remain explicit blockers.

## Required next gates

1. Build a serialisable shard-owned biological state and integrate it with
   causal apply/commit, WAL, checkpoint and recovery before changing Runner
   ownership.
2. Replace the validation echo with a generated causal client/server path that
   applies each event once at a durable shard receipt boundary; run the
   reorder/duplication/reconnect/stale-term/generation integration matrix.
3. Add quorum-backed terms and validate fencing at admission, transport, WAL,
   checkpoint, management and effect boundaries.
4. Generate and consume one versioned management contract through Rust, web and
   native clients with OIDC/PKCE, worker identity, audit and concurrency tests.
5. Implement and test the native/browser AER adapters, federation links and
   independent modality budgets; obtain physical-device and browser evidence.
6. Publish scientific datasets, numerical/transducer metrics and reproducible
   reports, then rehearse checkpoint migration/canary/rollback.
7. Remove legacy layer/vector/direct-worker paths only after all preceding gates
   and the rollback window pass.

## Validation and rollback

The required sequential commands were rerun after the final source/schema
review and passed. All migration flags remain disabled by default. Existing
Runner/layer/`SpikeBatch`/JSON/direct-management paths are compatibility
rollback paths; no new-only checkpoint or effectful workstation capability may
become the sole recovery path before migration evidence exists.

## Final cross-review status

The repository is green for host compilation and deterministic/reference tests,
including the portable mobile lifecycle/checkpoint/discovery/capability seam.
It is not production-ready. The blockers are tracked in
`docs/production-blocker-runbook.md` and in each phase plan: device-profile
equivalence, shard-owned biological state, causal application and durable
receipts, quorum fencing, generated management clients, live browser/native
I/O, federation, scientific validation, migration evidence and legacy-path
removal. No migration feature flag was enabled and no legacy path was removed.
The Android emulator has additionally proved ingress routing and fail-closed
authentication with an invalid-login response; live workspace/neural display
remains unverified because an authorised runtime credential was unavailable.

The native Graph Explorer screenshot is similarly read-only evidence: the
local Webots IPC path is live in the accompanying runtime logs, but this does
not certify the distributed cluster snapshot RPC or any production management,
peripheral or scientific gate.

- [x] `2026-08-23 16:05Z` Final review after the capture-lifecycle change:
  `git diff --check`, `cargo fmt --all --check`, `cargo check --locked
  --all-features --all-targets`, `cargo test --locked --workspace`, `cargo test
  --locked --workspace --doc` and `cargo +stable clippy --workspace
  --all-targets --all-features` all passed sequentially. The phase-plan review
  corrected the stale Phase 8 screenshot name and found no remaining stale
  Webots-timeout claim. The required direct-runner/layer/SpikeBatch/UI/
  persistence/management/AER/federation audit confirms additive reference
  seams only; generated protobuf consumers remain fresh through the
  all-feature build. No production flag was enabled, no legacy path was
  removed, and the explicit blockers remain unchanged.
- [x] `2026-08-23 17:13Z` Added the authenticated read-only workspace topology
  snapshot contract across `RunnerEngine`, `RuntimeManager`, the Rust gateway,
  OpenAPI and the Android remote client. Runtime tests cover a real bounded
  matrix-backed workspace, including the requested node/edge limits; the
  all-feature build, workspace tests, documentation tests and stable Clippy
  were rerun sequentially and passed. Android JVM tests/package and emulator
  install/Graph-tab evidence also passed. The cross-review `rg` audit covered
  direct Runner, layer assignment, `SpikeBatch`, UI, persistence, management,
  AER and federation paths. Generated protobuf output remains fresh through
  the all-feature build.
- [!] `2026-08-23 17:13Z` The topology endpoint is a bounded local-runner
  projection, not a cluster-global shard snapshot. Production blockers remain:
  certified device/OpenCL equivalence, shard-owned biological state, causal
  gRPC cutover, durable distributed recovery, consensus fencing, generated
  management clients, live browser/native USB-AER I/O, federation, scientific
  validation, migration evidence and legacy-path removal. No production flag
  was enabled and no legacy path was removed. Live Android neural data remains
  unverified because authorised runtime credentials were unavailable.
- [x] `2026-08-23 17:33Z` Rebuilt the Android debug APK after the final graph
  fallback change: `testDebugUnitTest assembleDebug --no-daemon` passed, the
  APK reinstalled on `emulator-5554`, and the Graph tab was selected and
  visually verified with layer-coloured nodes, bounded demonstration edges,
  zoom/rotation controls and pan guidance. The screenshot is offline UI
  evidence only; no live neural response was claimed.
- [x] `2026-08-23 17:39Z` Re-ran the required final Rust verification
  sequentially: `git diff --check`, `cargo fmt --all --check`, `cargo check
  --locked --all-features --all-targets`, `cargo test --locked --workspace`,
  `cargo test --locked --workspace --doc` and `cargo +stable clippy
  --workspace --all-targets --all-features` all passed. The all-feature check
  regenerated and compiled the protobuf consumers. Android verification also
  passed with the installed Android Studio JBR and SDK explicitly selected:
  `JAVA_HOME=/snap/android-studio/current/jbr
  ANDROID_HOME=/home/pbisaacs/Android/Sdk
  ANDROID_SDK_ROOT=/home/pbisaacs/Android/Sdk
  PATH=/snap/android-studio/current/jbr/bin:/home/pbisaacs/Android/Sdk/platform-tools:$PATH
  ./gradlew testDebugUnitTest assembleDebug --no-daemon`. The default shell's
  Java 8/absent SDK configuration was an environment failure, not a source
  failure; no global environment or repository-local SDK configuration was
  changed.
- [!] `2026-08-23 17:39Z` Final cross-review leaves production cutover blocked
  by certified CPU/OpenCL/device equivalence, shard-owned biological state,
  causal gRPC application and durable receipts, distributed recovery,
  quorum consensus/fencing, generated management clients, live browser/native
  USB-AER and concurrent media/HID I/O, federation, scientific validation,
  migration/rollback evidence and legacy-path removal. The topology endpoint
  is an authenticated, bounded local-runner projection only; no production
  flag was enabled, no legacy path was removed, and live authorised Android
  neural data remains unverified.
- [x] `2026-08-30 15:51Z` Added the additive `GetClusterNetworkSnapshot`
  gRPC contract and `/api/cluster_snapshot` gateway. The orchestrator now
  gathers one bounded shard snapshot from every assigned node, captures the
  node's in-memory channel buffers, canonicalises responses by node ID, and
  publishes only a complete common runner frontier with per-shard and cluster
  digests. Missing/duplicate/unexpected shards, malformed or oversized state,
  assignment mismatch, mixed frontiers and shape mismatch fail closed.
  `cluster_snapshot::tests` and the two distributed RPC tests pass; formatting
  and `git diff --check` pass. This remains a reference cut over legacy Runner
  state: durable shard-owned state, asynchronous GVT/consistent-cut protocol,
  warm replication and quorum fencing remain open.
- [x] `2026-08-30 16:26Z` Extended the reference durability seam with chained
  WAL integrity, idempotent warm-replica retransmission, durable receipt-ledger
  deduplication and explicit sealed shard checkpoint payloads. The causal
  adapter now stages receiver and receipt mutations together and rejects
  conflicting replay. Focused durability/causal tests and the Phase 2–8 gate
  pass. This does not close live shard-owner application, filesystem receipt
  recovery, warm replication, quorum fencing or measured failover.
- [x] `2026-08-30 16:20Z` Added the `DurableShard` staged apply/commit seam and
  `DurableCausalStreamAdapter`. A pure biological transition now cannot publish
  a receipt, WAL record or warm-replica sequence when it fails; exact causal
  retransmission is a no-op, and verified checkpoints restore the receiver
  cursor, receipts, WAL and biological byte state. Focused durability and
  causal adapter tests pass. This is not a live distributed cutover: the
  legacy `ManagedNetwork`/`Runner` loop, filesystem receipt transactions,
  quorum fencing, failover and measured RPO/RTO remain blockers.
- [x] `2026-08-30 16:20Z` Added a deterministic `QuorumLeaseAuthority`
  reference contract in `src/management.rs`. Lease issuance and revocation
  require an explicit majority of configured members, terms monotonically
  replace prior shard leases, and validation rejects stale node/token pairs.
  Unit tests cover quorum loss, recovery, replacement fencing and membership
  validation. It is not a production consensus implementation and is not
  wired into the live distributed node.
- [!] `2026-08-30 16:26Z` Rust UI cluster refreshes now use
  `GetClusterNetworkSnapshot` and validate the complete response before using
  a selected shard as the bounded visual projection. The projection does not
  merge biological state from multiple shards; full cluster visualization and
  durable cluster export remain deferred until shard-owned state and a real
  consistent-cut protocol exist. `cargo check --locked --features
  desktop_ui_workload --lib` passes; the narrower `ui` feature set retains
  three pre-existing unrelated compile errors (`stats_log_enabled`, inferred
  `ipc_dt`, and conditional `ipc_mapping`).
- [x] `2026-08-30 16:31Z` Closed two repository-local persistence integrity
  gaps without enabling a migration flag. `FileDurableShard` now atomically
  persists and reopens the full sealed shard boundary; `ClusterGlobalSnapshot`
  now digests channel state and can be immutably published and verified by
  content address. The generated cluster RPC and HTTP projection expose the
  channel digest. `cargo test --locked --lib durability` and
  `cargo test --locked --lib cluster_snapshot` pass.
- [!] `2026-08-30 16:31Z` Production cutover is still blocked by live shard
  ownership integration, asynchronous consistent cuts/GVT, real cross-process
  warm replication, persisted consensus/fencing, failover/rejoin and measured
  RPO/RTO. Generated management clients, browser/native AER, federation,
  scientific validation, migration and legacy-path removal remain open.
- [x] `2026-08-30 16:36Z` Final repository verification passed after the last
  digest/identity checks: formatting, diff check, all-feature/all-target
  compilation, workspace tests, workspace doc tests, and focused durability
  and cluster-snapshot tests. Only warnings were emitted. Existing unrelated
  dirty files were preserved and no production flag was promoted.
- [x] `2026-08-30 18:41Z` Re-ran the required host gates sequentially:
  `cargo fmt --all --check`, `cargo check --locked --all-features --all-targets`,
  `cargo test --locked --workspace`, `cargo test --locked --workspace --doc`,
  `cargo +stable clippy --workspace --all-targets --all-features`, and
  `git diff --check`; all passed with warnings only. `cargo xtask bindings check`,
  `cargo xtask qa matrix --available --include-examples`, and
  `scripts/qa/run-examples.sh --all` passed for the catalogued host examples.
  The QA harness now validates complete scenario-manifest fields and generated
  management outputs carry a checked schema-source fingerprint.
- [x] `2026-08-30 18:41Z` Added crash-safe consistent-cut epoch allocation,
  resumable partial coordinator state, and immutable content-addressed cut
  evidence in `FileConsistentCutStore`. The live cluster snapshot RPC uses
  `NM_CONSISTENT_CUT_ROOT` when explicitly configured and fails closed on
  persistence errors; the default compatibility counter remains in-memory.
  Restart/resume, monotonic epoch, digest and immutable-publication tests pass.
- [!] `2026-08-30 18:41Z` This closes a repository-local GVT evidence/restart
  gap but does not make the legacy `ManagedNetwork` runner a shard-owned causal
  executor, provide mature quorum consensus, or produce multi-host chaos,
  physical AER, iOS/Xcode, browser-live, scientific-dataset or migration
  evidence. No migration feature flag was enabled and no legacy path was
  removed.
- [x] `2026-08-30 19:06Z` Bound live cut evidence to the exact captured shard
  snapshot and channel-state payload used for assembly. Durable-owner snapshot
  reads can no longer be paired with a separately observed mutable runner
  frontier or queue map. Added a regression test for the captured frontier,
  queued causal work and marker epoch. `cargo test --locked --workspace
  --quiet`, `cargo check --locked --all-features --all-targets`, stable
  Clippy, binding freshness, the available QA matrix and the focused cluster
  snapshot tests all passed; warnings only. iOS/Xcode and other external
  production evidence remain explicitly unavailable.

## Progress update — 2026-08-31 14:22Z

- [x] Added secured-management regression coverage for unauthorised status
  reads and cross-principal operation lookup, plus a startup-role test proving
  workers do not expose the generated management service. Focused management,
  workspace, documentation, all-feature and generated-binding checks pass.
- [!] Production cutover remains blocked: `ManagedNetwork` is still a
  compatibility `Runner` projection, live exchange still uses legacy
  `SpikeBatch`, the durable quorum adapter is filesystem-local rather than
  network consensus, and physical multi-host RPO/RTO plus OIDC/PKCE/mTLS/audit
  evidence is unavailable.

- [x] `2026-08-31 14:50Z` Wired the versioned `CausalDataPlane` service into
  the live node server and added an exclusive `NM_CAUSAL_TRANSPORT_LIVE=1`
  sender path. Cross-process layer ingress is now validated against the stable
  brain/stream/generation identities, decoded with bounded AER payloads, and
  admitted through the durable receipt/WAL/warm-replica boundary before the
  response acknowledges it. A durable channel projection is restored after
  restart and a focused replicated-durability test proves the ingress receipt
  and queued layer state. The flag remains opt-in because transport TLS/mTLS,
  multi-sender stream allocation, network quorum, and physical fault evidence
  are not yet acceptance-complete.

## Progress update — 2026-08-31

- [x] Strengthened the local durable boundary after an additional crash-window
  review. `FileWarmReplica` now treats a synced WAL suffix without an active
  checkpoint publication as uncommitted and repairs it to the supplied active
  prefix; divergent data still fails closed. Added a regression test for this
  exact window.
- [x] Added explicit replicated-authority configuration parsing through
  `NM_AUTHORITY_REPLICAS=member=path,...` and wired live durable owners to the
  replicated binding before the single-file fallback. Added exact-member-set
  and explicit-availability tests. Durable snapshots and workspace projections
  now read the authoritative owner boundary when the opt-in durable profile is
  active, with a regression test rejecting a read-ahead runner projection.
- [!] These changes improve local durability and prevent two silent fallback
  classes; they do not close the production blockers. `ManagedNetwork` still
  exchanges legacy `SpikeBatch` frames, biological ownership is still backed by
  a compatibility `Runner` working projection, the authority adapter is not
  network consensus, and physical multi-host RPO/RTO plus OIDC/PKCE/mTLS,
  durable audit and live concurrency evidence are not available.

## Progress update — 2026-08-31

- [x] Made the opt-in live causal profile fail closed on partial deployment:
  it now requires durable and distinct warm roots, an explicit three-member
  replicated authority, mTLS, per-node credentials and a node-to-mTLS-leaf
  SHA-256 allow-list. The legacy `StreamSpikes` RPC and MPI receiver are
  rejected while this profile is active, preventing duplicate admission
  domains.
- [x] Added the corresponding configuration documentation and retained the
  local causal, durable, management, failover/rejoin and available QA gates.
- [!] This closes accidental mixed-mode and metadata-only identity paths, but
  production remains blocked by the compatibility `Runner` biological owner,
  filesystem rather than network consensus, absent physical multi-host
  chaos/RPO/RTO evidence, and incomplete OIDC/PKCE, durable audit delivery and
  external platform evidence.

## Outcomes & Retrospective

## Progress update — 2026-08-31

- [x] Extended the versioned cluster-global snapshot schema to carry the
  complete verified `authoritative_shard::ShardState` for every participating
  durable shard. Its canonical digest is included in the cluster digest, the
  biological bytes and applied logical frontier are checked against the
  captured runner boundary, and mixed complete/projection cuts fail closed.
  The generated distributed protobuf now carries the same state and digest
  fields through node-to-orchestrator snapshot RPCs. A regression test covers
  complete-state verification and projection tampering.
- [x] `cargo fmt --all --check`, `cargo check --locked --all-features
  --all-targets`, and the focused cluster snapshot tests pass. The full
  all-feature workspace test is running/being captured separately before this
  entry is treated as the final verification record.
- [!] This closes the snapshot contract gap only. Live biological stepping is
  still performed by the compatibility `Runner` and the durable state is its
  staged projection until full stable-ID model parity and migration evidence
  exist. The authority/warm paths remain filesystem adapters, so network
  consensus, physical failure-domain RPO/RTO and production management
  identity/audit evidence remain open.

The repository currently provides deterministic and governed reference slices,
not a production distributed whole-brain deployment. External hardware,
multi-process quorum/recovery, browser/native I/O, scientific datasets and
migration evidence are required to close the remaining blockers.

The cluster snapshot slice closes the previous single-node projection defect at
the contract boundary while making its safety limits explicit. It does not
close the durable distributed recovery, ownership, quorum, management-client,
I/O, federation, scientific or migration gates.

## Progress update — 2026-08-31

- [x] Served `web_ui/management-client.generated.js` from the Rust web gateway
  at the exact path loaded by `web_ui/index.html`, with JavaScript content type
  and no-store caching. The browser compatibility suite now verifies the
  source-to-gateway route is present; `cargo test --locked --test
  web_ui_browser_compat` passes all five tests.
- [!] The generated browser methods that submit/inspect orchestrator operations
  still require a real authenticated gateway-to-orchestrator adapter. The
  current browser application uses the generated request wrapper for the
  existing authenticated REST surface, while gRPC management remains an
  internal reference service. Adding a shared file or fixed service principal
  would break tenant isolation and is therefore not an acceptable cutover.

## Mobile cross-cutting review

The mobile requirements added to `AGENTS.md` are tracked in
`docs/execplans/mobile-cross-platform.md` and `docs/mobile-platform.md`.
`src/mobile_runtime.rs` and `tests/mobile_contract.rs` now provide the
platform-neutral host contract for explicit mobile modes, lifecycle-safe
checkpointing, discovery observations and safe-unavailable capabilities.
There is an Android Kotlin shell under `apps/android` with a checked-in Gradle
wrapper, host `xtask` native build orchestration, bounded JNI lifecycle seam and
safe capability report. Both Android ABIs package successfully and the debug
APK launches on the configured emulator; this is reference packaging/smoke
evidence only. There is no iOS project, production-generated management
binding, live Android peripheral/management adapter, physical-device evidence
or signed package artefact. These are explicit blockers, not silently skipped
tests; the existing migration flags and legacy rollback paths remain unchanged.

## Progress update — 2026-08-31

- [x] Extended `DurableShard` to maintain an independent causal receiver
  cursor for every producer stream while retaining one shard-global WAL
  commit order. Checkpoint restore reconstructs and validates each stream's
  contiguous receipt frontier, and term promotion rebuilds all cursors.
- [x] Added `sender_node_id` to the versioned causal envelope and derive live
  sender streams from `(network_id, sender_node_id)`. The live node rejects
  missing sender identity, stream/identity mismatch and senders not present in
  the enrolled peer registry. This prevents concurrent sender sequence
  collisions; transport cryptographic identity is still an external TLS/mTLS
  gate.
- [x] Secured management status and operation reads now refresh the persisted
  management document under its lock before authorization and lookup. Added a
  two-process regression test proving a second service observes a committed
  operation without restart.
- [x] Verification: the multi-sender durable-shard test, generated causal gRPC
  suite (3 tests), generated management gRPC suite (5 tests), formatting and
  all-feature/all-target compilation pass. Existing warning-only lint output is
  unchanged.
- [!] The implementation still cannot claim production cutover: live biology
  is staged through the compatibility `Runner`; sender identity is metadata
  until TLS/mTLS is configured; warm replication and fencing are filesystem
  adapters; there is no network consensus/election or physical multi-host
  kill/partition RPO/RTO run; OIDC/PKCE token validation and durable audit
  delivery are not integrated into the gRPC server. These require deployment
  infrastructure and security/operator evidence, not more local unit tests.

## Progress update — 2026-08-31

- [x] Added a real child-process failover/rejoin lane in
  `tests/failover_rejoin.rs`. It records a committed warm boundary, fences an
  old process after a quorum lease replacement, kills that process, restores
  the exact state on a replacement owner, verifies continued commit and
  rejects an old-node active rejoin. The resulting recovery bundle is
  immutable and machine-verifiable. `cargo test --locked --test
  failover_rejoin` and `cargo xtask qa run --suite recovery` pass.
- [x] Tightened secured generated-management scope: empty brain IDs are
  rejected and operation reads require an exact persisted brain match.
  `cargo test --locked --test management_grpc` passes.
- [!] The new evidence is still repository-local: the authority and warm
  replicas use filesystem adapters, not network consensus; live biological
  state still uses the compatibility `Runner` working projection; gRPC auth
  still relies on a configured bearer token rather than OIDC/PKCE/workload
  mTLS; no physical multi-host RPO/RTO or durable audit delivery evidence
  exists. Production flags remain disabled.

## Verification update — 2026-08-31

- [x] Causal acknowledgements now preserve the authenticated wire
  `sender_node_id` through validation, durable and authoritative gRPC service
  responses without adding transport metadata to the biological envelope.
  The generated causal integration suite asserts this across duplicate,
  multi-sender and authoritative paths.
- [x] Fixed the QA catalog parser expectation for the newly catalogued EX-012
  cross-process failover scenario.
- [x] Sequential verification passed: `cargo fmt --all --check`, `git diff
  --check`, `cargo test --locked --workspace`, `cargo test --locked
  --workspace --doc`, `cargo check --locked --all-features --all-targets`,
  `cargo +stable clippy --workspace --all-targets --all-features`, `cargo
  xtask bindings check`, `cargo xtask qa matrix --available
  --include-examples`, `cargo xtask examples run --id EX-012`, and
  `scripts/qa/run-examples.sh --all`. Clippy remains warning-only under the
  repository policy, including existing compatibility/reference warnings.
- [!] These green repository gates do not change the production status:
  network consensus/election, network-authenticated causal transport,
  physically separated multi-host failure evidence, integrated OIDC/PKCE or
  workload mTLS, and durable replicated audit delivery remain required before
  enabling production cutover.

## Verification update — 2026-08-31 15:43Z

- [x] Corrected the secured service authorization call site to use the
  persisted policy, and kept the operation lifecycle fail-closed by updating
  the phase gate to use `Pending -> Running -> Succeeded`.
- [x] Added stable producer/network-scoped managed event-ID coverage and made
  management startup reject missing or empty bearer-token/principal
  configuration before exposing the generated service. The existing
  request-level interceptor remains fail-closed as well.
- [x] Passed `cargo fmt --all --check`, `git diff --check`,
  `cargo check --locked --all-features --all-targets`, and the complete
  `cargo test --locked --workspace` suite (194 library tests, 193 binary
  tests, generated causal/management suites, child-process failover/rejoin,
  mobile and browser QA suites). The build remains warning-only under the
  repository's existing lint policy.
- [!] Production cutover remains blocked for the already recorded reasons:
  the live biological owner is still a compatibility `Runner` projection;
  the opt-in causal sender still lacks transport-authenticated identity and
  its default is legacy `SpikeBatch`; quorum/warm replication are local
  filesystem adapters rather than network consensus; and OIDC/PKCE or
  workload mTLS, durable audit delivery, physical multi-host RPO/RTO and
  migration-removal evidence are not present. These cannot be closed by
  additional local unit tests alone.

## Progress update — 2026-08-31 15:57Z

- [x] Replaced the volatile live-causal sender cursor with a bounded,
  digest-verified durable outbox in `src/managed_durability.rs`. Outbox
  updates use the same lock/atomic-replace-and-sync discipline as the local
  durable owner, preserve unacknowledged prefixes across restart, reject
  acknowledgement regression/overflow and isolate each destination's cursor.
- [x] Wired `NM_CAUSAL_TRANSPORT_LIVE=1` forwarding to the outbox and changed
  causal stream/event identity to include the producer, network and receiver.
  Empty new batches still drain a persisted pending prefix, and successful
  acknowledgements are removed only after the complete generated stream is
  echoed by the receiver.
- [x] Added restart, per-peer isolation, digest-corruption and destination
  identity regression tests. `cargo fmt --all --check`, the focused
  `managed_durability` tests and the generated causal gRPC suite pass.
- [!] This is a repository-local retry/replay improvement, not production
  cutover. The crash window is now covered by a durable commit-intent record;
  transport identity is still metadata until TLS/mTLS is deployed; quorum and
  warm replication remain filesystem adapters; and physical RPO/RTO, integrated
  OIDC/PKCE/workload identity and durable audit evidence remain unavailable.

## Verification update — 2026-08-31

- [x] Completed the managed commit-intent recovery boundary in
  `src/managed_durability.rs`. Each live durable step records its prior WAL
  frontier, biological snapshot, channel state and destination outbox
  frontiers before publication. Reopen validates the term, WAL frontier and
  every outbox sequence/batch, repairs only the exact missing suffix, and
  fails closed on divergence. Fixed the empty-prefix replay bug and added a
  crash-window regression test.
- [x] Kept startup fail-closed when `NM_DURABLE_SHARD_ROOT` is configured but
  the replicated-durability owner cannot be opened; the default-feature build
  now also rejects that configuration explicitly instead of referencing a
  durability-only value.
- [x] Verification passed: `cargo fmt --all --check`,
  `cargo test --locked --workspace`, `cargo check --locked --all-features
  --all-targets`, `cargo xtask qa matrix --available --include-examples`,
  `cargo xtask examples run --id EX-012`,
  `scripts/qa/run-examples.sh --all`, `cargo xtask bindings check`, and
  `git diff --check`.
- [!] The remaining cutover blockers are unchanged: biological execution is
  still computed by a compatibility `Runner` projection; the durable authority
  and warm replica are filesystem adapters rather than network consensus;
  transport identity is not bound to TLS/mTLS; no physically separated
  multi-host kill/partition RPO/RTO evidence exists; and OIDC/PKCE, workload
  identity, durable audit delivery and full management-to-cluster execution
  remain unintegrated. iOS/Xcode and physical AER/scientific/migration gates
  also remain unavailable in this workspace.

## Progress update — 2026-08-31

- [x] Added a typed `authoritative_shard::ShardState` read model. It is built
  only from one verified sealed checkpoint and includes biological bytes,
  channel state, causal WAL state, receipts, logical frontiers, generations,
  lease term and state digest. Durable live snapshot projection now reads this
  coherent state rather than independently reading biological and channel
  projections.
- [x] Added SHA-256 hash chaining to persisted management audit records, with
  sequence, previous digest and record digest. New and migrated records are
  verified on every persisted read; tampered or partially migrated histories
  fail closed. Added `management_grpc` tamper-evidence coverage.
- [x] Tightened the opt-in live causal transport startup contract: mutual TLS,
  a local per-node credential and a receiver-side `node=token` allowlist are
  required. Authenticated node metadata is sent on each causal stream and
  checked against the envelope sender identity before admission.
- [x] Verification: workspace check, authoritative-shard tests, causal gRPC
  tests and management gRPC tests pass after the changes; Cargo lock was
  refreshed for the SHA-256 audit dependency.
- [!] These changes close local coherence, audit-integrity and fail-closed
  transport seams only. They do not establish a network consensus/election
  algorithm, bind node IDs to certificate identities, replace the live
  compatibility `Runner` with stable-ID shard execution, or provide physical
  multi-host RPO/RTO evidence. Production cutover remains disabled.

## Progress update — 2026-08-31

- [x] Added a single production-cutover preflight shared by distributed-node
  and web startup. `NM_PRODUCTION_CUTOVER=1` now fails before listeners or
  workers start unless the live causal profile, replicated-durability and
  management-v1 build profiles, mTLS/domain, OIDC/JWKS/revocation, durable
  management state and stable node identity are present. The browser gateway
  additionally requires OIDC and secure session cookies. Reference builds
  remain unchanged when the flag is absent.
- [!] This is a fail-closed promotion guard, not evidence that the underlying
  compatibility Runner, filesystem authority, or external OIDC/audit and
  multi-host deployments are production-complete. Those acceptance gates
  remain open until their live evidence exists.

## Progress update — 2026-08-31

- [x] Strengthened production OIDC preflight validation in
  `src/management.rs`: the configured JWKS and revocation paths must resolve to
  readable regular files; the JWKS must decode as a non-empty `JwkSet`. This
  prevents a cutover from starting successfully and failing only on the first
  authenticated request.
- [x] Added regression coverage for valid OIDC files, malformed and empty JWK
  sets, a missing revocation source and a non-regular revocation path. Focused
  management tests pass after formatting.
- [x] Verification after the change passed: `cargo fmt --all --check`, `git
  diff --check`, `cargo check --locked --all-features --all-targets`, `cargo
  test --locked --workspace`, `cargo test --locked --workspace --doc`,
  `cargo +stable clippy --locked --workspace --all-targets --all-features`,
  `cargo xtask bindings check`, `cargo xtask qa matrix --available
  --include-examples`, `cargo xtask examples run --id EX-012`, and
  `scripts/qa/run-examples.sh --all`. The QA matrix reports iOS as not run
  because no Xcode project is present. A strict `clippy -D warnings` probe
  remains non-green on existing exporter/legacy warnings; the repository
  policy uses warning-tolerant Clippy and no new warning was introduced by
  this change.
- [!] The secured cutover still requires external OIDC/PKCE issuance and
  refresh, workload identity/mTLS provisioning, replicated audit delivery and
  live operator evidence. File validation closes only the local startup
  configuration defect.

## Progress update — 2026-08-31

- [x] Added `StableBiologicalState` to `src/authoritative_shard.rs` as a
  versioned, serialisable stable-ID kernel. It owns neuron membrane,
  threshold, refractory and adaptation state; explicit synapse weight,
  delay, release and plasticity fields; and logically tagged future events.
  Stable IDs are validated and ordered before any transition, and the state
  has a deterministic digest.
- [x] Added `AuthoritativeShard::apply_stable_event`, which decodes and
  stages the stable biological transition before publishing it through the
  existing receipt, WAL, warm-replica and checkpoint boundary. The generated
  authoritative causal service now has an explicit
  `new_with_stable_biology` constructor. Duplicate causal delivery remains a
  durable no-op, while malformed/stale transitions fail before publication.
- [x] Added unit and generated-gRPC coverage for stable-ID ordering,
  same-tick progression, persistence/reopen, warm checkpoint recovery and
  causal service application. `cargo test --locked --lib authoritative_shard`
  (6 tests) and `cargo test --locked --test causal_grpc` (4 tests) pass.
- [!] This is the first real shard-owned biological execution slice, not the
  complete live-model migration. `ManagedNetwork` still uses `Runner` for the
  existing model and layer compatibility path; the stable kernel is not yet
  selected by a production network/topology profile. The migration remains
  disabled until every biological object in the supported model is represented
  and parity evidence exists.

## Verification update — 2026-08-31 18:59Z

- [x] Added a negative recovery-evidence regression in `src/recovery.rs`:
  evidence that does not prove stale-writer rejection is now rejected by
  `RecoveryEvidenceBundle::verify()`. This closes the corresponding false-
  positive reporting gap for fencing evidence.
- [x] Strengthened the live causal-ingress regression in `src/distributed.rs`:
  it now compares the pre/post biological snapshot bytes, retries the exact
  wire frame, and verifies that the durable receipt count and channel
  projection do not change on duplicate delivery. This demonstrates that
  causal admission updates the durable channel boundary without stepping the
  compatibility biological projection and that replay is idempotent.
- [x] Sequential host verification passed: `cargo fmt --all --check`,
  `git diff --check`, `cargo test --locked --all-features --workspace`,
  `cargo xtask bindings check`, `cargo xtask qa matrix --available
  --include-examples`, `scripts/qa/run-examples.sh --all`, and
  `cargo +stable clippy --locked --workspace --all-targets --all-features`.
  The all-feature suite passed 304 library tests, 290 binary tests, generated
  causal/management gRPC tests, 2 cross-process failover/rejoin tests, mobile,
  browser, phase-gate, runtime and exporter suites. The available QA matrix
  passed and reported iOS as `not-run` because the workspace has no Xcode
  project. Clippy completed with warnings only under the repository policy.
- [!] Production cutover remains blocked for the same substantive reasons:
  the live managed model still computes through the compatibility `Runner`;
  the stable-ID shard kernel is not yet the live full-biology owner; the
  quorum/warm-replication implementation is a local filesystem adapter rather
  than network consensus; physical failure-domain partition/kill RPO/RTO,
  workload mTLS/OIDC-PKCE issuance and refresh, durable audit delivery, live
  browser/native AER, iOS/Xcode, scientific reference datasets and migration
  rehearsal evidence are unavailable. The new tests close local correctness
  claims but cannot substitute for those deployment and validation gates.

## Verification update — 2026-08-31

- [x] Restored the mutable `ManagedNetwork` binding required by the durable
  live-step regression; the causal-ingress regression remains intentionally
  immutable because it must prove admission does not execute biology.
- [x] Re-ran focused durable live-step, duplicate causal-ingress and recovery
  fencing tests. All passed.
- [x] Re-ran `cargo test --locked --all-features --workspace`, the available
  QA matrix with examples, `scripts/qa/run-examples.sh --all`, generated
  binding freshness, `cargo fmt --all --check`, `git diff --check`, and
  warning-tolerant stable Clippy. All passed. The available QA matrix and
  example runner report iOS as `not-run` because no Xcode project is present.
- [!] No production status is promoted by this verification. The remaining
  blockers are external/deployment or incomplete migration gates: a complete
  live stable-ID biological owner, network consensus rather than local
  filesystem quorum, physical multi-host chaos and RPO/RTO measurements,
  deployed OIDC/PKCE and workload mTLS identity, replicated audit delivery,
  native/browser AER execution evidence, iOS/Xcode evidence, scientific
  datasets, and migration rehearsal/rollback evidence.

## Verification update — 2026-08-31

- [x] Corrected QA wrapper executable permissions and reran the complete
  available wrapper loop: `run-portable.sh`, `run-aer-transport.sh`,
  `run-discovery.sh`, `run-federation.sh`, `run-web.sh`,
  `run-mobile-standalone.sh` and `run-hardware.sh`. All available suites
  passed; the hardware wrapper reported `not-run` because no registered
  physical device/approved lane is present.
- [x] Ran `scripts/qa/doctor.sh`. Host/toolchain checks pass and the report
  explicitly marks iOS unavailable because this workspace contains no Xcode
  project. This is unavailable evidence, not a production pass.
- [x] Final repository checks remain green: `cargo fmt --all --check`,
  `git diff --check`, `cargo xtask bindings check`, the available QA matrix
  with examples, the catalogued example runner, and warning-tolerant stable
  Clippy.
- [!] The wrapper and doctor results do not close the external gates. The
  production boundary still requires a full live stable-ID shard owner,
  network consensus/election, physically separated multi-host failover with
  measured RPO/RTO, deployed OIDC/PKCE and workload mTLS, replicated audit
  delivery, live browser/native AER, iOS/Xcode/device evidence, scientific
  reference datasets, and migration rehearsal/rollback before legacy paths or
migration flags can be promoted.

## Verification update — 2026-09-05: multi-shard reference execution

- [x] Added the stable-ID multi-shard reference executor with canonical
  ordering, bounded queues/deduplication, route validation, deterministic
  logical-time propagation, state digests and transactional rollback.
- [x] Added regression tests for split synapse ownership rejection, remote
  endpoint serialisation and complete rollback after emitted queue overflow.
  The focused executor, authoritative shard, causal gRPC and public executor
  integration suites pass.
- [!] This is a validated reference seam. The compatibility `Runner` still
  owns the live managed model, and durable actor integration, quorum-backed
  authority, physical chaos/RPO/RTO, peripheral handoff and scientific parity
  remain open production gates.

The stable executor now also exports durable `ShardState` envelopes and has an
end-to-end migration-transfer regression covering out-of-order frames,
new-term promotion and digest-preserving whole-fabric restore. The live
multi-shard WAL/output actor path remains intentionally gated.

## Verification update — 2026-09-05: fenced complete-fabric publication

- [x] Added `StableExecutorCheckpointSet` and
  `StableExecutorCheckpointStore`. Publication is bounded, immutable and
  digest-verified; all sibling shards must share the same brain, compiled plan,
  topology/partition generations, lease term and fabric cut.
- [x] Added `StableExecutorAuthority`, which validates the writer term and
  fencing token before checkpoint or event admission, publishes the complete
  cut after each committed step, and restores the exact pre-step executor when
  execution or immutable publication fails.
- [x] Added public coverage for immutable reopen, tamper/set validation,
  out-of-order shard transfer, new-term promotion and failed-publication
  rollback. `cargo test --locked --all-targets` passed; the stable executor and
  migration-transfer suites each passed all 3 tests. The replicated-durability
  library suite passed 269 tests, and all-feature/all-target checking,
  generated bindings, formatting and diff checks passed.
- [!] This closes a durable reference boundary only. The live compatibility
  `Runner`/`ManagedNetwork` path is unchanged, and the stable executor is not
  yet driven by the durable shard actors or a network consensus authority.
  Physical multi-host chaos/RPO/RTO, peripheral/effect handoff and scientific
  parity remain open before production selection or migration flags can move.

## Verification update — 2026-09-05: durable stable-shard handoff

- [x] Added `stable_executor_durable.rs`, a reusable bridge that publishes one
  complete stable-executor cut and then records its per-shard checkpoint through
  the existing `AuthoritativeShard` WAL, receipt and warm-replica boundary.
- [x] Added `AuthoritativeShard::apply_stable_checkpoint`, which rejects a
  divergent mirror before mutation and verifies that an exact retry cannot
  reuse a receipt for different biological bytes.
- [x] Added public coverage for a stable step reaching every shard mirror,
  durable receipt publication, owner/warm restart, exact duplicate delivery and
  whole-fabric digest preservation. The focused stable-shard suite now passes 6
  tests.
- [!] The bridge is a resumable local coordinator. A complete network
  transaction, quorum-backed fencing, physical failover and peripheral/effect
  cursor integration remain required before it can drive a production path.

## Verification update — 2026-09-05: bridge retry and transfer preparation

- [x] The durable bridge now prepares bounded `ShardTransferSource` values
  directly from its immutable actor checkpoints, retaining stable shard order
  and caller-supplied consistent-cut/placement evidence.
- [x] Fault-injection coverage proves a partial mirror failure remains pending,
  retries idempotently without replaying the neural step, and reconstructs
  destination actors under a newer lease term.
- [!] This remains repository-local reference evidence. It does not establish
  a network consensus transaction, live executor adoption, peripheral/effect
  cursor handoff, or physical multi-host RPO/RTO.

## Verification update — 2026-09-05: complete brain migration session

- [x] Added `brain_migration_session.rs`, which consumes actual stable bridge
  transfer sources, verifies/reassembles bounded frames out of order, restores
  every destination actor under a newer term, composes real cursor evidence,
  and publishes the complete target placement only after the group barrier is
  ready.
- [x] Added a journal commit boundary that copies the committed group and
  operation progress only after registry publication. It verifies brain, leader
  term, shard set, phase, transfer byte bounds and cut tag, and leaves the
  operation recoverable if journal persistence fails.
- [x] `cargo test --locked --test brain_migration_session` passed, including
  the two-shard bridge-to-registry-to-journal path. CLI proposal aliases and
  read-only operation watch are covered by `tests/placement_cli.rs`.
- [!] This closes the repository reference composition only. It does not close
  replicated consensus, live stable-ID executor adoption, explicit peripheral
  cursor handoff, physical multi-host RPO/RTO, security identity integration
  or scientific parity gates.

## Verification update — 2026-09-05: quorum-bound whole-brain session and structured CLI

- [x] Whole-brain transfer reception now exploits bounded parallel workers and
  retains deterministic shard ordering for evidence publication.
- [x] Added a single quorum transaction for source fencing and shared
  destination-term issuance, plus destination actor binding to the replicated
  fencing document and all-or-nothing lease revocation on materialisation
  failure.
- [x] The persisted end-to-end migration test now closes and reopens both the
  placement registry and migration journal after publication. It verifies the
  committed operation and target authorities survive restart.
- [x] Added and tested nested `brain`, `node` and `operation` CLI forms while
  keeping the established flat automation flags.
- [!] These are validated local reference seams. The orchestrator management
  RPC still journals migration requests but does not yet own a registered live
  `StableExecutorDurableBridge`; network consensus/election, physical
  multi-host RPO/RTO and production identity integration remain blockers.

## Verification update — 2026-09-05: explicit peripheral cursor state

- [x] Added `PeripheralCursorState` with bounded admission and effect cursor
  records, including queued samples, capture/mapping/device epochs, actuator
  lease term, armed state and accepted effect IDs. The DTO is validated before
  it can enter a checkpoint.
- [x] Included the cursor state in the sealed durable checkpoint and transfer
  state. Promotion retains the admitted/effect dedupe state and re-terms the
  actuator cursor under the destination lease. Cutover evidence now includes
  explicit cursor material in its route/effect digests.
- [x] Added `durability` coverage for legacy checkpoint digest compatibility
  and `migration_transfer` coverage for cursor preservation and destination
  re-fencing. `cargo test --locked --lib durability` passed (31 tests),
  `cargo test --locked --test migration_transfer` passed (3 tests), and
  `cargo test --locked --test brain_migration_session` passed (1 test).
- [!] The orchestrator RPC remains journal-only until a live executor registry
  and dispatch adapter is integrated. Network consensus/election, physical
  multi-host RPO/RTO, workload identity and scientific parity remain open.

## Verification update — 2026-09-05: post-cursor broad verification

- [x] `cargo test --locked --all-targets --quiet` passed: 263 library tests
  and all integration/example targets passed.
- [x] `cargo check --locked --all-features --all-targets --quiet`,
  `cargo xtask bindings check`, `cargo fmt --all -- --check` and
  `git diff --check` passed.
- [x] `scripts/qa/run-ansible-placement.sh` passed using the existing
  `/home/pbisaacs/Developer/swarmhpc/swarmhpc/ansible/` inventory. The current
  laptop and `qc00`–`qc04`, `sm00`, `sm01` were reachable; `qc05` was
  excluded as unreachable; only the six explicitly granted compute nodes were
  admitted.
- [!] These checks still do not establish network consensus/election, a live
  management executor registry, physical multi-host chaos/RPO/RTO, deployed
  workload identity or scientific parity.

## Verification update — 2026-09-05: management executor registration

- [x] Added the reusable brain-scoped migration executor registry and wired
  optional dispatch into both management service profiles. Dispatch runs
  blocking transfer work outside the gRPC task, holds one in-flight lease per
  brain, and finalises the journal only from a verified committed group.
- [x] Added a concrete stable bridge adapter and verified it end to end in
  `brain_migration_session`: source frames are transferred, one quorum term is
  promoted, the persisted placement is published, and the source bridge is
  fenced.
- [x] `RUSTFLAGS='-Awarnings' cargo test --locked --test migration_executor`
  passed (2 tests); the focused bridge-backed migration test passed as well.
- [!] The deployed orchestrator currently creates an empty registry handler;
  no deployed brain discovery/registration path exists yet. The implementation
  therefore remains an explicit integration seam. Network consensus/election,
  physical multi-host chaos/RPO/RTO, workload identity and scientific parity
  remain production blockers.

## Verification update — 2026-09-05: local example launcher authentication profile

- [x] `run_examples.sh` builds with explicit local features and no longer
  enables the production-only `management_v1` service through `--all-features`.
  This removes the stale local-example failure requiring
  `NM_MANAGEMENT_BEARER_TOKEN` while preserving fail-closed authentication for
  production management deployments.
- [x] The launcher selects free gRPC and dashboard ports, waits for
  `/api/config`, prints the exact dashboard URL, and bounds cleanup. The
  release smoke test started the orchestrator, both nodes and the dashboard
  without a management bearer token.
- [x] Added `tests/run_examples_launcher.rs`; its two tests pass alongside the
  three stable-runtime bootstrap tests and six web UI compatibility tests.

## Cross-review update — 2026-09-05: launcher and stable bootstrap follow-up

- [x] The stable managed-network test no longer constructs private runtime
  fields from an integration crate. `ManagedNetwork::new` centralizes paused,
  authority-free initialization and preserves explicit stable registration.
- [x] Four stable bootstrap tests pass, and the no-stable feature profile
  rejects the stable manifest flag before attempting to load runtime state.
- [x] The current laptop launcher smoke test reached the dashboard only after
  `/api/config` succeeded and printed the exact URL. The local launcher profile
  therefore avoids the production `management_v1` bearer-token requirement;
  production management authentication remains fail-closed.
- [!] This verification does not change the production blockers: the deployed
  orchestrator still has no live executor registration path, and stable
  physical multi-host routing, consensus/election, chaos RPO/RTO and scientific
  parity remain unverified.

## Cross-review update — 2026-09-05: controller evidence integrity and launcher UX

- [x] The automatic placement controller now binds each review to the source
  and candidate plan digests and checks the exact sorted moved-shard delta at
  commit. Five focused tests pass, including stale and tampered evidence
  rejection.
- [x] The local example profile remains explicitly non-production: it excludes
  `management_v1`, so it does not require `NM_MANAGEMENT_BEARER_TOKEN`; the
  production management profile continues to fail closed when authentication
  configuration is absent.
- [x] Launcher contract tests (2/2), web UI compatibility tests (6/6), and a
  bounded end-to-end laptop run passed. The launcher reported the selected
  gRPC ports and a verified dashboard URL before waiting for input.
- [!] No claim of production cutover is made. Deployed executor registration,
  stable multi-host routing, quorum-backed authority, physical chaos/RPO/RTO,
  workload identity and scientific parity remain open gates.

## Cross-review update — 2026-09-05: management placement admission

- [x] Both management profiles now invoke the deterministic placement
  controller after planner validation and before returning an automatic
  proposal. This enforces the configured residence, improvement, transfer,
  concurrency and emergency constraints at the orchestrator contract.
- [x] Apply success updates controller residence only after the placement
  registry has accepted the fenced plan and its cutover evidence. Persisted
  placement state is reloaded before review when `NM_PLACEMENT_REGISTRY_DIR`
  is configured, closing the restart-to-initial-placement gap.
- [x] The management RPC suite passes 14/14 tests, including the new
  post-apply automatic-move rejection; the controller suite passes 6/6.
- [!] This is an admission and authority-cache integration, not proof of live
  neural shard routing. The deployed orchestrator still has no registered
  stable executor, and physical multi-host migration, consensus/election,
  chaos RPO/RTO and scientific parity remain open production gates.

## Cross-review update — 2026-09-05: cluster verification after admission integration

- [x] The Ansible placement QA passed with six reachable, explicitly granted
  compute nodes (`qc00`, `qc02`, `qc03`, `qc04`, `sm00`, `sm01`); `qc05` remains
  unreachable and excluded. The result was non-degraded and proposal-only.
- [x] All-target tests passed, including the management and controller suites;
  all-feature checking, formatting, diff checks, JavaScript/shell syntax and
  the bounded `run_examples.sh` dashboard smoke passed as well.
- [!] These results validate resource discovery, admission and local UI/runtime
  behavior. They do not close stable physical routing, deployed executor
  registration, quorum/election, physical chaos/RPO/RTO, workload identity or
  scientific parity gates.

## Cross-review update — 2026-09-05: stable worker registration

- [x] Added a versioned stable executor capability registration to worker join,
  heartbeat and node status. The registration is validated as an observation
  of topology/partition identity, shard set, logical frontier, local fencing
  state and bounded budgets; it does not grant writer authority.
- [x] Stable workers now reconnect to the orchestrator through the normal node
  connection manager. The orchestrator records one stable worker per network,
  rejects plan-identity changes without a migration boundary, and retains a
  stable-network fence after worker disappearance.
- [x] The legacy layer rebalancer and legacy load/unload commands are isolated
  from registered stable networks. Focused stable-feature compilation and RPC
  tests pass, including malformed registration, duplicate-owner and
  rebalancing-isolation cases.
- [!] The registration is not yet a deployed migration executor registration,
  quorum lease/election, or stable shard data-plane route. The complete stable
  fabric remains local to the worker process, and physical multi-host chaos,
  RPO/RTO, workload identity and scientific parity remain blockers.

## Cross-review update — 2026-09-05: worker ownership subset contract

- [x] Stable worker registration schema/profile v2 now distinguishes the
  complete plan inventory from the worker's currently materialised shard
  subset. Validation rejects malformed or out-of-plan ownership, while plan
  identity remains stable across a valid ownership change.
- [x] Focused stable worker and distributed join/heartbeat tests pass, as do
  the stable managed-executor, bootstrap and checkpoint integration suites.
- [x] Ownership subset changes are now rejected on ordinary heartbeats and
  require both a newer lease term and fencing token, preserving the explicit
  migration boundary.
- [!] This is an observation and migration-contract seam only. The stable
  executor still owns the complete fabric locally; remote causal routing,
  durable handoff and quorum-backed multi-host execution remain blocked.

## Cross-review update — 2026-09-05: example launcher checkout regression

- [x] Fresh release-profile binaries start the orchestrator and both workers
  without the authenticated `management_v1` service. The dashboard readiness
  probe succeeds and the launcher prints its exact URL and port before waiting
  for input.
- [x] Both launchers now anchor relative paths to their own script directory;
  the launcher contract suite covers that requirement and the dashboard URL
  contract (3/3 tests passed).
- [!] The reported `The Orchestrator UI with Dashboard is now active onscreen`
  banner was from the separate `neuromorphic_demo` checkout, whose launcher
  still uses `--all-features`. It is not the `aarnn_rust` launcher validated by
  this plan.

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

## Verification update — 2026-09-05: stable causal cursor and node gRPC path

- [x] Stable causal ingress now stages a bounded `ReliableReceiver` cursor and
  event/payload identity history before biological admission. Only a durable
  poll commits the cursor; duplicate replay produces no additional biological
  work.
- [x] External stream sequence/receipt state is carried through the stable
  authoritative shard mirrors separately from the bridge's internal mirror
  sequence. Reconnect progress is reconstructed from matching durable receipt
  prefixes across all actors.
- [x] The distributed-node gRPC integration test covers sender enrolment,
  unknown sender denial, duplicate and conflicting replay, sequence gaps,
  stale leases, brain/stream mismatches, and continuation after rejection.
- [x] Stable all-target tests, default all-target tests, all-feature checking,
  formatting and diff validation passed.
- [!] The deployed system still lacks networked partial-shard execution,
  quorum/election, physical multi-host migration dispatch and chaos RPO/RTO
  evidence. The stable bridge remains the local reference authority until
  those gates are satisfied.

## Verification update — 2026-09-05: typed stable-shard data plane

- [x] Added the versioned `StableShardDataPlane` protobuf stream with explicit
  brain/shard/plan/generation, logical-time, event, typed-message, sequence,
  lease/fence and digest fields. The frame converter rejects metadata/payload
  disagreement before executor state is touched.
- [x] Added `DurableStableShardReceiver` with bounded source admission,
  contiguous sequence validation, idempotent duplicate handling, atomic
  checkpoint-plus-receipt publication, crash reopen and durable acknowledgements.
  `flush_pending` provides asynchronous reconnect retry and sender-log cleanup
  only after matching receiver acknowledgements.
- [x] `cargo test --locked --test stable_shard_transport -- --test-threads=1`
  passed: restart/replay/gap/fence coverage and generated tonic client/server
  coverage both passed.
- [!] This closes the reference network data-plane seam only. The managed
  stable loop still mirrors the complete fabric locally; quorum authority,
  authenticated node-session binding, physical dispatch, migration cutover and
  multi-host RPO/RTO evidence remain explicit production blockers.
- [x] Added explicit topology/partition generation fields to sealed outbound
  records and frames. Receiver admission rejects a correctly sealed frame from
  another generation before mutating biological state.
- [x] Final repository checks passed: all-feature all-target checking, all-target
  tests (270 library and 258 integration/target tests), formatting, diff and
  launcher syntax validation. The launcher contract still requires readiness
  at `/api/config` before printing `http://127.0.0.1:<port>`.

## Verification update — 2026-09-05 15:58Z: stable application evidence

- [x] Stable registration is now schema/profile v3 and carries one committed,
  durable application acknowledgement per owned shard. Admission verifies the
  complete sorted set and binds every record to the registration's plan,
  generation, lease/fence and logical frontier.
- [x] Managed workers source acknowledgements from sealed actor checkpoints;
  missing actor evidence produces an empty set and is rejected by the
  orchestrator. RPC tests cover missing, stale-plan, stale-fence and
  uncommitted evidence, plus a valid ownership update with a newer fence.
- [x] Durable reopen tests compare acknowledgement records before and after
  restart. A drained worker may now report zero owned shards and zero
  acknowledgements after a fenced handoff, which enables source detachment.
- [x] Focused stable-worker, managed-executor, distributed, placement CLI,
  causal gRPC and launcher tests passed. The launcher test confirms the
  dashboard readiness probe completes before the URL/port is printed.
- [!] This is still a registration/admission safety gate. The stable worker
  remains a complete-fabric local reference executor pending durable outbound
  causal routing, quorum-backed authority, physical migration dispatch and
  multi-host chaos/RPO/RTO evidence.

## Verification update — 2026-09-05 16:02Z: launcher end-to-end confirmation

- [x] The bounded local launcher run reached the dashboard after its
  `/api/config` readiness check and printed the exact URL and port. It used
  the explicit non-management example profile, so no bearer-token startup
  error occurred; the interrupt cleanup left no launcher-owned release
  process.

## Verification update — 2026-09-05: partial execution and durable retry boundary

- [x] `PartialShardExecutor` now validates exact local checkpoint membership,
  rejects split mutable synapse ownership and routes typed cross-shard events
  while preserving canonical logical tags. Duplicate control messages are
  accepted only when their payload digest matches; conflicting replays and
  plan-derived destination mismatches fail closed.
- [x] `StableOutboundLog` adds a separate bounded durable handoff record for
  each physical destination. Records carry plan/shard/fence/logical-tag/event
  identity, have independent sequence spaces, survive restart, publish with
  lock/fsync/atomic replace, and require a fenced digest-matching
  acknowledgement. Focused tests cover retry, corruption, stale authority
  and conflicting acknowledgement paths.
- [x] The current repository launcher was run with
  `AARNN_NATIVE_UI=0 AARNN_SKIP_BUILD=1 timeout --signal=INT
  --kill-after=3s 12s ./run_examples.sh`. It reached readiness and printed
  `Web dashboard URL (port 8080): http://127.0.0.1:8080`; no launcher-owned
  process remained and no `NM_MANAGEMENT_BEARER_TOKEN` error was present.
- [!] These additions are reference/data-plane seams. Network transport,
  receiver-side durable application, quorum authority and production
  partial-shard cutover remain deliberately disabled until their evidence
  gates pass.
- [x] The existing SwarmHPC Ansible inventory was reused for a read-only
  placement probe over the current laptop, `qc00`–`qc04`, `sm00` and `sm01`.
  All eight responded; `qc05` was unavailable and was safely excluded. The
  Rust planner produced a deterministic six-node enrolled proposal with
  `applied: false`.

## Cross-review update — 2026-09-05: placement-authorised dispatch boundary

- [x] Added `StableShardDispatcher` to bind physical dispatch to the current
  placement registry. It snapshots placement, validates every pending record,
  schedules independent destination streams concurrently and preserves failed
  records in the durable outbox.
- [x] Separated the biological execution-plan digest from the physical
  placement-plan digest in outbound records and protobuf frames. Receiver and
  dispatcher checks now fail closed on placement, fence or generation drift
  before executor mutation or network transmission.
- [x] Added five integration tests for digest identity, endpoint admission,
  retry retention, stale placement/fence/generation rejection and bounded
  batch enqueue. The focused dispatcher suite passes.
- [x] Current-laptop launcher validation confirms that `run_examples.sh`
  selects and prints the actual gRPC and dashboard ports, reports the ready
  dashboard URL only after `/api/config` succeeds, and starts without the
  production bearer-token requirement under its explicit local profile.
- [!] Production cutover remains blocked. The dispatcher has not yet been
  wired into the live worker loop, quorum/election and authenticated session
  fencing are not complete, and physical multi-host migration/RPO/RTO chaos
  evidence is still required.

## Cross-review update — 2026-09-05: staged partial-worker commit boundary

- [x] Added `ManagedPartialShardRuntime` to compose the partial biological
  executor and placement-authorised dispatcher behind a bounded async poll.
  It stages state, atomically seals outbound records, then commits the worker
  state; failed outbox admission cannot expose an unsealed biological step.
- [x] Hardened placement-aware outbound batches so a later queue, size,
  fencing or validation failure rolls back the whole batch. Added integration
  tests for successful partial output and atomic failure retention.
- [!] The adapter remains an explicit reference worker-loop seam. It is not
  automatically constructed from discovery or telemetry and is not connected
  to the deployed orchestrator worker loop. Receiver-side durable ownership,
  quorum/election, authenticated session fencing and physical chaos evidence
  remain production gates.

## Verification update — 2026-09-05 21:40Z: durable activation lifecycle and launcher closure

- [x] Worker activation lifecycle now has an explicit `Active` state. The
  registry rejects regressions from `Active` and resurrection of `Failed`
  under the same idempotency key, while preserving idempotent duplicate
  outcomes and requiring a new key for a retry.
- [x] The management registration callback validates stable-worker evidence
  against the immutable placement before promotion: brain/network identity,
  plan generations and digest, lease/fencing state, complete plan inventory,
  exact target ownership and committed per-shard application acknowledgements
  must all match. Callback work is performed outside distributed heartbeat
  locks, and persistence uses the registry's atomic publication boundary.
- [x] Added persisted reopen coverage for `Active` activation state and
  adjusted the stable heartbeat test to select only activation commands when
  unrelated queued control commands are present. Checkpoint transfer, live
  registration, stable heartbeat, placement and management focused suites
  pass.
- [x] Repository verification passed with `cargo test --locked
  --all-targets`, `cargo check --locked --all-features --all-targets`,
  formatting, diff and launcher syntax checks. The launcher contract suite
  passed all 3 tests.
- [x] The bounded `run_examples.sh` smoke reached `/api/config`, printed
  `Orchestrator gRPC: http://127.0.0.1:50051` and
  `Web dashboard URL (port 8080): http://127.0.0.1:8080`, avoided the
  bearer-token startup failure under its explicit local profile, and cleaned
  up its processes after the intentional timeout.
- [!] No production cutover is claimed. Placement is still published before
  complete remote target activation evidence in the live deployment path;
  deployed executor registration, source drain/WAL catch-up, physical
  multi-host activation, replicated quorum fencing and chaos/RPO/RTO remain
  open gates.
- [x] The final authorized Ansible checks passed for eight-shard distribution
  and single-host consolidation on sm00, with consolidation explicitly
  reporting degraded durability. The existing sm00/sm01 native-node
  validation also passed with no changes; qc00 and qc05 remained excluded by
  reachability.

## Verification update — 2026-09-05 23:31Z: source-scoped stable-shard receipts

- [x] Corrected the stable-shard receiver receipt model so durable sequence
  frontiers are keyed by authenticated source node. Each source has an
  independent per-destination outbound sequence space; the previous receiver
  global map could reject a valid second source at sequence zero as a conflict
  or gap.
- [x] Bumped the receiver document to schema 3 and added bounded recovery of
  schema-2 documents when exactly one allowed source can safely own the legacy
  frontier. Legacy receipts with multiple possible sources fail closed because
  their provenance cannot be reconstructed without guessing.
- [x] Added the multi-source restart/replay integration scenario and ran
  `cargo test --locked --features stable_executor_live --test
  stable_shard_transport -- --test-threads=1`; all 7 transport tests passed.
  `cargo check --locked --no-default-features --features
  'engine_runtime,stable_executor_live' --lib`, formatting and `git diff
  --check` also passed.
- [!] This closes a receiver correctness defect in the reference physical data
  plane. Quorum/network authority, authenticated production identity,
  source-drain/WAL catch-up and physical failure evidence remain open gates.

## Verification update — 2026-09-06: local example launcher authentication path

- [x] Confirmed the old `run_examples.sh` output was from the pre-fix launcher.
  The current launcher builds only the explicit local `engine_runtime,ui`
  profile and therefore does not start the authenticated `management_v1`
  endpoint that requires `NM_MANAGEMENT_BEARER_TOKEN`. Production
  `management_v1` authentication remains fail-closed.
- [x] Consolidated the orchestrator and web dashboard release build into one
  Cargo invocation with both binaries and one feature graph. This avoids a
  second rebuild of the shared library caused by compiling the binaries with
  different feature sets.
- [x] `run_examples.sh` and `run_webcluster.sh` continue to select free
  gRPC/web ports, wait for `/api/config`, and print the exact dashboard URL
  and port. The bounded current-laptop smoke printed
  `Web dashboard URL (port 8080): http://127.0.0.1:8080`, started both nodes,
  found no `NM_MANAGEMENT_BEARER_TOKEN` error in orchestrator, node or web
  logs, and cleaned up launcher-owned processes.
- [x] `cargo test --locked --test run_examples_launcher --
  --test-threads=1` passed all 3 tests; Bash syntax, `cargo fmt --all --check`,
  `git diff --check`, and the combined release build for `aarnn_rust` and
  `web_ui` also passed.
- [!] This validates the local reference launcher only. It does not change
  the open production gates for network consensus, workload identity, live
  worker registration, source drain/WAL catch-up, physical migration and
  failure/RPO/RTO evidence.

## Verification update — 2026-09-06: bounded source drain and WAL catch-up

- [x] Corrected the migration drain progress test so a valid causal step may
  replace one pending event with another without being misclassified as a
  stalled frontier. Drain completion is based on the durable bridge returning
  a committed step and reaching an empty queue within the explicit step bound.
- [x] The source drain now freezes admission only after the final durable
  actor state set is captured. A pre-fence failure can explicitly abort the
  drain and reopen the bridge; a repeated or over-limit drain fails with
  `MigrationDraining` or `MigrationDrainLimit` and never reports a successful
  migration boundary.
- [x] Catch-up verification now covers reconstructed checkpoint state, a
  contiguous replay-provenance WAL tail, destination application through the
  normal actor/WAL/warm-replica path, final biological/channel/logical state,
  WAL frontier equality, tamper rejection and post-drain cutover evidence.
- [x] Focused validation passed:
  `cargo test --locked --features stable_executor_live --test
  managed_stable_executor --test migration_transfer -- --test-threads=1`
  (12 tests), the broader migration/session/executor selection passed (17
  tests), `cargo check --locked --no-default-features --features
  'engine_runtime,stable_executor_live' --lib`, `cargo fmt --all --check`,
  and `git diff --check`.
- [!] This is still reference/local-authority evidence. Network quorum and
  election, authenticated physical worker identity, remote lifecycle
  integration, multi-host migration/failure injection, RPO/RTO measurement
  and rejoin/reclaim evidence remain open production gates.

## Verification update — 2026-09-06: migration authority provenance and launcher smoke

- [x] Reviewed the target bootstrap boundary after live migration testing. A
  transferred checkpoint may legitimately carry source term `N`; the target
  activation manifest now records that checkpoint term separately and opens
  the worker under target placement term `N+1`. The receiver and registration
  checks require the target term and fencing token, while checkpoint
  verification requires the recorded source term.
- [x] Re-ran the real in-process tonic migration path through checkpoint
  transfer, heartbeat activation, durable registration, source cutover and a
  fresh target-object restart with two idempotent activation retries. The
  broader focused migration selection passed 21 tests; the stable bootstrap
  selection passed 8 tests.
- [x] Re-ran the current release `run_examples.sh` on the laptop. It started
  the orchestrator, both nodes and the web dashboard, printed
  `Web dashboard URL (port 8080): http://127.0.0.1:8080`, and produced no
  `NM_MANAGEMENT_BEARER_TOKEN` startup error. Bounded cleanup left no
  launcher-owned process or listener.
- [!] Production migration is still gated on network quorum/election,
  authenticated mTLS/workload identity, live deployed executor registration,
  physical source drain and WAL catch-up, reverse reclaim/rejoin, and
  multi-host fault-injection with measured RPO/RTO.

## Progress update — 2026-09-06 00:30Z: authenticated control-plane node sessions

- [x] Bound live worker `join` and `heartbeat` requests to the declared node
  identity, per-node credential and presented mTLS leaf certificate. The
  orchestrator validates the binding before membership, resource observations,
  stable-worker registrations, command results or activation acknowledgements
  are processed.
- [x] Added the matching client-side metadata attachment for reconnect and
  heartbeat requests. The local node credential must agree with its
  deployment allow-list before a live request is sent; failures reconnect
  through the existing bounded connection manager instead of sending an
  unauthenticated request.
- [x] Kept the reference launcher/profile unchanged when
  `NM_CAUSAL_TRANSPORT_LIVE` is disabled. Existing causal, checkpoint-transfer
  and stable-shard data-plane certificate checks remain independent boundaries.
- [x] Validation passed: `cargo check --locked --features
  stable_executor_live --lib --bin aarnn_rust`; five `node_auth` unit tests;
  five live migration registration tests; three deployment-manifest tests; and
  three launcher contract tests. Formatting also passed.
- [!] This proves request-level identity binding in the shared implementation,
  not a deployed mTLS session across the physical estate. Certificate issuance,
  per-host fingerprints, network quorum/election and physical failure/RPO/RTO
  evidence remain required before production cutover.

## Verification update — 2026-09-06: loopback advertisement in local launcher

- [x] Updated `run_examples.sh` and `run_webcluster.sh` to keep gRPC listeners
  bound to `0.0.0.0` while advertising `127.0.0.1:<port>` for the local nodes
  and orchestrator. This prevents the orchestrator's dashboard snapshot
  probes from trying to reconnect through wildcard `0.0.0.0` endpoints.
- [x] Added launcher contract assertions for both node loopback advertisement
  arguments. `cargo test --locked --test run_examples_launcher --
  --test-threads=1`, shell syntax, formatting and `git diff --check` passed.
- [x] A bounded release smoke with the web dashboard reached
  `/api/config`, printed the selected dashboard URL and port, produced no
  `NM_MANAGEMENT_BEARER_TOKEN` startup error, produced no snapshot transport
  errors after node registration, and left no launcher-owned process.
- [!] This remains local reference-launcher evidence; it does not close the
  production network-consensus, deployed-identity, physical migration or
  multi-host RPO/RTO gates listed above.

## Progress update — 2026-09-06: authenticated outgoing peer RPC coverage

- [x] Added a reusable `authenticated_request` boundary in `src/node_auth.rs`
  and used it for outgoing live snapshot assembly, cluster-cut shard reads,
  GA evaluation forwarding, and both legacy spike-stream constructors. The
  helper is a no-op in the reference profile and fails before transmission
  when live credentials are absent or inconsistent.
- [x] Added receiver-side live-session validation for `GetNetworkSnapshot`,
  `GetClusterNetworkSnapshot` and `RunGAEvaluation`, binding metadata to the
  configured per-node token and mTLS leaf fingerprint before method work.
- [x] Validation passed: five node-auth tests, 33 distributed tests and
  `cargo check --locked --features stable_executor_live --lib --bin
  aarnn_rust`; formatting passed after the change.
- [!] The existing opt-in SwarmHPC Kubernetes profile still cannot claim live
  identity readiness. Its engine pods currently receive ephemeral generated
  node IDs, while the live protocol requires a per-node token and certificate
  fingerprint mapping. The profile must be extended with an explicit
  identity-provider/secret projection and stable `--node-id` binding before
  `NM_CAUSAL_TRANSPORT_LIVE=1` is rendered. Shared or wildcard credentials are
  prohibited.

## Progress update — 2026-09-06 00:54Z: host-bound deployment node identity

- [x] Updated the canonical SwarmHPC AARNN role to pass an explicit
  orchestrator `--node-id` and derive worker IDs from the Kubernetes
  `spec.nodeName` through the existing daemonset host boundary. A worker
  restart on the same host therefore retains its node identity instead of
  generating a new random writer identity.
- [x] Added a fail-closed stable-profile assertion requiring daemonset mode,
  the host-bound identity source and a non-empty orchestrator identity. The
  deployment still does not render live causal transport or shared
  credentials; no wildcard token or certificate was introduced.
- [x] Added `scripts/qa/validate-ansible-stable-profile.py` and its wrapper.
  The contract check passed against the canonical external role and
  `continuum_tenant_aarnn_site.yml --syntax-check` passed. The container
  entrypoint wrapper test also confirmed that an explicitly supplied
  provider-bound ID is forwarded unchanged.
- [!] Unique per-node token and mTLS certificate projection/rotation remain
  required before `NM_CAUSAL_TRANSPORT_LIVE=1` or production migration is
  enabled. Host-bound IDs solve restart identity churn but do not prove
  certificate uniqueness or cross-host causal routing.

## Verification update — 2026-09-06: physical placement proposal probe

- [x] Ran the repository's read-only Ansible placement harness against the
  existing inventory using the configured SSH access. `qc01`–`qc04`, `sm00`
  and `sm01` responded during the probe; `qc00` and `qc05` were excluded by
  reachability. Seven shards were proposed across five explicitly granted
  enrolled nodes with `degraded_durability=false` and `applied=false`.
- [x] An explicit consolidation proposal to `sm00` was rejected when the
  request retained a distinct warm-replica requirement, proving the planner
  preserves the durability constraint. The same proposal succeeded only after
  the request explicitly allowed the single-host migration tradeoff; it still
  remained proposal-only and reported `applied=false`.
- [x] `continuum_tenant_aarnn_site.yml --syntax-check` passed. No remote files,
  services or orchestrator state were changed by these probes.
