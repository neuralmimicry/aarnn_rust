# AARNN iOS video input surface

`AarnnVideoInputView.swift` is the portable SwiftUI source for the eventual
Xcode application. It uses the public Photos picker for `video-file` and
`AVCaptureSession` for `camera`, with an explicit sheet acting as the pop-out
preview window. `AarnnRemoteSession.swift` supplies the corresponding
authenticated remote session: it logs into the configured AARNN gateway,
retains its secure session cookie, and resolves the shared System Neural
Network under its `system` owner before requesting topology data. The source
names match `src/ui.rs`, `web_ui/app.js`, Android and the CLI.

This checkout does not currently contain an Xcode project or generated Rust
XCFramework. The signed application, entitlements, privacy strings and
generated bindings remain the iOS packaging gate described by the Phase 8
ExecPlan.
