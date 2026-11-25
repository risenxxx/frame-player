//! macOS: Velopack, the beta's installer and updater. The Windows side is `windows/velo.rs`; this
//! one does the same where macOS allows it.
//!
//! - [`startup`] is the first thing `main` does. On macOS the crate calls only `on_first_run` and
//!   `on_restarted` (its install / update / uninstall hooks are `cfg(windows)`); both write a line to
//!   `velopack.log` in the data folder ([`commands::data_dir`]), as does Velopack's own logging
//!   (the `log` crate, its targets only).
//! - An installed copy (an app bundle with the payload in `Contents/Resources`,
//!   [`commands::installed`]) started from Finder, the Dock or Spotlight has its stderr on
//!   `/dev/null`; then stdout and stderr go to `frameplayer.log` in the data folder.
//! - `FPUI_UPDATE_FEED` (an http(s) URL or a local folder) starts one update check a few seconds
//!   after the start (`FPUI_UPDATE_DELAY`, seconds, default 5). An update is downloaded, handed to
//!   Velopack's updater to apply after this process exits (silent, with a restart), and the windows
//!   are closed the ordinary way so that mpv shuts down before the bundle is swapped.
//!   `FPUI_UPDATE_APPLY=0` only downloads. Without the variable nothing is checked.

use std::io::Write;
use std::time::Duration;

use objc2::msg_send;
use objc2::runtime::AnyObject;
use velopack::sources::{FileSource, HttpSource};
use velopack::{UpdateCheck, UpdateManager, VelopackApp};

use crate::{commands, sys};

/// One line in `velopack.log`: seconds since the epoch, the process, the event.
pub fn log(event: &str) {
    let dir = commands::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("velopack.log")) {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        let _ = writeln!(f, "{}.{:03} pid {} {event}", t.as_secs(), t.subsec_millis(), std::process::id());
    }
}

struct VeloLog;
impl log::Log for VeloLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.target().starts_with("velopack")
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            log(&format!("velopack {} {}", r.level(), r.args()));
        }
    }
    fn flush(&self) {}
}
static LOGGER: VeloLog = VeloLog;

fn version() -> String {
    UpdateManager::new(FileSource::new("."), None, None)
        .map(|m| m.get_current_version_as_string())
        .unwrap_or_else(|e| format!("(not installed: {e})"))
}

/// A bundle started by launchd has stdout and stderr on `/dev/null`. Then an installed copy sends
/// both to `frameplayer.log` in its data folder; the previous start's log is kept as
/// `frameplayer.prev.log`. A terminal, a pipe or a file (a test run) is left alone.
fn log_to_file() {
    use std::os::fd::IntoRawFd;
    let null = unsafe {
        let mut st: libc::stat = std::mem::zeroed();
        let mut dn: libc::stat = std::mem::zeroed();
        libc::fstat(2, &mut st) == 0
            && libc::stat(c"/dev/null".as_ptr(), &mut dn) == 0
            && st.st_dev == dn.st_dev
            && st.st_ino == dn.st_ino
    };
    if !null {
        return;
    }
    let dir = commands::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::rename(dir.join("frameplayer.log"), dir.join("frameplayer.prev.log"));
    let Ok(file) = std::fs::File::create(dir.join("frameplayer.log")) else { return };
    let fd = file.into_raw_fd();
    unsafe {
        libc::dup2(fd, 1);
        libc::dup2(fd, 2);
        libc::close(fd);
    }
}

pub fn startup() {
    let installed = commands::installed();
    if installed {
        log_to_file();
        if log::set_logger(&LOGGER).is_ok() {
            log::set_max_level(log::LevelFilter::Debug);
        }
    }
    VelopackApp::build()
        .on_first_run(|v| log(&format!("first run {v}")))
        .on_restarted(|v| log(&format!("restarted after update {v}")))
        .run();
    if !installed {
        return;
    }
    let exe = std::env::current_exe().unwrap_or_default();
    log(&format!("start version {} exe {}", version(), exe.display()));
    if let Ok(feed) = std::env::var("FPUI_UPDATE_FEED") {
        std::thread::spawn(move || {
            let delay = std::env::var("FPUI_UPDATE_DELAY").ok().and_then(|v| v.parse().ok()).unwrap_or(5u64);
            std::thread::sleep(Duration::from_secs(delay));
            if let Err(e) = check(&feed) {
                log(&format!("update failed: {e}"));
            }
        });
    }
}

fn check(feed: &str) -> Result<(), String> {
    let um = if feed.contains("://") {
        UpdateManager::new(HttpSource::new(feed), None, None)
    } else {
        UpdateManager::new(FileSource::new(feed), None, None)
    }
    .map_err(|e| e.to_string())?;
    log(&format!("check {feed}: current {}", um.get_current_version_as_string()));
    let t = std::time::Instant::now();
    let info = match um.check_for_updates().map_err(|e| e.to_string())? {
        UpdateCheck::UpdateAvailable(info) => info,
        UpdateCheck::NoUpdateAvailable => {
            log("check: no update available");
            return Ok(());
        }
        UpdateCheck::RemoteIsEmpty => {
            log("check: remote is empty");
            return Ok(());
        }
    };
    log(&format!(
        "check: update {} ({} bytes), base {:?}, deltas {:?}",
        info.TargetFullRelease.Version,
        info.TargetFullRelease.Size,
        info.BaseRelease.as_ref().map(|b| b.Version.clone()),
        info.DeltasToTarget.iter().map(|d| format!("{} {} bytes", d.FileName, d.Size)).collect::<Vec<_>>()
    ));
    um.download_updates(&info, None).map_err(|e| e.to_string())?;
    log(&format!("download done in {} ms", t.elapsed().as_millis()));
    if std::env::var("FPUI_UPDATE_APPLY").is_ok_and(|v| v == "0") {
        log("apply skipped (FPUI_UPDATE_APPLY=0)");
        return Ok(());
    }
    // Velopack's UpdateMac replaces the bundle in place, and when it cannot write there it asks for an
    // administrator's password (`osascript … with administrator privileges`, from its strings). Run from
    // the disk image (read-only) or translocated by Gatekeeper (a read-only mount under
    // /private/var/folders/…/AppTranslocation), there is nothing to update: the download stays, and the
    // copy the user moves to Applications updates from it.
    if let Some(why) = not_updatable() {
        log(&format!("update not applied: {why}"));
        return Ok(());
    }
    um.wait_exit_then_apply_updates(&*info, true, true, Vec::<String>::new()).map_err(|e| e.to_string())?;
    log("updater started (silent, restart); closing the windows");
    // The ordinary close, as the interface's own close button does: Skathi's exit runs, mpv is
    // destroyed, the process ends, and the updater swaps the bundle.
    let closed = sys::on_main(Duration::from_secs(5), || unsafe {
        let app: *mut AnyObject = msg_send![objc2::class!(NSApplication), sharedApplication];
        let windows: *mut AnyObject = msg_send![app, windows];
        let n: usize = msg_send![windows, count];
        for i in 0..n {
            let w: *mut AnyObject = msg_send![windows, objectAtIndex: i];
            let _: () = msg_send![w, close];
        }
        n
    });
    // Not checked: whether closing the last window always ends the app. The updater waits for this
    // process, so it ends anyway after a grace period.
    std::thread::sleep(if closed.is_some() { Duration::from_secs(10) } else { Duration::ZERO });
    log("still running after the close; exiting");
    std::process::exit(0)
}

/// Why this bundle cannot be updated in place, if it cannot: translocated, or the bundle or the folder
/// holding it not writable by this user.
fn not_updatable() -> Option<String> {
    let contents = commands::bundle_contents()?;
    let bundle = contents.parent()?;
    let path = bundle.to_string_lossy();
    if path.contains("/AppTranslocation/") {
        return Some(format!("translocated by Gatekeeper ({path})"));
    }
    let writable = |p: &std::path::Path| {
        let c = std::ffi::CString::new(p.as_os_str().as_encoded_bytes()).unwrap_or_default();
        unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
    };
    for p in [bundle, bundle.parent().unwrap_or(bundle)] {
        if !writable(p) {
            return Some(format!("{} is not writable", p.display()));
        }
    }
    None
}
