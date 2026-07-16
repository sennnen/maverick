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

Install full Xcode, select it with `xcode-select`, then run:

    bash tools/platform/build_ios.sh

The script pins the Rust targets, builds arm64 device plus arm64/x86_64 simulator slices, creates
`apps/ios/build/mav-core/MavCore.xcframework`, generates
`apps/ios/build/mav-core/Sources/mav_ffi.swift`, and writes a complete SHA-256 inventory. It replaces
the package as one directory so stale slices cannot survive a rebuild. Generated output is ignored
by Git and must never be edited.

The app target links `MavCore.xcframework` and compiles `mav_ffi.swift`. Product code constructs
`MavRuntime`; `runCapture` remains the debug parity surface.
