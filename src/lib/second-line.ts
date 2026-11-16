/**
 * Where the second subtitle sits. secondsub.lua draws it; this decides where.
 *
 * mpv can draw a second subtitle (`secondary-sid`) only pinned to the top edge
 * of the frame: `secondary-sub-pos` becomes libass's line position, which lifts
 * a bottom-anchored line and stops it 7 px under the edge at 0, and any other
 * value hangs the line from a fixed *bottom*, so its distance from the top
 * changes with the number of lines and the size (measured on 0.41: 51, 14 and
 * 8 px for one, two and three lines at `secondary-sub-pos=12`). No option
 * reaches that margin — `sub-margin-y` included.
 *
 * So mpv's subtitle renderer is told not to draw it (`secondary-sub-visibility
 * =no`), keeps selecting and timing it, and secondsub.lua draws
 * `secondary-sub-text` as an OSD overlay anchored at its top edge (`\an8\pos`),
 * in the subtitles' own look — measured against the main line with the same
 * cue: the same size to the pixel under scale, bold, box and outline styles, and
 * the top edge where it was put for one, two and three lines.
 *
 * It was drawn by the web view in 1.47, which matched the look as well but kept
 * WebKit repainting at every cue, and that holds ~200 MB for as long as the line
 * is on (docs/subtitles-in-the-interface.md).
 *
 * Pure: the arithmetic is tested, and SecondSubLine.svelte only sends it.
 */

/// The height mpv's sizes are expressed against.
const REFERENCE_H = 720;

/// Where the line's top edge goes, in CSS pixels from the top of the window:
/// mpv's own subtitle margin (`sub-margin-y`, in the same 720-high units) when
/// nothing is over the picture, and under the title bar while it is up.
export function lineTop(marginY: number, windowH: number, barPx: number, chromeUp: boolean): number {
  const margin = (marginY * windowH) / REFERENCE_H;
  return chromeUp ? Math.max(margin, barPx + BAR_GAP) : margin;
}

/// Between the title bar and the line while the bar is up.
const BAR_GAP = 4;

/// The `place` message for secondsub.lua: shown, the top edge as a fraction of
/// the window's height, the centre as a fraction of its width. Rounded so a
/// resize that leaves the line where it was sends nothing new.
export function placeArgs(shown: boolean, topPx: number, windowH: number, centerX = 0.5): string[] {
  const top = windowH > 0 ? topPx / windowH : 0;
  return ['secondsub', 'place', shown ? '1' : '0', top.toFixed(4), centerX.toFixed(4)];
}
