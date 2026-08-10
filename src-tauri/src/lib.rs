use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::{Emitter, Manager};

#[cfg(target_os = "macos")]
mod macos_chrome;
#[cfg(target_os = "macos")]
mod macos_menu;
mod cast;
mod catalog;
mod dlna;
mod opensubtitles;
mod power;
mod screenshot;
mod step_engine;
mod thumb_service;
mod torrent;
mod upnp;
mod window_guard;

const OPEN_FILE_EVENT: &str = "frameplayer://open-file";

/// CLI arguments that are existing files — video paths from "Open with"
/// (Windows/Linux; several selected files arrive as several arguments and
/// become a playlist).
fn pick_file_args(args: &[String]) -> Vec<String> {
    args.iter()
        .skip(1)
        .filter(|a| !a.starts_with('-') && std::path::Path::new(a.as_str()).exists())
        .cloned()
        .collect()
}

/// Buffer for files that arrive before the frontend subscribes to
/// `OPEN_FILE_EVENT`. Needed because of macOS: Finder does not put the path in
/// argv, it sends an Apple Event (`RunEvent::Opened`), and that arrives before
/// the webview gets to run `onMount` — without the buffer the event went
/// nowhere and the player stayed on the start screen.
#[derive(Default)]
struct PendingOpen {
    paths: Mutex<Vec<String>>,
    ready: AtomicBool,
    /// argv files are handed out exactly once — otherwise a second drain (on
    /// window focus, say) would reopen the startup file on Windows.
    argv_taken: AtomicBool,
}

impl PendingOpen {
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.paths.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

/// Deliver files to the frontend.
///
/// The path always goes into the buffer first and is only then announced by an
/// event — the event is a "look in the buffer" signal, not a transport. That
/// way a lost event (frontend not subscribed yet, webview asleep, window
/// inactive) no longer means a lost file: the next drain picks it up, including
/// the one triggered by window focus. The event used to carry the path itself,
/// and a single missed emit left the player silently sitting on the old video.
fn deliver_files(app: &tauri::AppHandle, paths: Vec<String>) {
    if paths.is_empty() {
        return;
    }
    let state = app.state::<PendingOpen>();
    state
        .paths
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .extend(paths);
    if state.ready.load(Ordering::SeqCst) {
        let _ = app.emit(OPEN_FILE_EVENT, ());
    }
}

/// Take whatever accumulated. Called at startup, on the signal and on window focus.
#[tauri::command]
fn take_pending_files(state: tauri::State<'_, PendingOpen>) -> Vec<String> {
    // Windows/Linux: an "Open with" file arrives in the argv of the first
    // launch. macOS puts nothing here — everything comes via the Apple Event.
    let mut out = if state.argv_taken.swap(true, Ordering::SeqCst) {
        Vec::new()
    } else {
        pick_file_args(&std::env::args().collect::<Vec<_>>())
    };
    out.extend(state.take());
    out
}

/// The frontend has subscribed to `OPEN_FILE_EVENT`, so files can be signaled
/// from now on. Anything buffered while mpv was starting is announced at once.
#[tauri::command]
fn open_file_ready(app: tauri::AppHandle, state: tauri::State<'_, PendingOpen>) {
    state.ready.store(true, Ordering::SeqCst);
    let empty = state
        .paths
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .is_empty();
    if !empty {
        let _ = app.emit(OPEN_FILE_EVENT, ());
    }
}

#[derive(serde::Serialize)]
struct UserConf {
    path: String,
    options: Vec<(String, String)>,
}

const MPV_CONF_TEMPLATE: &str = r#"# Frame Player — low-level mpv options.
# Applied at player startup ON TOP of its defaults; mpv.conf syntax:
#   option=value
# lines starting with # are comments. Full list: https://mpv.io/manual/stable/#options
#
# You may override vo/hwdec and other options the player sets itself,
# but at your own risk — some of them are critical for video embedding.
#
# ---- Examples ----
#
# True HDR output on an HDR display (Windows HDR must be enabled):
#target-colorspace-hint=auto
#
# HDR -> SDR tone-mapping curve (default: spline):
#tone-mapping=bt.2446a
#
# Bitstream Dolby/DTS to an AV receiver (the AVR decodes; player volume control stops working):
#audio-spdif=ac3,eac3,truehd
#
# Software decoding instead of hardware:
#hwdec=no
"#;

/// Files sitting next to `path`, so opening one episode can queue the rest of
/// the folder.
///
/// Returns every plain file, unfiltered and unsorted. Deciding what counts as a
/// video belongs to the frontend, which already owns that list (`VIDEO_EXTENSIONS`)
/// — a second copy here would be the kind that drifts. Ordering belongs there
/// too: natural order is `Intl.Collator({numeric: true})`, which knows that
/// "ep2" comes before "ep10" and how the user's locale sorts letters, and Rust
/// has no comparable batteries included.
#[tauri::command]
fn folder_entries(path: String) -> Result<Vec<String>, String> {
    let parent = std::path::Path::new(&path)
        .parent()
        .ok_or_else(|| "the file has no parent directory".to_string())?;
    let dir = std::fs::read_dir(parent).map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    for entry in dir.flatten() {
        // `is_dir` on the entry rather than a metadata call per file: this runs
        // while a file is opening, and a folder on a network share can make
        // stat-per-entry take visibly long.
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(true) {
            continue;
        }
        if let Some(name) = entry.file_name().to_str() {
            // Hidden files are never episodes, and macOS scatters `._` AppleDouble
            // siblings next to real ones on non-native filesystems.
            if name.starts_with('.') {
                continue;
            }
        }
        if let Some(full) = entry.path().to_str() {
            files.push(full.to_string());
        }
    }
    Ok(files)
}

/// Which of these paths still exist, in the order given.
///
/// One round trip for the whole start screen rather than one per card, and it
/// exists so the "continue watching" list can be **filtered before it is
/// published**: it used to appear in full and then shed its dead entries one at
/// a time as their posters failed to decode, which read as the player showing
/// files that then vanished. A `stat` is microseconds where decoding a poster
/// is not, so this can happen before the first paint.
#[tauri::command]
fn paths_exist(paths: Vec<String>) -> Vec<bool> {
    paths
        .iter()
        .map(|p| std::path::Path::new(p).exists())
        .collect()
}

/// The bundle identifier, mirroring `identifier` in tauri.conf.json — there is
/// no way to read that at runtime without an AppHandle, and two places derive
/// paths from it. Changed together with the config, and changing it moves the
/// app data directory and the keychain entry: every stored setting, position
/// and token is left behind under the old name.
const APP_IDENTIFIER: &str = "app.frameplayer";

/// The app data directory, without an AppHandle. `ytdlp_path` is called from
/// `initPlayer` before anything else and from `ytdlp_status`, and threading a
/// handle through only to join two strings is not worth it. Mirrors what Tauri
/// derives from the bundle identifier.
fn dirs_app_data() -> Option<std::path::PathBuf> {
    let id = APP_IDENTIFIER;
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")?;
        Some(std::path::PathBuf::from(home).join("Library/Application Support").join(id))
    }
    #[cfg(windows)]
    {
        let base = std::env::var_os("APPDATA")?;
        Some(std::path::PathBuf::from(base).join(id))
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let home = std::env::var_os("HOME")?;
        Some(std::path::PathBuf::from(home).join(".local/share").join(id))
    }
}

/// Absolute path to a working yt-dlp, or None.
///
/// mpv's ytdl hook looks for `yt-dlp` in `PATH` — which is exactly what a GUI
/// application does not have. Launched from Finder or Explorer a process
/// inherits the session's minimal environment, not the shell's, so a perfectly
/// good `~/bin/yt-dlp` or `/opt/homebrew/bin/yt-dlp` is invisible to it. Hence
/// probing the places people actually install it and handing mpv an absolute
/// path through `ytdl_hook-ytdl_path`.
///
/// Checked by looking, NOT by running. Executing it would be the surer test,
/// but measured on a macOS universal build `yt-dlp --version` takes **11
/// seconds** — the binary is a 36 MB PyInstaller bundle that unpacks itself and
/// is re-verified by the system on every exec. Doing that at startup, twice,
/// delayed everything behind it by twenty seconds and made the player look
/// dead. A broken binary is caught where it matters instead: the link fails and
/// the dialog offers to install or update it, which is the recovery path
/// anyway. Downloaded copies ARE run once, in `ytdlp_install`, where the wait
/// is both expected and worth it.
#[tauri::command]
fn ytdlp_path() -> Option<String> {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();

    // Our own copy first: it is the one we can keep current, and a stale
    // yt-dlp is the failure people actually hit.
    if let Some(dir) = dirs_app_data() {
        candidates.push(dir.join("bin").join(YTDLP_BIN));
    }

    // Then whatever PATH we did get — a user who set it up deliberately wins
    // over the rest of the guesses below.
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            candidates.push(dir.join(YTDLP_BIN));
        }
    }

    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = std::path::PathBuf::from(home);
        for dir in ["bin", ".local/bin"] {
            candidates.push(home.join(dir).join(YTDLP_BIN));
        }
    }

    #[cfg(target_os = "macos")]
    for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"] {
        candidates.push(std::path::PathBuf::from(dir).join(YTDLP_BIN));
    }

    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let local = std::path::PathBuf::from(local);
        // winget shims and the usual manual drop-in spot.
        candidates.push(local.join("Microsoft\\WinGet\\Links").join(YTDLP_BIN));
        candidates.push(local.join("Programs").join(YTDLP_BIN));
    }

    for path in candidates {
        if !path.is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let executable = path
                .metadata()
                .map(|m| m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false);
            if !executable {
                continue;
            }
        }
        // Belt and braces: mpv's hook is handed this absolute path and does run
        // it (measured — it logs "No youtube-dl found … in config directories",
        // which is only about mpv's own config dir, and then spawns exactly this
        // path). Putting the directory on PATH as well means the hook finds it
        // through its ordinary lookup too, so the feature does not depend on
        // which way a future version of the script resolves the option.
        if let Some(dir) = path.parent().and_then(|d| d.to_str()) {
            let current = std::env::var("PATH").unwrap_or_default();
            if !current.split(':').any(|entry| entry == dir) {
                std::env::set_var("PATH", format!("{dir}:{current}"));
            }
        }
        return path.to_str().map(|s| s.to_string());
    }
    None
}

/// The title of a video behind a link, from the site's own oEmbed endpoint.
///
/// **The site's own, deliberately not a third-party aggregator.** YouTube
/// already knows you are watching this video — you are streaming it from them —
/// so asking them its name reveals nothing new. A middleman service learns it
/// for the first time, which is the same objection that keeps subtitle search
/// talking straight to OpenSubtitles (see ROADMAP 16). The cost is coverage:
/// only providers listed here get a name this way. Everything actually played
/// gets one for free from mpv anyway.
#[tauri::command]
async fn oembed_title(url: String) -> Option<String> {
    let parsed = reqwest::Url::parse(&url).ok()?;
    let host = parsed.host_str()?.trim_start_matches("www.").to_lowercase();
    let endpoint = match host.as_str() {
        "youtube.com" | "youtu.be" | "m.youtube.com" | "music.youtube.com" => {
            "https://www.youtube.com/oembed"
        }
        "vimeo.com" | "player.vimeo.com" => "https://vimeo.com/api/oembed.json",
        _ => return None,
    };
    let request =
        reqwest::Url::parse_with_params(endpoint, &[("url", url.as_str()), ("format", "json")])
            .ok()?;
    let body: serde_json::Value = reqwest::Client::new()
        .get(request)
        .header("User-Agent", "FramePlayer")
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    let title = body.get("title")?.as_str()?.trim();
    (!title.is_empty()).then(|| title.to_string())
}

/// Percentage of the yt-dlp download, 0..100.
const YTDLP_PROGRESS_EVENT: &str = "frameplayer://ytdlp-progress";

#[cfg(windows)]
const YTDLP_BIN: &str = "yt-dlp.exe";
#[cfg(not(windows))]
const YTDLP_BIN: &str = "yt-dlp";

/// Release asset for this platform. macOS gets one universal binary; Windows
/// has a build per architecture.
#[cfg(target_os = "macos")]
const YTDLP_ASSET: &str = "yt-dlp_macos";
#[cfg(all(windows, target_arch = "aarch64"))]
const YTDLP_ASSET: &str = "yt-dlp_arm64.exe";
#[cfg(all(windows, not(target_arch = "aarch64")))]
const YTDLP_ASSET: &str = "yt-dlp.exe";
#[cfg(not(any(target_os = "macos", windows)))]
const YTDLP_ASSET: &str = "yt-dlp";

/// Where our own copy goes: the app data directory, because that is the only
/// place we may write to afterwards. Program Files needs administrator rights
/// to overwrite, and a binary inside a macOS `.app` cannot be replaced without
/// invalidating the bundle's signature — and being able to overwrite it is the
/// entire point, since yt-dlp updates itself in place.
fn ytdlp_managed_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    let dir = app.path().app_data_dir().ok()?.join("bin");
    Some(dir.join(YTDLP_BIN))
}

#[derive(serde::Serialize)]
struct YtdlpStatus {
    path: Option<String>,
    /// The copy we installed, and therefore the only one we may update: a
    /// Homebrew or pip install belongs to its package manager, and `-U` there
    /// either refuses or fights it.
    managed: bool,
    version: Option<String>,
}

fn ytdlp_version(path: &std::path::Path) -> Option<String> {
    let out = std::process::Command::new(path)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!v.is_empty()).then_some(v)
}

#[tauri::command]
async fn ytdlp_status(app: tauri::AppHandle) -> YtdlpStatus {
    tauri::async_runtime::spawn_blocking(move || {
        let managed = ytdlp_managed_path(&app);
        let found = ytdlp_path();
        let is_managed = match (&managed, &found) {
            (Some(m), Some(f)) => std::path::Path::new(f) == m.as_path(),
            _ => false,
        };
        // No version here either: reading it means running it, and that is the
        // 11-second call this whole path exists to avoid. It is reported after
        // an install or update, where the process has just been run anyway.
        YtdlpStatus { path: found, managed: is_managed, version: None }
    })
    .await
    .unwrap_or(YtdlpStatus { path: None, managed: false, version: None })
}

/// Download yt-dlp into our data directory.
///
/// Only the first copy is fetched here; keeping it current is yt-dlp's own job
/// (`ytdlp_update`), which also verifies what it downloads against the signed
/// checksums in the release. Writing the bytes ourselves means the file never
/// gets macOS's `com.apple.quarantine` — that is applied by downloading
/// applications through LaunchServices, not by a plain write.
#[tauri::command]
async fn ytdlp_install(app: tauri::AppHandle) -> Result<String, String> {
    let target = ytdlp_managed_path(&app).ok_or_else(|| "no app data directory".to_string())?;
    let url = format!(
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/{YTDLP_ASSET}"
    );
    let response = reqwest::Client::new()
        .get(&url)
        .header("User-Agent", "FramePlayer")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;

    // Streamed rather than collected in one call, purely so the button can show
    // a figure: this is a 36 MB download over a link that may be slow, and a
    // button that says "downloading…" for a minute is indistinguishable from
    // one that has hung.
    let total = response.content_length().unwrap_or(0);
    let mut bytes: Vec<u8> = Vec::with_capacity(total as usize);
    let mut stream = response.bytes_stream();
    let mut last_emit = 0u64;
    while let Some(chunk) = futures_util::StreamExt::next(&mut stream).await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        bytes.extend_from_slice(&chunk);
        // Every whole percent, not every chunk: chunks arrive in the thousands
        // and each emit crosses into the webview.
        if total > 0 {
            let pct = bytes.len() as u64 * 100 / total;
            if pct != last_emit {
                last_emit = pct;
                let _ = app.emit(YTDLP_PROGRESS_EVENT, pct);
            }
        }
    }

    tauri::async_runtime::spawn_blocking(move || {
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        // Written beside the target and renamed, so an interrupted download
        // cannot leave a half-file that later looks like a working install.
        let tmp = target.with_extension("part");
        std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
        // Proven by running it: a downloaded file of the right size can still be
        // an error page or a build for the wrong architecture.
        ytdlp_version(&target).ok_or_else(|| {
            let _ = std::fs::remove_file(&target);
            "the downloaded file does not run".to_string()
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Update our own copy, using yt-dlp's own updater.
///
/// Deliberately not a download: `-U` fetches the release, checks it against the
/// signed checksums published with it and replaces the binary in place. Writing
/// that ourselves would mean reimplementing verification that already exists and
/// is maintained by people who ship it every few weeks.
#[tauri::command]
async fn ytdlp_update(app: tauri::AppHandle) -> Result<String, String> {
    let path = ytdlp_managed_path(&app).ok_or_else(|| "no app data directory".to_string())?;
    if !path.is_file() {
        return Err("not our copy".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let out = std::process::Command::new(&path)
            .arg("-U")
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(err.lines().last().unwrap_or("update failed").to_string());
        }
        ytdlp_version(&path).ok_or_else(|| "yt-dlp stopped running after the update".to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The bundled third-party notices, read from the resource directory.
///
/// It goes through a command rather than the frontend reading the file, because
/// there is no filesystem plugin here and adding one to read a single read-only
/// document we ship ourselves would be a far wider permission than the job needs.
#[tauri::command]
fn third_party_notices(app: tauri::AppHandle) -> Result<String, String> {
    let path = app
        .path()
        .resolve("THIRD-PARTY-NOTICES.md", tauri::path::BaseDirectory::Resource)
        .map_err(|e| e.to_string())?;
    std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
}

/// mpv.conf in the app config directory: created from the template on first
/// run, parsed as `key=value` (comments and blank lines are skipped).
#[tauri::command]
fn user_mpv_conf(app: tauri::AppHandle) -> Result<UserConf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("mpv.conf");
    if !path.exists() {
        std::fs::write(&path, MPV_CONF_TEMPLATE).map_err(|e| e.to_string())?;
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut options = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let k = k.trim();
            let v = v.trim().trim_matches('"').trim_matches('\'');
            // wid is critical for embedding — not up for overriding
            if k.is_empty() || k == "wid" {
                continue;
            }
            options.push((k.to_string(), v.to_string()));
        }
    }
    Ok(UserConf {
        path: path.to_string_lossy().into_owned(),
        options,
    })
}

/// Surgical mpv.conf edit for the interactive settings editor: only the last
/// uncommented line with this key changes (or a new one is appended, or the
/// line is commented out) — manual edits, comments and options the editor does
/// not know about are left untouched.
#[tauri::command]
fn mpv_conf_set(app: tauri::AppHandle, key: String, value: Option<String>) -> Result<(), String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("mpv.conf");
    let text = if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| e.to_string())?
    } else {
        MPV_CONF_TEMPLATE.to_string()
    };
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut target: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some((k, _)) = t.split_once('=') {
            if k.trim() == key {
                target = Some(i);
            }
        }
    }
    match (target, value) {
        (Some(i), Some(v)) => lines[i] = format!("{key}={v}"),
        (Some(i), None) => lines[i] = format!("#{}", lines[i]),
        (None, Some(v)) => lines.push(format!("{key}={v}")),
        (None, None) => {}
    }
    let mut out = lines.join("\n");
    out.push('\n');
    std::fs::write(&path, out).map_err(|e| e.to_string())
}

/// Zoom and pan applied atomically enough for the eye: three mpv properties set
/// back to back on one thread, microseconds apart, so no redraw fits in between
/// (three separate IPC calls did produce an intermediate frame — the picture
/// jerked, most visibly near the corners).
#[tauri::command]
fn zoom_pan(
    app: tauri::AppHandle,
    window: tauri::Window,
    z: f64,
    x: f64,
    y: f64,
) -> Result<(), String> {
    use tauri_plugin_libmpv::MpvExt;
    let label = window.label();
    let mpv = app.mpv();
    mpv.set_property("video-zoom", &serde_json::json!(z), label)
        .map_err(|e| e.to_string())?;
    mpv.set_property("video-pan-x", &serde_json::json!(x), label)
        .map_err(|e| e.to_string())?;
    mpv.set_property("video-pan-y", &serde_json::json!(y), label)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(serde::Serialize, Clone, Copy, Default)]
struct HdrStatus {
    /// The display is HDR-capable (regardless of whether it is switched on in
    /// Windows, or whether there is EDR headroom right now on macOS).
    supported: bool,
    /// HDR is active for this display at this moment.
    enabled: bool,
}

/// HDR state of the display the window is on. While the display is in SDR the
/// app cannot output HDR — target-colorspace-hint becomes a no-op and mpv
/// always tone-maps; the frontend surfaces this in the settings editor.
#[tauri::command]
#[cfg_attr(not(any(windows, target_os = "macos")), allow(unused_variables))]
fn hdr_status(window: tauri::WebviewWindow) -> HdrStatus {
    #[cfg(windows)]
    {
        hdr_status_impl(&window).unwrap_or_default()
    }
    #[cfg(target_os = "macos")]
    {
        // AppKit is main-thread only, and the command arrives on a worker. The
        // answer comes back over a channel; the timeout covers a busy main
        // thread (a fullscreen transition animation, say) — the settings editor
        // then simply keeps showing the previous value.
        let (tx, rx) = std::sync::mpsc::channel();
        let win = window.clone();
        if window
            .run_on_main_thread(move || {
                let _ = tx.send(macos_chrome::edr_headroom(&win));
            })
            .is_err()
        {
            return HdrStatus::default();
        }
        match rx.recv_timeout(std::time::Duration::from_millis(200)) {
            Ok(Some((supported, enabled))) => HdrStatus { supported, enabled },
            _ => HdrStatus::default(),
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        HdrStatus::default()
    }
}

#[cfg(windows)]
fn hdr_status_impl(window: &tauri::WebviewWindow) -> Option<HdrStatus> {
    use windows_sys::Win32::Devices::Display::{
        DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
        DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
        DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_HEADER,
        DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
        DISPLAYCONFIG_SOURCE_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
    };

    // Win11 24H2+: the exact display mode (SDR/WCG/HDR). On 24H2 the older
    // GET_ADVANCED_COLOR_INFO also reports auto color management (ACM) on SDR
    // panels, which is not HDR. windows-sys 0.61 has no struct for it yet, so
    // it is declared here after SDK 10.0.26100 (wingdi.h).
    const GET_ADVANCED_COLOR_INFO_2: i32 = 15;
    const COLOR_MODE_HDR: i32 = 2;
    #[repr(C)]
    struct GetAdvancedColorInfo2 {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
        value: u32, // bit 4 = highDynamicRangeSupported
        color_encoding: i32,
        bits_per_color_channel: u32,
        active_color_mode: i32,
    }

    let hwnd = window.hwnd().ok()?;
    unsafe {
        let hmon = MonitorFromWindow(hwnd.0 as _, MONITOR_DEFAULTTONEAREST);
        if hmon.is_null() {
            return None;
        }
        let mut mi: MONITORINFOEXW = std::mem::zeroed();
        mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(hmon, &mut mi as *mut _ as *mut _) == 0 {
            return None;
        }

        let (mut n_paths, mut n_modes) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut n_paths, &mut n_modes) != 0 {
            return None;
        }
        let mut paths: Vec<DISPLAYCONFIG_PATH_INFO> = vec![std::mem::zeroed(); n_paths as usize];
        let mut modes: Vec<DISPLAYCONFIG_MODE_INFO> = vec![std::mem::zeroed(); n_modes as usize];
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut n_paths,
            paths.as_mut_ptr(),
            &mut n_modes,
            modes.as_mut_ptr(),
            std::ptr::null_mut(),
        ) != 0
        {
            return None;
        }
        paths.truncate(n_paths as usize);

        for p in &paths {
            // Match the path to the window's display by GDI source name.
            let mut src: DISPLAYCONFIG_SOURCE_DEVICE_NAME = std::mem::zeroed();
            src.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            src.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            src.header.adapterId = p.sourceInfo.adapterId;
            src.header.id = p.sourceInfo.id;
            if DisplayConfigGetDeviceInfo(&mut src.header) != 0
                || src.viewGdiDeviceName != mi.szDevice
            {
                continue;
            }

            let mut info2: GetAdvancedColorInfo2 = std::mem::zeroed();
            info2.header.r#type = GET_ADVANCED_COLOR_INFO_2;
            info2.header.size = std::mem::size_of::<GetAdvancedColorInfo2>() as u32;
            info2.header.adapterId = p.targetInfo.adapterId;
            info2.header.id = p.targetInfo.id;
            if DisplayConfigGetDeviceInfo(&mut info2.header) == 0 {
                return Some(HdrStatus {
                    supported: info2.value & (1 << 4) != 0,
                    enabled: info2.active_color_mode == COLOR_MODE_HDR,
                });
            }

            // Before 24H2, "advanced color" *is* HDR.
            let mut info: DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO = std::mem::zeroed();
            info.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO;
            info.header.size = std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32;
            info.header.adapterId = p.targetInfo.adapterId;
            info.header.id = p.targetInfo.id;
            if DisplayConfigGetDeviceInfo(&mut info.header) != 0 {
                return None;
            }
            let v = info.Anonymous.value; // bit 0 = supported, bit 1 = enabled
            return Some(HdrStatus {
                supported: v & 1 != 0,
                enabled: v & 2 != 0,
            });
        }
    }
    None
}

/// Undecorated windows get an invisible helper child (TAURI_DRAG_RESIZE_BORDERS)
/// that implements edge resizing. When maximized it is collapsed, but
/// tauri-runtime-wry forgot about fullscreen: a strip SM_CYFRAME tall across
/// the entire top of the screen keeps answering HTTOP and eats the mouse — the
/// topmost pixels, including the close button in the corner, stop hovering and
/// clicking (see external-issues-backlog/). Hide the helper while fullscreen.
#[cfg(windows)]
fn sync_drag_resize_borders(window: &tauri::Window) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, ShowWindow, SW_HIDE, SW_SHOWNOACTIVATE,
    };
    let Ok(hwnd) = window.hwnd() else { return };
    let fullscreen = window.is_fullscreen().unwrap_or(false);
    let class: Vec<u16> = "TAURI_DRAG_RESIZE_BORDERS\0".encode_utf16().collect();
    let name: Vec<u16> = "TAURI_DRAG_RESIZE_WINDOW\0".encode_utf16().collect();
    unsafe {
        let child = FindWindowExW(
            hwnd.0 as _,
            std::ptr::null_mut(),
            class.as_ptr(),
            name.as_ptr(),
        );
        if !child.is_null() {
            ShowWindow(child, if fullscreen { SW_HIDE } else { SW_SHOWNOACTIVATE });
        }
    }
}

/// Show/hide the system window buttons (macOS), so the traffic lights fade out
/// together with the rest of the UI when idle. A no-op elsewhere: on other
/// platforms the frontend draws the window buttons itself.
#[tauri::command]
#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
fn window_buttons(window: tauri::WebviewWindow, visible: bool) {
    #[cfg(target_os = "macos")]
    {
        // AppKit is main-thread only, and commands arrive on a worker.
        let win = window.clone();
        let _ = window.run_on_main_thread(move || {
            macos_chrome::set_buttons_visible(&win, visible);
        });
    }
}

/// Let the window float over *other applications'* fullscreen spaces, for the
/// mini player (macOS). A no-op elsewhere: on Windows an always-on-top window is
/// already as high as a Win32 window goes, and there is no system compact-overlay
/// mode outside UWP.
#[tauri::command]
#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
fn window_float_over_fullscreen(window: tauri::WebviewWindow, on: bool) {
    #[cfg(target_os = "macos")]
    {
        // AppKit is main-thread only, and commands arrive on a worker.
        let win = window.clone();
        let _ = window.run_on_main_thread(move || {
            macos_chrome::set_float_over_fullscreen(&win, on);
        });
    }
}

/// The system double-click threshold. The webview detects dblclick by the same
/// value, so any single-click delay must match it exactly: set it lower and the
/// single-click action fires before the system recognizes the double click, so
/// a double click runs both.
#[tauri::command]
fn double_click_time() -> u32 {
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime()
    }
    #[cfg(target_os = "macos")]
    {
        // NSTimeInterval in seconds; the system default is 0.5 s, and the
        // slider in System Settings → Mouse/Trackpad moves it within 0.15…1.0 s.
        let secs = objc2_app_kit::NSEvent::doubleClickInterval();
        if secs.is_finite() && secs > 0.0 {
            (secs * 1000.0).round() as u32
        } else {
            500
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        400
    }
}

/// mpv draws through vo=gpu-next → Vulkan → MoltenVK, and the Vulkan loader
/// finds the driver by a JSON manifest in system directories (ours comes from
/// Homebrew). A machine without Homebrew has none, so point the loader at the
/// manifest shipped next to our libraries. This has to happen before the first
/// vkCreateInstance, i.e. before mpv is initialized.
#[cfg(target_os = "macos")]
fn point_vulkan_at_bundled_driver() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(dir) = exe.parent() else {
        return;
    };
    // dev: <exe>/lib, bundle: <exe>/../Resources/lib
    let candidates = [
        dir.join("lib").join("MoltenVK_icd.json"),
        dir.join("../Resources/lib").join("MoltenVK_icd.json"),
    ];
    if let Some(icd) = candidates.iter().find(|p| p.exists()) {
        // VK_DRIVER_FILES is the current name, VK_ICD_FILENAMES is for older
        // loaders; set both, the redundant one is simply ignored.
        std::env::set_var("VK_DRIVER_FILES", icd);
        std::env::set_var("VK_ICD_FILENAMES", icd);
    }
}


/// Forward a custom-scheme link that arrived in argv to the running window.
///
/// The deep-link plugin emits `deep-link://new-url` itself for the paths it
/// owns — the macOS Apple Event, and the first launch on Windows — but not for
/// a link that reaches an *already running* instance, because on Windows that
/// is a second process whose argv single-instance forwards. Emitting the same
/// event keeps the frontend with one subscription rather than two code paths
/// for one link.
fn deliver_deep_links(app: &tauri::AppHandle, args: &[String]) {
    let urls: Vec<String> = args
        .iter()
        .filter(|a| a.starts_with("frameplayer://"))
        .cloned()
        .collect();
    if !urls.is_empty() {
        let _ = app.emit("deep-link://new-url", urls);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "macos")]
    point_vulkan_at_bundled_driver();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // A second launch ("Open with" again, say): focus the existing
            // window and hand the file to it.
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.unminimize();
                let _ = win.set_focus();
            }
            deliver_files(app, pick_file_args(&args));
            // A `frameplayer://join/<code>` link on Windows arrives as the argv
            // of a *second* launch, which single-instance forwards here. On
            // macOS the same link is an Apple Event and the deep-link plugin
            // picks it up itself, so this half is Windows-shaped by nature —
            // `pick_file_args` above already ignores it, since it keeps only
            // arguments that exist on disk.
            deliver_deep_links(app, &args);
        }))
        // After single-instance, which is what the plugin's own documentation
        // asks for: on Windows a link opens a second process, and that process
        // has to hand over and exit rather than register itself as the handler.
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_libmpv::init())
        .manage(step_engine::StepState::default())
        .manage(thumb_service::ThumbState::default())
        .manage(PendingOpen::default())
        // An Arc rather than the value: the HTTP server's connection tasks
        // outlive any one command call and need a handle of their own.
        .manage(std::sync::Arc::new(torrent::TorrentService::default()))
        // Same shape for the same reason: the cast session task and the LAN
        // file server's connections outlive any one command call.
        .manage(std::sync::Arc::new(cast::CastService::default()))
        .manage(std::sync::Arc::new(dlna::DlnaService::default()))
        .invoke_handler(tauri::generate_handler![
            take_pending_files,
            open_file_ready,
            double_click_time,
            hdr_status,
            window_buttons,
            window_float_over_fullscreen,
            zoom_pan,
            folder_entries,
            paths_exist,
            ytdlp_path,
            oembed_title,
            ytdlp_status,
            ytdlp_install,
            ytdlp_update,
            user_mpv_conf,
            third_party_notices,
            mpv_conf_set,
            step_engine::step_prewarm,
            step_engine::step_enter,
            step_engine::step_move,
            step_engine::step_exit,
            thumb_service::thumb_start,
            thumb_service::thumb_get,
            thumb_service::container_titles,
            thumb_service::poster_frame,
            thumb_service::set_private_paths,
            thumb_service::forget_thumbs,
            thumb_service::forget_thumbs_under,
            thumb_service::clear_thumb_cache,
            torrent::torrent_add,
            torrent::torrent_status,
            torrent::torrent_set_seeding,
            torrent::torrent_set_port_forward,
            torrent::torrent_port_status,
            torrent::torrent_prefetch,
            torrent::torrent_local_path,
            torrent::torrent_offline_file,
            thumb_service::poster_capture,
            thumb_service::poster_saved,
            torrent::torrent_buffered,
            torrent::torrent_release,
            torrent::torrent_list,
            torrent::torrent_forget,
            torrent::torrent_relocate,
            torrent::torrent_clear_cache,
            torrent::torrent_forget_files,
            torrent::torrent_set_dir,
            torrent::torrent_dir,
            dlna::cast_diagnose,
            dlna::dlna_discover_start,
            dlna::dlna_discover_stop,
            dlna::dlna_devices,
            dlna::dlna_connect,
            dlna::dlna_load,
            dlna::dlna_load_url,
            dlna::dlna_status,
            dlna::dlna_control,
            dlna::dlna_disconnect,
            cast::cast_serve_torrent,
            cast::cast_load_url,
            cast::cast_discover_start,
            cast::cast_discover_stop,
            cast::cast_devices,
            cast::cast_connect,
            cast::cast_load,
            cast::cast_prepare,
            cast::cast_prepare_cached,
            cast::cast_prepare_cancel,
            cast::cast_hls_prepare,
            cast::cast_cache_size,
            cast::cast_clear_cache,
            cast::cast_forget_prepared,
            cast::cast_control,
            cast::cast_status,
            cast::cast_disconnect,
            catalog::catalog_ready,
            catalog::catalog_trending,
            catalog::catalog_search,
            catalog::catalog_details,
            catalog::catalog_releases,
            opensubtitles::subs_search,
            opensubtitles::subs_download,
            opensubtitles::subs_login,
            opensubtitles::subs_logout,
            opensubtitles::subs_account,
            opensubtitles::subs_delete_file,
            opensubtitles::release_hash,
            screenshot::screenshot_dir,
            screenshot::screenshot_clip_path,
            screenshot::screenshot_to_clipboard,
            #[cfg(target_os = "macos")]
            macos_menu::sync_window_menu,
            #[cfg(target_os = "macos")]
            macos_menu::set_menu_locale,
        ])
        .on_window_event(|window, event| {
            // The hidden `veil` shutter window keeps the app alive, so "exit
            // when all windows are closed" never fires on its own: without an
            // explicit exit, closing the player left a zombie process behind
            // (and single-instance forwarded files from Explorer into nowhere).
            if window.label() == "main" {
                match event {
                    tauri::WindowEvent::Destroyed => window.app_handle().exit(0),
                    // Every resize (fullscreen enter/exit included) re-checks
                    // the drag-border visibility; the state self-heals.
                    #[cfg(windows)]
                    tauri::WindowEvent::Resized(_) => sync_drag_resize_borders(window),
                    _ => {}
                }
            }
        })
        .setup(|app| {
            let _ = ffmpeg_the_third::init();
            // Recon only, off by default: see dlna.rs. It has to run inside the
            // app because macOS drops multicast for a process with no Local
            // Network permission, which makes a CLI probe answer "nothing here"
            // whatever the network holds.
            if std::env::var("FP_DLNA_PROBE").is_ok() {
                tauri::async_runtime::spawn(dlna::probe());
            }
            // `FP_DLNA_PLAY=<file>` drives a whole DLNA session through the
            // same commands the picker calls — how the transport is tested
            // without a hand on the mouse.
            if let Ok(path) = std::env::var("FP_DLNA_PLAY") {
                let handle = app.handle().clone();
                let target = std::env::var("FP_DLNA_TARGET").ok();
                tauri::async_runtime::spawn(async move {
                    dlna::selftest(&handle, target, path).await;
                });
            }
            // Quieten OUR copy of libavcodec. mpv links its own and installs a
            // log callback for it (its lines read `[ffmpeg/video] hevc: …`);
            // ours keeps libavcodec's default, which writes straight to stderr
            // as `[hevc @ 0x…] …` — two independent copies in one process, so
            // mpv's callback never covers this one. The thumbnail service
            // decodes whole files in software, and a chatty stream (Dolby
            // Vision RPU notices, for one) then buries everything else in the
            // dev console. Errors still get through; warnings about a file we
            // are only sampling frames from are not ours to report.
            ffmpeg_the_third::util::log::set_level(ffmpeg_the_third::util::log::Level::Error);
            if let Some(win) = app.get_webview_window("main") {
                #[cfg(target_os = "macos")]
                {
                    macos_chrome::apply(&win);
                    macos_chrome::watch_scroll_phase(app.handle());
                    // English first; the frontend owns the language choice and
                    // corrects the menu from `onMount`, before the window is
                    // shown and long before the menu bar is ever drawn.
                    if let Err(e) = macos_menu::build(app.handle(), "en") {
                        eprintln!("[macos_menu] failed to build the menu: {e}");
                    }
                }
                window_guard::clamp_to_visible_area(&win);
                // The window is created hidden (visible: false) and shown from
                // JS once it has a black background. Safety net for a broken
                // frontend: force it visible after 3 seconds.
                let fallback = win.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    let _ = fallback.show();
                });
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // `.run(context)` is replaced by `.build()` + `.run(callback)` purely for
    // RunEvent::Opened: on macOS, Finder ("Open with", double-clicking an
    // associated file) does not pass the path in argv — it arrives here as an
    // Apple Event. Without this handler the player started on an empty screen.
    app.run(|_app, _event| {
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Opened { urls } = &_event {
            let paths: Vec<String> = urls
                .iter()
                .filter_map(|u| u.to_file_path().ok())
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            deliver_files(_app, paths);
        }
    });
}
