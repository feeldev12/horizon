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

## Sync with upstream

```sh
git fetch upstream
git merge upstream/main        # conflicts, if any, are in the files listed above
cargo check -p horizon-client
tools/nerion/build-android.sh  # the Android library still builds
git push origin main
```

## Build for Android

Needs the NDK (r28+, which aligns to 16 KB pages), `cargo-ndk` and the `aarch64-linux-android`
target. See `tools/nerion/build-android.sh`. Resources come from `horizon-prepare` as on desktop and
go into the app's `files/.local/horizon-client/<version>`.
