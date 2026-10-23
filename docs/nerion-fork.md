# Nerion's fork

`feeldev12/horizon` is Horizon plus what [Nerion](https://github.com/feeldev12) needs to ship it as
an optional engine on Android. Upstream is `ludovic111/horizon`; everything that can go upstream is
offered there as a pull request, so this fork's diff stays small.

## What the fork adds

- `crates/horizon-android`: the Android library (`libhorizon_android.so`). Its root is
  `crates/horizon-client/src/android.rs`, which reuses main.rs's `game` module, reads the launch
  arguments from `files/launch.txt`, runs the game from the app's internal files folder and pipes
  stdout/stderr to logcat (tag `horizon`).
- `game.rs`: `LAUNCH_ARGS` (arguments from a platform with no argv) and `new_event_loop()` (the
  event loop on the Android activity).
- `clipboard.rs`: arboard has no Android backend; the game keeps its own copy there.
- `game.rs`/`gfx.rs`: surviving Android's suspend/resume (the surface is dropped and recreated;
  the loading screen waits while suspended).
- `android.rs`: GameActivity, and Nerion's launch channel (the `tok` Intent extra: sockets in
  `<filesDir>/run/<tok>/`, pid and exit code on `ctl`, arguments in `horizon-args.json`).

## Sync with upstream

```sh
git fetch upstream
git merge upstream/main        # conflicts, if any, are in the files listed above
cargo check -p horizon-client
tools/nerion/build-android.sh  # the Android library still builds
git push origin main
```

## Release for the launcher

`tools/nerion/release-android.sh <n>` builds the library and publishes it as the GitHub Release
`nerion-android-v<n>` (the `.so` and its SHA-256). The launcher pins that tag and hash in
`src-tauri/gen/android/app/horizon.properties`.

`tools/nerion/release-desktop.sh <n>` builds `horizon-client` for the computer it runs on and
adds `nerion-nativo-<os>-<arch>[.exe]` (the name players see in Windows) (and its SHA-256) to the release `nerion-desktop-v<n>`;
run it once per OS. The launcher pins each binary in `src-tauri/horizon.properties`.

`tools/nerion/release-facts.sh <version> <n>` publishes `nerion-facts-<version>-v<n>`: the
computed tables of a version already prepared on this computer (`horizon-prepare pack-facts`).
A phone has no Java, so the launcher downloads that zip and runs `horizon-prepare assemble` with
the player's client jar and asset index, which writes the same resources folder `prepare` does
(`crates/horizon-prepare/src/assemble.rs`).

## Build for Android

Needs the NDK (r28+, which aligns to 16 KB pages), `cargo-ndk` and the `aarch64-linux-android`
target. See `tools/nerion/build-android.sh`. Resources come from `horizon-prepare` as on desktop and
go into the app's `files/.local/horizon-client/<version>`.
