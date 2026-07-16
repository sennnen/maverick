# iOS shell

The thin native layer for iOS: CoreBluetooth ownership, the UniFFI binding to the core, and
eventually the UI that renders snapshots. Deliberately kept as small as possible; anything worth
getting wrong belongs in `core/`.

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

The Rust surface and the bindgen step are verified in CI. Linking the framework into a simulator
build needs Xcode and is a local step until the app milestone.
