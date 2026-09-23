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

- [x] `2026-09-20` Diagnosed the Kubernetes AARNN orchestrator rollout
  failure on `spirit`: the pod pulled the expected image and exited with code
  1 because the shared all-feature binary started `management_v1` without
  `NM_MANAGEMENT_BEARER_TOKEN`. The ordinary Ansible deployment does not
  provide the management bearer, mTLS files or durable management state; its
  intended profile is the existing distributed data plane. Added the explicit
  `NM_MANAGEMENT_ENABLED` runtime gate and set it to `0` for ordinary AARNN
  Kubernetes workloads while the stable migration profile sets it to `1` with
  its existing fail-closed credentials. Focused startup-profile tests and
  Ansible syntax validation passed; a fresh image build and deployment rollout
  remain required before claiming the live pod recovered.

- [x] `2026-09-19` Fixed the x64 CI test regression from commit `8ef72b0`.
  The complete feature graph is intentional for container images so deployed
  runtimes include the current parallel/non-blocking paths. Updated the
  stable workload regression expectation and the workflow comment to record
  that contract; the focused `container_workload_profiles` test now passes
  locally. The opt-in stable migration feature remains available as a named
  Cargo profile for deployments that require that narrower boundary.

- [x] `2026-09-18` Implemented the Webots runtime/performance pass. Webots
  launchers now use the explicit `engine_runtime,ui,robot_io,cuda` profile by
  default and accept `--all-features` (or `NM_WEBOTS_RUNTIME_FEATURES=all`) as
  an opt-in. The shared shell/Python profile helpers gate local management TLS
  provisioning on `management_v1`, preventing plaintext explicit binaries from
  receiving TLS client settings. Distributed simulation now has a supervisor
  with one stepping task per hosted network, bounded per-network output queues,
  and independent output sender tasks. Workspace autosave captures owned JSON
  after the committed boundary and publishes it through a bounded background
  writer outside the network lock. Explicit and all-feature Cargo checks,
  launcher contract tests, shell/Python syntax and whitespace checks passed.
  Queue capacities are configurable with
  `NM_NETWORK_OUTPUT_QUEUE_CAPACITY` and
  `NM_WORKSPACE_AUTOSAVE_QUEUE_CAPACITY`; committed queue sends await capacity
  and do not use lossy `try_send`.
- [x] `2026-09-18` Fixed a launcher argument transport defect found by the
  explicit C. elegans CLI path. `webots_cargo_profile_args` emitted a
  multi-line fragment, while its callers consumed only the first line with
  `read -a`; Cargo therefore saw `--no-default-features` without
  `engine_runtime`/Rayon and failed on `into_par_iter`. The helper now emits
  the explicit Cargo arguments on one line. The release build of both Webots
  binaries and the 12 launcher contract tests pass.
- [x] `2026-09-18` Fixed the Webots lifetime asymmetry exposed by the CLI run.
  `run_webot.sh` now supervises local orchestrator, node, worker, bridge and
  UDS neural processes while Webots is running. A live C. elegans probe
  terminated `nn_uds_server`; the launcher reported the supervised role and
  status, stopped Webots through the existing cleanup path, returned status 1,
  and left no Webots/controller/UDS processes behind.

- [x] `2026-09-18` Investigated reports that neurons moved before the native UI
  Start control was pressed. The simulation thread's `playing=false` gate was
  correct: no `Runner::step` call occurs and the displayed biological clock
  remains at `t=0`. The motion came from UI-only topology PID interpolation,
  camera-pivot smoothing, centroid recentering and region-label smoothing during
  paused layout refreshes. `src/ui.rs` now snaps those presentation states to
  the authoritative topology while paused, retains camera-drag animation, and
  includes a feature-gated regression test for stale PID state. Focused feature
  compilation, the focused test, the all-feature check, the existing 387-test
  library suite, formatting and whitespace checks all passed. Existing compiler
  warnings remain non-fatal and unrelated to this fix.

- [x] `2026-09-18` Investigated the flat Graphic EQ shown while `message.wav`
  was loaded. The audio provider decoded the file and the simulation advanced,
  but the UI retained a `spectral_bands` read guard across the complete frame
  while the simulation publisher used a non-blocking write. The UI now clones
  the short-lived band snapshot and releases the lock before rendering, so FFT
  bands can be published on the next simulation frame. The focused audio
  provider test, feature compilation, formatting and diff checks passed.

- [x] `2026-09-18` Reproduced the remaining flat Graphic EQ case after the
  lock-lifetime fix. Interactive `Choose File...` correctly replaced the
  simulation provider and emitted sensory spikes, but `sim_audio_diagnostic`
  was true only for audio loaded during application startup. The normal loop
  therefore published `last_bands()` only inside that startup diagnostic gate:
  a selected `message.wav` could drive the network while never reaching the
  EQ renderer. EQ publication now occurs for every provider that exposes
  spectral bands; the diagnostic flag controls periodic logging only. The
  focused provider test, locked UI feature check, formatting and whitespace
  checks passed. The rebuilt release launcher was then exercised with
  `/home/pbisaacs/Downloads/message.wav`; the native UI path reached the
  dashboard and logged `frame=120 eq_peak=0.7662 sensory_spikes=14/64` and
  `frame=180 eq_peak=0.8470 sensory_spikes=14/64` before bounded cleanup.

- [x] `2026-09-17` Cross-checked the former `scripts/tcp_aer_ipc_bridge.py`;
  its only
  launcher use is one per Unreal/Unity distributed brain from
  `scripts/run_sim.sh`; `scripts/nao_social_server.py` imports only the
  length-prefixed frame and legacy AER codec helpers. The simulator bridge is
  now implemented in `src/tcp_aer_ipc_bridge.rs` plus the
  `tcp_aer_ipc_bridge` binary, with the NAO helpers isolated in
  `scripts/aer_legacy_codec.py`. The Rust path retains bounded framing,
  asynchronous TCP/Unix-datagram I/O, logical AER timestamps, atomic
  environment readiness and the neural arm gate. The optimized release build,
  three Rust protocol tests, ten launcher contract tests, shell/Python syntax,
  and a live TCP/Unix-datagram loopback probe all pass.

- [x] `2026-09-17` Replaced the simulator Python bridge with the modular Rust
  `tcp_aer_ipc_bridge` binary. `run_sim.sh` builds or validates the release
  binary, and the NAO proxy's three legacy codec imports now come from
  `scripts/aer_legacy_codec.py`; no launcher imports or executes the removed
  Python bridge remain. The old Python bridge file is deleted; the legacy NAO
  codec remains isolated and intentionally Python because its owning proxy is
  Python.

- [x] `2026-09-14` Fresh automatic backend verification on the reported NVIDIA
  GeForce RTX 2080 stack passed all three AARNN parity tests with
  `NM_ENABLE_OPENCL_IN_TESTS=1 NM_GPU_BACKEND=auto cargo test --locked
  --no-default-features --features 'engine_runtime,ui,cuda' --lib
  'runner::tests::aarnn_gpu' -- --nocapture`. The startup probe measured
  OpenCL at `0.844 ms` and CUDA at `0.374 ms`, selected CUDA, and the three
  morphology/delay/growth and adaptation/neuromodulation tests passed. NVRTC
  PTX loading reported the installed driver's unsupported PTX version; the
  device-matched `nvcc` CUBIN fallback succeeded, so this remains a verified
  local compatibility path rather than an ignored CUDA failure.

- [x] `2026-09-14 12:20Z` Closed the CUDA dispatch gap found during the
  cross-backend audit. `src/gpu_api.rs` now dispatches the full `aarnn_step`
  signature and the voltage-gated `syn_filter` signature used by
  `CUDA_PROGRAM_SOURCE`; CUDA release, homeostasis, neuromodulation and growth
  dispatches are covered by the same argument contract. Release-mask kernels
  now perform their final comparison in `f32`, matching the Rust reference cast
  points and preventing precision-dependent event admission. `cargo check
  --locked --no-default-features --features engine_runtime,ui,cuda`, the
  OpenCL hardware gate, and all three opt-in AARNN runner parity tests passed.
  CUDA hardware execution remains unverified on this host and is still a
  required production gate.

- [x] `2026-09-14 12:28Z` Repository-wide verification after the CUDA and
  release-mask fixes passed: `cargo test --locked --workspace` (286 library
  tests plus all integration suites), `cargo check --locked --all-features
  --all-targets`, `cargo check --locked --no-default-features
  --features engine_runtime,ui,cuda`, `cargo fmt --all --check`, and
  `git diff --check`. The existing warnings remain non-fatal and unrelated to
  the backend contract changes.

- [x] `2026-09-14` Extended the AARNN accelerator gate and runner to cover the
  remaining numeric biology stages on the local NVIDIA GeForce RTX 2080
  OpenCL device. `src/cl_compute.rs` now certifies deterministic release
  masks, adaptive-threshold/homeostatic decay and spike feedback,
  neuromodulator/resonance EMA, growth eligibility, morphology energy, STP,
  delayed sparse accumulation, filtering, plasticity, and the AARNN membrane
  transition against CPU reference vectors before a device is admitted.
  `Runner` uses those stages transactionally with CPU fallback on any device
  error; GPU release/growth results are proposal inputs only, while topology
  publication and canonical morphology event ordering remain ordered CPU
  commit boundaries required by `INV-009` and `INV-014`. Negative delays are
  guarded before history indexing by both OpenCL and CUDA delayed kernels.
  The opt-in constructor gate and multi-step AARNN runner tests pass on the
  RTX 2080, as does the morphology/growth deterministic ordering test with
  `growth3d,morpho`. Broader device/driver coverage and long replay evidence
  remain required before production cutover.

- [x] `2026-09-14 10:44Z` Fixed the cluster input/control mismatch found in the
  native launcher. `src/ui.rs` now routes the selected audio-file or selected
  microphone provider into the managed network after cluster Start/Repeat,
  without stepping the UI's standalone Runner; `src/distributed.rs` admits a
  shape-checked external sensory batch with bounded single-entry backpressure
  and forwards it through the existing shard transport. Cluster GPU status now
  reports the selected managed view's playing state and distinguishes worker
  GPU reporting from the local standalone runner. The focused admission test,
  launcher contract tests, shell/Python checks and the live `run_examples.sh`
  relocation/growth probe passed. The WAV launcher run decoded
  `/home/pbisaacs/Downloads/message.wav` and produced the expected EQ/spike
  diagnostics. The later accelerator parity gate extends the managed AARNN
  numeric stages as recorded below; the launcher retains CPU fallback when a
  device/profile equivalence check fails.

- [x] `2026-09-14` Rebuilt after the parity extension and verified the focused
  OpenCL hardware gate with both `opencl` and `opencl,growth3d,morpho`, plus
  `runner::tests::aarnn_gpu*` on the RTX 2080. `cargo check --locked
  --no-default-features --features engine_runtime,ui,opencl` and formatting
  passed. Heterogeneous biological profiles still select the documented CPU
  reference fallback until per-neuron device parameter buffers are certified;
  this preserves equivalence rather than silently changing the model.

- [x] `2026-09-14` The requested GPU/microphone review was closed in the
  canonical `src/runner.rs`, `src/cl_compute.rs`, `src/ui.rs` and
  `src/providers.rs` paths. The native UI reports paused/model/fallback state
  separately, the certified LIF/Izh and AARNN paths select the GPU when their
  equivalence gate passes, and CPAL exposes stable microphone enumeration and
  selection. Unsupported heterogeneous AARNN profiles retain the CPU
  reference fallback rather than silently substituting Izhikevich dynamics.

- [~] `2026-09-11` Live distributed AARNN logs showed early-cell formation followed
  by repeated interface counters returning to `1/32` and `1/16`. The heartbeat
  path treated every larger layer count as a placement change and reissued a
  legacy `LoadNetwork` command containing the older snapshot, resetting worker
  growth. `src/distributed.rs` now treats count increases for already hosted
  layers as telemetry only; the focused regression test proves that this path
  does not request a snapshot reload, while a new hosted layer still requests
  placement work. Deployment and live growth evidence remain pending.

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

## WebGL browser simulator milestone — 2026-09-15

This milestone extends the existing Web product with a browser-runnable WebGL
simulation surface. It is scoped to the established `web_ui` gateway and
cluster runtime: the browser owns rendering and robot sensor/actuator adapters;
Rust remains the sole neural executor and authoritative AER admission path.

Traceability: specification sections 16.12, 16.20, 16.21 and 21.11;
`INV-002`, `INV-003`, `INV-005`, `INV-007`, `INV-008`, `INV-010`, `INV-015`,
`INV-016`, `INV-017`; browser capability and permission rules in
`APP-INV-002`–`APP-INV-004` where applicable. The launcher uses the existing
cluster worker/shard path, so `--node/--nodes` remains an operational placement
parameter rather than a browser semantic.

Canonical files after discovery:

- `src/bin/web_ui.rs`: authenticated browser gateway and AER inference route.
- `web_ui/webgl-sim.html`, `web_ui/webgl-sim.js`: browser scene and robot
  adapters, served as static embedded assets by `web_ui`.
- `scripts/run_webgl_sim.sh` and `scripts/run_sim.sh`: browser simulator
  launcher and shared robot/node configuration.
- `scripts/robot_profiles.py`: single source for robot sensory/output counts.
- `tests/web_ui_browser_compat.rs` and `tests/run_examples_launcher.rs`:
  browser asset and launcher contract checks.

Milestones:

- [x] Browser scene and six profile adapters render through WebGL and expose
  explicit sensor/actuator counts matching `scripts/robot_profiles.py`.
- [x] The gateway accepts a bounded inference request, forwards it through the
  same distributed AER path, and returns post-admission output spikes without
  exposing TCP or Unix sockets to the browser.
- [x] `scripts/run_sim.sh --sim webgl --robots ... --nodes N` starts the shared
  cluster and web gateway, prints the browser URL, and preserves cleanup.
- [x] Browser compatibility, launcher contract, shell/Python syntax and a live
  gateway round trip pass; unavailable WebGL is reported as a capability error.

Rollback boundary: remove the additive WebGL assets, route, and launcher. No
persisted schema or biological runtime semantics change. The browser remains a
management-only client if the simulator surface is unavailable.

## Progress update — 2026-09-15: WebGL browser simulator

- [x] `2026-09-15 07:30Z` Added the WebGL page, native WebGL renderer, six shared
  robot profile adapters, explicit capability messaging, and the authenticated
  `/api/aer/infer` gateway. The browser sends profile-sized sensory frames and
  maps returned output spikes to the visible actuator state; it never opens a
  TCP or Unix socket and does not claim global HID or media access.
- [x] `2026-09-15 07:35Z` Extended `scripts/run_sim.sh` with `--sim webgl`,
  `--web-port`, and `--orchestrator-port`; the launcher starts the existing
  `run_webot.sh` cluster with an isolated IPC directory and the release
  `web_ui`, preserving `--node/--nodes` shard distribution and cleanup.
- [x] `2026-09-15 07:42Z` Focused and live evidence passed: `cargo test --locked
  --test web_ui_browser_compat --test run_examples_launcher` (all tests
  passed), `cargo check --locked --bin web_ui --no-default-features --features
  engine_runtime,ui`, `cargo fmt --all --check`, `git diff --check`, shell/
  Python/Node syntax checks, and a live `--nodes 2` WebGL run. The live page
  and asset loaded, inference returned HTTP 200 with `accepted_batches: 1` and
  a worker target, and 8,193 input values were rejected with HTTP 413.
- [x] `2026-09-15` Diagnosed the reported one-minute control-plane failure in
  `logs/webgl_sim_109366`: the cluster node binary had been built by the WebGL
  launcher with only `engine_runtime,ui`, so `--ipc` could not bind the
  `robot_io` Unix socket. `run_webot.sh` then waited its 60-second socket
  deadline and shut down the cluster; the web UI was a casualty of launcher
  teardown, not the initiating crash. WebGL now builds `aarnn_rust` with
  `engine_runtime,ui,robot_io,cuda`, builds `web_ui` separately, and rejects an
  incompatible `--no-build` binary with an actionable error. A 35-second live
  `--no-build --nodes 2` run reached the gateway URL and logged both brain and
  worker IPC readiness.
- [x] `2026-09-15` Explained and corrected the apparent native/browser output
  raster mismatch: both labels count active cells across a sliding history
  window, rather than reporting only the latest simulation step, but the web
  window was 180 columns while the Rust UI retains 240. The browser window now
  matches the native `raster_cols` value. Cross-node comparisons still require
  selecting the same node because Rust `Local managed` reads its local runner
  while web `Auto` resolves the currently ranked active worker.
- [x] `2026-09-15` Corrected distributed activity semantics for the WebGL/control
  plane. Placement layer numbering excludes the sensory vector, so layer 0 is
  the imported 302-neuron hidden layer and the final layer is the 96-neuron
  output layer. The browser had used the separate sensory envelope for layer 0
  and shifted hidden activity onto the output shard, producing the reported
  `0/302` and misleading `16/96` display. The activity RPC now also publishes
  the current sensory history frame, allowing the browser graph/probes to show
  the actual 24-channel input independently. The focused browser compatibility
  suite (8 tests), distributed activity RPC regression, release builds for both
  binaries, and a live two-node gateway probe passed. The live activity samples
  showed 14/302 and 9/302 hidden spikes, confirming that the original layer is
  active.

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

## Verification update — 2026-09-14: local shard relocation and growth probe

- [x] Added the opt-in `AARNN_VERIFY_SHARD_GROWTH=1` mode to `run_examples.sh`
  and the bounded `scripts/qa/verify_example_sharding.py` helper. It copies
  the supplied snapshot into the selected runtime root, adds finite growth
  headroom and same-layer growth settings, verifies complete two-node active
  layer coverage, stops the current active owner, waits for automatic
  relocation, reads the surviving worker's actual snapshot, and requires
  further neuron growth after relocation. The checked-in `network.json` is
  never modified.
- [x] The local end-to-end run passed with
  `AARNN_VERIFY_SHARD_GROWTH=1 AARNN_SKIP_BUILD=1 AARNN_NATIVE_UI=0`; it
  observed a two-owner `cluster_master` assignment, stopped the owner of
  layer 0, observed relocation to the remaining node, and saw the destination
  snapshot retain 567 neurons and grow to 571 after relocation. Cleanup left
  no launcher-owned process.
- [x] `bash -n run_examples.sh`, Python syntax compilation, `cargo fmt --all
  --check` and `git diff --check` passed. The launcher contract suite now has
  four tests and remains separate from the stable-executor path.
- [!] This is live evidence for the current legacy layer-assignment
  compatibility path. It does not promote that path to authoritative stable
  shard ownership or close the production quorum, durable migration, identity,
  multi-host RPO/RTO or scientific parity gates.

## Verification update — 2026-09-14: WAV-backed Rust UI EQ and sensory admission

- [x] The canonical `src/providers.rs` audio provider now rejects zero sensory
  capacity, rejects empty or non-finite decoded audio, reports sample metadata,
  and fails on packet decode errors instead of silently accepting a bad source.
  Audio-to-spike dithering uses a counter-based frame/neuron coordinate, so the
  same decoded source produces the same sensory raster across provider runs.
- [x] The native Rust UI accepts an explicit `AARNN_AUDIO_FILE` startup source
  and bounded `AARNN_AUDIO_SENSORY_NEURONS` count. When a supplied snapshot has
  zero sensory neurons, the UI provisions the requested bounded input layer
  before loading the source. The UI displays the file metadata and last
  sensory spike count, enables the Graphic EQ, and autoplays an explicit source
  by default (`AARNN_AUDIO_AUTOPLAY=0` disables that behaviour).
- [x] Provider tests passed with `cargo test --locked --features ui --lib
  providers::tests -- --nocapture`: WAV decode/EQ energy, repeatable nonzero
  spikes, silence-to-zero policy, and zero-sensory rejection all passed.
- [x] The exact launcher release profile passed:
  `cargo build --release --locked --no-default-features --bin aarnn_rust
  --bin web_ui --features "engine_runtime,ui"`.
- [x] The supplied `/home/pbisaacs/Downloads/message.wav` was exercised through
  `run_examples.sh` with the native UI enabled. The orchestrator reached the
  dashboard at `http://127.0.0.1:8081`; its audio log recorded `8000 Hz`,
  `192160` mono samples, `8 EQ bands`, `S=64`, then
  `frame=60 eq_peak=0.7723 sensory_spikes=9/64` and
  `frame=120 eq_peak=0.7662 sensory_spikes=14/64`. The bounded run exited by
  timeout and launcher cleanup completed; no launcher-owned process remained.
- [!] This proves the local Rust UI/reference audio path and its visible EQ and
  sensory spike response. It does not promote the UI's direct provider into the
  specification's governed peripheral-session/transducer media plane, and it
  does not close the repository's separate production quorum, identity,
  durability, migration, or scientific validation gates.

## Verification update — 2026-09-14: isolated final launcher evidence

- [x] Added bounded launcher port-start overrides and an opt-out for default
  discovery targets. The local example now keeps explicit test endpoints
  isolated from stale nodes that may still reconnect to the historical
  broadcast port; normal discovery defaults remain unchanged outside the
  launcher override.
- [x] Rebuilt the exact locked release profile after the discovery isolation
  change:
  `cargo build --release --locked --no-default-features --bin aarnn_rust
  --bin web_ui --features "engine_runtime,ui"`.
- [x] The final isolated sharding run used ports 52051/52075/52087/8280 and
  `AARNN_VERIFY_SHARD_GROWTH=1`. It observed two intended owners, stopped the
  layer-0 owner, relocated all layers to the survivor, retained the snapshot,
  and observed growth from 551 to 555 after relocation. Launcher cleanup
  completed successfully.
- [x] The final isolated WAV run used ports 53051/53075/53087/8380 with
  `/home/pbisaacs/Downloads/message.wav`, `S=64`, and autoplay. The native UI
  log recorded `8000 Hz`, `192160` samples, `8 EQ bands`, then
  `frame=60 eq_peak=0.7723 sensory_spikes=9/64` and
  `frame=120 eq_peak=0.7662 sensory_spikes=14/64`.
- [x] The five-test launcher contract suite, four provider tests, shell/Python
  syntax checks, formatting check and diff check passed after the final edits.

## Verification update — 2026-09-14: production microphone stream conversion

- [x] Replaced the selected microphone's typed `f32` fallback with CPAL's raw
  callback and explicit conversion for signed/unsigned 8/16/24/32/64-bit and
  f32/f64 PCM formats. DSD and unknown formats fail closed instead of being
  reinterpreted as floating-point samples. Non-finite callback samples are
  discarded before admission to the bounded analysis buffer.
- [x] The locked release profile rebuilt successfully after this change:
  `cargo build --release --locked --no-default-features --bin aarnn_rust
  --bin web_ui --features "engine_runtime,ui"`.
- [x] Provider tests passed: five tests covering microphone enumeration, WAV
  decode/Graphic EQ energy, repeatable nonzero sensory spikes, silence, and
  bounded sensory admission.
- [x] The final native UI launcher run used the supplied
  `/home/pbisaacs/Downloads/message.wav` with `AARNN_UI_NEURON_MODEL=lif`,
  autoplay, and isolated ports. It initialized the NVIDIA GeForce RTX 2080
  OpenCL backend and recorded `8000 Hz`, `192160` samples, `8 EQ bands`, then
  `frame=60 eq_peak=0.7723 sensory_spikes=9/64` and
  `frame=120 eq_peak=0.7662 sensory_spikes=14/64`. Launcher cleanup left no
  launched runtime process.
- [x] The final focused hardware gate passed with
  `NM_ENABLE_OPENCL_IN_TESTS=1`; this validates accelerator initialization and
  reference equivalence on the detected GPU. LIF/Izh and homogeneous AARNN
  runs select the GPU when active; unsupported heterogeneous profiles retain
  the certified CPU reference fallback until per-neuron parameter buffers are
  proven across the device matrix.

## Verification update — 2026-09-14 11:45Z: full AARNN accelerator parity and end-to-end closure

- [x] The certified OpenCL constructor gate passed on the local NVIDIA GeForce
  RTX 2080 with `opencl,growth3d,morpho`. Its reference vectors cover the
  AARNN membrane transition, morphology energy, delayed sparse accumulation,
  release probability, STP, synaptic filtering, plasticity, adaptive
  threshold, homeostasis, neuromodulation/resonance and growth eligibility.
  CUDA parity code also compiles with `cargo check --locked
  --no-default-features --features 'engine_runtime,ui,cuda'` against the
  installed CUDA toolkit.
- [x] The focused AARNN runner tests passed:
  `cargo test --locked --no-default-features --features
  'engine_runtime,ui,opencl,growth3d,morpho' --lib 'runner::tests::aarnn_gpu'
  -- --nocapture`. CPU commit boundaries remain authoritative for topology
  publication and canonical event ordering; GPU release and growth outputs are
  admission proposals, preserving `INV-009`, `INV-014` and replay ordering.
- [x] The explicit morphology ordering regression passed:
  `runner::tests::morphology_release_order_is_independent_of_producer_order`.
  This verifies that producer/work-group order cannot change the canonical
  released-event sequence even when accelerated numeric stages are enabled.
- [x] The tightened relocation helper now requires both a snapshot increase
  after relocation and a second increase afterward, so it cannot pass solely
  because the pre-relocation count was retained or the growth cap was reached.
  The rebuilt `run_examples.sh` probe passed with native UI and audio enabled:
  the survivor grew from `542` to `547` after relocation. The CPU-only probe
  also passed (`567` to `571`).
- [x] The native Rust UI run with
  `/home/pbisaacs/Downloads/message.wav`, `AARNN_AUDIO_AUTOPLAY=1` and
  `AARNN_AUDIO_SENSORY_NEURONS=64` logged `OpenCL GPU device: NVIDIA GeForce
  RTX 2080`, decoded `8000 Hz`, `192160` samples and `8 EQ bands`, and emitted
  nonzero sensory activity: `frame=60 eq_peak=0.7723 sensory_spikes=9/64`,
  `frame=120 eq_peak=0.7662 sensory_spikes=14/64`, and
  `frame=180 eq_peak=0.8470 sensory_spikes=14/64`.
- [x] Final repository checks passed: `cargo check --locked --all-features
  --all-targets`, `cargo test --locked --workspace` (all executed tests passed),
  `cargo fmt --all --check`, `git diff --check`, shell/Python syntax checks,
  and the five launcher contract tests. Warnings remain in the pre-existing
  broad codebase; no new compile or test failure remains.
- [!] The local evidence closes repository-level implementation and regression
  checks for this change. It does not claim the separate normative production
  gates for multi-host device matrices, durable authoritative shard ownership,
  quorum migration, identity/security deployment or scientific validation.

## Verification update — 2026-09-14 11:59Z: post-growth accelerator cache barrier

- [x] The extended replay initially exposed a stale sparse morphology receiver
  cache after the first committed AARNN growth event. The OpenCL upload path
  could index the old row count when the hidden layer had already grown.
  `Runner` now validates morphology route-cache dimensions against the live
  topology at each biological-kernel boundary and rebuilds the cache before
  accelerator use; sparse input upload also fails safely on a transiently
  short cache instead of panicking.
- [x] The replay fixture now runs 32 one-millisecond transitions, crossing the
  documented 25 ms minimum early-cell maturation period. It compares CPU and
  OpenCL state through a committed neuron growth event, including voltages,
  recovery, adaptive/homeostatic thresholds, STP, weights, growth cooldowns,
  morphology, neuromodulation/resonance and ordered release events.
  `NM_ENABLE_OPENCL_IN_TESTS=1 cargo test --locked --no-default-features
  --features 'engine_runtime,ui,opencl,growth3d,morpho' --lib
  runner::tests::aarnn_gpu -- --nocapture` passed all three focused tests on
  the NVIDIA GeForce RTX 2080.
- [x] The same feature set passed
  `cl_compute::tests::hardware_reference_gate_is_opt_in` with the opt-in
  OpenCL hardware gate, and the deterministic morphology release-order
  regression passed. `cargo check --locked --all-features --all-targets`,
  `cargo test --locked --workspace`, `cargo fmt --all --check` and
  `git diff --check` also passed after the barrier change.
- [x] The rebuilt release binaries passed the live headless launcher probe:
  `AARNN_VERIFY_SHARD_GROWTH=1 AARNN_SKIP_BUILD=1 AARNN_NATIVE_UI=0
  AARNN_VERIFY_TIMEOUT_S=45 ./run_examples.sh` sharded 396 neurons across two
  workers, relocated the stopped owner, retained a 558-neuron survivor
  snapshot and continued growth to 563 before clean shutdown. The explicit
  CUDA feature compile also passed with
  `cargo check --locked --no-default-features --features
  'engine_runtime,ui,cuda'`.
- [!] This remains local device evidence. Multi-device/driver coverage,
  multi-host authoritative ownership, durability/fencing, security deployment
  and scientific validation remain separate production gates.

## Verification update — 2026-09-14: voltage-gated filtering and morphology safety audit

- [x] Closed the remaining AARNN kernel parity gap in `src/cl_compute.rs` and
  `src/runner.rs`: OpenCL and CUDA `syn_filter` now receive the pre-transition
  membrane voltage and apply the same bounded voltage-dependent NMDA gate as
  `aarnn::dynamics::apply_synaptic_filter`. The standalone device gate tests
  this branch with nonzero sensitivity, and
  `aarnn_gpu_membrane_step_matches_reference_runner` exercises it through the
  real runner dispatch.
- [x] Corrected an unsafe morphology/delay dispatch path. AARNN sparse GPU
  accumulation could bypass per-synapse release decisions and omit committed
  release-event records. AARNN morphology and physical-delay routing now stays
  on the ordered CPU accumulation/event path; certified GPU transition,
  filtering where applicable, STP, adaptive threshold, homeostasis,
  neuromodulation, plasticity, release/growth proposals and deterministic
  reference gates remain available without allowing structural divergence.
  This preserves `INV-009`, `INV-014` and canonical event ordering.
- [x] The no-default-features AARNN lane passed all three focused GPU tests and
  the OpenCL hardware gate on the NVIDIA GeForce RTX 2080:
  `NM_ENABLE_OPENCL_IN_TESTS=1 cargo test --locked --no-default-features
  --features 'engine_runtime,ui,opencl,growth3d,morpho' --lib
  'runner::tests::aarnn_gpu_' -- --nocapture` and the corresponding
  `cl_compute::tests::hardware_reference_gate_is_opt_in` invocation.
  `cargo check --locked --no-default-features --features
  'engine_runtime,ui,cuda'` also passed.
- [x] `cargo test --locked --workspace` passed, as did
  `cargo check --locked --all-features --all-targets`, formatting, diff and
  launcher syntax checks. The live relocation probe sharded 396 neurons across
  two workers, relocated the stopped owner, retained 559 neurons on the
  survivor and continued growth to 563. The live WAV probe logged GPU
  initialization, `8000 Hz`, `192160` samples, `8 EQ bands`,
  `frame=60 eq_peak=0.7723 sensory_spikes=9/64` and
  `frame=120 eq_peak=0.7662 sensory_spikes=14/64`; cleanup left no launched
  runtime processes.
- [!] The evidence remains local RTX 2080/OpenCL plus CUDA compilation. CUDA
  hardware execution, multi-device/driver equivalence, durable authoritative
  shard ownership, quorum migration, identity/security deployment and
  scientific validation remain separate production gates.

## Verification update — 2026-09-14: CUDA hardware execution and automatic latency selection

- [x] CUDA hardware initialization now passes on the local NVIDIA GeForce RTX
  2080. The installed CUDA 13.3 NVRTC emits PTX rejected by the installed
  595.84 driver's CUDA 13.2 compatibility level; `src/gpu_api.rs` reports the
  real driver/NVRTC diagnostic and falls back to a device-matched `sm_75`
  CUBIN compiled by `nvcc`. The resulting CUDA module passes the same full
  reference-equivalence gate as OpenCL.
- [x] Forced CUDA execution passed all three focused AARNN runner tests,
  including morphology, delays, release, STP, adaptive threshold, homeostasis,
  neuromodulation, plasticity, growth and ordered replay. Forced OpenCL passed
  the same suite afterward.
- [x] Automatic selection passed with both devices available. The measured
  startup transactions were OpenCL `0.828 ms` and CUDA `0.372 ms` in focused
  tests, selecting CUDA. The release `run_examples.sh` profile now includes
  `cuda`, so the launched orchestrator and workers can perform that selection
  rather than being compiled OpenCL-only.
- [x] The rebuilt launcher selected CUDA in all three local processes. Its
  WAV-backed native UI run decoded `/home/pbisaacs/Downloads/message.wav` at
  `8000 Hz` with `8 EQ bands`, and emitted `frame=60 eq_peak=0.7723
  sensory_spikes=9/64` and `frame=120 eq_peak=0.7662 sensory_spikes=14/64`.
- [x] The rebuilt launcher relocation probe passed after the CUDA profile
  change: it stopped the layer-0 owner, observed relocation, retained the
  survivor snapshot at `534`, and observed post-relocation growth to `539`.
- [!] CUDA CUBIN fallback requires a usable `nvcc` installation when a driver
  rejects the runtime-generated PTX. If neither compatible NVRTC PTX nor
  `nvcc` CUBIN compilation is available, the certified CPU/OpenCL fallback is
  retained and the diagnostic is logged.

## Verification update — 2026-09-14 13:44Z: hardware-gated parity recheck and UI status correction

- [x] Re-ran the focused AARNN parity suite with the hardware gate enabled,
  rather than relying on the opt-in test's skip behavior. Forced CUDA passed
  all three tests on the RTX 2080, including the 32-step morphology/delay/
  release/STP/adaptation/homeostasis/neuromodulation/plasticity/growth and
  ordered-event replay. Forced OpenCL passed the same three tests.
  Commands were `NM_ENABLE_OPENCL_IN_TESTS=1 NM_GPU_BACKEND=cuda cargo test
  --locked --no-default-features --features 'engine_runtime,ui,cuda' --lib
  'runner::tests::aarnn_gpu' -- --nocapture` and the corresponding
  `NM_GPU_BACKEND=opencl` invocation.
- [x] The Rust UI no longer claims that AARNN GPU parity is pending. It now
  reports the certified AARNN accelerator path when the runner's homogeneous
  profile gate is true, and explicitly identifies the CPU reference fallback
  for heterogeneous cell profiles. `Runner::aarnn_gpu_transition_supported`
  is exposed as a crate-local status query so the display reflects the actual
  execution route.
- [!] The certified execution evidence remains local to the RTX 2080 and its
  installed OpenCL/CUDA stack. Multi-device/driver equivalence and the broader
  distributed production gates remain outside this focused parity objective.

## Verification update — 2026-09-14: transactional accelerator fallback recheck

- [x] Staged OpenCL/CUDA neuron readback now validates every CPU destination
  before publishing any voltage, recovery, refractory, adaptive-threshold or
  STP state. A missing manager, failed read, missing buffer or shape mismatch
  selects the complete CPU reference replay path instead of committing a
  partial or zero-filled state. STP publication uses the same all-populations
  transaction boundary.
- [x] The post-change forced OpenCL AARNN suite passed all three parity tests on
  the NVIDIA GeForce RTX 2080. The post-change automatic suite also passed all
  three tests, measured OpenCL at `0.857 ms` and CUDA at `0.340 ms`, and
  selected CUDA. CUDA initialization used the installed `nvcc` `sm_75` CUBIN
  fallback after the driver rejected CUDA 13.3-generated PTX.
- [x] `cargo check --locked --no-default-features --features
  'engine_runtime,ui,cuda'`, `cargo test --locked --workspace`,
  `cargo fmt --all --check` and `git diff --check` passed. The workspace suite
  completed both library and binary unit suites, integration suites, launcher,
  migration, failover, mobile and doc tests without failures.
- [!] Evidence remains local to the RTX 2080 and installed OpenCL/CUDA stack;
  multi-device/driver equivalence and the broader distributed production gates
  remain separate acceptance work.

## Verification update — 2026-09-14: final rebuilt launcher acceptance

- [x] The release launcher was rebuilt with the explicit `engine_runtime,ui,cuda`
  profile and then exercised with
  `/home/pbisaacs/Downloads/message.wav`, `AARNN_AUDIO_AUTOPLAY=1` and
  `AARNN_AUDIO_SENSORY_NEURONS=64`. With both accelerators available, the
  runtime measured OpenCL at `1.545 ms` and CUDA at `0.409 ms`, selected CUDA,
  and initialized the NVIDIA device through the device-matched `sm_75` CUBIN
  fallback after the expected CUDA 13.3 PTX versus driver 13.2 mismatch.
- [x] The native UI audio diagnostic recorded `8000 Hz`, `192160` samples and
  `8 EQ bands`; its deterministic sensory raster produced
  `frame=60 eq_peak=0.7723 sensory_spikes=9/64` and
  `frame=120 eq_peak=0.7662 sensory_spikes=14/64`. This verifies the graphic
  EQ path and nonzero sensory admission in the current CUDA-selected run.
- [x] The rebuilt headless launcher probe sharded `cluster_master` across two
  workers, stopped the layer-0 owner, relocated all active layers to the
  survivor, retained a snapshot of `559` neurons, and observed continued
  growth to `563`. The probe exited successfully and launcher cleanup left no
  launched runtime process.
- [x] The full workspace test, formatting check and `git diff --check` passed
  after the transactional accelerator, hidden-plasticity, CUDA recurrent-buffer
  and automatic backend-selection changes.

## Verification update — 2026-09-14 14:25Z: close morphology and fallback gaps

- [x] Asynchronous morphology evolution now passes the selected accelerator
  into `Morphology::evolve`; it no longer silently disables the GPU energy
  transaction when morphology runs on its worker thread.
- [x] GPU morphology energy now composes the same skull ambient field as the
  CPU reference, including the no-entity case. The new
  `morphology::tests::gpu_morphology_energy_matches_cpu_with_skull_ambient`
  test passed with forced OpenCL and forced CUDA.
- [x] The complete `runner::tests::aarnn_gpu_full_parity_replays_morphology_delays_and_growth`
  replay passed with forced OpenCL and forced CUDA. It compared delayed
  histories, currents, membrane/recovery state, adaptive threshold,
  homeostasis, STP, release records, all plasticity matrices, neuromodulation,
  morphology, growth cooldowns, committed growth and canonical event order.
- [x] The new heterogeneous-profile fallback test passed with automatic
  selection. A per-cell biology mismatch rejects the scalar GPU transition,
  keeps sparse accumulation off the uncertified route, and matches the CPU
  reference over six transitions.
- [!] GPU morphology and event computation remains transactional: the device
  computes spatial energy, delayed sparse accumulation, release masks and
  growth eligibility; CPU state publication, topology generation changes and
  canonical event sorting remain the authoritative commit boundary. Any
  unsupported profile or device/readback failure replays through the CPU
  reference path.

## Verification update — 2026-09-14: final workspace and automatic-route checks

- [x] The current tree passed `cargo test --locked --workspace`: 286 library
  tests passed, along with all enabled binary, integration and documentation
  suites. `cargo fmt --all --check`, `git diff --check`, `bash -n
  run_examples.sh` and `python3 -m py_compile
  scripts/qa/verify_example_sharding.py` also passed.
- [x] The current hardware-gated automatic AARNN run passed all five focused
  parity tests with `NM_ENABLE_OPENCL_IN_TESTS=1 NM_GPU_BACKEND=auto cargo test
  --locked --no-default-features --features 'engine_runtime,ui,cuda' --lib
  'runner::tests::aarnn_gpu' -- --nocapture`. On the RTX 2080 it measured
  OpenCL at `1.172 ms` and CUDA at `0.416 ms`, selected CUDA, and passed
  morphology, delays, release probability, STP, adaptive threshold,
  homeostasis, neuromodulation, plasticity, growth, heterogeneous-profile
  CPU fallback and deterministic event-order replay. CUDA used the
  device-matched `sm_75` CUBIN fallback because the installed CUDA 13.3 PTX is
  rejected by the CUDA 13.2-compatible driver.
- [x] Backend choice is automatic for every process startup when both GPU
  candidates are available: `OpenCLManager::new_with_preferred_device_index`
  initializes and equivalence-gates both candidates, measures the same
  synchronized auxiliary transaction including readback, and retains the
  lower-latency candidate. `NM_GPU_BACKEND=opencl` or `cuda` remains an
  explicit diagnostic override; absent that override, the measured route is
  selected. Device failure falls back through the certified alternative or
  CPU reference path.
- [!] Production scope remains bounded by the recorded evidence: the tested
  accelerator hardware is this local RTX 2080 stack, and heterogeneous
  per-cell AARNN profiles intentionally use the certified CPU reference path
  until per-neuron device parameter buffers receive equivalent replay
  evidence.

## Verification update — 2026-09-14 14:42Z: lossless event and upload fallback audit

- [x] Homeostatic threshold/rate decay and post-spike homeostasis now run on
  staged host arrays. A device error leaves the live population untouched and
  selects the CPU reference update for that population; a successful readback
  is published as one host commit. This closes the partial-mutation fallback
  risk while preserving the existing CPU-owned state boundary.
- [x] Sparse morphology CSR uploads now return success explicitly. Row
  pointers, indices, weights and delays are all uploaded before the host
  synapse-ID mapping is published; any failed write makes the complete sparse
  transaction ineligible and the caller takes the CPU path. All input,
  forward, backward, recurrent and output sparse callers now propagate this
  result.
- [x] Removed the fixed 256-record morphology event limit and GPU-side
  deduplication. Every released synapse event is retained and then sorted by
  the canonical key, preventing high fan-in/fan-out activity from being
  silently discarded under `INV-007` and keeping CPU/GPU replay equivalent.
- [x] With both accelerator features enabled, the hardware-gated suite passed:
  `NM_ENABLE_OPENCL_IN_TESTS=1 NM_GPU_BACKEND=auto cargo test --locked
  --no-default-features --features 'engine_runtime,ui,opencl,cuda,growth3d,morpho'
  --lib 'runner::tests::aarnn_gpu' -- --nocapture`. All five tests passed;
  OpenCL measured `0.974 ms`, CUDA `0.406 ms`, and CUDA was selected through
  the `sm_75` CUBIN fallback.
- [x] After final event-path cleanup, the same five-test command passed again;
  the fresh probe measured OpenCL `0.833 ms`, CUDA `0.351 ms`, and selected
  CUDA.
- [x] The rebuilt release `run_examples.sh` relocation mode passed with
  `AARNN_VERIFY_SHARD_GROWTH=1`: it sharded 396 neurons across two workers,
  stopped the layer-0 owner, relocated to the survivor, retained 560 neurons
  and continued growth to 563. The WAV-backed native UI run selected CUDA
  after measuring OpenCL `0.845 ms` and CUDA `0.346 ms`, decoded
  `/home/pbisaacs/Downloads/message.wav` at `8000 Hz` with 8 EQ bands, and
  logged `frame=60 eq_peak=0.7723 sensory_spikes=9/64`,
  `frame=120 eq_peak=0.7662 sensory_spikes=14/64`, and
  `frame=180 eq_peak=0.8470 sensory_spikes=14/64`.
- [!] Remaining production gates are unchanged: multi-device/driver replay,
  authoritative durable shard ownership and recovery, security/deployment
  acceptance, and scientific validation still require their separate evidence.

## Verification update — 2026-09-14: independent backend and launcher recheck

- [x] The current tree passed `cargo test --locked --workspace`: 286 library
  tests passed, along with all binary, integration and documentation suites.
  The focused provider checks also passed: the WAV decoder/graphic-EQ test
  emitted repeatable nonzero bands and sensory spikes, and microphone
  enumeration returned a stable selectable device list (or an explicit error
  if the host provides none). `cargo fmt --all --check`, `git diff --check`,
  `bash -n run_examples.sh` and Python bytecode validation passed again.
- [x] Forced OpenCL and forced CUDA each passed all five
  `runner::tests::aarnn_gpu` tests with `growth3d,morpho` enabled. This
  independently verified the AARNN route for morphology, delays, release
  probability, STP, adaptive threshold, homeostasis, neuromodulation,
  plasticity, growth, heterogeneous-profile fallback and deterministic event
  ordering. CUDA again used the device-matched `sm_75` CUBIN fallback after
  the installed driver rejected CUDA 13.3 PTX.
- [x] Automatic selection with both candidates available measured OpenCL at
  `0.842 ms` and CUDA at `0.389 ms`, selected CUDA, and passed all five tests.
  The selector is therefore exercised in the same process that executes the
  full parity fixture; the forced runs separately prove each candidate.
- [x] A fresh headless `run_examples.sh` verification with
  `AARNN_VERIFY_SHARD_GROWTH=1`, `AARNN_NATIVE_UI=0` and the requested WAV
  path sharded 396 neurons across two workers, stopped the layer-0 owner,
  relocated to the survivor, retained 566 neurons and continued growth to
  571. Launcher cleanup completed without leaving a launched process.
- [!] The local production evidence remains bounded to the RTX 2080 and its
  installed OpenCL/CUDA stack. Multi-device/driver replay, durable
  authoritative shard migration/recovery, security/deployment acceptance and
  scientific validation remain separate production gates.

## Verification update — 2026-09-14 15:04Z: parameterized local cluster launcher

- [x] Added `scripts/run_cluster.sh` as the reusable process launcher for a
  standalone brain, a single-worker orchestrator cluster (`--nodes 1`) and a
  multi-worker cluster (`--nodes N`). It supports optional web startup,
  config/network snapshots, release binary reuse, dynamic or explicit ports,
  per-process logs, bounded readiness checks and signal cleanup. The launcher
  keeps the same `aarnn_rust` and `web_ui` binaries used by the existing local
  examples, so this operational path does not create a second runtime.
- [x] Port allocation now rejects occupied or duplicate explicit orchestrator
  and web ports before starting children. Readiness checks verify the child is
  still alive before accepting a listener, closing the startup race where an
  unrelated listener could make a failed child appear ready.
- [x] Live bounded checks passed from the repository root using the existing
  release binaries: `--standalone --no-web` remained healthy for 8 seconds;
  `--nodes 1 --no-web` reached orchestrator and worker readiness; and
  `--nodes 3` reached all three worker listeners plus `/api/config`. Each
  timed run exited with the expected `timeout` status and cleanup removed its
  launched processes. An occupied explicit orchestrator port was rejected
  with status 2 before any child started.
- [x] `bash -n scripts/*.sh scripts/qa/*.sh`, `shellcheck scripts/run_cluster.sh
  run_examples.sh run_webcluster.sh`, `cargo test --locked
  --test run_examples_launcher`
  (6 passed), `cargo fmt --all --check` and `git diff --check` passed. The
  launcher contract test now covers standalone, single-worker and multi-worker
  modes, readiness ordering and explicit-port validation.
- [x] The existing `run_webcluster.sh` profile was aligned with the same
  automatic accelerator policy by adding `cuda` to its explicit
  `engine_runtime,ui,cuda` build. A bounded `AARNN_SKIP_BUILD=1` run reached
  the orchestrator, both workers and the dashboard, then cleaned up on TERM.
- [!] Automatically selected ports are intended for one launcher instance at a
  time. Concurrent independent launchers can still race between port discovery
  and child bind; a losing child is detected and reported rather than being
  falsely reported ready. Operators needing concurrent instances should use
  distinct explicit port ranges.

## Verification update — 2026-09-14 15:28Z: placement expands to late workers

- [x] Reproduced the reported `scripts/run_cluster.sh --nodes 3` behavior:
  `/api/status` showed three connected workers, while `cluster_master` retained
  the two-node assignment created before the third worker registered. The
  cause was `preserve_sharded_node_assignments`, which treated complete active
  and backup coverage as sufficient and never admitted a newly eligible target.
- [x] Updated `src/distributed.rs` so preservation remains stable during
  ordinary telemetry changes but returns to deterministic assignment building
  when the policy-approved target set grows. This keeps `desired_shards` and
  single-target policies authoritative while allowing a late worker to receive
  an actual shard command and appear in placement.
- [x] Added a regression test for a complete two-node assignment expanding
  when a third eligible worker joins. The distributed unit suite passed all 46
  tests.
- [x] Rebuilt the release `engine_runtime,ui,cuda` binaries and reran the
  three-worker launcher. The live API reported three connected workers and
  three `cluster_master` placement nodes; after another interval it still
  reported three placement nodes and continued neuron accounting (`211` total
  in the bounded run). Cleanup completed on interrupt.

## Verification update — 2026-09-14 17:52Z: Webots `--nodes` forwarding and worker registration

- [x] Fixed the original `scripts/run_celegans_web_ui_webots.sh --nodes 3`
  failure. The multi-robot wrapper now parses `--nodes`, validates it, exports
  `NM_CLUSTER_NODES`, and forwards the option to `run_webot.sh`; `run_webot.sh`
  now creates the requested total worker count instead of reporting
  `Unknown option: --nodes`.
- [x] In local cluster mode, each configured brain receives one IPC-owning
  worker for its Webots controller. Remaining workers receive unique IDs such
  as `celegans_01_worker_01`, bind distinct gRPC ports, join the orchestrator,
  and run without a duplicate IPC socket or UI. In remote mode, the same
  requested worker count is distributed round-robin across reachable hosts and
  the launcher rejects a count mismatch.
- [x] The Webots build profile is explicit and production-reproducible:
  `engine_runtime,ui,robot_io,cuda`. `robot_io` is required for the primary
  worker's IPC endpoint, while `cuda` keeps CUDA available for the measured
  OpenCL/CUDA latency selector. The previous `--all-features` build is not
  used by this launcher because it enables management behavior that requires
  credentials unrelated to the local example path.
- [x] Static launcher coverage now checks option forwarding, unique extra
  worker IDs, registration readiness, worker-count reporting, and the explicit
  feature profile. `bash -n`, the launcher contract tests, formatting, and
  diff checks passed. A bounded live run of
  `scripts/run_celegans_web_ui_webots.sh --ui-mode rust --nodes 3 --no-build
  --no-webots --no-diag --node-ui-hidden` registered three workers; the
  orchestrator reported `Nodes connected: 3` and all launched processes were
  cleaned up afterward.
- [!] The worker-count fix proves process registration and placement input for
  the Webots launcher. Biological shard ownership, durable migration, quorum
  fencing, and the remaining production gates retain the status recorded
  above; a worker process count alone does not claim those semantics.

## Verification update — 2026-09-14 18:08Z: stable native dashboard spacing

- [x] Stabilized the native egui dashboard layout in `src/ui.rs`. The controls
  rail now has a 288 px minimum width on the dashboard, volatile status rows
  use fixed 18 px heights with truncation and hover text, and the GPU status
  no longer changes the vertical position of the resource sections when its
  backend or activity text changes.
- [x] Quantized the network canvas layout dimensions to 4 px before deciding
  whether to recompute node spacing. Subpixel panel-size fluctuations therefore
  cannot trigger a full layout rebuild and visible network jump.
- [x] `cargo fmt --all --check`, `git diff --check`, all launcher shell syntax
  checks, and `cargo check --locked --no-default-features --features
  'engine_runtime,ui,robot_io,cuda'` passed. The check retains the repository's
  existing non-fatal warning set.

## Verification update — 2026-09-14 18:24Z: narrow-dashboard spacing capture

- [x] Rebuilt the exact screenshot-enabled release binary with
  `cargo build --release --locked --no-default-features --bin aarnn_rust
  --features 'engine_runtime,ui,robot_io,cuda,ui_screenshot'`; the build
  completed successfully with warnings only. Startup selected CUDA on the
  local RTX 2080 after measuring OpenCL at `0.829 ms` and CUDA at `0.355 ms`.
- [x] Captured `/tmp/aarnn-spacing-fixed.png` at `1100x700` using
  `NM_UI_CAPTURE_DELAY_FRAMES=30` and `NM_UI_CAPTURE_CLOSE=1`. Visual review
  confirms the output-path diagnostic and probe hint remain separated, the
  oscilloscope and output raster have a stable gap without overlap, and the
  right controls rail remains vertically stable while status text is truncated
  within its fixed row.
- [x] `cargo test --locked --test run_examples_launcher` passed all 7 launcher
  contract tests, including explicit audio forwarding and the sharding/growth/
  relocation probe. No generated `.multi_neuroworld.wbproj` file was recreated.

## Verification update — 2026-09-15 06:57Z: simulator node-count forwarding

- [x] Added `--node <n>` as an alias for the existing distributed Webots worker
  count, with `--nodes` and `--node=<n>` forms accepted by `run_sim.sh` and
  `scripts/run_multi_robot_webots.sh`. The unified launcher forwards the value
  to `run_webot.sh`, preserving one IPC-owning worker per configured brain and
  using additional workers for shard placement.
- [x] Unreal and Unity now reject an explicitly requested `--node`/`--nodes`
  count because their current TCP path starts one standalone `nn_tcp_server`
  per brain and has no distributed TCP bridge. This prevents a requested shard
  placement from being silently ignored.
- [x] Updated the simulator wrappers, `sim/README.md`, and launcher contract
  coverage. `bash -n` passed for all affected shell scripts, and the help/error
  probes confirmed Webots forwarding and clear TCP-backend rejection.
- [!] This launcher change does not promote the Unreal/Unity TCP compatibility
  path to distributed execution. A future distributed TCP bridge must connect
  the simulator endpoint to the authenticated node/brain data plane before
  those backends can accept this option.

## Verification update — 2026-09-15: distributed Unreal and Unity TCP parity

- [x] Added the bounded per-brain TCP bridge, initially as
  `scripts/tcp_aer_ipc_bridge.py`, and later replaced it with the Rust
  `tcp_aer_ipc_bridge` binary. It forwards the existing length-prefixed
  handshake, raw-float and AER1 client traffic to the distributed node IPC
  socket. AER output timestamps are derived from the input logical timestamp
  and negotiated frame duration; the bridge never uses packet arrival time.
- [x] `run_sim.sh --sim unreal|unity --node N` now starts the same distributed
  `run_webot.sh --runtime cluster` backend used by Webots, creates one TCP
  bridge per configured brain, waits for every IPC socket and TCP listener, and
  preserves the existing standalone TCP mode when no node count is requested.
  `--sim all --node N` uses a separate IPC namespace for the second cluster so
  Webots and Unreal do not contend for one active IPC peer.
- [x] Added the `NM_IPC_SOCKET_DIR` namespace hook to the distributed runtime,
  updated simulator documentation/wrappers and added launcher contract
  coverage. Shell syntax, Python compilation, mock TCP/UDS
  handshake/raw/AER exchange and a live two-worker Unity launcher readiness
  run passed.
- [!] The bridge exposes the existing Webots-compatible IPC compatibility path;
  authoritative biological ownership, durable migration, quorum fencing and
  the remaining production gates retain their separate status above.

## Verification update — 2026-09-15 09:31Z: C. elegans biological I/O accounting

- [x] Corrected the C. elegans import semantics. The 302 uppercase connectome
  functions, including `CANL` and `CANR`, remain the sole initial biological
  neuron population. The 24 sensory values are external input channels and the
  96 post-synaptic muscle targets are external motor readout channels driven by
  the `96 x 302` `w_out` matrix.
- [x] Added the backward-compatible `NetworkConfig.io_channels_are_biological`
  field. Legacy profiles retain their historical accounting; the C. elegans
  profile defaults and generated snapshot set it false. `Runner::total_neurons`,
  growth limits, output plasticity assignment, distributed layer counts and
  resource accounting now use the biological-only population for that profile.
- [x] Extended `connectome_labels` with inferred `sensory_neuron_nodes`,
  `motor_neuron_nodes`, `motor_output_channels` and explicit `io_semantics`
  metadata. The Webots validator checks the 302-node population, role subsets,
  matrix shapes and external-I/O flag.
- [x] Regenerated the tracked Webots config and ignored local connectome
  snapshot from `scripts/build_celegans_network_json.py`; the import assertions
  passed. Default-feature Rust focused tests passed for the runner count and
  distributed placement projection. The standalone `growth3d` feature check
  remains blocked by pre-existing `gpu_candidate_masks` type-inference errors
  in `src/runner.rs:19663-19698`, unrelated to this change.


## Simulator content review — 2026-09-15 11:10Z

The requested Webots/Unity/Unreal/WebGL evaluation and shared sensory-world/model
upgrade is tracked in [the simulator content ExecPlan](simulator-content-parity.md)
and [the content evaluation](../../sim/content/README.md). One catalogue/compiler
now supplies five habitats and six profiles, with morphology landmarks, browser
spatial/retinal sensing, canonical worm/hexapod map corrections and visual-only
regeneration. The SIM-CONTENT-001 contract, browser and Webots construction lanes
pass; Unreal builds and an agar view has been rendered. Unity Editor evidence,
remaining native fly/fish mappings and calibrated visual/physics parity remain open.
Existing unrelated dirty work is preserved. No production migration flag, neural
kernel, persistence schema or phase safety gate was changed or promoted.

## Minecraft Java/Bedrock and water verification — 2026-09-15 13:37Z

The follow-on [NAO interaction and autonomous communication plan](nao-player-interaction.md)
tracks the opt-in reference social model, same-world participant cues, chat adapters
and refreshed Minecraft validation. It does not enable the production Phase 8 gate.

The final simulator evidence now includes nine engineered Rust speech acts,
browser interaction, native Webots/Unreal body exchange and an actual native
Bedrock villager-triggered inquiry/name-tag bubble. The Minecraft adapters wait
for validated body output before starting encounters. Native world creation uses
complete BDS metadata; vanilla NPC identity is checked before/after reload.
Final six-network BDS acceptance is `bedrock-native-82l2875w/`, with refreshed
Java acceptance in `world-tlmb7e_5/`. Unity and Bedrock client rendering remain
not-run, and scientific/physics calibration remains separate from content parity.

The [Minecraft delivery plan](minecraft-simulator-parity.md) and
[validation index](../../sim/minecraft/VALIDATION.md) record the native adapters,
companion JAR, Java/Bedrock saved worlds and install instructions. The shared
catalogue now contains 602 objects with explicit water above the fish in every
export. Java's 13 JVM tests, native GameTest/clean world export and 12 rendered
views pass. All six real Rust snapshots pass companion round trips; BDS 1.26.45.1
also passes four native frames per profile, water, disarming, save/reload of the
same 12 entities and clean shutdown. Native TCP and both RakNet UDP port collisions
are resolved automatically without changing original server.properties. Detection
finds versioned Developer installations and rejects a second writer on an open
world. The user's running BDS, Minecraft profiles, mods and saves are preserved.

Final native evidence: `target/qa/minecraft/bedrock-native-j68cl4mh/`; Bedrock API,
socket/detection fixtures and type checks: `target/qa/minecraft/bedrock-77tl8lpm/`.
Distribution is `dist/minecraft/` with SHA256SUMS. Unity Editor and Bedrock client
rendering remain unavailable, and native mapping/calibration/physics gates remain
open. The opt-in legacy AER1 sandbox preserves the closed production I/O gate;
no native engine content hash is presented as biological or physics equivalence.

## Verification update — 2026-09-17: simulator launcher readiness and Java token bootstrap

- [x] Reproduced the reported `scripts/run_sim.sh --sim unreal --robots
  "celegans=2" --nodes 3 --no-engine` path. The retained runtime log
  `logs/sim_cluster_15275/runtime.log` records three registered nodes
  (`celegans_0_ipc`, `celegans_1_ipc` and `celegans_0_worker_01`); the terminal
  previously exposed only the two TCP bridge rows and did not wait for the
  worker-registration line. `run_sim.sh` now waits for that line and prints the
  cluster log before starting bridges.
- [x] Confirmed the Unreal command's `--no-engine` flag was the direct reason
  no Unreal process appeared. The launcher now states this explicitly and fails
  if an engine or project path is requested but missing, instead of silently
  falling back to server-only mode.
- [x] Confirmed Java Minecraft preflight stopped before brain startup solely
  because `AARNN_MINECRAFT_TOKEN` was absent. Java runs now create an ephemeral
  private token when none is supplied; it is inherited by the companion and the
  launcher-started Java client. Bedrock continues to require its documented
  scoped `secrets.json` because the dedicated server does not consume the Java
  process environment token.

## Verification update — 2026-09-15 18:53Z: all-feature active-dendrite fixture

- [x] Investigated GitHub Actions run `35006958890`, job `104512616535`, with
  `gh` authentication. The x64 job failed only at
  `runner::tests::test_active_dendritic_compartments_boost_excitation`; the
  ARM64 verification job passed.
- [x] Reproduced the failure with the CI feature profile. `Runner::new` rebuilds
  the default human topology and resolves randomized per-cell profiles, so the
  test's edits to the shared `aarnn_bio` profile were not authoritative. The
  test fixture now clears the clumping design, regions and neuron types before
  asserting the shared-profile active-dendrite path. This changes no production
  biological or numerical semantics.
- [x] The focused CI-equivalent command passes:
  `cargo test --locked --all-features --lib
  runner::tests::test_active_dendritic_compartments_boost_excitation --
  --exact --nocapture`. The complete x64 workflow test command also passes:
  `NM_DISABLE_OPENCL=1 cargo test --locked --all-features --lib --bin web_ui
  --test '*'` (402 library tests plus all integration suites). `cargo fmt
  --all --check` and `git diff --check` also pass.

## Verification update — 2026-09-17: Unreal C. elegans causal locomotion

- [x] Reproduced the source of movement with zero or sparse motor output. The
  Unreal C. elegans adapter contained an explicit anti-flatline traveling-wave
  fallback that added a sinusoidal drive after eight low-drive applications.
  This path was independent of neural output and violated the requirement that
  locomotion originate from committed motor output.
- [x] Removed the fallback and its twitch state. The adapter now derives all
  dorsal/ventral and left/right joint targets from the decoded 96-channel
  actuator frame. A motor response is consumed once; when no newer response is
  available, the base bridge submits a neutral frame so muscle traces decay
  rather than replaying a stale spike.
- [x] Rechecked the shared C. elegans I/O catalogue: 24 ordered sensors match
  the Unreal collector (inertial, touch, light, heat, taste/chemical, flow and
  proximity); 96 ordered outputs match the handshake. `MDL01..24`,
  `MDR01..24`, `MVL01..23`, `MVR01..24` are body-wall mappings, while the
  catalogue's final `MVULVA` channel is intentionally not included in the
  body-wall muscle map. Sensory values above the AER threshold are the only
  external events sent to the brain; they can cause movement only through the
  resulting neural output.
- [x] The Unreal module build succeeded with `Build.sh NeuralMimicrySimEditor
  Linux Development`. A bounded empty-AER run spawned two worms, completed both
  24-sensor/96-output handshakes, and returned repeated
  `NmCelegansDrive: abs_mean=0.0000 abs_max=0.0000 max_target_deg=0.00` with
  `NmCelegansMotion` head/tail speeds at `0.00`. This verifies that empty motor
  frames do not inject locomotion. The run was then terminated cleanly and left
  no Unreal or bridge process running.

## Verification update — 2026-09-17: distributed simulator startup barrier

- [x] Traced the initial-spike loss to distributed startup: the orchestrator
  and preloaded workers were created with `playing=true`, and
  `DistributedNode::run_simulation` therefore advanced `step(None)` while
  Unreal was still loading. The TCP bridge handshake did not previously pause
  or arm that runtime.
- [x] Added a simulator-only readiness barrier. `run_sim.sh` forces the
  distributed runtime to load paused, each TCP bridge publishes an atomic
  readiness marker only after the Unreal handshake has completed its IPC
  round-trip, and the launcher sends the existing cluster `start` control only
  after every requested environment bridge is ready.
- [x] Worker preloads and `LoadNetwork` command-created networks now honour the
  same `NM_DISTRIBUTED_AUTOSTART` gate. The bridge holds the first sensory frame
  until every distributed worker has logged the applied `Start`, then publishes
  the arm marker. This preserves the first sensory sample through bounded TCP
  backpressure and prevents a paused IPC owner from deadlocking the response.
- [x] `bash -n`, Python compilation, `cargo fmt --all --check`, `git diff
  --check`, `cargo check --locked --no-default-features --features
  engine_runtime,ui,robot_io --bin aarnn_rust`, and all 10
  `run_examples_launcher` tests passed. A fake TCP/Unix-datagram integration
  probe confirmed that the handshake marker is published, the sensory frame is
  withheld before arm, and the exact frame is forwarded after arm.
- [x] A live Unreal run was completed with the rebuilt release binary using
  `TCP_READY_TIMEOUT=30 ./scripts/run_sim.sh --sim unreal --robots
  'celegans=2' --nodes 3 --no-build`. In
  `logs/sim_cluster_64784/runtime.log`, the cluster reports
  `distributed autostart: 0`; the terminal then records both environment
  handshakes, both `Sent start` commands, `Start reached all 3 distributed
  worker(s)` and `distributed neural processing armed` in that order. The
  worker logs show each `[distributed] simulator network ... armed
  (playing=true)` line before the first `Runner::step` profile, confirming the
  initial neural activity is held until Unreal is ready.

## Verification update — 2026-09-17: simulator worker visibility

- [x] Reproduced the reported visual mismatch with
  `./scripts/run_sim.sh --sim unreal --robots 'celegans=2' --nodes 3
  --no-engine --no-build`. Three worker processes registered with distinct IDs:
  `celegans_0_ipc`, `celegans_1_ipc` and `celegans_0_worker_01`. The previous
  simulator command suppressed the orchestrator dashboard and showed only the
  two configured brain IPC surfaces, making the third headless worker easy to
  miss.
- [x] Simulator distributed launches now keep the single orchestrator dashboard
  visible while hiding per-brain IPC UIs. Startup prints the complete registered
  node ID list, and the dashboard reports all connected workers. WebGL retains
  its browser-only dashboard path.
- [x] Closed the placement visibility gap for the one-layer C. elegans profile.
  The compatibility projection keeps one active owner and adds warm copies on
  otherwise unused policy-selected workers, so `desired_shards=3` reports all
  three selected placement hosts without fabricating multiple active writers.
  Finer neuron/sub-shard active execution remains a later executor boundary.
- [x] Validation passed: `bash -n scripts/run_sim.sh run_webot.sh`,
  `cargo fmt --all --check`, `git diff --check`, the focused launcher contract
  test, and the bounded live startup above. The live orchestrator log reports
  `Nodes connected: 3` and all three node IDs.

## Verification update — 2026-09-17 21:40Z: three-node placement confirmation and GAIL AER cross-check

- [x] Corrected the distributed launcher placement barrier. Placement lines
  are emitted by the authoritative `webots_orchestrator.log`, while
  `runtime.log` contains the wrapper output; `scripts/run_sim.sh` now waits on
  and prints the orchestrator placement evidence. A live
  `TCP_READY_TIMEOUT=30 ./scripts/run_sim.sh --sim unreal --robots
  'celegans=2' --nodes 3 --no-engine --no-build` run registered exactly three
  workers: `celegans_0_ipc`, `celegans_1_ipc`, and
  `celegans_0_worker_01`. It then confirmed both `celegans_0` and
  `celegans_1` distributed across three nodes before starting both Rust TCP
  bridges.
- [x] Kept the one-writer invariant for the one-layer C. elegans profile.
  The third placement is a warm compatibility copy; it is visible in node
  placement and resource telemetry but is excluded from authoritative global
  snapshot assembly. `src/distributed.rs` now filters empty active-layer
  assignments from cluster snapshot participants, and the cluster snapshot
  regression test covers a warm worker with a backup layer. This removes the
  prior empty-assignment and duplicate-layer failures without creating a
  second active writer.
- [x] Corrected the orchestrator UI projection to request the global snapshot
  without selecting a nonexistent shard named `orchestrator`. The final live
  run showed the three nodes and both placements; the only snapshot connection
  warnings occurred during intentional Ctrl-C shutdown after the orchestrator
  loop stopped. No persistent participant, empty-assignment, duplicate-layer,
  or missing-orchestrator-shard errors remained during the live interval.
- [x] Cross-checked GAIL at `/home/pbisaacs/Developer/neuralmimicry/gail`.
  Both implementations use the same `AER1` format: little-endian `u64`
  timestamp followed by varint timestamp delta, address, and value triplets;
  defaults remain sensory base `4096` and output base `16384`. GAIL's
  authenticated `POST /api/llm/mirror` request fields match AARNN's current
  DTO, and a live local web UI loopback accepted a GAIL-shaped request and
  returned a valid AER1 response. `cargo +stable test --locked` in GAIL
  passed 481 library tests plus the QLoRA shard test. The repository default
  Rust 1.92 toolchain cannot build GAIL's locked `sysinfo 0.39.6` dependency,
  which requires Rust 1.95; this is a toolchain constraint, not an AER
  compatibility failure.
- [x] AARNN validation for this slice passed: release build with
  `engine_runtime,ui,robot_io,cuda`; focused distributed snapshot, Rust bridge,
  and spike transport tests; `run_examples_launcher` (10/10); shell syntax;
  and `git diff --check` on the touched launcher/distributed/UI files.

## Verification update — 2026-09-18 07:21Z: default main promotion and tagging

- [x] Updated `.github/workflows/build-and-release.yml` so a successful push to
  `main` promotes the completed `container-manifest` node image to the
  existing `developer-blue` target, then runs `Version Bump and Tag` for the
  next patch release. Manual dispatch retains its explicit environment and
  version-bump controls.
- [x] Added a guard for the generated `chore: bump version ...` commit so its
  push-triggered workflow does not recursively create another version bump.
  YAML parsing, focused workflow dependency assertions and `git diff --check`
  passed. `yamllint` reports only the workflow's existing line-length warnings.

## Verification update — 2026-09-18: profile-aware CNS population growth

- [x] The canonical growth path is `src/runner.rs`: hidden neurons mature in
  `Topology3D`, while sensory/output formation currently advances toward fixed
  `target_num_sensory` and `target_num_output` values. `src/config.rs` already
  resolves organism profiles and sets the human cap to 86,000,000,000, so the
  ratio controller belongs in the persisted network configuration and the
  runner's topology admission boundary.
- [x] The requested human CNS model treats the mature hidden population as its
  interneuron budget: `S=floor(I/8,600)` and `M=floor(I/172,000)`. This yields
  zero new peripheral neurons below 8,600 and 172,000 mature hidden neurons,
  respectively. Existing explicit I/O remains append-only for compatibility;
  ratio-managed additions are bounded by the biological neuron cap.
- [x] Added persisted `GrowthIoRatioPolicy` configuration, human-profile
  defaults, integer target calculation, final-volume composition calculation,
  and profile isolation. At 86,000,000,000 total neurons the deterministic
  composition is 85,989,501,282 interneurons, 9,998,779 sensory neurons, and
  499,939 motor neurons.
- [x] Updated AARNN growth admission to refresh sensory/motor targets from the
  mature hidden population, exclude provisional early cells, preserve
  append-only I/O formation, and respect `max_total_neurons`.
- [x] Validation passed: focused configuration and runner tests, existing
  hidden-topology and early-cell growth tests, `cargo test --locked --features
  engine_runtime --lib` (382 passed), default `cargo check --locked --lib`,
  the full feature check, `cargo fmt --all -- --check`, and `git diff --check`.
  The standalone `growth3d` check still exposes unrelated pre-existing
  unconditional Rayon/OpenCL references when those features are disabled.

## Verification update — 2026-09-18: mapped species ratios and unknown-network average

- [x] Reviewed the canonical profile generators and checked-in Webots mapping
  contracts. The source populations are C. elegans `302/24/96` for biological
  connectome/sensory/muscle-readout counts, Drosophila `20,000/418/48`, and
  zebrafish `2,000/32/32`. C. elegans sensory and motor values remain external
  adapter/readout channels, so its policy is recorded but the biological-I/O
  admission gate remains closed for that profile.
- [x] Added exact rational profile policies: C. elegans `24/302` and `96/302`,
  Drosophila `418/20,000` and `48/20,000`, and zebrafish `32/2,000` for
  sensory and motor per hidden/interneuron population. Hexapod inherits the
  Drosophila policy; NAO resolves to the human profile and inherits the human
  CNS policy.
- [x] Added an unknown-network policy equal to the arithmetic average of the
  four independent source types (human, C. elegans, Drosophila and zebrafish),
  excluding derived Hexapod and NAO aliases. Its effective rounded thresholds
  are approximately 35 hidden neurons per sensory neuron and 12 per motor
  neuron, while target calculations retain the exact rational average.
- [x] Profile snapshots retain their mapped I/O populations as an append-only
  baseline. Unprofiled biological networks continue to begin with ratio-managed
  I/O formation, preventing early peripheral populations from overwhelming the
  developing hidden network.
- [x] Validation passed: `cargo test --locked --features engine_runtime --lib`
  (384 passed), `cargo check --locked --all-features`, default
  `cargo check --locked --lib`, `cargo fmt --all -- --check`, and
  `git diff --check`, including mapped-profile, unknown-average, human-ratio,
  mapped-I/O preservation and growth-admission tests.

## Verification update — 2026-09-18: morphology ratio observability

- [x] Added the shared `GrowthIoRatioView` projection in `src/config.rs`.
  Native UI, CLI summary logs, and growth target logs now use the same exact
  policy fractions, mature-interneuron target calculation, current sensory /
  motor populations, and profile label.
- [x] Added the ratio report to the native `Morphological Evolution
  (Physical)` panel in `src/ui.rs`, including the external adapter/readout
  explanation required by the C. elegans mapped profile.
- [x] Added the corresponding live report to the web UI's `Topology /
  Morphology` panel. It reads serialized `growth_io_ratio_policy` fields and
  live topology snapshot counts, preserving the same floor target semantics.
- [x] Added `[summary] Morphological I/O ratio ...` CLI lines and a
  change-triggered `[growth] I/O ratio ...` log for ongoing topology growth.
- [x] Validation passed: the three focused `growth_ratio_view` tests, full
  feature compilation with `cargo check --locked --all-features`,
  `node --check web_ui/app.js`, and `git diff --check`. The repository's
  existing compiler warnings remain non-fatal.

## Verification update — 2026-09-18 16:43Z: UI parity and non-blocking review

- [x] Cross-checked the native Rust, web, Android and checked-in iOS input
  surfaces. Native standalone/provider views consume real provider FFT bands;
  the web Graphic EQ is explicitly labelled as sensory-activity-derived;
  Android and iOS currently expose preview-only media adapters and now state
  that governed spectral bands are unavailable. Native cluster projection
  views no longer imply that local/provider FFT data belongs to the remote
  cluster; their EQ is labelled unavailable.
- [x] Web activity polling now coalesces overlapping requests and rejects a
  response whose source or request sequence is stale before updating the
  graph, placement, probes or derived EQ.
- [x] Native remote workspace Pull, Push, Start and Stop actions now queue
  through `ToolTaskResult`. Snapshot serialisation, blocking HTTP and control
  requests execute on worker threads; the egui thread only submits work and
  applies the completed snapshot/result. A single in-flight guard prevents
  duplicate remote operations.
- [x] Android remote refresh remains on its executor and now uses an atomic
  in-flight guard, preventing timer/manual refresh overlap. The client is
  volatile for cross-thread visibility.
- [x] Added compatibility tests for activity request coalescing/stale-source
  handling, native background workspace actions and Android refresh
  coalescing. `cargo test --locked --test web_ui_browser_compat` passed all 12
  tests. `cargo check --locked --no-default-features --features
  engine_runtime,ui`, `cargo fmt --all -- --check`, `node --check web_ui/app.js`
  and `git diff --check` passed. Android
  `:app:compileDebugKotlin` passed with the Android Studio JBR (Java 17); the
  default JVM 8 environment was rejected by Gradle before the retry.
- [!] Full Android/iOS UI parity remains an open delivery gate: Android has no
  governed spectral-band endpoint or morphology-ratio panel, and the
  repository still has no checked-in full iOS Xcode application. The review
  records these capability gaps rather than fabricating data or a partial
  production shell.
- [!] The `save_on_exit` native workspace option still performs one synchronous
  final push during `Drop`. It is deliberately retained as a shutdown durability
  boundary: detaching that request would allow the process to exit and silently
  lose the requested final snapshot. Interactive Pull/Push/Start/Stop paths
  remain fully backgrounded.

## Verification update — 2026-09-18 17:09Z: morphology stepping stall

- [x] Investigated `logs/nm-1789750465.log`. The first severe pause was an
  `App::update/render` frame of 8.42 s at `1789750493.867`; after that, the
  simulation reported synchronous `morphology/evolve` maxima of 450 ms,
  4.86 s, 5.39 s and later 7.39 s. The matching
  `Runner::step/morpho` timings identify morphology evolution as the repeated
  stepping blocker rather than audio or transport. The UI frame metric also
  shows the first pause occurred in presentation work, while the ordinary
  render frames resumed below a few milliseconds.
- [x] Enabled the existing morphology worker by default for `ui` profiles in
  `src/runner.rs`. `NM_MORPHO_ASYNC=0|1` remains the highest-priority explicit
  override; `NM_REALTIME_IPC` remains the compatibility fallback, and
  headless profiles retain their synchronous default unless configured.
  Completed morphology results are still applied on the runner thread, so
  topology and synapse ownership remain deterministic and single-writer.
- [x] Closed the async worker lifecycle edge: topology changes, reset and
  temporary morphology disablement invalidate a cloned result but retain its
  receiver until the worker exits. A stale result is discarded by sequence,
  preventing orphaned workers and unbounded overlapping morphology jobs.
- [x] Bounded native morphology overlay presentation in `src/ui.rs` to a
  topology-size-dependent draw cap and a 20,000-synapse scan budget. The UI
  reports when the projection is capped, so large growth cannot make an
  unbounded traversal monopolise an egui frame. The overlay now has its own
  `App::update/render/morphology_overlay` metric for the next reproduction.
- [x] Validation passed: `cargo check --locked --no-default-features
  --features desktop_ui_workload`, the focused
  `morphology_async_profile_has_explicit_override_precedence` test under the
  same desktop profile, `cargo fmt --all -- --check`, and `git diff --check`.
  The standalone `growth3d,morpho,ui` test command without the repository's
  OpenCL/parallel desktop profile remains invalid because existing code has
  unconditional Rayon/OpenCL references; that is recorded as a feature-profile
  limitation, not a failure of this change.
- [!] The exact internal sub-operation responsible for the single 8.42 s UI
  frame is not independently timed by the current metrics. The new bounded
  overlay path mitigates the identified unbounded presentation work; a future
  profiling pass should split `App::update/render` into layout, topology and
  painter scopes if the frame remains reproducible.

## Verification update — 2026-09-18: adaptive rendering and morphology loop review

- [x] Reviewed `logs/nm-1789752860.log`. The dominant outliers are still
  synchronous `morphology/evolve` calls, including a 41.00 s maximum, while
  the ordinary `Runner::step` and `App::update/render` samples are generally
  in the millisecond range. The morphology overlay itself is negligible in
  the same report, so it is not the source of the long stepping stall.
- [x] Split morphology timing into grid/energy, pruning, growth/movement,
  contact detection, and connectivity-repair metrics. These counters are
  emitted by subsequent runs; the supplied log predates this instrumentation
  and therefore cannot be used to claim a phase-level attribution for its
  41-second samples.
- [x] Replaced repeated full `self.synapses` scans during final growth
  insertion deduplication with an endpoint-pair map already maintained by the
  contact path. The stable endpoint semantics and single-writer morphology
  ownership remain unchanged, while proposed insertions no longer perform an
  O(new proposals × existing synapses) search.
- [x] Replaced the static-overlay renderer's fixed 20,000-edge limit with an
  adaptive budget derived from cached edge count, overlay density, topology
  size, and the explicit force-show setting. Small requested views remain
  complete; larger views receive proportionally more detail without allowing
  topology growth to monopolise an egui frame.
- [x] Validation passed: `cargo check --locked --no-default-features
  --features desktop_ui_workload`, the seven morphology unit tests, the
  focused morphology-growth and development-stage runner tests,
  `cargo fmt --all -- --check`, and `git diff --check`. Existing compiler
  warnings remain non-fatal.

## Verification update — 2026-09-18: latest morphology log comparison

- [x] Compared `logs/nm-1789755209.log` with the preceding runtime report.
  The new phase metrics attribute the long pauses conclusively to
  `morphology/evolve/contact_detection`: its maximum was 43.77 s and the
  enclosing `morphology/evolve` maximum was 43.82 s. The next largest
  morphology phases were `growth_movement` at 42.85 ms, `grid_energy` at
  2.52 ms, `connectivity_repair` at 1.70 ms and `pruning` at 0.65 ms.
  Native render frames remained in the ordinary 2–6 ms range and the
  morphology overlay remained below 0.1 ms, so presentation work is not the
  source of this stall.
- [x] The contact path now stops spatial-index candidate materialisation at
  the configured per-tip limit, filters repeated grid-cell references while
  collecting, reuses the duplicate-filter buckets across tips, and keys
  deduplication by the stable segment index. This preserves candidate order,
  contact limits and deterministic single-writer topology ownership while
  removing unbounded intermediate candidate growth and per-tip map setup.
- [x] Added separate `morphology/evolve/contact_detection` timings for index
  collection, the small-network pre-probe, indexed candidate evaluation and
  fallback probing. The next runtime report can therefore distinguish
  spatial-index traversal from compatibility, distance, migration and probe
  work. The supplied log predates this final collector reuse and therefore
  remains the baseline; a subsequent runtime log is required before claiming
  the 43.77 s stall is resolved.
- [x] Validation passed: `cargo check --locked --no-default-features
  --features desktop_ui_workload`, focused morphology tests, formatting and
  whitespace checks. Existing compiler warnings remain non-fatal.

## Verification update — 2026-09-18: contact setup remains the outlier

- [x] Reviewed `logs/nm-1789756544.log` after the bounded collector change.
  Typical contact-detection maxima fell to roughly 0.34 s, and one later
  report reached 8.59 s, but the run still contains a 40.01 s contact maximum.
  The aggregate is therefore improved for ordinary growth but the worst-case
  stall remains.
- [x] The new submetrics show that the 40.01 s sample spent only 483.90 ms
  in index collection, 1.02 ms in indexed candidate evaluation, 0.62 ms in
  pre-probing and 0.12 ms in fallback probing. The missing time occurs before
  those loops, during the contact phase's axon sprouting, segment-reference
  preparation or spatial-index construction. Candidate evaluation is no
  longer the dominant explanation for that outlier.
- [x] Added independent timings for
  `morphology/evolve/contact_detection/axon_sprouting` and
  `morphology/evolve/contact_detection/index_build`. The next started run can
  identify which setup operation consumes the remaining time before another
  algorithmic change is made.

- [x] The new timings identify spatial-index construction as the blocker:
  `index_build` reached 41.25 s while axon sprouting was 11.02 ms and indexed
  candidate evaluation was below 1 ms. `AxonSegIndex::build` now estimates
  uniform-grid expansion before insertion and selects the octree when the
  grid would exceed the configured density or reference budget. Sparse,
  low-detail networks continue to use the uniform grid, and retained grid
  builds reserve their estimated reference capacity to reduce rehashing.
- [x] Validation passed after the index-selection change: desktop-profile
  compilation, seven focused morphology tests, formatting and whitespace
  checks. A new started runtime log is still required to measure the actual
  index-build reduction.

## Verification update — 2026-09-18: latest morphology log comparison

- [x] Reviewed `logs/nm-1789757760.log` across 87 metrics reports. The
  previous multi-second contact-detection/index-build failure is no longer
  present: `morphology/evolve` peaked at 804.78 ms and contact detection at
  795.44 ms. The largest remaining contact subphase was axon sprouting at
  393.88 ms; this is proportional to the growing axon population and its
  detail-controlled energy search, and remains off the UI thread when the
  desktop morphology worker is enabled.
- [x] Spatial-index construction shows isolated early peaks of 573.67 ms,
  474.46 ms and 377.88 ms while the later reports settle near 1–2 ms, with
  the final observed maximum at 2.32 ms. This confirms the grid-density /
  octree selection removed the former 40–43 second pathological build.
- [x] The normal runtime after startup remained responsive: `Runner::step`
  peaked at 97.89 ms and ordinary `App::update/render` reports stayed below
  16 ms after the isolated 6.98 s render sample. That sample contained no
  corresponding morphology, topology-overlay or snapshot-copy cost, so it
  is retained as an un-attributed presentation outlier rather than used to
  justify a fixed reduction in adjustable biological detail.
- [x] No further morphology algorithm change was made from this log. Axon
  sprouting remains the next profiling target; any future reduction must use
  the configured detail/budget policy and preserve deterministic proposal
  ordering, rather than imposing a constant sample count.

## Verification update — 2026-09-18: complete-feature launcher profiles

- [x] Audited Cargo build/test commands in the repository-root launchers and
  `scripts/`. `run_examples.sh`, `run_webcluster.sh`, `run_webot.sh`,
  `scripts/run_cluster.sh`, `scripts/run_sim.sh` and
  `scripts/run_multi_robot_webots.sh` now build with explicit
  `--all-features`; the release packager and container workload metadata use
  the same profile. The Webots stub helper and example Makefile were aligned
  as well because they produce project binaries.
- [x] Local/container launch paths now default `NM_MORPHO_ASYNC=1`, including
  Podman workload wrappers. Existing environment overrides remain available
  for diagnostics and compatibility, while the default morphology worker,
  Rayon `parallel` feature and bounded asynchronous I/O paths are present in
  the standard launcher profile.
- [x] Validation passed: `bash -n` for all root and `scripts/*.sh` launchers,
  the Webots stub helper, launcher help paths, the examples Makefile dry run,
  `git diff --check`, and
  `cargo check --locked --all-features --all-targets` (warnings only).
- [x] The complete release binary graph also built successfully with
  `cargo build --locked --all-features --release --bins`; the existing
  compiler warning set remains non-fatal.

## Verification update — 2026-09-18: all-feature example startup contract

- [x] Reproduced the reported `run_examples.sh` failure. Compilation completed
  successfully; the orchestrator then exited during startup because enabling
  `management_v1` through `--all-features` activates fail-closed static
  management authentication and requires `NM_MANAGEMENT_BEARER_TOKEN`.
  Supplying that token exposed the next required local contract:
  `NM_GRPC_TLS_CERT`, `NM_GRPC_TLS_KEY`, `NM_GRPC_TLS_CA` and a persisted
  `NM_MANAGEMENT_STATE_PATH`.
- [x] Kept the complete feature profile and made the loopback launcher provide
  a per-run bearer credential, principal/policy, management state path, and an
  ephemeral local CA-signed mTLS certificate. Existing deployment credentials
  remain authoritative; incomplete externally supplied TLS configuration is
  rejected rather than silently downgraded.
- [x] Fixed the all-feature rustls startup ambiguity by selecting the ring
  provider explicitly in the native and web binaries. The complete graph also
  links reqwest's AWS-LC provider, so automatic provider selection otherwise
  panicked at the first TLS handshake.
- [x] Updated the web UI's distributed gRPC client to use the shared mTLS
  endpoint builder. A live `run_examples.sh` smoke reached the dashboard,
  joined both nodes to the orchestrator, and returned cluster data from
  `/api/status`; no startup authentication, TLS-provider, or client-certificate
  failure remained. The expected CUDA PTX compatibility fallback warnings are
  unrelated and non-fatal.

## Verification update — 2026-09-18: scripts all-feature launcher cross-check

- [x] Cross-checked every executable under `scripts/` that builds or launches
  an all-feature runtime. `scripts/run_cluster.sh`, `scripts/run_sim.sh`,
  `scripts/run_multi_robot_webots.sh`, and the Celegans, Drosophila and NAO
  Python combo launchers had the same missing `management_v1` startup
  environment as the original example launcher.
- [x] Added `scripts/local_management_env.py` as the shared local-launcher
  setup. It preserves operator supplied credentials, rejects partial TLS
  configuration, and otherwise creates a loopback-only static bearer identity,
  management state path, and short-lived local CA-signed mTLS certificate.
  The root `run_webcluster.sh` and `run_webot.sh` paths use the same helper so
  script wrappers do not reintroduce the failure through delegation.
- [x] Classified `scripts/package-release.sh`, container build/package scripts,
  and `scripts/run_aarnn_with_compatible_libclang.sh` as build or standalone
  UI paths rather than management-service launchers. `scripts/deploy_mixed_cluster.sh`
  remains a production deployment path and must receive its management bearer,
  TLS material, and durable state through Kubernetes deployment configuration;
  it must not generate developer credentials on the operator host.
- [ ] Production Kubernetes secret/PVC wiring for that deployment path remains
  an external deployment gate and is not claimed closed by the local launcher
  helper.

## Verification update — 2026-09-18 20:59Z: Webots cluster parallelism and blocking paths

- [x] Reproduced a fresh-state C. elegans Webots cluster run with
  `WEBOTS_WORKSPACE_RESUME_EXISTING=0`, an isolated runtime root, headless
  fast Webots mode, and two requested workers. The IPC owner executed the
  biological `Runner::step` work; `webots_celegans_01_worker_01.log` recorded
  only `distributed/node_step` samples and no `Runner::step` samples. The
  configured C. elegans profile has one hidden layer, and the compatibility
  placement deliberately does not split a biological layer between writers.
  `--nodes 2` therefore adds a registered standby/placement target rather
  than a second active biological compute stream for this workload.
- [x] Confirmed that `--all-features` selects the runtime branch guarded by
  `replicated_durability` and `superdense_executor`. Even without an
  explicitly configured durable root, each compatibility step is routed
  through `SuperdenseController::step`, which allocates/adopts causal events
  and settles them before returning to the IPC loop. This is runtime behavior,
  not merely a compile-size change. The all-feature release binary measured
  about 70 MB in this checkout; the explicit
  `engine_runtime,ui,robot_io,cuda` build measured about 56 MB.
- [x] Identified the main non-blocking opportunity in
  `DistributedNode::run_simulation`: the loop holds the network write lock
  across the biological step, snapshot generation, and output preparation;
  workspace autosave then performs full JSON publication, `sync_all`, rename,
  and manifest publication before releasing that lock. This should be changed
  only by capturing an immutable committed snapshot at the authoritative
  boundary and handing it to a bounded writer queue. The queue must publish
  monotonically and retain the latest-good checkpoint so `INV-012` and
  recovery semantics are preserved.
- [x] Identified two additional scheduling limits. Networks on one node are
  stepped serially in the single `run_simulation` loop, so independent brains
  wait behind one another; and spike forwarding is awaited before the loop
  advances to the next network. Independent per-network workers plus bounded
  output queues are candidates for `INV-010` compliance, but causal commit
  and fencing work must remain ordered per network.
- [x] The CLI Webots path now passes `--no-orchestrator-ui --node-ui-hidden`,
  removing the unused orchestrator dashboard/render loop while retaining the
  IPC-owning worker runtime. Shell syntax, launcher contract tests, and diff
  checks passed after this change.
- [!] An explicit-profile live comparison was attempted after preserving the
  all-feature binaries. The explicit binary built successfully, but the
  bounded launcher probe did not complete worker registration and repeatedly
  reported gRPC transport errors, so no numeric speedup claim is made from
  that probe. The next benchmark must first resolve that registration issue,
  then compare identical fresh-state runs with the same autosave and IPC
  settings.

## Verification update — 2026-09-20: Actions workflow duplication and scheduling

- [x] Inspected the canonical workflow at `.github/workflows/build-and-release.yml`
  and the supporting workflows under `.github/workflows/`. The unified workflow
  owns verification, Linux package publication, multi-architecture container
  images/manifests, rolling/latest promotion, version bumping and wiki sync;
  `minecraft.yml` is a path-scoped simulator contract lane and
  `publish-ui-arm64.yml` is an explicit HWE publication lane.
- [x] Reviewed the three most recent unified runs with `gh run list` and
  `gh run view`: runs `35493916852` (`v0.1.28`, in progress), `35493916612`
  (`main`, in progress), and `35454272269` (the preceding successful `main`
  run). Runs `35493916852` and `35493916612` were created at the same second
  for SHA `1e8d3b9a4db1c4bfbbf4b471b553bc7cda0fa610`; one was triggered by the
  generated tag and one by the branch update. Both repeated verification and
  Linux packaging, and both entered the container build graph. The tag run is
  therefore duplicate work for the same immutable source.
- [x] Confirmed that `package-linux` has four matrix entries but
  `max-parallel: 1`, and `container-build` has nine entries but
  `max-parallel: 1`. Their architecture concurrency groups protect the shared
  ARM runner, but the matrix-wide limit also serialises independent hosted
  amd64 work. The prior successful run spent roughly 16 hours in the package
  and container queues, including repeated arm64 variants.
- [x] Optimised the workflow so the branch run for an automatically generated
  version commit discovers the matching `v*` tag, publishes the immutable
  release/container aliases from that one build, and the redundant tag-triggered
  build is suppressed. Container image jobs now reuse the Ubuntu 24.04 package
  artifacts from `package-linux` instead of recompiling the same all-feature
  package for every workload/architecture variant. Matrix parallelism is
  enabled across independent hosted amd64 jobs and the shared ARM lock remains
  one-at-a-time; the three independent workload manifest publishers also run
  concurrently. Manual dispatch retains tag selection while its controls now
  fit GitHub's ten-input limit.
- [x] Validation passed with `git diff --check`, Ruby YAML parsing for every
  workflow, the branch/tag resolution probe for `v0.1.28`, and
  `go run github.com/rhysd/actionlint/cmd/actionlint@v1.7.7
  .github/workflows/*.yml` finding no new workflow or changed-step errors
  after the changed warning was removed. The command still exits non-zero for
  two pre-existing ShellCheck style warnings in untouched workflow scripts
  (`SC2129` and `SC2016`).

## Verification update — 2026-09-20 10:00Z: CI test scheduling and deployment gates

- [x] Reproduced the failed `runtime_manager_persists_and_resumes_workspace_state`
  test from run `35498950750`. The test was one of several Cargo harnesses
  launched beside the all-feature library suite; the scheduler remained at
  step 0 for the 15-second bound while the other tests completed. The focused
  all-feature harness passed locally with `NM_DISABLE_OPENCL=1`.
- [x] Updated `.github/workflows/build-and-release.yml` to execute the large
  library/web UI test group and the integration test group as separate steps.
  Cargo reuses the same target directory, so this removes CPU contention without
  recompiling the graph or duplicating package/container work. The runtime
  persistence test now uses a two-worker Tokio harness and a 30-second bounded
  readiness window.
- [x] Validation passed for the all-feature library and web UI group: 419 library
  tests and 15 web UI tests. The complete `runtime_manager` integration harness
  passed all four tests with the CI OpenCL-disabled environment. A local
  wildcard integration build without the workflow's resource limits hit the
  host linker with `SIGBUS`; this is retained as a local capacity limitation,
  not a Rust test assertion failure. The workflow's `CARGO_BUILD_JOBS=2`, zero
  incremental/debug artifacts and runner disk controls remain in effect.
- [x] Ansible syntax validation passed for
  `swarmhpc/ansible/continuum_tenant_aarnn_site.yml`. The accelerator facts are
  now derived before their environment/assertion consumers, and mutable AARNN
  aliases such as `latest` probe the complete commit-qualified orchestrator,
  web UI and node image set before allowing a local rebuild. Explicit component
  image references and intentional rebuild flags remain deliberate overrides.
- [ ] A new successful GitHub Actions run containing the runtime gate has not
  yet published the replacement image. The live Kubernetes orchestrator still
  requires redeployment against that new digest before rollout recovery can be
  claimed.
- [x] `2026-09-20 10:05Z` Stopped the local resource-limited wildcard build after
  disk pressure was reported. The ignored `target/debug` cache had grown to
  174 GB (including 69 GB incremental state and 101 GB test dependencies); it
  was removed with `find` while preserving source, Git data, release output and
  QA evidence. Free space increased from approximately 2 GB to 176 GB.

## Verification update — 2026-09-22: remote UI startup contract

- [x] Diagnosed the remote UI startup failure: `--orchestrator` started a local
  all-feature management service, but only the remote client bearer token was
  supplied, so the local service failed closed with `management principal is
  not configured`.
- [x] `--ui-remote-only` now implies `--ui`, rejects local `--orchestrator` or
  `--node` roles, and forwards `--orchestrator-bearer-token` into the remote UI
  connection. OpenMPI-inferred local roles receive the same guard.
- [x] Added the corrected remote UI command to `docs/operations.md`. Focused
  UI-profile tests pass: 6 tests passed; formatting and whitespace checks pass.

## Verification update — 2026-09-22: ARM matrix cancellation and publication recovery

- [x] Reviewed run `35714382288` with authenticated `gh` CLI and inspected its
  job annotations. The six ARM container entries shared one concurrency group;
  GitHub retains only one pending request per group, so later matrix entries
  cancelled earlier pending entries even though `cancel-in-progress` was false.
  The run consequently lacked immutable ARM tags and skipped its manifest jobs.
- [~] The active recovery is running on `qc00`; its queued entries are being
  recovered sequentially while the registry is checked for the nine required
  immutable workload/variant tags.
- [x] Changed package locks to be unique per runner architecture and Ubuntu
  artifact, and container locks to be unique per workload and variant. The
  physical `qc00` runner still serializes its own jobs, while GitHub retains
  every matrix entry. Container job names now include the variant.
- [x] Reduced manual dispatch to GitHub's ten-input limit by replacing the
  separate `publish_release` boolean with `release_mode=none|release|draft|prerelease`.
- [x] Ruby YAML parsing and `git diff --check` pass. Actionlint reports only
  the two existing ShellCheck style warnings at the package and release shell
  blocks; no changed workflow expression or event error remains.

## Verification update — 2026-09-22: remote UI status and CUDA diagnostic

- [x] Reproduced the reported native UI state from the supplied screenshot.
  The CUDA log reports an unsupported NVRTC PTX version, then successfully
  loads the device-matched CUBIN fallback and selects CUDA; this is a warning
  about the optional PTX path, not a CUDA backend initialization failure.
- [x] Verified the remote endpoint from the workstation: DNS resolves
  `aarnn-orchestrator.neuralmimicry.ai` to `192.168.1.61`, TCP/50051 accepts
  connections, and the port returns the expected plaintext HTTP/2 gRPC
  response. The live k3s pod is Ready and its logs show eight connected nodes
  and active networks.
- [x] Hardened the native UI remote status loop with the shared gRPC endpoint
  and TLS policy, configurable connect/RPC deadlines, an immediate connecting
  state, and explicit timeout/error rendering. The existing deployment timeout
  variables are reused (`NM_ORCHESTRATOR_CONNECT_TIMEOUT_MS` and
  `NM_ORCHESTRATOR_RPC_TIMEOUT_MS`).
- [x] `cargo check --features ui`, `cargo fmt --check`, and `git diff --check`
  pass. A bounded hidden remote-UI launch reached the simulation startup path;
  no `aarnn_cuda_*` temporary files were left in `/tmp`, and root filesystem
  free space remained approximately 212 GB. The live endpoint was separately
  verified as Ready with active networks and eight connected nodes.

## Verification update — 2026-09-22: remote-only connection stages and view isolation

- [x] Moved remote status polling to a dedicated current-thread Tokio runtime
  so UI/simulation runtime work cannot leave the endpoint stuck at an
  uninformative `Connecting...` state. The worker now emits Connecting,
  Authenticating/requesting inventory, accepted, rejected/error and bounded
  retry outcomes.
- [x] Added a remote-only canvas gate. Until a real remote cluster snapshot is
  accepted, the canvas shows the connection stage and explicitly says that no
  local neural network is loaded; the default local `Runner` graph is not
  rendered or used as a remote fallback.
- [x] Wired accepted remote inventory into the remote cluster view and added
  the Loading neural network and Ready stages. Remote snapshot RPCs now use the
  configured RPC deadline and report load failures instead of remaining
  pending indefinitely.
- [x] Fixed the remaining remote-only state-machine deadlock: the UI now drains
  remote status messages before selecting the cluster view, allowing the first
  accepted inventory to transition `Standalone` to `ClusterGlobal` and start
  snapshot loading. Periodic inventory refreshes preserve `LoadingNetwork` and
  `Ready` instead of regressing the displayed stage to `Accepted`.
- [x] `cargo check --features ui`, `cargo fmt --check`, and `git diff --check`
  pass; `cargo test --features ui --lib service_` passes all 9 focused tests.
  Existing repository compiler warnings remain non-fatal.

## Verification update — 2026-09-23: GHCR latest promotion boundary

- [x] Cross-checked `.github/workflows/build-and-release.yml`,
  `scripts/build_container.sh`, `scripts/container_workloads.sh` and the
  Ansible AARNN image-resolution path. The workflow's immutable SHA manifests
  are the source of truth; Ansible consumes the role aliases
  `latest-orchestrator`, `latest-node` and `latest-web-ui` after probing the
  complete set.
- [x] Moved mutable GHCR alias publication out of the manifest matrix. The
  new serialized `container-latest` job runs only after every manifest build
  and pull verification succeeds, then promotes and verifies all role aliases
  plus the compatibility `:latest` alias from the node/runtime manifest.
  `container-promote` and automatic version tagging now wait for that final
  promotion on the main push path.
- [x] Validation passed with Ruby YAML parsing for every workflow, Bash syntax
  checks for the supporting container scripts, static promotion dependency/order
  assertions and `git diff --check`. Actionlint reports only the two existing
  ShellCheck style warnings in untouched workflow blocks (`SC2129` and
  `SC2016`); no new workflow expression or changed-step error was reported.
  No GHCR publication or deployment was run locally.
- [x] Cross-repository deployment alignment completed in
  `swarmhpc/ansible/roles/continuum_tenant_aarnn`, `host_vars/spirit.yml` and
  `scripts/deploy_mixed_cluster.sh`: default rollout tags now use `latest`,
  legacy and current base-image inputs resolve to the workflow's
  `latest-orchestrator`, `latest-node` and `latest-web-ui` aliases, and the
  full `ansible-playbook --syntax-check -i inventory
  continuum_tenant_aarnn_site.yml` passed.
- [x] Replaced the spirit host's stale pinned orchestrator image with
  `latest-orchestrator-arm64-64k-hwe`, so the documented Ansible command with
  `continuum_tenant_aarnn_image_tag: latest` now selects a coherent latest
  HWE orchestrator, node and web UI set unless an explicit environment override
  is supplied.

## Verification update — 2026-09-23 19:31Z: remote-only cluster UI resilience

- [x] Correlated the local Webots cluster logs with the native remote-only UI
  path. The UI was opening a fresh gRPC channel for every two-second inventory
  poll, marking a previously healthy endpoint disconnected after any transient
  status error, and immediately retrying the expensive multi-shard snapshot.
  The resulting repeated `snapshot connect failed: transport error` messages
  explained both the intermittent canvas and the loss of the stable biological
  witness target.
- [x] Kept one gRPC client channel per remote endpoint, reconnecting only after
  a failed status RPC with a bounded 1/2/4/8/10-second backoff. A transient
  failure now preserves the last accepted inventory and selected network while
  showing reconnecting state; an endpoint with no accepted inventory still
  fails closed as before. Incomplete empty inventory responses also retain the
  last good node/network maps.
- [x] Added equivalent bounded backoff to cluster snapshot refreshes and kept a
  last valid snapshot during refresh failure. A complete biological topology
  in any successful merged cluster projection is now retained as a topology
  witness across placement digest churn, and therefore remains higher priority
  than the synthetic layer/placement layout. View changes still invalidate the
  witness so one brain cannot bleed into another.
- [x] Validation passed with `cargo check --locked --no-default-features
  --features engine_runtime,ui,growth3d,robot_io --bin aarnn_rust`, the focused
  `topology_presentation_tests` suite (7 passed), `cargo fmt --all --check`,
  and `git diff --check`. Existing compiler warnings remain non-fatal; no live
  Webots process was stopped or reset.

## Verification update — 2026-09-23 19:53Z: remote-only biological topology profile

- [x] Confirmed the latest local UI log failure was a strict cluster-cut
  frontier mismatch (`worker_02` ahead of the requested cut), followed by
  `growth3d topology is unavailable`. The latter came from the documented
  command using `--features ui` without the `growth3d` feature, so the client
  could only render the synthetic ordered layer view.
- [x] Made the desktop `ui` feature include `growth3d`, so the supported
  `cargo run --bin aarnn_rust --features ui -- --ui-remote-only` profile can
  decode and render the biological topology returned by a remote worker.
- [x] Stabilised witness recovery in `src/ui.rs`: remote-only mode can use the
  known orchestrator as a witness source while placement metadata is incomplete;
  the request remains keyed to the orchestrator after a worker witness succeeds;
  and a complete witness remains visible while a later strict cut resynchronises.
  Remote status stays Ready with an explicit resynchronising detail instead of
  flickering into an error state.
- [x] Added a focused regression test proving that a mismatched cluster cut
  preserves a witness only for the same selected network. Validation passed:
  `cargo check --locked --no-default-features --features
  engine_runtime,ui,robot_io --bin aarnn_rust`, `cargo check --locked
  --features ui --bin aarnn_rust`, the focused topology suite (8 passed),
  `cargo fmt --all --check`, and `git diff --check`. Compiler warnings remain
  non-fatal and pre-existing.

## Verification update — 2026-09-23: WebGL backend startup profile

- [x] Correlated `logs/webgl_sim_496241/webots_orchestrator.log` with the
  launcher failure. `launch_webgl` built `aarnn_rust` and `web_ui` with
  `--all-features`; the resulting orchestrator enabled `management_v1` and
  exited with `management endpoint requires NM_MANAGEMENT_BEARER_TOKEN in
  static-reference mode`. Worker connection retries and discovery
  `Address already in use` messages were secondary effects of that early
  orchestrator exit.
- [x] Changed `launch_webgl` to build the neural backend with
  `NM_WEBGL_RUNTIME_FEATURES` through the shared `webots_cargo_profile_args`
  helper, defaulting to the explicit local `engine_runtime,ui,robot_io,cuda`
  profile. The headless `web_ui` gateway is built separately with its explicit
  `engine_runtime` profile. The backend remains `--no-orchestrator-ui
  --node-ui-hidden`; after gRPC readiness the launcher starts `web_ui` and
  exposes the WebGL page.
- [x] Added a launcher contract test covering the profile selection and
  headless/backend ordering. `bash -n`, `git diff --check`, the focused Cargo
  feature checks, and the launcher contract test passed. A bounded live probe
  with three workers registered all nodes, verified `/api/config`, and printed
  the WebGL URL. The probe logs show no management-token startup error; the
  existing CUDA PTX warning is non-fatal and the process was stopped by the
  bounded test cleanup.
- [x] Kept the gateway transport profile consistent with the backend by
  exporting `NM_WEBOTS_RUNTIME_FEATURES` for delegated startup and clearing
  inherited local mTLS variables from `web_ui` when `management_v1` is absent.
