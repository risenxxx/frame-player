//! What the frame stepper needs to know before it lets mpv decode backward.
//!
//! Stepping back by one frame through mpv's own `frame-back-step` is an exact
//! seek: decode from the keyframe before the target up to it. On a file whose
//! keyframes are seconds apart that costs a second or more *per press*. mpv's
//! backward playback (`play-direction=backward`) does the same decode once and
//! keeps every frame of the group, so the presses after it are one frame
//! period each — at the price of holding a whole group of pictures, decoded,
//! in memory. That price is the question this module answers: how long the
//! groups around a position are, and how much memory this machine has.
//!
//! Only packets are read — the container's index and the key flags — never a
//! frame decoded, so a probe is a few milliseconds on a local file. See the
//! frame-stepping line in docs/rules/mpv-playback.md.

#![deny(clippy::undocumented_unsafe_blocks)]

use ffmpeg::media;
use ffmpeg_the_third as ffmpeg;

/// Stop reading after this much of the file past the position: a group longer
/// than this is not one backward playback should hold anyway.
const MAX_SCAN_SECS: f64 = 120.0;
/// …and after this many video packets, for a stream with broken timestamps.
const MAX_SCAN_PACKETS: usize = 20_000;
const PTS_EPS: f64 = 0.0005;

#[derive(serde::Serialize, Debug)]
pub struct GopProbe {
    /// The keyframe at or before the position, in seconds of the file.
    start: f64,
    /// The keyframe after it, if the file has one.
    next: Option<f64>,
    /// The keyframe before `start`: stepping back crosses into that group, and
    /// mpv starts decoding it as soon as the current one is handed over.
    prev: Option<f64>,
    /// The longest of the groups measured, in seconds.
    longest: f64,
    /// Physical memory, in bytes; 0 when the system would not say.
    ram: u64,
}

struct Probe {
    ictx: ffmpeg::format::context::Input,
    stream_index: usize,
    time_base: f64,
    start_offset: f64,
}

impl Probe {
    fn open(path: &str) -> Result<Self, String> {
        let ictx = ffmpeg::format::input(path).map_err(|e| format!("open: {e}"))?;
        let stream = ictx
            .streams()
            .best(media::Type::Video)
            .ok_or("no video stream")?;
        let stream_index = stream.index();
        let tb = stream.time_base();
        let time_base = tb.numerator() as f64 / tb.denominator() as f64;
        let st = stream.start_time();
        let start_offset = if st == i64::MIN { 0.0 } else { st as f64 * time_base };
        Ok(Probe { ictx, stream_index, time_base, start_offset })
    }

    /// The keyframe at or before `t` and the one after it.
    fn group_at(&mut self, t: f64) -> Result<(f64, Option<f64>), String> {
        let ts = ((t + self.start_offset).max(0.0) * ffmpeg::ffi::AV_TIME_BASE as f64) as i64;
        // ..=ts (inclusive!): see gotcha 2 in CLAUDE.md.
        self.ictx.seek(ts, ..=ts).map_err(|e| format!("seek: {e}"))?;
        let mut start: Option<f64> = None;
        let mut read = 0usize;
        for res in self.ictx.packets() {
            let Ok((stream, packet)) = res else { break };
            if stream.index() != self.stream_index {
                continue;
            }
            read += 1;
            let Some(raw) = packet.pts().or(packet.dts()) else { continue };
            let at = raw as f64 * self.time_base - self.start_offset;
            if packet.is_key() {
                if at <= t + PTS_EPS {
                    start = Some(at);
                } else if start.is_some() {
                    return Ok((start.unwrap_or(0.0), Some(at)));
                } else {
                    // The demuxer landed past the position: nothing here says
                    // where the group containing it begins.
                    return Err("seek landed after the position".into());
                }
            }
            if read >= MAX_SCAN_PACKETS || at > t + MAX_SCAN_SECS {
                break;
            }
        }
        start.map(|s| (s, None)).ok_or_else(|| "no keyframe found".into())
    }
}

fn probe(path: &str, pos: f64) -> Result<GopProbe, String> {
    let mut p = Probe::open(path)?;
    let (start, next) = p.group_at(pos)?;
    // Backing off by a millisecond is enough when the demuxer seeks exactly; a
    // container whose index is coarse can land on `start` again, which only
    // means the previous group stays unknown — the frontend's landing check is
    // what catches a group longer than the budget it was given.
    let prev = if start > 0.01 {
        p.group_at(start - 0.001).ok().map(|(s, _)| s).filter(|s| *s < start - PTS_EPS)
    } else {
        None
    };
    let mut longest = 0.0f64;
    if let Some(n) = next {
        longest = longest.max(n - start);
    }
    if let Some(pv) = prev {
        longest = longest.max(start - pv);
    }
    if longest <= 0.0 {
        // Only one keyframe found (the last group of a file, or a file with a
        // single one): the group runs at least to the position.
        longest = (pos - start).max(0.0);
    }
    Ok(GopProbe { start, next, prev, longest, ram: physical_memory() })
}

#[tauri::command]
pub async fn frame_step_probe(path: String, pos: f64) -> Result<GopProbe, String> {
    tauri::async_runtime::spawn_blocking(move || probe(&path, pos))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(target_os = "macos")]
fn physical_memory() -> u64 {
    let mut value: u64 = 0;
    let mut size = std::mem::size_of::<u64>();
    let name = c"hw.memsize";
    // SAFETY: `value` is a u64 and `size` says so; sysctl writes at most that.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            &mut value as *mut u64 as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc == 0 { value } else { 0 }
}

#[cfg(windows)]
fn physical_memory() -> u64 {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: a struct of integers, for which all zeroes is a valid value.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: a local struct for the call to fill, with `dwLength` set to its
    // size first, as the call requires.
    if unsafe { GlobalMemoryStatusEx(&mut status) } != 0 {
        status.ullTotalPhys
    } else {
        0
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn physical_memory() -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FP_TEST_VIDEO=<path> cargo test --lib frame_step_probe_smoke -- --nocapture
    #[test]
    fn frame_step_probe_smoke() {
        let Ok(path) = std::env::var("FP_TEST_VIDEO") else {
            return;
        };
        let _ = ffmpeg::init();
        for pos in [0.5, 12.3, 30.0, 34.9] {
            let g = probe(&path, pos).expect("probe");
            println!("pos {pos}: {g:?}");
            assert!(g.start <= pos + PTS_EPS, "the group must begin at or before the position");
            if let Some(n) = g.next {
                assert!(n > pos, "the next keyframe must lie after the position");
            }
            assert!(g.longest > 0.0);
        }
        assert!(physical_memory() > 0, "this platform reports its memory");
    }
}
