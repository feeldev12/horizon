//! Android entry point (horizon-android). `GameActivity` loads this library and calls
//! `android_main`, which runs the same game as main.rs with `<filesDir>/horizon` as its home.
//!
//! Started by Nerion's launcher, the activity's Intent carries `tok`: the launcher has bound four
//! unix sockets in `<filesDir>/run/<tok>/` (`ctl`, `in`, `out`, `err`, the protocol its Java game
//! process uses). The game connects them in that order, reports its pid on `ctl`, sends stdout and
//! stderr to `out`/`err`, reads its arguments from `horizon-args.json` there, and writes its exit
//! code on `ctl` before leaving. Without `tok` (started by hand) the arguments come from
//! `<home>/launch.txt` and the output goes to logcat under `horizon`.
#![cfg(target_os = "android")]
mod game;

use std::os::fd::{AsRawFd, FromRawFd};
use std::path::Path;
use std::time::Duration;

use horizon_client::{crash, logging};
use winit::platform::android::activity::AndroidApp;

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    let files = app
        .internal_data_path()
        .expect("the app has no internal files directory");
    let home = files.join("horizon");
    let _ = std::fs::create_dir_all(&home);
    let launch = intent_extra(&app, "tok").and_then(|tok| match Launch::connect(&files, &tok) {
        Ok(launch) => Some(launch),
        Err(e) => {
            log_to_logcat(&format!("Horizon: could not reach the launcher: {e}"));
            None
        }
    });
    let args = match &launch {
        Some(launch) => launch.args.clone(),
        None => {
            redirect_stdio_to_logcat();
            std::fs::read_to_string(home.join("launch.txt"))
                .unwrap_or_default()
                .split_whitespace()
                .map(String::from)
                .collect()
        }
    };
    // SAFETY: nothing else runs yet; the game reads HORIZON_HOME for its home.
    unsafe { std::env::set_var("HORIZON_HOME", &home) };
    let _ = std::env::set_current_dir(&home);
    let _ = game::LAUNCH_ARGS.set(args);
    let _ = game::ANDROID_APP.set(app);
    logging::init(&home, "Render thread");
    crash::install(home.clone(), "client");
    let code = match game::run() {
        Ok(()) => 0,
        Err(error) => {
            log::error!("Horizon: {error}");
            crash::report_error(&home, "client", &*error);
            1
        }
    };
    logging::flush();
    if let Some(launch) = launch {
        launch.exit(code);
    }
}

/// The launcher's side of this run: its sockets and the arguments it wrote.
struct Launch {
    ctl: std::os::unix::net::UnixStream,
    // Kept open for the whole run: the launcher reads them as the game's stdio.
    _stdin: std::os::unix::net::UnixStream,
    _stdout: std::os::unix::net::UnixStream,
    _stderr: std::os::unix::net::UnixStream,
    args: Vec<String>,
}

impl Launch {
    fn connect(files: &Path, tok: &str) -> Result<Launch, String> {
        let dir = files.join("run").join(tok);
        let args: Vec<String> = std::fs::read(dir.join("horizon-args.json"))
            .map_err(|e| format!("horizon-args.json: {e}"))
            .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()))?;
        let connect = |name: &str| {
            let path = dir.join(name);
            let mut last = None;
            // The launcher binds before starting the activity; this only rides out start-up jitter.
            for _ in 0..40 {
                match std::os::unix::net::UnixStream::connect(&path) {
                    Ok(stream) => return Ok(stream),
                    Err(e) => last = Some(e),
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(format!("{}: {}", path.display(), last.map(|e| e.to_string()).unwrap_or_default()))
        };
        // The launcher accepts in this order and reads the pid from ctl first.
        let ctl = connect("ctl")?;
        let stdin = connect("in")?;
        let stdout = connect("out")?;
        let stderr = connect("err")?;
        let pid = std::process::id();
        std::io::Write::write_all(&mut &ctl, &pid.to_le_bytes()).map_err(|e| format!("ctl: {e}"))?;
        // SAFETY: plain dup2 onto the process's own stdio descriptors.
        unsafe {
            libc::dup2(stdout.as_raw_fd(), 1);
            libc::dup2(stderr.as_raw_fd(), 2);
        }
        Ok(Launch { ctl, _stdin: stdin, _stdout: stdout, _stderr: stderr, args })
    }

    /// Reports `code` and ends the process without unwinding. Running the C++ static destructors
    /// of a live ART process (what `exit` does) can abort on mutexes other threads still hold.
    fn exit(self, code: i32) -> ! {
        let _ = std::io::Write::write_all(&mut &self.ctl, &code.to_le_bytes());
        // SAFETY: `_exit` only ends the process.
        unsafe { libc::_exit(code) }
    }
}

/// A string extra of the activity's Intent.
fn intent_extra(app: &AndroidApp, key: &str) -> Option<String> {
    use jni::objects::{JObject, JString};
    // SAFETY: android-activity hands out the process's JavaVM and a global reference to the
    // activity, both valid while the app runs; the JObject is only borrowed, never deleted.
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast()) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let intent = env
        .call_method(&activity, "getIntent", "()Landroid/content/Intent;", &[])
        .ok()?
        .l()
        .ok()?;
    let name = env.new_string(key).ok()?;
    let value = env
        .call_method(
            &intent,
            "getStringExtra",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[(&name).into()],
        )
        .ok()?
        .l()
        .ok()?;
    if value.is_null() {
        return None;
    }
    let value = JString::from(value);
    let text: String = env.get_string(&value).ok()?.into();
    Some(text)
}

#[link(name = "log")]
unsafe extern "C" {
    fn __android_log_write(prio: i32, tag: *const u8, text: *const u8) -> i32;
}

fn log_to_logcat(line: &str) {
    let text = std::ffi::CString::new(line).unwrap_or_default();
    // SAFETY: both strings are NUL-terminated.
    unsafe { __android_log_write(4, c"horizon".as_ptr().cast(), text.as_ptr().cast()) };
}

/// Rust's stdout and stderr go nowhere on Android: pipe them into logcat under `horizon`.
fn redirect_stdio_to_logcat() {
    use std::io::{BufRead, BufReader};
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
            log_to_logcat(&line);
        }
    });
}
