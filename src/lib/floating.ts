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
