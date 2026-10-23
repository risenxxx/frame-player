//! The HLS rendition AirPlay is given when the viewer's tracks and subtitles
//! have to reach the television: the video stream-copied into fMP4 segments,
//! every audio track as its own rendition, every text subtitle as WebVTT
//! segments, and a master playlist that offers them — switched on the
//! receiver through AVFoundation's selection groups, without a reload.
//!
//! Measured on an LG set before any of this was written (docs/airplay.md):
//! the receiver follows a selection made on the Mac for HLS renditions where
//! it ignored the same selection for an MP4, shows WebVTT where it showed no
//! `mov_text` at all, and seeks inside the stream — but refuses a variant that
//! declares 4K or HDR, which is why the frontend keeps the MP4 copy as the
//! fallback and remembers which receiver refused what.
//!
//! One ffmpeg run cuts everything, because ffmpeg's HLS muxer cannot carry
//! several subtitle variants itself (it refuses with "No streams to mux"):
//! the video and each audio track are HLS outputs of the same invocation, each
//! subtitle a plain WebVTT file that is cut into segments here, on the video's
//! own segment boundaries.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;

use crate::cast::{CastService, TrackSource};

/// What the frontend knows about the file and the tracks it wants offered.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HlsPlan {
    pub audio: Vec<TrackSource>,
    pub subs: Vec<TrackSource>,
    pub hevc_tag: bool,
    pub width: i64,
    pub height: i64,
    /// Frames per second. Not optional for HDR either: AVFoundation refused a
    /// `VIDEO-RANGE=PQ` variant that declared no `FRAME-RATE` (`-1002`,
    /// "unsupported URL" — measured on the same 4K HDR release that played
    /// once the attribute was there). Zero when unknown.
    #[serde(default)]
    pub frame_rate: f64,
    /// `PQ` or `HLG` for an HDR picture. Not optional for one: AVFoundation
    /// refuses an HDR rendition declared without it (`-12927`, measured).
    pub range: Option<String>,
    /// A file under a private root: the master names its tracks by language
    /// only, never by a title the release gave them.
    pub hidden: bool,
}

const SEGMENT_SECONDS: &str = "6";

/// Inputs in order of first use: the video file, then each external file once.
fn inputs(src: &str, plan: &HlsPlan) -> (Vec<String>, Vec<usize>, Vec<usize>) {
    let mut inputs = vec![src.to_string()];
    let mut input_of = |file: &Option<String>| -> usize {
        match file {
            None => 0,
            Some(f) => inputs.iter().position(|i| i == f).unwrap_or_else(|| {
                inputs.push(f.clone());
                inputs.len() - 1
            }),
        }
    };
    let audio: Vec<usize> = plan.audio.iter().map(|t| input_of(&t.file)).collect();
    let subs: Vec<usize> = plan.subs.iter().map(|t| input_of(&t.file)).collect();
    (inputs, audio, subs)
}

fn hls_output(init: &str, segments: &str, playlist: &str) -> Vec<String> {
    [
        "-f", "hls", "-hls_time", SEGMENT_SECONDS, "-hls_playlist_type", "vod", "-hls_segment_type", "fmp4",
        "-hls_fmp4_init_filename", init, "-hls_segment_filename", segments, playlist,
    ]
    .map(String::from)
    .to_vec()
}

/// The one ffmpeg invocation, run with the session directory as its working
/// directory so every playlist names its files bare. Pure, so pinned by tests.
pub fn hls_args(src: &str, plan: &HlsPlan) -> Vec<String> {
    let (inputs, audio_in, sub_in) = inputs(src, plan);
    let mut a: Vec<String> =
        ["-y", "-hide_banner", "-loglevel", "error", "-nostats", "-progress", "pipe:1"].map(String::from).to_vec();
    for i in &inputs {
        a.extend(["-i".into(), i.clone()]);
    }
    a.extend(["-map".into(), "0:v:0".into(), "-c:v".into(), "copy".into()]);
    if plan.hevc_tag {
        a.extend(["-tag:v".into(), "hvc1".into()]);
    }
    a.extend(hls_output("video_init.mp4", "video_%05d.m4s", "video.m3u8"));
    for (k, (t, n)) in plan.audio.iter().zip(&audio_in).enumerate() {
        a.extend(["-map".into(), format!("{n}:a:{}", t.index)]);
        if t.transcode {
            a.extend(["-c:a".into(), "eac3".into(), "-b:a".into(), "640k".into()]);
            // E-AC-3 tops out at 5.1; a stereo source must not be upmixed.
            if t.channels > 6 {
                a.extend(["-ac".into(), "6".into()]);
            }
        } else {
            a.extend(["-c:a".into(), "copy".into()]);
        }
        a.extend(hls_output(&format!("a{k}_init.mp4"), &format!("a{k}_%05d.m4s"), &format!("a{k}.m3u8")));
    }
    for (k, (t, n)) in plan.subs.iter().zip(&sub_in).enumerate() {
        a.extend([
            "-map".into(),
            format!("{n}:s:{}", t.index),
            "-c:s".into(),
            "webvtt".into(),
            "-f".into(),
            "webvtt".into(),
            format!("s{k}.vtt"),
        ]);
    }
    a
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    /// Whatever followed the end time on the timing line (position, align).
    pub settings: String,
    pub text: String,
}

/// `[hh:]mm:ss.mmm`, the two forms WebVTT allows.
fn parse_time(t: &str) -> Option<f64> {
    let parts: Vec<&str> = t.trim().split(':').collect();
    let (h, m, s) = match parts.as_slice() {
        [m, s] => (0.0, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        [h, m, s] => (h.parse::<f64>().ok()?, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        _ => return None,
    };
    Some(h * 3600.0 + m * 60.0 + s)
}

pub fn format_time(t: f64) -> String {
    let ms = (t.max(0.0) * 1000.0).round() as u64;
    format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// The cues of a WebVTT file; anything else in it (the header, NOTE, STYLE,
/// REGION blocks) is not a cue and is left behind.
pub fn parse_vtt(text: &str) -> Vec<Cue> {
    let normalized = text.replace("\r\n", "\n");
    let mut cues = Vec::new();
    for block in normalized.split("\n\n") {
        let lines: Vec<&str> = block.lines().collect();
        let Some(at) = lines.iter().position(|l| l.contains("-->")) else { continue };
        let timing = lines[at];
        let Some((a, rest)) = timing.split_once("-->") else { continue };
        let rest = rest.trim();
        let (b, settings) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let (Some(start), Some(end)) = (parse_time(a), parse_time(b)) else { continue };
        let body = lines[at + 1..].join("\n");
        if body.trim().is_empty() {
            continue;
        }
        cues.push(Cue { start, end, settings: settings.trim().to_string(), text: body });
    }
    cues
}

/// `#EXTINF` durations of a media playlist, in order.
pub fn segment_durations(playlist: &str) -> Vec<f64> {
    playlist
        .lines()
        .filter_map(|l| l.strip_prefix("#EXTINF:"))
        .filter_map(|l| l.split(',').next()?.trim().parse().ok())
        .collect()
}

/// A subtitle track cut into one WebVTT segment per video segment, each with
/// every cue that overlaps it (a cue spanning a boundary is repeated, as the
/// HLS spec asks, and the player shows it once). The timestamp map anchors
/// cue time zero to media time zero: ffmpeg's fMP4 segments start at the
/// source's own zero (measured), unlike its TS segments, which start at 1.4 s.
pub fn cut_vtt(cues: &[Cue], durations: &[f64], prefix: &str) -> (String, Vec<(String, String)>) {
    let target = durations.iter().cloned().fold(0.0_f64, f64::max).ceil().max(1.0) as u64;
    let mut playlist = format!(
        "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:{target}\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-PLAYLIST-TYPE:VOD\n"
    );
    let mut files = Vec::new();
    let mut start = 0.0;
    for (i, d) in durations.iter().enumerate() {
        let end = start + d;
        let mut body = String::from("WEBVTT\nX-TIMESTAMP-MAP=MPEGTS:0,LOCAL:00:00:00.000\n");
        for c in cues.iter().filter(|c| c.start < end && c.end > start) {
            body.push('\n');
            body.push_str(&format_time(c.start));
            body.push_str(" --> ");
            body.push_str(&format_time(c.end));
            if !c.settings.is_empty() {
                body.push(' ');
                body.push_str(&c.settings);
            }
            body.push('\n');
            body.push_str(&c.text);
            body.push('\n');
        }
        let name = format!("{prefix}_{i:05}.vtt");
        playlist.push_str(&format!("#EXTINF:{d:.6},\n{name}\n"));
        files.push((name, body));
        start = end;
    }
    playlist.push_str("#EXT-X-ENDLIST\n");
    (playlist, files)
}

/// A rendition's NAME: the release's title for the track, else its language,
/// else a number — and unique within its group, which the spec requires.
fn names(tracks: &[TrackSource], hidden: bool, fallback: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (k, t) in tracks.iter().enumerate() {
        let title = if hidden { None } else { t.title.as_deref().filter(|s| !s.trim().is_empty()) };
        let base = title
            .or(t.lang.as_deref().filter(|s| !s.trim().is_empty()))
            .map(|s| s.replace('"', "'"))
            .unwrap_or_else(|| format!("{fallback} {}", k + 1));
        let mut name = base.clone();
        let mut n = 2;
        while out.contains(&name) {
            name = format!("{base} ({n})");
            n += 1;
        }
        out.push(name);
    }
    out
}

fn language(t: &TrackSource) -> String {
    t.lang
        .as_deref()
        .filter(|l| !l.trim().is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .map(|l| format!(",LANGUAGE=\"{l}\""))
        .unwrap_or_default()
}

/// The master playlist. Its audio and subtitle groups are in the plan's order,
/// which is what `airplay_select`'s indices count in.
pub fn master(plan: &HlsPlan, bandwidth: u64) -> String {
    let mut m = String::from("#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n");
    for (k, (t, name)) in plan.audio.iter().zip(names(&plan.audio, plan.hidden, "Audio")).enumerate() {
        let default = if k == 0 { "YES" } else { "NO" };
        m.push_str(&format!(
            "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"{name}\"{},DEFAULT={default},AUTOSELECT=YES,URI=\"a{k}.m3u8\"\n",
            language(t)
        ));
    }
    for (k, (t, name)) in plan.subs.iter().zip(names(&plan.subs, plan.hidden, "Subtitles")).enumerate() {
        m.push_str(&format!(
            "#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"sub\",NAME=\"{name}\"{},DEFAULT=NO,AUTOSELECT=YES,FORCED=NO,URI=\"s{k}.m3u8\"\n",
            language(t)
        ));
    }
    let mut inf = format!("#EXT-X-STREAM-INF:BANDWIDTH={bandwidth}");
    if plan.width > 0 && plan.height > 0 {
        inf.push_str(&format!(",RESOLUTION={}x{}", plan.width, plan.height));
    }
    if plan.frame_rate > 0.0 {
        inf.push_str(&format!(",FRAME-RATE={:.3}", plan.frame_rate));
    }
    if let Some(range) = plan.range.as_deref().filter(|r| *r == "PQ" || *r == "HLG") {
        inf.push_str(&format!(",VIDEO-RANGE={range}"));
    }
    if !plan.audio.is_empty() {
        inf.push_str(",AUDIO=\"aud\"");
    }
    if !plan.subs.is_empty() {
        inf.push_str(",SUBTITLES=\"sub\"");
    }
    m.push_str(&inf);
    m.push_str("\nvideo.m3u8\n");
    m
}

/// Where a session's rendition lives: beside the Cast HLS sessions, so "clear
/// the casting cache" and its leftover sweep cover these too.
pub fn sessions_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(crate::portable::cache_dir(app)?.join("cast").join(crate::cast::HLS_DIR))
}

/// A directory this module made, and so one it may delete.
pub fn is_session_dir(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("ap-"))
        && path.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()) == Some(crate::cast::HLS_DIR)
}

/// Cut the rendition. Returns the session directory, to be served whole.
#[tauri::command]
pub async fn airplay_hls_prepare(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<CastService>>,
    path: String,
    plan: HlsPlan,
    duration: f64,
) -> Result<String, String> {
    let src = PathBuf::from(&path);
    let size = src.metadata().map_err(|e| e.to_string())?.len();
    let root = sessions_root(&app)?;
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    // Sessions a crash left behind; never the one being served right now.
    let served = crate::cast::served_path(service.inner());
    if let Ok(entries) = std::fs::read_dir(&root) {
        for e in entries.flatten() {
            let p = e.path();
            if is_session_dir(&p) && Some(&p) != served.as_ref() {
                let _ = std::fs::remove_dir_all(&p);
            }
        }
    }
    let dir = root.join(format!("ap-{:016x}", rand::random::<u64>()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let ffmpeg = crate::cast::ffmpeg_bin(&app).ok_or("no ffmpeg binary")?;
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.current_dir(&dir).args(hls_args(&src.to_string_lossy(), &plan));

    let service = service.inner().clone();
    let work = dir.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        crate::cast::run_ffmpeg_job(&app, &service, cmd, duration)?;
        let video = std::fs::read_to_string(work.join("video.m3u8")).map_err(|e| e.to_string())?;
        let durations = segment_durations(&video);
        for k in 0..plan.subs.len() {
            let vtt_path = work.join(format!("s{k}.vtt"));
            let cues = parse_vtt(&std::fs::read_to_string(&vtt_path).unwrap_or_default());
            let (playlist, files) = cut_vtt(&cues, &durations, &format!("s{k}"));
            for (name, body) in files {
                std::fs::write(work.join(name), body).map_err(|e| e.to_string())?;
            }
            std::fs::write(work.join(format!("s{k}.m3u8")), playlist).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_file(vtt_path);
        }
        // An estimate is all the receiver needs: one variant, nothing to pick
        // between. The file's average rate with headroom for its peaks.
        let bandwidth = if duration > 0.0 { (size as f64 * 8.0 / duration * 1.3) as u64 } else { 0 }.max(2_000_000);
        std::fs::write(work.join("master.m3u8"), master(&plan, bandwidth)).map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?;
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e);
    }
    Ok(dir.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(file: Option<&str>, index: i64, lang: Option<&str>, title: Option<&str>, transcode: bool, channels: i64) -> TrackSource {
        TrackSource {
            file: file.map(String::from),
            index,
            lang: lang.map(String::from),
            title: title.map(String::from),
            transcode,
            channels,
        }
    }

    fn plan() -> HlsPlan {
        HlsPlan {
            audio: vec![t(None, 0, Some("rus"), Some("Dub"), false, 6), t(Some("/r/en.mka"), 0, Some("eng"), None, true, 8)],
            subs: vec![t(None, 1, Some("eng"), Some("Full"), false, 0), t(Some("/r/en.mka"), 0, Some("eng"), Some("Full"), false, 0)],
            hevc_tag: true,
            width: 3840,
            height: 2160,
            frame_rate: 23.976,
            range: Some("PQ".into()),
            hidden: false,
        }
    }

    #[test]
    fn one_invocation_writes_every_rendition() {
        let s = hls_args("/r/film.mkv", &plan()).join(" ");
        // The external file shared by a dub and a subtitle is one input.
        assert!(s.contains("-i /r/film.mkv -i /r/en.mka -map 0:v:0 -c:v copy -tag:v hvc1"));
        assert_eq!(s.matches(" -i ").count(), 2);
        assert!(s.contains("-hls_segment_type fmp4 -hls_fmp4_init_filename video_init.mp4 -hls_segment_filename video_%05d.m4s video.m3u8"));
        assert!(s.contains("-map 0:a:0 -c:a copy -f hls"));
        assert!(s.contains("-map 1:a:0 -c:a eac3 -b:a 640k -ac 6 -f hls"));
        assert!(s.contains("a1_init.mp4 -hls_segment_filename a1_%05d.m4s a1.m3u8"));
        assert!(s.contains("-map 0:s:1 -c:s webvtt -f webvtt s0.vtt"));
        assert!(s.contains("-map 1:s:0 -c:s webvtt -f webvtt s1.vtt"));
    }

    #[test]
    fn webvtt_is_parsed_in_both_time_forms_and_cut_on_segment_boundaries() {
        let vtt = "WEBVTT\n\nSTYLE\n::cue { color: yellow }\n\n1\n00:01.000 --> 00:04.500 align:start\nHello\nthere\n\n00:00:05.000 --> 00:00:13.000\nLong\n\n00:20.000 --> 00:21.000\n\n";
        let cues = parse_vtt(vtt);
        assert_eq!(cues.len(), 2, "STYLE and the empty cue are not cues");
        assert_eq!(cues[0], Cue { start: 1.0, end: 4.5, settings: "align:start".into(), text: "Hello\nthere".into() });
        let (playlist, files) = cut_vtt(&cues, &[6.0, 6.0, 6.0], "s0");
        assert_eq!(files.len(), 3);
        assert!(files[0].1.starts_with("WEBVTT\nX-TIMESTAMP-MAP=MPEGTS:0,LOCAL:00:00:00.000\n"));
        assert!(files[0].1.contains("00:00:01.000 --> 00:00:04.500 align:start\nHello\nthere"));
        // The long cue spans the boundary at 6 and 12 s, so it is in all three.
        assert!(files[0].1.contains("Long") && files[1].1.contains("Long") && files[2].1.contains("Long"));
        assert!(!files[1].1.contains("Hello"));
        assert!(playlist.contains("#EXT-X-TARGETDURATION:6\n"));
        assert!(playlist.contains("#EXTINF:6.000000,\ns0_00002.vtt\n"));
        assert!(playlist.ends_with("#EXT-X-ENDLIST\n"));
    }

    #[test]
    fn durations_come_from_the_video_playlist() {
        let p = "#EXTM3U\n#EXT-X-MAP:URI=\"video_init.mp4\"\n#EXTINF:6.000000,\nvideo_00000.m4s\n#EXTINF:4.250000,\nvideo_00001.m4s\n#EXT-X-ENDLIST\n";
        assert_eq!(segment_durations(p), vec![6.0, 4.25]);
        assert_eq!(format_time(3725.5), "01:02:05.500");
    }

    #[test]
    fn the_master_declares_what_the_receiver_decides_by() {
        let m = master(&plan(), 20_000_000);
        assert!(m.contains("TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"Dub\",LANGUAGE=\"rus\",DEFAULT=YES,AUTOSELECT=YES,URI=\"a0.m3u8\""));
        assert!(m.contains("NAME=\"eng\",LANGUAGE=\"eng\",DEFAULT=NO"));
        // Two subtitle tracks titled alike get distinct names.
        assert!(m.contains("NAME=\"Full\",LANGUAGE=\"eng\""));
        assert!(m.contains("NAME=\"Full (2)\""));
        assert!(m.contains("#EXT-X-STREAM-INF:BANDWIDTH=20000000,RESOLUTION=3840x2160,FRAME-RATE=23.976,VIDEO-RANGE=PQ,AUDIO=\"aud\",SUBTITLES=\"sub\"\nvideo.m3u8\n"));
    }

    #[test]
    fn a_private_file_names_its_tracks_by_language_only() {
        let mut p = plan();
        p.hidden = true;
        p.range = None;
        p.subs.clear();
        let m = master(&p, 1);
        assert!(!m.contains("Dub"));
        assert!(m.contains("NAME=\"rus\""));
        assert!(!m.contains("VIDEO-RANGE"));
        assert!(!m.contains("SUBTITLES="));
        assert!(m.contains("BANDWIDTH=1,"));
    }

    #[test]
    fn only_its_own_directories_may_be_deleted() {
        assert!(is_session_dir(Path::new("/c/cast/.hls/ap-00ff")));
        assert!(!is_session_dir(Path::new("/c/cast/.hls/0123abcd")));
        assert!(!is_session_dir(Path::new("/films/ap-film")));
    }
}
