//! Windows: Velopack, the beta's installer and updater.
//!
//! - [`startup`] is the first thing `main` does: Velopack's install / update / uninstall hooks run in
//!   it, and the process exits there when it was started for one. Every hook writes a line to
//!   `velopack.log` in the data folder ([`commands::data_dir`]), as does Velopack's own logging (the
//!   `log` crate, its targets only).
//! - An installed copy (the payload beside the exe, [`commands::installed`]) has no console: when
//!   nobody redirected stderr, stdout and stderr go to `frameplayer.log` in the data folder.
//! - `FPUI_UPDATE_FEED` (an http(s) URL or a local folder) starts one update check a few seconds
//!   after the start (`FPUI_UPDATE_DELAY`, seconds, default 5). An update is downloaded (a delta
//!   when the feed has one and the base package is on disk), handed to Velopack's updater to apply
//!   after this process exits (silent, with a restart), and the window is closed the ordinary way so
//!   that mpv shuts down before the files are swapped. Without the variable nothing is checked.
//! - [`set_window`]: in an installed copy the window's first title is "Frame Player beta <version>".
//!   The interface sets its own title afterwards.

use std::io::Write;
use std::sync::atomic::{AtomicIsize, Ordering};

use velopack::sources::{FileSource, HttpSource};
use velopack::{UpdateCheck, UpdateManager, VelopackApp};

use crate::commands;

static HWND: AtomicIsize = AtomicIsize::new(0);

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

/// A release build has no console. When nobody redirected stderr (a start from the shortcut), an
/// installed copy sends stdout and stderr to `frameplayer.log` in its data folder; the previous
/// start's log is kept as `frameplayer.prev.log`. Hook runs (`--veloapp-*`) leave the logs alone.
fn log_to_file() {
    use std::os::windows::io::IntoRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Console::{GetStdHandle, SetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE};
    if std::env::args().nth(1).is_some_and(|a| a.starts_with("--veloapp-")) {
        return;
    }
    let inherited = unsafe { GetStdHandle(STD_ERROR_HANDLE) }.is_ok_and(|h| !h.is_invalid() && !h.0.is_null());
    if inherited {
        return;
    }
    let dir = commands::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::rename(dir.join("frameplayer.log"), dir.join("frameplayer.prev.log"));
    let Ok(file) = std::fs::File::create(dir.join("frameplayer.log")) else { return };
    let h = HANDLE(file.into_raw_handle());
    unsafe {
        let _ = SetStdHandle(STD_ERROR_HANDLE, h);
        let _ = SetStdHandle(STD_OUTPUT_HANDLE, h);
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
        .on_after_install_fast_callback(|v| log(&format!("hook after-install {v}")))
        .on_before_update_fast_callback(|v| log(&format!("hook before-update (obsolete) {v}")))
        .on_after_update_fast_callback(|v| log(&format!("hook after-update {v}")))
        .on_before_uninstall_fast_callback(|v| log(&format!("hook before-uninstall {v}")))
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
            std::thread::sleep(std::time::Duration::from_secs(delay));
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
    um.wait_exit_then_apply_updates(&*info, true, true, Vec::<String>::new()).map_err(|e| e.to_string())?;
    log("updater started (silent, restart); closing the window");
    let hwnd = HWND.load(Ordering::Relaxed);
    if hwnd != 0 {
        use windows::Win32::Foundation::{HWND as H, LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
        unsafe {
            let _ = PostMessageW(Some(H(hwnd as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    } else {
        std::process::exit(0);
    }
    Ok(())
}

/// The window, once Skathi made it: kept for the update's close, and titled with the version in an
/// installed copy.
pub fn set_window(hwnd: isize) {
    HWND.store(hwnd, Ordering::Relaxed);
    if !commands::installed() {
        return;
    }
    let title = format!("Frame Player beta {}", version());
    log(&format!("title {title:?}"));
    let wide: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    use windows::Win32::Foundation::HWND as H;
    use windows::Win32::UI::WindowsAndMessaging::SetWindowTextW;
    unsafe {
        let _ = SetWindowTextW(H(hwnd as *mut _), windows::core::PCWSTR(wide.as_ptr()));
    }
}
