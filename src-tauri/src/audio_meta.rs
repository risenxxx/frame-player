//! What an audio file carries besides its sound: the cover, the track number
//! and how long it runs.
//!
//! Both are read from the container header alone — `avformat_open_input`
//! without `avformat_find_stream_info`, the same arrangement as
//! `container_titles` — because both live there: FFmpeg parses an ID3 `APIC`,
//! a FLAC `PICTURE` block, an MP4 `covr` atom and a Vorbis-comment
//! `METADATA_BLOCK_PICTURE` while reading the header and hands each one back as
//! a stream with `AV_DISPOSITION_ATTACHED_PIC`, its bytes already in
//! `attached_pic`. Nothing is decoded: the picture goes to the web view as the
//! JPEG or PNG it was stored as.
//!
//! The cover is shown by the web view, not by mpv (`audio-display=no` on the
//! frontend): mpv drawing it would make it the video, and the window would take
//! its square shape, at its size — 250 px for a typical netlabel release.

use ffmpeg_the_third as ffmpeg;
use std::path::Path;

/// Larger than any cover worth showing and small enough to cross the IPC
/// bridge without a visible stall. A scan this big is a print master, not a
/// thumbnail, and the folder search skips it rather than the whole lookup.
const COVER_MAX_BYTES: u64 = 16 * 1024 * 1024;

const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "gif"];

/// Names a ripper or a store gives the front cover, in the order they win.
/// Matched against the stem, lower-cased. `AlbumArt_{GUID}_Large` is what
/// Windows Media Player writes, hence the prefix match below.
const COVER_NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart", "thumb"];

/// The cover of an audio file: the picture inside it, or one beside it.
///
/// Empty bytes for none — a file without a cover is the ordinary case and not
/// an error. Raw bytes rather than JSON, because a JSON array of a 300 KB JPEG
/// is a megabyte and a half of text.
#[tauri::command]
pub async fn cover_art(path: String) -> tauri::ipc::Response {
    let bytes = tauri::async_runtime::spawn_blocking(move || cover_bytes(&path).unwrap_or_default())
        .await
        .unwrap_or_default();
    tauri::ipc::Response::new(bytes)
}

/// Shared with the Now Playing listing, which wants the same picture.
pub fn cover_bytes(path: &str) -> Option<Vec<u8>> {
    embedded_cover(path).or_else(|| folder_cover(Path::new(path)))
}

/// `(disc, track)` per file, in the order given; `None` where the tags do not
/// say. A disc that is not written is disc 1, which is what keeps a two-disc
/// set with the disc tag on only some files in a sensible order.
#[tauri::command]
pub async fn audio_track_numbers(paths: Vec<String>) -> Vec<Option<(u32, u32)>> {
    tauri::async_runtime::spawn_blocking(move || {
        let _ = ffmpeg::init();
        paths.iter().map(|p| track_number(p)).collect()
    })
    .await
    .unwrap_or_default()
}

/// How long each file runs, in seconds, in the order given; `None` where it
/// cannot be told. A drop of audio onto a film asks this to tell a dub (as long
/// as the film) from a song (anything else) — see `looksLikeDub` on the
/// frontend.
#[tauri::command]
pub async fn audio_durations(paths: Vec<String>) -> Vec<Option<f64>> {
    tauri::async_runtime::spawn_blocking(move || paths.iter().map(|p| duration_of(p)).collect())
        .await
        .unwrap_or_default()
}

/// An open format context that is closed on every way out.
struct Header(*mut ffmpeg::sys::AVFormatContext);

impl Header {
    fn open(path: &str) -> Option<Self> {
        let _ = ffmpeg::init();
        let c_path = std::ffi::CString::new(path).ok()?;
        let mut ctx: *mut ffmpeg::sys::AVFormatContext = std::ptr::null_mut();
        // SAFETY: `avformat_open_input` fills a null local with a context it
        // allocates, given a NUL-terminated path and no format or options; on
        // failure it frees what it allocated and leaves the local null.
        let rc = unsafe {
            ffmpeg::sys::avformat_open_input(
                &mut ctx,
                c_path.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        (rc >= 0 && !ctx.is_null()).then_some(Self(ctx))
    }

    fn streams(&self) -> &[*mut ffmpeg::sys::AVStream] {
        // SAFETY: the context is open, and `streams` holds `nb_streams` valid
        // stream pointers for as long as it stays open — which `self` outlives
        // the borrow of.
        unsafe {
            let ctx = &*self.0;
            if ctx.streams.is_null() || ctx.nb_streams == 0 {
                return &[];
            }
            std::slice::from_raw_parts(ctx.streams, ctx.nb_streams as usize)
        }
    }

    /// A tag from the container, or failing that from its first audio stream:
    /// Ogg and Opus keep their comments on the stream, every other format on
    /// the container.
    fn tag(&self, key: &str) -> Option<String> {
        let key = std::ffi::CString::new(key).ok()?;
        // SAFETY: the dictionaries belong to the open context and its streams;
        // `av_dict_get` with no flags matches case-insensitively and returns an
        // entry the dictionary owns, whose value is copied out at once.
        unsafe {
            let read = |dict: *mut ffmpeg::sys::AVDictionary| -> Option<String> {
                let entry = ffmpeg::sys::av_dict_get(dict, key.as_ptr(), std::ptr::null(), 0);
                if entry.is_null() {
                    return None;
                }
                let value = std::ffi::CStr::from_ptr((*entry).value).to_string_lossy().trim().to_string();
                (!value.is_empty()).then_some(value)
            };
            read((*self.0).metadata).or_else(|| {
                self.streams().iter().find_map(|&st| {
                    let audio = ffmpeg::media::Type::from((*(*st).codecpar).codec_type) == ffmpeg::media::Type::Audio;
                    if audio { read((*st).metadata) } else { None }
                })
            })
        }
    }
}

impl Drop for Header {
    fn drop(&mut self) {
        // SAFETY: opened in `open` and closed nowhere else.
        unsafe { ffmpeg::sys::avformat_close_input(&mut self.0) };
    }
}

fn embedded_cover(path: &str) -> Option<Vec<u8>> {
    let header = Header::open(path)?;
    header.streams().iter().find_map(|&st| {
        // SAFETY: a valid stream of the open context. `attached_pic` is filled
        // while the header is read, for exactly the streams carrying this
        // disposition, and its buffer is copied out before the context closes.
        unsafe {
            let disposition = ffmpeg::format::stream::Disposition::from_bits_truncate((*st).disposition);
            if !disposition.contains(ffmpeg::format::stream::Disposition::ATTACHED_PIC) {
                return None;
            }
            let pic = &(*st).attached_pic;
            if pic.data.is_null() || pic.size <= 0 || pic.size as u64 > COVER_MAX_BYTES {
                return None;
            }
            Some(std::slice::from_raw_parts(pic.data, pic.size as usize).to_vec())
        }
    })
}

/// A picture in the file's folder that is plainly the album's cover.
///
/// Only plainly: one named after the file, one named as covers are named, or
/// the folder's only picture. A folder holding several unnamed pictures — the
/// back, the disc, the booklet — answers nothing rather than a guess, because a
/// scan of the inlay presented as the cover is worse than no cover.
fn folder_cover(file: &Path) -> Option<Vec<u8>> {
    let dir = file.parent()?;
    let stem = file.file_stem()?.to_string_lossy().to_lowercase();
    let mut images: Vec<(std::path::PathBuf, String)> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| {
            let path = e.path();
            let ext = path.extension()?.to_string_lossy().to_lowercase();
            if !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
                return None;
            }
            let name = path.file_stem()?.to_string_lossy().to_lowercase();
            if name.starts_with('.') {
                return None;
            }
            Some((path, name))
        })
        .collect();
    images.sort();
    let chosen = pick_cover(&images.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>(), &stem)?;
    let path = &images[chosen].0;
    if std::fs::metadata(path).ok()?.len() > COVER_MAX_BYTES {
        return None;
    }
    std::fs::read(path).ok()
}

/// Which of the folder's pictures (lower-cased stems, sorted) is the cover.
fn pick_cover(names: &[&str], stem: &str) -> Option<usize> {
    if let Some(i) = names.iter().position(|n| *n == stem) {
        return Some(i);
    }
    for want in COVER_NAMES {
        if let Some(i) = names.iter().position(|n| n == want || n.starts_with(&format!("{want}_"))) {
            return Some(i);
        }
    }
    (names.len() == 1).then_some(0)
}

/// The header first: FLAC's STREAMINFO, Matroska's segment duration and an MP3's
/// Xing frame are all read by `avformat_open_input`. A raw `.ac3`/`.dts`/`.aac`
/// has no such field, and only `avformat_find_stream_info` estimates one — from
/// the bitrate, which is close enough to tell a film from a song.
fn duration_of(path: &str) -> Option<f64> {
    let header = Header::open(path)?;
    header.duration().or_else(|| {
        // SAFETY: the context is open; probing reads packets into it and
        // fills in the timings, nothing it hands out outlives `header`.
        let rc = unsafe { ffmpeg::sys::avformat_find_stream_info(header.0, std::ptr::null_mut()) };
        if rc < 0 { None } else { header.duration() }
    })
}

impl Header {
    /// The container's duration, or failing that its longest stream's.
    fn duration(&self) -> Option<f64> {
        // SAFETY: the context and its streams are valid while `self` is open;
        // only plain fields are read.
        let container = unsafe { (*self.0).duration };
        if container != ffmpeg::sys::AV_NOPTS_VALUE && container > 0 {
            return Some(container as f64 / f64::from(ffmpeg::sys::AV_TIME_BASE));
        }
        self.streams()
            .iter()
            .filter_map(|&st| {
                // SAFETY: a valid stream of the open context; two plain fields
                // are copied out.
                let (d, tb) = unsafe { ((*st).duration, (*st).time_base) };
                (d != ffmpeg::sys::AV_NOPTS_VALUE && d > 0 && tb.den > 0)
                    .then(|| d as f64 * f64::from(tb.num) / f64::from(tb.den))
            })
            .reduce(f64::max)
    }
}

fn track_number(path: &str) -> Option<(u32, u32)> {
    let header = Header::open(path)?;
    let track = leading_number(&header.tag("track")?)?;
    let disc = header.tag("disc").and_then(|d| leading_number(&d)).unwrap_or(1);
    Some((disc, track))
}

/// `"3"`, `"03"` and `"3/12"` are all track three.
fn leading_number(s: &str) -> Option<u32> {
    let digits: String = s.trim().chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_numbers_come_in_every_spelling() {
        assert_eq!(leading_number("3"), Some(3));
        assert_eq!(leading_number("03"), Some(3));
        assert_eq!(leading_number("3/12"), Some(3));
        assert_eq!(leading_number(" 7 "), Some(7));
        assert_eq!(leading_number("A1"), None);
        assert_eq!(leading_number(""), None);
    }

    #[test]
    fn the_cover_is_picked_only_when_it_is_plain() {
        // Named after the file beats a generic name.
        assert_eq!(pick_cover(&["cover", "song"], "song"), Some(1));
        // The generic names, in their order.
        assert_eq!(pick_cover(&["back", "folder", "front"], "x"), Some(1));
        assert_eq!(pick_cover(&["albumart_{guid}_large", "back"], "x"), Some(0));
        // A lone picture is the cover, whatever it is called.
        assert_eq!(pick_cover(&["aydio_-_deltitnu"], "x"), Some(0));
        // Several unnamed ones: no guess.
        assert_eq!(pick_cover(&["back", "disc", "inlay"], "x"), None);
        assert_eq!(pick_cover(&[], "x"), None);
    }

    /// Every audio file under `FP_TEST_AUDIO`, with what was found for it:
    /// `FP_TEST_AUDIO=<dir> cargo test --lib audio_meta_smoke -- --nocapture`.
    #[test]
    fn audio_meta_smoke() {
        let Ok(root) = std::env::var("FP_TEST_AUDIO") else {
            return;
        };
        let mut files = Vec::new();
        let mut dirs = vec![std::path::PathBuf::from(root)];
        while let Some(dir) = dirs.pop() {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    dirs.push(p);
                } else if matches!(
                    p.extension().and_then(|x| x.to_str()),
                    Some("mp3" | "flac" | "m4a" | "opus" | "ogg")
                ) {
                    files.push(p.to_string_lossy().to_string());
                }
            }
        }
        files.sort();
        for f in &files {
            let embedded = embedded_cover(f).map(|b| b.len());
            let any = cover_bytes(f).map(|b| b.len());
            println!(
                "{f}: track {:?}, {:?} s, embedded {embedded:?}, cover {any:?}",
                track_number(f),
                duration_of(f)
            );
        }
    }
}
