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

### Milestone 8.6 — Federation and multi-workstation load

Implement positive-delay authorised links, time mapping, revocation, backpressure and replay/dedupe. Run two workstations/two brains plus the four-brain fleet; verify isolation and explicit dependency/failure behaviour.

### Milestone 8.7 — Evidence-led optimisation

Profile causal critical paths, allocator/state layout, queues, batching, GPU transfers/kernels, checkpoint cadence and scheduler predictions. Apply only changes that preserve reference digests or documented fast-profile tolerances; retain benchmark and scientific evidence.

### Milestone 8.8 — Migration, legacy removal and final gate

Provide persisted-state/config/deployment migrations, rolling-upgrade and rollback rehearsal. Close all deferred tests, publish project/architecture/protocol/security/scientific/runbook documentation, remove layer-group fallback and temporary flags, and prove no old direct-worker/layer-broadcast path is reachable.

## Progress

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

Only governed peripheral/effect reference types and host tests are present.
Browser/native media and USB AER adapters, scientific fixtures, federation,
device timing and migration evidence are absent; layer-group paths remain
reachable for rollback.

## Decision Log

- Initial decision: capture/device time plus a versioned mapping determines biological eligibility; USB completion or network arrival time is used only when the device lacks a clock and its uncertainty is recorded. Authority: Sections 3.4 and 16.18.
- Initial decision: USB AER is a separately sequenced bidirectional peripheral modality that may run concurrently with A/V/HID; it is not an internal shard transport. Authority: Sections 16.15–16.20.
- Initial decision: browser input is focused/consented and browser global HID output is unavailable. Authority: Sections 16.21 and 16.23.
- Initial decision: native/global HID is optional and remains independently safety-gated after general workstation I/O completion. Authority: Sections 16.15, 16.19 and 16.23.
- Initial decision: federation links use positive minimum delay unless a separately approved component design proves otherwise. Authority: Sections 12.2–12.3.

## Outcomes & Retrospective

The governed reference contracts and host checks pass. Browser/native I/O,
USB-AER, federation, scientific validation, migration/rollback and legacy
removal evidence remain open, so the final definition-of-done gate is not
claimed.
