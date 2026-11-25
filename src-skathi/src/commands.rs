//! The player's small Rust commands, as plain functions the `native:mpv` module exports. Each one
//! mirrors the command of the same name in version 1's `src-tauri/src` (cited per function), minus
//! Tauri. Paths come from version 2's own data and resource directories (`data_dir`,
//! `resource_dir`), so nothing here reads or writes version 1's settings.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{json, Value};

use crate::sys;

const MPV_CONF_TEMPLATE: &str = include_str!("mpv.conf.template");

/// The beta's identifier, which names its data folder: version 1's is `app.frameplayer`, and the
/// two must never share one (`docs/v2-beta-release.md`, "Identity", on the
/// `docs/native-frames` branch of the repository).
pub const APP_ID: &str = "app.frameplayer.beta";

/// Where version 2 keeps its state: Skathi's storage, settings files, caches, fonts, screenshots.
/// `FPUI_DATA_DIR` overrides it (tests, isolated runs); otherwise the per-user application data
/// folder under [`APP_ID`]. On Windows that is the roaming one (`%APPDATA%`), where version 1 keeps
/// its own and where the first Velopack betas kept theirs, so an update finds its settings.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FPUI_DATA_DIR") {
        return PathBuf::from(dir);
    }
    #[cfg(target_os = "macos")]
    let base = PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Library/Application Support");
    #[cfg(windows)]
    let base = PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default());
    base.join(APP_ID)
}

/// An installed copy: the payload lies beside the executable on Windows (a Velopack install, or a
/// folder laid out like one), in `Contents/Resources` of an app bundle on macOS; not in the source
/// tree.
pub fn installed() -> bool {
    installed_payload().is_some()
}

/// `…/X.app/Contents` when this executable runs from `X.app/Contents/MacOS`.
#[cfg(target_os = "macos")]
pub fn bundle_contents() -> Option<&'static Path> {
    static CONTENTS: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    CONTENTS
        .get_or_init(|| {
            let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
            let macos = exe.parent()?;
            let contents = macos.parent()?;
            (macos.file_name()? == "MacOS" && contents.file_name()? == "Contents").then(|| contents.to_path_buf())
        })
        .as_deref()
}

/// Where an installed copy keeps its read-only files: `Contents/Resources` of the bundle (macOS),
/// the executable's folder (Windows).
fn installed_root() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    return bundle_contents().map(|c| c.join("Resources"));
    #[cfg(windows)]
    return std::env::current_exe().ok()?.parent().map(Path::to_path_buf);
}

fn installed_payload() -> Option<PathBuf> {
    let path = installed_root()?.join("app.skpk");
    path.exists().then_some(path)
}

/// What Tauri calls the resource directory: `lua/`, `shaders/` and the notices. `FPUI_RESOURCES`,
/// else an installed copy's (the bundle's `Contents/Resources` on macOS, `resources/` beside the
/// executable on Windows), else the one `scripts/stage.mjs` fills in the source tree (a development
/// build).
pub fn resource_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FPUI_RESOURCES") {
        return PathBuf::from(dir);
    }
    #[cfg(target_os = "macos")]
    if let Some(res) = installed_root().filter(|r| r.join("lua").is_dir()) {
        return res;
    }
    beside_exe("resources").unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources"))
}

/// The interface's payload: `FPUI_PAYLOAD`, else an installed copy's `app.skpk`, else the one
/// `sk publish --payload-only` writes in the source tree.
pub fn payload_file() -> PathBuf {
    if let Some(file) = std::env::var_os("FPUI_PAYLOAD") {
        return PathBuf::from(file);
    }
    installed_payload()
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("app/dist/release/.sk/app.skpk"))
}

fn beside_exe(name: &str) -> Option<PathBuf> {
    let path = std::env::current_exe().ok()?.parent()?.join(name);
    path.exists().then_some(path)
}

/// Version 2's mpv.conf directory (version 1's is `app.frameplayer`'s config folder).
fn config_dir() -> Result<PathBuf, String> {
    let dir = data_dir().join("config");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    Ok(dir)
}

/// lib.rs `folder_entries`: every plain, non-hidden file next to `path`, unsorted.
pub fn folder_entries(path: &str) -> Result<Vec<String>, String> {
    let parent = Path::new(path).parent().ok_or_else(|| "the file has no parent directory".to_string())?;
    let dir = std::fs::read_dir(parent).map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    for entry in dir.flatten() {
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(true) {
            continue;
        }
        if entry.file_name().to_str().is_some_and(|n| n.starts_with('.')) {
            continue;
        }
        if let Some(full) = entry.path().to_str() {
            files.push(full.to_string());
        }
    }
    Ok(files)
}

/// lib.rs `paths_exist`: which of these paths still exist, in order.
pub fn paths_exist(paths: &[String]) -> Vec<bool> {
    paths.iter().map(|p| Path::new(p).exists()).collect()
}

// ---- external_dirs.rs `external_track_dirs`: folders below the file's own that hold subtitles or
// audio tracks, as relative paths mpv takes in `sub-file-paths` / `audio-file-paths`.

const SUB_EXTS: &[&str] = &[
    "ass", "idx", "lrc", "mks", "pgs", "rt", "sbv", "scc", "smi", "srt", "ssa", "sub", "sup", "utf", "utf-8", "utf8", "vtt",
];
const AUDIO_EXTS: &[&str] = &[
    "3gp", "aac", "ac3", "aif", "aiff", "amr", "ape", "au", "awb", "dts", "eac3", "flac", "m4a", "mka", "mp3", "oga", "ogg",
    "ogm", "opus", "thd", "wav", "wma", "wv",
];
const MAX_DEPTH: usize = 3;
const MAX_ENTRIES: usize = 4000;

fn ext_of(name: &str) -> Option<String> {
    Path::new(name).extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase())
}

fn dir_name(entry: &std::fs::DirEntry) -> Option<String> {
    let ft = entry.file_type().ok()?;
    if !ft.is_dir() || ft.is_symlink() {
        return None;
    }
    let name = entry.file_name().to_str()?.to_string();
    if name.starts_with('.') || name.contains(':') || name.contains(';') {
        return None;
    }
    Some(name)
}

fn scan(root: &Path) -> (Vec<String>, Vec<String>) {
    let (mut subs, mut audio) = (Vec::new(), Vec::new());
    let mut queue: VecDeque<(PathBuf, String, usize)> = VecDeque::new();
    let mut budget = MAX_ENTRIES;
    let Ok(top) = std::fs::read_dir(root) else { return (subs, audio) };
    for entry in top.flatten() {
        if budget == 0 {
            break;
        }
        budget -= 1;
        if let Some(name) = dir_name(&entry) {
            queue.push_back((entry.path(), name, 1));
        }
    }
    while let Some((dir, rel, depth)) = queue.pop_front() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let (mut has_sub, mut has_audio) = (false, false);
        for entry in rd.flatten() {
            if budget == 0 {
                break;
            }
            budget -= 1;
            if let Some(name) = dir_name(&entry) {
                if depth < MAX_DEPTH {
                    queue.push_back((entry.path(), format!("{rel}/{name}"), depth + 1));
                }
                continue;
            }
            let Some(ext) = entry.file_name().to_str().and_then(ext_of) else { continue };
            has_sub |= SUB_EXTS.contains(&ext.as_str());
            has_audio |= AUDIO_EXTS.contains(&ext.as_str());
        }
        if has_sub {
            subs.push(rel.clone());
        }
        if has_audio {
            audio.push(rel);
        }
    }
    (subs, audio)
}

pub fn external_track_dirs(path: &str) -> Value {
    let Some(parent) = Path::new(path).parent() else { return json!({ "subs": [], "audio": [] }) };
    let (subs, audio) = scan(parent);
    json!({ "subs": subs, "audio": audio })
}

// ---- lib.rs `user_mpv_conf` / `mpv_conf_set`

/// mpv.conf, created from the template on first use, parsed as `key=value` (`wid` never passes).
pub fn user_mpv_conf() -> Result<Value, String> {
    let path = config_dir()?.join("mpv.conf");
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
            if k.is_empty() || k == "wid" {
                continue;
            }
            options.push(json!([k, v]));
        }
    }
    Ok(json!({ "path": path.to_string_lossy(), "options": options }))
}

/// Changes only the last uncommented line with `key` (or appends one, or comments it out for `None`).
pub fn mpv_conf_set(key: &str, value: Option<&str>) -> Result<(), String> {
    let path = config_dir()?.join("mpv.conf");
    let text = if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| e.to_string())?
    } else {
        MPV_CONF_TEMPLATE.to_string()
    };
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut target = None;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if t.split_once('=').is_some_and(|(k, _)| k.trim() == key) {
            target = Some(i);
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

/// lib.rs `third_party_notices`: the notices from the resource directory.
pub fn third_party_notices() -> Result<String, String> {
    let path = resource_dir().join("THIRD-PARTY-NOTICES.md");
    std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
}

// ---- screenshot.rs

/// Where screenshots go. Version 1's is `~/Pictures/Frame Player`; version 2 keeps its own until
/// the settings move (V2-3).
pub fn screenshot_dir() -> Result<String, String> {
    let dir = data_dir().join("screenshots");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.to_string_lossy().into_owned())
}

/// The scratch PNG on its way to the clipboard: one fixed name in the temp directory.
pub fn screenshot_clip_path() -> String {
    std::env::temp_dir().join("fpui-clipboard.png").to_string_lossy().into_owned()
}

/// Puts the PNG mpv wrote on the clipboard and deletes it. The player decodes it to RGBA for
/// arboard; NSPasteboard takes the PNG bytes as they are.
pub fn screenshot_to_clipboard(path: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    sys::clipboard_png(bytes)?;
    let _ = std::fs::remove_file(path);
    Ok(())
}

/// trash.rs `trash_file`: an existing regular file at an absolute path, to the Trash and nowhere
/// else. (The player also lets its storyboard decoder go of the file first; there is none here.)
pub fn trash_file(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if !p.is_absolute() {
        return Err("not an absolute path".into());
    }
    let meta = std::fs::metadata(p).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a file".into());
    }
    sys::trash(path)
}

// ---- lib.rs `debug_log_state` / `debug_log_set`: the verbose switch, live and forgotten at
// restart. Nothing in version 2 logs more when it is on yet.

static VERBOSE: AtomicBool = AtomicBool::new(false);
static SINCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// `{ verbose, since, from_env }`, `since` in milliseconds since the epoch and null while off.
pub fn debug_log_state() -> Value {
    let on = VERBOSE.load(Ordering::Relaxed);
    let since = SINCE.load(Ordering::Relaxed);
    json!({ "verbose": on, "since": if on { json!(since) } else { Value::Null }, "from_env": false })
}

pub fn debug_log_set(on: bool) -> Value {
    if on && !VERBOSE.load(Ordering::Relaxed) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        SINCE.store(now, Ordering::Relaxed);
    }
    VERBOSE.store(on, Ordering::Relaxed);
    debug_log_state()
}

/// lib.rs `mpv_shader_cache_dir`: where mpv keeps compiled shaders (libmpv has none by default).
pub fn mpv_shader_cache_dir() -> Result<String, String> {
    let dir = data_dir().join("cache/shaders");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    Ok(dir.to_string_lossy().into_owned())
}

/// The verbose switch, for what logs more when it is on (`logs::verbose`, the cast discovery).
pub fn verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}
