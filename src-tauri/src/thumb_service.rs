//! ThumbService: thumbnails for the seekbar preview.
//!
//! A separate FFmpeg session (so it does not disturb StepEngine's GOP ring):
//! a frame is decoded at the requested position into a ~320px JPEG. A
//! background storyboard warms up thumbnails across the whole file and stores
//! them in a disk cache (keyed by path + size + mtime), so reopening is instant.
//!
//! A single keyframe is not enough: on a file whose GOP is longer than the grid
//! step (a 17-second camera clip has keyframes every 5.3 s against a 0.25 s
//! step), seeking to the nearest keyframe returns the same frame for a couple
//! of dozen neighbouring cells and the preview appears stuck. So after the seek
//! the frame is refined by decoding forward to the exact position — but only
//! when it would otherwise duplicate the neighbouring cell (see `frame_at`),
//! since on a long film that would mean decoding the entire file for a
//! difference the eye cannot see.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::color;
use ffmpeg::media;
use ffmpeg::software::scaling;
use ffmpeg::util::frame::video::Video;
use ffmpeg_the_third as ffmpeg;
use tauri::Manager;

const THUMB_WIDTH: u32 = 320;
const JPEG_QUALITY: u8 = 78;
const MAX_BUCKETS: usize = 1000;
/// How many grid cells we would like per file (the step is clamped to
/// [MIN_INTERVAL, MAX_INTERVAL] regardless).
const TARGET_BUCKETS: f64 = 240.0;
const MIN_INTERVAL: f64 = 0.25;
const MAX_INTERVAL: f64 = 10.0;
/// Safety valve for files with a pathologically long GOP: the maximum number
/// of frames decoded forward while refining a thumbnail to the exact position.
const MAX_FORWARD_FRAMES: u32 = 900;
/// Thread ceiling for the thumbnail decoder. Scaling by core count took three
/// cores on 4K60 and stole them from playback — the storyboard is in no hurry,
/// the player is.
const DECODE_THREADS: usize = 3;
/// Duty cycle of the background storyboard: after each cell it rests for
/// 1/YIELD_RATIO of the time that cell took, but never longer than
/// MAX_YIELD_MS. Needed even with lowered QoS — Windows and Linux have no QoS
/// classes at all.
///
/// The ratio was tuned twice: 1:1 (rest equals work) together with
/// QOS_BACKGROUND was too much, and a three-minute 4K60 storyboard crawled for
/// minutes.
const YIELD_RATIO: u32 = 3;
const MAX_YIELD_MS: u64 = 120;
/// The same duty cycle on battery, where the trade is the other way round: the
/// storyboard rests *twice* as long as it works, so a pass costs roughly a
/// third of a core instead of two and takes about three times as long.
///
/// Both halves are needed. Measured on a 4K HEVC file, an expensive cell takes
/// ~1.3 s, so on mains the 120 ms ceiling — not the 1:3 ratio — is what
/// actually decides the rate: 1.3 s of work against 120 ms of rest is a ~90 %
/// duty cycle, which is the 164–208 % of processor a fresh 4K file spends for
/// its first ~40 seconds. Raising the ratio without raising the ceiling would
/// change nothing at all on exactly the files where this matters.
const BATTERY_YIELD_MULT: u32 = 2;
const BATTERY_MAX_YIELD_MS: u64 = 2000;
/// Past this distance, decoding forward is certainly costlier than a new seek.
const MAX_CONTINUE_SECS: f64 = 6.0;
/// The same, for the exact hover preview. Much tighter than MAX_CONTINUE_SECS:
/// there the bound is the grid step, here a keyframe seek puts us at most one
/// GOP behind the target, so continuing forward only pays off over distances
/// shorter than a typical GOP.
const MAX_EXACT_CONTINUE_SECS: f64 = 1.0;
const PTS_EPS: f64 = 1e-4;
/// "KTB6". v3 was the bump for keyframe-only caches (v2 held duplicates instead
/// of frames at the requested positions on long-GOP files). v4 is when the
/// source path joined the header: the file
/// name is a hash of path + size + mtime, so without the path inside there was
/// no way to answer "which cached storyboards belong to this folder" — and that
/// is exactly what excluding a folder has to be able to do. **v5 is the colour
/// conversion** (`color.rs`): the cache key is a hash of the file, not of how it
/// was decoded, so without a bump every storyboard already on disk would keep
/// serving frames made with the wrong matrix — permanently, and for exactly the
/// files the change was made for, since a Dolby Vision release is one somebody
/// has already hovered over. Old caches are simply not read and get regenerated
/// in the background. **v6 is the exposure calibration in `DOLBY_WHITE`**, and
/// the rule generalises: this cache is keyed by the file, so *any* change to
/// how a frame is turned into a picture has to come with a bump, or the files
/// the change was for keep the answer it replaced.
const CACHE_MAGIC: u32 = 0x4B54_4236;

struct ThumbSession {
    ictx: ffmpeg::format::context::Input,
    decoder: ffmpeg::decoder::Video,
    /// The last successfully decoded frame and its position in seconds.
    frame: Video,
    /// Receiver for `avcodec_receive_frame`: it calls `av_frame_unref` before
    /// any check, so decoding straight into `frame` is not an option — a failed
    /// call (EOF while decoding forward) would wipe the frame already obtained.
    scratch: Video,
    has_frame: bool,
    cur_pts: Option<f64>,
    /// `send_eof` has been sent — no new packets may follow (reset by a seek).
    eof_sent: bool,
    /// The file's GOP is longer than the grid step, so thumbnails have to be
    /// refined by decoding. Knowing this lets the storyboard run linearly
    /// instead of re-seeking the same GOP for every cell.
    refine_mode: bool,
    scaler: Option<(ffmpeg::format::Pixel, u32, u32, scaling::Context)>,
    /// How this file's colour is encoded, when swscale cannot be trusted with
    /// it. `None` is the ordinary SDR case and the fast path; see `color.rs`.
    hdr: Option<color::Hdr>,
    stream_index: usize,
    time_base: f64,
    start_offset: f64,
    out_w: u32,
    out_h: u32,
}

unsafe impl Send for ThumbSession {}

impl ThumbSession {
    fn open(path: &str) -> Result<Self, String> {
        let ictx = ffmpeg::format::input(&path).map_err(|e| format!("open: {e}"))?;
        let stream = ictx
            .streams()
            .best(media::Type::Video)
            .ok_or("no video stream")?;
        let stream_index = stream.index();
        let tb = stream.time_base();
        let time_base = tb.numerator() as f64 / tb.denominator() as f64;
        let start_offset = {
            let st = stream.start_time();
            if st == i64::MIN {
                0.0
            } else {
                st as f64 * time_base
            }
        };
        let mut dec_ctx = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
            .map_err(|e| format!("codec ctx: {e}"))?;
        // Refining a thumbnail to the exact position can decode a whole GOP,
        // so threads pay off here — but strictly bounded ones: this is
        // background work and must not claim the whole machine.
        dec_ctx.set_threading(ffmpeg::codec::threading::Config {
            kind: ffmpeg::codec::threading::Type::Frame,
            count: DECODE_THREADS,
            ..Default::default()
        });
        let decoder = dec_ctx
            .decoder()
            .video()
            .map_err(|e| format!("decoder: {e}"))?;
        let (vw, vh) = (decoder.width().max(1), decoder.height().max(1));
        // Anamorphic streams (SAR != 1:1) would otherwise be stretched
        // horizontally: measure from the display width, not the pixel width.
        let sar = decoder.aspect_ratio();
        let disp_w = if sar.numerator() > 0 && sar.denominator() > 0 {
            vw as f64 * sar.numerator() as f64 / sar.denominator() as f64
        } else {
            vw as f64
        };
        let out_w = (THUMB_WIDTH.min(disp_w.round().max(2.0) as u32)).max(2) & !1;
        let out_h = (((out_w as f64 * vh as f64 / disp_w).round().max(2.0)) as u32).max(2) & !1;
        let hdr = Self::detect_hdr(&stream, &decoder);
        Ok(ThumbSession {
            ictx,
            decoder,
            frame: Video::empty(),
            scratch: Video::empty(),
            has_frame: false,
            cur_pts: None,
            eof_sent: false,
            refine_mode: false,
            scaler: None,
            hdr,
            stream_index,
            time_base,
            start_offset,
            out_w,
            out_h,
        })
    }

    /// Does this frame need the colour path in `color.rs`, or can swscale have
    /// it?
    ///
    /// **Dolby Vision is asked about first, because a profile 5 stream carries
    /// no colour tags at all** — the file this was written for reports
    /// `yuv420p10le(tv)` and nothing else, since the colour lives in the RPU.
    /// Only profile 5 (and its ancestor 4) encode the picture as IPT rather
    /// than Y'CbCr; 7 and 8 have an HDR10-compatible base layer, so they are
    /// already handled by the transfer characteristic below and must not be
    /// dragged through the IPT path.
    fn detect_hdr(
        stream: &ffmpeg::format::stream::Stream,
        decoder: &ffmpeg::decoder::Video,
    ) -> Option<color::Hdr> {
        if matches!(dolby_profile(stream), Some(4) | Some(5)) {
            return Some(color::Hdr::Dolby5);
        }
        use ffmpeg::util::color::TransferCharacteristic as Trc;
        match decoder.color_transfer_characteristic() {
            Trc::SMPTE2084 => Some(color::Hdr::Pq),
            Trc::ARIB_STD_B67 => Some(color::Hdr::Hlg),
            _ => None,
        }
    }

    fn next_video_packet(&mut self) -> Option<ffmpeg::Packet> {
        for res in self.ictx.packets() {
            let Ok((stream, packet)) = res else {
                return None;
            };
            if stream.index() == self.stream_index {
                return Some(packet);
            }
        }
        None
    }

    fn seek_to_keyframe(&mut self, pos: f64) -> Result<(), String> {
        let ts = ((pos + self.start_offset).max(0.0) * ffmpeg::ffi::AV_TIME_BASE as f64) as i64;
        // ..=ts (inclusive!): with an exclusive bound max_ts = ts-1 < ts, and
        // avformat_seek_file rejects the call with EPERM.
        self.ictx
            .seek(ts, ..=ts)
            .map_err(|e| format!("seek: {e}"))?;
        self.decoder.flush();
        self.eof_sent = false;
        self.cur_pts = None;
        Ok(())
    }

    /// Decodes the next frame into `self.frame` (false means the stream ended).
    fn decode_next(&mut self) -> Result<bool, String> {
        loop {
            if self.decoder.receive_frame(&mut self.scratch).is_ok() {
                std::mem::swap(&mut self.frame, &mut self.scratch);
                self.has_frame = true;
                // Without PTS (broken or raw streams) time-based refinement is
                // impossible — cur_pts stays None and the thumbnail degrades to
                // a keyframe.
                self.cur_pts = self
                    .frame
                    .timestamp()
                    .or(self.frame.pts())
                    .map(|ts| ts as f64 * self.time_base - self.start_offset);
                return Ok(true);
            }
            if self.eof_sent {
                return Ok(false);
            }
            match self.next_video_packet() {
                Some(packet) => self
                    .decoder
                    .send_packet(&packet)
                    .map_err(|e| format!("send_packet: {e}"))?,
                None => {
                    // Threaded decoding lags by thread_count frames — after EOF
                    // they still have to be drained.
                    let _ = self.decoder.send_eof();
                    self.eof_sent = true;
                }
            }
        }
    }

    /// The frame at `target` (`interval` is the thumbnail grid step): (pts, JPEG).
    fn frame_at(&mut self, target: f64, interval: f64) -> Result<(f64, Vec<u8>), String> {
        // The storyboard runs forward: if the file is already known to have a
        // long GOP and the session sits a little behind the target, decoding
        // forward is cheaper than a seek that re-decodes the same GOP from its
        // start.
        let continue_forward = self.refine_mode
            && self.cur_pts.is_some_and(|cur| {
                target >= cur - PTS_EPS && target - cur <= interval.min(MAX_CONTINUE_SECS) + PTS_EPS
            });

        let mut refine = true;
        if !continue_forward {
            self.seek_to_keyframe(target)?;
            if !self.decode_next()? {
                return Err("no frame decoded".into());
            }
            // Refine only if the keyframe landed further away than the grid
            // step: otherwise it is already unique for this cell, and decoding
            // a GOP is expensive (on a long film the 10 s step is usually
            // longer than the GOP, so no refinement happens).
            refine = target - self.cur_pts.unwrap_or(0.0) >= interval - PTS_EPS;
            self.refine_mode = refine;
        }
        if refine {
            let mut budget = MAX_FORWARD_FRAMES;
            while budget > 0 && self.cur_pts.is_some_and(|p| p < target - PTS_EPS) {
                budget -= 1;
                // A frame is already in hand, so an error or EOF while
                // decoding forward means stop, not lose the thumbnail.
                if !self.decode_next().unwrap_or(false) {
                    break;
                }
            }
        }
        let jpeg = self.encode_current()?;
        Ok((self.cur_pts.unwrap_or(0.0), jpeg))
    }

    /// The frame at exactly `pos`, ignoring the grid: (pts, JPEG).
    ///
    /// Used when the cursor comes to rest over the seekbar — that position is
    /// the moment being aimed at, and the grid cell containing it is up to half
    /// a step away. Measured on a 24-minute file (6 s step): the cell's frame
    /// belongs to a different scene than the one playback lands on in 37 % of
    /// hover positions even after rounding, 60 % before it.
    ///
    /// Deliberately does NOT touch `refine_mode`: that flag says what the
    /// background storyboard has to do for this file, and a hover request must
    /// not push the whole background pass into linear decoding.
    fn frame_exact_at(&mut self, pos: f64) -> Result<(f64, Vec<u8>), String> {
        let pos = pos.max(0.0);
        let can_continue = self
            .cur_pts
            .is_some_and(|cur| pos >= cur - PTS_EPS && pos - cur <= MAX_EXACT_CONTINUE_SECS);
        if !can_continue {
            self.seek_to_keyframe(pos)?;
            if !self.decode_next()? {
                return Err("no frame decoded".into());
            }
        }
        let mut budget = MAX_FORWARD_FRAMES;
        while budget > 0 && self.cur_pts.is_some_and(|p| p < pos - PTS_EPS) {
            budget -= 1;
            // A frame is already in hand, so an error or EOF while decoding
            // forward means stop, not lose the thumbnail.
            if !self.decode_next().unwrap_or(false) {
                break;
            }
        }
        let jpeg = self.encode_current()?;
        Ok((self.cur_pts.unwrap_or(0.0), jpeg))
    }

    /// The current frame, scaled to the thumbnail size and packed tight (the
    /// scaler's rows are padded). Shared by the encoder and the scorer so a
    /// candidate is scaled once, not twice.
    fn scaled_rgb(&mut self) -> Result<Vec<u8>, String> {
        if !self.has_frame {
            return Err("no frame decoded".into());
        }
        let fmt = self.frame.format();
        let (fw, fh) = (self.frame.width(), self.frame.height());
        // HDR leaves swscale doing the resize and nothing else: the target is
        // 16-bit planar YUV, which is a format change and a scale with no
        // matrix in it, and `color.rs` takes it from there.
        let target = match self.hdr {
            Some(_) => ffmpeg::format::Pixel::YUV444P16LE,
            None => ffmpeg::format::Pixel::RGB24,
        };
        let rebuild = match &self.scaler {
            Some((sf, sw, sh, _)) => *sf != fmt || *sw != fw || *sh != fh,
            None => true,
        };
        if rebuild {
            let mut ctx = scaling::Context::get(
                fmt,
                fw,
                fh,
                target,
                self.out_w,
                self.out_h,
                // heavy downscale (4K -> 320px): FAST_BILINEAR is far cheaper
                scaling::Flags::FAST_BILINEAR,
            )
            .map_err(|e| format!("scaler: {e}"))?;
            if self.hdr.is_none() {
                set_sdr_colorspace(&mut ctx, &self.frame);
            }
            self.scaler = Some((fmt, fw, fh, ctx));
        }
        let mut out = Video::empty();
        self.scaler
            .as_mut()
            .unwrap()
            .3
            .run(&self.frame, &mut out)
            .map_err(|e| format!("scale: {e}"))?;

        let (w, h) = (self.out_w as usize, self.out_h as usize);
        if let Some(kind) = self.hdr {
            // `data()` hands back bytes; the planes are 16-bit samples, and the
            // stride is in bytes, so both have to be divided by two.
            let plane = |i: usize| -> (&[u16], usize) {
                let bytes = out.data(i);
                let s = out.stride(i) / 2;
                let n = s * h;
                let words =
                    unsafe { std::slice::from_raw_parts(bytes.as_ptr() as *const u16, n.min(bytes.len() / 2)) };
                (words, s)
            };
            let (y, sy) = plane(0);
            let (u, su) = plane(1);
            let (v, sv) = plane(2);
            let primaries = match self.frame.color_primaries() {
                ffmpeg::util::color::Primaries::BT709 => color::Primaries::Bt709,
                _ => color::Primaries::Bt2020,
            };
            let full = self.frame.color_range() == ffmpeg::util::color::Range::JPEG;
            return Ok(color::yuv444_16_to_srgb(
                [y, u, v],
                [sy, su, sv],
                w,
                h,
                kind,
                primaries,
                full,
            ));
        }
        let stride = out.stride(0);
        let row = w * 3;
        let data = out.data(0);
        let mut tight = Vec::with_capacity(row * h);
        for y in 0..h {
            tight.extend_from_slice(&data[y * stride..y * stride + row]);
        }
        Ok(tight)
    }

    fn encode_current(&mut self) -> Result<Vec<u8>, String> {
        let tight = self.scaled_rgb()?;
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, JPEG_QUALITY)
            .encode(
                &tight,
                self.out_w,
                self.out_h,
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|e| format!("jpeg: {e}"))?;
        Ok(jpeg)
    }
}

/// The Dolby Vision profile this stream declares, if any.
///
/// FFmpeg 8 keeps stream-level side data on the codec parameters
/// (`coded_side_data`), not on `AVStream` where it used to live — the crate's
/// own `Stream::side_data` reads the old field and finds nothing. The payload
/// is an `AVDOVIDecoderConfigurationRecord`, i.e. a struct rather than the raw
/// box bytes, so the profile is a field read and not a bit unpack.
fn dolby_profile(stream: &ffmpeg::format::stream::Stream) -> Option<u8> {
    /// `dovi_meta.h` is outside the sys crate's bindings, and the record is
    /// nine bytes of `uint8_t` whose order is fixed by the Dolby specification
    /// the header cites — so it is declared here rather than reached for. Only
    /// the length is trusted from outside: the size check below is what makes
    /// reading this pointer safe if FFmpeg ever grows the struct.
    #[repr(C)]
    struct DoviRecord {
        version_major: u8,
        version_minor: u8,
        profile: u8,
        level: u8,
        rpu_present: u8,
        el_present: u8,
        bl_present: u8,
        bl_signal_compatibility_id: u8,
        md_compression: u8,
    }
    unsafe {
        let par = stream.parameters();
        let par = par.as_ptr();
        if par.is_null() {
            return None;
        }
        let list = (*par).coded_side_data;
        let n = (*par).nb_coded_side_data;
        if list.is_null() || n <= 0 {
            return None;
        }
        for i in 0..n as isize {
            let sd = &*list.offset(i);
            if sd.type_ != ffmpeg::sys::AVPacketSideDataType::DOVI_CONF {
                continue;
            }
            if sd.data.is_null() || sd.size < std::mem::size_of::<DoviRecord>() {
                return None;
            }
            return Some((*(sd.data as *const DoviRecord)).profile);
        }
        None
    }
}

/// Tell swscale what the frame's colour actually is.
///
/// Without this it uses its own default coefficients — BT.601 — for everything,
/// which is a hue error on every BT.709 file and a large one on anything wider.
/// Measured on a BT.2020 frame: up to 32/255 out on the green channel. The
/// destination is RGB, so it is full range by definition; a failure to set the
/// details is not worth reporting, since what follows is exactly the behaviour
/// there was before.
fn set_sdr_colorspace(ctx: &mut scaling::Context, frame: &Video) {
    use ffmpeg::util::color::{Range, Space};
    let src = match frame.color_space() {
        Space::BT709 => ffmpeg::sys::SWS_CS_ITU709,
        Space::BT2020NCL | Space::BT2020CL => ffmpeg::sys::SWS_CS_BT2020,
        Space::SMPTE240M => ffmpeg::sys::SWS_CS_SMPTE240M,
        Space::FCC => ffmpeg::sys::SWS_CS_FCC,
        Space::BT470BG | Space::SMPTE170M => ffmpeg::sys::SWS_CS_ITU601,
        // Unspecified is the common case for SD and for anything hand-muxed,
        // and BT.709 is the right guess for everything this player will meet:
        // swscale's own default of 601 is a guess about videotapes.
        _ => ffmpeg::sys::SWS_CS_ITU709,
    };
    let full = i32::from(frame.color_range() == Range::JPEG);
    unsafe {
        let coeff = ffmpeg::sys::sws_getCoefficients(src as i32);
        let dst = ffmpeg::sys::sws_getCoefficients(ffmpeg::sys::SWS_CS_ITU709 as i32);
        ffmpeg::sys::sws_setColorspaceDetails(
            ctx.as_mut_ptr(),
            coeff,
            full,
            dst,
            1,
            0,
            1 << 16,
            1 << 16,
        );
    }
}

struct ThumbInner {
    path: String,
    session: Option<ThumbSession>,
    interval: f64,
    /// Known only once `thumb_start` has run; 0 until then. Needed to keep
    /// rounding from addressing a cell past the end of the file.
    duration: f64,
    /// bucket -> (pts, jpeg)
    thumbs: HashMap<u32, (f64, Vec<u8>)>,
    cache_file: Option<PathBuf>,
    dirty: bool,
}

impl Default for ThumbInner {
    fn default() -> Self {
        ThumbInner {
            path: String::new(),
            session: None,
            interval: 5.0,
            duration: 0.0,
            thumbs: HashMap::new(),
            cache_file: None,
            dirty: false,
        }
    }
}

#[derive(Default)]
pub struct ThumbState {
    inner: Arc<Mutex<ThumbInner>>,
    generation: Arc<AtomicU64>,
    /// Number of pending hover requests: background generation yields to them.
    pending: Arc<AtomicU64>,
}

fn lock(arc: &Arc<Mutex<ThumbInner>>) -> MutexGuard<'_, ThumbInner> {
    arc.lock().unwrap_or_else(|p| p.into_inner())
}

// ---- Private paths --------------------------------------------------------
// Folders inside which the player leaves no traces on disk. Kept in a static
// rather than in Tauri state, because the check is needed deep in the cache
// layer, where no AppHandle reaches.

#[derive(Default)]
struct PrivacyState {
    /// History is off entirely — everything is private, folders are not consulted.
    all: bool,
    roots: Vec<String>,
}

static PRIVACY: std::sync::OnceLock<Mutex<PrivacyState>> = std::sync::OnceLock::new();

fn privacy() -> &'static Mutex<PrivacyState> {
    PRIVACY.get_or_init(|| Mutex::new(PrivacyState::default()))
}

/// Whether the file lives inside a private folder. Matching is on a path
/// component boundary — otherwise "/Movies" would swallow "/Movies2" — and
/// case-insensitive: macOS and Windows file systems are so by default.
pub fn is_private(path: &str) -> bool {
    let state = privacy().lock().unwrap_or_else(|e| e.into_inner());
    if state.all {
        return true;
    }
    state.roots.iter().any(|root| path_under(path, root))
}

/// Is `path` inside `root`? Matching is on a component boundary — otherwise
/// "/Movies" would swallow "/Movies2" — and case-insensitive, because macOS and
/// Windows file systems are so by default. Mirrored in JS (`isPrivatePath`);
/// change one, change the other.
fn path_under(path: &str, root: &str) -> bool {
    // Separators normalized on **both** sides, matching the JS twin
    // (`pathUnder` in history.svelte.ts). The two strings come from different
    // places — a folder picked in the OS dialog against whatever mpv reports as
    // `path` — and on Windows they disagree about the slash direction. Accepting
    // either only on the *path* side, which is what this did, leaves a root
    // spelled `E:\Films` failing to match `E:/Films/a.mkv`: a privacy root that
    // does not match is a leak, so this is the one comparison that must not be
    // laxer than the queue's `samePath`, which has normalized all along.
    let norm = |s: &str| s.to_lowercase().replace('\\', "/");
    let root = norm(root);
    let root = root.trim_end_matches('/');
    if root.is_empty() {
        return false;
    }
    let p = norm(path);
    p == root || p.starts_with(&format!("{root}/"))
}

/// Delete every cached storyboard for files inside a folder.
///
/// Called when a folder is added to the exclusions: hiding those videos from
/// the start screen while their frames stay on disk is the wrong half of a
/// privacy control. This is what the source path in the cache header is for —
/// the file name is a hash, so before v4 an entry could not be attributed to a
/// folder at all.
///
/// Entries in an older format carry no path and cannot be attributed either, so
/// they are removed as well: they are unreadable to the current build anyway
/// and would otherwise be the one place an excluded video could still be found.
#[tauri::command]
pub fn forget_thumbs_under(app: tauri::AppHandle, folder: String) -> usize {
    // Saved posters are the same kind of thing kept in a different file, so a
    // folder excluded from the history must take them with it — one caller,
    // both stores, or the second one is the leak nobody checks.
    purge_posters(&app, &folder);
    let Some(dir) = thumbs_dir(&app) else {
        return 0;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let file = entry.path();
        if file.extension().and_then(|s| s.to_str()) != Some("ktb") {
            continue;
        }
        let doomed = match load_cache(&file) {
            Some((source, _, _)) => path_under(&source, &folder),
            None => true,
        };
        if doomed && std::fs::remove_file(&file).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// `all` means history is off entirely. A separate flag rather than a `/`
/// root: such a root gets trimmed to an empty string and would match nothing.
#[tauri::command]
pub fn set_private_paths(paths: Vec<String>, all: bool) {
    let mut state = privacy().lock().unwrap_or_else(|e| e.into_inner());
    state.all = all;
    state.roots = paths;
}

fn thumbs_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_cache_dir().ok()?.join("thumbs");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Forget one file: delete its storyboard from disk.
#[tauri::command]
pub fn forget_thumbs(app: tauri::AppHandle, path: String) {
    if let Some(file) = cache_path_for(&app, &path) {
        let _ = std::fs::remove_file(file);
    }
}

/// Delete the whole thumbnail disk cache (the "clear history" action).
#[tauri::command]
pub fn clear_thumb_cache(app: tauri::AppHandle) {
    // "Clear the thumbnail cache" means every frame this player kept, and a
    // saved poster is a frame this player kept.
    if let Some(posters) = posters_dir(&app) {
        if let Ok(entries) = std::fs::read_dir(posters) {
            for e in entries.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let Some(dir) = thumbs_dir(&app) else { return };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) == Some("ktb") {
            let _ = std::fs::remove_file(p);
        }
    }
}

fn cache_path_for(app: &tauri::AppHandle, path: &str) -> Option<PathBuf> {
    // A private file gets no cache file at all: without one, load/save_cache
    // become no-ops and thumbnails live only in session memory.
    if is_private(path) {
        return None;
    }
    let meta = std::fs::metadata(path).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    meta.len().hash(&mut h);
    if let Ok(m) = meta.modified() {
        if let Ok(d) = m.duration_since(std::time::UNIX_EPOCH) {
            d.as_secs().hash(&mut h);
        }
    }
    Some(thumbs_dir(app)?.join(format!("{:016x}.ktb", h.finish())))
}

/// Poster frame for the start screen: [f64 pts LE][JPEG].
///
/// A throwaway session rather than the shared ThumbState: the start screen
/// shows several posters at once, and running them through the shared session
/// would reset it for every file. A ready frame from the storyboard is tried
/// first (then there is no decoding at all), and only failing that is a single
/// keyframe decoded: a poster needs speed, not precision.
#[tauri::command]
pub async fn poster_frame(
    app: tauri::AppHandle,
    path: String,
    pos: f64,
) -> Result<tauri::ipc::Response, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !std::path::Path::new(&path).exists() {
            return Err("file is gone".to_string());
        }
        if let Some(cf) = cache_path_for(&app, &path) {
            if let Some((_, interval, map)) = load_cache(&cf) {
                // No duration here, and none needed: a miss simply falls
                // through to decoding a keyframe below.
                let bucket = bucket_for(pos, interval, 0.0);
                if let Some((pts, jpeg)) = map.get(&bucket) {
                    return Ok(respond(*pts, jpeg));
                }
            }
        }
        let mut session = ThumbSession::open(&path)?;
        let (pts, jpeg) = session.frame_at(pos.max(0.0), MAX_INTERVAL)?;
        Ok(respond(pts, &jpeg))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn respond(pts: f64, jpeg: &[u8]) -> tauri::ipc::Response {
    let mut out = Vec::with_capacity(8 + jpeg.len());
    out.extend_from_slice(&pts.to_le_bytes());
    out.extend_from_slice(jpeg);
    tauri::ipc::Response::new(out)
}

/// Returns the source path alongside the frames — the path is what makes a
/// cache entry attributable to a folder.
fn load_cache(file: &PathBuf) -> Option<(String, f64, HashMap<u32, (f64, Vec<u8>)>)> {
    let mut f = std::fs::File::open(file).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let mut off = 0usize;
    let take = |off: &mut usize, n: usize| -> Option<&[u8]> {
        let s = buf.get(*off..*off + n)?;
        *off += n;
        Some(s)
    };
    let magic = u32::from_le_bytes(take(&mut off, 4)?.try_into().ok()?);
    if magic != CACHE_MAGIC {
        return None;
    }
    let source_len = u32::from_le_bytes(take(&mut off, 4)?.try_into().ok()?) as usize;
    let source = String::from_utf8(take(&mut off, source_len)?.to_vec()).ok()?;
    let count = u32::from_le_bytes(take(&mut off, 4)?.try_into().ok()?);
    let interval = f64::from_le_bytes(take(&mut off, 8)?.try_into().ok()?);
    let mut map = HashMap::new();
    for _ in 0..count {
        let bucket = u32::from_le_bytes(take(&mut off, 4)?.try_into().ok()?);
        let pts = f64::from_le_bytes(take(&mut off, 8)?.try_into().ok()?);
        let len = u32::from_le_bytes(take(&mut off, 4)?.try_into().ok()?) as usize;
        let data = take(&mut off, len)?.to_vec();
        map.insert(bucket, (pts, data));
    }
    Some((source, interval, map))
}

/// Disk cache ceiling for thumbnails. One film is a few megabytes, so this is
/// one or two hundred watched files; the oldest are evicted first.
const CACHE_BUDGET_BYTES: u64 = 256 * 1024 * 1024;

/// The cache is keyed by path + size + mtime, so a file that was overwritten
/// or renamed leaves an orphan behind — the directory never shrinks on its own.
/// Pruning is by budget: sort by mtime and delete the oldest until it fits.
fn prune_cache(dir: &std::path::Path, keep: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = entries
        .filter_map(|e| {
            let e = e.ok()?;
            let path = e.path();
            if path.extension().and_then(|s| s.to_str()) != Some("ktb") {
                return None;
            }
            let meta = e.metadata().ok()?;
            Some((
                meta.modified().unwrap_or(std::time::UNIX_EPOCH),
                meta.len(),
                path,
            ))
        })
        .collect();

    let mut total: u64 = files.iter().map(|(_, len, _)| *len).sum();
    if total <= CACHE_BUDGET_BYTES {
        return;
    }
    // Oldest first.
    files.sort_by_key(|(mtime, _, _)| *mtime);
    for (_, len, path) in files {
        if total <= CACHE_BUDGET_BYTES {
            break;
        }
        // Never touch the current file's cache, even if it is the oldest:
        // the storyboard is writing to it right now.
        if path == keep {
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}

fn save_cache(file: &PathBuf, source: &str, interval: f64, thumbs: &HashMap<u32, (f64, Vec<u8>)>) {
    let mut out = Vec::new();
    out.extend_from_slice(&CACHE_MAGIC.to_le_bytes());
    // The video this belongs to. Only ever read to decide whether the entry is
    // inside a folder the viewer has excluded — the cache is still addressed by
    // the hashed name.
    let source = source.as_bytes();
    out.extend_from_slice(&(source.len() as u32).to_le_bytes());
    out.extend_from_slice(source);
    out.extend_from_slice(&(thumbs.len() as u32).to_le_bytes());
    out.extend_from_slice(&interval.to_le_bytes());
    for (bucket, (pts, jpeg)) in thumbs {
        out.extend_from_slice(&bucket.to_le_bytes());
        out.extend_from_slice(&pts.to_le_bytes());
        out.extend_from_slice(&(jpeg.len() as u32).to_le_bytes());
        out.extend_from_slice(jpeg);
    }
    if let Ok(mut f) = std::fs::File::create(file) {
        let _ = f.write_all(&out);
    }
    if let Some(dir) = file.parent() {
        prune_cache(dir, file);
    }
}

/// Resets the state for a new file (if it changed).
fn ensure_path(inner: &mut ThumbInner, path: &str) {
    if inner.path != path {
        inner.path = path.to_string();
        inner.session = None;
        inner.duration = 0.0;
        inner.thumbs.clear();
        inner.cache_file = None;
        inner.dirty = false;
    }
}

/// Number of grid cells the storyboard generates for a file.
fn bucket_count(duration: f64, interval: f64) -> usize {
    ((duration / interval).ceil() as usize).clamp(1, MAX_BUCKETS)
}

/// The grid cell for a position: the NEAREST one, not the one containing it.
///
/// Truncation made the preview systematically earlier than the position being
/// aimed at — by up to a full step, which is 10 s on anything longer than
/// 40 minutes — while the seek that follows a click is exact. The two errors
/// therefore added instead of canceling, and the previewed frame was reliably
/// behind the frame playback started from. Measured on a 24-minute file (6 s
/// step, 30 hover positions): the preview showed a different scene in 60 % of
/// them with truncation, 37 % with rounding.
///
/// Cell N still holds the frame at `N*interval`, so this changes only the
/// lookup — caches written before it stay valid.
fn bucket_for(pos: f64, interval: f64, duration: f64) -> u32 {
    let bucket = (pos.max(0.0) / interval + 0.5) as u32;
    if duration > 0.0 {
        // Rounding up near the end of the file would otherwise address a cell
        // the storyboard never makes, at a position the decoder cannot reach.
        bucket.min(bucket_count(duration, interval).saturating_sub(1) as u32)
    } else {
        bucket
    }
}

fn thumb_at(inner: &mut ThumbInner, pos: f64) -> Result<(u32, Vec<u8>), String> {
    let interval = inner.interval;
    let bucket = bucket_for(pos, interval, inner.duration);
    if let Some((_pts, jpeg)) = inner.thumbs.get(&bucket) {
        return Ok((bucket, jpeg.clone()));
    }
    if inner.session.is_none() {
        inner.session = Some(ThumbSession::open(&inner.path)?);
    }
    let target = bucket as f64 * interval;
    let (pts, jpeg) = inner.session.as_mut().unwrap().frame_at(target, interval)?;
    inner.thumbs.insert(bucket, (pts, jpeg.clone()));
    inner.dirty = true;
    Ok((bucket, jpeg))
}

/// Thumbnail grid step: ~TARGET_BUCKETS per file, but no finer than
/// MIN_INTERVAL, no coarser than MAX_INTERVAL and no more than MAX_BUCKETS.
fn interval_for(duration: f64) -> f64 {
    (duration / TARGET_BUCKETS)
        .clamp(MIN_INTERVAL, MAX_INTERVAL)
        .max(duration / MAX_BUCKETS as f64)
}

/// Lowers the service class of the current thread.
///
/// The worker threads ffmpeg creates for frame-threaded decoding inherit their
/// creator's QoS, so one call at thread start is enough. A no-op elsewhere
/// (there the YIELD_RATIO duty cycle does the job).
///
/// UTILITY, not BACKGROUND: on Apple Silicon, BACKGROUND pins the thread to
/// efficiency cores entirely, and a three-minute 4K60 storyboard stretched to
/// minutes — thumbnails were not ready by the time they were reached for.
/// UTILITY means "long work with a user-visible result": performance cores stay
/// available, but the player preempts the storyboard rather than the reverse.
fn background_qos() {
    #[cfg(target_os = "macos")]
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
    }
}

/// Starts background storyboard generation for a file.
#[tauri::command]
pub fn thumb_start(
    app: tauri::AppHandle,
    state: tauri::State<'_, ThumbState>,
    path: String,
    duration: f64,
) {
    let gen = state.generation.fetch_add(1, Ordering::SeqCst) + 1;
    let inner_arc = state.inner.clone();
    let gen_arc = state.generation.clone();
    let pending_arc = state.pending.clone();

    {
        let mut inner = lock(&inner_arc);
        ensure_path(&mut inner, &path);
        let new_interval = interval_for(duration);
        // Early hover thumbnails may have been stored at the default interval —
        // with a new interval their bucket numbers mean different positions.
        if (inner.interval - new_interval).abs() > 1e-9 && !inner.thumbs.is_empty() {
            inner.thumbs.clear();
        }
        inner.interval = new_interval;
        inner.duration = duration;
        inner.cache_file = cache_path_for(&app, &path);
        if let Some(cf) = inner.cache_file.clone() {
            if let Some((_, interval, map)) = load_cache(&cf) {
                inner.interval = interval;
                inner.thumbs = map;
                inner.dirty = false;
            }
        }
    }

    std::thread::spawn(move || {
        background_qos();
        let (interval, buckets) = {
            let inner = lock(&inner_arc);
            (inner.interval, bucket_count(duration, inner.interval))
        };
        // A storyboard on a long file runs for minutes: without intermediate
        // saves, closing the player halfway threw all the work away.
        let mut last_save = std::time::Instant::now();
        for b in 0..buckets as u32 {
            if gen_arc.load(Ordering::SeqCst) != gen {
                return;
            }
            // hover requests outrank background generation — yield to them
            let mut waited = 0u32;
            while pending_arc.load(Ordering::SeqCst) > 0 && waited < 1000 {
                std::thread::sleep(std::time::Duration::from_millis(5));
                waited += 5;
            }
            let started = std::time::Instant::now();
            {
                let mut inner = lock(&inner_arc);
                if inner.path != path {
                    return;
                }
                if !inner.thumbs.contains_key(&b) {
                    let _ = thumb_at(&mut inner, b as f64 * interval);
                }
                if inner.dirty && last_save.elapsed() >= std::time::Duration::from_secs(10) {
                    if let Some(cf) = inner.cache_file.clone() {
                        save_cache(&cf, &inner.path, inner.interval, &inner.thumbs);
                        inner.dirty = false;
                    }
                    last_save = std::time::Instant::now();
                }
            }
            // Duty cycle: rest for a fraction of what was spent. On cheap
            // cells (cache hit, keyframe) that is the same ~5 ms as before; on
            // expensive ones (refining a whole 4K GOP) it is tens of
            // milliseconds during which the machine belongs to the player. The
            // mutex is released meanwhile, so hover requests skip the queue.
            //
            // On battery the same arithmetic runs with the other pair of
            // numbers, asked per cell so that unplugging is noticed within a
            // few of them. Only the *background* pass changes: a hover request
            // is somebody waiting, and it decodes at full speed on either
            // supply.
            let spent = started.elapsed();
            let rest = if crate::power::on_battery() {
                (spent * BATTERY_YIELD_MULT)
                    .min(std::time::Duration::from_millis(BATTERY_MAX_YIELD_MS))
            } else {
                (spent / YIELD_RATIO).min(std::time::Duration::from_millis(MAX_YIELD_MS))
            };
            std::thread::sleep(rest.max(std::time::Duration::from_millis(5)));
        }
        let mut inner = lock(&inner_arc);
        if inner.path == path && inner.dirty {
            if let Some(cf) = inner.cache_file.clone() {
                save_cache(&cf, &inner.path, inner.interval, &inner.thumbs);
                inner.dirty = false;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The privacy predicate, against the vectors the JS twin also reads.
    ///
    /// `shared/path-under.txt` is the contract, not this file: `path_under` here
    /// and `pathUnder` in src/lib/format.ts are one rule written in two
    /// languages, and a disagreement between them is a leak with nothing on
    /// screen to show for it — this is the copy that decides whether a thumbnail
    /// lands on disk for a file inside an excluded folder. Reading the cases
    /// from a shared file rather than restating them is what makes "change one,
    /// change the other" enforceable: a case added there fails on whichever side
    /// does not already agree.
    ///
    /// `include_str!` rather than a runtime read, so the vectors travel with the
    /// binary and rustc rebuilds the test when they change.
    #[test]
    fn path_under_agrees_with_the_shared_vectors() {
        let vectors = include_str!("../../shared/path-under.txt");
        let mut checked = 0;
        for (n, line) in vectors.lines().enumerate() {
            let line = line.trim_end_matches(['\r', '\n']);
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split('\t');
            let (Some(path), Some(root), Some(want), None) =
                (parts.next(), parts.next(), parts.next(), parts.next())
            else {
                panic!("line {}: expected <path> TAB <root> TAB yes|no, got {line:?}", n + 1);
            };
            let want = match want.trim() {
                "yes" => true,
                "no" => false,
                other => panic!("line {}: expected yes|no, got {other:?}", n + 1),
            };
            assert_eq!(
                path_under(path, root),
                want,
                "line {}: path_under({path:?}, {root:?})",
                n + 1,
            );
            checked += 1;
        }
        // A vectors file that parsed to nothing — moved, renamed, or reformatted
        // so every line looks like a comment — would otherwise pass in silence,
        // which is the one way a shared contract can quietly stop being one. A
        // floor rather than an exact count, so adding a case does not mean
        // editing two test files to let it in.
        assert!(checked >= 15, "only {checked} vectors parsed — did the file move?");
    }

    /// FP_TEST_VIDEO=<path> cargo test container_title_smoke -- --nocapture
    ///
    /// Prints rather than asserts a value: most files carry no title tag at
    /// all (measured on an ordinary MKV rip: the container held only `encoder`
    /// and `creation_time`), so the thing under test is that the read is fast
    /// and does not leak the context — not that any given file has a name.
    #[test]
    fn container_title_smoke() {
        let Ok(path) = std::env::var("FP_TEST_VIDEO") else {
            return;
        };
        let _ = ffmpeg::init();
        for _ in 0..3 {
            let t = std::time::Instant::now();
            let title = container_title(&path);
            println!("container_title {:?} -> {title:?}", t.elapsed());
        }
    }

    /// Smoke test: FP_TEST_VIDEO=<path> cargo test thumb_smoke -- --nocapture
    ///
    /// `FP_TEST_POS=2100,2400` picks the positions and `FP_TEST_DUMP=<dir>`
    /// writes the JPEGs out — which is the only way to check a *colour* change,
    /// since a thumbnail that is the right size and the wrong hue passes every
    /// assertion that can be written about it.
    #[test]
    fn thumb_smoke() {
        let Ok(path) = std::env::var("FP_TEST_VIDEO") else {
            return;
        };
        let _ = ffmpeg::init();
        let mut s = ThumbSession::open(&path).expect("open session");
        println!("hdr = {:?}", s.hdr);
        let positions: Vec<f64> = match std::env::var("FP_TEST_POS") {
            Ok(v) => v.split(',').filter_map(|p| p.trim().parse().ok()).collect(),
            Err(_) => vec![0.0, 5.0, 30.0, 59.0],
        };
        let dump = std::env::var("FP_TEST_DUMP").ok();
        for pos in positions {
            let (pts, jpeg) = s.frame_at(pos, 10.0).expect("frame_at");
            println!("pos={pos} -> pts={pts:.3} jpeg={} bytes", jpeg.len());
            assert!(jpeg.len() > 500);
            if let Some(dir) = &dump {
                let file = std::path::Path::new(dir).join(format!("thumb-{pos}.jpg"));
                std::fs::write(&file, &jpeg).expect("write dump");
                println!("  wrote {}", file.display());
            }
        }
    }

    /// The poster scorer, against a real file: does it rank a lit frame above
    /// a black one, and is the threshold in the right place?
    /// `FP_TEST_VIDEO=<path> cargo test poster_score_smoke -- --nocapture`
    #[test]
    fn poster_score_smoke() {
        let Ok(path) = std::env::var("FP_TEST_VIDEO") else {
            return;
        };
        let _ = ffmpeg::init();
        let mut s = ThumbSession::open(&path).expect("open session");
        for pos in [0.0, 5.0, 30.0, 59.0] {
            if s.seek_to_keyframe(pos).is_err() || !s.decode_next().unwrap_or(false) {
                println!("pos={pos} -> no frame");
                continue;
            }
            let rgb = s.scaled_rgb().expect("scale");
            let luma: Vec<f64> = rgb
                .chunks_exact(3)
                .map(|p| 0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64)
                .collect();
            let mean = luma.iter().sum::<f64>() / luma.len() as f64;
            let spread = luma.iter().map(|v| (v - mean).abs()).sum::<f64>() / luma.len() as f64;
            let score = rgb_score(&rgb);
            let jpeg = s.encode_current().expect("encode");
            let out = std::env::temp_dir().join(format!("poster-{pos}.jpg"));
            std::fs::write(&out, &jpeg).ok();
            println!(
                "pos={pos} mean={mean:.1} spread={spread:.1} score={score:.1} -> {}",
                out.display()
            );
        }
    }

    /// A black frame must score below the threshold, or the whole point of
    /// scoring is lost — this is the case the feature exists to avoid.
    #[test]
    fn black_frames_are_refused() {
        // 320x180 of pure black, and of mid-gray noise, through the same maths
        // the scorer runs on the scaled buffer.
        let black = vec![0u8; 320 * 180 * 3];
        let mut noisy = Vec::with_capacity(320 * 180 * 3);
        for i in 0..320 * 180 {
            let v = if i % 2 == 0 { 90 } else { 170 };
            noisy.extend_from_slice(&[v, v, v]);
        }
        assert!(!poster_usable(&black));
        assert!(poster_usable(&noisy));
        // A dark but real scene — the case an absolute threshold got wrong.
        let dark: Vec<u8> = (0..320 * 180)
            .flat_map(|i| {
                let v = if i % 3 == 0 { 8u8 } else { 26 };
                [v, v, v]
            })
            .collect();
        assert!(poster_usable(&dark));
        assert!(rgb_score(&noisy) > rgb_score(&dark));
    }

    /// Rounding, and the clamp that keeps it from running off the end.
    #[test]
    fn bucket_rounding() {
        // Nearest cell, not the containing one: 7 s on a 10 s grid belongs to
        // cell 1 (10 s), which is 3 s away, not cell 0, which is 7 s away.
        assert_eq!(bucket_for(7.0, 10.0, 0.0), 1);
        assert_eq!(bucket_for(4.9, 10.0, 0.0), 0);
        assert_eq!(bucket_for(0.0, 10.0, 0.0), 0);
        // Worst case is now half a step instead of a whole one.
        for pos in [0.3, 12.7, 99.9, 355.5] {
            let cell = bucket_for(pos, 10.0, 0.0) as f64 * 10.0;
            assert!(
                (cell - pos).abs() <= 5.0 + 1e-9,
                "pos {pos} -> cell {cell}: further than half a step"
            );
        }
        // The far right of the seekbar must not address a cell the storyboard
        // never generates, at a position past the end of the file.
        let last = bucket_count(100.0, 10.0) as u32 - 1;
        assert_eq!(bucket_for(100.0, 10.0, 100.0), last);
        assert!((last as f64 * 10.0) <= 100.0);
        assert_eq!(
            bucket_for(105.0, 10.0, 105.0),
            bucket_count(105.0, 10.0) as u32 - 1
        );
    }

    /// The exact preview has to land on the position asked for, not on the
    /// keyframe before it — that difference is the whole point of it.
    /// FP_TEST_VIDEO=<path> cargo test thumb_exact_lands -- --nocapture
    #[test]
    fn thumb_exact_lands() {
        let Ok(path) = std::env::var("FP_TEST_VIDEO") else {
            return;
        };
        let _ = ffmpeg::init();
        let mut s = ThumbSession::open(&path).expect("open session");
        for pos in [3.7, 17.3, 41.9, 52.4] {
            let (pts, jpeg) = s.frame_exact_at(pos).expect("frame_exact_at");
            println!("pos={pos} -> pts={pts:.3} jpeg={} bytes", jpeg.len());
            assert!(jpeg.len() > 500);
            // One frame of slack: the decoder stops at the first frame at or
            // past the target, exactly as mpv's hr-seek does.
            assert!(
                pts >= pos - 0.1 && pts - pos < 0.5,
                "pos {pos}: landed on {pts:.3}, not the frame asked for"
            );
        }
        // The storyboard's verdict for the file must survive a hover request.
        assert!(!s.refine_mode, "frame_exact_at must not set refine_mode");
    }

    /// The grid's key property: neighbouring cells are different frames, even
    /// when the GOP is longer than the step (otherwise the preview appears
    /// stuck for seconds of seekbar).
    /// FP_TEST_VIDEO=<path> cargo test thumb_grid_distinct -- --nocapture
    #[test]
    fn thumb_grid_distinct() {
        let Ok(path) = std::env::var("FP_TEST_VIDEO") else {
            return;
        };
        let _ = ffmpeg::init();
        let mut s = ThumbSession::open(&path).expect("open session");
        let interval = interval_for(60.0);
        let mut prev: Option<(f64, Vec<u8>)> = None;
        for b in 0..40u32 {
            let target = b as f64 * interval;
            let (pts, jpeg) = s.frame_at(target, interval).expect("frame_at");
            assert!(
                (pts - target).abs() <= interval,
                "bucket {b}: target={target:.3} -> pts={pts:.3}, missed by more than one step"
            );
            if let Some((ppts, pjpeg)) = &prev {
                assert!(
                    pts > *ppts,
                    "bucket {b}: pts is not increasing ({ppts:.3} -> {pts:.3})"
                );
                assert!(
                    *pjpeg != jpeg,
                    "bucket {b}: same image as the previous cell"
                );
            }
            prev = Some((pts, jpeg));
        }
        println!("step {interval:.3} s, 40 cells — every frame distinct");
    }
}

/// Thumbnail for a position: [f64 key LE][JPEG].
///
/// `exact = false` (the cursor is moving) answers from the grid, which is what
/// the storyboard fills and the disk cache holds; the key is the cell index —
/// deterministic, unlike PTS, which some containers do not carry at all and
/// which would collapse different positions into one key.
///
/// `exact = true` (the cursor has come to rest) decodes the frame at that exact
/// position instead, and the key is its PTS. Such a frame is deliberately NOT
/// stored in `thumbs`: the map is the grid, one cell per index, and it is what
/// gets written to disk. The two key spaces overlap numerically, so the caller
/// keeps them in separate maps.
#[tauri::command]
pub async fn thumb_get(
    state: tauri::State<'_, ThumbState>,
    path: String,
    pos: f64,
    exact: bool,
) -> Result<tauri::ipc::Response, String> {
    let arc = state.inner.clone();
    let pending = state.pending.clone();
    pending.fetch_add(1, Ordering::SeqCst);
    let res = tauri::async_runtime::spawn_blocking(move || {
        let mut inner = lock(&arc);
        ensure_path(&mut inner, &path);
        let (key, jpeg) = if exact {
            if inner.session.is_none() {
                inner.session = Some(ThumbSession::open(&inner.path)?);
            }
            inner.session.as_mut().unwrap().frame_exact_at(pos)?
        } else {
            let (bucket, jpeg) = thumb_at(&mut inner, pos)?;
            (bucket as f64, jpeg)
        };
        let mut out = Vec::with_capacity(8 + jpeg.len());
        out.extend_from_slice(&key.to_le_bytes());
        out.extend_from_slice(&jpeg);
        Ok(tauri::ipc::Response::new(out))
    })
    .await
    .map_err(|e| e.to_string());
    pending.fetch_sub(1, Ordering::SeqCst);
    res?
}

/// Container titles for a list of local files, in the order given.
///
/// Reads the header and nothing else: `avformat_open_input` *without*
/// `avformat_find_stream_info`, because a title lives in the container header
/// while the probe exists to identify streams — which this does not need.
/// Measured on an MKV (warm): 0.10 ms per file this way against 0.27 ms with
/// the probe, and the first read of a cold file is dominated by the disk in
/// either case. That is what makes it affordable to name a whole folder at
/// once; it is also why the caller still does it off the opening path.
///
/// A file with no title tag answers `None` rather than a guess — the caller
/// already has a better fallback (the file name, tidied) than anything that
/// could be invented here.
#[tauri::command]
pub async fn container_titles(paths: Vec<String>) -> Vec<Option<String>> {
    tauri::async_runtime::spawn_blocking(move || {
        let _ = ffmpeg::init();
        paths.iter().map(|p| container_title(p)).collect()
    })
    .await
    .unwrap_or_default()
}

fn container_title(path: &str) -> Option<String> {
    let c_path = std::ffi::CString::new(path).ok()?;
    let key = std::ffi::CString::new("title").ok()?;
    unsafe {
        let mut ctx: *mut ffmpeg::sys::AVFormatContext = std::ptr::null_mut();
        if ffmpeg::sys::avformat_open_input(
            &mut ctx,
            c_path.as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) < 0
        {
            return None;
        }
        let entry =
            ffmpeg::sys::av_dict_get((*ctx).metadata, key.as_ptr(), std::ptr::null(), 0);
        let title = if entry.is_null() {
            None
        } else {
            let value = std::ffi::CStr::from_ptr((*entry).value)
                .to_string_lossy()
                .trim()
                .to_string();
            (!value.is_empty()).then_some(value)
        };
        // Always closed, on every path out of the block above.
        ffmpeg::sys::avformat_close_input(&mut ctx);
        title
    }
}

// ---- Saved posters ---------------------------------------------------------
//
// A poster is normally decoded on demand from the file at the watch position,
// which needs the file to still be there and readable — and for a torrent still
// downloading it is neither: the holes read back as zeros, so the "poster" would
// be a black rectangle presented as a frame of the film. Capturing one *while it
// plays* removes both problems at once, because the data is on disk exactly then
// and the picture outlives the file.
//
// Stored beside the thumbnail cache with the source path written into the file,
// for the same reason `CACHE_MAGIC` v4 carries it: without the path an entry
// cannot be attributed to a folder, and `purge_thumbs` could not honour a
// privacy root being excluded.

/// v2 for the same reason `CACHE_MAGIC` went to v5: a poster captured before
/// the colour conversion existed is a wrongly-coloured picture of the right
/// film, and nothing else would ever replace it — this one is captured once,
/// while the file plays, and then kept.
const POSTER_MAGIC: &[u8; 4] = b"FPP3";

/// Mean luma and its mean absolute deviation, on a packed RGB buffer.
fn rgb_stats(rgb: &[u8]) -> (f64, f64) {
    if rgb.is_empty() {
        return (0.0, 0.0);
    }
    let luma: Vec<f64> = rgb
        .chunks_exact(3)
        .map(|p| 0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64)
        .collect();
    let mean = luma.iter().sum::<f64>() / luma.len() as f64;
    let spread = luma.iter().map(|v| (v - mean).abs()).sum::<f64>() / luma.len() as f64;
    (mean, spread)
}

/// How much of a *picture* a frame is, 0 upwards, on a packed RGB buffer.
///
/// The on-demand poster is whatever frame sits at the requested second, black
/// frames and fades included — position is the only criterion there. This is
/// the content one: mean absolute deviation of luma, which is high for a lit
/// scene with detail and near zero for a black frame, a white flash or a flat
/// title card. Deliberately crude, and computed on the already-scaled 320px RGB
/// rather than the source frame: it only has to rank a handful of candidates
/// against each other, and ffmpeg's own `thumbnail` filter picks by much the
/// same idea (distance from the average histogram).
fn rgb_score(rgb: &[u8]) -> f64 {
    let (mean, spread) = rgb_stats(rgb);
    // A frame can be busy and still be a bad poster: an almost-black scene with
    // noise, or a blown-out flash. Weight the spread by how far the mean sits
    // from either extreme, so the middle of the range wins ties.
    let headroom = ((mean / 255.0) * (1.0 - mean / 255.0) * 4.0).clamp(0.0, 1.0);
    spread * headroom
}

/// **A score is for ranking candidates, never for refusing them.** Measured on
/// a real 4K HDR release, whose frames decode dark because nothing tone-maps
/// them on the way to RGB: a perfectly legible scene scored 2.7 and a bright
/// one 8.6, so any absolute threshold tuned on ordinary SDR content refuses
/// most of an HDR film and the card keeps its link mark for ever. What is worth
/// refusing is only the *degenerate* frame — the black one at the head of a
/// release, a white flash, a flat card — and that is a fact about the pixels
/// rather than a judgement about the scene.
fn poster_usable(rgb: &[u8]) -> bool {
    let (mean, spread) = rgb_stats(rgb);
    mean >= 2.0 && spread >= 2.0
}

fn posters_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager as _;
    let dir = app.path().app_cache_dir().ok()?.join("posters");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn poster_file(app: &tauri::AppHandle, id: &str) -> Option<PathBuf> {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut h);
    Some(posters_dir(app)?.join(format!("{:016x}.fpp", h.finish())))
}

/// Decode a few candidate positions, keep the one that looks most like a
/// picture, and save it under `id`.
///
/// The candidates come from the caller because only it knows which parts of the
/// file are readable — for a torrent that is the buffered map, and asking for a
/// second that has not arrived would decode zeros. Bounded work by construction:
/// a handful of seeks, at the storyboard's background QoS, once per file.
#[tauri::command]
pub async fn poster_capture(
    app: tauri::AppHandle,
    id: String,
    path: String,
    positions: Vec<f64>,
) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // The privacy gate is the same one the thumbnail cache uses: a file
        // under an excluded root leaves nothing on disk.
        if is_private(&path) || positions.is_empty() {
            eprintln!("[poster] {id}: skipped (private or no candidates)");
            return Ok(false);
        }
        let Some(file) = poster_file(&app, &id) else {
            return Ok(false);
        };
        background_qos();
        let mut session = match ThumbSession::open(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[poster] {id}: cannot open {path}: {e}");
                return Ok(false);
            }
        };
        let mut best: Option<(f64, Vec<u8>)> = None;
        for pos in positions.into_iter().take(6) {
            if session.seek_to_keyframe(pos.max(0.0)).is_err()
                || !session.decode_next().unwrap_or(false)
            {
                eprintln!("[poster] {id}: no frame at {pos:.1}s");
                continue;
            }
            let Ok(rgb) = session.scaled_rgb() else { continue };
            let score = rgb_score(&rgb);
            let usable = poster_usable(&rgb);
            eprintln!(
                "[poster] {id}: {pos:.1}s score={score:.1} {}",
                if usable { "usable" } else { "degenerate" }
            );
            if !usable || best.as_ref().is_some_and(|(b, _)| *b >= score) {
                continue;
            }
            if let Ok(jpeg) = session.encode_current() {
                best = Some((score, jpeg));
            }
        }
        let Some((_, jpeg)) = best else {
            eprintln!("[poster] {id}: nothing usable among the candidates");
            return Ok(false);
        };
        let mut out = Vec::with_capacity(4 + 2 + path.len() + jpeg.len());
        out.extend_from_slice(POSTER_MAGIC);
        out.extend_from_slice(&(path.len() as u16).to_le_bytes());
        out.extend_from_slice(path.as_bytes());
        out.extend_from_slice(&jpeg);
        std::fs::write(&file, out).map_err(|e| e.to_string())?;
        eprintln!("[poster] {id}: saved {}", file.display());
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The saved poster for `id`, if there is one. Answers with the same 8-byte
/// header the decoded posters use, so the caller cannot tell them apart.
#[tauri::command]
pub fn poster_saved(app: tauri::AppHandle, id: String) -> Result<tauri::ipc::Response, String> {
    let file = poster_file(&app, &id).ok_or("no cache directory")?;
    let bytes = std::fs::read(file).map_err(|_| "no saved poster".to_string())?;
    let (jpeg, _) = poster_parse(&bytes).ok_or("unreadable poster")?;
    Ok(respond(0.0, jpeg))
}

/// `(jpeg, source path)` out of a stored poster.
fn poster_parse(bytes: &[u8]) -> Option<(&[u8], String)> {
    if bytes.len() < 6 || &bytes[..4] != POSTER_MAGIC {
        return None;
    }
    let n = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
    let path = String::from_utf8(bytes.get(6..6 + n)?.to_vec()).ok()?;
    Some((bytes.get(6 + n..)?, path))
}

/// Drop saved posters whose source is inside `root` — the poster half of
/// `purge_thumbs`, and the reason the source path is stored in the file.
pub fn purge_posters(app: &tauri::AppHandle, root: &str) {
    let Some(dir) = posters_dir(app) else { return };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let keep = std::fs::read(&p)
            .ok()
            .and_then(|b| poster_parse(&b).map(|(_, src)| !path_under(&src, root)))
            // Unreadable or from an older format: cannot be attributed, so it
            // goes — the same call the thumbnail cache makes.
            .unwrap_or(false);
        if !keep {
            let _ = std::fs::remove_file(&p);
        }
    }
}
