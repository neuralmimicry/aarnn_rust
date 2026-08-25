# AARNN Android application

This is the checked-in Kotlin/Android shell for the shared Rust implementation.
It targets Android API 34 and the installed emulator profile
`Pixel_3a_API_34_extension_level_7_x86_64`; the product ABI matrix is
`x86_64` for the emulator and `arm64-v8a` for physical devices.

The default debug build is a safe shell and reports the Rust ABI as unavailable
when the native library has not been built. Build the shared Rust library into
Gradle-generated output with:

Open `apps/android` in Android Studio Quail 3 and run the `app` debug
configuration, or use the checked-in Gradle wrapper:

```text
ANDROID_HOME=/home/pbisaacs/Android/Sdk ./gradlew assembleDebug
```

Add `-PwithRust=true` when the Android NDK has been installed and the shared
Rust library should be built as part of the application build.

`-PwithRust=true` requires an Android NDK installed through Android Studio's
SDK Manager. The Gradle task invokes `cargo xtask build --product android` for
both supported ABIs and places
the resulting libraries only under `app/build/generated`; no `.so` files are
checked into source or copied manually into the application. Signing and
release credentials are intentionally absent.

The JNI surface is limited to versioned lifecycle/checkpoint operations. The
checked-in seam also supports bounded checkpoint restore, but no session is
created automatically by the shell. It is not production evidence for Android
lifecycle, thermal, USB Host, camera,
microphone, discovery, enrolment, AER or federation scenarios. Those remain
separate acceptance gates in the mobile ExecPlan.

The shell also has an explicit, read-only remote observation form. For the
development deployment used by the emulator, enter the gateway URL
`http://192.168.1.2`, ingress host `aarnn.neuralmimicry.ai` and authorised
credentials. The client uses `/api/login`, then the workspace-scoped
`/api/runtime/workspaces`, `/activity` and bounded `/topology` endpoints; the
topology response carries a versioned generation, layer metadata, active nodes
and exact non-zero weighted edges for the returned node/edge budget. It never
connects directly to workers or the raw orchestrator port. Passwords are held
only for the request and are not persisted. Cleartext HTTP is enabled only by
the debug manifest for the emulator lane; release manifests reject cleartext
and production requires HTTPS, certificate validation and generated management
client bindings.

The Compose shell opens on a visual Dashboard. Account and connection controls
are kept on a separate bottom-navigation destination with session status,
capability state and privacy guidance. A separate Graph bottom-navigation
destination provides a dense, bounded neural graph like the workstation Graph
Explorer: layer-coloured nodes, exact returned weighted connection lines,
active-node highlighting, pinch/drag camera gestures, zoom and rotation sliders,
and camera reset. It uses the same authorised workspace snapshot as Dashboard
and remains read-only. When disconnected it renders only a clearly labelled
local demonstration projection; when a connected topology projection is
unavailable it shows the nodes without fabricated edges and reports that
limitation.
