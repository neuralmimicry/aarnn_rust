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
- Implement a separate server-managed virtual-simulation ingress for allow-listed workload identities. Reuse exact brain-scoped input grants; do not bypass or weaken workstation PeripheralSession/local-consent checks.
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
library/web-UI tests and integration tests; architecture-matched GitHub-hosted
X64 and ARM64 runners execute the build and focused runner suite. Android is a Gradle
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

### Milestone 8.1.1 — Persistent virtual-simulation ingress

Add a distinct authenticated server route for virtual-world sensor frames so a
managed Webots service may continue while browser viewers disconnect. Require
an allow-listed service-account identity, `aarnn:use`, an exact brain-scoped
`PeripheralInput` grant, a stable producer session ID and the bounded
placement-aware sensory admission path. Keep `/api/aer/inject` workstation
sessions unchanged. The route cannot represent local device consent or permit
physical/global actuation. Verify denials for auth mode `none`, user identities,
unlisted services and out-of-scope brains, then verify service/grant revocation
blocks later frames. Production rollout also requires the service allow-list
and network grants to be applied together from the simulation_environment
Ansible profile.

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

- [x] `2026-10-02 18:44Z` A second isolated replay used exactly one hexapod, three local workers, the rebuilt all-feature release binary, a fresh workspace under `target/qa/hexapod-raster-owner-20261002/runtime`, and logs in the sibling `logs` directory. All workers registered and Webots connected. The native UI logged the actual output owner (`hexapod_01_worker_02`, layer 5) at steps 9–10 with 18 historical startup spikes, then the input owner (`hexapod_01_ipc`) at step 41 with 475 historical sensory spikes. The IPC owner consumed sensory frames through sequence 39. The output owner's autosaved snapshot at step 320 (`layer_range=[4,6]`) has zero layer-4/layer-5 spikes, zero output spikes, resting output voltages near -70 mV and only 0.0009–0.0031 output current; the neighbouring worker's step-330 temporary snapshot also has zero layer-2/layer-3 spikes. The controller's 26 sampled replies had one initial 18/18 active reply and 25 neutral 0/18 replies. Burst spike forwarding logged 120 ms timeouts and switched to persistent streams; whether these caused the sustained downstream silence remains unproven. Webots later exited with status zero and the launcher stopped its children; no OOM or neural-runtime panic was observed. `cargo build --locked --release --all-features --bin aarnn_rust -j2`, both focused native owner-filter tests, and `git diff --check` passed. The UI now accepts raster data only from the corresponding assigned I/O owner and labels absent/stale owners independently. This proves why the output panel is blank after startup but does not satisfy committed-output-to-Webots actuation or scientifically calibrate the hexapod circuit.

- [~] `2026-10-02 18:22Z` The isolated one-hexapod replay ended after an intentional SIGTERM to the test launcher; its temporary X11 display closed during native UI shutdown and winit then panicked, rather than a neural-runtime crash. The pixel capture shows 3,562 input-raster spikes, confirming the separate 128-step display history; the controller nevertheless reported neutral 18-channel replies and repeated lock-step waits. A successful cluster activity poll reported only one of the assigned worker samples, so the empty output raster cannot yet distinguish a quiet output owner from an absent output-owner RPC. The native UI will retain source-specific poll evidence and label each raster accordingly; output spike production and managed-output-to-Webots actuation remain separate Phase 8 acceptance work. The fresh workspace's committed snapshot has `t=280`, sensory spikes in its two-step biological ring, and zero local output current/spikes, but its `[0,1]` layer range means it is not authoritative evidence for the layer-5 output owner. Preserve the one-robot test limit and existing dirty work.

- [~] `2026-10-02 18:00Z` The hexapod raster root cause is now pinned to its saved `hist_len=2` biological ring. `src/distributed.rs` records a separate 128-step sparse sensory/output display ring after successful managed steps, and `GetNetworkActivity` uses it without expanding biological/GPU delay buffers. `src/ui.rs` preserves raster history across layout-only refreshes, labels a missing/stale worker feed, backs failed activity polls off for five seconds and refreshes stable full topology snapshots every 15 seconds. Local `run_webot.sh` listeners now bind to loopback; remote-compute listeners remain network-facing. `cargo check --locked --all-features --bin aarnn_rust -j2` passed before the final timeout/snapshot tuning; the sparse-ring regression, existing two activity-RPC tests and the new RPC regression all passed with `cargo test --locked --all-features --lib ... -j2`. `bash -n run_webot.sh` and `git diff --check` passed. The final release rebuild and isolated one-hexapod replay remain in progress; neither output spikes nor actual hexapod movement is established by these tests.

- [~] `2026-10-02 17:39Z` Diagnosing the live one-hexapod run without interrupting it. `ps` confirms the launcher, orchestrator, three local workers and Webots remain active. `logs/webots_hexapod_01_worker_01.log` proves managed sensory consumption through frame 1495; `logs/webots_hexapod_01.log:100494-100496` shows frame 1496 failed after repeated gRPC prepare resets, and Webots' controller reports neutral 18-channel replies and a stopped lock-step clock. `ss` and orchestrator inventory confirm six unrelated LAN workers joined the local `0.0.0.0:50051` listener, causing nine-node/four-network placement churn. `src/ui.rs` reads 128-step worker activity histories on a bounded poll, but layout changes call `refresh_ui_buffers` and erase both rasters; the empty labels also fail to distinguish quiet activity from a stale or unavailable poll. Next: confine the local launcher listener, preserve same-view raster history through layout-only changes, show stale transport state, and run focused non-Webots checks before an isolated one-robot replay when memory permits.

- [x] `2026-10-02 16:10Z` The final single-hexapod historical replay used `scripts/run_sim.sh --sim webots --robots hexapod=1 --node 3 --no-build --world target/qa/hexapod-startup/world-retry.wbt --webots-headless` with isolated `WEBOTS_RUNTIME_ROOT=target/qa/hexapod-startup/runtime-retry`, `LOG_DIR=target/qa/hexapod-startup/logs-retry`, explicit workspace resume and the matching all-features binary. The launcher selected 360/720/780-second worker/distribution/controller deadlines without timeout flags; all three workers read the same pinned 12,204,451-byte snapshot and registered. `webots_runtime.log` records `Brain 'hexapod_01': Connected.`; the IPC owner acknowledged managed frame 0 and its managed Runner consumed exactly sequences 0–64, with no gap or IPC rejection. Four bounded backpressure records occurred during slow steps, without advancing past a failed frame. Clean Ctrl-C shutdown left no runtime or Webots processes. The controller's 28 sampled diagnostics still show 0/18 non-neutral output channels, so this run proves startup and managed sensory continuity, not managed-output-to-actuator equivalence or robot motion. `cargo test --locked --all-features --lib sensory` passed 41 tests, the all-features release binary rebuilt, shell syntax and `git diff --check` passed. The remaining output binding is a separate Phase 8 acceptance item; do not infer motion from these logs.

- [~] `2026-10-02 15:53Z` The rebuilt one-hexapod, three-worker historical replay connected Webots and acknowledged managed sensory frame 0; `hexapod_01_worker_02` logged consumed sequences 0–20 with 16–30 input spikes. Under repeated 8-second gRPC prepare timeouts, frame 21 then failed commit with `sensory frame was not prepared` and the IPC owner deliberately stopped replies. No worker reload was logged after initial placement. The current sender issues a remote `AbortSensoryInput` after an ambiguous prepare failure, but that abort RPC itself has an 8-second timeout and can arrive after a later retry has reserved the same frame identity. Remove the unsafe remote abort on ambiguous prepare failure: a single remote owner holds a bounded 5-second reservation, same-identity retries are idempotent, and the receiver expires an abandoned reservation before a later frame. Keep fail-closed handling for any ambiguous commit; do not fabricate an acknowledgement or skip a sequence. The controller's sampled output remained neutral, which is a separate local-runner/managed-output parity limit. Record and test the retry behaviour without loading more than one robot at once.

- [~] `2026-10-02 15:38Z` The pinned, parallel one-hexapod historical-workspace replay loaded the same 12,204,451-byte startup snapshot in all three workers and all registered. Webots sent its first 34-channel frame, but the IPC owner repeatedly rejected its managed route to `hexapod_01_worker_01`: its local `expected_load_fingerprint` and `reported_load_fingerprint` were both `None`. Only the orchestrator stores the expected fingerprint and already gates publication of `sensory_ingress_node_id` on the worker's matching load heartbeat. The IPC owner re-runs that orchestrator-only check on its empty fingerprint map, so a longer startup timeout alone cannot complete the controller handshake. Fix the worker-origin path to use only the orchestrator-published bridge ID and connected peer, then require the selected bridge's prepare/commit mailbox to validate playing state, ownership and width. Keep the orchestrator fingerprint gate and fail-closed remote validation. Add a worker-to-worker routed ingress regression and rerun the one-robot historical startup; no network-state lock may span a remote RPC or retry delay.

- [~] `2026-10-02 15:09Z` Reproduced the hexapod timeout with a copied historical workspace: `data/runtime/users/webots/workspaces/webots-hexapod-01/latest.snapshot.json` is 12,204,451 bytes, and a copy under `target/qa/hexapod-startup/runtime-historical/` was resumed with one hexapod and three worker processes. At 60 seconds both the IPC owner and first extra worker were alive and still initialising. With explicit `--connect-timeout 300 --cluster-distribution-timeout 900`, they logged `distributed node ready` after 185.341 and 184.646 seconds and then joined the orchestrator. The second extra worker is still initialising. A fresh 4,057,528-byte workspace with the matching all-features release binary joined in about 51 seconds per worker and reached a confirmed Webots controller connection. The first isolated attempt reused a stale plaintext-profile release binary while selecting all-features TLS credentials; its `connect: transport error` was a test-profile mismatch, not evidence for this timeout. A fresh `cargo build --locked --release --all-features --bin aarnn_rust` passed before the successful measurements. Next: derive a bounded snapshot-sensitive automatic budget, retain explicit override precedence, then run the historical one-robot scenario without explicit timeouts.

- [~] `2026-10-02 14:54Z` Investigating the one-hexapod, three-worker startup timeout. `scripts/run_sim.sh` forwards to `scripts/run_multi_robot_webots.sh`, whose automatic `--connect-timeout` is based only on the largest saved snapshot; the 4 MB hexapod workspace selects 60 seconds. `run_webot.sh` waits for each extra worker to log `Successfully joined orchestrator` using that same budget. The user's first-worker log stopped after CUDA initialization with no registration or fatal error. The canonical launcher files are `scripts/run_sim.sh`, `scripts/run_multi_robot_webots.sh`, and `run_webot.sh`; root `Cargo.toml` is the runtime manifest and `cargo metadata --no-deps --format-version 1` confirms the three-package workspace. An isolated `--no-build`, all-features, one-robot measurement is running with 300-second connect and 900-second distribution limits under `target/qa/hexapod-startup/`. Preserve the extensive pre-existing dirty workspace, including both launcher/plan files; measure registration, then change and test only the automatic budget if startup completes.

- [x] `2026-10-02 14:17Z` A single-worm, three-worker run in an isolated runtime and Xephyr display confirmed live native input-raster pixels: `target/qa/webots-input-raster-visible/ui-raised.png` shows `celegans_01` with 599 buffered input spikes and an active output raster, while `target/qa/webots-input-raster-visible/logs/webots_celegans_01.log` records acknowledged managed frame 0 and subsequent 13–19-spike frames. The launch used `scripts/run_sim.sh --sim webots --robots celegans=1 --node 3 --no-build --world target/qa/webots-input-raster-visible/multi_neuroworld.wbt --webots-headless` with `NM_WEBOTS_RUNTIME_FEATURES=all-features`, `WEBOTS_WORKSPACE_RESUME_EXISTING=0`, isolated `WEBOTS_RUNTIME_ROOT`/`LOG_DIR`, and `DISPLAY=:8`; clean Ctrl-C shutdown completed. The user screenshot/default logs predated the 13:59Z sensory-admission build. A separate one-worm contact probe found the worm's front/rear bumpers had no collision shapes and the initial habitat placed contact props against both tips. `scripts/build_webots_celegans_assets.py` now gives each bumper a 0.022 × 0.010 × 0.026 m collision face above the agar; the canonical `CelegansRobot.proto` was regenerated, and `scripts/build_webots_multi_world.py` places the rear barrier at -0.26 m and center taste sphere at +0.265 m from spawn. `env DISPLAY=:8 WAYLAND_DISPLAY= XDG_SESSION_TYPE=x11 python3 scripts/qa/probe_celegans_contact_sensitivity.py` passed in the normal generated habitat: 20 baseline frames had neither bumper high; 29 front-contact frames had front=1/rear=0; 29 rear-contact frames had rear=1/front=0; both released after the object moved away (`target/qa/celegans-contact-sensitivity/report.json`). The existing C. elegans encoder maps a value of 1 to a spike with probability 1. `python3 scripts/qa/probe_webots_neural_actuators.py --kind celegans` passed 90 frame/reply cycles and 23 moving spine motors, with no always-high sensory channels; `python3 scripts/qa/probe_simulator_webots.py --kind celegans`, the two focused simulator-content generator tests, Python compile, and `git diff --check` passed. Other robots were not loaded concurrently; the dirty primary mixed world was preserved. The bumper test validates this Webots proxy in metres and binary contact, not biological mechanoreceptor calibration or the Phase 8 security/committed-effect gate.

- [x] `2026-10-02 13:59Z` One-worm managed sensory admission repaired: `scripts/run_sim.sh --sim webots --robots "celegans=1" --node 3` launched `celegans_01_ipc` with a UDS-driven local `App` Runner and a distinct managed `DistributedNode` Runner. The original `logs/webots_runtime.log` had non-neutral 24-channel sensory values and matched 96-channel replies, but `logs/webots_celegans_01.log` had no managed `consumed AER sensory frame` record. `src/ui.rs` now sends each decoded IPC spike frame through the existing bounded managed sensory channel and waits for its admission acknowledgement before the local motor reply. IPC startup retries an unpublished bridge, incomplete owner load or paused managed brain for the existing 120-second bound; invalid width remains a permanent error. The first live attempt exposed that bridge-publication race. The isolated rerun used one worm, three workers, `WEBOTS_RUNTIME_ROOT=target/qa/webots-input-raster-live/runtime`, `WEBOTS_WORKSPACE_RESUME_EXISTING=0`, and matching `NM_WEBOTS_RUNTIME_FEATURES=all-features` for the rebuilt binary. `target/qa/webots-input-raster-live/logs-2/webots_celegans_01.log` records the first managed acknowledgement and 92 consumed frames, sequences 0–91 with 8–19 spikes each; `webots_runtime.log` retained non-neutral output replies. The apparent frame-93 `channel closed` error happened after test SIGINT shutdown, when the runtime closed the bridge. `cargo check --locked --all-features --bin aarnn_rust`, the all-feature release build, focused startup-retry, activity-RPC, raster-merge and six-profile UDS encoder tests, and `git diff --check` passed. The screenshot API failed in this Wayland session, so live pixel-level input-raster acceptance remains unobserved. Authority: user screenshot/report; Sections 16.16–16.19 and 21.13; `INV-007`, `INV-015`–`INV-017`.

- [x] `2026-10-02 14:10Z` A visible native UI now confirms the managed sensory owner history paints the input raster in a one-worm three-worker run. Contact-specific sensing during an object push remains under the new bumper-geometry probe above. This is still a compatibility deployment with a second local Runner providing Webots motor replies, so output-raster-to-robot equivalence and committed-effect fencing remain Phase 8 acceptance work, not a claim made by this sensory fix.

- [x] `2026-10-02 13:28Z` Latest one-worm Webots run raster audit: `logs/webots_runtime.log` shows the `celegans_01` controller repeatedly exchanging 24-channel sensory and 96-channel output frames with non-neutral output; `logs/webots_celegans_01.log` confirms the live UDS owner received frame #500 at S=24. The native UI screenshot and `logs/webots_orchestrator.log` instead show its view pinned to `tenant-aarnn`, whose projected snapshot has zero output connections. The generic orchestrator UI selected that larger pre-existing network (~1,650 neurons) over the launched 398-neuron worm. `run_webot.sh` now passes the first requested brain as `NM_UI_PREFERRED_NETWORK_ID`, and `src/ui.rs` waits for that inventory entry in the initial cluster/remote view while preserving later manual selection. `cargo test --locked --all-features --lib launched_brain_wins_initial_cluster_view_over_larger_existing_network --quiet` and `worker_output_histories_advance_raster_and_merge_shared_steps` passed; `bash -n run_webot.sh` and `git diff --check` passed. `cargo fmt --all -- --check` still reports formatting differences in the existing dirty `src/bin/web_ui.rs` and other pre-existing `src/ui.rs` raster edits; this UI-selection change does not reformat that work. No new full Webots GUI run was made after the UI selection change, so visual acceptance remains for the next launch. This is display selection only; committed neural activity and control-plane ownership are unchanged. Authority: user report; Sections 16.16, 16.19 and 21.13.

- [x] `2026-10-02 13:12Z` Webots habitat and physical-motion pass: `scripts/build_webots_multi_world.py` now emits robot-local ecology beside the shared habitats, uses ordinary 9.81 m/s² gravity and an 8 ms fly step, and aligns the single-fish named Fluid with the visible aquarium. The fly controller applies bounded world-vertical heuristic lift only under fresh neural wing drive; stale/neutral drive removes it. The scene-load, direct-motor and neural-actuator probes now each start one robot profile per Webots process and iterate through all six profiles sequentially, with optional `--kind` for isolated diagnosis. `python3 scripts/qa/run_simulator_content.py --lane webots` passed at `target/qa/simulator-content/webots-qrlejc4r/`; `python3 scripts/qa/probe_webots_actuator_sensitivity.py` passed at `target/qa/webots-actuator-sensitivity/run-v8uvz6n3/`; `python3 scripts/qa/probe_webots_neural_actuators.py` passed at `target/qa/webots-neural-actuators/run-6ipaqh8c/`. All six sensory handshakes and motor diagnostics passed; both flies rose from 0.008 m to 0.316 m and landed near 0.008 m, while the fish samples stayed at 0.187–0.217 m. The source controller rebuilt with `make -B -C webots_world/controllers/nao_nn_controller_uds -j2`. Generated assets other than the deliberately preserved dirty primary `multi_neuroworld.wbt` match their generators. The full content contract lane still has its two known assertions against that primary world (generated export mismatch and expected two worms); it was not overwritten. These synthetic sandbox probes do not establish biological locomotion or the Phase 8 gate. Authority: user request and Sections 16.16, 16.19 and 21.13; no neural event, grant or persistence change.

- [~] `2026-10-02 11:45Z` Six-robot sensory sensitivity and input-raster parity: preserve all dirty visualisation, morphology, mobile and Webots world work. Canonical capture is `include/device_mapper.hpp`, Webots UDS is `webots_world/controllers/nao_nn_controller_uds/nao_nn_controller_uds.cpp`, neural admission is in the Rust UDS/runner path, and existing output rasters are in `src/ui.rs` and `web_ui/app.js`. The cluster RPC currently carries only current sensory indices while output has 128 history frames (`proto/distributed.proto` and `src/distributed.rs`), so an equally stepped input raster needs bounded sensory history from the owner rather than repainting a stale current value. Inventory six robots' sensory ranges and packet-to-neural admission, then add input-history presentation alongside output on every UI that exposes rasters. Read-only mobile shells currently expose activity indices but no raster; assess their equivalent presentation without inventing admission or biological activity. Verify isolated Webots sensory captures, owner receipt, neutral/active/decay behaviour, and UI parity. Authority: user request, Sections 16.5, 16.16, 16.19 and 21.13; preserve `INV-015`/`INV-017`.

- [x] `2026-10-02 12:06Z` Six-robot sensory mismatch located and repaired: the first launcher-width probe exposed zebrafish controller S=36 versus model S=32; unprefixed eye cameras were excluded, while four named inertial axes expanded to twelve values. `include/device_mapper.hpp` now respects `.x/.y/.z` names and takes per-sensor raw bounds and polarity from the Webots lookup table. `scripts/build_webots_zebrafish_assets.py` generates prefixed eye devices, its canonical PROTO was regenerated, and launcher retina overrides follow the new device names. The fish-specific 1×1 camera mapping now matches the model's luminance and temporal-gradient ports. `make -B -C webots_world/controllers/nao_nn_controller_uds -j2` passed. The intermediate six-robot probe at `target/qa/webots-neural-actuators/run-b22r2a1e/` passed 90 received/replied frames each with widths 24/418/418/34/250/32; all six had varying sensory channels, and zebrafish continuously-high channels fell from 18 to zero after proximity polarity correction. Final evidence is recorded below.

- [x] `2026-10-02 12:06Z` Input-raster source and client parity implemented: bounded owner-only `sensory_history` is emitted by `GetNetworkActivity` and combined with the output-owner HTTP response; native and web rasters consume neural-step histories including quiet columns. `RunnerEngine::activity()` exposes bounded workspace histories, and `src/runtime.rs` now caches that read-only projection after each local step so workspace clients actually receive it. Android's read-only Dashboard shows paired rasters after parsing bounded histories; iOS's remote session exposes workspace activity and `AarnnSpikeRastersView.swift` is integrated as an optional read-only `AarnnConnectomeView` component. `cargo test --locked --all-features --lib` targeted profile, sensory-owner, workspace-history and native-raster tests passed; the web activity-owner tests passed, `node --check web_ui/app.js`, `node scripts/qa/test_visualization_policy.cjs`, Android `testDebugUnitTest --offline` with JDK 21, and `git diff --check` passed. iOS has no Xcode project or Swift toolchain in this checkout, so device rendering remains unverified. The full `scripts/qa/test_simulator_content.py` still has the two documented unrelated dirty-world failures (export parity and expected two worms); its other six tests pass.

- [x] `2026-10-02 12:15Z` Final six-robot sensory contract probe: `python3 scripts/qa/probe_webots_neural_actuators.py` passed with diagnostics at `target/qa/webots-neural-actuators/run-7c11e3bi/`. Every Webots controller sent and received 90 frames with exact 24/418/418/34/250/32 widths and valid 0–1 values. Controller handshake names matched each respective model's sensory port list, including `network_nao.json` and the corrected zebrafish luminance/temporal-gradient eye names. Channels varying by more than 0.03 were 19/24 worm, 400/418 BANC, 401/418 FAFB, 22/34 hexapod, 239/250 NAO and 12/32 zebrafish under this specific 90-step scene. Zero zebrafish channels remained continuously above 0.99 after physical lookup-table polarity calibration; four fly proximity channels and one NAO channel remained high and may reflect sustained nearby surfaces. `cargo test --locked --all-features --lib six_webots_sensory_frames_reach_the_matching_runner_inputs --quiet` passed a socket-decode → profile-encode → Runner-history seam for all six dimensions. `cargo check --locked --all-features --bin aarnn_rust --bin web_ui --quiet` and final `git diff --check` passed. This verifies the receiving network's channel mapping and sensory history, while the live six-brain managed deployment and iOS signed device view remain separate acceptance work.

- [x] `2026-10-02 11:33Z` C. elegans Webots muscle sensitivity and six-robot crosscheck: the root Cargo workspace's canonical controller is `webots_world/controllers/nao_nn_controller_uds/nao_nn_controller_uds.cpp`, with shared motor mapping in `include/device_mapper.hpp` and generated anatomy from `scripts/build_webots_celegans_assets.py`. The earlier isolated three-worker replay at `target/qa/webots-spike-verify/logs/webots_runtime.log` confirmed 96 received neural channels and 23 spine motors but weak `drive_abs[mean,max]` of about `[0.03–0.10,0.09–0.34]`; additive trace saturation and two smoothing stages attenuated brief changes. A bounded, timestep-based simulated-muscle response and motor-speed/range-calibrated Webots command filter are now built into the tracked controller binary. `g++ -std=c++17 -O2 -Iinclude` and the two response test sources passed; `make -B -C webots_world/controllers/nao_nn_controller_uds -j2` regenerated the stale dependency file and rebuilt the tracked controller. `python3 scripts/qa/probe_webots_neural_actuators.py` passed with fallback motion disabled: each of six controllers replied to 90 synthetic neural frames, moderate output drove accepted non-neutral targets, strong output increased target magnitude, and targets relaxed after neutral input. Both fly profiles' neural leg commands increased from 0.565 to 0.717. Relative to each robot's root, the Webots Supervisor observed movement of 23/23 worm and 4/4 zebrafish movable spine segments; report and log are at `target/qa/webots-neural-actuators/run-immalcdu/`. The separate direct motor probe report at `target/qa/webots-actuator-sensitivity/run-6zbq2n5q/` showed accepted commands for 23 worm, 26 each fly, 18 hexapod, 40 NAO and 32 zebrafish movable motors; every available position sensor changed. The new simulator-content route test and `git diff --check` passed. The full simulator-content suite has two pre-existing failures against the dirty `webots_world/worlds/multi_neuroworld.wbt` (export parity and expected two worms); preserve that world and all unrelated visualization/mobile/morphology changes. This is a sandboxed Webots adapter change; no neural event, persistence, protocol or grant semantics change.

- [x] `2026-10-02 11:34Z` The separate isolated three-worker C. elegans live replay at `target/qa/celegans-muscle/live/logs/webots_runtime.log` shows the neural output reaches the Webots controller: each sampled interval received 8–11 replies, 16–38 of 96 output channels were non-neutral, and the bridge commanded spine values outside 0.5 (`[0.320,0.692]` across sampled intervals) with twitch fallback off. This proves live output and commanded bending; the synthetic Supervisor probe above measures resulting articulated movement. Neither test establishes sustained forward locomotion under the current neural pattern and world physics.

- [~] `2026-10-01 17:02Z` A fresh authenticated `/api/status` read shows
  `neuralmimicry-shared-snn` distributed across five workers with layer 0
  assigned to `native-qc04`, layer 1 to `native-qc02`, and layer 2 to
  `native-sm01`. `qc04` still reports changing layer assignments, and the
  persistent Webots controller has a frame retrying after the owner rejects it
  for not owning layer 0. Added a scheduler-authored expected-load fingerprint
  per worker and require the worker heartbeat fingerprint to match before the
  orchestrator publishes or accepts sensory ingress. Local mailbox readiness
  also checks that the target layer is assigned and the network is playing.
  `cargo test --locked --all-features --lib sensory` passes 37 tests, including
  stale-fingerprint rejection; `sharded_rebalance_expands_beyond_existing_affinity`
  passes with an assertion that every assignment receives an expected
  fingerprint; formatting and `git diff --check` pass. These source changes
  are not deployed, so the shared-SNN sensory/motor verification remains open.

- [~] `2026-10-01 17:02Z` Live status still lists `tenant-aarnn` as playing
  alongside `neuralmimicry-shared-snn`, though the five current worker records
  list only the shared SNN under `active_networks`. The Gail
  Ansible role requires its AARNN bridge network to match
  `continuum_tenant_aarnn_startup_network_id`, whose deployed default is
  `tenant-aarnn`. Keep that network until Gail is migrated and verified against
  an equivalent separate brain. All five worker journals also show recent
  8-second heartbeat timeouts; the orchestrator pod's last termination was
  `OOMKilled` at 16:39Z and its current memory is about 4.9 GiB. Investigate
  control-plane lock/memory pressure alongside the ingress rollout; a current
  `Running` pod and advancing world clock do not establish neural I/O health.

- [~] `2026-10-01 16:22Z` Read-only live diagnostics confirmed that
  `GetNetworkActivity` is failing on the same lock path identified by the
  worker logs. From `sm00`, authenticated GETs to `/api/activity` addressed
  directly to `native-qc04` (`192.168.1.64:50051`), `native-qc03`
  (`192.168.1.63:50051`), `native-qc02` (`192.168.1.62:50051`), `native-sm00`
  (`192.168.1.66:50051`) and `native-sm01` (`192.168.1.68:50051`) all returned
  retryable `network is busy; retry activity later`. This matches the prior
  `try_read` observer path; `src/distributed.rs` now waits at most 250 ms for
  the network read lock, copies the bounded activity snapshot, then releases
  the guard before encoding. `cargo fmt --all -- --check` and
  `cargo test --locked --lib activity -- --nocapture` pass (3 tests, 0 failed),
  including the held-writer/read-release regression. The activity change is
  local and not deployed. The sensory path still times out during prepare on
  `native-qc04`; no sensory admission or motor application is verified.
  Meanwhile `sm00` keeps the persistent world clock advancing at about 0.98x
  wall time and records coalesced source frames. On `qc01`, the AARNN engine pod
  has restarted four times (latest exit 137), and the orchestrator pod's last
  termination was `OOMKilled`; native worker configs still use
  `192.168.1.61:50051`, served by the `tenant-aarnn` orchestrator process.
  Removing that tenant/service now cannot satisfy the user's zero-impact
  condition, so it remains in place until a replacement coordinator is
  independently deployed and every dependent product is verified.

- [~] `2026-10-01 06:40Z` The ARM64 `Verify` leg of workflow run
  `36818861504` compiled on the only self-hosted ARM runner, which is live
  cluster node `qc01`. During that run the AARNN engine and orchestrator were
  OOM-killed; the run was cancelled before release or image publication. The
  subsequent read through the Ansible control host `spirit` found both pods
  Ready, with restart counts of 2 and 5 and `OOMKilled` as their last
  termination reason; current use was about 1.8 GiB for the engine and 1.9 GiB
  for the orchestrator. This confirms process readiness only, not recovery of
  every live brain. Four FPV worker pods remain unready with exit code 2 and
  hundreds of restarts; investigate that separate worker failure before
  claiming full runtime health. Move verification, package, container, and
  promotion jobs onto architecture-matched GitHub-hosted runners before
  dispatching the all-features build again. The workflow edits now select
  `ubuntu-24.04`/`ubuntu-24.04-arm`; syntax validation passed with `yamllint`,
  while actionlint is unavailable. Commit `199d49f` moved verification,
  package, container and promotion jobs to architecture-matched hosted runners
  and was pushed to this branch. All-feature build run `36826210234` is now
  verifying; after its workload manifests pass, update Ansible pins and deploy.
  Before closing recovery, confirm live neural state and investigate the FPV
  worker crash loop.

- [x] `2026-10-01 06:47Z` Re-converged the persistent Webots world from the
  `codex/webots-shared-world-20260929` checkout with
  `ansible-playbook -i inventory/hosts.ini playbooks/shared_world.yml`, with
  the role search path set to this checkout's `ansible/roles`; both `sm00` and
  `sm01` completed with zero changes. The service remains active only on primary
  `sm00`, both GPU checks
  pass, and both hosts expose `libgpgme.so.11` plus OpenCV 4.6 video-I/O. The
  live broker health endpoint reports one healthy `shared-fleet` world. The
  AARNN ingress image rollout still awaits verified images from run
  `36826210234`.

- [~] `2026-10-01 04:59Z` Read-only source tracing found why managed Webots
  output polls can report no actuator spikes even while sensory ingress is
  admitted. `src/bin/web_ui.rs::resolve_network_addrs` ranks active workers by
  largest shard, and `/api/activity` returns the first successful
  `GetNetworkActivity` response; that is not necessarily the worker owning the
  configured output-source layer. `send_aer_inference` also polls the same
  target used for sensory delivery. `src/runner.rs::get_io_layers` and the
  deployment `NetworkStatus` provide the authoritative output-layer rule and
  active layer assignment needed to select the owner without probing backups.
  No live brain state, snapshot or credentials were read or changed in this
  pass. Before editing, inspect the network-payload decoding and API test
  seams, then add a deterministic regression proving output polling selects
  the active output-layer owner while display/snapshot ranking stays unchanged.
  Evidence: `rg`/`sed` inspection of `src/bin/web_ui.rs` lines 6907–6970 and
  9429–9580, `src/distributed.rs` lines 1530–1554, `src/runner.rs` lines
  1958–2009, and `proto/distributed.proto` lines 637–670. Relevant authority:
  Sections 16.16–16.19, 17.4, 20.9 and 21.13; `INV-002`, `INV-007`,
  `INV-015`–`INV-017`.

- [~] `2026-10-01 05:02Z` Confirmed `NetworkActivityResponse` has no output-owner
  provenance, while the live `Runner` supplies the exact source layer and
  `ManagedNetwork` carries the active assignment. The chosen compatibility fix
  is additive: report the output-source layer and whether this worker owns it;
  only that worker returns output indices/history. `/api/activity` checks the
  status resolver's bounded active-node candidates and selects the
  output-owning response, failing retryably when no owner is available.
  Explicit node selection remains available for diagnostics.
  This avoids guessing the source from shard size or parsing large checkpoint
  JSON on every activity poll. The Webots C++ controller consumes only output
  indices/history, so the added JSON provenance is backward-compatible. No
  biological state, checkpoint, actuator output or permission is changed by
  this display/API resolver. Validation will cover an earlier larger non-output
  shard, the correct output owner, no active owner, and the worker response's
  ownership flag. Evidence: `src/runner.rs` lines 1958–2009 and
  `src/bin/web_ui.rs` lines 9429–9590/10120–10165; `webots_world/controllers/
  nm_api_robot_controller/nm_api_robot_controller.cpp` lines 321–386.

- [~] `2026-10-01 05:09Z` The first ownership regression exposed that
  `Runner::layer_range` can include a warm compatibility copy and is not by
  itself active ownership evidence. Refined the runtime marker to use
  `ManagedNetwork.assigned_layers`; an empty set is a complete local network
  only when its Runner is unsliced. Warm-only copies now return no output
  authority. The initial API selector tests passed; rerun them with this tighter
  active-owner rule before publishing. No live state changed.

- [~] `2026-10-01 05:13Z` Applied active output-owner selection to both the
  browser `/api/activity` projection and the AER inference poller. Inference
  now checks the candidate workers returned by current cluster status and only
  accepts activity from the worker that reports active ownership of the
  configured output-source layer; a non-owner response with spikes is ignored.
  Added a regression for this fail-closed behavior. Verification passed:
  `cargo test --locked --bin web_ui activity_selection_ -- --nocapture`,
  `cargo test --locked --bin web_ui recent_output_ -- --nocapture`,
  `cargo test --locked --lib network_activity_ -- --nocapture`, formatting and
  `git diff --check`. This remains local and is not yet deployed.

- [~] `2026-09-30 09:45Z` Compared the deployed AARNN contract with this source
  branch. The source at `src/bin/web_ui.rs` requires an
  `X-AARNN-Peripheral-Session` header for `/api/aer/inject` and checks a
  matching, locally consented session in process-local registry state. The
  public production OpenAPI currently differs: it has no
  `/api/peripheral/sessions` route and documents `/api/aer/inject` as requiring
  only the deployment-configured exact network grant. The authenticated live
  `/api/peripheral/input-grants` response confirms principal `webots` is scoped
  to `neuralmimicry-shared-snn` and `tenant-aarnn`. Thus the running API image
  has not adopted the checked-in session contract (or traffic reaches a
  different API deployment); the Webots C++ controller's stable JSON
  `session_id` is only an idempotent producer identity and will not satisfy the
  new header if that source is deployed. Its logs also show intermittent HTTP
  401s, which remain unexplained even though the same bearer currently
  authenticates read-only calls. `/api/me` reports broad AARNN control access
  for this service identity and needs a least-privilege review. Do not equate
  the JSON producer ID with user consent or bypass the gate. A background
  simulator needs an explicit, exact-network managed-session contract with
  visible status, revocation and restart/failover semantics; workstation-local
  consent remains unchanged. Phase 7 replicated session authority is still an
  unmet prerequisite for production enablement. Evidence: source blame assigns
  the check to `95006716`; production OpenAPI and
  `/api/peripheral/input-grants` were read on 2026-09-30, alongside `sm00`
  controller logs from 09:37–09:39Z.

- [~] `2026-09-30 09:53Z` Accepted ADR-0007 and amended the v1.1 specification
  to distinguish virtual server-managed simulation input from workstation
  capture. The integration will use a separate endpoint; local PeripheralSession
  consent remains mandatory on workstation routes. Implementation and route
  verification are in progress, and the production feature must remain
  fail-closed unless both the service identity and exact brain grant are set.

- [~] `2026-09-30 10:05Z` A read-only inspection of the live `aarnn-web-ui`
  deployment found `NM_PERIPHERAL_INPUT_GRANTS_JSON` configured but no
  `NM_MANAGEMENT_STATE_PATH`. The compatibility UI therefore has no persisted
  management policy to consult. Keep exact principal/brain grant checks by
  falling back to that validated deployment grant list only when persisted
  management authority is absent; if configured authority exists but cannot be
  read, deny. This is required for the currently deployed configuration and
  does not relax workstation-session consent. The AARNN implementation now
  includes this fallback and scoped tests; production rollout remains pending.

- [~] `2026-09-30 09:45Z` The same live controller journal shows
  `neuralmimicry-shared-snn` sensory frames being admitted (about 1,095 by
  simulation step 513,001) with 10–11 input spikes, but every available
  activity report in the sampled interval had zero output spikes and zero
  mapped actuators; some activity projections returned busy. `tenant-aarnn`
  frames are rejected because the actual network sensory width is zero. The
  authenticated cluster status reports exactly these two networks active on
  `native-qc02`, `native-qc03`, `native-qc04`, `native-sm00` and
  `native-sm01`; it does not list `qc00`, `qc01` or `qc05` as active native
  AARNN workers. No brain snapshot, weight or topology was read or changed.
  The `tenant-aarnn` mismatch needs an authorised topology/I/O decision; a
  profile declaration of 32 inputs does not resize the network.

- [x] `2026-09-30 09:45Z` Verified public route boundaries: unauthenticated
  `https://neuralmimicry.ai/webots` and
  `https://webots.neuralmimicry.ai/` return HTTP 200, while an unauthenticated
  `/stream` request returns 401. An authenticated browser handoff was not
  exercised, so the site route alone is not browser acceptance evidence.
  Recovered the same existing 49-byte `sm00`/`sm01` Webots service credential
  into the profile's intended controller-side source in the sibling
  `simulation_environment` repository,
  `ansible/playbooks/.secrets/customers/spirit/webots_access_token`, after
  adding that directory to `.gitignore`; the directory/file are mode 0700/0600
  and Ansible resolves a value of at least 32 characters. No credential value
  was printed or committed.

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

- [~] `2026-10-01 04:22Z` Read-only production inspection found the persisted
  `neuralmimicry-shared-snn` snapshot has 32 sensory channels and zero saved
  output channels, while the deployed Webots profile declares a 32/16 I/O
  contract. Startup specs loaded the snapshot without that contract, leaving
  robot actuator output unavailable. Added optional
  `NM_ORCHESTRATOR_STARTUP_IO_CONTRACT` handling and deterministic startup
  snapshot alignment, with the simulation Ansible profile pointing it at the
  mounted `default-network.json` and checking readability before deployment.
  The deployment also scopes that fallback to the two configured Webots
  network IDs so unrelated orchestrator networks retain their own contracts.
  The focused zero-to-16-output deterministic alignment regression passes.
  Running the complete startup-alignment test group exposed a pre-existing
  build-profile bug: sensory spike-history frames were resized only under
  `growth3d`; moving that state resize into the common Runner path makes all
  five startup-alignment tests pass in the default build. No production rollout
  has occurred. A fresh backup completed at `2026-10-01T04:30:58Z`:
  `/var/lib/aarnn-backup/neuralmimicry-shared-snn-20261001T043058Z.snapshot.json`
  (271,640,404 bytes, with a SHA-256 sidecar). Rollback is the prior image and
  removal of the environment variable; the source snapshot is not modified by
  this startup transform. Verify live sensory admission/output mapping after
  rollout. The Phase 8 gate remains open.

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

- The 2026-10-02 one-hexapod local replay accepted AER sensory frames through sequence 1495 (typically 24–27 spikes), then frame 1496 repeatedly timed out and ended in HTTP/2 `GoAway(too_many_resets, ENHANCE_YOUR_CALM)` at `logs/webots_hexapod_01.log:100494`. The IPC owner stopped replying at line 100496, matching the stopped Webots clock. Every sampled controller reply remained neutral on all 18 motors, so these logs prove sensory admission but do not prove output spikes or motor drive. The native rasters obtain only bounded recent worker/snapshot histories; their empty state after the stall does not refute the earlier admission.
- The final saved hexapod snapshot has `runtime_state.hist_len=2`, with only two `spk_hist_s` and two `spk_hist_o` frames. The worker's `GetNetworkActivity` caps a read at 128 frames but reads these biological rings directly, so it can return only two. Logged admitted sensory frames are roughly ten Runner steps apart near sequence 1495; a UI poll often sees two quiet steps and truthfully paints no input spikes even while admission works. Raising `hist_len` would alter delay/replay state and accelerator buffers, so retain a separate bounded sparse, read-only display ring populated after a successful managed step and never use it for biological transitions or persistence.
- This supposedly local run bound its orchestrator to `0.0.0.0:50051`. `ss` shows established LAN connections to six `native-*` workers, and the orchestrator inventory later lists nine nodes and four networks (`hexapod_01`, `tenant-aarnn`, `neuralmimicry-shared-snn`, `celegans_01`). `Join`/heartbeat auto-registration and rebalance in `src/distributed.rs` admit worker-reported networks; `run_webot.sh` exposes the local test orchestrator and workers on all interfaces. The extra placement and RPC work plausibly exacerbates the timeouts, but no single causal lock has yet been demonstrated. The active user run must remain untouched; bind future local launcher instances to loopback while retaining the separate remote-compute launch path.

- The no-override historical rerun exposed a launch-time snapshot race. The IPC owner and first extra worker each read the copied 12,204,451-byte `latest.snapshot.json` and spent about 184–185 seconds rebuilding it. During that wait the workspace autosave atomically replaced `latest.snapshot.json`; only then did the sequential launcher start extra worker 2, which read a 55,755,977-byte evolved snapshot and skipped morphology rebuild in 2.555 seconds. Thus the three processes joined with different model generations, defeating deterministic startup and making the subsequent bridge-readiness result unsuitable as acceptance evidence. Pin one per-brain startup snapshot for the orchestrator and every local worker before launching any of them; keep workspace autosave directed to its separate live `latest` path. Start all extra workers before waiting for any registration so one worker's initialisation does not serialise the others. Preserve the one-robot-per-test limit.

- The copied 12 MB hexapod workspace registered its IPC owner and two extra workers after 185.341, 184.646 and 194.830 seconds respectively under explicit 300-second per-worker startup limits. Webots then loaded, but the managed sensory bridge was still unpublished. `src/ui.rs` exhausted its separate, hardcoded 120-second IPC sensory retry on frame 0, permanently stopped Webots replies, and prevented the controller's `Connected.` marker. This is a second startup deadline beyond `--connect-timeout`; extending the launcher alone cannot make the historical workspace usable. The bridge readiness gate must remain fail-closed, and any extra wait must retain frame identity and a finite bound. The isolated run was stopped before the launcher could wait out its 300-second controller deadline; all three worker processes had registered.

- The installed Webots `resources/nodes/TouchSensor.wrl` says a TouchSensor's `boundingObject` is critical to contact detection. Both generated worm bumpers were bare `TouchSensor { type "bumper" }` nodes, and the previous 90-frame physical probe reported no variation in channels 6 or 7, even though 19 of the 24 sensory channels varied. A first collision-shape probe then showed both channels stuck at 1 throughout the default habitat: the original front food sphere was centred at +0.20 m with a 0.03 m radius and the rear barrier at -0.18 m, overlapping the ±0.19 m bumper positions at spawn. Moving both props to leave roughly 3–4 cm of starting clearance restored quiet/active/released contact states in the physically gravitating normal habitat. Inertial channels 0–5 continue to encode body motion; this Webots contact geometry is a sandbox measurement proxy rather than a species-level touch model.

- The one-worm raster report exposed two different Runners in the IPC owner process. UDS float frames were decoded into local `App` spikes and used for motor replies, while the orchestrator's input and output rasters observed the separate managed network. The old six-profile socket-to-Runner test covered only the local path. A corrected live run now proves the managed owner consumed Webots spikes. The first corrected attempt rejected frame 0 before placement publication; retaining the same frame through bounded startup backpressure allowed the managed bridge to accept it. A `--no-build` check with an all-feature binary must set `NM_WEBOTS_RUNTIME_FEATURES=all-features`; otherwise the launcher removes management credentials for its default narrower binary profile. The final timeout sent SIGINT and then killed a still-exiting launcher; remaining test worker processes were stopped explicitly. A native UI capture through an isolated Xephyr X server subsequently showed the input raster populated, closing the pixel-level check that Wayland screenshot capture could not perform.

- The first physically gravitating, single-fish neural probe sampled a 0.252 m startup height above the aquarium's 0.24 m wall. The shared freshwater Fluid then extended beyond the visible 0.42 × 0.32 m tank and above its water surface. Aligning the named physical Fluid to the tank's 0.06–0.2076 m internal water column brought the fish's sampled heights into 0.187–0.217 m without changing its neural output path. This is buoyancy/immersion evidence in Webots, not a calibrated hydrodynamic model. For multi-fish scenes, the existing shared Fluid still spans the group; per-tank named-fluid binding remains a separate refinement.
- The direct motor probe's worm and fish joints expose no `PositionSensor` devices for their articulated spine motors, so accepted targets alone are insufficient feedback. Its Supervisor measured movement in 24/24 worm segments and 5/5 fish segments in their separate worlds. The fly force is deliberately world-vertical and bounded, rather than a resolved body-axis wing force; its observed rise and landing validates this Webots proxy under the probe stimulus only.

The six-profile Webots probe caught a zebrafish contract mismatch before neural admission: the robot emitted 36 indexed values for a 32-input network. Eye cameras had no `zebrafish_s_` prefix, and the inertial names' axis suffixes were documented but ignored by `DeviceMapper`. Once the width was repaired, 18 channels stayed above 0.99 because the generic distance lookup response increased toward no contact. Reading the actual lookup-table polarity removes that tonic no-contact input without assuming one raw transfer direction for every Webots robot. The previous workspace activity cache also stored only the immediate step summary, so mobile read-only activity endpoints would have returned empty histories despite `RunnerEngine::activity()` being able to construct them.

- The Webots worm's 96 `celegans_o_*` devices are channel-only joints; locomotion uses 23 separate `celegans_spine_*` motors. The current bridge converts each neural pulse into an additive trace (`trace = 0.92 * trace + 0.62 * drive`, capped at 1), so ordinary repeated pulses can saturate both dorsal and ventral sides and cancel. The bridge then uses a 0.22 target EMA and `DeviceMapper` uses another 0.1 EMA before calling `Motor::setPosition`. A three-worker isolated replay confirmed non-neutral bridge targets without fallback twitch, but did not measure joint angle or displacement. The generated PROTO and dirty world must not be edited for this tuning.

- Webots' equal-position-limit convention makes the worm and zebrafish passive root-lock motors appear numerically unbounded to `DeviceMapper`; they are held at neutral and excluded from movable-motor diagnostics and sensitivity calibration. In the six-robot synthetic UDS probe, the 0.65 output phase produced maximum target fractions 0.153–0.370 of half range, and the 1.0 phase produced 0.431–1.000. NAO and zebrafish reach the configured inward motor margin on a sustained full-scale command; Webots still enforces declared joint speed/torque limits. This is simulator command acceptance and posture evidence, not biological locomotion or a physical-actuator safety claim.

- Live authorization and placement evidence confirms that `tenant-aarnn` is
  still an external product dependency: Gail's rollout asserts bridge/network
  alignment with the startup network, currently named `tenant-aarnn`. Live
  cluster status also lists it as a playing network. Its removal would change
  Gail behavior unless the bridge is explicitly migrated and independently
  verified, so it remains untouched.
- `aarnn-orchestrator` recently terminated with `OOMKilled` and all five
  native workers logged 8-second heartbeat timeouts. This may drive placement
  churn as well as stale route advertisement; test the fingerprint gate and
  separately identify the control-plane memory/lock cause before treating the
  shared SNN as operational.

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

- `2026-10-02 / DEC-WEBOTS-RASTER-OWNER-EVIDENCE`: the native cluster rasters accept sensory/output frames only when both the worker's activity response and the orchestrator's assignment identify that worker as the respective I/O owner. Track the most recent new step for each owner separately and label an unavailable/stale owner independently from a responding owner with zero spikes. A non-owner compatibility Runner is not biological evidence for either raster. This is bounded read-only presentation state; it cannot manufacture a spike, admit input, grant an actuator, or change logical time. Evidence: the first isolated replay's one-of-three activity poll, the second replay's separately observed input/output owners, and focused owner-filter regressions. Authority: Sections 16.8, 16.19, 16.22 and 21.13; preserve `INV-010`, `INV-014` and `INV-016`. Rollback is the previous shared poll-success label, which can falsely call an absent source silent.

- `2026-10-02 / DEC-WEBOTS-LOCAL-ISOLATION`: the local `run_webot.sh` cluster profile binds its orchestrator and worker gRPC listeners to loopback. Remote-compute mode keeps its separately configured network listener. This confines a one-robot parity test to its declared nodes and prevents accidental cross-cluster admission without changing authoritative events, placement policy, or the active run. Authority: Sections 12.1, 16.8 and 16.16, `INV-010`, and the one-robot user constraint. Rollback is the previous listener binding; a remote worker test must select the remote-compute profile explicitly.
- `2026-10-02 / DEC-WEBOTS-DISPLAY-BACKOFF`: the native cluster display poll may skip an unavailable worker projection and retry at a slower bounded rate; the raster must label that state as stale rather than claiming zero spikes. Accepted activity and committed sensory frames are unchanged. This keeps optional visualisation traffic from repeatedly cancelling a saturated worker's HTTP/2 streams while Webots ingress has priority. Authority: Sections 16.8, 16.22 and 16.24, `INV-007` and `INV-010`.
- `2026-10-02 / DEC-WEBOTS-SPARSE-HISTORY`: keep a 128-step sparse sensory/output display history per managed network, independent of the Runner's `hist_len` biological delay ring. Record it only after a successful managed step and clear it on a step discontinuity; owner filtering remains in the activity RPC. The native cluster rasters use this worker history whenever workers own the network: a full snapshot's two-step biological ring cannot advance the cursor ahead of intermittent worker spikes. An unassigned single-process cluster retains the bounded snapshot fallback, and a later assignment change clears its old cursor. This is read-only telemetry and is neither checkpointed nor used to infer admission, output commitment or actuator effects. It lets the UI see intermittent stimuli without increasing model delay buffers or accelerator memory. Authority: Sections 9.5, 16.8, 16.22 and 18.1; `INV-007`, `INV-008` and `INV-016` remain unchanged.
- `2026-10-02 / DEC-WEBOTS-TOPOLOGY-POLL`: refresh a stable cluster topology snapshot at most every 15 seconds while the separate sparse activity poll continues at its own rate. Immediate view/target changes and bounded failure retries remain eligible. The previous two-second full-snapshot cadence contributed large read-only RPC load during the complex hexapod run; this tuning affects only presentation freshness, not neural time or committed events. Authority: Sections 16.8 and 16.22, `INV-010`.

- `2026-10-02 / DEC-WEBOTS-WORKER-ROUTE-RETRY`: keep the expected-load-fingerprint gate at the orchestrator, which alone owns the expected command fingerprint and advertises a sensory ingress owner only after the exact worker load heartbeat. An IPC worker uses only that published route and checks peer connectivity; the destination's bounded prepare mailbox validates actual playing state, sensory-layer ownership and width without retaining a node-state lock across RPCs. After an ambiguous remote prepare timeout, do not send an unversioned remote abort that could arrive after a same-frame retry and erase its reservation; retry the same identity or let its five-second slot expire. A commit ambiguity still fails closed. Evidence: three pinned historical hexapod workers loaded in about 204 seconds; the old IPC-owner check found no fingerprint, while the corrected route admitted frame 0 and consumed 0–20; frame 21 later exposed `sensory frame was not prepared` amid timed-out prepare calls and no worker reload. Rollback restores the previous route check/abort, which makes worker-origin sensory ingress unusable or vulnerable to a late abort; the Phase 8 dedicated governed ingress remains required. Authority: user non-blocking/robustness request and Sections 16.17–16.19/21.13; preserve `INV-007`, `INV-015`–`INV-017`.

- `2026-10-02 / DEC-WEBOTS-PINNED-PARALLEL-STARTUP`: for a local cluster launch, copy each brain's selected snapshot once into a private per-run startup directory and use that immutable input path for the orchestrator and every worker. Keep the workspace binding on its live autosave path; remove only the launcher's private snapshot copies after all child processes exit. Spawn all extra workers before polling any registration, enforce one common bounded deadline, report live progress and fail promptly if any process exits. Evidence: the historical hexapod rerun's first two processes read 12,204,451 bytes while the serially started third read a 55,755,977-byte autosave replacement. The pin prevents a mixed startup generation and the parallel launches avoid waiting for one worker's ~185-second initialisation before another can start. This changes launch orchestration only, not snapshot publication, model event order or distributed ownership. Rollback removes the copy/poll loop; the former sequential path is retained only as historical evidence, not a valid consistent-startup guarantee. Authority: user's parallelism and startup-time requests plus `INV-008`, `INV-012` and Section 18.3.

- `2026-10-02 / DEC-WEBOTS-CALIBRATED-STARTUP-BUDGET`: estimate each local worker's join budget from the largest persisted snapshot at 30 seconds per rounded-up MiB, with a 120-second floor and 1,800-second cap; set cluster-distribution and managed-IPC readiness to twice that estimate, with a 300-second floor and 3,600-second cap, and give the controller another 60 seconds to finish its handshake. The 4 MB clean hexapod loaded in about 51 seconds; the user's copied 12 MB historical snapshot loaded in 185–195 seconds, beyond the former 60-second limit. Explicit `WEBOTS_CONNECT_TIMEOUT`/`--connect-timeout`, distribution, controller, and IPC timeout overrides retain precedence. A Webots IPC frame arriving before its managed sensory bridge exists retains its original sequence and waits for the finite managed-IPC readiness budget instead of being permanently rejected after the previous fixed 120 seconds; ordinary workstation input keeps its 120-second bound, and malformed/permanent errors still fail closed. This is launcher and pre-admission timing only: it does not alter biological time, placement authority, committed events, or actuator fencing. Rollback is the previous script/Rust retry constants; observe eventual bridge publication under the historical snapshot before claiming full Webots startup acceptance. Authority: user timeout report and Sections 16.17, 16.24, 18.3 and 21.13; preserve `INV-007`, `INV-015`–`INV-017`.

- `2026-10-02 / DEC-CELEGANS-WEBOTS-BUMPER-CONTACT`: keep the existing two 24-channel-indexed worm touch ports and give their Webots `TouchSensor` nodes 0.022 × 0.010 × 0.026 m collision boxes at the body tips, raised 0.010 m in local vertical. Move the default habitat's front food sphere and rear barrier to leave about 3–4 cm starting clearance rather than tonic contact. Evidence is the installed Webots TouchSensor node contract, the previously inert channel-6/7 sensory report, and the one-robot Supervisor contact probe's quiet/front/rear/released frames. The geometry affects only simulated Webots samples; it changes neither biological topology, endpoint identity, managed admission nor the UI history protocol. Regenerate the PROTO from `scripts/build_webots_celegans_assets.py`; rollback reverts the generator/PROTO and ecology placements together. The dimensions are calibrated to this 0.34 m Webots spine and the existing 24-port proxy, not a measured C. elegans mechanoreceptor field. Authority: user push-stimulus report and Sections 16.17/21.13; preserve `INV-015` and `INV-017`.

- `2026-10-02 / DEC-WEBOTS-IPC-MANAGED-SENSORY`: in the current layer-sharded compatibility runtime, route the Webots IPC owner's already-encoded spike frame into the selected managed network's existing bounded prepare/commit sensory mailbox, with an IPC-only acknowledgement before generating the local motor reply. The producer retains one session and increasing frame sequence; transient startup placement/Start errors retry for at most 120 seconds without skipping frame 0, while shape/identity errors stop robot replies. The old local Runner remains the Webots motor-reply compatibility path until the Phase 8 committed-output binding replaces it; managed and local output rasters must not be represented as equivalent effects. Evidence is the original empty input raster, the missing managed-consumption log, and the isolated 92-frame live rerun. Rollback is removing this IPC forwarding/acknowledgement path; the separate local Runner remains the pre-existing compatibility behavior. This does not grant a workstation peripheral session or claim completion of virtual-simulation service authorization, capture-time mapping, or `INV-016` effect fencing. Authority: user report, ADR-0007, Sections 16.16–16.19 and 21.13; preserve `INV-007`, `INV-015`, `INV-017`.

- `2026-10-02 / DEC-WEBOTS-UI-INITIAL-NETWORK`: a Webots launcher with a named brain passes that brain ID to the native UI as `NM_UI_PREFERRED_NETWORK_ID`. Initial automatic selection waits until the named network is in the authorised inventory; it does not substitute the largest unrelated network. An already selected view remains under the user's control. Evidence: the live `celegans_01` UDS log versus the `tenant-aarnn` UI snapshot and focused selection/raster tests. This changes only the default display target and is reversible by removing the launch environment variable; no biological step, peripheral grant, committed output or network placement is altered. Authority: user raster defect report and Section 21.13.

- `2026-10-02 / DEC-WEBOTS-SEQUENTIAL-PARITY`: run each of the six robot profiles in its own Webots process, in a fixed sequence, for scene, direct-actuator and neural/flight parity checks. One recorder Supervisor may accompany the sole tested robot for measurement; no other physical robot profile is loaded. Preserve a per-profile log/report and an aggregate report. The content wrapper permits six bounded 90-second launches rather than a 180-second fleet deadline. This reduces concurrent memory use and isolates attribution, while increasing total test duration. Rollback is the prior concurrent probe scripts; no runtime product policy or biological timing changes. Authority: user's GNOME out-of-memory report and Section 21.13.
- `2026-10-02 / DEC-WEBOTS-PHYSICAL-PROXIES`: retain physical gravity at 9.81 m/s², use the fish's named Webots Fluid/immersion for buoyancy, align single-fish water to the aquarium's visible internal dimensions, and apply a bounded vertical force from fresh neural fly output with neutral/stale decay. The 8 ms fly step prevents observed floor tunnelling during landing. These are reproducible Webots-scale heuristics in metres, seconds and newtons; the fly rig's approximate 14 g mass gives a 0.137 N hover reference, while the force controller caps at 0.20 N. The six isolated probes validate bounded motion and fail-safe decay, not species-accurate aerodynamics or fish hydrodynamics. Revert the generator/controller changes to return to the previous sandbox physics; no committed neural semantics or physical-actuator authority is changed. Authority: user gravity/flight request and Sections 16.19 and 21.13.

- `2026-10-02 / DEC-WEBOTS-SENSORY-POLARITY`: derive each simulated distance receptor's normalised 0–1 range from `DistanceSensor::getMinValue/getMaxValue`, then inspect the first and last physical lookup-table response values. Invert only a response that increases with physical range, so nearby stimuli are strong regardless of that device's transfer direction. A named inertial `.x/.y/.z` device contributes the one selected measured axis; an unsuffixed device retains three axes. The zebrafish eyes use their configured 1×1 cameras for bounded log-luminance and temporal-gradient outputs in four indexed input positions, matching the model's declared port names rather than the generic ON/OFF camera encoder. Evidence is the six-controller UDS probe and the generated Webots PROTO; this is a heuristic sandbox transducer calibration in Webots values, not a measured biological firing-rate claim. It does not change neural event semantics or physical-actuator authorisation. Authority: user sensory sensitivity request, Sections 16.16 and 16.19; preserve `INV-015` and `INV-017`.

- `2026-10-02 / DEC-INPUT-RASTER-HISTORY`: input and output rasters consume bounded, ordered neural-step spike histories from their respective authoritative owners; quiet steps remain columns and a non-owner cannot manufacture sensory activity. Workspace activity caches the engine's read-only history projection after stepping. Mobile read-only shells render the same paired histories without deriving spikes from camera previews or wall-clock refresh. Authority: user UI-parity request, Sections 16.5 and 21.13; display state is non-authoritative.

- `2026-10-02 / DEC-CELEGANS-WEBOTS-MUSCLE-RESPONSE`: tune the C. elegans Webots sandbox muscle decoder with a bounded first-order response in milliseconds and saturating dorsal-versus-ventral gain based on the existing ±0.38-radian joint range and observed weak-drive distribution. Calibrate the shared Webots motor command filter from each motor's declared range, speed and simulation step, while retaining legacy filtering for channel-only motors and passive root locks. Preserve neutral output, opposite-side sign, the existing joint position/velocity/torque limits and 96-channel ordering. The worm response version, gain override and optional per-robot motor diagnostics make this heuristic reproducible and reversible; the 0.65/1.0 six-robot probe is its cross-profile acceptance check. This is not a biological adequacy claim or authorisation for a physical actuator. Authority: user request, Sections 16.19, 18.1 and 21.13; `INV-016` remains bounded by the existing output path.

- `2026-10-01 / DEC-WEBOTS-OUTPUT-ACTIVITY`: a Webots output/activity response
  must identify its configured output-source layer and prove that the active
  worker assignment owns it. Largest-shard and highest-capacity ranking are
  not valid substitutes for biological output ownership. The HTTP activity
  route selects only an output-owning response unless a diagnostic node was
  explicitly requested; absence/busy ownership remains retryable and must not
  be presented as an empty actuator frame. This response is a read-only
  activity projection and does not assert output commitment or authorise an
  effectful actuator. Authority: Sections 16.16, 16.19 and 18.1; `INV-007`,
  `INV-016`, plus the observed Webots output-resolution defect.

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
- `2026-09-30 / DEC-WEBOTS-SIMULATION-INGRESS` (ADR-0007): server-managed
  virtual-world sensors use a distinct route guarded by an allow-listed service
  account, `aarnn:use` and the existing exact brain-scoped `PeripheralInput`
  policy. This route never creates or impersonates a workstation session and
  grants no physical device or global actuation access. A viewer disconnect
  does not stop the server simulation; stopping the world or revoking the grant
  stops later admission. Authority: explicit persistent-Webots user request,
  `INV-002`, `INV-007`, `INV-015`, `INV-017`, and ADR-0007.
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
