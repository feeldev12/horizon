#!/usr/bin/env bash
# Builds libhorizon_android.so for arm64 phones (Nerion's fork, docs/nerion-fork.md).
# Needs ANDROID_NDK_HOME (r28+), cargo-ndk and `rustup target add aarch64-linux-android`.
set -euo pipefail
out="${1:-target/android-libs}"
cargo ndk -t arm64-v8a --platform 29 -o "$out" build --profile fast -p horizon-android
so="$out/arm64-v8a/libhorizon_android.so"
readelf="$(ls "$ANDROID_NDK_HOME"/toolchains/llvm/prebuilt/*/bin/llvm-readelf* | head -1)"
# Play requires 16 KB page alignment on Android 15+ devices.
if "$readelf" -lW "$so" | awk '$1 == "LOAD" { print $NF }' | grep -qv '0x4000'; then
  echo "error: $so is not 16 KB aligned" >&2
  exit 1
fi
echo "built $so"
