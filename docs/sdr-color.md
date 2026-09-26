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

IINA (1.4.4), measured for reference, **drew the same content in two
different states**. In one it matched our "as in mpv" to within 0.4 on every
grey bar. In the other it drew Y = 20 at 0.3, 28 at 2.8, 64 at 44.4 and 128 at
121.7 — to within half a level the `icc-profile-auto` row of the table below:
darker, with the lowest steps pushed into black, which is the complaint's
picture. What selects the state was not isolated. Across five IINA
sessions, the BT.709-tagged ladder came out dark in all five of its opens, the
untagged 720p ladder light in five of its six, and the untagged 1080p colour
bars dark in the one open they had — so it is not simply the tag, and not
simply the first file after a launch. Every IINA number in this chapter is therefore "what IINA drew on
that open", not a property of IINA. For QuickTime and for this player the
tags change nothing, and the result does not vary between opens.

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
  IINA renders through its own layer, and it drew either exactly what we draw
  or the ICC-only row, depending on a state not isolated here.)
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

**Making the file is the first trap.** FFmpeg 8.1.2 labels the frames of its
`smptehdbars` source as `bt470bg` (BT.601), although the values in them are
the BT.709 bars of RP 219. So the obvious command — `-colorspace bt709` and
friends as output options — makes FFmpeg insert a 601 → 709 conversion on its
own (`auto_scale_0` in a verbose log) and re-colour the bars: 75 % red comes
out at R' = 174 instead of 191, 75 % yellow at 189,202,6 instead of
191,191,0, while 75 % white, which a matrix change cannot move, stays right.
Nothing warns about it, and the resulting file is still tagged BT.709. The
labels have to be put on the frames instead, which converts nothing:

```
ffmpeg -f lavfi -i smptehdbars=size=1920x1080:rate=30:duration=10 \
  -vf setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv \
  -c:v libx264 -crf 0 -pix_fmt yuv420p bars.mp4
```

That file decodes to exactly RP 219: every 75 % channel is Y'CbCr that goes
through the BT.709 matrix to 180 (limited) and every empty one to 16. The first
run of this measurement used the converted file; the table below is the
repeat on the clean one, and every conclusion drawn from the first survived it.

CRF 0 makes it lossless High 4:4:4 Predictive, which QuickTime plays, and
which VideoToolbox decodes. The patches were found in the decoded frame itself
(runs of identical Y'CbCr along four rows), 27 of them; the middle of each was
averaged in the window capture, converted from the display's P3 to sRGB by
ColorSync (`sips --matchTo`), and compared with the **theoretical value: the
patch's Y'CbCr through the BT.709 matrix, expanded from limited to full range,
and read as sRGB**. The deviation is the mean absolute difference over R, G
and B, in 8-bit levels. The thread's own formula could not be read (Reddit
refuses automated access), so this is the plain form of "average deviation";
a Euclidean or ΔE formula can reorder players that are close — in mean
ΔE2000 IINA (1.11) is ahead of QuickTime (1.16), in the level metric it is
behind.

| Patch | BT.709 as sRGB | QuickTime | "As in mpv" | "As in macOS" | IINA 1.4.4 |
|---|---|---|---|---|---|
| 40 % grey ×2 | 102,102,102 | 114,114,114 (11.5) | 102,102,102 (0.0) | 114,114,114 (11.1) | 94,94,94 (8.6) |
| 75 % white ×2 | 191,191,191 | 198,198,198 (7.0) | 191,191,191 (0.1) | 198,198,198 (7.3) | 187,187,187 (3.8) |
| 75 % yellow | 191,191,0 | 199,198,2 (5.7) | 191,191,1 (0.5) | 198,198,1 (5.2) | 187,187,1 (2.9) |
| 75 % cyan | 0,191,190 | 0,198,197 (4.6) | 2,191,190 (0.8) | 2,198,197 (5.5) | 1,188,186 (3.0) |
| 75 % green | 0,191,0 | 0,198,1 (2.8) | 2,191,1 (1.0) | 2,198,1 (3.6) | 2,187,1 (2.1) |
| 75 % magenta | 191,0,192 | 199,0,200 (5.5) | 191,1,192 (0.5) | 199,1,200 (5.3) | 187,1,188 (2.8) |
| 75 % red | 191,0,1 | 199,0,0 (2.8) | 191,1,0 (0.4) | 199,1,0 (2.9) | 188,1,0 (1.7) |
| 75 % blue | 0,0,191 | 0,0,199 (2.6) | 0,0,191 (0.1) | 0,0,199 (2.5) | 0,0,188 (1.3) |
| 100 % cyan | 0,254,255 | 3,255,255 (1.2) | 3,255,255 (1.2) | 3,255,255 (1.2) | 3,255,255 (1.2) |
| −I | 0,58,107 | 0,66,118 (6.3) | 1,58,107 (0.2) | 0,66,118 (6.6) | 0,48,99 (6.5) |
| 100 % blue | 1,0,255 | 1,0,255 (0.1) | 1,0,255 (0.2) | 0,0,255 (0.3) | 0,0,255 (0.4) |
| 100 % yellow | 254,255,0 | 255,255,0 (0.3) | 254,255,2 (0.6) | 254,255,2 (0.7) | 254,255,2 (0.7) |
| +Q | 67,13,123 | 76,11,134 (7.4) | 67,14,123 (0.1) | 76,10,134 (7.7) | 56,3,116 (9.3) |
| 100 % red | 255,1,0 | 255,0,0 (0.2) | 255,1,1 (0.5) | 255,1,1 (0.4) | 255,1,1 (0.3) |
| 15 % grey ×2 | 38,38,38 | 43,43,43 (4.6) | 38,38,38 (0.0) | 43,43,43 (4.8) | 26,27,26 (12.2) |
| 0 % black ×6 | 0,0,0 | 0,0,0 (0.0) | 0,0,0 (0.1) | 0,0,0 (0.1) | 0,0,0 (0.1) |
| 100 % white | 255,255,255 | 255,255,255 (0.0) | 255,255,255 (0.1) | 255,255,255 (0.1) | 255,255,255 (0.3) |
| +2 % (PLUGE) | 5,5,5 | 4,4,4 (0.7) | 5,5,5 (0.0) | 1,1,1 (3.4) | 0,0,0 (4.4) |
| +4 % (PLUGE) | 10,10,10 | 8,8,8 (2.5) | 10,10,10 (0.0) | 6,6,6 (4.2) | 2,2,1 (8.9) |
| **Mean of 27 patches** | | **3.29** | **0.27** | **3.59** | **3.55** |
| Worst patch | | 11.5 | 1.2 | 11.1 | 12.2 |

In mean ΔE2000 the four columns are 1.16, 0.04, 1.19 and 1.11.

**What it measures.** The theoretical values are the video's values, expanded
to full range and handed to an sRGB display with no curve of their own —
which is precisely what mpv does, so "As in mpv" scores 0.27 by construction,
not by being more careful than anyone. QuickTime scores 3.29 for the reason
the rest of this chapter is about: its 1.96 curve lifts every 75 % channel by
7–8 levels and the 40 % grey by 12, while the 0 and 100 % patches, where the
curves meet, stay within 1.2. "As in macOS" lands next to QuickTime (3.59)
because it draws what QuickTime draws. A player that reproduces QuickTime will
therefore always score about 3.3–3.6 in this test, and one that passes the
values through will always score about zero: **the number says which BT.709
convention a player follows, not how accurately it follows its own.** It
cannot see a wrong hue that happens to sit at 0 or 100 % either, and 7 of the
27 patches are black or full white.

IINA is the only column below the reference in the shadows — the 15 % grey
at 26 against 38, the +4 % PLUGE bar at 2 against 10 — which is its dark state
(see above). Which state the thread's author caught cannot be told from here.

**Colour bars against the grey ladder.** The hypothesis was that the bars show
much less of the difference between the two pipelines than the mid-tones do.
Measured directly, QuickTime against "As in mpv", in the same level metric:

| | Mean | Mid-tones (Y 40–160) | Worst |
|---|---|---|---|
| Colour bars, 27 patches | 3.42 | — | 11.56 |
| Grey ladder, 16 steps | 5.07 | 8.28 | 11.33 |

Confirmed: the bars average less than half the mid-tone gap. The breakdown
says why — the 11 patches where every channel is 0 or 100 % differ by 0.27,
the 14 at 75 % and the other part-levels by 4.73, and only the two 40 % grey
patches are mid-tones, at 11.56, the same as the worst step of the ladder.
The bars are mostly made of the places where the two curves meet. The ladder
against the same kind of reference ((Y − 16) / 219 × 255): QuickTime 4.70
(7.42 in the mid-tones), "As in mpv" 1.00, "As in macOS" 5.04, IINA on the
untagged file 1.38 — mpv's one level being the ladder file's own, which is
CRF 4 and decodes up to 1.5 levels off its nominal steps. QuickTime against
"As in macOS" is 0.48 on the bars and 0.56 on the ladder, worst 2.7 and 2.5.

**The window capture against Digital Color Meter.** Both read on the same
QuickTime frame of the bars, cursor hidden:

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

## Range before curve: what "180 and 16" rewards

The thread's table takes as its reference the file's code values as they are
— 180 for a 75 % channel and 16 for an empty one — and players that show
the empty channel at 0 score badly against it. But 16 and 180 are *limited
range* code values, and a limited-range video is shown by expanding 16–235 to
0–255:

```
(180 − 16) × 255 / 219 = 190.96  → 191
( 16 − 16) × 255 / 219 = 0
```

A player that shows 191 and 0 is decoding the file correctly; one that shows
180 and 16 is treating a limited-range video as full range. So the hypothesis:
**against the code values, the ranking sorts players by whether they expand
the range, and the winner is the one that does not.**

Tested with two files identical in everything but that one flag — the clean
`bars.mp4` above (`tv`), and a copy whose SPS and container both say `pc`:

```
ffmpeg -i bars.mp4 -c copy -bsf:v h264_metadata=video_full_range_flag=1 \
  -color_range pc bars-pc.mp4
```

`ffprobe` reports `tv` and `pc` for the two, at the stream and the frame
level, and the decoded planes hash identically. (A third copy with no tags at
all, and a lossy High-profile pair, came out the same way — see below.) The
six 75 % colour bars — yellow, cyan, green, magenta, red, blue — read as
before, with "dark" the empty channels (one or two per bar) and "high" the full ones, and
the deviation computed against both references:

| `tv` file | Dark channel | High channel | vs code values (180 / 16) | vs expanded (191 / 0) |
|---|---|---|---|---|
| QuickTime | 0.3 (0–2) | 198.6 (197–200) | 17.02 | 3.99 |
| "As in mpv" | 0.9 (0–2) | 190.8 (190–192) | 12.85 | **0.54** |
| "As in macOS" | 1.1 (0–2) | 198.4 (197–200) | 16.59 | 4.18 |
| IINA 1.4.4 | 0.9 (0–2) | 187.3 (186–188) | 11.13 | 2.30 |

| `pc` file, same pixels | Dark channel | High channel | vs code values (180 / 16) | vs expanded (191 / 0) | vs a correct full-range read |
|---|---|---|---|---|---|
| QuickTime | 12.1 (10–14) | 189.7 (188–192) | 6.69 | 6.74 | 5.02 |
| "As in mpv" | 13.7 (11–16) | 181.8 (180–184) | **1.94** | 11.42 | **0.40** |
| "As in macOS" | 10.8 (7–14) | 190.1 (188–192) | 7.52 | 6.01 | 5.85 |
| IINA 1.4.4 | 3.6 (2–4) | 177.7 (176–180) | 7.33 | 8.41 | 7.33 |

**Confirmed, and by a wide margin.** The same player on the same pixels
scores 12.85 against the code values when it expands the range correctly and
1.94 when it has been told not to — six and a half times "better" for the
wrong decode. Against the correctly expanded values it is the other way round,
0.54 against 11.42. Every player measured here expands a `tv` file (dark
channel 0–2), so in a code-value ranking they all sit behind any player that
does not, whatever their curve; the ranking has measured the range before it
has measured anything about colour. The curve question of the previous
section only appears once the reference is the expanded one — and there it
is the same 3–4 levels as before.

**Our player handles the flag correctly.** On the `tv` file the dark channel
is 0–2; on the `pc` file it is 11–16, and the whole reading matches a correct
full-range decode of the same Y'CbCr to 0.40. "16" is not quite the right
expectation there: read as full range, the empty channels of these bars land
between 12 and 16, because their chroma was computed for limited range. mpv
reports `video-params/colorlevels` as `limited` and `full` for the two, and
the result is the same with VideoToolbox (it decoded both the lossless 4:4:4
file and the High-profile pair) — no difference between the hardware and the
software path. The SDR colour mode does not touch range at all.

**A file with no range tag is limited, in both mpv and QuickTime.** mpv
reports `video-dec-params/colorlevels` as `auto` and `video-params/colorlevels`
as `limited` for the untagged copy, and drew it exactly as the `tv` one;
QuickTime's captures of the untagged and the `tv` file are byte-identical.
IINA's captures of the two are byte-identical as well: none of the four
measured here guesses otherwise.

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

"As in mpv" is what mpv draws on macOS (IINA, in one of its two states,
measured identical),
and a player that silently changed it would look wrong to the people who chose
it for mpv's rendering. "As in macOS" is what the same file looks like in
QuickTime — and, by the report that started this, in other AVFoundation
players — which is what most people compare against. Both are defensible, the
difference is visible, and the viewer is the one who knows which of the two
they expect — so it is theirs to choose. It is macOS only because the
difference is between mpv and AVFoundation; on Windows there is no system
player whose curve anybody expects.
