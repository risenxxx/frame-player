//! Finding the black bars baked into a picture.
//!
//! A letterboxed film (2.39:1 content in a 16:9 frame) or a pillarboxed one
//! (4:3 in 16:9) carries its bars as pixels, so nothing in the container says
//! they are there. This measures them on decoded frames and answers a rectangle
//! mpv can take as `video-crop`.
//!
//! **It looks at several frames from across the file, not the one on screen.**
//! One frame answers for one scene: a night shot has dark edges that are
//! picture, and a scene may leave the top of the frame black. The caller
//! samples the length of the file, and `combine` keeps the *smallest* bar each
//! edge showed, give or take one outlier. The picture reaching an edge in two
//! frames is enough to say the edge is picture.
//!
//! Pure arithmetic on the luma plane and unit-tested, because a wrong answer
//! here is a plausible picture with a strip missing, which nobody reports until
//! a subtitle is cut in half.

/// A sample this far above black, on the 8-bit scale, is picture. Letterbox
/// bars are rarely exactly black: compression leaves them a few levels up, and
/// 16 sits well above that noise and well below the darkest real scene detail
/// that should stop the scan.
const BRIGHT_ABOVE_BLACK: u32 = 16;

/// A row or column is still bar while no more than this fraction of its
/// samples is bright (1 in 50). Not zero: a single hot pixel or a speck of
/// grain in the bar must not end it.
const BRIGHT_FRACTION_DEN: usize = 50;

/// Bars thinner than this share of the dimension are ignored. A few rows at
/// the edge are encoder padding or a soft edge, and cropping them would change
/// the window's shape for nothing anyone can see.
const MIN_BAR_FRACTION: f64 = 0.01;

/// Fewer valid frames than this and there is no answer. A short clip that is
/// dark from start to end has nothing to measure.
const MIN_FRAMES: usize = 3;

/// A crop that keeps less than this share of either dimension is treated as a
/// misreading rather than a picture.
const MIN_KEEP_FRACTION: f64 = 0.3;

/// The luma plane of one decoded frame, as FFmpeg laid it out.
pub struct Luma<'a> {
    pub data: &'a [u8],
    /// Bytes per row.
    pub stride: usize,
    pub width: usize,
    pub height: usize,
    /// 1 for 8-bit formats, 2 for anything deeper (little-endian).
    pub bytes: usize,
    /// Bits the sample is shifted up inside its word (6 for P010).
    pub shift: u32,
    /// Significant bits per sample.
    pub depth: u32,
    /// Full-range black is 0, limited-range (the usual case) is 16.
    pub full_range: bool,
}

impl Luma<'_> {
    /// The sample at (x, y) on the 8-bit scale.
    fn at(&self, x: usize, y: usize) -> u32 {
        let i = y * self.stride + x * self.bytes;
        let raw = if self.bytes == 1 {
            self.data.get(i).copied().unwrap_or(0) as u32
        } else {
            let lo = self.data.get(i).copied().unwrap_or(0) as u32;
            let hi = self.data.get(i + 1).copied().unwrap_or(0) as u32;
            lo | (hi << 8)
        };
        (raw >> self.shift) >> self.depth.saturating_sub(8)
    }

    fn black(&self) -> u32 {
        if self.full_range { 0 } else { 16 }
    }
}

/// The bars one frame shows, in pixels from each edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bars {
    pub top: usize,
    pub bottom: usize,
    pub left: usize,
    pub right: usize,
}

/// The rectangle to keep, in the frame's own pixels: what `video-crop` takes as
/// `WxH+X+Y`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

/// Measure one frame. `None` means the frame says nothing: it is black, or
/// nearly, from edge to middle.
pub fn frame_bars(l: &Luma) -> Option<Bars> {
    let (w, h) = (l.width, l.height);
    if w < 16 || h < 16 {
        return None;
    }
    let limit = l.black() + BRIGHT_ABOVE_BLACK;
    // Every pixel of a 4K row is not needed to tell a bar from a picture. A
    // step keeps a frame at a few thousand reads per line whatever its size.
    let xs = (w / 480).max(1);
    let ys = (h / 270).max(1);

    let row_dark = |y: usize, x0: usize, x1: usize| {
        let (mut n, mut bright) = (0usize, 0usize);
        let mut x = x0;
        while x < x1 {
            n += 1;
            if l.at(x, y) > limit {
                bright += 1;
            }
            x += xs;
        }
        bright * BRIGHT_FRACTION_DEN <= n
    };
    let col_dark = |x: usize, y0: usize, y1: usize| {
        let (mut n, mut bright) = (0usize, 0usize);
        let mut y = y0;
        while y < y1 {
            n += 1;
            if l.at(x, y) > limit {
                bright += 1;
            }
            y += ys;
        }
        bright * BRIGHT_FRACTION_DEN <= n
    };

    let mut top = 0;
    while top < h / 2 && row_dark(top, 0, w) {
        top += 1;
    }
    if top >= h / 2 {
        return None;
    }
    let mut bottom = 0;
    while bottom < h / 2 && row_dark(h - 1 - bottom, 0, w) {
        bottom += 1;
    }
    // The columns are judged only over the rows that are picture, or a
    // letterbox would make every column look half black.
    let (y0, y1) = (top, h - bottom);
    let mut left = 0;
    while left < w / 2 && col_dark(left, y0, y1) {
        left += 1;
    }
    if left >= w / 2 {
        return None;
    }
    let mut right = 0;
    while right < w / 2 && col_dark(w - 1 - right, y0, y1) {
        right += 1;
    }
    Some(Bars { top, bottom, left, right })
}

/// Turn what the frames showed into one rectangle, or `None` when there is
/// nothing worth cropping.
///
/// Each edge keeps its smallest bar, except that one frame may disagree when
/// there are enough of them: a burnt-in subtitle standing in the bottom bar,
/// or a logo in the corner, would otherwise veto the crop for the whole film.
/// Two frames reaching an edge is not an outlier, it is the picture.
pub fn combine(frames: &[Bars], width: usize, height: usize) -> Option<Rect> {
    if frames.len() < MIN_FRAMES {
        return None;
    }
    let pick = |f: fn(&Bars) -> usize| {
        let mut v: Vec<usize> = frames.iter().map(f).collect();
        v.sort_unstable();
        if v.len() >= 5 { v[1] } else { v[0] }
    };
    let min_v = ((height as f64 * MIN_BAR_FRACTION).ceil() as usize).max(4);
    let min_h = ((width as f64 * MIN_BAR_FRACTION).ceil() as usize).max(4);
    let keep = |bar: usize, min: usize| if bar < min { 0 } else { bar };
    // Rounded to even numbers, inward: 4:2:0 chroma has one sample per two
    // pixels, and an odd edge would split one.
    let even_up = |n: usize| (n + 1) & !1;
    let top = even_up(keep(pick(|b| b.top), min_v));
    let bottom = even_up(keep(pick(|b| b.bottom), min_v));
    let left = even_up(keep(pick(|b| b.left), min_h));
    let right = even_up(keep(pick(|b| b.right), min_h));
    if top + bottom + left + right == 0 {
        return None;
    }
    let w = width.checked_sub(left + right)?;
    let h = height.checked_sub(top + bottom)?;
    if (w as f64) < width as f64 * MIN_KEEP_FRACTION || (h as f64) < height as f64 * MIN_KEEP_FRACTION {
        return None;
    }
    Some(Rect { x: left, y: top, w, h })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An 8-bit limited-range frame: black bars around a flat picture of `level`.
    fn frame(w: usize, h: usize, bars: Bars, level: u8) -> Vec<u8> {
        let mut v = vec![16u8; w * h];
        for y in bars.top..h - bars.bottom {
            for x in bars.left..w - bars.right {
                v[y * w + x] = level;
            }
        }
        v
    }

    fn luma(data: &[u8], w: usize, h: usize) -> Luma<'_> {
        Luma { data, stride: w, width: w, height: h, bytes: 1, shift: 0, depth: 8, full_range: false }
    }

    const LETTERBOX: Bars = Bars { top: 138, bottom: 138, left: 0, right: 0 };

    #[test]
    fn letterbox_is_measured() {
        let d = frame(1920, 1080, LETTERBOX, 120);
        assert_eq!(frame_bars(&luma(&d, 1920, 1080)), Some(LETTERBOX));
    }

    #[test]
    fn pillarbox_is_measured_over_the_picture_rows_only() {
        // 4:3 inside 16:9, and letterboxed as well: a windowbox.
        let b = Bars { top: 60, bottom: 60, left: 240, right: 240 };
        let d = frame(1920, 1080, b, 90);
        assert_eq!(frame_bars(&luma(&d, 1920, 1080)), Some(b));
    }

    #[test]
    fn a_black_frame_says_nothing() {
        let d = vec![16u8; 1920 * 1080];
        assert_eq!(frame_bars(&luma(&d, 1920, 1080)), None);
    }

    #[test]
    fn noise_in_the_bar_does_not_end_it() {
        let mut d = frame(1920, 1080, LETTERBOX, 120);
        // A few hot pixels in the top bar, under one in fifty of the sampled ones.
        for x in (0..1920).step_by(400) {
            d[10 * 1920 + x] = 200;
        }
        // Compression noise a few levels above black.
        for y in 0..138 {
            d[y * 1920 + 7] = 22;
        }
        assert_eq!(frame_bars(&luma(&d, 1920, 1080)), Some(LETTERBOX));
    }

    #[test]
    fn ten_bit_p010_is_read_through_its_shift() {
        let (w, h) = (64usize, 36usize);
        let mut d = vec![0u8; w * h * 2];
        for y in 0..h {
            for x in 0..w {
                // Limited-range black is 64 in 10 bits; picture at 500.
                let v: u16 = if (6..30).contains(&y) { 500 } else { 64 };
                let word = v << 6;
                d[(y * w + x) * 2..][..2].copy_from_slice(&word.to_le_bytes());
            }
        }
        let l = Luma { data: &d, stride: w * 2, width: w, height: h, bytes: 2, shift: 6, depth: 10, full_range: false };
        assert_eq!(frame_bars(&l), Some(Bars { top: 6, bottom: 6, left: 0, right: 0 }));
    }

    #[test]
    fn one_frame_with_a_subtitle_in_the_bar_is_outvoted() {
        let with_sub = Bars { top: 138, bottom: 40, left: 0, right: 0 };
        let frames = [LETTERBOX, LETTERBOX, with_sub, LETTERBOX, LETTERBOX, LETTERBOX];
        assert_eq!(combine(&frames, 1920, 1080), Some(Rect { x: 0, y: 138, w: 1920, h: 804 }));
    }

    #[test]
    fn two_frames_reaching_an_edge_make_it_picture() {
        // An open-matte film: most scenes letterboxed, two shown full frame.
        let full = Bars { top: 0, bottom: 0, left: 0, right: 0 };
        let frames = [LETTERBOX, full, LETTERBOX, full, LETTERBOX, LETTERBOX];
        assert_eq!(combine(&frames, 1920, 1080), None);
    }

    #[test]
    fn a_dark_scene_does_not_widen_the_crop() {
        let dark = Bars { top: 300, bottom: 280, left: 100, right: 90 };
        let frames = [LETTERBOX, dark, LETTERBOX, LETTERBOX];
        assert_eq!(combine(&frames, 1920, 1080), Some(Rect { x: 0, y: 138, w: 1920, h: 804 }));
    }

    #[test]
    fn slivers_are_ignored_and_odd_bars_round_inward() {
        let b = Bars { top: 3, bottom: 2, left: 131, right: 131 };
        let frames = [b, b, b];
        assert_eq!(combine(&frames, 1440, 1080), Some(Rect { x: 132, y: 0, w: 1176, h: 1080 }));
    }

    #[test]
    fn too_few_frames_is_no_answer() {
        assert_eq!(combine(&[LETTERBOX, LETTERBOX], 1920, 1080), None);
    }
}
