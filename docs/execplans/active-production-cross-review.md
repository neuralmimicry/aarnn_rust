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

## Current status

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

## Outcomes & Retrospective

The repository currently provides deterministic and governed reference slices,
not a production distributed whole-brain deployment. External hardware,
multi-process quorum/recovery, browser/native I/O, scientific datasets and
migration evidence are required to close the remaining blockers.

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
