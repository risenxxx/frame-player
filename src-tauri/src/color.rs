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

/// ITU reference white: what a correctly graded HDR frame puts a sheet of paper
/// at, and therefore what becomes white on an SDR thumbnail. Applies to PQ and
/// HLG, whose code values mean nits.
const DIFFUSE_WHITE: f32 = 203.0 / 10_000.0;

/// The same anchor for a Dolby Vision profile 5 base layer, which is a fitted
/// number rather than a standard one: its levels only mean nits once the RPU
/// has reshaped them, and a plain decoder never does that.
///
/// Fitted against the player's own output — mpv renders these files correctly
/// through libplacebo — on four scenes of one episode spanning the range worth
/// spanning: a sunlit park, a warm interior, a dim greenhouse and a near-black
/// night exterior. Reading a profile 5 frame at 203 like PQ renders that
/// interior almost black; the robe that carries the scene measures **2.9 nits**
/// in this signal.
///
/// **60 rather than the 40 the screenshots alone gave**, which is worth
/// recording because it says what the fitting can and cannot settle: a still
/// compared side by side gets the *hue* exact and leaves half a stop of
/// brightness inside the noise, and half a stop is precisely what reads as "a
/// bit bright" once the previews are in front of somebody in the player. The
/// number is a knob, not a derivation — one constant, and raising it darkens
/// everything by the same amount, which is the point of anchoring per file
/// rather than per frame.
const DOLBY_WHITE: f32 = 60.0 / 10_000.0;

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

/// IPT → LMS' (still PQ-encoded), the inverse of the Ebner–Fairchild matrix.
///
/// **This is IPT and not ICtCp, and the difference is not academic.** The two
/// are near neighbours — ICtCp is BT.2100's requantised descendant of IPT — and
/// the ICtCp inverse is the one every reference to "Dolby Vision uses ICtCp"
/// leads to. Fitting both against a frame whose colours are known (a
/// greenhouse the player renders green) settles it: over the bank of leaves,
/// the ICtCp inverse gives `r/g/b 0.53 / 0.27 / 0.20` — foliage rendered red —
/// while IPT gives `0.40 / 0.44 / 0.15`. The failure is not subtle once there
/// is something in frame whose colour is not negotiable, and it is invisible on
/// interiors and skin, which is exactly where it was first judged "plausible".
///
/// The plane order goes with it: `I, P, T`, so the **U plane is P** (the
/// red–green axis, where a Y'CbCr reader expects blue–yellow) and the V plane
/// is T. Reading them the ICtCp way — Ct then Cp — is a second, independent way
/// to get the same class of wrongness.
fn ipt_to_lms(i: f32, p: f32, t: f32) -> [f32; 3] {
    [
        i + 0.097_569 * p + 0.205_226 * t,
        i - 0.113_876 * p + 0.133_217 * t,
        i + 0.032_615 * p - 0.676_887 * t,
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
/// both the colour and the *level* of what comes out are approximations — the
/// colour a good one, once the matrix above is the right one, and the level a
/// calibration, which is what `DOLBY_WHITE` is.
fn dolby5_to_linear(i: f32, p: f32, t: f32) -> [f32; 3] {
    let lms = ipt_to_lms(i, p, t);
    let linear = [pq_eotf(lms[0]), pq_eotf(lms[1]), pq_eotf(lms[2])];
    lms_to_bt2020(undo_crosstalk(linear))
}

/// Where white sits for this frame.
///
/// **One number for the whole file, never the frame's own light.** Exposing
/// each frame from its own percentile was the first shape of this and it is
/// wrong in the way that gets reported: it hands every frame the same average
/// brightness, so a night scene comes back as a grey afternoon. Measured across
/// four scenes of one episode, the 99.5th percentile runs from 4.8 nits (jets
/// at dusk) to 95.8 (a sunlit park) — a factor of twenty, i.e. twenty times the
/// wrong gain — while what the viewer wants from a preview is precisely the
/// difference between those two scenes.
///
/// For PQ and HLG the encoded value means nits, so the anchor is the ITU
/// reference white of 203 and there is nothing to calibrate. A profile 5 base
/// layer means nothing until the RPU has reshaped it, so its anchor is fitted
/// instead — see `DOLBY_WHITE`.
fn white_point(kind: Hdr) -> f32 {
    match kind {
        Hdr::Dolby5 => DOLBY_WHITE,
        Hdr::Pq | Hdr::Hlg => DIFFUSE_WHITE,
    }
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
/// `planes` are Y, U, V as little-endian 16-bit samples with their own strides
/// in *samples* — for Dolby Vision that is I, P, T rather than luma and two
/// chroma difference signals, which is what `ipt_to_lms` is about.
/// `full_range` is the frame's own flag. The output is `w * h * 3` bytes,
/// tight.
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
    let white = white_point(kind);
    let wide = primaries == Primaries::Bt2020 || kind == Hdr::Dolby5;
    let mut out = vec![0u8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let s = |p: usize| planes[p][y * strides[p] + x] as f32 / 65535.0;
            let (mut a, mut b, mut c) = (s(0), s(1) - 0.5, s(2) - 0.5);
            if !full {
                a = (a - 16.0 / 255.0) / (219.0 / 255.0);
                b /= 224.0 / 255.0;
                c /= 224.0 / 255.0;
            }
            let linear = match kind {
                Hdr::Dolby5 => dolby5_to_linear(a, b, c),
                Hdr::Pq | Hdr::Hlg => {
                    let e = ycbcr_to_rgb(a, b, c, primaries == Primaries::Bt2020);
                    let f = if kind == Hdr::Pq { pq_eotf } else { hlg_eotf };
                    [f(e[0]), f(e[1]), f(e[2])]
                }
            };
            let mut v = [
                expose(linear[0].max(0.0), white),
                expose(linear[1].max(0.0), white),
                expose(linear[2].max(0.0), white),
            ];
            if wide {
                v = bt2020_to_bt709(v);
            }
            let o = (y * w + x) * 3;
            for ch in 0..3 {
                out[o + ch] = (srgb_oetf(v[ch].clamp(0.0, 1.0)) * 255.0).round() as u8;
            }
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

    /// **A dark frame has to come back dark.** This is the bug that shipped in
    /// the first version of this module: the white point was the frame's own
    /// 99.5th percentile, so a night exterior was scaled up until it looked
    /// like an overcast afternoon, and every scene of a film arrived at the
    /// same average brightness. The exposure now depends on the encoding and
    /// nothing else, which is what makes two scenes comparable.
    #[test]
    fn exposure_does_not_depend_on_the_frame() {
        let level = |nits: f32| (srgb_oetf(expose(nits / 10_000.0, DOLBY_WHITE)) * 255.0) as u8;
        // The medians of two real scenes of the calibration episode, in this
        // signal: jets at dusk against a sunlit park.
        let night = level(0.45);
        let day = level(4.94);
        // Deliberately loose, and about the *relationship* rather than the
        // level: `DOLBY_WHITE` is a calibration and will be nudged again, while
        // what must never come back is a night frame arriving at the same
        // brightness as a daylight one.
        assert!(night < 40, "a night frame came back at {night}");
        assert!(day > night + 40, "day {day} is not clear of night {night}");
    }
}
