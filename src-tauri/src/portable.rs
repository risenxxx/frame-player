//! A copy of the player that keeps everything beside its own executable.
//!
//! An ordinary installation scatters its state across the profile the way the
//! platform wants: `%APPDATA%/app.frameplayer` for `mpv.conf`,
//! `%LOCALAPPDATA%/app.frameplayer` for the caches, the thumbnails, the torrent
//! store and — through the webview's own data store — every position, track
//! choice and hotkey override the player remembers. A portable copy puts all of
//! that under one dot-prefixed directory next to the executable, so that the
//! folder *is* the installation and moving it moves the player.
//!
//! **A file is what decides, never a guess.** `.portable` beside the executable
//! and nothing else: inferring portability from, say, a missing uninstall
//! registry entry would mean that an ordinary installation whose entry was
//! removed suddenly stopped finding its own watch history. The marker ships in
//! the portable archive and in nothing else.
//!
//! The layout mirrors the platform's three directories one for one, awkward
//! `data/data` included, so that nobody reading both has to learn a second
//! vocabulary:
//!
//! ```text
//! Frame Player/
//! ├── .portable            the marker, and all it has to be is present
//! ├── .data/
//! │   ├── config/          app_config_dir  — mpv.conf
//! │   ├── data/            app_data_dir    — bin/yt-dlp, opensubtitles.key
//! │   ├── cache/           app_cache_dir   — thumbs, posters, cast, torrents
//! │   └── webview/         localStorage: positions, tracks, hotkeys, geometry
//! ├── frameplayer.exe
//! └── …
//! ```
//!
//! Both names begin with a dot because the in-place update's file-set rule
//! excludes exactly that — see `is_foreign_top_level` in update.rs. Without it
//! a portable copy's own state would read as paths the payload had dropped, and
//! every update would refuse itself and fall back to an installer that would
//! install a *second*, ordinary copy into the profile.
//!
//! **Windows only.** A macOS application is already a relocatable directory,
//! its Resources are sealed by the signature, and nothing about the bundle
//! invites keeping a torrent store inside it.
//!
//! One thing stays outside the folder on purpose: the OpenSubtitles password
//! lives in the OS credential store, because the alternative is a password in a
//! file inside a directory people copy onto other people's machines. The
//! settings sheet says so rather than leaving it to be discovered.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tauri::Manager;

/// Present beside the executable means portable. Its contents are not read;
/// a later version may put options in it.
const MARKER: &str = ".portable";

/// Everything a portable copy keeps, under one name.
const DATA: &str = ".data";

/// The directory the running executable lives in — the installation, for every
/// purpose this file and update.rs have.
///
/// From the running image rather than from the registry or a configured path:
/// a portable copy has no registry entry at all, and an installation's entry
/// can still name where a previous one was.
pub fn app_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "the executable has no directory".to_string())
}

/// `Some(<app dir>/.data)` when this copy is portable, `None` when it is an
/// ordinary installation. Decided once: the answer cannot change while the
/// process runs, and every path in the player depends on it.
fn root() -> Option<&'static Path> {
    static ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();
    ROOT.get_or_init(|| {
        if !cfg!(windows) {
            return None;
        }
        let dir = app_dir().ok()?;
        if !dir.join(MARKER).is_file() {
            return None;
        }
        let data = dir.join(DATA);
        eprintln!("[portable] state in {}", data.display());
        Some(data)
    })
    .as_deref()
}

/// Whether this copy keeps its state beside itself. Read by the settings sheet,
/// by the shell-handler registration and by the update's fallback, each of which
/// does something different about it.
pub fn is_portable() -> bool {
    root().is_some()
}

/// One of the three directories, created on demand.
///
/// The `Result<_, String>` shape is the platform resolver's, so a call site
/// reads the same whichever answer it gets.
fn dir(app: &tauri::AppHandle, leaf: &str, fallback: PlatformDir) -> Result<PathBuf, String> {
    let path = match root() {
        Some(root) => root.join(leaf),
        None => match fallback {
            PlatformDir::Config => app.path().app_config_dir(),
            PlatformDir::Data => app.path().app_data_dir(),
            PlatformDir::Cache => app.path().app_cache_dir(),
        }
        .map_err(|e| e.to_string())?,
    };
    // The platform resolver does not create these either, and every call site
    // used to do it by hand — half of them forgetting, which is how a missing
    // cache directory turned into "thumbnails silently do not work".
    std::fs::create_dir_all(&path).map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    Ok(path)
}

enum PlatformDir {
    Config,
    Data,
    Cache,
}

/// `mpv.conf` and anything else the viewer edits by hand.
pub fn config_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    dir(app, "config", PlatformDir::Config)
}

/// What the player installed for itself and must not lose: the yt-dlp binary,
/// the replaceable OpenSubtitles key.
pub fn data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    dir(app, "data", PlatformDir::Data)
}

/// Everything that may be deleted without losing anything: thumbnails, posters,
/// the cast transcode, the torrent store.
pub fn cache_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    dir(app, "cache", PlatformDir::Cache)
}

/// Where the webview keeps localStorage, or `None` to leave it where it has
/// always been.
///
/// `None` is load-bearing rather than merely tidy. Tauri forces the data
/// directory to `LocalData/<identifier>` when nothing else sets it — in the
/// manager, not from the configuration (`tauri/src/manager/webview.rs`) — so an
/// ordinary installation that passes nothing keeps the exact directory it has
/// always used. Passing a computed "same" path instead would be one typo away
/// from moving every viewer's watch history, tracks, hotkeys and window
/// geometry somewhere they would never be found again.
///
/// It also cannot come from `tauri.conf.json`: that field is documented as a
/// path *relative* to `appDataDir()/<label>`, and an absolute one is refused
/// outright ("is not a relative path, ignoring config"). Hence the main window
/// is built in Rust.
pub fn webview_dir() -> Option<PathBuf> {
    root().map(|root| root.join("webview"))
}

/// Whether an installer put this copy here, which is a **different question**
/// from where it keeps its state and has to be asked separately.
///
/// Conflating the two was a bug: an installation that keeps its state beside
/// itself — which the installer's own checkbox now offers — would have been
/// sent to the download page when a swap was refused, although it has an
/// installer to fall back on, and would have registered shell handlers its
/// installer had already written. Both answers come from here instead.
///
/// The test is the uninstall entry naming *this* directory. Not merely the
/// entry existing: a portable copy unpacked beside an installation must not
/// read the installation's entry as its own.
pub fn installed(app: &tauri::AppHandle) -> bool {
    #[cfg(windows)]
    {
        uninstall_entry(app).is_some()
    }
    // A macOS application is a bundle from a disk image; there is no second kind
    // of copy to tell it apart from, and nothing on that platform asks.
    #[cfg(not(windows))]
    {
        let _ = app;
        true
    }
}

/// The uninstall entry that names this directory — the hive it is in and its
/// path — or `None` when no installer put this copy here.
///
/// One definition, because two things read it: this file, to answer
/// `installed()`, and update.rs, to write `DisplayVersion` after a swap.
#[cfg(windows)]
pub fn uninstall_entry(
    app: &tauri::AppHandle,
) -> Option<(&'static windows_registry::Key, String)> {
    let product = app.config().product_name.clone()?;
    let path = format!(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{product}");
    let here = app_dir().ok()?;
    // Per-user first, which is what the installer writes by default; a
    // per-machine installation keeps the same entry under the machine's own
    // hive.
    for hive in [
        windows_registry::CURRENT_USER,
        windows_registry::LOCAL_MACHINE,
    ] {
        let Ok(key) = hive.open(&path) else { continue };
        // The installer writes this one quoted.
        let Ok(location) = key.get_string("InstallLocation") else {
            continue;
        };
        if Path::new(location.trim_matches('"'))
            .as_os_str()
            .eq_ignore_ascii_case(here.as_os_str())
        {
            return Some((hive, path));
        }
    }
    None
}

/// What the settings sheet shows: which mode this copy is in, where its state
/// is, and who registered it with the system — so that it is never a mystery
/// which kind of copy a given folder holds.
#[derive(serde::Serialize)]
pub struct State {
    /// Whether this copy keeps its state beside its executable.
    pub portable: bool,
    /// Whether an installer put it here.
    pub installed: bool,
    /// Where that state is, either way.
    pub location: String,
}

#[tauri::command]
pub fn portable_state(app: tauri::AppHandle) -> State {
    State {
        portable: is_portable(),
        installed: installed(&app),
        location: match root() {
            Some(root) => root.display().to_string(),
            None => app
                .path()
                .app_data_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|e| e.to_string()),
        },
    }
}
