#!/usr/bin/env bash
# Publishes the computed tables of a prepared version (horizon-prepare pack-facts) as a GitHub
# Release on Nerion's fork. The launcher downloads that zip, pinned by tag and SHA-256, and runs
# `horizon-prepare assemble` on the phone with the player's own client jar: no Java there.
# Run a full `horizon-prepare --prepare` for the version first.
# Usage: tools/nerion/release-facts.sh <minecraft-version> <n>   -> tag nerion-facts-<version>-v<n>
set -euo pipefail
version="${1:?usage: release-facts.sh <minecraft-version> <number>}"
n="${2:?usage: release-facts.sh <minecraft-version> <number>}"
tag="nerion-facts-$version-v$n"
resources=".local/horizon-client/$version"
out="target/nerion-facts"
rm -rf "$out"
mkdir -p "$out"
cargo build --release -p horizon-prepare
zip="$out/horizon-facts-$version.zip"
target/release/horizon-prepare pack-facts --resources "$resources" --out "$zip"
sha=$(sha256sum "$zip" | cut -d' ' -f1)
echo "$sha  horizon-facts-$version.zip" > "$zip.sha256"
gh release create "$tag" "$zip" "$zip.sha256" \
  --repo feeldev12/horizon --target "$(git rev-parse HEAD)" \
  --title "Nerion prepared tables $version ($n)" \
  --notes "Computed tables for Minecraft $version from $(git rev-parse --short HEAD), for horizon-prepare assemble. SHA-256: $sha"
echo "published $tag sha256=$sha"
