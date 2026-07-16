# Android shell

The thin native layer for Android: BLE stack ownership, the UniFFI binding to the core, and
eventually the UI that renders snapshots. Deliberately kept as small as possible; anything worth
getting wrong belongs in `core/`.

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

The Rust surface and the bindgen step are verified in CI. Linking into an emulator build needs the
Android toolchain and is a local step until the app milestone.
