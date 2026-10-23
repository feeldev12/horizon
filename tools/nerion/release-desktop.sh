#!/usr/bin/env bash
# Builds horizon-client for this desktop and adds it to a GitHub Release on Nerion's fork, which
# the launcher pins by URL and SHA-256 (src-tauri/horizon.properties in the launcher).
# Usage: tools/nerion/release-desktop.sh <n>   -> tag nerion-desktop-v<n>
# Run it once per OS (Windows first); each run adds that OS's binary to the same release.
set -euo pipefail
n="${1:?usage: release-desktop.sh <number>}"
tag="nerion-desktop-v$n"
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) os=windows; exe=horizon-client.exe ;;
  Linux) os=linux; exe=horizon-client ;;
  Darwin) os=macos; exe=horizon-client ;;
  *) echo "unsupported OS $(uname -s)" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  x86_64|amd64) arch=x64 ;;
  arm64|aarch64) arch=arm64 ;;
  *) echo "unsupported arch $(uname -m)" >&2; exit 1 ;;
esac
cargo build --profile fast -p horizon-client
out="target/nerion-release-desktop"
rm -rf "$out" && mkdir -p "$out"
name="nerion-nativo-$os-$arch${exe#horizon-client}"
cp "target/fast/$exe" "$out/$name"
sha=$(sha256sum "$out/$name" | cut -d' ' -f1)
echo "$sha  $name" > "$out/$name.sha256"
if gh release view "$tag" --repo feeldev12/horizon >/dev/null 2>&1; then
  gh release upload "$tag" "$out/$name" "$out/$name.sha256" --repo feeldev12/horizon --clobber
else
  gh release create "$tag" "$out/$name" "$out/$name.sha256" \
    --repo feeldev12/horizon --target "$(git rev-parse HEAD)" \
    --title "Nerion desktop client $n" \
    --notes "horizon-client for desktop, built from $(git rev-parse --short HEAD)."
fi
echo "published $tag $name sha256=$sha"
