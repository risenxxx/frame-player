/**
 * Where the main subtitle line is and how far `sub-pos` moves it.
 *
 * mpv draws the main line itself — frame export keeps it, and ASS and image
 * subtitles have nothing else to be drawn by — and `sub-pos` is the one lever
 * that moves it (sub-shift.svelte.ts). For a plain-text track libass puts the
 * bottom of the line box at
 *
 *     frame top + (frame height − margin) · sub-pos / 100
 *
 * (ass_render.c, a bottom-aligned event with a line position), the margin
 * being `sub-margin-y` in 720-high units, scaled like the font. Measured on
 * 0.41 in the window as the frame (`sub-use-margins`, mpv's default): exact to
 * 2 px at margins 0, 10, 30, 34, 40 and 60, in a 720- and a 900-high window and
 * at `sub-scale` 1.5; the bottom does not depend on the number of lines, and
 * past the top the block is stopped at the frame's top edge. Above 100 the line
 * goes under the bottom edge, so a drag stops at 100.
 *
 * Pure: the arithmetic is tested.
 */

import type { Box } from './second-line';

/// mpv 0.41's own `sub-margin-y` — 22 in older versions — and the value assumed
/// until mpv answers.
export const DEFAULT_SUB_MARGIN_Y = 34;

/// The height mpv's sizes are expressed against.
const REFERENCE_H = 720;

/// What sublines.lua reports for the main line: where it is drawn, the
/// `sub-pos` it was measured at, and the share of the window's height that 100
/// units of `sub-pos` span.
export interface MainLine {
  box: Box;
  pos: number;
  range: number;
}

/// `user-data/sublines/main`, or null — no plain-text line on screen. mpv
/// keeps `user-data` as a node, so the string arrives in quotes.
export function parseMainLine(value: string): MainLine | null {
  const parts = value.trim().replace(/^"|"$/g, '').trim().split(/\s+/).map(Number);
  if (parts.length !== 6 || parts.some((n) => !Number.isFinite(n))) return null;
  const [x0, y0, x1, y1, pos, range] = parts;
  if (x1 <= x0 || y1 <= y0 || range <= 0) return null;
  return { box: { x0, y0, x1, y1 }, pos, range };
}

/**
 * The highest `sub-pos` — the lowest line — whose line box still ends `gap`
 * pixels above a bar `barPx` high, in a window `windowPx` high; null when there
 * is no room at all. With the window as the frame, the line box ends at
 * (H − m) · pos / 100, so the bar's top is reached at 100 · (H − bar − gap) / (H − m).
 *
 * `marginY` is mpv's option, unscaled: with `sub-scale` above 1 the real margin
 * is larger and the line sits a little higher than this assumes, which errs on
 * the side of clearing the bar.
 */
export function liftCeiling(windowPx: number, barPx: number, marginY: number, gap: number): number | null {
  const range = windowPx - (marginY * windowPx) / REFERENCE_H;
  if (windowPx <= 0 || range <= 0) return null;
  const limit = windowPx - barPx - gap;
  return Math.min(100, Math.max(0, (limit / range) * 100));
}

/// Where a vertical drag of `dyPx` CSS pixels takes a line that was at
/// `startPos`, in a window `windowH` high whose `sub-pos` spans `range` of it.
/// Between the top and 100 — below that the line leaves the window — and to a
/// tenth, which is under a pixel at any window size.
export function dragSubPos(startPos: number, dyPx: number, windowH: number, range: number): number {
  if (windowH <= 0 || range <= 0) return startPos;
  const pos = startPos + (dyPx / (windowH * range)) * 100;
  return Math.round(Math.min(100, Math.max(0, pos)) * 10) / 10;
}
