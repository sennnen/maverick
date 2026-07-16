# Android app

The native Android application owns Android BLE, platform presentation, and the UniFFI binding to
the core. It renders immutable read models and does not contain decoding, timeline, storage, or
analytics logic. Its implementation sequence is defined in the
[platform lane](../../docs/plans/active/platform.md).
The exact runtime, event, action, snapshot, threading, and compatibility rules are in
[the platform contract](../../docs/platform.md).

The UI is the NOOP Aura compose UI, copied file-for-file into `ui/aura/` (package rename only):
Today, Recovery, Strain, and Sleep hubs, the Material bottom navigation, one app-wide settings
sheet, and the trends/reports/metric-detail/workout-detail/timer surfaces. A Mav-owned
`AppViewModel` adapter exposes NOOP's member surface over the core's `host-snapshot/v1`; day
history, workouts, and ML signals stay empty until the core serves them, and the routed legacy
destinations (live console, devices, coach, journal, …) are same-signature Mav stand-ins so
`AuraRoot.kt` stays byte-identical to its source. NOOP's actual data engine—its Room repositories,
WHOOP BLE client, importers, ML assets, services, widgets, and notification machinery—is still not
an app dependency.

## Build and test

Use JDK 17 and set `ANDROID_HOME` or `ANDROID_SDK_ROOT`, then run:

    ./gradlew lintDebug testDebugUnitTest assembleDebug assembleRelease

Gradle builds the Rust package first, compiles the generated Kotlin binding, runs the strict host
snapshot decoder tests, and emits `app/build/outputs/apk/debug/app-debug.apk`. The APK contains only
the shipped arm64-v8a and x86_64 libraries. The same gate builds the minified release variant through
R8; signing is reserved for the release-candidate workflow. The launch surface is the Aura shell;
the strict snapshot-decoder tests and the ported NOOP helper tests (logical day, widget anchor,
stage segments, zone parsing) run in the same gate.

## Building the core for Android

Install the pinned NDK once:

    sdkmanager "ndk;29.0.14206865"

Then run:

    bash tools/platform/build_android.sh

The script builds API 26 arm64-v8a and x86_64 shared libraries, generates Kotlin, and writes
`apps/android/build/mav-core` with a complete SHA-256 inventory. Generated output is ignored by Git
and must never be edited.

Gradle adds `apps/android/build/mav-core/Sources` as a source directory,
`apps/android/build/mav-core/jniLibs` as its native library directory, and exactly
`net.java.dev.jna:jna:5.12.0@aar`. UniFFI requires JNA 5.12.0 or newer; Maverick pins the documented
minimum until a measured reason justifies changing it.
