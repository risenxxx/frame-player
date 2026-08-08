//! StepEngine: an FFmpeg sidecar for instant frame-by-frame navigation.
//!
//! Keeps a ring of decoded RGBA frames around the current position. Steps
//! inside the ring are free; crossing a GOP boundary decodes the neighbouring
//! GOP. Meanwhile mpv sits paused behind a canvas overlay, and the frontend
//! walks it to the displayed frame with a background pre-seek.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use ffmpeg::media;
use ffmpeg::software::scaling;
use ffmpeg::util::frame::video::Video;
use ffmpeg_the_third as ffmpeg;

/// Memory budget for the frame ring.
const MEM_CAP_BYTES: usize = 700 * 1024 * 1024;
/// On entry, decode slightly past the position so a forward step is ready.
const ENTER_AHEAD_SECS: f64 = 0.2;
const PTS_EPS: f64 = 0.0005;

struct CachedFrame {
    pts: f64,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

pub struct StepSession {
    path: String,
    ictx: ffmpeg::format::context::Input,
    decoder: ffmpeg::decoder::Video,
    scaler: Option<(ffmpeg::format::Pixel, u32, u32, scaling::Context)>,
    stream_index: usize,
    time_base: f64,
    start_offset: f64,
    frames: VecDeque<CachedFrame>,
    cursor: usize,
    bytes: usize,
    /// Demuxer and decoder sit right after the ring's last frame.
    continuous_tail: bool,
}

// Access to the session is serialized through a Mutex; FFmpeg contexts are not
// thread-bound (no TLS), so moving them between threads is safe.
unsafe impl Send for StepSession {}

impl StepSession {
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
        let dec_ctx = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
            .map_err(|e| format!("codec ctx: {e}"))?;
        let decoder = dec_ctx
            .decoder()
            .video()
            .map_err(|e| format!("decoder: {e}"))?;
        Ok(StepSession {
            path: path.to_string(),
            ictx,
            decoder,
            scaler: None,
            stream_index,
            time_base,
            start_offset,
            frames: VecDeque::new(),
            cursor: 0,
            bytes: 0,
            continuous_tail: false,
        })
    }

    fn seek_keyframe_before(&mut self, target_secs: f64) -> Result<(), String> {
        let ts =
            ((target_secs + self.start_offset).max(0.0) * ffmpeg::ffi::AV_TIME_BASE as f64) as i64;
        // ..=ts (inclusive!): with an exclusive bound max_ts = ts-1 < ts, and
        // avformat_seek_file rejects the call with EPERM.
        self.ictx
            .seek(ts, ..=ts)
            .map_err(|e| format!("seek: {e}"))?;
        self.decoder.flush();
        self.continuous_tail = false;
        Ok(())
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

    /// Decodes the next video frame (or None at EOF).
    fn decode_next(&mut self) -> Result<Option<CachedFrame>, String> {
        let mut decoded = Video::empty();
        loop {
            if self.decoder.receive_frame(&mut decoded).is_ok() {
                return Ok(Some(self.convert(&decoded)?));
            }
            match self.next_video_packet() {
                Some(packet) => {
                    self.decoder
                        .send_packet(&packet)
                        .map_err(|e| format!("send_packet: {e}"))?;
                }
                None => {
                    let _ = self.decoder.send_eof();
                    if self.decoder.receive_frame(&mut decoded).is_ok() {
                        return Ok(Some(self.convert(&decoded)?));
                    }
                    return Ok(None);
                }
            }
        }
    }

    fn convert(&mut self, frame: &Video) -> Result<CachedFrame, String> {
        let (w, h) = (frame.width(), frame.height());
        let fmt = frame.format();
        let rebuild = match &self.scaler {
            Some((sf, sw, sh, _)) => *sf != fmt || *sw != w || *sh != h,
            None => true,
        };
        if rebuild {
            let ctx = scaling::Context::get(
                fmt,
                w,
                h,
                ffmpeg::format::Pixel::RGBA,
                w,
                h,
                scaling::Flags::BILINEAR,
            )
            .map_err(|e| format!("scaler: {e}"))?;
            self.scaler = Some((fmt, w, h, ctx));
        }
        let scaler = &mut self.scaler.as_mut().unwrap().3;
        let mut rgb = Video::empty();
        scaler
            .run(frame, &mut rgb)
            .map_err(|e| format!("scale: {e}"))?;

        let stride = rgb.stride(0);
        let row = w as usize * 4;
        let data = rgb.data(0);
        let mut rgba = Vec::with_capacity(row * h as usize);
        for y in 0..h as usize {
            rgba.extend_from_slice(&data[y * stride..y * stride + row]);
        }
        let ts = frame.timestamp().or(frame.pts()).unwrap_or(0);
        Ok(CachedFrame {
            pts: ts as f64 * self.time_base - self.start_offset,
            width: w,
            height: h,
            rgba,
        })
    }

    fn push_back_frame(&mut self, f: CachedFrame) {
        self.bytes += f.rgba.len();
        self.frames.push_back(f);
    }

    fn index_at_or_before(&self, pos: f64) -> usize {
        let mut idx = 0;
        for (i, f) in self.frames.iter().enumerate() {
            if f.pts <= pos + PTS_EPS {
                idx = i;
            } else {
                break;
            }
        }
        idx
    }

    /// Fills the ring around a position (called from prewarm and step_enter).
    fn fill_for_enter(&mut self, pos: f64) -> Result<(), String> {
        if let (Some(front), Some(back)) = (self.frames.front(), self.frames.back()) {
            if pos >= front.pts - PTS_EPS && pos <= back.pts + PTS_EPS {
                self.cursor = self.index_at_or_before(pos);
                return Ok(());
            }
        }
        self.frames.clear();
        self.bytes = 0;
        self.cursor = 0;
        self.seek_keyframe_before(pos)?;
        loop {
            match self.decode_next()? {
                Some(f) => {
                    let fpts = f.pts;
                    self.push_back_frame(f);
                    if fpts >= pos + ENTER_AHEAD_SECS {
                        break;
                    }
                }
                None => break,
            }
        }
        self.continuous_tail = true;
        if self.frames.is_empty() {
            return Err("no frames decoded".into());
        }
        self.cursor = self.index_at_or_before(pos);
        self.trim();
        Ok(())
    }

    fn extend_forward(&mut self) -> Result<(), String> {
        if !self.continuous_tail {
            let back = self.frames.back().map(|f| f.pts).unwrap_or(0.0);
            self.seek_keyframe_before(back + PTS_EPS)?;
            loop {
                match self.decode_next()? {
                    Some(f) if f.pts <= back + PTS_EPS => continue,
                    Some(f) => {
                        self.push_back_frame(f);
                        self.continuous_tail = true;
                        return Ok(());
                    }
                    None => {
                        self.continuous_tail = true;
                        return Ok(());
                    }
                }
            }
        }
        if let Some(f) = self.decode_next()? {
            self.push_back_frame(f);
        }
        Ok(())
    }

    fn extend_backward(&mut self) -> Result<(), String> {
        let front = match self.frames.front() {
            Some(f) => f.pts,
            None => return Ok(()),
        };
        if front <= PTS_EPS {
            return Ok(());
        }
        // First target is slightly before the current start; if the keyframe
        // seek lands on the same spot, back off further and further.
        for target in [front - 0.01, front - 1.0, front - 5.0, 0.0] {
            let target = target.max(0.0);
            self.seek_keyframe_before(target)?;
            let mut collected: Vec<CachedFrame> = Vec::new();
            loop {
                match self.decode_next()? {
                    Some(f) => {
                        if f.pts >= front - PTS_EPS {
                            break;
                        }
                        collected.push(f);
                    }
                    None => break,
                }
            }
            self.continuous_tail = false;
            if !collected.is_empty() {
                for f in collected.into_iter().rev() {
                    self.bytes += f.rgba.len();
                    self.frames.push_front(f);
                    self.cursor += 1;
                }
                return Ok(());
            }
            if target == 0.0 {
                break;
            }
        }
        Ok(())
    }

    fn step(&mut self, delta: i32) -> Result<(), String> {
        if delta > 0 {
            if self.cursor + 1 >= self.frames.len() {
                self.extend_forward()?;
            }
            if self.cursor + 1 < self.frames.len() {
                self.cursor += 1;
            }
        } else if delta < 0 {
            if self.cursor == 0 {
                self.extend_backward()?;
            }
            if self.cursor > 0 {
                self.cursor -= 1;
            }
        }
        self.trim();
        Ok(())
    }

    /// Trims the ring to the memory budget, dropping frames furthest from the cursor.
    fn trim(&mut self) {
        while self.bytes > MEM_CAP_BYTES && self.frames.len() > 1 {
            if self.cursor > self.frames.len() / 2 {
                if let Some(f) = self.frames.pop_front() {
                    self.bytes -= f.rgba.len();
                    if self.cursor > 0 {
                        self.cursor -= 1;
                    }
                }
            } else {
                if let Some(f) = self.frames.pop_back() {
                    self.bytes -= f.rgba.len();
                    self.continuous_tail = false;
                }
                if self.cursor >= self.frames.len() {
                    self.cursor = self.frames.len() - 1;
                }
            }
        }
    }

    fn current_response(&self) -> Result<tauri::ipc::Response, String> {
        let f = self.frames.get(self.cursor).ok_or("empty ring")?;
        let mut out = Vec::with_capacity(16 + f.rgba.len());
        out.extend_from_slice(&f.width.to_le_bytes());
        out.extend_from_slice(&f.height.to_le_bytes());
        out.extend_from_slice(&f.pts.to_le_bytes());
        out.extend_from_slice(&f.rgba);
        Ok(tauri::ipc::Response::new(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test: FP_TEST_VIDEO=<path> cargo test step_smoke -- --nocapture
    #[test]
    fn step_smoke() {
        let Ok(path) = std::env::var("FP_TEST_VIDEO") else {
            return;
        };
        let _ = ffmpeg::init();
        let mut s = StepSession::open(&path).expect("open session");
        s.fill_for_enter(30.0).expect("fill_for_enter");
        let start_pts = s.frames[s.cursor].pts;
        for _ in 0..40 {
            s.step(-1).expect("step back");
        }
        let back_pts = s.frames[s.cursor].pts;
        assert!(back_pts < start_pts, "backward stepping must move back");
        for _ in 0..5 {
            s.step(1).expect("step fwd");
        }
        let fwd_pts = s.frames[s.cursor].pts;
        assert!(fwd_pts > back_pts, "forward stepping must move forward");
        println!(
            "start={start_pts:.3} back={back_pts:.3} fwd={fwd_pts:.3} frames={} mem={}MB",
            s.frames.len(),
            s.bytes / 1024 / 1024
        );
    }
}

#[derive(Default)]
pub struct StepState(pub Arc<Mutex<Option<StepSession>>>);

fn lock(arc: &Arc<Mutex<Option<StepSession>>>) -> MutexGuard<'_, Option<StepSession>> {
    arc.lock().unwrap_or_else(|p| p.into_inner())
}

fn ensure_session(
    guard: &mut MutexGuard<'_, Option<StepSession>>,
    path: &str,
) -> Result<(), String> {
    let rebuild = match guard.as_ref() {
        Some(s) => s.path != path,
        None => true,
    };
    if rebuild {
        **guard = Some(StepSession::open(path)?);
    }
    Ok(())
}

#[tauri::command]
pub async fn step_prewarm(
    state: tauri::State<'_, StepState>,
    path: String,
    pos: f64,
) -> Result<(), String> {
    let arc = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = lock(&arc);
        ensure_session(&mut guard, &path)?;
        guard.as_mut().unwrap().fill_for_enter(pos)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn step_enter(
    state: tauri::State<'_, StepState>,
    path: String,
    pos: f64,
) -> Result<tauri::ipc::Response, String> {
    let arc = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = lock(&arc);
        ensure_session(&mut guard, &path)?;
        let session = guard.as_mut().unwrap();
        session.fill_for_enter(pos)?;
        session.current_response()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn step_move(
    state: tauri::State<'_, StepState>,
    delta: i32,
) -> Result<tauri::ipc::Response, String> {
    let arc = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = lock(&arc);
        let session = guard.as_mut().ok_or("no step session")?;
        session.step(delta)?;
        session.current_response()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn step_exit(state: tauri::State<'_, StepState>) -> Result<(), String> {
    let mut guard = lock(&state.0);
    if let Some(s) = guard.as_mut() {
        s.frames.clear();
        s.bytes = 0;
        s.cursor = 0;
        s.continuous_tail = false;
    }
    Ok(())
}
