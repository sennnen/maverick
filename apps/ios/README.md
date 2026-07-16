# iOS app

The native iOS application owns CoreBluetooth, Apple platform presentation, and the UniFFI binding
to the core. It renders immutable read models and does not contain decoding, timeline, storage, or
analytics logic. Its implementation sequence is defined in the
[platform lane](../../docs/plans/active/platform.md).
The exact runtime, event, action, snapshot, threading, and compatibility rules are in
[the platform contract](../../docs/platform.md).

The app preserves Aura's four hubs—Today, Recovery, Strain, and Sleep—behind its floating glass tab
bar, with one app-wide settings sheet. Its `Maverick/` sources copy only visual Aura code and use a
small `MavStore` for immutable `host-snapshot/v1` values. NOOP's `AppModel`, `Repository`, WHOOP
packages, analytics, onboarding, widgets, HealthKit, and background machinery are not dependencies.

Unavailable metrics remain unavailable. The app does not reuse old scores or manufacture temporary
values: Recovery shows the structured core reason; Strain and Sleep become numeric only when their
Maverick analytics are admitted.

## Building the core for iOS

Install full Xcode, select it with `xcode-select`, install XcodeGen (`brew install xcodegen`), then
run:

    bash tools/platform/build_ios.sh

The script pins the Rust targets, builds arm64 device plus arm64/x86_64 simulator slices, creates
`apps/ios/build/mav-core/MavCore.xcframework`, generates
`apps/ios/build/mav-core/Sources/mav_ffi.swift`, and writes a complete SHA-256 inventory. It replaces
the package as one directory so stale slices cannot survive a rebuild. Generated output is ignored
by Git and must never be edited.

The app target links `MavCore.xcframework` and compiles `mav_ffi.swift`. Product code constructs
`MavRuntime`; `runCapture` remains the debug parity surface.

## Building the app

Run the complete, reproducible app build from the repository root:

    bash tools/platform/build_ios_app.sh

It first rebuilds the ignored core package, then generates `Maverick.xcodeproj` from `project.yml`
and runs the iPhone 16 Pro simulator unit tests. The project itself is committed; run XcodeGen again
after changing the project specification.
