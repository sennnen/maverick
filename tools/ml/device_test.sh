#!/usr/bin/env bash
# Build, install and run the model-zoo instrumented tests on the attached device.
#
# The build has to be checked before the install, not after. Installing an APK from a failed
# build silently reruns the previous binary and reports its result as though it were the new
# one, which is how a fix that did not compile was measured twice and believed.
#
#   tools/ml/device_test.sh [test-class-or-method]
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$here/apps/android"

# Gradle's Kotlin DSL cannot parse this host's JDK 25 version string, so the toolchain is
# pinned to the 17 the project targets anyway.
export JAVA_HOME="${MAV_JDK:-/opt/homebrew/Cellar/openjdk@17/17.0.19/libexec/openjdk.jdk/Contents/Home}"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/29.0.14206865}"

target="${1:-com.sennnen.mav.ModelZooParityInstrumentedTest}"

echo "== build =="
./gradlew :app:assembleDebug :app:assembleDebugAndroidTest 2>&1 | grep -E "^e:|BUILD|FAILURE" || true
# The pipe above swallows gradle's exit status, so ask again rather than trusting the log.
./gradlew -q :app:assembleDebug :app:assembleDebugAndroidTest >/dev/null

echo "== install =="
adb install -r app/build/outputs/apk/debug/app-debug.apk | tail -1
adb install -r app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk | tail -1

echo "== run $target =="
adb shell am instrument -w -e class "$target" \
    com.sennnen.mav.debug.test/androidx.test.runner.AndroidJUnitRunner
