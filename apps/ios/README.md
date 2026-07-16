# iOS app

The native iOS application owns CoreBluetooth, Apple platform presentation, and the UniFFI binding
to the core. It renders immutable read models and does not contain decoding, timeline, storage, or
analytics logic. Its implementation sequence is defined in the
[platform lane](../../docs/plans/active/platform.md).
The exact runtime, event, action, snapshot, threading, and compatibility rules are in
[the platform contract](../../docs/platform.md).

The UI specification is the current Aura iOS shell from the prior NOOP workspace: Today, Recovery,
Strain, and Sleep hubs behind the floating glass tab bar, with one app-wide settings sheet. We will
copy or rewrite only presentation code that fits this boundary. NOOP's `AppModel`, `Repository`,
WHOOP packages, analytics, onboarding, widgets, HealthKit, and background machinery are not app
dependencies here.

## Building the core for iOS

The core is exposed through `mav-ffi`, which builds a static library plus a generated Swift module.
From `core/`:

    # Build the static library for the simulator (add device and cross-arch targets as needed).
    cargo build -p mav-ffi --target aarch64-apple-ios-sim

    # Generate the Swift bindings from the built library.
    cargo run -p mav-ffi --features cli --bin uniffi-bindgen -- \
        generate --library target/debug/libmav_ffi.dylib --language swift --out-dir generated/swift

That produces `mav_ffi.swift`, `mav_ffiFFI.h`, and `mav_ffiFFI.modulemap`. Package the static
library and the modulemap into an `.xcframework` and add the generated Swift file to the app target;
the app then calls `coreVersion()` and `runCapture(manifestJson:captureJson:)` directly. The exact
`.xcframework` packaging is a step for the app milestone, not the M0 binding.

`runCapture` returns canonical session and analytics JSON plus one parity hash for each. Hosts should
render availability reasons from the analytics JSON rather than reconstructing capability rules in
Swift.

The Rust surface and bindgen step are verified in CI. PL-P3 replaces these manual commands with the
reproducible XCFramework build used by local development and release CI.
