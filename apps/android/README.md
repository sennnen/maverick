# Android app

The native Android application owns Android BLE, platform presentation, and the UniFFI binding to
the core. It renders immutable read models and does not contain decoding, timeline, storage, or
analytics logic. Its implementation sequence is defined in the
[platform lane](../../docs/plans/active/platform.md).

The UI specification is the current Aura Android shell from the prior NOOP workspace: Today,
Recovery, Strain, and Sleep hubs, platform-appropriate bottom navigation, and one app-wide settings
sheet. We will copy or rewrite only presentation code that fits this boundary. NOOP's
`AppViewModel`, Room repositories, WHOOP BLE client, analytics, onboarding, Health Connect, ML
assets, services, widgets, and notification machinery are not app dependencies here.

## Building the core for Android

The core is exposed through `mav-ffi`, which builds a shared library plus generated Kotlin. From
`core/`:

    # Build the shared library for an Android ABI (repeat per ABI you ship).
    cargo build -p mav-ffi --target aarch64-linux-android

    # Generate the Kotlin bindings from the built library.
    cargo run -p mav-ffi --features cli --bin uniffi-bindgen -- \
        generate --library target/debug/libmav_ffi.so --language kotlin --out-dir generated/kotlin

That produces `uniffi/mav_ffi/mav_ffi.kt`. Put the built `.so` under the app's `jniLibs/<abi>/` and
add the generated Kotlin (which depends on the `net.java.dev.jna` JNA runtime) to the app source;
the app then calls `coreVersion()` and `runCapture(manifestJson, captureJson)` directly. Wiring the
NDK cross-compile per ABI and the Gradle packaging is a step for the app milestone, not the M0
binding.

`runCapture` returns canonical session and analytics JSON plus one parity hash for each. Hosts should
render availability reasons from the analytics JSON rather than reconstructing capability rules in
Kotlin.

The Rust surface and bindgen step are verified in CI. PL-P3 replaces these manual commands with the
reproducible Android library build used by local development and release CI.
