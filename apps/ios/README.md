# iOS app

The native iOS application owns CoreBluetooth, Apple platform presentation, and the UniFFI binding
to the core. It renders immutable read models and does not contain decoding, timeline, storage, or
analytics logic. Its implementation sequence is defined in the
[platform lane](../../docs/plans/active/platform.md).
The exact runtime, event, action, snapshot, threading, and compatibility rules are in
[the platform contract](../../docs/platform.md).

The app preserves the Aura UI in `Maverick/UI/`: the four hubs—Today, Recovery, Strain, and Sleep—behind the floating glass
tab bar, one app-wide settings sheet, and the live/trends/reports/journal/diagnostics/timer
surfaces. `Maverick/Model/` supplies Mav-owned adapter stores with the presentation member surface
(`AppModel`, `Repository`, `LiveState`, …); day history, sleeps, and workouts stay empty until the
core serves them. Connector import, approval, lifecycle management, and generic BLE execution are
live; coach/workouts/strength/alarm/history-import destinations remain Aura stand-ins. The legacy
actual data engine—its GRDB store, WHOOP packages, Swift analytics, onboarding, widgets, HealthKit,
and background machinery—is still not a dependency.

Unavailable metrics remain unavailable. The app does not reuse old scores or manufacture temporary
values: Recovery shows the structured core reason; Strain and Sleep become numeric only when their
Maverick analytics are admitted.

## Building the core for iOS

Install Xcode 26 or later, select it with `xcode-select`, install XcodeGen (`brew install xcodegen`), then
run:

    bash tools/platform/build_ios.sh

The script pins the Rust targets, builds arm64 device plus arm64/x86_64 simulator slices, creates
`apps/ios/build/mav-core/MavCore.xcframework`, generates
`apps/ios/build/mav-core/Sources/mav_ffi.swift`, and writes a complete SHA-256 inventory. It replaces
the package as one directory so stale slices cannot survive a rebuild. Generated output is ignored
by Git and must never be edited.

The app target links `MavCore.xcframework` and compiles `mav_ffi.swift`. Product code constructs
`MavRuntime`, inspects exact `.mavconn` bytes before approval, and feeds the closed generic transport
event/action API from CoreBluetooth. No device protocol or connector implementation is linked into
the application.

An optional `MAVConnectorRegistry` Info.plist dictionary supplies HTTPS URL, registry id, root key
id, and base64 Ed25519 public key. The app streams indexes under 1 MiB, caches exact signed bytes and
checkpoint metadata, and delegates refresh, offline restore, and artifact binding to core. An empty
dictionary disables registry discovery without changing direct import.

## Building the app

Run the complete, reproducible app build from the repository root:

    bash tools/platform/build_ios_app.sh

It first rebuilds the ignored core package, then generates `Maverick.xcodeproj` from `project.yml`
and runs the first available iPhone simulator's unit tests.

`project.yml` is the source of truth for the project, and the project itself is not committed —
`Maverick.xcodeproj` is generated and gitignored. It used to be committed, and could not be
regenerated identically anywhere: the spec pulls in `build/mav-core/Sources`, which the core build
writes and Git ignores, so the same spec produced one project before that build and a different one
after. The build script regenerates it every run, so nothing ever read the committed copy anyway.

To open the app in Xcode without a full build, run `xcodegen generate` in `apps/ios`. That works on
a fresh clone, before the core has ever been built; the *build* still needs the core, and says so.

Set `MAV_BUILD_RELEASE=1` to also produce an unsigned generic-device Release app at
`build/release/Build/Products/Release-iphoneos/Mav.app`. CI archives it and publishes an
`edge-<commit>` prerelease with Android's signed release APK after pushes to `main`.
