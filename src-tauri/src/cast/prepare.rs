//! The ffmpeg side of preparing a file for a receiver: the cache, the
//! multi-track copy's arguments and the job runner. The commands are in mod.rs.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Deserialize;

use super::CastService;

// ---- Prepare: remux / audio-transcode into a Cast-ready MP4 ----------------
//
// The phase-2 rung, shaped by what a real television validated: video is NEVER
// re-encoded (`-c:v copy` — lossless, and the whole reason a film takes
// seconds), audio is copied when the receiver can take it and re-encoded to
// E-AC-3 640k otherwise (passthrough confirmed working on the real chain).
// The output is a complete `+faststart` MP4 in the cast cache, so the serving
// side stays the plain Range-on-a-real-file path — no HLS, per casting.md.
//
// The CLI rather than ffmpeg-the-third: `-movflags +faststart`, progress
// reporting and kill-to-cancel are free with the binary (a 0.5 MB shim over
// the DLLs the app already bundles), and the library route would mean hand
// wiring a decode→encode chain for the audio rung. macOS does not bundle the
// CLI today, so prepare is Windows-only until that is decided — the command
// fails with "no ffmpeg binary" there, which the frontend reports honestly.

/// Emitted while a prepare runs: the fraction done, 0..1.
const PREPARE_EVENT: &str = "frameplayer://cast-prepare";

/// HLS sessions live under this dot-dir inside the cast cache: transient by
/// contract (removed at disconnect, swept at the next HLS start), and the
/// dot name keeps them out of the file-only LRU maths.
pub(crate) const HLS_DIR: &str = ".hls";

#[cfg(windows)]
const FFMPEG_BIN: &str = "ffmpeg.exe";
#[cfg(not(windows))]
const FFMPEG_BIN: &str = "ffmpeg";

/// Next to the exe (dev: build.rs copies it there; Windows install: bundled to
/// the exe's own directory), then the **resource directory**, which is where it
/// lands inside a macOS `.app` — the exe there is in `Contents/MacOS` and the
/// resources in `Contents/Resources`, so "next to the exe" is not one place on
/// both platforms. The source tree is the last resort, for a dev build that
/// predates the copy step.
pub(crate) fn ffmpeg_bin(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager as _;

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(FFMPEG_BIN));
        }
    }
    if let Ok(res) = app.path().resource_dir() {
        candidates.push(res.join(FFMPEG_BIN));
    }
    let tree = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    candidates.push(tree.join("ffmpeg").join("bin").join(FFMPEG_BIN));
    candidates.push(tree.join("ffmpeg-macos").join("bin").join(FFMPEG_BIN));
    candidates.into_iter().find(|p| p.is_file())
}

/// Cache key: source identity (path + size + mtime, the thumbnail cache's
/// convention) plus what was asked of the transcode — a different audio track
/// or rung is a different output.
pub(super) fn prepare_key(path: &std::path::Path, meta: &std::fs::Metadata, audio_index: i64, transcode: bool) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.to_string_lossy().hash(&mut h);
    meta.len().hash(&mut h);
    if let Ok(modified) = meta.modified() {
        if let Ok(d) = modified.duration_since(std::time::UNIX_EPOCH) {
            d.as_secs().hash(&mut h);
        }
    }
    audio_index.hash(&mut h);
    transcode.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Prepared files are full film size (video is copied), so the cap is real.
/// LRU by mtime; the file just produced is never pruned — a cap of zero
/// therefore means "keep nothing but the current session's file", and the
/// frontend deletes that one too when the session ends.
pub(super) fn prune_cast_cache(dir: &std::path::Path, keep: &std::path::Path, cap: u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(PathBuf, u64, std::time::SystemTime)> = entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if !p.is_file() || p == keep {
                return None;
            }
            let m = e.metadata().ok()?;
            Some((p, m.len(), m.modified().ok()?))
        })
        .collect();
    let keep_size = keep.metadata().map(|m| m.len()).unwrap_or(0);
    let mut total: u64 = keep_size + files.iter().map(|(_, s, _)| s).sum::<u64>();
    files.sort_by_key(|(_, _, t)| *t);
    for (path, size, _) in files {
        if total <= cap {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= size;
        }
    }
}

/// One track of a multi-track copy: which input it comes from (`None` is the
/// video file itself, `Some` an external file mpv loaded beside it), its index
/// among that input's streams of its kind, and what the copy needs to know.
#[derive(Deserialize, Hash, Clone, Debug)]
pub struct TrackSource {
    pub file: Option<String>,
    pub index: i64,
    pub lang: Option<String>,
    pub title: Option<String>,
    /// Audio only: the codec is one the receiver does not take, so E-AC-3.
    #[serde(default)]
    pub transcode: bool,
    #[serde(default)]
    pub channels: i64,
}

/// The source's identity, every external file's identity, and the plan — a
/// subtitle file added later is a different copy.
pub(super) fn tracks_key(
    src: &std::path::Path,
    meta: &std::fs::Metadata,
    audio: &[TrackSource],
    subs: &[TrackSource],
    hevc_tag: bool,
) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    "tracks-v1".hash(&mut h);
    prepare_key(src, meta, -1, false).hash(&mut h);
    for t in audio.iter().chain(subs) {
        t.hash(&mut h);
        if let Some(f) = &t.file {
            if let Ok(m) = std::fs::metadata(f) {
                m.len().hash(&mut h);
                if let Ok(d) = m.modified().map(|t| t.duration_since(std::time::UNIX_EPOCH)) {
                    d.map(|d| d.as_secs()).unwrap_or(0).hash(&mut h);
                }
            }
        }
    }
    audio.len().hash(&mut h);
    hevc_tag.hash(&mut h);
    format!("t{:015x}", h.finish() >> 4)
}

/// Pure, so the argument list is pinned by a test: inputs deduplicated in
/// order of first use, one `-map` per track, per-stream codecs, and metadata
/// written only where the stream itself has none to carry (an external file).
fn tracks_args(src: &str, tmp: &str, audio: &[TrackSource], subs: &[TrackSource], hevc_tag: bool) -> Vec<String> {
    let mut inputs: Vec<String> = vec![src.to_string()];
    let mut input_of = |file: &Option<String>| -> usize {
        match file {
            None => 0,
            Some(f) => match inputs.iter().position(|i| i == f) {
                Some(n) => n,
                None => {
                    inputs.push(f.clone());
                    inputs.len() - 1
                }
            },
        }
    };
    let audio_maps: Vec<(usize, i64)> = audio.iter().map(|t| (input_of(&t.file), t.index)).collect();
    let sub_maps: Vec<(usize, i64)> = subs.iter().map(|t| (input_of(&t.file), t.index)).collect();

    let mut a: Vec<String> = ["-y", "-hide_banner", "-loglevel", "error", "-nostats", "-progress", "pipe:1"]
        .map(String::from)
        .to_vec();
    for i in &inputs {
        a.push("-i".into());
        a.push(i.clone());
    }
    a.extend(["-map".into(), "0:v:0".into()]);
    for (n, i) in &audio_maps {
        a.extend(["-map".into(), format!("{n}:a:{i}")]);
    }
    for (n, i) in &sub_maps {
        a.extend(["-map".into(), format!("{n}:s:{i}")]);
    }
    a.extend(["-c:v".into(), "copy".into()]);
    if hevc_tag {
        a.extend(["-tag:v".into(), "hvc1".into()]);
    }
    for (k, t) in audio.iter().enumerate() {
        if t.transcode {
            a.extend([format!("-c:a:{k}"), "eac3".into(), format!("-b:a:{k}"), "640k".into()]);
            // E-AC-3 tops out at 5.1; a stereo source must not be upmixed.
            if t.channels > 6 {
                a.extend([format!("-ac:a:{k}"), "6".into()]);
            }
        } else {
            a.extend([format!("-c:a:{k}"), "copy".into()]);
        }
    }
    if !subs.is_empty() {
        a.extend(["-c:s".into(), "mov_text".into()]);
    }
    for (kind, tracks) in [("a", audio), ("s", subs)] {
        for (k, t) in tracks.iter().enumerate() {
            if t.file.is_none() {
                continue;
            }
            if let Some(lang) = t.lang.as_deref().filter(|l| !l.is_empty()) {
                a.extend([format!("-metadata:s:{kind}:{k}"), format!("language={lang}")]);
            }
            if let Some(title) = t.title.as_deref().filter(|l| !l.is_empty()) {
                a.extend([format!("-metadata:s:{kind}:{k}"), format!("title={title}")]);
            }
        }
    }
    // A receiver that cannot be told which track to play takes the default
    // one, so the first — the chosen — is it, and no other is.
    if !audio.is_empty() {
        a.extend(["-disposition:a".into(), "0".into(), "-disposition:a:0".into(), "default".into()]);
    }
    if !subs.is_empty() {
        a.extend(["-disposition:s".into(), "0".into()]);
    }
    a.extend(["-movflags".into(), "+faststart".into(), "-f".into(), "mp4".into(), tmp.to_string()]);
    a
}

pub(super) fn tracks_command(
    ffmpeg: &std::path::Path,
    src: &std::path::Path,
    tmp: &std::path::Path,
    audio: &[TrackSource],
    subs: &[TrackSource],
    hevc_tag: bool,
) -> std::process::Command {
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.args(tracks_args(&src.to_string_lossy(), &tmp.to_string_lossy(), audio, subs, hevc_tag));
    cmd
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_prepare(
    app: &tauri::AppHandle,
    service: &Arc<CastService>,
    ffmpeg: &std::path::Path,
    src: &std::path::Path,
    tmp: &std::path::Path,
    out: &std::path::Path,
    audio_index: i64,
    transcode_audio: bool,
    channels: i64,
    hevc_tag: bool,
    duration: f64,
    cap_bytes: u64,
) -> Result<(), String> {
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.arg("-y")
        .args(["-hide_banner", "-loglevel", "error", "-nostats"])
        .args(["-progress", "pipe:1"])
        .arg("-i")
        .arg(src)
        .args(["-map", "0:v:0"])
        .args(["-map", &format!("0:a:{}", audio_index.max(0))])
        .args(["-c:v", "copy"]);
    if hevc_tag {
        // The tag the receiver's MP4 parser expects for HEVC; wrong on H.264,
        // hence conditional.
        cmd.args(["-tag:v", "hvc1"]);
    }
    if transcode_audio {
        cmd.args(["-c:a", "eac3", "-b:a", "640k"]);
        // E-AC-3 tops out at 5.1: a 7.1 TrueHD/DTS-HD core folds down, a
        // stereo source must NOT be upmixed — hence conditional.
        if channels > 6 {
            cmd.args(["-ac", "6"]);
        }
    } else {
        cmd.args(["-c:a", "copy"]);
    }
    cmd.args(["-movflags", "+faststart", "-f", "mp4"]).arg(tmp);

    if let Err(e) = run_ffmpeg_job(app, service, cmd, duration) {
        let _ = std::fs::remove_file(tmp);
        return Err(e);
    }
    std::fs::rename(tmp, out).map_err(|e| format!("{e}"))?;
    prune_cast_cache(
        out.parent().unwrap_or(std::path::Path::new(".")),
        out,
        cap_bytes,
    );
    Ok(())
}

/// Spawn one ffmpeg, park it for cancellation, forward its progress, reap it.
/// Shared by the MP4 prepare and the HLS generation — one place for the child
/// bookkeeping, the stderr drain and the µs-progress quirk.
pub(crate) fn run_ffmpeg_job(
    app: &tauri::AppHandle,
    service: &Arc<CastService>,
    mut cmd: std::process::Command,
    duration: f64,
) -> Result<(), String> {
    use std::io::BufRead as _;
    use tauri::Emitter as _;

    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        // No console window flash from a GUI process.
        use std::os::windows::process::CommandExt as _;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }

    let mut child = cmd.spawn().map_err(|e| format!("ffmpeg: {e}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // Parked in the service so `cast_prepare_cancel` can kill it. If a cancel
    // already emptied... no: this is the only writer; a leftover child from a
    // previous run was reaped below.
    *service.prepare.lock().unwrap_or_else(|p| p.into_inner()) = Some(child);

    // Drain stderr on its own thread: `-loglevel error` keeps it tiny, but a
    // filling pipe would deadlock the progress loop below.
    let err_buf = std::sync::Arc::new(Mutex::new(String::new()));
    let err_sink = err_buf.clone();
    let err_thread = std::thread::spawn(move || {
        if let Some(stderr) = stderr {
            let reader = std::io::BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                let mut buf = err_sink.lock().unwrap_or_else(|p| p.into_inner());
                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(&line);
            }
        }
    });

    // ffmpeg's `-progress` blocks: `out_time_us` is microseconds (as is the
    // misnamed `out_time_ms`, a known quirk — both are µs).
    if let Some(stdout) = stdout {
        let reader = std::io::BufReader::new(stdout);
        let mut last_pct = -1i64;
        for line in reader.lines().map_while(Result::ok) {
            let Some(value) = line
                .strip_prefix("out_time_us=")
                .or_else(|| line.strip_prefix("out_time_ms="))
            else {
                continue;
            };
            let Ok(us) = value.trim().parse::<i64>() else {
                continue;
            };
            if duration <= 0.0 {
                continue;
            }
            let frac = (us as f64 / 1_000_000.0 / duration).clamp(0.0, 1.0);
            let pct = (frac * 100.0) as i64;
            if pct != last_pct {
                last_pct = pct;
                let _ = app.emit(PREPARE_EVENT, frac);
            }
        }
    }

    let status = {
        let child = service
            .prepare
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        match child {
            Some(mut child) => child.wait().map_err(|e| format!("ffmpeg: {e}"))?,
            // Canceled from the outside: the canceler killed and reaped it.
            // The caller removes whatever partial output its mode produced.
            None => return Err("canceled".into()),
        }
    };
    let _ = err_thread.join();

    if !status.success() {
        let err = err_buf.lock().unwrap_or_else(|p| p.into_inner());
        let tail = err.lines().last().unwrap_or("ffmpeg failed").to_string();
        return Err(tail);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_args_map_every_track_and_dedupe_inputs() {
        let t = |file: Option<&str>, index: i64, transcode: bool, channels: i64| TrackSource {
            file: file.map(String::from),
            index,
            lang: file.map(|_| "rus".to_string()),
            title: None,
            transcode,
            channels,
        };
        let audio = vec![t(None, 0, false, 2), t(Some("/r/dub.mka"), 0, true, 8), t(None, 1, true, 2)];
        let subs = vec![t(None, 2, false, 0), t(Some("/r/dub.mka"), 0, false, 0), t(Some("/r/e.srt"), 0, false, 0)];
        let a = tracks_args("/r/film.mkv", "/c/x.part", &audio, &subs, true);
        let s = a.join(" ");
        // The external file shared by a dub and a subtitle is one input.
        assert_eq!(a.iter().filter(|x| *x == "-i").count(), 3);
        assert!(s.contains("-i /r/film.mkv -i /r/dub.mka -i /r/e.srt"));
        assert!(s.contains("-map 0:v:0 -map 0:a:0 -map 1:a:0 -map 0:a:1 -map 0:s:2 -map 1:s:0 -map 2:s:0"));
        assert!(s.contains("-tag:v hvc1"));
        assert!(s.contains("-c:a:0 copy"));
        assert!(s.contains("-c:a:1 eac3 -b:a:1 640k -ac:a:1 6"));
        // A stereo source transcoded is not upmixed.
        assert!(s.contains("-c:a:2 eac3 -b:a:2 640k"));
        assert!(!s.contains("-ac:a:2"));
        assert!(s.contains("-c:s mov_text"));
        // Metadata only for streams from external files.
        assert!(s.contains("-metadata:s:a:1 language=rus"));
        assert!(!s.contains("-metadata:s:a:0"));
        assert!(s.contains("-metadata:s:s:2 language=rus"));
        assert!(s.contains("-disposition:a 0 -disposition:a:0 default -disposition:s 0"));
        assert!(s.ends_with("-movflags +faststart -f mp4 /c/x.part"));
    }

    #[test]
    fn tracks_args_without_subtitles_leave_them_out() {
        let audio = vec![TrackSource { file: None, index: 0, lang: None, title: None, transcode: false, channels: 2 }];
        let s = tracks_args("/f.mp4", "/t", &audio, &[], false).join(" ");
        assert!(!s.contains("mov_text"));
        assert!(!s.contains("-disposition:s"));
        // The one audio track is the default one, for a receiver that cannot
        // be told which to play.
        assert!(s.contains("-disposition:a 0 -disposition:a:0 default"));
        assert!(!s.contains("hvc1"));
    }
}
