# Cross-platform mobile delivery track

This plan implements the mobile extension through the existing nine phases. It
does not create a second Rust workspace or duplicate biological semantics.
Native application, signing and physical-device work remains gated by the
availability of Xcode/Apple SDKs, Android Studio/SDK/NDK, devices and protected
release credentials.

## Traceability

- Repository authority: `AGENTS.md`,
  `docs/specifications/distributed-whole-brain-emulator-v1.1.md`, and the
  phase plans in `docs/execplans/`.
- New application invariants: `APP-INV-001` through `APP-INV-006`.
- Host evidence: `tests/mobile_contract.rs` and the unit tests in
  `src/mobile_runtime.rs`.
- Native scenarios: `UT-ARCH-001`, `UT-FFI-001`, `UT-MOBLIFE-001`,
  `UT-MOBCAP-001`, `MOB-E2E-001` through `MOB-E2E-013`, `AER-E2E-001`/`002`,
  `DISC-E2E-001`/`002`, `ENROL-E2E-001`/`002`, `USB-E2E-001`, `NET-E2E-001`,
  `FED-E2E-001`/`002` and `WEB-E2E-001`.

## Progress

- [x] `2026-08-23` Re-read the mobile additions to `AGENTS.md` and the
  normative mobile, discovery, enrolment, AER, lifecycle, FFI, product-matrix
  and acceptance requirements in the specification.
- [x] `2026-08-23` Added the platform-neutral `mobile_runtime` contract with
  explicit modes, lifecycle/checkpoint behaviour, discovery observations and
  safe-unavailable capability reports. Added host tests for `MOB-E2E-013`,
  `UT-MOBLIFE-001` and `UT-MOBCAP-001`.
- [x] `2026-08-23` Added the mobile product boundary and native acceptance
  requirements in `docs/mobile-platform.md`.
- [x] `2026-08-23` Revalidated the portable contract with `cargo test
  --locked --test mobile_contract` (4 passed), `cargo test --locked --workspace`
  (all workspace tests passed), and the sequential format/check/all-target,
  documentation and stable-Clippy commands recorded in the active plan.
- [x] `2026-08-23` Confirmed the host all-feature build regenerates the tonic/
  prost Rust output from `proto/distributed.proto`; no checked-in generated
  Swift/Kotlin management output exists to become stale.
- [!] `2026-08-23` The iOS application, production-generated Swift/Kotlin
  bindings, platform adapters, signing/package inspection, physical-device
  lifecycle/media/thermal/accessory tests, discovery/enrolment, federation and
  store evidence remain incomplete. Android native production adapters and
  physical-device evidence are also incomplete; the Android debug/reference
  packaging and emulator smoke gate are recorded below.
- [x] `2026-08-23 10:54Z` Re-ran the final host verification sequentially;
  formatting, all-feature compilation, workspace tests, documentation tests and
  stable Clippy passed. This confirms the portable mobile seam and protobuf
  generation remain build-safe, but does not change the native-platform
  blocker above.
- [x] `2026-08-23 11:54Z` Verified Android Studio Quail 3
  (`2026.1.3 Patch 1`) at `/snap/android-studio/241`, API 34 SDK/build-tools,
  emulator 37.1.11, the configured
  `Pixel_3a_API_34_extension_level_7_x86_64` AVD, Rust Android targets and NDK
  r27d. Added the Kotlin shell, conservative capability/lifecycle controller,
  bounded JNI lifecycle/checkpoint surface, Gradle 9.1 wrapper and
  `cargo xtask build --product android` dual-ABI hook.
- [x] `2026-08-23 11:54Z` `cargo xtask` compiled
  `x86_64-linux-android` and `aarch64-linux-android`; Gradle
  `assembleDebug -PwithRust=true` packaged both `x86_64` and `arm64-v8a`
  libraries. `testDebugUnitTest` passed (3 tests), and the APK installed and
  launched on the emulator with ABI version 1 and all non-reference
  capabilities visibly gated. This is packaging/emulator smoke evidence, not
  physical-device or production feature acceptance.
- [x] `2026-08-23 12:07Z` Final sequential Rust verification passed:
  `git diff --check`, `cargo fmt --all --check`,
  `cargo check --locked --all-features --all-targets`,
  `cargo test --locked --workspace`, `cargo test --locked --workspace --doc`
  and `cargo +stable clippy --workspace --all-targets --all-features`.
  The all-feature build regenerated the protobuf output successfully.
- [x] `2026-08-23 12:17Z` Final Android lane recheck passed
  `./gradlew testDebugUnitTest --no-daemon` and Rust-enabled
  `assembleDebug -PwithRust=true --no-daemon`; the APK reinstalled and launched
  on `emulator-5554`, with ABI 1 visible and 9/10 capabilities unavailable or
  gated. This remains emulator/reference evidence: generated production
  management bindings, physical-device lifecycle/thermal/USB, live AER/media,
  enrolment, federation, signing and store evidence remain blocked.
- [x] `2026-08-23 13:35Z` Reinstalled the rebuilt debug APK on `emulator-5554`
  and confirmed the Compose remote screen, Rust ABI availability and gateway
  form. A live invalid-credential probe reached `192.168.1.2` with ingress host
  `aarnn.neuralmimicry.ai` and returned `HTTP 401 invalid_credentials`; this
  validates routing and fail-closed authentication handling. The authorised
  credential required to display the live `neuralmimicry-shared-snn` workspace
  was not available in the repository or workstation environment, so live
  neuron/activity display remains unclaimed. Release cleartext is now disabled;
  only the debug manifest enables the emulator HTTP lane.
- [x] `2026-08-23` Reworked the Android Compose shell around standard bottom
  navigation with Dashboard and Account destinations. Dashboard is visual-first
  with a graphical empty state, status hero, metric cards, bounded topology
  projection, layer activity bars and distributed-node badges. Account isolates
  credentials and connection actions and adds session, capability and privacy
  controls. Material navigation icons include content descriptions; the remote
  client/controller and read-only authority boundary are unchanged.
- [x] `2026-08-23` Added a dedicated Android Graph Explorer bottom-navigation
  destination. It renders a bounded dense layered projection from the same
  authorised workspace snapshot, highlights reported active neurons, and
  supports pinch/drag pan, pinch zoom, rotation, explicit zoom/rotation sliders
  and reset-camera. Dashboard's topology card links to the new destination.
  Emulator evidence confirms the selectable screen and offline demonstration
  rendering; authorised live workspace credentials were not available for this
  lane, so live Android neural display remains unclaimed.
- [x] `2026-08-23` Added the authenticated workspace topology endpoint and
  Android client consumption. The response is versioned and workspace-scoped,
  bounded by node/edge budgets, and includes exact non-zero weighted matrix
  edges plus active-node state. Graph Explorer renders those edges; its
  disconnected demonstration remains explicitly labelled and connected
  sessions no longer fabricate edges when the authoritative projection is
  unavailable. Runtime integration and Android build tests pass; live
  authorised workspace and local-native topology evidence remain unclaimed.
- [x] `2026-08-23 17:33Z` Preserved additive gateway compatibility in the
  Android client: a missing topology route is represented as explicit topology
  unavailability while authentication, transport and malformed responses still
  fail closed. Connected sessions therefore never receive fabricated edges.
- [x] `2026-08-23 17:39Z` Revalidated Android with the installed Android Studio
  JBR and SDK using the exact scoped command
  `JAVA_HOME=/snap/android-studio/current/jbr ANDROID_HOME=/home/pbisaacs/Android/Sdk
  ANDROID_SDK_ROOT=/home/pbisaacs/Android/Sdk
  PATH=/snap/android-studio/current/jbr/bin:/home/pbisaacs/Android/Sdk/platform-tools:$PATH
  ./gradlew testDebugUnitTest assembleDebug --no-daemon`; the build passed.
  The default shell remains Java 8 without SDK variables, so this is explicit
  workstation reference evidence rather than a global environment assumption.

## Phase mapping and next implementation order

1. Phase 0: add product/target/toolchain matrix, `cargo xtask` QA orchestration,
   scenario manifests and architecture/dependency checks.
2. Phases 1–2: extract portable identity/time/numeric/event/executor crates and
   keep mobile local execution bounded until shard ownership is accepted.
3. Phases 3–6: integrate stable ownership, causal transport, durable cuts and
   lifecycle loss as transient edge failure; no platform API enters these crates.
4. Phase 7: generate management/client contracts and add authenticated
   discovery, enrolment, credential renewal and revocation.
5. Phase 8: implement iOS and Android adapters, standalone local brain,
   capability/permission UI, AER-over-transport, LAN/WAN path changes,
   federation consent and native packaging.
6. Final gate: run every applicable mobile scenario, distinguish simulator from
   physical evidence, complete upgrade/rollback and remove compatibility paths
   only after the shared production gates pass.

## Rollback and blockers

The mobile contract is additive and has no production default. A missing native
adapter reports `Unavailable`; it does not emulate unavailable hardware. Local
standalone checkpoint/restore is the only host-safe mobile behaviour currently
implemented. Remote, edge, AER, federation and management cutovers continue to
use the existing disabled feature flags and legacy rollback paths described in
`docs/production-blocker-runbook.md`.

## Validation and acceptance

Host evidence covers `MOB-E2E-013`, `UT-MOBLIFE-001`, `UT-MOBCAP-001` and
discovery-observation isolation in `tests/mobile_contract.rs`. Android evidence
now additionally covers `testDebugUnitTest`, both Rust ABI builds, Gradle
packaging, emulator install/launch/UI status and the selectable Graph Explorer
screen. These prove bounded local checkpoint/lifecycle contracts, reference
packaging and Graph Explorer rendering only; background execution, network
enrolment, AER, federation, performance, privacy, store and physical-device
acceptance remain blocked. The emulator screenshot is offline demonstration
evidence unless an authorised workspace login has succeeded.

## Rollout, compatibility and rollback

The mobile contract is additive and uses no production default. Local
standalone checkpoint/restore is host-safe; unsupported adapters report
`Unavailable`. Remote, edge-worker, AER, federation and management cutovers
remain behind the existing disabled phase flags. Rollback is therefore to the
existing host/legacy paths and does not require a mobile checkpoint format
change.

## Risks and mitigations

- Do not infer trust, enrolment, compute, AER or federation from discovery.
- Do not advance biological time during iOS suspension or Android process loss.
- Do not claim unavailable USB, background or global-actuation capabilities.
- Keep native bindings generated from the authoritative management schema once
  Phase 7 supplies that schema; do not hand-edit generated outputs.

## Surprises & Discoveries

The repository contains one primary Rust package plus an exporter dependency and
a host-only `tools/xtask` workspace member. It now contains the Android shell
under `apps/android`, but no iOS project. The portable seam and Android JNI
bootstrap remain bounded adapters until Phases 1–8 provide the required
ownership, management, transport, durability and I/O gates.

## Decision Log

- `2026-08-23 MOB-001`: keep mobile support in the shared Rust implementation
  and expose only platform-neutral contracts. Authority: updated `AGENTS.md`
  product matrix and mobile runtime requirements.
- `2026-08-23 MOB-002`: keep all native and effectful capabilities unavailable
  until their platform/device evidence exists. Authority: `APP-INV-002`–`006`
  and Sections 16, 17 and 21 of the specification.

## Outcomes & Retrospective

Portable host contracts are implemented and verified. Official iOS/Android
delivery, generated bindings, native lifecycle/media/accessory operation,
enrolment and federation remain external-gated work and are not claimed as
complete.
