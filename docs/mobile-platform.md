# Mobile platform delivery boundary

This repository currently contains the platform-neutral Rust mobile contract,
not signed iOS or Android applications. The shared engine remains the only
biological implementation. Mobile adapters must not change its biological
equations, logical time, event ordering, ownership, persistence or effect
deduplication semantics.

## Current host implementation

`src/mobile_runtime.rs` provides:

- explicit `RemoteClient`, `ForegroundEdgeWorker`, `StandaloneBrain` and
  `OfflineDemonstrator` modes;
- lifecycle transitions that checkpoint before backgrounding and reject local
  execution while backgrounded;
- bounded versioned checkpoint DTOs and restore validation;
- discovery observations that carry no enrolment, trust, compute or federation
  authority; and
- conservative capability reports that represent missing adapters as
  unavailable rather than successful.

The host tests cover the portable contract and are not iOS/Android acceptance.
In particular, the current checkpoint uses the existing engine snapshot adapter
until the shard-owned durable checkpoint format is production-ready.

## Product boundaries

| Product | Required native boundary | Current status |
|---|---|---|
| iOS | SwiftUI/Xcode, ARM64 device and simulator Rust slices, generated Swift bindings, Keychain, Network/Bonjour, reviewed accessory path | No Xcode project or Apple SDK is present in this workspace; not run |
| Android | Kotlin/Gradle shell, `arm64-v8a` and `x86_64` emulator Rust ABI hooks, bounded JNI control seam | Quail 3, API 34 SDK, NDK r27d and Gradle 9.1 build both ABIs; the debug APK installs and launches on the configured emulator. Physical-device and production adapter gates remain open |
| Web | Secure-context browser APIs or authenticated local AER companion; no global HID claim | Existing web client remains a legacy/compatibility path |
| Workstation | Native media, USB AER and effect gateway adapters | Governance reference exists; live hardware evidence is absent |

## Versioned product/target matrix (MOB-2026-08-23-1)

| Product | Entry point and target | Capability profile | Toolchain/package | Owner and minimum OS | QA evidence required |
|---|---|---|---|---|---|
| Server worker | `aarnn_rust --node`; supported Linux host/container targets | Authoritative shard execution, transport and replication | Rust release binary/container | Server/platform owner; deployment profile-defined Linux baseline | Rust workspace, multi-process, fault and durability suites |
| Orchestrator | `aarnn_rust --orchestrator`; supported Linux host/container targets | Quorum control, scheduling, management and fencing | Rust release binary/container | Control-plane owner; deployment profile-defined Linux baseline | Consensus, API security, failover and audit suites |
| Workstation | Rust UI/native gateway on supported desktop targets | Management, visualisation, governed A/V/HID/USB-AER | Native desktop package | Workstation owner; supported desktop OS policy required | Native UI, media, USB-AER and safety suites |
| Web | `web_ui` and versioned web assets in a secure browser context | Sandboxed management and permitted browser media/input | HTTPS web assets/WASM where used | Web owner; supported-browser policy required | Browser automation, permissions, responsiveness and WebUSB/companion suites |
| iOS | Future SwiftUI shell with Rust XCFramework; device and simulator targets | Standalone/remote/edge brain, governed AER and scoped discovery/federation | Xcode app, generated Swift bindings, signed IPA | Mobile owner; declared iOS minimum version required | Simulator plus physical-device lifecycle, thermal, accessory, privacy and store suites |
| Android | Kotlin/Compose shell with ABI-specific Rust libraries; `arm64-v8a` and agreed emulator ABI | Standalone/remote/edge brain, governed AER and scoped discovery/federation | Gradle app, generated Kotlin bindings, signed APK/AAB | Mobile owner; declared Android API baseline required | Emulator plus physical-device lifecycle, thermal, USB Host, privacy and store suites |

The matrix is a planning contract, not a claim that unavailable native
products or their QA lanes already exist. Signing identities, entitlements,
privacy declarations, accessibility review, SBOM and release credentials must
be supplied by the product owners before either mobile row can be promoted.

Mobile lifecycle rules are deliberately conservative. iOS suspension and
Android process death are transient edge-worker loss unless a local standalone
brain has a crash-safe checkpoint. Suspension never advances biological time.
Remote peers continue independently and observe an explicit disconnect/gap.

Discovery uses observations only. Enrolment, AER input, AER output, compute
lending and federation are separate scoped, revocable grants. Local network,
Wi-Fi, cellular, relay and USB path changes must not change capture provenance,
brain logical time or effect identity.

## Required native acceptance

The platform lanes must be added before product completion:

1. Build and test generated bindings and bounded error/cancellation handling.
2. Run `StandaloneBrain` create/import/run/checkpoint/terminate/restore/export
   with all network interfaces disabled and compare the host digest.
3. Exercise permission denial/revocation, lifecycle interruption, process death,
   memory/thermal/battery pressure, upgrade and rollback.
4. Exercise LAN/WAN/relay path changes, discovery, enrolment, credential
   rotation/revocation, AER transport and federation consent.
5. Inspect signed packages for correct ABI/framework slices, entitlements,
   privacy declarations, licences, SBOM and absence of debug bypasses.

The checked-in Android shell is under `apps/android`. Its debug application is
safe to build without native Rust output and visibly reports that capability as
unavailable. `-PwithRust=true` invokes `cargo xtask build --product android`
for both `x86_64` and `arm64-v8a` into Gradle-generated output; native output is
never checked into the application source tree. The shell includes the bounded
JNI lifecycle/checkpoint seam and conservative capability report. The emulator
smoke test proves packaging, native loading and safe UI status only; it is not
standalone-brain, USB, media, discovery, enrolment or federation acceptance.

On `2026-08-23`, the installed debug APK reached the live ingress from
`emulator-5554` using `http://192.168.1.2` and the configured
`aarnn.neuralmimicry.ai` host header. A deliberately invalid login returned
`HTTP 401 invalid_credentials`, proving emulator-to-gateway routing and
authentication error handling. Live workspace/neural display was not claimed:
an authorised runtime credential was not available to this validation lane.
The password field is cleared after submission, and cleartext HTTP is enabled
only in the debug manifest; release builds require HTTPS.

Required unavailable-capability cases must be reported as passing safe-unavailable
assertions only where the scenario marks the capability optional. A simulator
does not prove physical timing, thermal behaviour, cellular handover,
USB Host/Accessory or iOS accessory support.
