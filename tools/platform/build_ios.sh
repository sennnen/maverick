#!/usr/bin/env bash
set -euo pipefail

MAV_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
export MAV_ROOT
source "$MAV_ROOT/tools/platform/lib.sh"

require_command cargo
require_command rustc
require_command rustup
require_command xcodebuild
require_command xcrun
require_command lipo

xcrun --sdk iphoneos --show-sdk-path >/dev/null 2>&1 ||
  die "full Xcode is not selected; install Xcode and run xcode-select"
xcrun --sdk iphonesimulator --show-sdk-path >/dev/null 2>&1 ||
  die "iOS Simulator SDK is unavailable in the selected Xcode"

readonly CORE="$MAV_ROOT/core"
readonly BUILD_ROOT="$MAV_ROOT/apps/ios/build"
readonly DESTINATION="$BUILD_ROOT/mav-core"
readonly DEPLOYMENT_TARGET="${MAV_IOS_DEPLOYMENT_TARGET:-17.0}"
readonly DEVICE_TARGET="aarch64-apple-ios"
readonly SIM_ARM_TARGET="aarch64-apple-ios-sim"
readonly SIM_INTEL_TARGET="x86_64-apple-ios"

mkdir -p "$BUILD_ROOT"
STAGE="$(mktemp -d "$BUILD_ROOT/.mav-core.XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT

rustup target add "$DEVICE_TARGET" "$SIM_ARM_TARGET" "$SIM_INTEL_TARGET"

(
  cd "$CORE"
  cargo build --locked --release -p mav-ffi --features cli
  IPHONEOS_DEPLOYMENT_TARGET="$DEPLOYMENT_TARGET" \
    cargo build --locked --release -p mav-ffi --target "$DEVICE_TARGET"
  IPHONEOS_DEPLOYMENT_TARGET="$DEPLOYMENT_TARGET" \
    cargo build --locked --release -p mav-ffi --target "$SIM_ARM_TARGET"
  IPHONEOS_DEPLOYMENT_TARGET="$DEPLOYMENT_TARGET" \
    cargo build --locked --release -p mav-ffi --target "$SIM_INTEL_TARGET"
)

mkdir -p "$STAGE/Sources" "$STAGE/include" "$STAGE/lib/device" "$STAGE/lib/simulator"
(
  cd "$CORE"
  cargo run --quiet -p mav-ffi --features cli --bin uniffi-bindgen -- generate \
    --library "$CORE/target/release/libmav_ffi.dylib" \
    --language swift \
    --out-dir "$STAGE/generated"
)

cp "$STAGE/generated/mav_ffi.swift" "$STAGE/Sources/"
cp "$STAGE/generated/mav_ffiFFI.h" "$STAGE/include/"
cp "$STAGE/generated/mav_ffiFFI.modulemap" "$STAGE/include/module.modulemap"
cp "$CORE/target/$DEVICE_TARGET/release/libmav_ffi.a" "$STAGE/lib/device/libmav_ffi.a"
lipo -create \
  "$CORE/target/$SIM_ARM_TARGET/release/libmav_ffi.a" \
  "$CORE/target/$SIM_INTEL_TARGET/release/libmav_ffi.a" \
  -output "$STAGE/lib/simulator/libmav_ffi.a"

xcodebuild -create-xcframework \
  -library "$STAGE/lib/device/libmav_ffi.a" \
  -headers "$STAGE/include" \
  -library "$STAGE/lib/simulator/libmav_ffi.a" \
  -headers "$STAGE/include" \
  -output "$STAGE/MavCore.xcframework"

rm -rf "$STAGE/generated" "$STAGE/include" "$STAGE/lib"
printf \
  '{"schema":"mav-core-package/v1","platform":"ios","profile":"release","deployment_target":"%s","targets":["aarch64-apple-ios","aarch64-apple-ios-sim","x86_64-apple-ios"]}\n' \
  "$DEPLOYMENT_TARGET" \
  >"$STAGE/package.json"
write_checksums "$STAGE"
replace_directory "$STAGE" "$DESTINATION"
trap - EXIT

printf '%s\n' "$DESTINATION"
