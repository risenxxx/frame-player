//! Screenshot destination and clipboard hand-off.
//!
//! mpv writes the frame itself (`screenshot-to-file`), which keeps the exact
//! decoded picture — including HDR tone mapping — instead of re-deriving it on
//! our side the way StepEngine's canvas path did. What mpv cannot do is create
//! the target directory or reach the clipboard, so both live here.

use tauri::Manager;

/// Directory screenshots go to, created on demand.
///
/// `screenshot-to-file` fails outright when the directory is missing, and mpv's
/// own `screenshot-directory` would put frames next to the video — acceptable
/// for a CLI player, unwelcome for a GUI one that may be pointed at a read-only
/// share or someone else's media library.
#[tauri::command]
pub fn screenshot_dir(app: tauri::AppHandle) -> Result<String, String> {
    let base = app
        .path()
        .picture_dir()
        .or_else(|_| app.path().home_dir())
        .map_err(|e| e.to_string())?;
    let dir = base.join("Frame Player");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.to_string_lossy().into_owned())
}

/// Scratch file for a frame on its way to the clipboard. A fixed name, so a
/// failed run leaves at most one stale file behind instead of littering temp.
#[tauri::command]
pub fn screenshot_clip_path(app: tauri::AppHandle) -> Result<String, String> {
    let dir = app.path().temp_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir
        .join("frameplayer-clipboard.png")
        .to_string_lossy()
        .into_owned())
}

/// Put a PNG on the clipboard and delete it.
///
/// The clipboard wants raw RGBA, so the file mpv just wrote is decoded straight
/// back — a round trip through disk, but the alternative (`screenshot-raw`)
/// returns an mpv node, and node-format reads crash the wrapper (architecture.md).
#[tauri::command]
pub fn screenshot_to_clipboard(path: String) -> Result<(), String> {
    let decoded = image::open(&path).map_err(|e| e.to_string())?.to_rgba8();
    let (width, height) = decoded.dimensions();
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard
        .set_image(arboard::ImageData {
            width: width as usize,
            height: height as usize,
            bytes: decoded.into_raw().into(),
        })
        .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&path);
    Ok(())
}
