# Android app

The native Android application owns Android BLE, platform presentation, and the UniFFI binding to
the core. It renders immutable read models and does not contain decoding, timeline, storage, or
analytics logic. Its implementation sequence is defined in the
[platform lane](../../docs/plans/active/platform.md).
The exact runtime, event, action, snapshot, threading, and compatibility rules are in
[the platform contract](../../docs/platform.md).

The UI preserves the Aura compose presentation layer in `ui/aura/`:
Today, Recovery, Strain, and Sleep hubs, the Material bottom navigation, one app-wide settings
sheet, and the trends/reports/metric-detail/workout-detail/timer surfaces. A Mav-owned
`AppViewModel` owns a byte-oriented connector manager. Android's document picker, VIEW/SEND intents,
content URIs, and HTTPS downloads converge on core inspection before approval; a generic
BluetoothGatt executor drives the closed event/action host. Day history, workouts, and ML signals
stay empty until the core serves them, and the remaining routed destinations are honest Mav
stand-ins. The legacy data engine—its Room repositories, device-specific BLE client, importers, ML
assets, services, widgets, and notification machinery—is not an app dependency.

## Build and test

Use JDK 17 and set `ANDROID_HOME` or `ANDROID_SDK_ROOT`, then run:

    ./gradlew lintDebug testDebugUnitTest assembleDebug assembleRelease

Gradle builds the Rust package first, compiles the generated Kotlin binding, runs connector
acquisition/approval/transport and presentation tests, and emits
`app/build/outputs/apk/debug/app-debug.apk`. The APK contains only
the shipped arm64-v8a and x86_64 libraries. The same gate builds the minified release variant through
R8; local release builds remain unsigned. CI pushes to `main` decode and use the Android release
keystore, verify its signature with `apksigner`, and publish the signed APK. Required GitHub Actions
secrets are `ANDROID_KEYSTORE_BASE64` (single-line Base64 JKS), `ANDROID_KEYSTORE_PASSWORD`,
`ANDROID_KEY_ALIAS`, and `ANDROID_KEY_PASSWORD`. The launch surface is the Aura shell;
the frozen decoder tests and presentation helper tests (logical day, widget anchor, stage segments,
zone parsing) run in the same gate.

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
