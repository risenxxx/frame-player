# SDR color on macOS: mpv against QuickTime

The complaint this chapter measures: the same SDR file looks darker in an
mpv-based player than in QuickTime, with the shadows "crushed". It is usually
reported on MKV files that declare no color at all (`ffprobe` shows neither
`color_space` nor `color_range`), and the fix passed around for IINA is
`gamma-factor=1.1218765935747068`, `vf=format=gamma=gamma2.2` and ICC enabled.

The short answer, measured: **the difference is real, it is not about missing
metadata, and the recipe from IINA is wrong for this player** — its ICC half
colour-manages our picture twice. What does match QuickTime is in
`SDR_COLOR_OPTIONS` in `src/lib/player.svelte.ts`, offered as
Settings → Video → "SDR video color" → "As in macOS". The rule is in
[rules/mpv-playback.md](rules/mpv-playback.md).

## How it was measured

**The file.** A 1280×720 H.264 still (so any moment of playback is the same
frame) with three parts: sixteen grey bars of known luma — Y = 16, 20, 24, 28,
32, 40, 48, 64, 80, 96, 112, 128, 160, 192, 224, 235 — across the middle third,
`testsrc2` above and below it for saturated primaries, and an 8-pixel white
border so the video rectangle can be found in a screenshot of any window. The
colour metadata is stripped at both levels: `setparams` sets everything to
unknown before `libx264`, and the stream is remuxed from a raw `.h264` so the
container carries nothing either. `ffprobe` then reports `unknown` for
range, space, transfer and primaries, in the stream and in the frame. Two more
stills were made the same way from real footage: a night-side Earth (96 % of
its pixels below Y = 64) and a frame of a 1080p anime episode (a bright
interior, a third of its pixels in the mid-tones).

The same stream went into MKV for our player and MP4 for QuickTime, which does
not open MKV. A copy tagged BT.709 (`h264_metadata` + `colr`) was made to see
whether the tags matter.

**The instrument.** `screencapture -l <window>` of each player's window on the
same display — a MacBook Pro's built-in XDR panel, captured in Display P3.
This compares what the two players send to the display, which is the question;
mpv's `screenshot-to-file` alone could not, since QuickTime has no equivalent.
(It turned out to agree with the window capture to ±1 level anyway — see
below.) Pixels were read with numpy: the mean of each grey bar, the six colour
bars, and for the real frames the mean of the whole picture, of the pixels
whose *source* luma is below 64 ("shadows") and of those between 64 and 159
("mids"). Settings were changed through mpv's IPC socket on a dev build, and
the final numbers were taken from the player driving itself through the
setting's own code.

## What QuickTime does

Grey bars, display values out of 255:

| Y | QuickTime | Frame Player, "as in mpv" | Frame Player, "as in macOS" |
|---|---|---|---|
| 16 | 0.0 | 0.1 | 0.1 |
| 20 | 3.7 | 4.3 | 1.2 |
| 24 | 6.3 | 7.8 | 3.6 |
| 28 | 11.0 | 13.6 | 10.6 |
| 32 | 17.3 | 18.3 | 17.2 |
| 40 | 29.7 | 27.6 | 29.7 |
| 48 | 40.0 | 35.7 | 40.0 |
| 64 | 63.3 | 55.5 | 63.4 |
| 80 | 84.0 | 74.1 | 84.0 |
| 96 | 102.3 | 91.6 | 102.5 |
| 112 | 123.0 | 111.4 | 122.6 |
| 128 | 140.0 | 128.7 | 139.9 |
| 160 | 176.7 | 167.1 | 176.5 |
| 192 | 210.7 | 204.5 | 210.7 |
| 224 | 242.7 | 240.6 | 242.6 |
| 235 | 254.7 | 254.3 | 254.5 |

mpv hands the display the code values untouched (Y = 128 is 50.7 % of the
range and comes out as 128.7), and the display reads them with the sRGB curve.
QuickTime is up to 12 levels brighter from Y = 40 to Y = 160 — about 8 % at
mid-grey — and fits one model closely: **decode BT.709 with a pure power of
~1.961, encode for the display with the sRGB curve.** At Y = 64 that predicts
63.6 and QuickTime shows 63.3; at Y = 128, 141 against 140. 2.2 / 1.961 is the
`gamma-factor` in the IINA recipe.

So "crushed shadows" is not quite the shape of it. The deepest steps (Y = 20–32)
are, if anything, a level or two *lighter* in mpv; what mpv loses is the lower
mid-tones, where most of a dark scene actually lives. On real frames:

| Frame | Player | Mean | Shadows | Mids |
|---|---|---|---|---|
| Earth at night | QuickTime | 14.4 | 9.9 | 102.1 |
| | "as in mpv" | 13.4 | 9.2 | 92.8 |
| | "as in macOS" | 14.2 | 9.7 | 102.1 |
| Anime interior | QuickTime | 177.5 | 54.6 | 122.4 |
| | "as in mpv" | 170.9 | 47.9 | 111.9 |
| | "as in macOS" | 177.4 | 54.3 | 122.0 |

**The metadata is not the cause.** The BT.709-tagged copy measured identically
in both players, bar for bar, to the untagged one: mpv guesses BT.1886 for an
untagged HD video and QuickTime guesses BT.709, and both then treat it exactly
as they treat a tagged file. The mode therefore applies to every SDR video
that mpv reads as `bt.1886`, not only to files without tags. (IINA is the exception — see below.)

The primaries already matched before anything was changed — BT.709 red is
232,51,35 in QuickTime and 231,50,34 in ours — because our Vulkan layer is
tagged sRGB and ColorSync converts it for the display exactly as it converts
QuickTime's output.

IINA (1.4.4), measured on the same stills for reference, drew the grey bars
within 0.4 of our "as in mpv" — **on the untagged file**. On the BT.709-tagged
copy of that same file it drew something else: Y = 20 at 0.3, 28 at 2.8, 64 at
44.4, 128 at 121.7, which is, to within half a level, the `icc-profile-auto`
row of the table below. So in IINA the tags *do* change the picture, and in the
direction of the complaint — darker, with the lowest steps pushed into black.
Why it takes that path for a tagged file was not investigated; the numbers are
the finding. For QuickTime and for this player the tags change nothing.

## Which settings reproduce it

Each row is one combination, applied live to the grey-bar file:

| Variant | Y = 20 | Y = 28 | Y = 64 | Y = 128 | Red |
|---|---|---|---|---|---|
| QuickTime | 3.7 | 11.0 | 63.3 | 140.0 | 232,51,35 |
| mpv defaults | 4.3 | 13.6 | 55.5 | 128.7 | 231,50,34 |
| `icc-profile-auto` only | 0.3 | 3.2 | 44.7 | 122.0 | — |
| IINA recipe: `gamma-factor` + `format=gamma=gamma2.2` + ICC | 1.3 | 10.8 | 63.5 | 139.9 | **213,68,49** |
| `gamma-factor` + `format=gamma=gamma2.2`, no ICC | 6.7 | 18.7 | 65.5 | 138.6 | 231,50,34 |
| … + `target-trc=srgb` | 6.7 | 18.7 | 65.5 | 138.6 | 231,50,34 |
| … + `treat-srgb-as-power22=no` | **1.2** | **10.6** | **63.4** | **139.9** | **231,50,34** |

Three findings in that table:

- **ICC colour-manages our picture twice.** The IINA recipe gets the tone
  curve right and desaturates every primary (green 117,251,76 → 152,244,103),
  because mpv converts to the display profile and then macOS converts the
  "sRGB" layer to the display again. ICC on its own is the darkest row of all.
  Neither belongs here. (Why the recipe works in IINA was not investigated;
  IINA renders through its own layer, and it drew exactly what we draw on an
  untagged file and the ICC-only row on a tagged one.)
- **Without ICC the curve alone lifts the shadows** (Y = 28 at 18.7 against
  11.0): mpv encodes for a pure power 2.2 display, and the display is sRGB,
  whose toe is linear.
- **`target-trc=srgb` does nothing by itself.** mpv 0.41's
  `treat-srgb-as-power22` defaults to `auto`, which treats an sRGB target as
  pure 2.2 anyway. Setting it to `no` is what makes the encode the real sRGB
  curve, and that is the row that matches — within 0.5 of a level from Y = 32
  up, and within 2.5 below it, where QuickTime is a little lighter than its
  own model predicts.

A curve without the `format` filter does not work: mpv's BT.1886 is not a pure
power (it accounts for the display's black level), so no single
`gamma-factor` on top of it reproduced the 1.961 — the nearest attempt lifted
Y = 20 to 15.4.

## The colour-bar test, and what it measures

A thread on r/macapps (1stnktc) ranked Mac video players by colour accuracy:
SMPTE HD colour bars, BT.709, encoded losslessly with FFmpeg, each bar read
with Digital Color Meter in "Display in sRGB" mode on a built-in Mac display,
and the average deviation from the theoretical BT.709 values. It was repeated
here to find out what that number is a measurement *of*.

The file — the three tags have to be forced into x264, which otherwise takes
them from the frame and writes `unknown` for the transfer and the primaries:

```
ffmpeg -f lavfi -i smptehdbars=size=1920x1080:rate=30:duration=10 \
  -c:v libx264 -crf 0 -pix_fmt yuv420p \
  -color_primaries bt709 -color_trc bt709 -colorspace bt709 -color_range tv \
  -x264-params colorprim=bt709:transfer=bt709:colormatrix=bt709 bars.mp4
```

CRF 0 makes it lossless High 4:4:4 Predictive, which QuickTime plays. The
patches were found in the decoded frame itself (runs of identical Y'CbCr along
four rows), 27 of them; the middle of each was averaged in the window capture,
converted from the display's P3 to sRGB by ColorSync (`sips --matchTo`), and
compared with the **theoretical value: the patch's Y'CbCr through the BT.709
matrix, with the resulting R'G'B' code values read as sRGB**. The deviation is
the mean absolute difference over R, G and B, in 8-bit levels. The thread's own
formula could not be read (Reddit refuses automated access), so this is the
plain form of "average deviation"; a Euclidean or ΔE formula can reorder
players that are close — in mean ΔE2000 IINA (1.15) is ahead of QuickTime
(1.18), in the level metric it is behind.

| Patch | BT.709 as sRGB | QuickTime | "As in mpv" | "As in macOS" | IINA 1.4.4 |
|---|---|---|---|---|---|
| 40 % grey ×2 | 100,102,101 | 111,113,113 (11.5) | 100,102,101 (0.1) | 111,113,112 (11.2) | 91,93,92 (8.6) |
| 75 % white ×2 | 189,191,191 | 197,199,198 (7.5) | 189,191,191 (0.1) | 197,199,198 (7.4) | 185,188,187 (3.8) |
| 75 % yellow | 189,202,6 | 197,208,2 (5.9) | 188,202,6 (0.3) | 196,208,2 (5.6) | 184,199,2 (3.9) |
| 75 % cyan | 15,211,187 | 10,216,195 (6.2) | 15,211,186 (0.3) | 12,216,195 (5.4) | 6,208,182 (5.6) |
| 75 % green | 14,223,5 | 6,227,1 (5.2) | 13,223,4 (0.5) | 11,227,2 (3.2) | 4,221,2 (4.9) |
| 75 % magenta | 174,0,183 | 182,2,191 (6.0) | 174,1,183 (0.5) | 182,1,190 (5.5) | 169,0,178 (3.5) |
| 75 % red | 174,0,1 | 182,2,2 (3.6) | 174,1,2 (0.3) | 182,1,0 (3.2) | 168,0,1 (1.9) |
| 75 % blue | 0,0,182 | 0,0,190 (2.8) | 0,0,181 (0.2) | 0,0,190 (2.8) | 0,0,177 (1.7) |
| 100 % cyan | 20,255,251 | 24,255,252 (1.4) | 20,255,251 (0.2) | 19,255,252 (0.5) | 7,255,251 (4.5) |
| −I | 0,62,102 | 0,70,113 (6.5) | 0,61,102 (0.3) | 1,70,113 (6.6) | 1,51,93 (6.7) |
| 100 % blue | 1,0,241 | 1,0,243 (0.7) | 1,0,241 (0.1) | 0,0,243 (1.0) | 0,0,240 (0.8) |
| 100 % yellow | 251,255,8 | 251,255,6 (0.8) | 250,255,8 (0.3) | 251,255,4 (1.6) | 250,255,2 (2.4) |
| +Q | 60,0,116 | 68,0,127 (6.2) | 60,0,116 (0.1) | 69,0,127 (6.5) | 49,0,108 (6.6) |
| 0 % black ×5 | 0,0,0 | 0,0,0 (0.0) | 0,0,0 (0.1) | 0,0,0 (0.1) | 0,0,0 (0.1) |
| 100 % white ×2 | 253,255,255 | 254,255,255 (0.3) | 253,255,255 (0.2) | 254,255,255 (0.2) | 253,255,254 (0.2) |
| 100 % red | 232,0,0 | 234,0,1 (1.0) | 231,1,0 (0.4) | 235,1,1 (1.3) | 230,0,1 (0.8) |
| 15 % grey ×2 | 35,38,37 | 39,42,42 (4.2) | 35,38,37 (0.2) | 39,42,42 (4.3) | 23,26,25 (12.3) |
| +2 % (PLUGE) | 3,5,5 | 3,4,4 (0.7) | 3,5,5 (0.1) | 1,2,1 (3.1) | 0,0,0 (4.0) |
| +4 % (PLUGE) | 8,10,9 | 5,8,8 (1.9) | 8,10,9 (0.1) | 3,6,5 (4.3) | 1,1,1 (7.8) |
| **Mean of 27 patches** | | **3.54** | **0.20** | **3.61** | **3.91** |
| Worst patch | | 11.5 | 0.5 | 11.2 | 12.3 |

In mean ΔE2000 the four columns are 1.18, 0.09, 1.19 and 1.15.

**What it measures.** The theoretical values are the video's code values
handed to an sRGB display untouched — which is precisely what mpv does, so
"As in mpv" scores 0.20 by construction, not by being more careful than
anyone. QuickTime scores 3.54 for the reason the rest of this chapter is about:
its 1.96 curve lifts every 75 % channel by about 8 levels and the 40 % grey
by 11, while the 0 and 100 % patches, where the curves meet, stay within 1.4. "As in macOS" lands
on QuickTime's score (3.61) because it draws what QuickTime draws. A player
that reproduces QuickTime exactly will therefore always score about 3.5 in
this test, and one that passes code values through will always score about
zero: **the number says which BT.709 convention a player follows, not how
accurately it follows its own.** It cannot see a wrong hue that happens to sit
at 0 or 100 % either, and 7 of the 27 patches are black or full white.

IINA is the only column below the reference in the shadows — the 15 % grey
at 23 against 35, the +4 % PLUGE bar at 1 against 8 — which is its ICC path on
a tagged file (see above). The author's file is tagged, so that is the IINA the
thread measured.

**Colour bars against the grey ladder.** The hypothesis was that the bars show
much less of the difference between the two pipelines than the mid-tones do.
Measured directly, QuickTime against "As in mpv", in the same level metric:

| | Mean | Mid-tones (Y 40–160) | Worst |
|---|---|---|---|
| Colour bars, 27 patches | 3.60 | — | 11.45 |
| Grey ladder, 16 steps | 5.07 | 8.28 | 11.33 |

Confirmed: the bars average less than half the mid-tone gap. The breakdown
says why — the 7 patches at 0 or 100 % differ by 0.17, the 18 at 75 % and
100 % colour by 4.07, and only the two 40 % grey patches are mid-tones, at
11.45, the same as the worst step of the ladder. The bars are mostly made of
the places where the two curves meet. The ladder against the same kind of
reference ((Y − 16) / 219 × 255): QuickTime 4.70 (7.42 in the mid-tones),
"As in mpv" 1.00, "As in macOS" 5.04, IINA on the untagged file 1.38 — mpv's
one level being the ladder file's own, which is CRF 4 and decodes up to 1.5
levels off its nominal steps. QuickTime against "As in macOS" is 0.60 on the
bars and 0.56 on the ladder, worst 2.5 on both.

**The window capture against Digital Color Meter.** Both read on the same
QuickTime frame, cursor hidden:

- "Display native values": **identical on all 27 patches** — DCM's native
  values are the display-space pixels a window capture holds.
- "Display in sRGB": **within 1 level** on every patch (mean of the per-patch
  worst channel 0.33) against the capture converted with `sips --matchTo` —
  rounding, not a difference of method. Numbers from this chapter's captures
  and from DCM in sRGB mode are directly comparable.

Two traps for anyone repeating this. DCM samples the **cursor** along with the
screen, so with the pointer on a patch it reads the arrow's tip (252,252,252
on a 40 % grey); it has to be hidden. And DCM **stops updating while its window
is covered**, and does not see a window at screen-saver level at all. The sRGB
mode was read on the QuickTime capture shown 1:1 in a floating window with a
hole over DCM, since a background process cannot bring QuickTime forward; the
native mode was read on QuickTime itself.

## Things that were checked because they could have gone wrong

- **Live, without reopening.** All four parts are runtime properties. Switching
  in the settings changes the picture at once, a paused frame included, and
  switching back returns it to the same numbers.
- **Hardware decoding survives.** `hwdec-current` stays `videotoolbox` with the
  `format` filter in the chain: the filter only re-labels parameters.
- **HDR is never touched.** The decision reads `video-params/gamma`, which is
  the picture *entering* the filter chain; our filter changes
  `video-out-params`, so the mode cannot decide about itself. PQ, HLG and RGB
  images (`srgb`) are left alone. Going from an SDR file with the mode on into
  an HDR10 one: `video-params/gamma` reports `pq` at 175 ms after `loadfile`,
  the options are back at 214 ms, and the first frame is presented at 233 ms —
  no HDR frame is drawn through the SDR curve.
- **The viewer's mpv.conf wins.** A key set there (tested with
  `gamma-factor=1.05`) keeps its value in both positions; the filter is
  labeled `@fpsdr`, so a `vf` line of theirs is never replaced.
- **Screenshots follow the mode.** `screenshot-to-file … video` renders
  through gpu-next with the same target, and read back within ±1 level of the
  window capture in both positions — an exported frame looks the way the
  player showed it.

## Why it is an option and not the default

"As in mpv" is what mpv draws on macOS (IINA measured identical on an untagged
file),
and a player that silently changed it would look wrong to the people who chose
it for mpv's rendering. "As in macOS" is what the same file looks like in
QuickTime — and, by the report that started this, in other AVFoundation
players — which is what most people compare against. Both are defensible, the
difference is visible, and the viewer is the one who knows which of the two
they expect — so it is theirs to choose. It is macOS only because the
difference is between mpv and AVFoundation; on Windows there is no system
player whose curve anybody expects.
