# AARNN repository instructions for Codex

## Purpose and authority

This repository is implementing the distributed whole-brain emulator defined by `docs/specifications/distributed-whole-brain-emulator-v1.1.md`. That specification is the normative source for biological semantics, logical time, distributed execution, determinism, durability, management, workstation input/output—including concurrent bidirectional USB AER exchange—security and acceptance. The same authoritative Rust implementation shall support official server, orchestrator, workstation, web, iOS and Android products through explicit product profiles and platform adapters; it shall not be copied into product-specific forks. Mobile products shall be capable of independently creating, importing, executing, checkpointing, restoring and managing a local brain, operating as governed bidirectional AER producers/consumers for other AARNN instances, discovering and enrolling compatible nodes, and opting into or out of explicitly authorised federated AARNN neural networks. Publishing directives such as `[FIGURE:*]` and `[PAGEBREAK]` in the Markdown source are non-normative.

If code, comments, tests or older documentation conflict with the specification, do not silently preserve the conflict. Record it in the active ExecPlan, determine whether it is an intentional compatibility path, and implement the specification through the staged migration. Do not weaken a safety invariant merely to preserve current behaviour.

The nine phase plans under `docs/execplans/` define the implementation order. `.agent/PLANS.md` defines how those living ExecPlans must be maintained.

## Instruction precedence

1. Follow applicable system, security and repository instructions.
2. Follow this `AGENTS.md` for all repository work.
3. Follow the normative implementation specification.
4. Follow the active phase ExecPlan.
5. Follow more specific `AGENTS.md` or `AGENTS.override.md` files only within their directory scope.

A phase plan may refine implementation details but may not contradict a specification invariant. When a conflict cannot be resolved from repository evidence, stop before changing authoritative state, record the blocker and request the narrow decision required.

## Required repository discovery

Before modifying code:

1. Read this file, `.agent/PLANS.md`, the active phase plan and every referenced specification section completely.
2. Locate the Git/workspace root and read workspace manifests, contributing guidance, generated-file notices, CI workflows, deployment definitions, mobile projects, application signing configuration and existing architecture decisions.
3. Use symbol/reference search to find canonical modules. Uploaded names such as `runner(1).rs` or `morphology(1).rs` are evidence, not paths. Never create suffixed duplicate modules merely because an uploaded file used that name.
4. Inspect `git status` and preserve all unrelated user changes. Do not reset, overwrite or reformat unrelated work.
5. Record verified paths, commands, current behaviour and discrepancies in the active ExecPlan before relying on them.

The specification names candidate files including `distributed.rs`, `runner.rs`, `network.rs`, `topology.rs`, `transmission.rs`, `aer.rs`, `bridge.rs`, `dynamics.rs`, `morphology.rs`, `transport.rs`, `ui.rs`, `app.js`, `service-access.js`, `shell.js`, `index.html` and `swagger.html`. Mobile work may additionally discover Xcode/Swift, Gradle/Kotlin, UniFFI or other binding sources. Resolve every canonical repository-relative location before editing. Do not create a second Rust workspace beneath an application project.

## ExecPlans and phase order

Significant features, cross-module refactors, protocol/schema changes, migrations, security-sensitive work and work expected to exceed one focused coding session require an ExecPlan maintained in accordance with `.agent/PLANS.md`.

Execute phases in this order unless the specification explicitly permits preparatory work that does not expose later-phase behaviour:

1. `phase-00-baseline.md`
2. `phase-01-deterministic-primitives.md`
3. `phase-02-superdense-executor.md`
4. `phase-03-partitioning-and-scc.md`
5. `phase-04-distributed-data-plane.md`
6. `phase-05-multi-brain-scheduler.md`
7. `phase-06-durability-and-recovery.md`
8. `phase-07-management-plane.md`
9. `phase-08-workstation-io.md`

Do not declare a phase complete until its gate and mapped verification evidence pass. Later phases may add interfaces behind disabled coarse feature flags, but must not bypass an unmet earlier safety gate.

Mobile delivery is a cross-cutting track through these phases, not a disconnected phase added after the server is complete:

- Phase 0 inventories platform dependencies, establishes the product/target matrix, captures portable reference examples and introduces the shared QA/example runner without changing runtime semantics.
- Phase 1 extracts platform-neutral identity, logical-time, numeric, event, serialisation and digest crates and proves identical fixtures on every compilable target.
- Phase 2 moves the reference executor behind a platform-neutral runtime API; iOS and Android may run only bounded local/reference examples until later safety gates pass.
- Phases 3–6 keep partitioning, scheduling, causal transport, durability and recovery independent of UI and operating-system APIs. Mobile lifecycle loss is modelled as a transient edge-node failure, not hidden by platform-specific state.
- Phase 7 exposes generated management/client contracts, node identity/enrolment, authenticated discovery/directory resources and secure resumable sessions suitable for workstation, web and mobile shells.
- Phase 8 implements platform adapters, independent mobile brains, AER-over-transport, autodetection, federation discovery/consent, mobile lifecycle/capability handling, store-ready native applications and the cross-product acceptance matrix while preserving the workstation USB-AER requirements.

Before mobile code is merged, the active ExecPlan shall map each mobile milestone to the owning phase, requirements, target products, scenarios, toolchain constraints and rollback boundary. If official mobile delivery requires a normative semantic change, update the specification through an ADR/specification revision before implementation; `cfg` branches shall not become an undocumented alternative semantic model.

## Non-negotiable invariants

Preserve specification invariants `INV-001` through `INV-017` exactly:

- `INV-001`: each committed shard state has exactly one active writer term.
- `INV-002`: an event carries the logical time of the synaptic transition it represents, never packet-arrival or wall-clock time.
- `INV-003`: same-time causal output from `(t, μ)` advances to `(t, μ + 1)`.
- `INV-004`: positive delay `δ` advances to `(t + δ, 0)`.
- `INV-005`: no receiver declares closure from silence.
- `INV-006`: settling-limit exhaustion is non-convergence, never quiescence. Complete the current-microstep proof, checkpoint provisional state, preserve unresolved events, retag them to the configured next quantum and mark `deferred_from_nonconvergence`.
- `INV-007`: committed events are never silently dropped for responsiveness.
- `INV-008`: deterministic-reference replay is independent of thread scheduling and transport order.
- `INV-009`: topology changes become visible atomically at an agreed logical boundary.
- `INV-010`: unrelated causal components and whole-brains do not wait for one another.
- `INV-011`: management clients never bypass orchestrator authorisation, operation state or fencing.
- `INV-012`: a checkpoint is immutable after publication.
- `INV-013`: route watermarks do not prove termination of a distributed same-tick cycle.
- `INV-014`: every axon terminal, synapse, weight, delay, release state and plasticity trace has exactly one authoritative owner in a topology generation.
- `INV-015`: admitted peripheral samples retain capture sequence/time, clock-mapping version and uncertainty; admission derives from the declared mapping.
- `INV-016`: external effects originate only from committed neural output and use stable `EffectId`, actuator fencing and deduplication.
- `INV-017`: peripheral access is explicit, scoped, revocable, locally visible and independently authorised from brain management.

The mobile/discovery/federation extension adds repository delivery invariants that shall be promoted into the specification traceability table before implementation:

- `APP-INV-001`: a mobile standalone brain uses the same biological, logical-time, event-ordering and persistence semantics as the host reference; suspension advances no biological time.
- `APP-INV-002`: a discovery observation grants no identity, trust, role, permission, data access, compute admission or federation membership.
- `APP-INV-003`: enrolment, AER capture/input, AER output/actuation, compute lending and federation are separate scoped and revocable grants.
- `APP-INV-004`: changing LAN/WAN/Wi-Fi/5G/USB/Lightning/relay path never changes capture provenance, biological time, endpoint identity or exactly-once effect rules.
- `APP-INV-005`: federation preserves each brain's independent identity, timeline, authority, logs and checkpoints; only explicit versioned link events cross the boundary.
- `APP-INV-006`: opting out of discovery or federation prevents new discovery/link establishment within the bounded expiry policy without corrupting a local standalone brain or fabricating closure of already committed events.

## Architectural boundaries

Keep three representations separate:

- biological model: neurons, synapses, morphology, plasticity, delays, fields and growth;
- virtual execution: stable shards, zero-delay components, routes, logical tags, queues, logs and replica roles;
- physical placement: nodes, CPU/GPU/NUMA devices, RAM/VRAM, storage, network and failure domains.

Keep biological data, causal transport, control, management, discovery, enrolment/trust, federation-directory and peripheral/AER media planes logically separate even when an early deployment shares processes. Discovery and directory outages shall not corrupt local execution; management status traffic shall not carry high-rate AER frames; AER congestion shall not prevent credential revocation or emergency stop.

Treat USB AER as an independently governed peripheral modality. A workstation may concurrently operate USB AER input/output, microphone, camera, display, keyboard, pointer, audio/video presentation and management sessions for the same authorised brain. Give every channel independent identity, sequencing, clock mapping, queue/credit budget and failure state. USB hot-plug, reconnect, congestion or failure shall degrade only the affected AER channel unless an explicit brain missing-input policy states otherwise; it shall not stop or retimestamp the other channels.

Biological kernels may depend on identity, logical-time and numeric primitives. They must not depend on gRPC, HTTP, UI or consensus. Transport must not depend on model-specific neuron implementations. UI clients depend on generated management contracts, not executor internals. Persist explicit schema DTOs rather than raw in-memory layouts.

There shall be one authoritative implementation of each shared concept, including `LogicalTag`, stable IDs, canonical event ordering, fixed-point arithmetic, deterministic RNG, state digest, permission evaluation and operation state.

## Required product variants

Maintain one versioned product matrix. Each row shall name its entry point, supported host/target triples, capability profile, required toolchain, packaging format, signing/publishing owner, minimum supported operating-system policy and QA suites. At minimum it shall contain:

| Product | Responsibility | Packaging boundary |
|---|---|---|
| Server worker | Authoritative shard execution, replication and transport adapters. | Linux service/container and native server binary. |
| Orchestrator | Control plane, scheduling, management API, fencing and operations. | Linux service/container and native server binary. |
| Workstation | Native management, visualisation and governed A/V/HID/USB-AER gateway. | Supported desktop native packages. |
| Web | Sandboxed management and permitted browser media/input capabilities. | Versioned web assets/WASM where used. |
| iOS | Official signed native application that can run an independent local brain, manage remote brains, visualise, translate mobile sensors/effectors to/from governed AER, discover/enrol nodes and join/leave federations subject to iOS lifecycle and wired-accessory constraints. | Xcode application containing a generated Rust XCFramework and Swift bindings. |
| Android | Official signed native application that can run an independent local brain, manage remote brains, visualise, translate mobile sensors/effectors to/from governed AER, discover/enrol nodes and join/leave federations over supported network/USB modes. | Gradle Android application containing ABI-specific Rust native libraries and Kotlin bindings. |

`server`, `orchestrator`, `workstation`, `web`, `ios` and `android` are product profiles, not mutually inconsistent semantic feature sets. A persisted brain, event, checkpoint, operation or peripheral record shall have one schema across products. Product-specific capability absence must be explicit and safe; it must not select a different logical-time, ownership, ordering or durability rule.

Official mobile applications shall use documented Apple/Google toolchains, code signing, entitlements/permissions, privacy declarations, accessibility, upgrade and store-release processes. Development-only sideloading, jailbreak/DFU exploits, downloaded executable code and private APIs are not production dependencies. A model import is validated data, never executable code.

## Required modular restructuring

First inventory the real workspace and dependency graph with `cargo metadata`; then converge towards the following responsibility layout without creating duplicate implementations merely to match these illustrative names:

```text
aarnn_rust/
├── crates/
│   ├── aarnn-identity/          # stable typed identities only
│   ├── aarnn-time/              # LogicalTag, mappings and ordering
│   ├── aarnn-numeric/           # deterministic profiles, RNG and digests
│   ├── aarnn-events/            # portable envelopes, transitions and DTOs
│   ├── aarnn-model/             # biological model and validated topology
│   ├── aarnn-executor/          # platform-neutral reference/shard execution
│   ├── aarnn-protocol/          # versioned wire/checkpoint schema types
│   ├── aarnn-client/            # generated-contract management client core
│   ├── aarnn-peripheral/        # sessions, samples, effects and capabilities
│   ├── aarnn-node/              # node identity, role and capability contracts
│   ├── aarnn-discovery/         # transport-neutral discovery observations
│   ├── aarnn-enrolment/         # pairing, credentials, renewal and revocation
│   ├── aarnn-aer-transport/     # governed batched AER-over-transport protocol
│   ├── aarnn-federation-client/ # offers, consent and federation-directory client
│   ├── aarnn-mobile-runtime/    # standalone brain, lifecycle and transient edge modes
│   └── aarnn-mobile-ffi/        # generated Swift/Kotlin control bindings
├── adapters/
│   ├── server/                  # Linux/service/storage/network adapters
│   ├── workstation/             # desktop media, HID and USB adapters
│   ├── web/                     # browser capability adapters
│   ├── ios/                     # Rust-facing iOS adapter contracts
│   └── android/                 # Rust-facing Android adapter contracts
├── apps/
│   ├── server/
│   ├── orchestrator/
│   ├── workstation/
│   ├── web/
│   ├── ios/                     # SwiftUI/Xcode project
│   └── android/                 # Kotlin/Compose/Gradle project
├── examples/                         # catalogued cross-product examples
├── qa/                               # scenarios, fixtures, goldens and profiles
├── scripts/qa/                       # thin human/CI wrappers
└── tools/xtask/                      # authoritative build/QA/example orchestration
```

The exact split shall follow measured cohesion and the current workspace, but these dependency rules are mandatory:

1. Identity, time, numeric, event and biological crates contain no UI, HTTP/gRPC implementation, database, operating-system media, USB, CUDA, `libtorch`, Python or application-framework dependency.
2. Platform-neutral crates compile for the documented host, `wasm32` where applicable, `aarch64-apple-ios`, Apple simulator and Android NDK targets without dummy implementations that claim unavailable capabilities.
3. Server accelerator/storage/consensus implementations remain behind traits in server-side adapter crates. A mobile build shall not link container tooling, Python, CUDA, server `libtorch`, desktop file dialogs or Linux-only IPC.
4. Native shells depend inward through generated bindings and portable contracts. Rust core crates never import Swift/Kotlin/Gradle/Xcode concepts.
5. Operating-system conditionals belong in narrow adapter crates. Avoid scattered `cfg(target_os = ...)` branches in biological or deterministic code; enforce this with architecture tests.
6. Shared schemas and bindings have one generator and compatibility fixtures. Generated Swift/Kotlin/Rust outputs are regenerated and diff-checked; they are never edited by hand.
7. Feature flags select replaceable adapters or coarse migration paths, not biological semantics. Maintain tested product feature sets rather than assuming `--all-features` is meaningful across mutually exclusive platforms.
8. Preserve stable public paths with temporary re-exports while callers migrate. Remove each compatibility facade after its recorded rollback window; do not maintain permanent duplicate modules.
9. Every FFI boundary uses fixed-width/versioned DTOs, typed errors, cancellation, bounded buffers and explicit ownership. No panic, borrowed pointer, Rust reference, platform object or executor lock may cross the boundary.
10. Use generated UniFFI Swift/Kotlin bindings for ordinary lifecycle, management and configuration calls unless an ADR proves another maintained binding generator is safer. High-rate A/V/AER/event traffic shall cross in bounded versioned batches through a measured low-overhead ABI; never make one FFI call per biological or USB event.

Introduce a `ComputeBackend` boundary. Start mobile deterministic-reference execution on CPU. Metal, Android GPU or neural-accelerator adapters may be added only after parity, determinism, thermal, memory and transfer benchmarks prove benefit. Do not force AARNN's sparse self-modifying event workload into a static tensor/ML API merely because the accelerator exposes one.

## Mobile runtime and platform boundaries

The official iOS and Android applications shall support four explicit modes:

- `RemoteClient`: management, visualisation and governed sensory/effector streaming to an authoritative orchestrator.
- `ForegroundEdgeWorker`: bounded non-authoritative leaf/sensorimotor shards with leases, checkpoints and deterministic reassignment.
- `StandaloneBrain`: an independent locally authoritative single-device AARNN instance capable of local create/import/run/pause/checkpoint/restore/export and operation without any server.
- `OfflineDemonstrator`: bounded curated local examples using the same `StandaloneBrain` runtime, portable executor and scenario fixtures as host reference tests.

A mobile device may be the authoritative executor/orchestrator and primary checkpoint holder for a brain explicitly created or imported in `StandaloneBrain` mode. That authority is scoped to the local brain generation and shall use encrypted crash-safe local checkpoints, explicit export/backup status and clear warnings when no second durable copy exists. Local standalone execution must not require an account, LAN, WAN or another AARNN instance.

A mobile device lending capacity through `ForegroundEdgeWorker` remains volatile and shall not be quorum-eligible, the sole checkpoint holder or an anchor on which unrelated causal components depend. Federation does not merge the independent ownership, timelines or checkpoint authority of participating brains. On lifecycle transition, network loss, thermal/memory pressure or process termination, each mode shall stop admission safely, checkpoint where permitted, retain explicit gaps and either resume the local brain or follow normal lease expiry/fast-forward/reassignment. It shall not pretend to have executed while suspended or to be a continuously running Linux daemon.

Platform adapters shall implement narrow contracts for lifecycle, monotonic/capture clocks, secure storage, network reachability, camera, microphone, display/screen capability, focused keyboard/touch/pointer input, audio/video presentation, local notifications and supported accessories. Permission denial/revocation is a normal state. Every adapter reports an accurate capability record; an unsupported adapter fails closed and never returns success from a stub.

### Independent standalone mobile brain

`StandaloneBrain` is a production mode, not a demonstration facade. It shall use the same biological model, `LogicalTag`, canonical event ordering, deterministic numerical profile, checkpoint schema and executor contracts as server/workstation instances. Resource scale may differ, but semantics may not. It shall provide:

- local brain create/import/validate/start/pause/resume/stop/reset/clone/checkpoint/restore/export/delete operations through the shared operation model;
- an embedded local orchestrator/executor with one-writer generation/term rules and no dependency on a remote consensus service;
- encrypted application-sandbox persistence, atomic manifest publication, crash recovery, retention/space budgets and visible last-good-checkpoint/export status;
- configurable neuron/synapse/event/queue/checkpoint budgets based on measured device memory, storage and thermal capacity, with admission rejection before resource exhaustion;
- deterministic headless core tests and a foreground UI that remains responsive while the executor works on bounded worker threads;
- explicit local, remote-client, edge-worker and federation identities so that reconnect/import can never accidentally merge two brain generations;
- optional encrypted user-authorised backup/export, but no mandatory cloud service or account;
- clean offline operation, followed by explicit reconnect/federate/export choices rather than automatic state upload.

Suspension does not advance biological time. Before a suspend deadline, the app requests a safe committed boundary and durable checkpoint; if the OS terminates first, crash recovery returns to the most recent published checkpoint and reports the uncommitted interval. Android may use a policy-compliant foreground service for user-visible continued execution. iOS runs continuously only while the application/platform grants execution time; otherwise it checkpoints and suspends honestly.

### Mobile as a governed AER device

Every product may expose a common `AerEndpoint` contract with independently authorised `Producer`, `Consumer` or `Duplex` direction. A mobile endpoint may transform camera frames, microphone blocks, touch, stylus, focused keyboard/pointer, motion/IMU, location or other explicitly consented sensors into versioned derived AER frames, and may consume committed AER output for approved in-app audio/visual/haptic/sandboxed effects. Raw capture and derived AER remain distinct representations.

Define a versioned `AARNN-AER/1` application protocol above transport. It shall include endpoint/session/device epochs, source sequence, capture timestamp and clock-mapping version/uncertainty, direction, address-space/mapping version, polarity/payload type, logical admission provenance where applicable, frame sequence, gap/overflow/quality records, CRC/integrity, credit window, acknowledgement boundary and stable `EffectId` for output. Frames are bounded and batched. Transport reconnection resumes from explicit acknowledged state and never converts arrival time into biological time.

The same protocol shall connect any permitted pair of server, workstation, web, iOS and Android products. Capability negotiation selects a supported path without changing AER semantics:

| Path | Discovery/connection requirement | Production rule |
|---|---|---|
| LAN/Ethernet/Wi-Fi | DNS-SD/mDNS or configured unicast discovery, then mutually authenticated QUIC/TLS; WSS fallback where required. | Preferred local path; permissions and network changes are explicit. |
| WAN/5G | Authenticated rendezvous/federation directory; direct QUIC where reachable; ICE/STUN with TURN or an outbound relay fallback behind NAT/CGNAT; WSS for dial-only/web clients. | 5G is an IP bearer, not a separate semantic protocol. No unauthenticated public listener is required. |
| USB/USB-C | Android USB Host/Accessory, approved native accessory, or IP-over-USB/tethered network when the OS exposes it. | Feature-probe roles, power direction, endpoints and permission; never assume every handset implements every USB mode. |
| Lightning | Approved MFi/External Accessory protocol or user-enabled IP-over-USB path where supported. | An official iOS app shall not use raw private USB/usbmux interfaces, jailbreaks or DFU exploits. If no public path exists, use Wi-Fi/LAN/5G and report wired AER unavailable. |
| Browser/web | Outbound WebTransport/WebRTC data channel or WSS through an authorised gateway/orchestrator. | A normal browser is usually dial-only and cannot advertise mDNS or access arbitrary Lightning/USB endpoints; capability reporting must say so. |

Transport selection uses policy plus measured latency, loss, metering, energy, MTU and cost. A path change—for example Wi-Fi to 5G, LAN QUIC to relay, or USB detach to Wi-Fi—creates a recorded path epoch and resumes the same authenticated logical stream if safety permits. It shall not create a new AER endpoint identity, duplicate effects, renumber historical frames or conceal a gap. Users may disable cellular, relayed, metered, roaming or USB paths independently.

Mobile AER advertising/capture/output is off by default. Enabling discovery does not enable capture; enrolment does not grant AER direction; federation does not arm effectful output. Those remain distinct consent and authorisation gates with visible status and one-action stop.

### Node autodetection, discovery and secure enrolment

Implement discovery as untrusted observation followed by authenticated enrolment. No advertisement alone may become an active node, worker, brain link or federation member.

Use a transport-neutral `DiscoveryObservation` containing only an ephemeral discovery instance ID, protocol/version ranges, node/product class, dial/listen capabilities, capability digest, supported enrolment methods, optional authenticated realm hint, endpoint candidates and expiry. Do not advertise user identity, brain names, stable node ID, raw device serial, public checkpoint metadata or secrets. Rotate local discovery aliases and expire stale observations.

Discovery providers shall include, where the platform permits:

- DNS-SD/mDNS on a local link, using versioned AARNN service types declared in iOS Bonjour entitlements and Android NSD registration;
- configured unicast DNS-SD or an authenticated local orchestrator registry for routed LANs/VLANs where multicast is unavailable;
- Android Wi-Fi Direct/Wi-Fi Aware only behind capability and permission adapters, never as the sole discovery mechanism;
- QR/deep-link/manual code and explicit address fallback for isolated networks;
- an authenticated WAN rendezvous/node directory for opt-in discovery across 5G, NAT and firewalls;
- USB/Lightning attach/accessory observations only after platform permission and protocol negotiation;
- orchestrator-proxied discovery for web clients that cannot browse or advertise local services directly.

Expose four user policies: `Invisible`, `DiscoverLocal`, `DiscoverTrustedRealm` and `DirectoryListed`. Default to `Invisible` until onboarding obtains explicit consent. `DirectoryListed` requires an explicit scope (`private`, named team/project or deliberately public), expiry and revocation control; public discoverability must never be inferred from signing in.

Auto-enrolment is allowed only when policy can prove both node identity and authority. Supported flows may include:

1. Same-user/team automatic enrolment through an authenticated orchestrator-issued, short-lived, single-use token and pre-authorised role/capability policy.
2. Proximity pairing using a QR/deep link or short verification phrase whose transcript is bound to both ephemeral keys and displayed fingerprints.
3. Managed deployment enrolment using an approved device-management/attestation policy.
4. Explicit administrator approval for unknown nodes or elevated worker/AER/federation roles.

Generate a non-exportable device key in Keychain/Secure Enclave where available or Android Keystore, then issue a scoped renewable node credential. Enrolment records node ID, owner/tenant/project, requested and granted roles, product/version, capability digest, expiry, trust evidence and audit event. Implement renewal, rotation, revocation, lost-device handling and re-enrolment. Rate-limit and audit pairing. Reject replayed/expired tokens, identity collisions, capability downgrades and version/schema incompatibility.

After enrolment, autodetected nodes may reconnect automatically within their granted policy. Auto-reconnect does not auto-start capture, arm an actuator, import a brain, lend compute or join a federation unless a separate durable user policy explicitly authorises that exact action and remains revocable.

### Discoverable federated AARNN neural networks

Treat a federation as an authorised link between independently owned brains, not discovery-level clustering and not shared mutable state. Each brain retains its own ID, timeline, numerical profile, executor ownership, logs, checkpoints, permissions and stop/reset semantics.

An opted-in brain may publish a minimal signed `FederationOffer` through local discovery, a trusted-realm directory or the WAN directory. The offer includes an ephemeral listing ID, brain/federation protocol ranges, permitted input/output modalities and abstract receptor/effector schemas, time-mapping modes, capacity/rate/latency envelopes, policy/tenant scope, required trust level, expiry and offer digest. Private biological state, topology, weights, owner identity and raw sensor data are not advertised unless a later separately authorised disclosure requires them.

The federation UI shall let a user:

- opt each brain into or out of local/trusted-directory/public listing independently from node discovery;
- filter compatible offers by trust scope, protocol, modality, latency/cost and policy;
- inspect verified identity/fingerprint, requested directions, timing translation, data classification, resource/rate limits and consequences before consent;
- send, accept, reject, revoke and time-limit an invitation;
- require dual authorisation when brains have different owners/tenants;
- pause, drain and unfederate at an explicit safe boundary while preserving audit, gap/effect-dedupe and replay evidence;
- block identities/scopes and remove cached directory data.

Joining shall create a versioned positive-delay federation link with independent per-direction credits, dedupe, clock/time mapping and failure policy. Reject an unapproved zero-delay federation cycle. Discovery disappearance alone shall not tear down an authenticated live link; credential revocation, consent withdrawal, expiry or explicit unfederation shall. Conversely, a discovered offer never activates a link without the required consent policy.

### iOS application

- Use a native Swift/SwiftUI shell and Xcode as the authoritative application, simulator, signing, archive and release boundary. RustRover may edit/build shared Rust code but does not replace Xcode or Apple's SDK/signing tools.
- Build the portable Rust library for physical ARM64 iOS and supported simulator targets, generate Swift bindings, and package the slices and symbols as an XCFramework consumed by the Xcode project. Generated frameworks belong in build output, not source control, unless an explicit vendoring policy says otherwise.
- Keep camera/microphone/media, lifecycle, Keychain, networking, accessibility and accessory access in reviewed platform adapters with the minimum entitlements and purpose descriptions.
- Use Network framework/Bonjour through the required local-network permission and declared service types for local discovery/listening. Continue to offer QR/manual and authenticated directory discovery when local permission or multicast is unavailable.
- Ordinary iOS background suspension is not permission for continuous brain execution. Use only documented background modes for their declared purposes; otherwise checkpoint a standalone brain or lease-expire a borrowed shard. A remote/federated peer continues independently and sees an explicit disconnect/gap.
- Direct Lightning/USB AER is available only through a documented supported accessory route such as an approved External Accessory/MFi protocol or a platform-exposed IP-over-USB connection. Otherwise report the capability unavailable and use authenticated Wi-Fi/LAN/5G. The SUNSHINE/USBLiter8 DFU exploit board is not part of the official application architecture.
- Store standalone-brain data and enrolment credentials separately; deleting/revoking a remote node identity must not silently delete a local brain, and deleting a local brain must not leave federation/AER credentials active.

### Android application

- Use a native Kotlin/Jetpack Compose shell and Gradle as the authoritative application, test, bundle and signing boundary. Package Rust through supported Android NDK targets and JNI/UniFFI bindings.
- Support `arm64-v8a` for physical devices and the agreed emulator ABI; add other ABIs only when the product matrix, dependency audit and QA lane cover them. Never publish an ABI with an untested or stale Rust library.
- Keep camera/microphone/media, lifecycle, Keystore, network, accessibility and Android USB Host access in reviewed platform adapters. USB permission, detach and reattach must create explicit device-epoch transitions.
- Use Android Network Service Discovery for DNS-SD local discovery and `ConnectivityManager` callbacks for Wi-Fi/cellular/path changes. Wi-Fi Direct/Aware and USB Host/Accessory are optional capability providers, not unconditional requirements.
- A foreground service or scheduled/background worker may be used only when its platform policy and user-visible purpose are genuinely satisfied. Otherwise follow the same checkpoint/lease-expiry path as iOS.
- Support a workstation or hardware endpoint acting as an Android Open Accessory host when the device/product matrix proves the mode; support Android USB Host for attached AER hardware when available. Fall back to authenticated IP transport without claiming generic Android device-mode USB support.
- Store standalone-brain data and enrolment credentials separately; uninstall/clear-data, backup/restore and device-transfer behaviour must be documented and tested.
- Build an Android App Bundle for release; signing material and store credentials stay outside the repository and are used only by protected release workflows.

Both applications shall provide an always-visible connection/execution/capture state, one-action stop/disconnect, per-channel consent, accessible error recovery and safe behaviour when the orchestrator is unreachable. App upgrades shall migrate or reject local configuration/checkpoints explicitly and preserve the ability to reconnect without duplicating events or effects.

## JetBrains IDE implementation workflow

Use JetBrains RustRover for the canonical Cargo workspace and Android Studio for the Gradle application. On macOS, use Xcode alongside RustRover for the iOS project. Codex operating through a JetBrains IDE shall perform the restructuring in the following order:

1. Open the repository root in RustRover, not an individual crate. Select the repository-pinned Rust toolchain, allow Cargo sync to complete, and run the discovered baseline before moving symbols.
2. Create or update the active ExecPlan with a dependency graph, forbidden-dependency inventory, product matrix, public API/re-export migration map, scenario IDs and exact rollback points.
3. Add architecture tests and target compile-smoke jobs that fail when portable crates acquire forbidden server/desktop/platform dependencies.
4. Add `tools/xtask` commands for `doctor`, `build`, `qa`, `examples`, `standalone`, `discover`, `enrol`, `aer-link`, `federate`, binding generation and package verification. `xtask` owns orchestration; IDE configurations, Gradle/Xcode phases and shell scripts call it instead of reimplementing logic.
5. Use JetBrains symbol/reference search and safe Move/Rename refactors to extract identity/time/numeric/event/schema code first. Add temporary re-exports, run the narrow unit suite and inspect the diff after each move.
6. Extract the portable reference executor and client/peripheral contracts. Replace direct OS calls with injected traits and deterministic fakes before adding a new platform implementation.
7. Introduce the local embedded orchestrator, mobile lifecycle state machine and FFI crate. Generate Swift/Kotlin bindings from the same source, add round-trip/error/cancellation tests, then run a minimal `StandaloneBrain` create/run/checkpoint/restart sequence on each simulator/emulator without any server.
8. In Android Studio, open `apps/android`, complete Gradle sync, select the checked-in debug variant and managed emulator, and verify that the Gradle native-build task calls `cargo xtask build --product android`. Do not copy `.so` files manually into source directories.
9. On macOS, select the pinned Xcode/SDK with `xcode-select`, build the XCFramework through `cargo xtask build --product ios`, open `apps/ios` in Xcode, and run the shared scheme/test plan on a simulator. Use a physical registered device for camera, thermal, lifecycle and supported-accessory gates.
10. Add native application features vertically: standalone brain lifecycle/persistence, capability/permission UI, discovery observation, secure enrolment, recorded synthetic AER input/output, live permitted input, output presentation, LAN link, WAN/5G reconnect/relay, supported USB/Lightning path, federation offer/consent/leave, then optional foreground edge execution. After each slice, run the shared scenario on host reference and the affected native target and compare the declared oracle.
11. Store team-safe JetBrains run/debug configurations as versioned project files under `.run/`; keep personal paths, device identifiers, credentials, signing teams and secrets out of them. Provide configurations named at least `QA - Portable`, `Examples - Host`, `Server - Standalone`, `Mobile - Standalone`, `Discovery - LAN Lab`, `AER - Transport Matrix`, `Federation - Local Lab`, `Web - Test`, `Android - Emulator`, `Android - QA`, `iOS - Simulator`, `iOS - QA` and `QA - Available Matrix`.
12. Use compound configurations only for genuinely parallel independent services. Readiness probes, allocated ports and deterministic scenario coordination replace fixed sleeps.
13. Before hand-off, run the target-specific matrix, inspect generated binding/schema diffs, exercise application upgrade/resume, update the ExecPlan and record exact commands plus retained result-bundle paths.

RustRover feature/target selection is a developer convenience, not the product definition. Checked-in manifests, `xtask`, Gradle, Xcode schemes/test plans and CI are authoritative and must reproduce every IDE action from the command line.

## Engineering rules

- Prefer small cohesive crates/modules, traits at volatile boundaries and dependency injection for tests.
- Keep I/O non-blocking. Use bounded queues, explicit backpressure, cancellation, deadlines and structured task ownership.
- Keep USB enumeration, endpoint reads/writes and hot-plug callbacks off UI/render/audio and async control threads. Use narrow asynchronous adapter traits, bounded transfer pools and cancellation-safe device shutdown.
- Use platform network/path callbacks and expiring discovery records; do not battery-drain by scanning, reconnecting or polling continuously. Apply jittered exponential backoff and user metered/roaming/relay policy.
- Parse discovery, enrolment, directory, federation and AER frames as hostile input before allocation, display or connection. Bound counts, TXT/metadata size, addresses, candidates, certificates, frames and outstanding handshakes.
- Keep rendezvous/relay services ignorant of biological payload where end-to-end encryption is feasible; minimise and expire discovery/directory metadata.
- Never hold a lock across `.await`, blocking I/O or a long CPU/GPU kernel.
- Avoid global mutable state, unbounded channels, polling loops, blocking sleeps and spin-lock contention.
- Use deterministic fixtures, fake clocks/transports and deterministic fault injection instead of timing-dependent sleeps.
- Validate external sizes, counts, codecs, schemas and identifiers before allocation or dispatch.
- Use typed errors with context and stable public error codes. Do not swallow transport, storage, authorisation or corruption failures.
- Document units, rounding, overflow, ownership, logical-time meaning, replay behaviour and safety invariants at public boundaries.
- Keep a clear reference implementation before optimisation. Benchmark before adding unsafe code or a complex CPU/GPU path.
- Unsafe code is prohibited unless a dedicated safety comment, invariant proof, focused tests and review justify it.
- Use British professional English in project-authored documentation and user-facing messages.

## Generated files and protocols

Find the authoritative schema or generator before editing generated code. Update `.proto`, OpenAPI or other source schemas first, regenerate clients/servers, and include compatibility fixtures. Use additive protocol evolution, reserve removed fields and provide explicit unknown enum values. Do not hand-edit generated outputs or claim completion while generated files are stale.

Persist schema versions in checkpoints, log segments, messages, manifests and management resources. Document rolling-upgrade compatibility and downgrade limitations before deployment.

Treat `DiscoveryObservation`, enrolment transcript/credential, `NodeCapabilities`, `AARNN-AER` frame/session, path epoch, directory listing and `FederationOffer`/invitation/link as versioned protocols with golden fixtures and downgrade rejection. Service-discovery TXT records are hints pointing to an authenticated handshake, not authoritative schema delivery. Reserve removed fields and cap every extensible collection.

## Example catalogue and unified QA harness

Do not create product-specific demonstration logic. Every runnable example shall be a catalogued scenario using the same validated model/input fixtures, portable runner contract and oracle as QA. Human-facing example commands and automated tests may select different duration/resource profiles, but may not implement separate semantics.

Maintain at least this structure, adapting names only when the discovered repository already has an authoritative equivalent:

```text
examples/
├── catalog.toml
├── models/
├── inputs/
└── configs/
qa/
├── scenarios/        # one versioned manifest per scenario/test ID
├── fixtures/         # immutable or generator-versioned inputs
├── goldens/          # exact digests/events or tolerance policies
├── profiles/         # host, seven-node, web, iOS, Android, hardware
└── results/          # ignored local output; CI retains failures
scripts/qa/
├── doctor.sh
├── run-portable.sh
├── run-examples.sh
├── run-web.sh
├── run-android.sh
├── run-ios.sh
├── run-mobile-standalone.sh
├── run-discovery.sh
├── run-aer-transport.sh
├── run-federation.sh
├── run-network-matrix.sh
├── run-hardware.sh
└── run-matrix.sh
```

The authoritative entry point shall be `cargo xtask qa ...` / `cargo xtask examples ...`. Shell scripts are thin fail-fast wrappers with quoted arguments and no duplicated test-selection or build logic. They shall detect prerequisites, print the resolved product/profile/scenario set, forward filters/seeds, place artefacts in a unique result directory and return non-zero if a required scenario fails or is silently skipped.

Each scenario manifest shall record:

- stable scenario/test ID, title, version and mapped specification requirements/invariants;
- model/config/input fixture digests, deterministic seed and logical stop condition;
- supported/required products, execution modes, target triples and capabilities;
- whether simulator/emulator, physical hardware, network emulation or a real USB-AER device is required;
- resource, queue, latency and wall-time bounds;
- exact, tolerance, safety-state or performance oracle and the reference implementation/profile;
- expected event/state/output digests or an approved golden-generation procedure;
- permitted pre-admission loss/coalescing and required gap/quality records;
- result artefacts required on success and failure.

Every run shall emit a machine-readable result bundle containing toolchain/app/schema versions, Git revision and dirty state, platform/device class without sensitive identifiers, scenario manifest digest, seed, capability report, event/state/output digests, timing/resource metrics, logs, trace/checkpoint references, pass/fail/skip reason and reproduction command. A skip is valid only when the manifest marks the capability optional and the result states the precise reason; required capability absence fails the lane.

Maintain runnable examples covering at least:

| Example ID | Demonstrates | Required lanes |
|---|---|---|
| `EX-001` | Small deterministic neuron/synapse network and canonical state digest. | Host reference, web if supported, iOS simulator and Android emulator. |
| `EX-002` | Zero-delay cascade/SCC settlement and positive-delay feedback. | Host reference plus server single/multi-process. |
| `EX-003` | Bounded non-convergence, marked deferral and replay. | Host reference plus distributed server. |
| `EX-004` | Seven-node many-virtual-shard execution with transport fallback. | Server integration profile. |
| `EX-005` | Multiple isolated brains, scheduling and federation link. | Server/orchestrator plus every management client. |
| `EX-006` | Recorded microphone/camera/focused-input transduction and committed presentation. | Workstation, web, iOS and Android applicable capability profiles. |
| `EX-007` | Synthetic bidirectional USB-AER loopback alongside active A/V/HID. | Host fakes in CI; workstation and Android physical-hardware lane; iOS only when an approved accessory exists. |
| `EX-008` | Mobile remote-client disconnect, resume and operation-stream cursor recovery. | iOS and Android simulator/emulator. |
| `EX-009` | Bounded mobile offline/foreground edge execution, checkpoint and cluster reassignment. | Host oracle, iOS and Android physical-device lane. |
| `EX-010` | Checkpoint/schema/app upgrade and rollback compatibility. | Server, workstation, web storage where used, iOS and Android. |
| `EX-011` | Independent mobile brain create/import/run/checkpoint/terminate/restore/export with no server or network. | Host oracle, iOS and Android simulator/emulator plus physical device. |
| `EX-012` | Phone camera/microphone/touch/IMU translated into AER and consumed by a local standalone brain. | Host recorded-fixture oracle, iOS and Android physical device. |
| `EX-013` | LAN/Wi-Fi autodetection, opt-in enrolment and duplex AER between mobile, workstation and server. | Multi-process host lab, iOS and Android physical-device LAN. |
| `EX-014` | WAN/5G discovery, NAT traversal/relay and path migration while AER is active. | Network-emulated host lab and metered physical-device lane. |
| `EX-015` | USB/USB-C/Lightning capability negotiation and safe fallback to IP transport. | Android Host/Accessory hardware matrix; approved iOS accessory where available; explicit iOS safe-unavailable case. |
| `EX-016` | Discover, invite, dual-authorise, run, pause and leave a positive-delay federation of independent mobile/server brains. | Host federation lab, iOS and Android clients, WAN directory lane. |
| `EX-017` | Dial-only web client connects through a gateway to a discovered/enrolled mobile AER endpoint. | Real browser, mobile simulator/emulator and gateway. |

`EX-001`, `EX-006`, `EX-008`, `EX-011`, `EX-012` and a bounded form of `EX-009` shall be discoverable in the official applications without developer menus. `EX-013`–`017` shall be available through a clearly labelled connectivity/federation lab that defaults to private, synthetic, disarmed and non-public settings. Examples requiring dangerous/effectful output, directory publication, cellular data or physical USB shall require explicit confirmation.

The implementation shall support commands equivalent to the following; discover existing conventions before fixing final spelling:

```bash
cargo xtask doctor --product all-available
cargo xtask examples list
cargo xtask examples run --id EX-001 --product host --profile deterministic-reference
cargo xtask examples run --all --product host
cargo xtask qa run --suite section-21 --product server
cargo xtask qa run --suite io-e2e --product workstation --profile synthetic
cargo xtask qa run --suite mobile-contract --product android --device emulator
cargo xtask qa run --suite mobile-contract --product ios --device simulator
cargo xtask qa run --suite mobile-standalone --product android --device emulator
cargo xtask qa run --suite discovery-enrolment --topology lan-lab
cargo xtask qa run --suite aer-transport --paths lan,wan-relay,usb-available
cargo xtask qa run --suite federation-discovery --directory local-test
cargo xtask qa matrix --available --include-examples
scripts/qa/run-examples.sh --all --product host
scripts/qa/run-android.sh --suite mobile-contract
scripts/qa/run-ios.sh --suite mobile-contract
scripts/qa/run-mobile-standalone.sh --product all-available
scripts/qa/run-network-matrix.sh --synthetic --include-handover
```

Documentation command snippets shall be extracted or registered and executed in CI. If a documented example cannot run automatically, state the hardware/store/signing prerequisite and provide a synthetic contract-equivalent lane.

### Required mobile test extensions

Reuse the full Section 21 catalogue. `UT-TIME-*`, `UT-ID-*`, `UT-NUM-*`, `UT-RNG-*`, `UT-ORDER-*`, `UT-IOTIME-*`, `UT-IOSAMPLE-*`, `UT-EFFECT-*` and applicable causal/non-convergence fixtures shall run unchanged against portable crates. `IO-E2E-001`–`014`, `CT-013`–`016` and `API-013`–`019` shall gain iOS/Android profiles wherever the platform exposes the capability; unsupported capabilities shall execute their explicit safe-unavailable assertion rather than disappear from the report.

Add at least these test IDs to the scenario catalogue and specification traceability table before declaring the mobile products complete:

| Test ID | Scenario | Required result |
|---|---|---|
| `UT-ARCH-001` | Portable dependency architecture. | Forbidden server/desktop/platform imports and reverse dependencies fail the test. |
| `UT-FFI-001` | Swift/Kotlin binding round-trip, cancellation and malformed input. | Versioned values/errors survive exactly; buffers are bounded; panic/invalid handle cannot cross FFI. |
| `UT-MOBLIFE-001` | Mobile lifecycle state machine. | Every transition has one safe admission/lease/checkpoint action; repeated callbacks are idempotent. |
| `UT-MOBCAP-001` | Permission and capability mapping. | Unknown, denied, revoked and unsupported states default deny and never advertise success. |
| `MOB-E2E-001` | `EX-001` on host, iOS and Android. | Committed event/state digest and canonical output sequence equal the host reference. |
| `MOB-E2E-002` | iOS background, suspension, termination and relaunch during edge execution. | No fabricated continuous execution; lease/checkpoint/reassignment is explicit; resume does not duplicate or retimestamp events/effects. |
| `MOB-E2E-003` | Android rotation, background, foreground-service transition and process death. | UI recreation does not recreate runtime ownership; policy-compliant lifecycle recovery is idempotent and bounded. |
| `MOB-E2E-004` | Network loss, address change and orchestrator failover. | Resumable streams continue from acknowledged cursors; stale terms are rejected; no duplicate operation or effect. |
| `MOB-E2E-005` | Camera/microphone/local-network/accessory permission denied then revoked live. | Only the affected channel closes with explicit gap/state; other channels and management remain responsive. |
| `MOB-E2E-006` | Memory warning, thermal throttling, low battery and queue pressure. | Budgets/backpressure act before termination; authoritative state is safe; quality/degraded state is visible; no silent event loss. |
| `MOB-E2E-007` | iOS approved accessory disconnect or unsupported USB-AER capability. | New device epoch/explicit gap when supported, otherwise accurate safe-unavailable UI and network-gateway alternative; no private API/exploit path. |
| `MOB-E2E-008` | Android USB-AER permission, detach/reconnect, stall and overflow while A/V/input are active. | `IO-E2E-013/014` fairness, isolation, epoch, dedupe and responsiveness criteria pass. |
| `MOB-E2E-009` | App upgrade with local configuration/checkpoint and generated-binding/schema change. | Compatible state migrates once; incompatible state is rejected recoverably; rollback boundary is honoured. |
| `MOB-E2E-010` | Offline example reconnects to an existing remote brain. | Local and remote identities/generations cannot merge accidentally; user chooses an explicit import/new-brain/reconnect operation. |
| `MOB-E2E-011` | Two mobile devices, two workstations and multiple brains under load. | Identity, permission, clock, session, sample, effect and checkpoint isolation matches `IO-E2E-007`. |
| `MOB-E2E-012` | Release build/package inspection and first launch. | Correct ABI/framework slices, signatures, entitlements/permissions, privacy declarations, licences, no debug bypass and successful smoke example. |
| `MOB-E2E-013` | Independent mobile brain with all network interfaces disabled. | Create/import/run/checkpoint/forced termination/restore/export succeeds without a server; digest matches host; suspension time does not advance the brain. |
| `AER-E2E-001` | One recorded mobile sensor fixture streams to server, workstation, web-gateway, iOS and Android consumers. | Every path yields the same admitted AER/event digest for the pinned mapping and records transport-only latency differences. |
| `AER-E2E-002` | Duplex AER session migrates LAN→5G relay→USB/IP while an effect acknowledgement is delayed. | Path epochs and any gap are explicit; stream/endpoint identity persists; input is applied once and committed output is never applied twice. |
| `DISC-E2E-001` | LAN DNS-SD autodetection with permission grant, denial, opt-out and stale advertisement. | Only opted-in services appear; denial/invisibility leaks no stable identity; expiry removes stale observations; manual pairing remains available. |
| `DISC-E2E-002` | Routed LAN and WAN/5G nodes behind NAT/CGNAT. | Authenticated registry/rendezvous discovers only scoped listings; direct/ICE/relay selection is observable; public inbound ports are not assumed. |
| `ENROL-E2E-001` | Same-realm auto-enrol, QR/phrase enrol and malicious/replayed advertisement/token. | Only policy-authorised roles enrol; fingerprints bind the transcript; replay/expiry/downgrade/identity collision fails and audits. |
| `ENROL-E2E-002` | Credential renewal, key rotation, lost-device revocation and attempted reconnect. | New credentials preserve authorised node identity; revoked/stale keys cannot discover private listings, connect, stream AER or federate. |
| `USB-E2E-001` | Android Host/Accessory, USB/IP, approved iOS Lightning accessory and unsupported iOS wired case. | Each capability report is accurate; supported paths satisfy `IO-E2E-013/014`; unsupported paths fail safely and offer an authenticated network route. |
| `NET-E2E-001` | Wi-Fi loss, cellular/metered/roaming transition, VPN/interface change and relay failover. | User path policy is honoured, connectivity callbacks drive bounded reconnect, logical timing is unchanged and no busy polling occurs. |
| `FED-E2E-001` | Two independently running brains discover offers, invite, dual-authorise and exchange positive-delay events. | Separate identities/timelines/checkpoints persist; versioned mapping and dedupe pass; unapproved zero-delay cycle is rejected. |
| `FED-E2E-002` | Listing opt-out, invitation expiry, live revocation, drain and unfederate. | Directory listing/cache disappears by policy; no new link forms; live link closes at a safe boundary; effects release/dedupe and audit evidence remain correct. |
| `WEB-E2E-001` | Browser is dial-only and requests a mobile AER endpoint through an authorised gateway. | Browser cannot bypass enrolment/consent or claim local discovery/USB capability; WSS/WebTransport/WebRTC path preserves the AER oracle. |

Simulator/emulator lanes use deterministic fake clocks, media, lifecycle, discovery, directory, enrolment, network path, NAT/relay and USB/accessory adapters. Multi-process network labs use separate namespaces/containers/processes with deterministic DNS-SD, delay/loss/duplication, NAT/CGNAT and relay injection. Physical-device lanes validate real clocks, permissions, interruption, memory/thermal behaviour, camera/microphone, Wi-Fi/cellular handover, signing/upgrade and supported accessories. Do not claim physical, 5G, MFi/Lightning or USB Host/Accessory validation from a simulator result.

## Testing and validation

For each milestone:

1. Add or update a test that demonstrates the missing behaviour before or alongside implementation.
2. Run the narrowest relevant unit/property/integration tests.
3. Run the affected crate/package suite.
4. Run the affected examples through the unified runner and compare the declared host/product oracle.
5. Run applicable workspace formatting, lint, architecture, generated-binding, documentation and schema checks discovered in Phase 0.
6. Run mapped deterministic, multi-node, browser, mobile simulator/emulator, physical-device, chaos, security or performance suites when the milestone changes those contracts.
7. Record exact commands, result-bundle paths and concise evidence in the active ExecPlan.

If supported by the discovered workspace, the host Rust verification baseline includes `cargo fmt --all --check`, Clippy with the repository's agreed deny/warn policy, affected workspace tests and `cargo test --doc`. Do not assume that one host can run `--workspace --all-targets --all-features`: iOS, Android, web, server accelerator and desktop features may be target-specific or mutually exclusive. Phase 0 shall define named product feature sets and `cargo xtask qa matrix` shall run formatting plus the applicable compile/test/lint/doc/schema/binding/package lanes for each product.

The minimum cross-product matrix is:

- portable crates on every supported host plus compile-smoke for web, iOS device/simulator and Android device/emulator targets;
- server/orchestrator unit, deterministic-reference, distributed, durability, chaos, security and package tests;
- workstation unit/integration and synthetic plus controlled physical A/V/HID/USB-AER tests;
- web unit, browser automation, permission/capability, accessibility and production-bundle tests;
- iOS Rust/FFI unit tests, simulator unit/UI/scenario tests, physical-device lifecycle/media/thermal/accessory tests and archive/package inspection;
- Android Rust/FFI and Gradle unit tests, managed-emulator instrumented/scenario tests, physical-device lifecycle/media/thermal/USB tests and App Bundle/package inspection;
- independent mobile standalone-brain parity, offline persistence/crash recovery, resource-pressure and no-network acceptance;
- local/routed/WAN discovery, enrolment, renewal/revocation, malicious-advertisement, privacy/opt-out and stale-directory tests;
- AER transport parity and path-handover tests across LAN/Wi-Fi, WAN/5G relay, supported USB/Lightning and browser dial-only profiles;
- federation listing/invitation/dual-consent, positive-delay exchange, isolation, revocation/drain/unfederate and zero-delay-cycle rejection tests;
- schema, generated binding, example, upgrade/rollback, dependency/licence, SBOM, secret/debug-bypass and documentation-command checks.

Do not invent unavailable features or replace repository-specific commands; record the authoritative command set and pinned toolchain/SDK/NDK versions. A target that cannot be built on the current host is `not-run`, not passed, and must be covered by its designated CI runner before the phase/product gate closes.

Verification proves the mechanism is implemented correctly. Validation separately assesses numerical agreement, event timing and the selected biological/transducer model. Never describe deterministic equivalence as proof of biological adequacy.

## Git and change safety

- Work on a task branch or isolated worktree, not directly on the protected main branch.
- Make small reviewable changes aligned to one ExecPlan milestone.
- Do not use destructive reset/checkout/clean commands on user work.
- Do not amend or squash user commits without explicit instruction.
- Do not deploy, publish, rotate credentials, mutate live brains or perform irreversible migrations unless the user explicitly authorises that action.
- Do not upload an application to App Store Connect/Google Play, change signing identities, register bundle/package identifiers or mutate store metadata without explicit authorisation. Local unsigned/simulator and authorised development-device builds are verification, not publication.
- Before a schema or persistence cutover, document forward migration, rollback boundary, checkpoint compatibility and the point of no return.

## Prohibited shortcuts

Do not:

- align wall-clock timers to simulate biological coherence;
- use a whole-network barrier after every biological tick;
- call timeout, silence, queue emptiness or watermark alone quiescence;
- drop or fabricate unresolved events;
- allow nodes to change fidelity independently;
- retain mutable vector indices as cross-generation identity;
- broadcast replicated layers in the new production path;
- fan workstation commands directly to workers;
- trust client-side permission or optimistic UI state as server truth;
- overwrite checkpoints, reuse stale terms or promote without quorum;
- emit effects from uncommitted or replayed output;
- use packet-arrival time as peripheral biological time;
- bypass peripheral admission by translating USB AER frames directly into a worker/shard buffer;
- make USB AER, audio, video or HID mutually exclusive within a workstation session, share their sequence/clock namespace, or allow one saturated modality to starve the others;
- route raw high-rate media through management status APIs;
- claim ordinary browser code can perform global OS keyboard/pointer actuation;
- claim an official mobile application can run indefinitely in the background, bypass platform lifecycle/policy, use private APIs or depend on a jailbreak/DFU exploit;
- make a transient mobile edge worker quorum-eligible, sole-authoritative or the only holder of a shared/distributed brain's committed state; a local `StandaloneBrain` may deliberately be single-device authoritative only with crash-safe checkpoints and visible single-copy/export status;
- pass individual high-rate events across FFI, expose Rust references/pointers to Swift/Kotlin, or let a panic unwind across FFI;
- copy Rust sources or hand-built native libraries into iOS/Android projects outside the generated build pipeline;
- hide unavailable permissions/accessories behind a successful stub or silently omit a required mobile scenario;
- allow mobile, web, workstation and server profiles to change biological, logical-time, event-ordering or persistence semantics;
- equate discovery with trust, enrolment, compute admission, capture consent, actuator arming or federation membership;
- auto-enrol an unknown advertisement without authenticated scope/policy, or reuse a pairing/enrolment token;
- auto-federate brains solely because they are compatible/discovered, merge their timelines/state, or permit a federation link to bypass dual-owner policy;
- advertise stable personal/device/brain identity, private topology/state or secrets through mDNS or a public directory;
- require inbound public ports on mobile networks, silently use cellular/roaming/relay data, or treat path migration as a new biological clock;
- claim generic raw Lightning/USB access on iOS or generic Android USB device mode when the public platform capability is absent;
- auto-arm effectful output after reconnect or hide capture/actuator state;
- ignore flaky distributed tests instead of correcting deterministic orchestration.

## Definition of done for a change

A change is complete only when:

- its requirement/invariant mapping is recorded;
- implementation, schema, generated outputs and documentation agree;
- tests cover success, error, restart/replay and relevant failure paths;
- exact verification commands pass or a justified blocker is recorded;
- memory, queue, latency and compatibility effects are reported where relevant;
- every affected catalogued example passes its required product lanes against its declared oracle;
- target-specific feature sets compile without forbidden dependency leakage and generated bindings are current;
- the active ExecPlan's progress, discoveries and decision log are current;
- no requirement is represented only by a stub or silently skipped test;
- the diff contains no unrelated changes.

An iOS or Android product milestone additionally requires:

- simulator/emulator and physical-device evidence are distinguished and both required gates are present;
- a complete create/import/run/checkpoint/terminate/restore/export `StandaloneBrain` scenario passes with all networks disabled and matches the host reference oracle;
- lifecycle interruption, permission revocation, offline/reconnect, process death, resource pressure and upgrade/rollback scenarios pass;
- applicable LAN/WAN/Wi-Fi/5G/USB/Lightning AER paths and network transitions have explicit capability evidence, with safe-unavailable evidence for unsupported paths;
- node discovery/enrolment/credential rotation/revocation and federation opt-in/invite/join/drain/leave/opt-out privacy suites pass;
- release packaging contains the correct native slices/ABIs and no debug authentication, test keys, development endpoints or unapproved entitlements/permissions;
- store privacy/security/accessibility metadata, dependency licences, SBOM and user-visible capture/execution/stop behaviour agree with the implementation;
- the same example and deterministic fixtures remain runnable on host for diagnosis.

Every implementation hand-off or pull request shall state:

- Scope: requirements/invariants, decisions and changed files.
- Behaviour: user-visible, logical-time, determinism, durability/failover and security impact.
- Evidence: exact tests/commands, retained QA result bundles, example matrix, performance results and protocol/schema/binding compatibility.
- Delivery: products/targets, flags, packaging, rollout/rollback, documentation, limitations and remaining work.

## Code review rules

Review against the specification and active ExecPlan. Treat causal-ordering errors, multiple-writer paths, event loss, false quiescence, replay divergence, stale-term acceptance, unauthorised management, duplicate external effects, hidden peripheral capture/actuation, unbounded queues, FFI ownership/panic faults, lifecycle-fabricated execution, platform-semantic divergence, forbidden dependency leakage, stale generated bindings, silent scenario skips and persistence/app-upgrade incompatibility as correctness or safety defects rather than stylistic issues.
