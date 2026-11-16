# Subtitles drawn by the interface

Since 1.47 the second subtitle is drawn by the web view rather than by mpv
(`SecondSubLine.svelte`, `second-line.ts`). This note answers the question that
followed: should the main subtitles move there too, how would a saved frame
keep them, what would it cost, and does it make dragging subtitles — as on
YouTube, with a grab cursor over the line — easier. Nothing here is built; it
is the investigation, so the next one does not have to measure it again.

**Short answer.** Not all of them, and not now. ASS and image subtitles cannot
leave mpv, so "all" is not on the table; moving plain-text tracks buys free 2D
dragging and selectable text, and costs a WebKit memory overhead for most of a
film, frame export by composition, and subtitles that lag a live resize. A
vertical drag can be had with mpv still drawing the line.

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

**mpv cannot give the second line a margin.** At `secondary-sub-pos` 0 libass
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
   stop). A cue changes every few seconds, so this would be held for most of a
   film with subtitles. The second line already pays it while it is on. Measure
   on a real film before deciding; a canvas instead of DOM text would not help,
   it is still a repaint.
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

## Recommendation

1. Vertical drag over mpv's own drawing — small, none of the costs above, and it
   covers the usual reason to move subtitles (burnt-in text, a face).
2. Canvas composition for frame export, so the second line is in saved frames.
3. Plain-text tracks in the web view only if free 2D dragging or click-to-look-up
   is wanted, and only after measuring WebKit's memory on a real film.
