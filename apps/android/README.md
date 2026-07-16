# Android app

The native Android application owns Android BLE, platform presentation, and the UniFFI binding to
the core. It renders immutable read models and does not contain decoding, timeline, storage, or
analytics logic. Its implementation sequence is defined in the
[platform lane](../../docs/plans/active/platform.md).
The exact runtime, event, action, snapshot, threading, and compatibility rules are in
[the platform contract](../../docs/platform.md).

The UI specification is the current Aura Android shell from the prior NOOP workspace: Today,
Recovery, Strain, and Sleep hubs, platform-appropriate bottom navigation, and one app-wide settings
sheet. We will copy or rewrite only presentation code that fits this boundary. NOOP's
`AppViewModel`, Room repositories, WHOOP BLE client, analytics, onboarding, Health Connect, ML
assets, services, widgets, and notification machinery are not app dependencies here.

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
