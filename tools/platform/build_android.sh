#!/usr/bin/env bash
set -euo pipefail

MAV_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
export MAV_ROOT
source "$MAV_ROOT/tools/platform/lib.sh"

require_command cargo
require_command rustc
require_command rustup

readonly CORE="$MAV_ROOT/core"
readonly BUILD_ROOT="$MAV_ROOT/apps/android/build"
readonly DESTINATION="$BUILD_ROOT/mav-core"
readonly NDK_VERSION="29.0.14206865"
readonly API_LEVEL="${MAV_ANDROID_API_LEVEL:-26}"
readonly SDK_ROOT="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Library/Android/sdk}}"
readonly NDK_ROOT="${ANDROID_NDK_HOME:-$SDK_ROOT/ndk/$NDK_VERSION}"
readonly PREBUILT_ROOT="$NDK_ROOT/toolchains/llvm/prebuilt"

[[ -d "$NDK_ROOT" ]] ||
  die "Android NDK $NDK_VERSION missing; install with sdkmanager \"ndk;$NDK_VERSION\""
[[ -d "$PREBUILT_ROOT" ]] || die "invalid Android NDK: $NDK_ROOT"

TOOLCHAIN="$(find "$PREBUILT_ROOT" -mindepth 1 -maxdepth 1 -type d -print | LC_ALL=C sort | head -n 1)"
[[ -n "$TOOLCHAIN" ]] || die "Android NDK LLVM toolchain missing"

readonly ARM_TARGET="aarch64-linux-android"
readonly ARM_ABI="arm64-v8a"
readonly X86_TARGET="x86_64-linux-android"
readonly X86_ABI="x86_64"
readonly ARM_CLANG="$TOOLCHAIN/bin/aarch64-linux-android${API_LEVEL}-clang"
readonly X86_CLANG="$TOOLCHAIN/bin/x86_64-linux-android${API_LEVEL}-clang"
readonly LLVM_AR="$TOOLCHAIN/bin/llvm-ar"
readonly LLVM_READELF="$TOOLCHAIN/bin/llvm-readelf"
readonly LLVM_STRIP="$TOOLCHAIN/bin/llvm-strip"

[[ -x "$ARM_CLANG" ]] || die "missing Android linker: $ARM_CLANG"
[[ -x "$X86_CLANG" ]] || die "missing Android linker: $X86_CLANG"
[[ -x "$LLVM_AR" ]] || die "missing Android archiver: $LLVM_AR"
[[ -x "$LLVM_READELF" ]] || die "missing Android ELF reader: $LLVM_READELF"
[[ -x "$LLVM_STRIP" ]] || die "missing Android strip tool: $LLVM_STRIP"

mkdir -p "$BUILD_ROOT"
STAGE="$(mktemp -d "$BUILD_ROOT/.mav-core.XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT

rustup target add "$ARM_TARGET" "$X86_TARGET"

(
  cd "$CORE"
  cargo build --locked --release -p mav-ffi --features cli
  CC_aarch64_linux_android="$ARM_CLANG" \
    AR_aarch64_linux_android="$LLVM_AR" \
    CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$ARM_CLANG" \
    CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384" \
    cargo build --locked --release -p mav-ffi --target "$ARM_TARGET"
  CC_x86_64_linux_android="$X86_CLANG" \
    AR_x86_64_linux_android="$LLVM_AR" \
    CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$X86_CLANG" \
    CARGO_TARGET_X86_64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384" \
    cargo build --locked --release -p mav-ffi --target "$X86_TARGET"
)

mkdir -p \
  "$STAGE/Sources" \
  "$STAGE/jniLibs/$ARM_ABI" \
  "$STAGE/jniLibs/$X86_ABI"

HOST_LIBRARY="$CORE/target/release/libmav_ffi.so"
if [[ "$(uname -s)" == "Darwin" ]]; then
  HOST_LIBRARY="$CORE/target/release/libmav_ffi.dylib"
fi

(
  cd "$CORE"
  cargo run --quiet -p mav-ffi --features cli --bin uniffi-bindgen -- generate \
    --library "$HOST_LIBRARY" \
    --language kotlin \
    --out-dir "$STAGE/generated"
)

cp -R "$STAGE/generated/uniffi" "$STAGE/Sources/"
cp "$CORE/target/$ARM_TARGET/release/libmav_ffi.so" \
  "$STAGE/jniLibs/$ARM_ABI/libmav_ffi.so"
cp "$CORE/target/$X86_TARGET/release/libmav_ffi.so" \
  "$STAGE/jniLibs/$X86_ABI/libmav_ffi.so"

"$LLVM_STRIP" --strip-unneeded "$STAGE/jniLibs/$ARM_ABI/libmav_ffi.so"
"$LLVM_STRIP" --strip-unneeded "$STAGE/jniLibs/$X86_ABI/libmav_ffi.so"
for library in "$STAGE"/jniLibs/*/libmav_ffi.so; do
  "$LLVM_READELF" -lW "$library" |
    awk '$1 == "LOAD" { seen = 1; if ($NF != "0x4000") bad = 1 } END { exit (!seen || bad) }' ||
    die "native library is not 16 KiB page aligned: $library"
done

rm -rf "$STAGE/generated"
printf \
  '{"schema":"mav-core-package/v1","platform":"android","profile":"release","api_level":%s,"ndk":"%s","jna":"5.12.0@aar","targets":{"arm64-v8a":"aarch64-linux-android","x86_64":"x86_64-linux-android"}}\n' \
  "$API_LEVEL" \
  "$NDK_VERSION" \
  >"$STAGE/package.json"
write_checksums "$STAGE"
replace_directory "$STAGE" "$DESTINATION"
trap - EXIT

printf '%s\n' "$DESTINATION"
