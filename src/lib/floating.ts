/**
 * Placing a floating box inside the window.
 *
 * Every windowing toolkit solves this with the same three rules, applied in
 * order: **flip** to the other side of the anchor when the preferred side has
 * no room, **shift** along the other axis to stay inside, and **resize** when
 * the box is simply taller than the space left for it. Floating UI is that
 * plus scroll containers, virtual anchors and auto-update — none of which
 * exists here: the player has one non-scrolling viewport and every anchor is a
 * real element. So the rules are two functions rather than a dependency, and
 * being in one place is the actual win: tooltips, the context menu and its
 * submenu all placed themselves, which is why each of them was wrong in a
 * different way.
 *
 * Everything is in CSS pixels relative to the viewport.
 */

/// Margin kept between a floating box and the window edge.
const PAD = 8;

export interface Flipped {
  /// Where the box starts on this axis.
  pos: number;
  /// True when it ended up on the near side of the anchor (above / left of it).
  before: boolean;
  /// How much room the chosen side has — the box's cap when it does not fit.
  room: number;
}

/**
 * Pick a side of the anchor and place the box on it.
 *
 * `near`/`far` are the anchor's two edges on this axis (top/bottom, or
 * left/right). The preferred side is taken when the box fits there; otherwise
 * the other side if it fits; otherwise the roomier of the two, and `room` says
 * how much the caller may use.
 */
export function flipAxis(opts: {
  near: number;
  far: number;
  size: number;
  limit: number;
  gap?: number;
  pad?: number;
  preferBefore: boolean;
}): Flipped {
  const { near, far, size, limit, preferBefore } = opts;
  const gap = opts.gap ?? 0;
  const pad = opts.pad ?? PAD;

  const roomBefore = near - gap - pad;
  const roomAfter = limit - pad - (far + gap);
  const fitsBefore = size <= roomBefore;
  const fitsAfter = size <= roomAfter;

  // The preferred side when it fits, the other when only that one does.
  let before = preferBefore ? fitsBefore || !fitsAfter : !fitsAfter && fitsBefore;
  // Neither fits: take the roomier one rather than the preferred one, so the
  // box is cut down as little as possible.
  if (!fitsBefore && !fitsAfter) before = roomBefore > roomAfter;

  const room = before ? roomBefore : roomAfter;
  const pos = before ? Math.max(pad, near - gap - size) : far + gap;
  return { pos, before, room };
}

/**
 * Slide the box along an axis until it is inside, keeping it as close to where
 * it wanted to be as possible.
 *
 * A box wider than the window is left at the near edge: something has to be
 * cut off, and the start of a menu item is worth more than its end.
 */
export function shiftAxis(want: number, size: number, limit: number, pad = PAD): number {
  const max = limit - pad - size;
  if (max < pad) return pad;
  return Math.min(Math.max(want, pad), max);
}

/**
 * How wide the seekbar's hover preview may be — the third rule, resize, for the
 * one floating box whose shape is fixed by the video it shows.
 *
 * The preview hangs above the seekbar, so the space for it is what lies between
 * the bar and the top of the window, less the lines under the frame (the time,
 * the chapter) — and across, the bar itself, which is what the popup is clamped
 * to. At full size neither binds; in the mini player, which can be dragged down
 * to 240×135, both do, and a fixed 184px frame was cut off by the window's top
 * edge. The frame keeps its aspect and shrinks to whichever of the two runs out
 * first, and never grows past `max`.
 */
export function previewWidth(opts: {
  /// The largest the frame is ever drawn, the full-window size.
  max: number;
  /// The seekbar's width: the popup is clamped inside it.
  across: number;
  /// Room from the popup's bottom edge up to the window's top.
  above: number;
  /// What the popup stacks under the frame: gaps and text lines.
  below: number;
  /// Width over height of the video.
  aspect: number;
  pad?: number;
}): number {
  const { max, across, above, below, aspect } = opts;
  const pad = opts.pad ?? PAD;
  const byHeight = Math.max(0, above - pad - below) * aspect;
  return Math.max(0, Math.floor(Math.min(max, across, byHeight)));
}
