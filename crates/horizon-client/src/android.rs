//! Android entry point (horizon-android). `NativeActivity` loads this library and calls
//! `android_main`, which runs the same game as main.rs from the app's internal files folder.
//! Launch arguments come from `launch.txt` there, written with `adb shell run-as`.
#![cfg(target_os = "android")]
mod game;

use horizon_client::{crash, logging};
use winit::platform::android::activity::AndroidApp;

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    redirect_stdio_to_logcat();
    let files = app
        .internal_data_path()
        .expect("the app has no internal files directory");
    // SAFETY: nothing else runs yet; the game reads HORIZON_HOME for its home.
    unsafe { std::env::set_var("HORIZON_HOME", &files) };
    let _ = std::env::set_current_dir(&files);
    let args = std::fs::read_to_string(files.join("launch.txt"))
        .unwrap_or_default()
        .split_whitespace()
        .map(String::from)
        .collect();
    let _ = game::LAUNCH_ARGS.set(args);
    let _ = game::ANDROID_APP.set(app);
    logging::init(&files, "Render thread");
    crash::install(files.clone(), "client");
    if let Err(error) = game::run() {
        log::error!("Horizon: {error}");
        crash::report_error(&files, "client", &*error);
    }
    logging::flush();
}

/// Rust's stdout and stderr go nowhere on Android: pipe them into logcat under `horizon`.
fn redirect_stdio_to_logcat() {
    use std::io::{BufRead, BufReader};
    use std::os::fd::FromRawFd;
    #[link(name = "log")]
    unsafe extern "C" {
        fn __android_log_write(prio: i32, tag: *const u8, text: *const u8) -> i32;
    }
    let mut fds = [0; 2];
    // SAFETY: plain libc calls on descriptors this function owns.
    unsafe {
        if libc::pipe(fds.as_mut_ptr()) != 0 {
            return;
        }
        libc::dup2(fds[1], 1);
        libc::dup2(fds[1], 2);
    }
    let read = unsafe { std::fs::File::from_raw_fd(fds[0]) };
    std::thread::spawn(move || {
        for line in BufReader::new(read).lines().map_while(Result::ok) {
            let text = std::ffi::CString::new(line).unwrap_or_default();
            // SAFETY: both strings are NUL-terminated.
            unsafe { __android_log_write(4, c"horizon".as_ptr().cast(), text.as_ptr().cast()) };
        }
    });
}
