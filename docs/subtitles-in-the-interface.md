# Subtitles drawn by the interface

In 1.47 the second subtitle was drawn by the web view rather than by mpv; it
has since moved to mpv's OSD (`sublines.lua`, see "The second line on mpv's
OSD" below). This note answers the question that followed the first move: should the main subtitles move there too, how would a saved frame
keep them, what would it cost, and does it make dragging subtitles — as on
YouTube, with a grab cursor over the line — easier. Nothing here is built; it
is the investigation, so the next one does not have to measure it again.

**Short answer.** Not all of them, and not now. ASS and image subtitles cannot
leave mpv, so "all" is not on the table; moving plain-text tracks buys free 2D
dragging and selectable text, and costs WebKit's ~200 MB repaint overhead for
part of the film, frame export by composition, and subtitles that lag a live
resize. A vertical drag can be had with mpv still drawing the line — and for the
second line, drawn on mpv's OSD since, a free one.

## What was measured

All on mpv 0.41 (Homebrew build, `--no-config`) and the player's dev build on
macOS 26, 2026-11-16.

**A frame saved with subtitles needs mpv to be drawing them.** With
`sub-visibility=no` and `secondary-sub-visibility=no`, `screenshot-to-file …
subtitles` wrote a frame with no subtitle pixels at all; with both `yes`, both
lines. So the moment a line is drawn by the web view, mpv's screenshot no longer
contains it — which is already true of the second subtitle.

**The text reaches the web view within the frame it starts on.** A 24 fps clip
with a cue every 0.4 s on both slots, played for 30 s; on every change of
`sub-text` / `secondary-sub-text` in the page, mpv's `time-pos` was read and
half the read's round trip subtracted. 73 main and 72 second-line changes:
−5 to +32 ms after the cue's start, median 15 ms, repeating with a period of
five cues — which is the cue start against the 41.7 ms frame grid, not jitter.
WebKit's paint adds about one display refresh on top. Not noticeable; the
remaining risk is an event-queue overflow, after which a line stays up until the
1 s resync (`RESYNC` sweeps `secondary-sub-text` for that reason).

**The look can be matched.** The same cue on both slots, mpv's line under ours:
726 and 727 device pixels wide, 119 high each, with CSS `font-size` =
0.85 × libass size and `line-height` = the size (`EM_PER_LIBASS_SIZE`).
macOS only, mpv's default `sans-serif`, which both resolve to Helvetica; a
Windows font stack has not been compared.

**mpv's subtitle renderer cannot give the second line a margin** (its OSD
can — see below). At `secondary-sub-pos` 0 libass
pins the block 7 px under the frame's top edge; any other value hangs it from a
fixed bottom (`sd_ass.c`: line position = `100 − pos`), so at 12 one, two and
three lines sat 51, 14 and 8 px from the top, and `sub-margin-y` moved nothing.
This is why the second line moved to the web view in the first place.

**What mpv offers for placing a text subtitle**: `sub-pos` (vertical, 0–150),
`sub-align-x` (left / center / right) with `sub-margin-x`. There is no free
horizontal position short of rewriting the event with `\pos`.

## Why "all subtitles" is not an option

- **ASS is typesetting, not text**: `\pos`, `\move`, `\fad`, karaoke, `\clip`,
  vector drawings, rotations, and fonts attached to the MKV. An anime release
  depends on all of it. The web view could only match it by running libass
  compiled to WebAssembly over a canvas, which is a second renderer, a font
  extraction step and a CPU cost, for no gain over mpv.
- **Image subtitles** (PGS, VobSub, DVB) are bitmaps; there is no text to draw.
- Closed captions arrive as text but are positioned on a 15×32 grid
  (`tracks-and-subtitles.md`), which `sub-text` does not carry.

So mpv keeps drawing subtitles either way, and the real question is whether
plain-text tracks (SubRip, WebVTT, `mov_text`) move.

## What moving plain-text tracks would cost

In order of weight:

1. **WebKit's GPU process holds ~200 MB while anything on the page repaints**
   (measured 2026-10-02, independent of window size, gone ~4 s after repaints
   stop). The guess here was that a cue every few seconds would hold it for most
   of a film; measured for the second line (below), a line changing every 2.5 s
   held it about a quarter of the time, in stretches, not throughout. Still a
   cost, and a canvas instead of DOM text would not help — it is still a
   repaint.
2. **Frame export has to compose** — see below.
3. **Live resize**: the web content is a frame behind the window and pinned to
   its top-left corner, and everything drawn over the picture is hidden while
   the frame changes (`chrome.unsteady`, `.afloat`). mpv's subtitles move with
   the picture; ours would vanish or trail for the length of the resize.
4. **Geometry mpv does for free**: letterbox margins (`sub-use-margins`), crop,
   rotation, the mini player. The web view would place the line from
   `osd-dimensions/*`.
5. **Every subtitle option has to be mirrored by hand**. The second line copies
   font, size, scale, bold, outline, box, shadow offset and the three colours;
   `sub-blur`, `sub-spacing`, `sub-justify`, `sub-shadow`… from a viewer's
   mpv.conf would be silently ignored until they are added.
6. **HDR**: a white line in the web view is SDR white over an EDR picture and
   may read dimmer than mpv's own; not measured.
7. **A lost property event** leaves a line up until the 1 s sweep.

What it would buy: free 2D dragging, text that can be selected and copied, a
click on a word to look it up (the language-learning case the second subtitle
was asked for), and the browser's bidi and fallback handling.

## Keeping subtitles in a saved frame

- **Compose in the web view (the workable one).** mpv writes the clean frame
  (`screenshot-to-file … video`); the page loads it, draws the visible lines on
  a canvas at the frame's own resolution — `lineStyle` already scales mpv's
  720-high units to any height, and the position is the one the viewer sees,
  dragged or not — and writes the PNG. Costs: a 16-bit PNG (mpv's default for a
  10-bit source) comes back as 8-bit, and a 4K encode in the page takes about a
  second; the sticky "saving" popup already covers a wait. This would also put
  the second line back into saved frames today.
- **Let mpv draw for the duration of the screenshot.** Rejected: setting the
  visibility back waits for the encode, so for up to a second the line is on
  screen twice.
- **Draw the text in Rust** (`image` + a glyph rasteriser) over mpv's PNG:
  keeps 16 bits, but means a text engine of our own, fonts and fallback
  included.

## Dragging

- **With mpv drawing** it is still possible, vertically. An invisible hit box
  over the line, sized from `sub-text` with the calibrated `lineStyle`, gives
  the grab cursor and the drag; the drag writes `sub-pos`, which is already the
  measured lever (`sub-shift.svelte.ts`). Only for plain-text tracks, where the
  box can be computed — an ASS line's metrics are its own. Frame export, memory
  and resize stay as they are.
- **With the web view drawing** it is trivial and two-dimensional: the line is
  an element, `cursor: grab` and pointer events.
- **A trap common to both**: a pointer moving over the subtitles raises the
  control bar, and the bar lifts the subtitles (`subShift.lift`) out from under
  the pointer. The lift has to hold still while the pointer is over the line
  and during a drag, and the dragged position has to become the viewer's own
  `sub-pos` base rather than something the lift adds to.

## The second line on mpv's OSD

Built after this note, and the answer to the second line's memory and resize
costs without giving up its margin. mpv's subtitle renderer still selects and
times the track (`secondary-sub-visibility=no`, `secondary-sub-text`), and
`src-tauri/lua/sublines.lua` draws the text as an `ass-events` OSD overlay,
`{\an8\pos(x,y)}` — anchored at its top, so the margin is ours. The player
sends only where (`script-message-to sublines place <shown> <top> <x>`).

**The look is mpv's OSD style, copied from the subtitle style.** An external
overlay's events use the "OSD" style built from the `osd-*` options
(`osd_libass.c`), and the box style has no override tag, so the script mirrors
`sub-*` onto `osd-*` — free, since the player draws its own OSD. Two things do
not carry over by copying: `osd-scale` reaches only mpv's own OSD text, never an
overlay, so `sub-scale` is multiplied into the font size, outline, shadow and
spacing; and `sub-scale-with-window=no` / `sub-scale-by-window=no` change a text
subtitle's scale (`sd_ass.c`, `configure_ass`), which the script repeats from
`osd-dimensions`.

Measured on Homebrew mpv 0.41 with the same cue on both slots, main line at
the bottom and the overlay at the top, glyph bounding boxes in mpv's window
screenshot:

| Options | Main line | Second line |
|---|---|---|
| defaults, 1280×720 | 339 × 28 | 339 × 28 |
| `sub-scale` 1.4, size 44, bold, `opaque-box`, 1600×700 (pillarboxed) | 564 × 37 | 564 × 38 |
| `sub-scale-with-window=no`, outline 4, shadow 3, 1000×900 (letterboxed) | 261 × 18 | 261 × 18 |
| `sub-scale-by-window=no`, `sub-scale` 1.2, 1000×900 | 402 × 29 | 402 × 28 |

A red outline 4 px wide with a shadow came out identical by eye as well. The
top edge: one, two and three lines all started at the same row (41, 42, 42 px
at a requested 36 px box top; the difference is the font's own ascent gap),
where `secondary-sub-pos` had given 51, 14 and 8. `{…}` in an SRT is removed by
mpv before `secondary-sub-text` (the main line loses it too), and a backslash is
escaped by the script and drawn as itself.

**In the app** (dev build, macOS 26, a 640-point-high window, `sub-margin-y` 34
from the viewer's mpv.conf): the glyphs' top at 73 device pixels with the bars
hidden and 117 with them up — 13 px under the requested 60 and 104 in both
cases, i.e. `sub-margin-y` and the title bar's 48 px + 4, as `lineTop` says.

**Memory**, WebKit's GPU process (`footprint`, every 5 s), a 4-minute clip with
a cue every 2.5 s on both slots, the bars hidden, the same driver for both
builds, two runs each; 18 samples over 90 s with the second line on:

| Build | Run 1 | Run 2 |
|---|---|---|
| line in the web view (1.47) | 17 MB, ~220 MB in 5 of 18 | ~218 MB for the first 18 s (4 of 18), then 17 MB |
| line on mpv's OSD | 16 MB in 18 of 18 | 16 MB in 18 of 18 |

So the web view's line did not hold the ~200 MB throughout, as this note had
guessed — what makes WebKit keep it some of the time and not the rest was not
found — but the OSD line never costs it. With the second line hidden both
builds sat at 16–17 MB.

What else changed: the line moves with the picture during a live resize (it is
mpv's drawing), and it jumps rather than glides when the bars come and go —
mpv has no animation for an overlay, the main line's lift jumps for the same
reason. Frame export still carries only the main line: an overlay is not in
`screenshot-to-file … subtitles`.

**Dragging it was built next**, two-dimensional without the web view drawing
the line, since `\pos` takes an x as well. The script measures what it drew
(the overlay's `compute_bounds`) and publishes it as `user-data/sublines/second`;
the page lays an invisible handle over that box only while the bars are up.
Driven in the app with pointer events on the handle (dev build, 1137×640 CSS
px): a move of (−200, +150) px moved the centre by exactly −200/1137 and the
box by 150 px; a drag ending 8 px off the middle landed on x = 0.5 with the
guide up; a drag into the lower half switched the line to its bottom edge, and
with the bars up it stood on the control bar. The rules it keeps are in the
second-subtitle line of `docs/rules/tracks-and-subtitles.md`.

## Recommendation

1. Vertical drag over mpv's own drawing — small, none of the costs above, and it
   covers the usual reason to move subtitles (burnt-in text, a face). **Built**
   for plain-text tracks: `sublines.lua` measures where libass put the line, a
   handle is laid over it, and the drag writes `sub-pos`; the lift became a
   ceiling on the way (see the lift line in `docs/rules/tracks-and-subtitles.md`).
   The second line, on mpv's OSD, is dragged in both directions the same way.
2. Canvas composition for frame export, so the second line is in saved frames.
3. Plain-text tracks in the web view only if free 2D dragging or click-to-look-up
   is wanted, and only after measuring WebKit's memory on a real film.
