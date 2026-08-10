//! Turning a decoded frame into something a person can look at.
//!
//! The thumbnail service used to hand every frame to swscale with nothing but
//! the pixel formats — no matrix, no range, no transfer — which means swscale
//! falls back to its default coefficients (BT.601) whatever the file says. For
//! an ordinary SDR file that is a small hue error; for anything HDR it is not,
//! and for **Dolby Vision profile 5 it is the pink picture** that gets reported
//! as "the previews are broken while the video is fine".
//!
//! So there are two paths. Everything SDR stays on swscale, which is fast and
//! correct once it is told the matrix and the range (`sws_setColorspaceDetails`
//! at the call site). Everything HDR comes through here: swscale is asked for
//! `yuv444p16le` — a format change and a resize, no matrix applied — and the
//! rest is done in floating point on a 320px frame, which is some forty
//! thousand pixels and costs nothing next to decoding one.
//!
//! What this is not: a colour-managed pipeline. It is what makes a *preview*
//! recognisable — the frame the viewer is about to seek to. mpv renders the
//! real thing through libplacebo, and that stays the only correct answer for
//! anything the viewer looks at properly (which is why saving a frame goes
//! through mpv's own `screenshot-to-file` and not through here).

/// How the luminance and the colour of a frame are encoded, when it is not the
/// ordinary SDR case swscale can be trusted with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hdr {
    /// SMPTE ST 2084 — HDR10, and the common case for a 4K release.
    Pq,
    /// ARIB STD-B67 — HLG, mostly broadcast.
    Hlg,
    /// Dolby Vision profile 5: the picture is IPT-PQ-C2 rather than Y'CbCr, and
    /// the RPU that carries the reshaping curves is not something a plain
    /// decoder applies. See `dolby5_to_linear` for what is approximated away.
    Dolby5,
}

/// Which primaries the linear RGB coming out of the conversion is in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Primaries {
    Bt709,
    Bt2020,
}

/// The 99.5th percentile of the frame's own light is what becomes white, and it
/// is never lifted above this floor for content whose levels are meaningful:
/// 203 nits is the ITU reference white, i.e. what a correctly graded HDR frame
/// puts a sheet of paper at. Without the floor a dim night scene would be
/// exposed like daylight; without the percentile a Dolby Vision frame — whose
/// absolute levels mean nothing until the RPU has been applied — comes out
/// almost black.
const DIFFUSE_WHITE: f32 = 203.0 / 10_000.0;

/// How far above white the roll-off reaches before it clips, in units of white.
/// Extended Reinhard: highlights compress instead of turning into flat patches,
/// which on a thumbnail is the difference between a lamp and a white blob.
const HIGHLIGHT_HEADROOM: f32 = 4.0;

/// SMPTE ST 2084 (PQ) EOTF. Input is the encoded value, output is luminance
/// with 1.0 = 10 000 nits.
pub fn pq_eotf(v: f32) -> f32 {
    const M1: f32 = 2610.0 / 16384.0;
    const M2: f32 = (2523.0 / 4096.0) * 128.0;
    const C1: f32 = 3424.0 / 4096.0;
    const C2: f32 = (2413.0 / 4096.0) * 32.0;
    const C3: f32 = (2392.0 / 4096.0) * 32.0;
    let p = v.max(0.0).powf(1.0 / M2);
    ((p - C1).max(0.0) / (C2 - C3 * p)).powf(1.0 / M1)
}

/// ARIB STD-B67 (HLG) EOTF, scene light, normalised the same way as `pq_eotf`
/// against a 1000-nit display — the reference HLG peak.
pub fn hlg_eotf(v: f32) -> f32 {
    const A: f32 = 0.17883277;
    const B: f32 = 1.0 - 4.0 * A;
    // `ln` is not const, and spelling the value out by hand is how a constant
    // stops matching the formula above it.
    let c = 0.5 - A * (4.0 * A).ln();
    let v = v.clamp(0.0, 1.0);
    let e = if v <= 0.5 {
        v * v / 3.0
    } else {
        (((v - c) / A).exp() + B) / 12.0
    };
    // System gamma for a 1000-nit reference display, then to the 10 000-nit
    // scale everything downstream works in.
    e.powf(1.2) * 0.1
}

/// ICtCp → LMS' (BT.2100), i.e. still PQ-encoded.
fn ictcp_to_lms(i: f32, ct: f32, cp: f32) -> [f32; 3] {
    [
        i + 0.008_609_04 * ct + 0.111_03 * cp,
        i - 0.008_609_04 * ct - 0.111_03 * cp,
        i + 0.560_031 * ct - 0.320_627 * cp,
    ]
}

/// Undo the 2 % channel cross-talk Dolby mixes into LMS before encoding — the
/// "C2" in IPT-PQ-C2. It exists to keep saturated colours inside the container
/// gamut; leaving it in place tints everything.
/// The mix is `(1 − 3a)·I + a·J` (each row keeps its sum at 1, so neutral stays
/// neutral), and its inverse is `(I − a·J) / (1 − 3a)` — a **(1 − a)** on the
/// diagonal, not the (1 − 2a) that mirrors the forward matrix and is the shape
/// one writes by hand. The difference is 2 % of every channel: too small to see
/// on a thumbnail and exactly the kind of thing that then stays wrong for
/// years, which is what `crosstalk_undo_inverts_the_mix` is for.
fn undo_crosstalk([l, m, s]: [f32; 3]) -> [f32; 3] {
    const A: f32 = 0.02;
    let d = 1.0 - 3.0 * A;
    [
        ((1.0 - A) * l - A * m - A * s) / d,
        (-A * l + (1.0 - A) * m - A * s) / d,
        (-A * l - A * m + (1.0 - A) * s) / d,
    ]
}

/// LMS → linear BT.2020 RGB (the inverse of the BT.2100 LMS matrix).
fn lms_to_bt2020([l, m, s]: [f32; 3]) -> [f32; 3] {
    [
        3.436_607 * l - 2.506_452 * m + 0.069_845 * s,
        -0.791_330 * l + 1.9836 * m - 0.192_271 * s,
        -0.025_950 * l - 0.098_914 * m + 1.124_864 * s,
    ]
}

/// Linear BT.2020 → linear BT.709. Values outside the smaller gamut go negative
/// and are clipped by the caller, which is the honest thing to do at this size:
/// a gamut mapper would cost more than the whole conversion.
fn bt2020_to_bt709([r, g, b]: [f32; 3]) -> [f32; 3] {
    [
        1.660_491 * r - 0.587_641 * g - 0.072_850 * b,
        -0.124_550 * r + 1.1329 * g - 0.008_350 * b,
        -0.018_151 * r - 0.100_579 * g + 1.118_73 * b,
    ]
}

/// The sRGB transfer function, which is what a JPEG thumbnail is read as.
fn srgb_oetf(x: f32) -> f32 {
    if x <= 0.003_130_8 {
        12.92 * x
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

/// Y'CbCr → R'G'B' for a normalised, range-corrected pixel.
///
/// Only two matrices are worth carrying: BT.2020 non-constant luminance, which
/// every HDR release uses, and BT.709 for the odd file that is PQ-tagged
/// without being BT.2020. Constant-luminance BT.2020 exists on paper and in
/// approximately no files.
fn ycbcr_to_rgb(y: f32, cb: f32, cr: f32, bt2020: bool) -> [f32; 3] {
    let (kr, kb) = if bt2020 { (0.2627, 0.0593) } else { (0.2126, 0.0722) };
    let kg = 1.0 - kr - kb;
    [
        y + 2.0 * (1.0 - kr) * cr,
        y - (2.0 * (kb * (1.0 - kb) * cb + kr * (1.0 - kr) * cr)) / kg,
        y + 2.0 * (1.0 - kb) * cb,
    ]
}

/// Dolby Vision profile 5, as far as it can be taken without the RPU.
///
/// **What is missing and why it is still worth doing.** The base layer is
/// reshaped: the RPU carries polynomial and MMR curves that map these codes
/// back to the mastered signal, and libavcodec does not apply them (libplacebo
/// does, which is why the picture is right in the player and wrong here). So
/// both the colour and the *level* of what comes out are approximations. The
/// colour approximation is good — the ICtCp inverse plus the cross-talk undo
/// removes the magenta cast that makes these previews unusable — while the
/// level is not, which is what `expose` is for.
fn dolby5_to_linear(i: f32, ct: f32, cp: f32) -> [f32; 3] {
    let lms = ictcp_to_lms(i, ct, cp);
    let linear = [pq_eotf(lms[0]), pq_eotf(lms[1]), pq_eotf(lms[2])];
    lms_to_bt2020(undo_crosstalk(linear))
}

/// Where white sits for this frame.
///
/// `trust_levels` says whether the encoded luminance means nits. For PQ and HLG
/// it does, so the exposure is anchored at reference white and only a brighter
/// frame moves it — a dark scene stays dark, exactly as it does in the player.
/// For Dolby Vision it does not, so the frame's own light is all there is to go
/// on.
fn white_point(lum: &mut [f32], trust_levels: bool) -> f32 {
    if lum.is_empty() {
        return DIFFUSE_WHITE;
    }
    lum.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p = lum[((lum.len() - 1) as f32 * 0.995) as usize];
    let floor = if trust_levels { DIFFUSE_WHITE } else { 1e-6 };
    p.max(floor)
}

/// Everything above white rolls off towards `HIGHLIGHT_HEADROOM` instead of
/// clipping there.
fn expose(x: f32, white: f32) -> f32 {
    let y = x / white;
    const W2: f32 = HIGHLIGHT_HEADROOM * HIGHLIGHT_HEADROOM;
    (y * (1.0 + y / W2)) / (1.0 + y)
}

/// A whole frame of planar 16-bit YUV → packed sRGB bytes.
///
/// `planes` are Y, U (or Ct), V (or Cp) as little-endian 16-bit samples with
/// their own strides in *samples*; `full_range` is the frame's own flag. The
/// output is `w * h * 3` bytes, tight.
pub fn yuv444_16_to_srgb(
    planes: [&[u16]; 3],
    strides: [usize; 3],
    w: usize,
    h: usize,
    kind: Hdr,
    primaries: Primaries,
    full_range: bool,
) -> Vec<u8> {
    // Dolby Vision is signalled limited-range by the container as often as not,
    // and it is neither: the RPU decides. Full range is the better guess — it
    // is what the format specifies — and being wrong costs a little contrast
    // rather than the hue.
    let full = full_range || kind == Hdr::Dolby5;
    let mut linear = vec![0f32; w * h * 3];
    let mut lum = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let s = |p: usize| planes[p][y * strides[p] + x] as f32 / 65535.0;
            let (mut a, mut b, mut c) = (s(0), s(1) - 0.5, s(2) - 0.5);
            if !full {
                a = (a - 16.0 / 255.0) / (219.0 / 255.0);
                b /= 224.0 / 255.0;
                c /= 224.0 / 255.0;
            }
            let rgb = match kind {
                Hdr::Dolby5 => dolby5_to_linear(a, b, c),
                Hdr::Pq | Hdr::Hlg => {
                    let e = ycbcr_to_rgb(a, b, c, primaries == Primaries::Bt2020);
                    let f = if kind == Hdr::Pq { pq_eotf } else { hlg_eotf };
                    [f(e[0]), f(e[1]), f(e[2])]
                }
            };
            let o = (y * w + x) * 3;
            for ch in 0..3 {
                linear[o + ch] = rgb[ch].max(0.0);
            }
            // BT.2020 luma weights: this only picks the exposure, so the exact
            // primaries matter less than picking one and staying with it.
            lum[y * w + x] = 0.2627 * linear[o] + 0.678 * linear[o + 1] + 0.0593 * linear[o + 2];
        }
    }
    let white = white_point(&mut lum, kind != Hdr::Dolby5);
    let mut out = vec![0u8; w * h * 3];
    for p in 0..w * h {
        let mut v = [
            expose(linear[p * 3], white),
            expose(linear[p * 3 + 1], white),
            expose(linear[p * 3 + 2], white),
        ];
        if primaries == Primaries::Bt2020 || kind == Hdr::Dolby5 {
            v = bt2020_to_bt709(v);
        }
        for ch in 0..3 {
            out[p * 3 + ch] = (srgb_oetf(v[ch].clamp(0.0, 1.0)) * 255.0).round() as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two ends and the reference point of the PQ curve, from ST 2084
    /// itself: these are what every later step is measured against, so an
    /// arithmetic slip here would quietly mis-expose every HDR preview.
    #[test]
    fn pq_hits_its_reference_levels() {
        assert!(pq_eotf(0.0).abs() < 1e-6);
        assert!((pq_eotf(1.0) - 1.0).abs() < 1e-3);
        // 0.5081 is the ST 2084 code for 100 nits, i.e. 0.01 of the 10 000-nit
        // scale everything here works in.
        assert!((pq_eotf(0.508_1) - 0.01).abs() < 5e-4, "{}", pq_eotf(0.508_1));
    }

    /// Neutral in must be neutral out, whatever the path: a grey frame that
    /// comes back tinted is precisely the bug this module exists to fix, and it
    /// is the one failure a person notices instantly.
    #[test]
    fn grey_stays_grey() {
        for kind in [Hdr::Pq, Hdr::Dolby5] {
            let y = vec![32768u16; 4];
            let c = vec![32768u16; 4];
            let out = yuv444_16_to_srgb(
                [&y, &c, &c],
                [2, 2, 2],
                2,
                2,
                kind,
                Primaries::Bt2020,
                true,
            );
            for px in out.chunks(3) {
                let (r, g, b) = (px[0] as i32, px[1] as i32, px[2] as i32);
                assert!((r - g).abs() <= 2 && (g - b).abs() <= 2, "{kind:?} gave {px:?}");
            }
        }
    }

    /// The cross-talk undo has to be the exact inverse of the mix, or every
    /// colour drifts by 2 % in a direction nobody would think to look for.
    #[test]
    fn crosstalk_undo_inverts_the_mix() {
        const A: f32 = 0.02;
        let mixed = |v: [f32; 3]| {
            [
                (1.0 - 2.0 * A) * v[0] + A * v[1] + A * v[2],
                A * v[0] + (1.0 - 2.0 * A) * v[1] + A * v[2],
                A * v[0] + A * v[1] + (1.0 - 2.0 * A) * v[2],
            ]
        };
        let v = [0.3f32, 0.7, 0.1];
        let back = undo_crosstalk(mixed(v));
        for i in 0..3 {
            assert!((back[i] - v[i]).abs() < 1e-4, "{back:?} vs {v:?}");
        }
    }

    /// Levels are trusted for PQ and not for Dolby Vision, and that difference
    /// is the whole reason a dark DV frame comes out visible while a dark HDR10
    /// one stays dark — as it does in the player.
    #[test]
    fn exposure_trusts_pq_levels_and_not_dolby() {
        let mut dark = vec![0.000_5f32; 100];
        assert_eq!(white_point(&mut dark.clone(), true), DIFFUSE_WHITE);
        assert!(white_point(&mut dark, false) < DIFFUSE_WHITE);
        // A bright frame moves the white point in both modes.
        let mut bright = vec![0.5f32; 100];
        assert!(white_point(&mut bright, true) > DIFFUSE_WHITE);
    }
}
