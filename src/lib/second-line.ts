/**
 * Where the second subtitle sits. sublines.lua draws it; this decides where.
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
 * =no`), keeps selecting and timing it, and sublines.lua draws
 * `secondary-sub-text` as an OSD overlay anchored at one edge (`\an8` / `\an2`
 * + `\pos`), in the subtitles' own look — measured against the main line with
 * the same cue: the same size to the pixel under scale, bold, box and outline
 * styles, and the anchored edge where it was put for one, two and three lines.
 *
 * It was drawn by the web view in 1.47, which matched the look as well but kept
 * WebKit repainting at every cue (docs/subtitles-in-the-interface.md).
 *
 * The viewer can drag it anywhere (`SecondSubLine.svelte`). A place is kept as
 * fractions of the window, so it survives a resize and fullscreen, and as an
 * edge: a line put in the lower half hangs from its bottom, so a cue with an
 * extra line grows away from the frame's edge instead of off it.
 *
 * Pure: the arithmetic is tested, and the component only applies it.
 */

/// The height mpv's sizes are expressed against.
const REFERENCE_H = 720;

/// Between a bar and the line while the bar is up, in CSS pixels.
const BAR_GAP = 4;

/// How close to the middle, in CSS pixels, a dragged line's centre has to come
/// to be taken there exactly. Centred is where a subtitle almost always
/// belongs, and a hand on a mouse does not land on one pixel.
export const SNAP_PX = 12;

export type Edge = 'top' | 'bottom';

/// Where the line goes, as fractions of the window: `x` is its centre, `y` is
/// its `edge`.
export interface LinePlace {
  x: number;
  y: number;
  edge: Edge;
}

/// Where the line was drawn — the bitmap's bounds, outline and shadow
/// included — as fractions of the window.
export interface Box {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

/// Before the viewer has moved it: centred, at mpv's own subtitle margin
/// (`sub-margin-y`, in 720-high units) from the top.
export function defaultPlace(marginY: number): LinePlace {
  return { x: 0.5, y: marginY / REFERENCE_H, edge: 'top' };
}

/// The place in force: the viewer's, kept clear of a bar while it is up — the
/// title bar for a line hanging from the top, the control bar for one standing
/// on the bottom (only where the viewer lets subtitles get out of its way:
/// `avoidControls`, the main line's own setting). The mirror of the main
/// line's lift.
export function placeInForce(
  place: LinePlace,
  windowH: number,
  topBarPx: number,
  controlsPx: number,
  chromeUp: boolean,
  avoidControls: boolean,
): LinePlace {
  if (!chromeUp || windowH <= 0) return place;
  if (place.edge === 'top') {
    return { ...place, y: Math.max(place.y, (topBarPx + BAR_GAP) / windowH) };
  }
  if (!avoidControls || controlsPx <= 0) return place;
  return { ...place, y: Math.min(place.y, 1 - (controlsPx + BAR_GAP) / windowH) };
}

/// `user-data/sublines/second`, or null while nothing is drawn. mpv keeps
/// `user-data` as a node, and a node read as a string comes formatted as one —
/// the text arrives in quotes (seen in the app).
export function parseBox(value: string): Box | null {
  const parts = value.trim().replace(/^"|"$/g, '').trim().split(/\s+/).map(Number);
  if (parts.length !== 4 || parts.some((n) => !Number.isFinite(n))) return null;
  const [x0, y0, x1, y1] = parts;
  if (x1 <= x0 || y1 <= y0) return null;
  return { x0, y0, x1, y1 };
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/**
 * Where a drag puts the line. `start` is the place in force when the drag began
 * and `box` where the line was drawn then; `dx`/`dy` are how far the pointer
 * has moved, in CSS pixels of a `windowW`×`windowH` window.
 *
 * The box moves with the pointer, stays inside the window, and its centre
 * snaps to the middle within `SNAP_PX`. The edge follows the box's centre:
 * the upper half hangs from the top, the lower stands on the bottom.
 *
 * The bounds are not the anchor: libass draws the outline outside the line and
 * the top of a line box above its glyphs, so the box starts a little under a
 * top anchor and ends a little under a bottom one (measured: 0.007 and 0.010 of
 * the window). That offset is taken from `start` and kept, so grabbing the line
 * and letting go of it does not move it; on a change of edge the same offset is
 * assumed for the other one, which is right within a pixel or two.
 */
export function dragPlace(
  start: LinePlace,
  box: Box,
  dx: number,
  dy: number,
  windowW: number,
  windowH: number,
): { place: LinePlace; snapped: boolean } {
  const w = box.x1 - box.x0;
  const h = box.y1 - box.y0;
  const offset = (start.edge === 'top' ? box.y0 : box.y1) - start.y;

  let x = start.x + (windowW > 0 ? dx / windowW : 0);
  const snapped = Math.abs(x - 0.5) * windowW <= SNAP_PX;
  if (snapped) x = 0.5;
  x = clamp(x, Math.min(w / 2, 0.5), Math.max(1 - w / 2, 0.5));

  const top = clamp(box.y0 + (windowH > 0 ? dy / windowH : 0), 0, Math.max(0, 1 - h));
  const place: LinePlace =
    top + h / 2 > 0.5 ? { x, y: top + h - offset, edge: 'bottom' } : { x, y: top - offset, edge: 'top' };
  return { place, snapped };
}

/// A place read back from storage, or null when it is not one.
export function validPlace(value: unknown): LinePlace | null {
  if (typeof value !== 'object' || value === null) return null;
  const { x, y, edge } = value as Record<string, unknown>;
  if (typeof x !== 'number' || typeof y !== 'number') return null;
  if (!(x >= 0 && x <= 1 && y >= 0 && y <= 1)) return null;
  if (edge !== 'top' && edge !== 'bottom') return null;
  return { x, y, edge };
}

/// The `place` message for sublines.lua. Rounded so a resize that leaves the
/// line where it was sends nothing new.
export function placeArgs(shown: boolean, place: LinePlace): string[] {
  return ['sublines', 'place', shown ? '1' : '0', place.x.toFixed(4), place.y.toFixed(4), place.edge];
}
