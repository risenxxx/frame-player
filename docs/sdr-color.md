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
that mpv reads as `bt.1886`, not only to files without tags.

The primaries already matched before anything was changed — BT.709 red is
232,51,35 in QuickTime and 231,50,34 in ours — because our Vulkan layer is
tagged sRGB and ColorSync converts it for the display exactly as it converts
QuickTime's output.

IINA, measured on the same stills for reference, drew the grey bars within
0.4 of our "as in mpv". It is the same mpv behaviour.

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
  IINA renders through its own layer, and on this machine, with its current
  defaults, it drew exactly what we draw.)
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

"As in mpv" is what mpv-based players on macOS draw (IINA measured identical),
and a player that silently changed it would look wrong to the people who chose
it for mpv's rendering. "As in macOS" is what the same file looks like in
QuickTime — and, by the report that started this, in other AVFoundation
players — which is what most people compare against. Both are defensible, the
difference is visible, and the viewer is the one who knows which of the two
they expect — so it is theirs to choose. It is macOS only because the
difference is between mpv and AVFoundation; on Windows there is no system
player whose curve anybody expects.
