#!/usr/bin/env bash
# Builds libhorizon_android.so and publishes it as a GitHub Release on Nerion's fork, which the
# launcher's Gradle build pins by tag and SHA-256 (docs/nerion-fork.md).
# Usage: tools/nerion/release-android.sh <n>   -> tag nerion-android-v<n>
set -euo pipefail
n="${1:?usage: release-android.sh <number>}"
tag="nerion-android-v$n"
out="target/nerion-release"
rm -rf "$out"
"$(dirname "$0")/build-android.sh" "$out"
so="$out/arm64-v8a/libhorizon_android.so"
sha=$(sha256sum "$so" | cut -d' ' -f1)
echo "$sha  libhorizon_android.so" > "$out/libhorizon_android.so.sha256"
gh release create "$tag" "$so" "$out/libhorizon_android.so.sha256" \
  --repo feeldev12/horizon --target "$(git rev-parse HEAD)" \
  --title "Nerion Android library $n" \
  --notes "libhorizon_android.so (arm64-v8a) built from $(git rev-parse --short HEAD). SHA-256: $sha"
echo "published $tag sha256=$sha"
