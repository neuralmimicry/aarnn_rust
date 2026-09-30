# Build governed workstation I/O, federation and complete migration

This ExecPlan is a living document maintained under `.agent/PLANS.md`. It implements Phase 8 and the final definition of done in `docs/specifications/distributed-whole-brain-emulator-v1.1.md`.

## Purpose and observable outcome

Complete the platform so authorised web and Rust workstations can both manage brains and concurrently act as governed audio, visual, keyboard, pointer and bidirectional USB AER endpoints. Capture/device time is mapped deterministically into biological logical time; committed effects are deduplicated and safety-gated; one modality's congestion or reconnect creates channel-scoped quality state without stopping the others; federated brains exchange authorised positive-delay events; measured optimisation uses all suitable CPU/GPU/memory/storage resources without changing semantics; and the obsolete layer-sharding path is removed after migration.

## Specification authority and traceability

- Primary sections: 3.4, 12.2–12.4, 16.15–16.24, 17.4, 17.7, 18, 19, 20.9–20.10, 21.5, 21.9 and 21.13–21.15.
- Invariants: all `INV-001`–`INV-017`, especially `INV-002`, `INV-003`, `INV-008`, `INV-013`, `INV-016` and `INV-017`.
- Tests: `UT-IOTIME-001`, `UT-IOSAMPLE-001`, `UT-EFFECT-001`, `UT-HID-001`, `UT-AERUSB-001`, `UT-AERUSB-002`, `CT-013`–`016`, `API-013`–`019`, `IO-E2E-001`–`014`, federation tests in Section 21.9, full determinism/numerical matrix and Section 21.15.
- Phase gate: both clients complete governed live/recorded A/V/keyboard/pointer input, concurrent bidirectional USB AER exchange and committed A/V/sandbox output; optional native/global HID remains separately disabled until its hazard review and safety gate pass; federation, upgrade, migration, rollback and full acceptance evidence are complete; legacy layer execution is unreachable and removed.

## Prerequisites and phase boundary

Phases 0–7 must be green, including production fencing and peripheral/federation resource governance. This final phase must close every explicitly deferred test and cannot weaken an earlier gate for performance. Global OS-level keyboard/mouse actuation is not implied by general completion: it is an optional privileged capability requiring separate operating-system-specific hazard approval.

## Scope

- Implement a secure peripheral gateway and session/binding state machines governed by Phase 7 resources, grants, generations and actuator leases.
- Implement versioned external-clock calibration with drift, uncertainty, discontinuity, late policy and immutable capture-to-logical-time mapping.
- Implement bounded modality pipelines and deterministic transducers for microphone/audio, camera/video, display capture, focused keyboard and pointer input.
- Implement a bidirectional USB AER adapter with device capability negotiation, address/event mapping, device or host capture timestamps, sequence/CRC/overflow evidence, bounded asynchronous transfers, hot-plug epochs and independent input/output channels.
- Permit USB AER, A/V and HID channels to remain active simultaneously for one brain. Allocate independent channel IDs, sequences, clock mappings, queues/credits, metrics and cancellation domains; enforce fair multiplexing and reserve management/safety capacity.
- Implement neural effect decoders and committed audio/video/sandbox presentation with `EffectId` dedupe, deadlines, quality and safe neutral states.
- Support WebRTC/media/data channels or an evidence-backed equivalent for high-rate I/O; keep management/lease/emergency traffic reliable and reserved.
- Implement browser consent/capability UX and native device adapters without blocking UI/render/audio threads.
- Implement recorded input and pinned transducer/clock replay.
- Implement optional native virtual-HID adapters behind allow-lists, local arming, short fenced lease, watchdog, release-all and emergency stop; keep off by default until independent approval.
- Implement authorised federation links, time-base mapping, positive minimum delay, backpressure, revocation, replay/dedupe and cycle validation.
- Tune batching, checkpoint cadence, predictor/device placement and state layout from reproducible profiles only.
- Migrate persisted data/deployments, publish runbooks/docs/ADRs and delete temporary flags/legacy layer broadcasts after the rollback window.

## Non-goals

- Do not claim human-equivalent perception, biological validity or real-time deadlines without measured profile-specific evidence.
- Do not use packet arrival time as capture/logical time or retroactively retimestamp admitted samples.
- Do not let browser code produce global/native HID effects.
- Do not apply provisional/non-convergent high-risk effects by default.
- Do not approve zero-delay cross-brain federation cycles implicitly.

## Repository orientation

Locate Phase 7 management clients, browser asset/build paths (`app.js`, `index.html`, service/shell modules), Rust UI event/render loops (`ui.rs`), gateway/protocol crates, media/device dependencies, AER/bridge/transmission transducers and deployment/network policy. Locate existing USB/libusb/rusb/serial drivers, native permissions/udev rules, WebUSB support or a signed local companion, endpoint/framing definitions and hot-plug handling. Record supported browsers/OSs, secure-context requirements, codec/device feature gates and where USB/audio/render/input callbacks could block or allocate unboundedly.

The intended modules are `peripheral/session`, `peripheral/clock`, `peripheral/admission`, `peripheral/multiplexer`, `device/usb_aer`, `transducer/{audio,video,aer,hid}`, `effect/commit`, `effect/dedupe`, `effect/safety`, `gateway/media`, `client_web/io`, `client_native/io`, `federation/link` and `federation/time`. USB/media adapters do not mutate neural state directly; transducers emit ordinary versioned causal events through the governed data plane.

The current checkout resolves the implementation paths to `src/providers.rs` and
`src/ui.rs` for the native Rust UI, `web_ui/index.html`, `web_ui/app.js` and
`web_ui/style.css` for the browser shell, `apps/android/app/src/main/**` for
the Android shell, and `apps/ios/*.swift` for the portable SwiftUI source (there
is no checked-in Xcode project). The existing visual
providers produce sensory spikes but do not retain a preview frame; the web
input panel only admits HTTP AER; Android reports camera/media unavailable;
and the flat CLI has no video-file or camera source flags. This feature closes
the user-visible preview/parity slice while retaining the governed admission
boundary and recording the missing iOS project as a delivery constraint.

The INV-017 ingress review resolves the current workspace to the root Cargo
package/library, `aarnn-biox6-exporter`, and `tools/xtask` (`cargo metadata
--format-version 1 --no-deps`). The relevant authorization and ingress code is
in `src/management.rs`, `src/peripheral.rs`, `src/bin/web_ui.rs`,
`src/distributed.rs`, and `proto/distributed.proto`; the Webots client is under
`webots_service/`. `build.rs` generates tonic/prost Rust bindings from the two
protobuf sources into Cargo's build output. No `CONTRIBUTING` file or
deployment reference to `NM_PERIPHERAL_INPUT_GRANTS_JSON` was found. CI's
authoritative host checks include formatting, Clippy, all-feature check, the
library/web-UI tests and integration tests; the ARM runner executes the
all-feature check/build and its focused runner suite. Android is a Gradle
reference shell with no signing credentials; iOS currently has Swift sources
but no Xcode project, signing configuration or generated XCFramework.

On `main` at `345b788`, the browser AER routes `/api/aer/inject`,
`/api/aer/infer`, and `/api/aer/stream` are protected only by general
`aarnn:use`; `AuthMode::None` bypasses that service check. `Capability` already
contains `PeripheralInput` and `PeripheralOutput`, but these capabilities are
not consulted by those routes. `src/peripheral.rs` has only caller-supplied
input/output booleans on a bound channel; it does not yet model a principal-
and-brain-scoped grant, expiry, revocation, or a local-consent/indicator state.
The distributed `PrepareSensoryInput`/`CommitSensoryInput`/`AbortSensoryInput`
methods are worker-to-worker admission and do not supply workstation consent.
The reference browser gateway now reads an exact principal/brain AER-input
grant list from `NM_PERIPHERAL_INPUT_GRANTS_JSON`, formatted as
`[{"principal":"...","brain_id":"..."}]`. The matching authenticated
principal must still create a locally consented session (300-second default,
900-second maximum) before `/api/aer/inject`, `/api/aer/infer`, or
`/api/aer/stream` accepts input. The short-lived session can be inspected and
revoked by its owner; `AuthMode::None` always denies it. The registry is
process-local reference state and is not Phase 7 replicated authority, so
restart removes active sessions and production workstation I/O remains gated.

The local and origin Codex branch `codex/webots-api-ingress-20260929` is still
checked out in `/tmp/aarnn-webots-api-ingress-20260930` and contains five
commits beyond `main`. Its placement-aware sparse ingress is being merged
through the existing brain-scoped policy and this short-lived session gate;
the branch's parallel deployment-static grant map is excluded. Its internal
gRPC ingress now fails closed when its service bearer is unconfigured. Hosted
Webots acceptance and Phase 7 replicated session authority remain incomplete.
The prior Phase 7 plan records its
replicated-authority, identity, and audit gates as incomplete; this change may
add a fail-closed reference/session contract, but it must not claim production
management authority or enable `workstation_io`.

## Architecture and safety constraints

External samples carry device/session sequence and capture-clock timestamps. A versioned calibration maps capture time to an eligible `LogicalTag` using declared rounding, uncertainty and late policy; arrival jitter affects latency metrics only. Sleep/clock jumps close one mapping and create another. Recorded replay pins raw input, clock map, transducer version/config, numerical profile and admission policy.

High-rate unreliable/partially reliable media is separated from reliable buttons, management, lease, effect commit and emergency stop. Pre-admission coalescing/drop is allowed only by modality policy and produces gap/quality records. After sensory-event admission, causal events cannot be silently dropped. Queues/memory are bounded and backpressure preserves neural/control safety traffic.

USB AER is not an internal shard transport and cannot bypass the peripheral gateway, binding, clock mapping, durable admission or committed-output boundary. AER input maps device addresses/polarities into stable receptor targets; AER output maps committed effector events to allow-listed device addresses. Prefer device-provided monotonic timestamps; otherwise stamp at the host read/completion boundary and record the larger uncertainty. A device reconnect creates a new device epoch and never reuses a stale mapping implicitly.

Each modality has an independent failure and flow-control domain. USB removal, endpoint stall, FIFO overflow or malformed AER frame degrades/closes the AER channel and records an explicit gap/quality event while microphone, camera, display and HID continue. Conversely, video saturation cannot starve AER acknowledgements or USB output. A fair multiplexer enforces per-channel budgets plus reserved control, lease and emergency-stop capacity.

Outputs are staged by the authoritative brain and exposed only after the declared commit boundary. `EffectId` dedupe survives reconnect/failover. Deadline miss is applied/expired explicitly and never rewrites neural state. Non-convergence/failover quality reaches consumers; high-risk effect channels suppress by default.

Browser capture requires visible consent/session indication and one-action stop; keyboard/pointer are focused and Pointer Lock exit remains available. Browser clients never claim global actuation. Native/global HID, if approved separately, requires local physical arming, narrow allow-list, fenced short lease, watchdog, release-all on every failure and independent emergency stop.

Federation preserves independent `BrainId`, time domains, quotas and authority. Links require dual authorisation, stable event IDs, declared time mapping and positive minimum delay unless a separately approved combined-component design exists. Unapproved zero-delay cycles are rejected. Failure policy is link-local and explicit.

## Milestones

### Milestone 8.1 — Governed session, clock and admission reference

Implement peripheral session/binding state machines, capability/grant checks and a single-thread reference clock mapper/admission path. Golden-test drift, uncertainty, mapping discontinuity, reorder, duplicate, gap, coalescing and late policies before live devices.

### Milestone 8.2 — Deterministic sensory transducers and replay

Implement versioned audio, visual, USB-AER, keyboard and pointer transforms with units, parameter provenance and scientific fixtures. Record raw input plus pinned device epoch, clock mapping and transform and prove exact admitted-event replay in the deterministic profile.

### Milestone 8.3 — Committed effects and safety core

Implement effect staging/commit, stable `EffectId`, gateway/client dedupe, deadline/quality semantics, lease enforcement, sandbox outputs and fail-safe neutral/release-all state. Fault-test disconnect, failover and provisional output before optional native actuation.

### Milestone 8.4 — Web workstation I/O

Add secure-context capability detection, consent/revocation, microphone/camera/display capture, focused keyboard/pointer/Pointer Lock input, A/V/sandbox presentation, visible indicators and stop control. Where WebUSB is securely supported, add explicit user-selected USB AER access; otherwise use an authenticated, origin-bound local companion with an equivalent permission/indicator/stop model and no general device proxy. Use browser-supported media/data channels, worker/worklet paths and bounded buffers; state clearly that global HID is unavailable. Prove USB AER remains active concurrently with A/V/HID without blocking the main thread.

### Milestone 8.5 — Rust workstation I/O

Add OS capability reports, permission/hot-plug handling, non-blocking audio/video/display/focused-input pipelines, a narrow libusb/rusb-equivalent USB AER adapter and committed presentation. Use asynchronous/bounded USB transfer submission and completion; never block the render, audio or management runtime. Keep optional virtual HID behind a separately compiled/configured safety gate; run watchdog/emergency-stop tests per supported OS before enabling it anywhere.

### Milestone 8.5a — Cross-product video preview and interface parity

When the selected input is a video file or camera, retain only the latest
bounded decoded frame for presentation in a native floating egui window, a
browser video pop-out, and mobile preview dialogs/sheets. Keep raw preview
pixels separate from admitted sensory events, preserve explicit stop/close
state, and expose the same source vocabulary (`video-file` and `camera`) through
the CLI and product capability reports. iOS source is added as a portable
SwiftUI contract because the repository currently has no Xcode project; the
actual signed application remains an external platform packaging gate.

### Milestone 8.6 — Federation and multi-workstation load

Implement positive-delay authorised links, time mapping, revocation, backpressure and replay/dedupe. Run two workstations/two brains plus the four-brain fleet; verify isolation and explicit dependency/failure behaviour.

### Milestone 8.7 — Evidence-led optimisation

Profile causal critical paths, allocator/state layout, queues, batching, GPU transfers/kernels, checkpoint cadence and scheduler predictions. Apply only changes that preserve reference digests or documented fast-profile tolerances; retain benchmark and scientific evidence.

### Milestone 8.8 — Migration, legacy removal and final gate

Provide persisted-state/config/deployment migrations, rolling-upgrade and rollback rehearsal. Close all deferred tests, publish project/architecture/protocol/security/scientific/runbook documentation, remove layer-group fallback and temporary flags, and prove no old direct-worker/layer-broadcast path is reachable.

## Progress

- [~] `2026-09-30 06:32Z` Started the authorized INV-017 vertical slice after
  verifying the clean `main` baseline (`345b788`), Cargo workspace, current
  AER ingress authorization, and the still-existing Webots Codex branch.
  General `aarnn:use` and auth-mode `none` are insufficient; the implementation
  is being designed around a separate resource-scoped peripheral permission,
  a short-lived active session, explicit local consent/visibility, and
  immediate revocation. The user clarified that robot elements share
  wall-clock time to account for computation/latency differences; record that
  as a common pacing/service-level reference while retaining independent
  per-brain logical time and no fleet-wide slowest-network barrier.

- [x] `2026-09-30 07:21Z` Implemented deployment-configured exact principal/brain
  grants using the existing `Capability::PeripheralInput` policy, short-lived
  locally consented AER-input sessions, owner status/revoke endpoints, and
  browser/WebGL indicators with explicit stop/revoke handling. AER inject,
  infer, stream and network-targeted LLM mirror requests check both the current
  grant and active matching session; auth mode `none` fails closed. Grant
  configuration replaces stale persisted input grants on restart. Validation:
  `cargo test --lib peripheral` (10 passed), `cargo test --bin web_ui` (18
  passed), and `cargo test --test web_ui_browser_compat
  webgl_simulator_is_shipped_through_the_authenticated_gateway` (1 passed).
  `rustfmt --check`, JavaScript syntax, shell syntax, workflow YAML and
  `git diff --check` passed. The session registry is still process-local
  reference authority; production profiles remain disabled pending Phase 7.

- [x] `2026-09-30 07:21Z` Fixed container promotion disk exhaustion on `sm00`:
  isolated Podman runroot, graphroot and temp storage now use disk-backed
  `/var/tmp`; manifest and latest-alias jobs prepare and clean their isolated
  storage even on failure. The failing run's ARM64/X64 builds and three
  manifest jobs had succeeded, leaving only latest-alias promotion failed.
  Recovery dispatch and remote success verification remain pending after the
  workflow fix reaches `main`.

- [~] `2026-09-30 07:21Z` Reviewed `codex/webots-api-ingress-20260929` (five
  commits): its placement-aware sparse sensory admission and bridge-width fixes
  are useful, but its deployment-static allow-list is a duplicate policy
  authority. Merge the data path only through the existing management policy
  and the new short-lived peripheral session gate; then run the ingress and
  workflow recovery checks before deleting Codex branches.

- [~] `2026-09-30 07:25Z` Reconciled the Webots sparse ingress with the
  `PeripheralInput` policy and active session gate at `/api/aer/inject`; the
  internal orchestrator RPC retains its fail-closed service bearer. Sparse
  retries now require a bounded stable producer `session_id`, and frame
  sequence is bound to the non-negative source step. The branch's separate
  static grant map is excluded. Run the distributed ingress regressions and
  final merged-tree checks before marking this consolidation complete.

- [x] `2026-09-30 07:35Z` Merged-tree verification passed: `cargo test --lib
  external_sensory` (2 passed), `cargo test --bin web_ui` (18 passed), the
  browser gateway compatibility test (1 passed), `cargo fmt --all --check`,
  and `git diff --cached --check`. Actions run `36579172049` failed on its
  older `fcb7487` workflow because `Publish Release` invoked `gh` without
  installing it; the current `main` baseline contains the CLI installation
  step from `230a4ee`. Push and verify a fresh run after consolidation; then
  remove the reviewed Codex branches and worktrees.

- [~] `2026-09-30 10:51Z` Actions run `36689171325` built the Ubuntu 24.04
  X64 package, then failed its metadata-probe container with exit 126:
  rootless Podman's `pasta` helper could not open its network namespace under
  the isolated `/var/tmp` runroot (`Permission denied`). The build and smoke
  install already use host networking; the metadata-only probe did not. Added
  `--network host` to that probe. Validate the workflow and rerun the complete
  release workflow before closing this CI recovery item.

- [~] `2026-09-30 08:20Z` The fresh run `36684684809` confirmed the X64 build
  and lint checks, then exposed 13 test-only `NetworkResources` initializers
  missing the new `load_fingerprint` field. Added neutral zero fingerprints
  to those unit and integration fixtures. Reviewed and retained commit
  `b8b91ad`, which restores explicitly configured biological I/O dimensions
  after worker reload/reset using deterministic resize seeds. Local
  verification passed with all features: 558 library tests and 18 web UI
  tests, the worker-reload I/O regression (1), and compile-only checks for
  `live_migration_registration` and `stable_activation_heartbeat`; formatting
  passed. Publish this fix and verify the replacement Actions run before
  closing the workflow recovery item.

- [x] `2026-09-29 08:21Z` Diagnosed and fixed the frozen output raster as a split data
  source: canvas brightness consumed fresh `GetNetworkActivity` worker polls,
  while the raster only advanced from aggregate cluster snapshots. The worker
  RPC already returned up to 128 output-history frames, but the UI discarded
  them. The native view now carries those sparse histories, merges owners by
  simulation step, includes silent steps, ignores a poll behind the raster
  cursor, and clears history on an actual step rewind. The regression confirms
  scrolling zero-output steps and merged multi-owner spikes; all 29 native
  presentation tests pass. `./run_examples.sh` built the release example and
  passed readiness checks with both workers joined and managed audio input
  acknowledged. The captured dashboard showed stage-three connections and
  192 output spikes in the raster. That single screenshot confirms live raster
  data, while deterministic tests verify advancement across simulation steps.

- [x] `2026-09-29 03:34Z` The stage-three paint trace showed that acknowledged
  audio activity and cached synthetic edges were present, and that the neuron
  pass emitted opaque one-device-pixel rectangles above the connections. Egui
  0.34's rectangle tessellator simplifies rectangles no wider than its
  feathering threshold into line segments, which can leave a one-pixel neuron
  without a covered fragment. Replaced per-neuron `rect_filled` calls with one
  batched mesh of framebuffer-aligned coloured quads and removed the temporary
  white paint marker/trace. The new mesh geometry/brightness regression and all
  28 native topology/presentation tests pass. `cargo fmt --check`,
  `bash -n run_examples.sh` and `git diff --check` pass. A fresh all-features
  `./run_examples.sh` run built the release binaries, verified both workers
  joined `cluster_master`, acknowledged the first 64-wide sensory frame at
  `node_2`, and captured stage three with 1,637 connections and eight active
  neurons at `/tmp/aarnn-stage3-pixel-20260929.png`. The framebuffer shows
  brighter blue sensory pixels over the synthetic connection lines. The
  launcher was terminated after capture and ran its cleanup trap (exit 143 is
  the expected SIGTERM status after all readiness/capture checks passed).

- [x] `2026-09-29 03:05Z` Three further `./run_examples.sh` captures confirmed the
  cluster audio route was acknowledged and stage three contained 1,573–2,119
  projected edges, yet still showed no activity pixels. An opt-in native paint
  trace reports the expected 64/300/96 sensory/hidden/output positions and
  verifies that a clipped diagnostic shape appears at the first sensory
  coordinate. Egui drains only registered area layers, so the neuron pass was
  moved back to the active canvas painter after its connections. The later
  paint trace ruled out visibility, colour and painter order; the remaining
  rasterisation issue is resolved by the 03:34Z mesh fix above. The stage-three
  capture readiness condition also waits for a managed frame acknowledgement
  for the selected brain.

- [x] `2026-09-29 02:33Z` Repeated the required live run with
  `./run_examples.sh` after the 01:45Z stage-three fix. The capture showed
  2,100 synthetic connections and reported activity, but no distinct neuron
  pixels. The first capture also preceded the managed route acknowledgement;
  a second run showed the sensory display could become active from local audio
  preview while that route was still waiting. Pixel-stage colours now remain
  opaque over edge strokes, and markers use a clipped topmost painter. The
  stage-three capture gate now additionally requires an acknowledged managed
  sensory frame for the selected brain, so preview activity cannot satisfy
  cluster-input readiness. The subsequent trace isolated the egui rectangle
  rasterisation failure, and the 03:34Z capture above verifies the completed
  repair with acknowledged `message.wav` input.

- [x] `2026-09-29 01:45Z` Reproduced the user's stage-three report with the existing live
  checks: the cluster had a cached edge projection and fresh worker activity,
  but static and live connection strokes were painted after the neuron pixels
  and obscured their activity brightness. Pixel-stage nodes are now deferred
  until all edge layers have been painted, then rendered as framebuffer-aligned
  single-pixel mesh quads; placement rings are omitted at pixel detail to preserve
  the requested geometry. The new one-pixel-grid regression and all 26 native
  topology/presentation tests pass, as do workspace formatting, launcher shell
  syntax and git diff --check. The required live stage-three capture through
  ./run_examples.sh with message.wav is verified by the 03:34Z capture above.

- [x] `2026-09-29 01:24Z` Reopened native stage-three validation after the live
  capture showed zero activity and no completed edge projection. The cluster
  edge worker subsequently reported 2,124 edges, while `GetNetworkActivity`
  returned live worker spikes and the WAV sensory bridge acknowledged frames.
  The native `ClusterGlobal` view still reads activity from its aggregate
  snapshot only; it does not refresh current spikes from assigned worker
  owners. Its stage-three soma radius is also larger than one framebuffer
  pixel, and the screenshot timer can fire before the edge/activity projection
  is ready. Planned work is a bounded, parallel, display-only activity poll
  fenced to the current network assignment, single-pixel stage-one-to-seven
  markers, and opt-in readiness-gated stage-three capture. These changes must
  not wait on or mutate neural traversal. Verification will include focused
  worker-activity/render tests and the manual-stage-three `./run_examples.sh`
  run with the selected WAV; the completed evidence is recorded above.

- [~] `2026-09-29 00:40Z` The user's manual stage-three cluster screenshot shows
  active audio acknowledgement and populated synthetic neuron columns, but
  `Per-layer connections: (busy)` remains indefinitely and no edge lines or
  distinct activity brightness appear. Live API inspection confirms non-zero
  sensory weights plus active sensory/hidden indices. The native UI discarded
  its edge cache on every refreshed cluster cut and fenced the asynchronous
  visual projection to exact snapshot-tick equality, so same-assignment results
  could be invalidated while the neural cut advanced. The projection now stays
  visible across newer cuts, accepts a result only when its network,
  assignment, generation and non-future tick still match, and refreshes if the
  neural cut advanced during projection. Network/assignment changes and time
  rewinds still invalidate the cache. The focused presentation suite passed
  21 tests; `cargo fmt --check` and `git diff --check` passed. The scripted live
  run confirmed both workers, non-zero sensory/hidden activity and 12,888
  non-zero sensory weights. Its framebuffer still used Auto because initial
  cluster-view selection overwrote the manual launch override, so it did not
  verify stage-three rendering.

- [x] `2026-09-29 00:48Z` Made explicit native visualisation startup overrides
  survive the distributed dashboard's automatic cluster-view selection, then
  reapply the selected stage through the same layout/cache path used by manual
  slider changes. Extended the launch-override regression to cover manual
  stage-three mode. The later all-features `run_examples.sh` capture with the
  selected WAV verified the override.

- [~] `2026-09-28 23:41Z` Traced the reported stage-three visual mismatch through
  the native and browser renderers. The native synthetic overlay was suppressed
  whenever a complete biological topology happened to be cached, even though
  stage three had selected the conventional synthetic arrangement. The browser
  renderer also passed an empty activity list to every sensory neuron. Fixes
  now remove the unrelated topology gate, give stage-three activity a stronger
  bounded brightness mapping, and wire browser sensory activity into the
  canvas renderer. Focused native and browser regressions are being added;
  live `./run_examples.sh` verification remains pending.

- [~] `2026-09-28 23:57Z` The first requested `./run_examples.sh` live run built
  both all-features binaries, verified `node_1` and `node_2` under
  `cluster_master`, and returned non-zero sensory indices from
  `/api/activity` at step 92. It also reproduced timeouts in the orchestrator's
  aggregate cluster-snapshot requests while sensory delivery was applying
  bounded backpressure. The native UI therefore needed a display-only fallback
  for acknowledged sensory frames while snapshots lag; that path now uses a
  non-blocking lock and rejects unacknowledged provider samples. Focused Rust
  tests (16 topology/render policy cases), browser canvas stage/activity checks,
  `rustfmt --check` and `git diff --check` pass. A final live run and all-feature
  executable rebuild remain pending.

- [~] `2026-09-29 00:08Z` The user screenshot confirms accepted audio frames
  but no visible stage-three links or changing neuron brightness. The local
  native dashboard was decoding the orchestrator's aggregate snapshot as if it
  had to include a shard named after the orchestrator; the active layer owner
  can instead be a worker. The activity fallback also replayed the same last
  acknowledged frame on every redraw, preventing its brightness trace from
  decaying. The snapshot selector now keeps worker validation strict while
  allowing an aggregate orchestrator projection, and acknowledged display
  frames now carry network/session/sequence provenance and are applied once.
  The 18 focused native presentation tests, both browser visualisation suites,
  `rustfmt --check` and `git diff --check` pass. The requested live
  `./run_examples.sh` run with the selected WAV remains pending.

- [~] `2026-09-29 00:23Z` The scripted live cluster joined both workers,
  acknowledged the selected WAV directly at a 64-input bridge, returned active
  sensory/hidden indices, and began publishing aggregate snapshots. Its first
  native capture was too early to assess the populated canvas, and the second
  used the application's default automatic detail mode rather than the user's
  manually selected stage three. Added bounded `NM_UI_VISUALIZATION_STAGE` and
  `NM_UI_VISUALIZATION_AUTO` launch overrides for reproducible capture runs, a
  test proving the cluster edge projection retains non-zero sensory/output
  links, and a regression for the orchestrator aggregate-shard selection. All
  20 focused native tests and both browser visualisation suites pass; a manual
  stage-three `./run_examples.sh` capture remains pending.

- [x] `2026-09-28 22:58Z` Fixed transient sensory bridge timeouts dropping a
  managed audio route. A bounded retry resends the same frame/session identity
  for up to 120 seconds when gRPC reports timeout, deadline, cancellation or
  resource-pressure errors; the bounded UI producer channel pauses further
  provider reads behind it. Permanent shape/ownership/pause errors close that
  route generation so later frames cannot conceal a gap. A regression admits
  a one-neuron frame while the managed Runner write lock is held. `cargo test
  --locked --all-features --lib sensory -- --nocapture` passed 35 tests, the
  retry-policy test passed, all 18 `run_examples_launcher` tests passed, four
  audio-I/O contract tests and six local cluster/credential tests passed, and
  `rustfmt --check`, `bash -n` and `git diff --check` passed. The required
  `./run_examples.sh` run decoded `/home/pbisaacs/Downloads/message.wav` at
  8 kHz/192,160 samples with S=64, verified both workers in `cluster_master`,
  activated the direct sensory route to `node_2`, and returned non-zero
  sensory indices from `/api/activity` at step 621. Transient gRPC cancellation
  was logged as ordered backpressure and the route stayed active until clean
  shutdown. The run ended with a frame still waiting behind worker computation,
  so complete file delivery under sustained load remains unverified and the
  Phase 8 gate stays open.

- [x] `2026-09-28 22:04Z` Fixed the live `run_examples.sh` audio path across
  the complete `--all-features` profile. A focused worker-startup regression
  confirms that the compatibility Runner honours `NM_DISTRIBUTED_AUTOSTART`
  even when `stable_executor_live` is compiled but no stable manifest is
  registered. The live rerun then exposed a second race: example workers had
  preloaded the checked-in zero-width `config.json`, began growth, and reported
  a hosted network before applying the orchestrator's aligned S=64 snapshot.
  Both worker launches now set `NM_PRELOAD_NODE_NETWORK=0`, so the distributed
  snapshot is the first network they load. With `message.wav`, the launcher
  reported both workers ready, activated the direct route to `node_1`, logged
  acknowledgement of frame 0 at width 64, and `/api/activity` reported
  non-zero sensory indices at step 157. No paused-network or width-mismatch
  errors occurred. Later frame preparations 77, 81, 132, 197 and 259 timed out
  during the CPU/morphology-loaded run, so uninterrupted full-file delivery
  remains unverified. The focused autostart test, all 18 launcher integration
  tests, four Python I/O-contract tests, `bash -n`, targeted Rust formatting,
  `git diff --check`, and the release build performed by `./run_examples.sh`
  passed; launcher cleanup left no service processes or listeners. The Phase 8
  workstation-I/O gate remains open.

- [x] `2026-09-28 21:30Z` Investigated the report that an audio file is
  selected while the cluster dashboard shows no managed sensory frames. The
  reproduced startup contract is zero-width in both `config.json` and
  `network.json`; `run_examples.sh` only prepares a positive run-local I/O
  contract when `AARNN_AUDIO_FILE` is supplied before launch. The native UI's
  later file selection cannot change the already-distributed topology. A
  positive config contract also currently updates `net_cfg` while the startup
  snapshot JSON remains zero-width, so orchestrator distribution can still
  publish a snapshot without the sensory matrix columns. The run-local I/O
  helper now prepares positive input capacity even when the file is selected
  after startup; the canonical Runner resize aligns snapshot matrices and
  runtime arrays before distribution. Focused snapshot QA passes, and the
  live route is covered by the 22:04Z entry below. The workspace root and
  package set were verified with `cargo metadata --locked --no-deps`; unrelated
  dirty files were preserved.

- [x] `2026-09-28 21:47Z` The focused snapshot tests pass and the release
  launcher reaches a two-worker ready state with an aligned S=64 snapshot.
  Live I/O exposed a separate startup-state mismatch: `/api/status` reported
  the orchestrator registry as playing, but the direct ingress owner rejected
  frames as `managed network is paused`. The initial diagnosis blamed a missing
  worker environment export; later inspection corrected this because the
  launcher already exported `NM_DISTRIBUTED_AUTOSTART=1` to every process.
  Instead, the `#[cfg(not(feature = "stable_executor_live"))]` fallback was
  absent in the launcher's all-features build, leaving a preloaded compatibility
  Runner paused when no stable manifest was supplied. The 22:04Z entry records
  the feature-independent policy fix and live evidence.

- [x] `2026-09-28 19:26Z` Reproduced the missing cluster members with
  `AARNN_NATIVE_UI=0 ./run_examples.sh`. The runtime cache contained the local
  gRPC certificate issued on 18 September and expired on 19 September; the
  environment helper previously reused it based only on file presence. Both
  workers then logged `connect: transport error`, `/api/status` returned 503,
  and the orchestrator reported zero connected nodes even though the launcher
  announced that the processes had started. The helper now checks certificate
  validity, CA-chain verification and private-key matches, rotating stale
  local material. The launcher waits for both worker addresses in the same
  authenticated status response used by the dashboard before reporting ready,
  and prints recent service logs on timeout. `python3
  scripts/qa/test_local_management_env.py` passed both reuse/rotation tests;
  `cargo test --locked --test run_examples_launcher` passed all 14 tests;
  `python3 -m py_compile ...`, `bash -n run_examples.sh` and `git diff
  --check` passed. The post-fix release run recovered the expired credentials,
  reported `node_1_775090649` and `node_2_1587868789` joined at ports 50075 and
  50087, and `/api/status` showed both nodes plus `cluster_master` distributed
  across two nodes (node 1 active, node 2 backup for layer 0). The orchestrator
  log reported `Nodes connected: 2`; shutdown left no service processes or
  listeners. The overall Phase 8 workstation-I/O gate remains open.

- [x] `2026-09-28 15:45Z` Re-ran the live cluster test through
  `AARNN_AUDIO_FILE=/home/pbisaacs/Downloads/message.wav
  AARNN_AUDIO_SENSORY_NEURONS=64
  EXAMPLE_RUNTIME_ROOT=/tmp/aarnn-run-examples-audio-managed-wait
  ./run_examples.sh`. The screenshot's `cluster_master` had zero connected
  nodes and no layer distribution, so it had no worker to act as the direct
  sensory I/O bridge; the WAV was decoded but could not be admitted. In the
  launcher run the route first reported that it was waiting, then activated
  directly to `node_1_926099529` at width 64 and logged acknowledgement of
  frame 0. `/api/status` showed layer 0 assigned to that worker and the
  network playing; `/api/activity?network_id=cluster_master` returned 18
  active sensory indices at step 1685. The UI had also been advancing the
  audio provider in its unrelated standalone Runner while a managed route was
  waiting. `SimControl::SetDistributedInput` now carries managed-view state;
  the simulation controller waits without advancing that Runner or consuming
  the file until the direct route is available. Wait and route changes are
  logged once for diagnosis. `cargo test --locked --all-features --lib
  sensory -- --nocapture` passed 33 tests, including the new managed-view wait
  test; `python3 scripts/qa/test_audio_io_contract.py` passed four tests;
  `rustfmt --check --edition 2024 src/ui.rs` and `git diff --check` passed.
  The release build in `./run_examples.sh` passed and launcher shutdown left no
  child processes. The complete Phase 8 gate remains open.

- [~] `2026-09-28 15:11Z` Re-ran the requested launcher with the selected WAV
  after the earlier sensory-width fix. `cargo metadata --locked --no-deps
  --format-version 1` confirms the canonical workspace and the native UI and
  distributed runtime live in `src/ui.rs` and `src/distributed.rs`. The
  run-local contract correctly changed both zero-width inputs to `S=64`, but
  `logs/nm-1790607888.log` records 4,732 rejected frames from 15:04:53Z through
  15:04:58Z while `cluster_master` was still being loaded; the first worker
  load is logged at 15:04:58Z. The assigned worker then appears in the
  orchestrator's hosted-network heartbeat metrics. This is a placement/readiness
  race: distribution plus a connected peer was treated as a live I/O bridge.
  Gate route publication and admission on the assigned worker reporting the
  network loaded, then verify that the retained file provider starts once the
  route becomes ready. The earlier launcher left three orphaned test processes;
  they were confirmed by PID, command line and private runtime root and stopped
  before the next run. Live acknowledgement evidence remains pending.

- [x] `2026-09-28 14:27Z` Revalidated the managed Start-to-audio repair after
  acknowledgement-schema validation was added. The two-worker integration test
  now sets a configured sensory target on the second worker and proves that
  only that worker receives frames directly, with prepare/commit acknowledgements,
  duplicate retry idempotency and changed-payload rejection. The lock-contention
  retry test passes; `cargo test --locked --all-features --lib sensory --
  --nocapture` passes all 30 matching tests. The all-features `aarnn_rust` and
  `web_ui` binary check, workspace format check and `git diff --check` pass.
  The orchestrator keeps route/control metadata scoped to each managed cluster
  network; input payloads go to that network's selected I/O bridge. Interactive
  desktop Start-to-playback verification remains outstanding.

- [x] `2026-09-28 12:09Z` Repaired the bounded native audio-to-sensory adapter for
  networks with fewer sensory neurons than audio feature bands. Its current
  rounded band-to-neuron ranges can be empty, silently discarding parts of the
  source. Keep the network's configured sensory width and heuristically map
  every source band onto an available input; where width is insufficient,
  multiple bands select the same neuron and each input receives the strongest
  mapped activity. The shared mapper serves audio-file and microphone
  providers, reuses bounded projection scratch, and leaves the neural topology
  unchanged. `cargo test --locked --features ui --lib providers::tests --
  --nocapture` passed all 10 provider tests, including deterministic mapping
  coverage from 1–64 inputs and low/high tones into a one-neuron network.
  `cargo build --release --locked --all-features --bin aarnn_rust --bin
  web_ui` also passes (5m20s; repository compiler warnings remain).
  `cargo fmt --all -- --check` and `git diff --check` pass. This is a bounded
  legacy UI transducer improvement; it does not close the governed Phase 8
  admission/transducer gate.

- [x] `2026-09-28 12:42Z` Cross-checked the reported silent sensory probe and
  found that the audio provider was sized from the workstation's local Runner,
  while managed input admission validates against the selected brain's sensory
  width. The provider's `last sensory spikes: 13/64` counter therefore showed
  generated local frames, not proof that a 64-wide frame had been admitted by a
  one-input managed brain. Startup audio also lacked a route when the UI opened
  directly on an already-playing managed view. Size providers from the live
  managed Runner (or its registry config), change provider width before enabling
  the route, start the route on selection of an already-playing managed view,
  and restore local width on stop/view change. Unknown managed width leaves the
  route disabled with a visible status. Audio-file selection in managed views
  no longer resizes the local Runner. Four route-sizing tests and the dynamic
  64-to-1 audio-provider regression pass; `cargo check --locked --features ui
  --lib`, workspace formatting and `git diff --check` pass with existing
  warnings. A live `run_examples.sh` cluster rerun remains outstanding.

- [x] `2026-09-28 13:06Z` Rechecked route activation from managed-view selection
  through provider generation and distributed admission. The start decision
  treated a stale registry `playing=false` as authoritative, even when the live
  managed Runner or local playing cache said the network was running; that
  could leave a preloaded audio source emitting 64-wide local frames while the
  selected one-input brain received none. Prefer live managed state, reconcile
  route state on every UI update (including starts/stops from another client),
  and track target width so network growth refreshes provider shape. A live
  zero-width Runner is authoritative and remains unavailable; stale config must
  not turn it into a false one-input target. Add a visible mapping target and
  distinguish provider-generated frame counts from managed admission. A saved
  audio path alone does not indicate that its provider is active after a source
  change; managed mic input is stopped before provider replacement. All nine
  `sensory_input_route_tests` pass, including stale-registry precedence, route
  eligibility, and zero/one-input handling. The
  low/high-tone 64-to-1 provider regression and the distributed single-neuron
  sensory-admission/shape-rejection test pass. `cargo check --locked
  --all-features --bin aarnn_rust --bin web_ui`, `cargo fmt --all -- --check`
  and `git diff --check` pass; existing warnings remain. A live `run_examples.sh`
  sensory probe was not performed in this pass, so live admission evidence
  remains open.

- [x] `2026-08-23 12:00Z` Implemented and tested the governed peripheral/effect
  reference contracts, independent channel state, bounded payload admission
  and per-device-epoch duplicate-sequence rejection in `src/peripheral.rs`;
  the two focused peripheral tests and Phase 8 reference channel case passed.
- [!] `2026-08-23 12:00Z` No maintained live browser/native USB AER adapter,
  bidirectional device negotiation/hot-plug path, production native I/O
  client, browser automation or physical-device evidence exists. The Android
  reference shell is tracked in the mobile plan and does not close these
  production gates.
- [!] `2026-08-23 12:00Z` Scientific transducer datasets/reports, authorised
  federation links, migration rehearsal, rollback evidence and legacy-path
  removal are absent. The `workstation_io` flag remains disabled and global
  HID remains separately unavailable.
- [!] `2026-08-23 12:00Z` The complete Section 21 definition-of-done gate is
  not met; it depends on Phases 1–7 production gates and external platform,
  hardware and scientific evidence.
- [x] `2026-08-23 12:07Z` Final cross-review verified the Android reference
  lane: Quail 3/API 34, NDK r27d and Gradle 9.1 built/package-tested both
  ABIs, Android JVM tests passed, and the APK launched on the configured
  emulator. This is bounded packaging and safe-unavailable UI evidence only;
  workstation I/O, AER, federation, scientific validation, migration and
  legacy-removal blockers remain explicit.
- [x] `2026-08-23 12:17Z` Final cross-review reran the Rust verification set,
  Android JVM tests, Rust-enabled APK packaging and emulator UI smoke test.
  The observed safe-unavailable capability report confirms that no live AER,
  media, discovery, federation or management adapter was accidentally enabled.
  Browser/native USB AER, physical-device lifecycle/thermal/USB evidence,
  federation, scientific validation, migration/rollback and legacy removal
  remain blockers; `workstation_io` and effectful/global-HID paths remain
  disabled.
- [x] `2026-08-23 13:35Z` Android remote validation reached the live AARNN
  ingress and proved the bounded read-only client receives an authentication
  denial from the emulator. It did not exercise live workspace activity,
  browser/native media, USB AER, federation or effectful output; an authorised
  runtime credential and all corresponding production adapters/evidence remain
  blockers. The debug-only cleartext lane is retained solely for this emulator
  validation and `workstation_io` remains disabled.
- [x] `2026-08-23 13:41Z` Moved the IPC readiness bind to the Rust entry path,
  before distributed node preloading and gRPC startup, and transferred the
  bound `IpcUdsServer` into `App`. `cargo fmt --all --check`,
  `cargo check --locked --all-features --all-targets` and
  `cargo build --release --bin aarnn_rust --all-features` passed. The real
  Webots launch showed `/home/pbisaacs/aarnn_rust.celegans_01.nn` immediately
  and reported the brain ready inside the 60-second socket deadline. The
  earlier timeout wording below is superseded by the later round-trip evidence.
- [x] `2026-08-23 13:41Z` Added a separate read-only Graph Explorer surface:
  Dashboard and Graph Explorer tabs, zero-width operational rail in graph
  mode, full-width topology canvas, wheel/pinch zoom, drag rotation,
  Ctrl/Command-drag pan and camera reset. The actual C. elegans snapshot was
  loaded by the Rust UI and captured as `logs/rust-ui-celegans-graph-live.png`
  (the later live-connected capture is recorded below); this is
  snapshot-backed visual evidence, not proof of live gRPC/IPC activity or
  biological adequacy.
- [x] `2026-08-23 14:55Z` Re-ran the Rust UI/Webots scenario with
  `NM_UI_AUTO_SELECT_DISTRIBUTED_VIEW=0`, `NM_UI_GRAPH_ONLY=1`,
  `NM_UI_CAPTURE_DELAY_FRAMES=120` and `NM_UI_CAPTURE_CLOSE=0`. The Rust UI
  saved `logs/rust-ui-celegans-graph-live-connected-final.png` with visible
  weighted edges, and the same run continued serving the controller: the
  Webots runtime recorded `Connected`, repeated `tx/rx` pairs and non-neutral
  output activity; the Rust log reached IPC frames 1, 100, 200 and 300.
  `NM_UI_CAPTURE_CLOSE=0` is opt-in; the default one-shot capture still closes
  its viewport.
- [!] `2026-08-23 14:55Z` The evidence above is a local Rust-runner/Webots
  round trip and a read-only graph capture, not production workstation I/O.
  Cluster-global remote snapshot RPCs still reset in the captured logs before
  weights arrive, and maintained browser/native USB-AER/media adapters,
  federation, physical-device evidence, scientific validation,
  migration/rollback rehearsal and legacy-path removal remain open. The
  `workstation_io` flag and effectful/global-HID paths remain disabled.
- [x] `2026-08-23 17:13Z` Cross-reviewed the Android Graph Explorer topology
  consumer. It renders exact bounded weighted edges returned by the authorised
  gateway and limits synthetic edges to the explicitly disconnected demo;
  connected sessions show no fabricated edges when topology data is absent.
  This is UI/reference evidence, not live browser/native USB-AER evidence.
- [!] `2026-08-23 17:13Z` Maintained browser/native USB-AER and concurrent
  media/HID adapters, physical-device evidence, federation, scientific
  validation, migration/rollback rehearsal and legacy-path removal remain
  blockers. `workstation_io` and effectful/global-HID paths remain disabled.
- [x] `2026-08-23 17:39Z` Final Rust verification passed sequentially, and
  `JAVA_HOME=/snap/android-studio/current/jbr
  ANDROID_HOME=/home/pbisaacs/Android/Sdk
  ANDROID_SDK_ROOT=/home/pbisaacs/Android/Sdk
  PATH=/snap/android-studio/current/jbr/bin:/home/pbisaacs/Android/Sdk/platform-tools:$PATH
  ./gradlew testDebugUnitTest assembleDebug --no-daemon` passed for the Android
  Graph Explorer package. The connected-session fallback audit confirms that
  only returned authoritative edges are drawn; synthetic edges are confined
  to the explicitly disconnected demonstration state.
- [!] `2026-08-23 17:39Z` Explicit workstation blockers remain: maintained
  browser/native USB-AER adapters, concurrent bounded A/V/HID paths, physical
  device and hot-plug evidence, federation, scientific validation,
  migration/rollback rehearsal and legacy-path removal. `workstation_io` and
  effectful/global-HID paths remain disabled; the Android screenshot is
  reference/offline evidence and not live authorised neural I/O.
- [x] `2026-08-30 18:41Z` Catalogued scenario manifests now record fixture
  references, target/capability requirements, device and resource bounds,
  reference profile, digest procedure and admission-loss policy. The xtask
  example runner rejects missing required manifest fields before invoking a
  test; `scripts/qa/run-examples.sh --all` passed for all five catalogued
  host-runnable scenarios.
- [!] `2026-08-30 18:41Z` The scenario harness does not convert host reference
  tests into physical USB/Lightning/MFi, browser automation, native media,
  scientific or signed mobile evidence. Those required lanes remain blocked
  and `workstation_io` remains disabled.
- [x] `2026-09-18 08:00Z` Completed milestone 8.5a's cross-product video
  preview slice. Rust UI providers now retain one bounded latest RGB frame and
  show it in a floating egui window; web, Android and the portable iOS
  SwiftUI surface expose `video-file` and `camera` with explicit preview
  lifecycle and pop-out controls; CLI `--video-file` and `--camera` select the
  same native UI sources. `cargo fmt --all --check`, the feature-gated Rust
  check, the bounded provider test, the web parity/browser-compatibility test,
  `node --check web_ui/app.js`, Android JVM tests and `git diff --check` pass.
  The iOS Xcode/signing gate and governed mobile AER admission remain open.
- [x] `2026-09-18 11:45Z` Tidied the I/O dashboard around an explicit source
  selection group and source-specific status/actions. Rust now enumerates all
  Nokhwa cameras, preserves numeric or backend string identities, disambiguates
  duplicate names, refreshes hot-plug state, stops capture when the selected
  device disappears, and starts the selected device instead of camera 0. Video
  containers with a decodable audio track are composed with the audio provider;
  camera microphone pairing remains a separate opt-in and enables Graphic EQ
  only after the audio provider starts. The web and Android surfaces now have
  matching camera/audio selectors, refresh actions and audio/EQ state; iOS
  enumerates camera devices and keeps audio permission separate. The focused
  provider tests, feature check, browser parity test, Node syntax check and
  Android JVM tests pass.
- [x] `2026-09-18 13:10Z` Closed the reported video-audio EQ regression. MP4/MOV
  audio probing now enables Symphonia ISO-MP4 support, selects only tracks with
  a registered audio decoder, and accepts AAC containers whose channel count is
  supplied by the decoder rather than the container metadata. Video startup
  through the CLI now initializes the same Graphic EQ and audio diagnostics as
  picker-based video selection. The permanent MP4/AAC fixture regression test,
  focused provider suite, feature build, web parity test, Node syntax check and
  diff validation pass.

- [x] `2026-09-25 10:47Z` Fixed the native webcam's missing MJPEG decode path
  reported by a blank-preview screenshot. `webcam_input` disables Nokhwa's
  defaults and previously enabled camera capture without its separate
  `decoding` feature; `AbsoluteHighestFrameRate` can negotiate MJPEG, whose
  decoder otherwise returns `NotImplementedError` and leaves the preview
  empty. Enabled `decoding` and added a small MJPEG fixture to the existing
  source-format test, which now covers MJPEG, NV12 and BGR. The focused test,
  locked native feature check, workspace formatting check and `git diff
  --check` pass. The screenshot does not identify the camera's actual FourCC,
  and no live stream was opened; physical preview confirmation remains open.

- [x] `2026-09-25 10:52Z` Added one-time webcam diagnostics for the first frame
  capture or conversion error. Conversion failures include Nokhwa's source
  format and decoder error, so a blank preview can be distinguished from an
  MJPEG/other-format decode failure without logging on every simulation tick.
  The locked MJPEG/NV12/BGR regression, locked feature check, formatting and
  diff checks pass. The actual physical camera mode remains unverified.
- [x] `2026-09-25 14:12Z` Traced the reported frozen webcam to synchronous
  `Camera::frame()` inside `WebcamCaptureProvider::next_spikes()`, which the
  simulation controller calls on its own thread (`src/providers.rs`,
  `src/ui.rs`). Camera construction and `open_stream()` also run in the egui
  handler. A stalled driver can therefore stop neural stepping, while the
  preview correctly retains its last frame with no freshness indicator. Moved
  camera open/read/decode and manual device refresh to background workers,
  added a bounded latest-sample handoff, 250 ms–5 s reconnect backoff,
  non-blocking cancellation, neutral sensory input after two seconds without a
  fresh frame, preview invalidation and explicit camera health status.
  `cargo metadata --no-deps --format-version 1` confirmed the root workspace
  (packages `aarnn_rust`, `aarnn-biox6-exporter`, `xtask`). The referenced
  specification requirements are Sections 16.17, 16.22 and 16.24; the gate
  remains incomplete and this targeted fix does not claim governed media I/O.
  `cargo fmt --all --check` and `git diff --check` pass. The focused feature
  `cargo check` is blocked by seven existing unresolved
  `Runner::log_gpu_cpu_fallback` calls in the already-dirty `src/runner.rs`;
  it reports no diagnostics in the webcam changes. The physical camera path
  remains unverified.
- [x] `2026-09-28 13:18Z` Fixed the native audio-file picker regression
  against the already-dirty checkout. `cargo metadata --format-version 1
  --no-deps` confirms the canonical workspace contains `aarnn_rust`,
  `aarnn-biox6-exporter` and `xtask`; the affected native path is
  `src/ui.rs`, with WAV decoding owned by `src/providers.rs`. The chooser
  had constructed the provider only when the selected view had a known,
  positive managed sensory width. This left the selected path empty when
  managed width was unavailable. Provider sizing is now independent from route
  eligibility: known valid managed widths remain preferred; otherwise a
  bounded local/configured width permits decode and path retention while the
  managed route remains fail-closed. Twelve `sensory_input_route_tests` and
  ten `providers::tests` pass. `cargo check --locked --all-features --bin
  aarnn_rust --bin web_ui`, `cargo fmt --all -- --check` and `git diff --check`
  pass with existing repository warnings. No interactive desktop picker run
  was available. Unrelated dirty visualization, morphology, mobile and Webots
  files were preserved.
- [x] `2026-09-28 14:21Z` Repaired the native managed-audio Start path. A
  contended orchestrator state lock no longer drops Start while reporting
  optimistic success: the control retries on a background task with a bounded
  timeout and the UI shows its pending state. The orchestrator publishes the
  selected sensory I/O bridge in additive heartbeat route metadata; frame
  payloads go directly to the node owning the configured sensory target layer
  (or the first active layer when no target is configured). That bridge alone
  reserves and acknowledges each frame, then existing direct peer routes carry
  neural activity through the cluster. Duplicate frame sequences with changed
  spike data and ambiguous bridge ownership fail closed. The lock-contention
  test, direct two-worker bridge test, bridge-selection tests and 27-test
  sensory suite pass. `cargo check --locked --all-features --bin aarnn_rust
  --bin web_ui`, `cargo fmt --all -- --check` and `git diff --check` pass with
  repository warnings. Live desktop/cluster Start verification is outstanding;
  the Phase 8 gate remains open.

## Validation and acceptance

- `UT-IOTIME-001`/`UT-IOSAMPLE-001`: capture mapping, uncertainty, dedupe/reorder/gap/coalescing and modality drop policies are exact and arrival-independent.
- `UT-AERUSB-001`: USB AER framing, sequence, address/polarity mapping, timestamp provenance, CRC/length validation and device-epoch transitions match golden fixtures.
- `UT-AERUSB-002`: the fair multiplexer preserves per-channel order/bounds; saturation or cancellation of USB AER, audio, video or HID cannot starve or retimestamp another channel.
- `UT-EFFECT-001`/`UT-HID-001`: effects apply at most once; disarm/crash/expiry releases all held state and rejects stale actuation.
- `CT-013`–`016`: gateway failure, workstation clock jump, client crash and USB removal/reconnect/endpoint stall produce bounded channel-scoped gaps, new mapping/device epochs, no duplicate effects and fail-safe release while unaffected modalities continue.
- `API-013`–`019`: separate/directional I/O grants, local-device binding/epoch authority, single actuator lease, reconnect dedupe and hostile media/USB/rate rejection pass end to end.
- `IO-E2E-001`–`014`: both clients pass capture timing, permission, motion/button, recorded replay, deadline, failover, isolation, provisional quality, browser safety, native watchdog, clock discontinuity, saturated-transducer, simultaneous A/V/HID/USB-AER and USB hot-plug/overflow scenarios.
- Section 21.9 federation tests cover time mapping, backpressure, source failure, revocation, dual authorisation, rejected zero-delay cycle and replay/dedupe.
- Section 21.5 repeats deterministic input replay, checkpoint/failover/migration and supported CPU/GPU kernels with equal committed digests/event sequence; fast profiles use declared tolerances.
- Section 21.14 documentation and Section 21.15 complete definition of done pass, including absence of legacy layer ownership/direct worker management.

## Rollout, compatibility and rollback

Enable modality capabilities independently by deployment/browser/OS profile. Start with recorded input and sandbox output, then live capture/A/V presentation; native/global HID remains off unless its separate hazard decision is approved. Rollback revokes sessions/leases, drains committed effects to a safe boundary and selects compatible protocol/transducer versions. Retain pre-migration immutable checkpoints throughout the declared rollback window. Delete legacy flags/code only after all active persisted states and deployments are migrated and rollback uses supported new-format recovery.

## Risks and mitigations

- Capture/arrival conflation changes biology under jitter. Pin clock mapping and test arrival perturbation.
- Codec/device callbacks can block or allocate without bound. Use real-time-safe rings/worklets/threads and bounded conversion pools.
- Effect replay can cause physical harm. Commit, dedupe, lease, allow-list, watchdog, neutral state and emergency stop are independent layers.
- Transducer determinism can be mistaken for biological adequacy. Publish reference data, units, errors, sensitivity and limitations.
- Federation can recreate a global barrier/cycle. Require positive delay and link-local progress; reject unapproved zero-delay cycles.
- Optimisation can alter ordering/numerics. Gate every change against the reference interpreter/digests or named tolerance profile.

## Surprises & Discoveries

The previous worktree summary said no Codex branches remained, but repository
inspection found `codex/webots-api-ingress-20260929` both locally and at
`origin`, checked out in a separate worktree. It contains placement-aware
Webots ingress, sparse-width bridge fixes and a snapshot reload optimisation.
Its static principal/network map lacked expiry, immediate revocation and local
consent, so it is excluded in favour of the existing brain-scoped policy and
short-lived session gate. The data path is being retained through the
authenticated HTTP gateway; hosted acceptance remains unverified.

The browser gateway has a separately configured persisted `Policy` and
already-defined brain-scoped `Capability::PeripheralInput`/`PeripheralOutput`,
but the AER handlers use only the web service-level `aarnn:use` requirement.
Those two authorization dimensions need separate checks. The present web
authentication middleware deliberately treats `AuthMode::None` as a local
development identity, so a peripheral guard must independently default-deny
that mode rather than treating the injected username as proof of consent.

Only governed peripheral/effect reference types and host tests are present.
Browser/native media and USB AER adapters, scientific fixtures, federation,
device timing and migration evidence are absent; layer-group paths remain
reachable for rollback.

The output raster and neuron brightness were reading different fresh-data
paths. Brightness used the asynchronous per-worker activity poll, but raster
columns depended only on the aggregate snapshot. The activity RPC already
contained recent output history; the dashboard had been ignoring it. When an
aggregate snapshot stalled, the equaliser and live neuron projection could
continue updating while the raster retained its previous columns. Reusing the
bounded worker history keeps the raster moving independently of snapshot
refresh and does not block the simulation or render loop.

The stage-three live trace isolated the missing neuron pixels to the egui
primitive choice. Its rectangle tessellator deliberately converts very thin
filled rectangles into feathered line segments; a one-device-pixel neuron can
therefore disappear despite correct positions, opaque activity colours, and
registered painter order. Direct coloured mesh quads preserve exact pixel
geometry and allow all markers to be submitted in one draw shape. The
acknowledged audio frame and synthetic connections were already available in
this reproduction, so this fix stays entirely in the display projection and
does not enter the neural traversal or sensory admission path.

The current screenshot's `Provider frames: 0` is consistent with an audio
provider waiting for a valid managed route, not evidence that the WAV decoder
failed: it displays a decoded 8 kHz mono file, while both checked-in startup
documents declare `num_sensory_neurons: 0`. Because users select files after
the dashboard opens, the launcher now prepares a private sensory I/O contract
on every run without starting playback. The orchestrator aligns the imported
snapshot matrices and runtime arrays to that contract before distribution, so
the distributed network and provider agree on sensory width without changing
the checked-in documents.

The initial live S=64 diagnosis was wrong: `run_examples.sh` already exported
`NM_DISTRIBUTED_AUTOSTART=1` to the orchestrator and workers. The all-features
build instead compiled out the fallback that applies this policy to a
preloaded compatibility Runner when no stable runtime manifest is present. Once
that was corrected, the live test exposed a second race. The workers preloaded
the repository's zero-width default config, began autonomous growth, and
reported the placeholder network as hosted before the orchestrator delivered
the run-local S=64 snapshot. The example workers now disable local preload and
wait for the authoritative `LoadNetwork` snapshot; the loaded network is then
playing with the expected width before route readiness is published. The live
run acknowledged its first 64-wide frame and reported sensory activity, while
five later prepare requests timed out under the loaded morphology workload.
Those timeouts leave uninterrupted full-file delivery unverified; preserve
worker-side acknowledgements and backpressure evidence rather than treating
peer presence or a ready dashboard as proof of input delivery.

The Rust webcam preview slice currently calls the blocking Nokhwa frame read
from the simulation controller thread. A disconnected or stalled driver can
therefore stall neural stepping as well as preview refresh. The egui camera
start action also performs synchronous camera initialization. This conflicts
with Sections 16.22 and 16.24; the fix isolates capture and reports loss of
fresh frames while keeping the simulation path non-blocking. A backend call
that never returns cannot be safely force-cancelled by Nokhwa, so recovery
from that specific driver fault is limited to isolating it from neural/UI
execution and signalling stale input until the call returns or the process
exits. Initial startup camera enumeration still occurs during application
construction; camera opening, streaming and user-triggered enumeration refresh
run on background workers. A physical webcam was not available for verification
in this session.

The audio-file status counter measures spikes emitted by the local sensory
provider before distributed ingress. Managed ingress independently requires
the frame width to match the selected brain. Because the workstation Runner
and selected managed brain can have different input widths, displaying local
provider activity did not prove that the selected brain received it. The UI
also did not enable the provider route when it opened directly on a managed
brain that was already playing. The route now uses the managed sensory width
and is activated on that view transition. It also reports bridge
acknowledgement separately from local provider output. The 22:04Z live launcher
evidence confirms that the first frame reached the managed network.

The follow-up route audit found a second activation race: `resolve_view_playing`
consulted the registry before the live managed Runner. A stale `false` is a
known value, so it prevented fallback to a live `true` and skipped the route
start on view selection. Remote starts/stops could also occur after initial
selection without reconfiguring the workstation route. The UI now prioritises
live state, reconciles the selected route continuously, and updates provider
width when the managed sensory layer grows. Remote-only mode has no local
managed ingress consumer, so it must report that input as unavailable rather
than claiming a route. A live zero-width Runner also cannot be replaced with a
one-input assumption from stale registry config; it must wait for an actual
managed sensory input to exist. A remembered audio filename likewise does not
prove that the active provider is still an audio-file source after switching
to another input; route readiness now follows the live provider state.

The latest file-picker report exposed an unintended coupling in that safety
fix: an unknown managed sensory width correctly disables distributed routing,
but the chooser also used that route-eligibility result to decide whether it
could decode and remember the audio file at all. Provider construction needs a
bounded positive local width; the managed route must continue to require an
authoritative positive target width.

The subsequent “Start did not change the input” report exposed a separate
distributed ingress gap. The UI marked the route active and emitted shaped
provider frames, but `DistributedNode::inject_external_sensory_spikes` first
required `NodeState.networks[network_id]` on the UI process. In orchestrator
mode that map is commonly empty: the live network is owned by workers and is
represented locally by `network_registry`. The existing fallback stream has
no per-frame acknowledgement and can overwrite a worker's pending single
sensory slot, so it is not a safe admission path for recorded audio.

The initial acknowledged-ingress change fanned frames from the orchestrator to
every assigned worker. The user clarified that each cluster master/I/O bridge
receives the stream directly and that the orchestrator supplies route
discovery/control only. The final route now resolves the configured sensory
target layer to one active owner, publishes that bridge identity in heartbeat
route metadata, and sends frame payloads directly to that peer. Cluster
activity then uses its existing direct inter-node spike routes.

The Start report also exposed a dropped-control race: `apply_network_control`
returns before enqueuing if the cluster-state lock is contended, while the UI
previously treated that result as accepted. The UI now retries away from the
render thread and waits for authoritative route reconciliation.

The live launcher exposed a distinct startup-readiness race. The native UI
started its selected audio provider as soon as the placement registry named an
owner, even though that worker had not yet applied its queued `LoadNetwork`
command. A connected peer and desired placement are not proof that the assigned
I/O bridge is ready; the UI now waits for the worker's hosted-network heartbeat
and keeps the bounded provider idle until route readiness. A follow-up run also
showed that preloading the worker's unrelated zero-width repository config
could make that heartbeat describe a placeholder network. The local example
therefore disables worker preload and uses the orchestrator's aligned snapshot
as the worker's first network. The live run now acknowledges its first input at
the configured width; later request timeouts remain under investigation.

The supplied cluster screenshot showed zero connected nodes and an empty
distribution for `cluster_master`. In that state the decoded file has no
assigned sensory bridge and cannot be admitted. A separate launcher run with
workers confirmed that this is a placement/readiness condition, not WAV
recognition: once the worker heartbeat reported the loaded network, direct
input was acknowledged and appeared in the cluster's sensory activity. While
waiting, the managed UI must keep the file provider's cursor still instead of
feeding its local preview Runner.

The follow-up launcher reproduction found that the missing worker records had
a separate operational cause: the local development CA and client/server
certificate were issued for one day, then cached indefinitely because the
helper checked only for non-empty files. The orchestrator and workers required
mutual TLS, so expired cached credentials turned successful process startup
into repeated gRPC transport errors. `/api/status` itself returned 503, while
`run_examples.sh` only checked process liveness and web asset readiness. The
local helper now renews expired or mismatched generated credentials, and the
launcher waits until both expected node endpoints appear in the authenticated
cluster status before claiming success. The retry evidence includes a healthy
two-node `cluster_master` distribution; this is developer-launcher TLS
recovery evidence and does not claim production credential lifecycle closure.

The next run showed that a loaded worker can still exceed the gRPC sensory
prepare/commit acknowledgement window while its Runner is busy. Tonic surfaces
the timeout as `Cancelled`/`Timeout expired`, rather than only as a deadline
error. Retrying that same frame identity preserves sequence and lets the
bounded producer queue apply backpressure without advancing the WAV provider.
The live run continued to show non-zero sensory activity while transient
timeouts were retried. A frame remained pending when the smoke run was stopped,
so this proves active ingress and neural observation but not completion of the
full recording under load.

The reported stage-three native view also had a caller/aggregate mismatch:
the orchestrator obtains a complete cluster cut whose authoritative shard IDs
are workers, yet the local UI decoder required the orchestrator's own node ID
to be one of those shards. That rejected projection kept the edge-cache worker
from receiving fresh cluster matrices. Separately, displaying the most recent
acknowledged input by merging it on every UI repaint turned a transient spike
into constant brightness. Presentation now accepts the complete orchestrator
projection and consumes each acknowledged sensory frame once, independent of
the provider's pending frame cursor.

The follow-up manual stage-three capture showed that edge projections could
still starve after snapshots began arriving. Every newer cut cleared the last
edge list and advanced its worker generation, while a projection result was
accepted only for the exact current tick. A small display worker could thus
finish against a cut that had already been superseded, leaving the UI without
any edges or connection counts. The current implementation retains the last
projection across monotonic cuts for the same assignment, accepts safely
lagging results, and refreshes if the cut advanced during computation. A
network/layer reassignment or time rewind still rejects that projection.

The first Webots ingress commits accepted external sensory gRPC calls without
a credential and protected the HTTP route only with general `aarnn:use`; the
branch's latest commit changes the gRPC path to fail closed without its
service bearer. The ingress is now being consolidated only behind the HTTP
route's exact `PeripheralInput` policy and active local session. Hosted Webots
acceptance and Phase 7 replicated session authority remain unverified.

## Decision Log

- `2026-09-30 / DEC-INV017-SESSION-GATE`: workstation sensory ingress requires
  both a deployment-provisioned peripheral grant scoped to the authenticated
  principal, requested brain, channel and direction and an ephemeral active
  session established by an explicit
  local action. The session has a bounded expiry, locally visible state and
  an owner-initiated immediate revoke; general `aarnn:use`, brain control,
  anonymous/development auth and discovery are never substitutes. Apply this
  first to the existing browser AER ingress and portable peripheral contract;
  keep production media/AER profiles disabled until Phase 7 authority and
  Phase 8 hardware/client gates pass. Authority: `INV-017`, Sections 16.5,
  16.16, 16.23–16.24 and 21.10/API-013–019.
- `2026-09-30 / DEC-SHARED-ROBOT-CLOCK`: use one shared monotonic wall-clock
  reference across the robot fleet for pacing, deadline and computation/
  latency measurements. A network with lower neuron count may complete its
  cycle earlier and proceed independently; unrelated networks do not wait
  for the slowest network. Each brain retains its own logical tags, and only
  its declared versioned mapping translates between wall-clock capture/deadline
  time and that brain's eligible tags. Wall-clock never decides causal order.
  Authority: user's multi-robot timing clarification, Section 3.4, Sections
  12.1–12.3, and `INV-002`/`INV-010`.

- `2026-09-30 / DEC-0085U` (superseded by DEC-0085V): keep the candidate
  Webots ingress unmerged until
  external sensory injection enforces a dedicated, scoped and revocable
  peripheral-input grant. An optional shared bearer plus general `aarnn:use`
  does not satisfy Section 16.5 or `INV-017`; hosted Webots acceptance is also
  unverified. The unrelated 64K HWE image-manifest fix may be consolidated
  independently. Authority: Section 16.5 and `INV-017`.
- `2026-09-30 / DEC-0085V`: preserve the Webots placement-aware sparse ingress
  only behind the authenticated web gateway's existing brain-scoped
  `Capability::PeripheralInput` and active `PeripheralAuthorizationSession`;
  remove the branch's parallel static grant map. The internal orchestrator
  RPC still requires its service bearer. This reference path does not claim
  hosted Webots or production replicated-session acceptance. Authority:
  explicit INV-017 implementation authorisation, Sections 16.5, 16.16–16.18,
  and API-013–019.

- `2026-09-29 / DEC-0085R`: populate the cluster output raster from the same
  bounded worker activity responses that drive cluster neuron brightness.
  Merge sparse outputs by simulation step, retain zero-spike steps so the
  raster scrolls while outputs are silent, and reject histories older than the
  displayed raster cursor. An actual step rewind clears the display trace.
  Aggregate snapshots remain a valid source and dedupe by the same cursor.
  Authority: Sections 16.22 and 21.12; presentation-only, with no neural-state
  or event-admission effect.
- `2026-09-29 / DEC-0085S`: render stage-one-to-seven single-pixel neurons as
  framebuffer-aligned coloured mesh quads, batched per frame. Egui's thin
  rectangle simplification may rasterise the equivalent `rect_filled` marker
  as an invisible feathered line. Keep marker intensity in opaque RGB and
  place the mesh after graph strokes so activity remains visible. This is a
  presentation-only decision; neural state, admitted stimuli and logical time
  are unaffected. Authority: Sections 16.22 and 21.12.
- Initial decision: capture/device time plus a versioned mapping determines biological eligibility; USB completion or network arrival time is used only when the device lacks a clock and its uncertainty is recorded. Authority: Sections 3.4 and 16.18.
- Initial decision: USB AER is a separately sequenced bidirectional peripheral modality that may run concurrently with A/V/HID; it is not an internal shard transport. Authority: Sections 16.15–16.20.
- Initial decision: browser input is focused/consented and browser global HID output is unavailable. Authority: Sections 16.21 and 16.23.
- Initial decision: native/global HID is optional and remains independently safety-gated after general workstation I/O completion. Authority: Sections 16.15, 16.19 and 16.23.
- Initial decision: federation links use positive minimum delay unless a separately approved component design proves otherwise. Authority: Sections 12.2–12.3.
- `2026-09-28 / DEC-0085H`: route sensory frames directly to one cluster I/O
  bridge: the owner of the configured sensory target layer, or the first
  active layer when no target is configured. The orchestrator publishes that
  route identity but does not proxy or fan out frame payloads. The bridge uses
  bounded prepare/commit/abort admission with per-frame acknowledgement and
  waits for the previous slot to be consumed. Reject ambiguous/missing bridge
  ownership; do not treat local provider frame counts as managed admission.
  The cluster's ordinary direct peer transport moves subsequent neural
  activity. This remains a compatibility-path fix rather than the complete
  governed peripheral session/data-plane implementation. Authority: Sections
  16.17, 16.18, 16.20, 16.22 and 16.24, plus `INV-007` and `INV-015`.
- `2026-09-18 / DEC-0085A`: use one canonical source vocabulary (`video-file`
  and `camera`) across Rust UI, web, Android, iOS and CLI. Preview pixels are
  latest-frame UI state only and never become biological timestamps or causal
  events. Authority: Sections 16.17–16.22 and `INV-002`, `INV-015`, `INV-017`.
- `2026-09-18 / DEC-0085B`: camera identity is the backend-provided Nokhwa
  index string, not a display name or an assumed numeric slot. Camera and
  microphone permissions remain independent. A video file's audio track, or an
  explicitly selected microphone companion for a camera, is composed at the
  sensory boundary and is the only condition that turns on Graphic EQ for that
  video session. Authority: Sections 16.17, 16.21 and `INV-017`.
- `2026-09-25 / DEC-0085C`: perform camera open/read/decode on a dedicated
  worker. The simulation controller only tries to consume the latest bounded
  visual sample and emits neutral input after the capture becomes stale;
  preview pixels remain separate display state. Returned capture errors retry
  with bounded backoff, and stop never joins a potentially blocked driver
  call. Authority: Sections 16.17, 16.22 and 16.24. This is a legacy local UI
  adapter fix, not the complete governed peripheral pipeline.
- `2026-09-28 / DEC-0085D`: size a workstation sensory provider from the
  selected managed brain's live sensory width, with its registry configuration
  as the fallback; never resize the managed brain to fit the workstation
  provider. Apply the provider resize and route target in one ordered
  simulation-control message, activate an already-playing brain when its view
  is selected, and fail closed while width is unknown. Authority: Sections
  16.16, 16.17, 16.24 and `INV-007`.
- `2026-09-28 / DEC-0085E`: route activation follows the freshest managed
  playing state, reconciling changes from both this UI and other clients. A
  route is active only when a local distributed ingress is available, the
  selected managed brain is playing, and an audio-file/microphone provider is
  ready. A remembered file path is insufficient after provider replacement.
  Track the routed sensory width so managed growth resizes subsequent frames;
  a live zero-width Runner is unavailable and never coerced to one. Label
  provider output separately from the managed mapping target. Authority:
  Sections 16.17 and 16.24 and the Phase 8 workstation boundary.
- `2026-09-28 / DEC-0085F`: selecting and decoding an audio file is independent
  of managed-route eligibility. If a managed target width is unavailable,
  construct the local provider at a bounded local/configured width, retain the
  selected path and keep managed ingress disabled until an authoritative
  positive width exists. Never resize the managed Runner to fit the local
  provider. Authority: Sections 16.17 and 16.22 and `INV-007`.
- `2026-09-28 / DEC-0085G`: a contended orchestrator state lock must not cause
  the UI to claim a Start/Stop command was accepted or to activate sensory
  routing from an optimistic value. Retry the short enqueue off the render
  thread with a bounded deadline, report pending/failure state, and let the
  accepted registry/live Runner state drive route reconciliation. Authority:
  Sections 16.17 and 16.22 and `INV-007`.
- `2026-09-28 / DEC-0085I`: managed sensory input readiness requires evidence
  that the placement-selected bridge has loaded the network. A connected peer
  and assigned layer describe desired placement, not admission readiness. The
  orchestrator exposes the bridge to clients only after its heartbeat reports
  the network, rejects ingress before that evidence exists, and lets the
  workstation retain the bounded selected provider until route reconciliation
  can safely activate it. Authority: Sections 16.16, 16.17 and 16.24 and
  `INV-007`, `INV-015`.
- `2026-09-28 / DEC-0085J`: a selected managed view owns advancement of its
  sensory provider. While that network is paused or its direct bridge is
  loading/unavailable, keep the bounded audio provider intact and idle the
  workstation's unrelated local Runner; resume provider advancement only
  after route reconciliation activates the placement-selected bridge. Preserve
  an explicit wait reason and log route activation/acknowledgement without
  routing media through the orchestrator. Authority: Sections 16.17, 16.22
  and 16.24, plus `INV-007` and `INV-015`.
- `2026-09-28 / DEC-0085K`: generated local launcher credentials are reused
  only while their CA and leaf certificates remain valid, the leaf verifies
  under that CA, and both certificate/private-key pairs match. Stale local
  material is rotated; explicitly supplied deployment credentials remain
  caller-managed. A local example reports ready only after both worker
  endpoints appear in orchestrator status. This is launcher reliability
  behaviour and does not relax mutual-TLS identity requirements. Authority:
  local management profile and Phase 8 workstation testability boundary.
- `2026-09-28 / DEC-0085L`: the local example may seed an otherwise zero-width
  network with a bounded default sensory population so a user can select an
  input after startup. This changes only the private run-local startup
  snapshot, leaves playback opt-in, preserves any already-positive sensory
  width, and uses the canonical Runner resize path with deterministic
  initialisation. Keep the UI route fail-closed until the selected cluster
  brain and its loaded sensory bridge are ready. Authority: Sections 16.16,
  16.17 and 16.24; `INV-007` and `INV-015`.
- `2026-09-28 / DEC-0085M`: the local example exports the same autostart
  policy to its orchestrator and worker processes. Registry `playing` state
  alone does not activate a worker's local Runner; direct sensory admission
  remains gated by the worker's own loaded/playing state and per-frame
  acknowledgement. Authority: Sections 16.17 and 16.24 and `INV-007`.
- `2026-09-28 / DEC-0085N`: apply distributed autostart independently of
  whether `stable_executor_live` is compiled; a registered stable runtime may
  start, otherwise the compatibility Runner follows the explicit distributed
  autostart policy. Example workers that do not share the orchestrator's
  run-local snapshot must disable local preload and wait for `LoadNetwork`
  before advertising the sensory bridge as ready. The bridge remains the
  direct payload recipient and validates the actual sensory width. Authority:
  Sections 16.16, 16.17 and 16.24 and `INV-007`, `INV-015`.

- `2026-09-28 / DEC-0085O`: when a managed sensory frame is not acknowledged
  because the bridge is busy or a gRPC acknowledgement times out, retry the
  same frame/session identity for a bounded 120-second window. The existing
  bounded provider queue then backpressures source advancement. A permanent
  route or shape error stops that session instead of forwarding later frames
  across an unrecorded gap. Authority: Sections 16.17 and 16.24 and
  `INV-007`, `INV-015`.
- `2026-09-29 / DEC-0085P`: the local native orchestrator decodes its complete
  aggregate cluster snapshot without requiring its control-plane identity to
  own a neural shard; worker snapshot selection remains strict. For the
  display-only sensory trace, retain the acknowledged frame's brain, session
  and sequence identity and merge it once. Do not replay an old frame on each
  repaint or use a not-yet-acknowledged provider frame as evidence of neural
  activity. Authority: Phase 8 visualisation isolation boundary and
  `INV-007`, `INV-015`.
- `2026-09-29 / DEC-0085Q`: synthetic connection projection is presentation
  state, so the last valid edge list may remain visible while a worker builds
  edges from a newer cut of the same network assignment. Accept a completed
  projection only for the selected network, unchanged assignment and cache
  generation, with a cut no newer than the current snapshot. Reassignment or
  time rewind invalidates it. This keeps visualisation non-blocking without
  changing neural traversal, admission or commitment. Authority: Phase 8
  visualisation isolation boundary and `INV-007`.

## Outcomes & Retrospective

The governed reference contracts and host checks pass. Browser/native I/O,
USB-AER, federation, scientific validation, migration/rollback and legacy
removal evidence remain open, so the final definition-of-done gate is not
claimed.

The video preview slice is complete within those boundaries: preview pixels
are display state only, source changes close stale previews, and each shipped
surface has the same `video-file`/`camera` vocabulary and source-ready pop-out
state. Mobile remains a preview/reference shell until its signed packaging and
governed admission integrations are delivered.

The native audio route now follows the live managed playing state and sensory
width, distinguishes an active file provider from a remembered path, and reports
its mapping target separately from provider output. Focused regression coverage
passes. Live cluster admission and durable peripheral semantics remain open, so
the Phase 8 workstation-I/O gate is not claimed.

The audio chooser can now decode and retain a selected file when managed input
width is not yet available, using only a bounded local provider width. The
existing zero-width managed-route guard remains covered and passing. The native
dialog itself still needs a manual workstation run for end-to-end confirmation.

The Start control now survives brief state-lock contention without blocking UI
rendering or silently losing the command. The orchestrator supplies bridge
discovery metadata, while audio frames travel directly to the sensory-target
owner and receive per-frame acknowledgements. The 2026-09-28 `run_examples.sh`
run confirms worker-readiness wait, direct route activation, acknowledgement
of the first S=64 managed audio frame, and non-zero sensory activity. Five
later frame preparations timed out under the CPU/morphology workload, so
uninterrupted full-file delivery remains unverified. The Phase 8 gate remains
open for that throughput/recovery evidence plus governed production peripheral
sessions, USB-AER, federation, scientific validation, migration/rollback and
legacy removal evidence.

The example launcher now also detects expired cached developer mTLS
certificates and waits for both expected workers to join before announcing
cluster readiness. The live status and orchestrator telemetry confirm that
both launched nodes appear in the cluster. This developer profile does not
replace production certificate rotation, identity enrolment or revocation
evidence.

The managed input sender now retries a transiently unacknowledged frame and
lets its bounded upstream queue apply backpressure. The required launcher
confirmed that the selected WAV reached the sensory-target worker and
produced non-zero cluster sensory activity. Full-file completion at this
worker load remains an open validation item; the Phase 8 workstation-I/O gate
is not claimed.
