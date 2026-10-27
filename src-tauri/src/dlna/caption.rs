//! An external subtitle sent along with a load: SubRip or SAMI as they are,
//! anything else converted to SubRip, then registered on the casting server
//! beside the video.

use std::sync::Arc;

/// What the renderer is handed for an external subtitle. SubRip and SAMI go
/// as they are — byte for byte, whatever their encoding, which is what the
/// television meets on a USB stick and copes with the same way; anything else
/// in text (ASS, WebVTT, MicroDVD…) is turned into SubRip first, the one
/// format every renderer that takes a subtitle at all reads. The converted
/// copy lives in the casting cache under a name derived from the source, so it
/// says nothing about the file, and only for as long as the session: it is a
/// copy of what a private file contains, and making it again takes a moment.
async fn caption_file(app: &tauri::AppHandle, sub: &std::path::Path) -> Option<std::path::PathBuf> {
    if !sub.is_file() {
        return None;
    }
    let ext = sub.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if matches!(ext.as_str(), "srt" | "smi" | "sami") {
        return Some(sub.to_path_buf());
    }
    let ffmpeg = crate::cast::ffmpeg_bin(app)?;
    let dir = crate::portable::cache_dir(app).ok()?.join("cast").join(crate::cast::CAPTION_DIR);
    std::fs::create_dir_all(&dir).ok()?;
    let out = dir.join(format!("{}.srt", caption_key(sub)));
    if out.is_file() {
        return Some(out);
    }
    // One session's worth at a time: whatever an earlier one left (a switch to
    // another subtitle, a session that never reached its disconnect) goes now.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let src = sub.to_path_buf();
    let target = out.clone();
    let made = tauri::async_runtime::spawn_blocking(move || {
        let partial = target.with_extension("part.srt");
        let mut cmd = std::process::Command::new(ffmpeg);
        cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(&src)
            // `text`, not `srt`: the SubRip encoder carries an ASS style over
            // as `<font size="20">`, a size in the script's own coordinates
            // that a television draws as it likes or prints as written. Plain
            // lines with their breaks are what every renderer shows right.
            .args(["-map", "0:s:0", "-c:s", "text", "-f", "srt"])
            .arg(&partial)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let output = cmd.output().ok()?;
        if !output.status.success() {
            eprintln!(
                "[dlna] subtitle conversion failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            let _ = std::fs::remove_file(&partial);
            return None;
        }
        std::fs::rename(&partial, &target).ok()
    })
    .await
    .ok()
    .flatten();
    made.map(|_| out)
}

/// The source's identity (path, size, mtime), the convention the casting and
/// thumbnail caches already share.
fn caption_key(path: &std::path::Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.to_string_lossy().hash(&mut h);
    if let Ok(meta) = std::fs::metadata(path) {
        meta.len().hash(&mut h);
        if let Ok(d) = meta.modified().map(|m| m.duration_since(std::time::UNIX_EPOCH)) {
            d.map(|d| d.as_secs()).unwrap_or(0).hash(&mut h);
        }
    }
    format!("{:016x}", h.finish())
}

/// The subtitle beside the registration that was just made, if there is one
/// and it can be sent: its URL and type, for the metadata.
pub(super) async fn attach_subtitle(
    app: &tauri::AppHandle,
    cast_service: &Arc<crate::cast::CastService>,
    subtitle: Option<String>,
) -> Option<(String, &'static str)> {
    let file = caption_file(app, std::path::Path::new(&subtitle?)).await?;
    let url = crate::cast::attach_caption(cast_service, &file)?;
    Some((url, crate::cast::caption_mime(&file)))
}
