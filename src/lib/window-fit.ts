/**
 * The arithmetic behind "match video aspect ratio": which shape the picture is
 * drawn in, the smallest window of that shape, and the size a fit comes to.
 *
 * Pure, and kept apart from `window-prefs.svelte.ts` for that reason: every
 * case worth getting right here only shows at a screen edge, at the minimum
 * size or on a clip filmed upright — none of which a quick look at the player
 * with an ordinary film on a large monitor will ever reach.
 *
 * One unit throughout a call. The callers work in physical pixels; nothing
 * here knows or needs to.
 */

export interface Size {
  w: number;
  h: number;
}

/**
 * The picture as it is *drawn*: `dwidth`×`dheight`, turned when the VO turns it.
 *
 * mpv reports the display size before rotation (measured on 0.41, `vo=gpu`): a
 * phone clip carrying a 90° display matrix reads 1280×720 with
 * `video-out-params/rotate` at 270, and is drawn 720×1280. Taking the size
 * alone gave every upright clip a landscape window with the picture standing in
 * the middle of it.
 */
export function pictureShape(w: number, h: number, rotate: number): Size | null {
  if (!(w > 0) || !(h > 0)) return null;
  return Math.abs(rotate) % 180 === 90 ? { w: h, h: w } : { w, h };
}

/** A shape as a key: two files of one shape are the same window. */
export function shapeKey(shape: Size): string {
  return `${shape.w}x${shape.h}`;
}

/**
 * The smallest size of this shape that still covers both minimums.
 *
 * The window's minimum is a width and a height that know nothing of each other
 * (480×320), so clamping a fitted size to them one axis at a time bends the
 * shape exactly where there is no room left to see it — and a system resize
 * constraint does worse: on macOS the ratio outranks the minimum, and a 16:9
 * window was dragged down to 480×270 (measured). The floor of a shape is
 * therefore a size *of that shape*.
 */
export function floorForShape(shape: Size, min: Size): Size {
  const ratio = shape.w / shape.h;
  const w = Math.ceil(Math.max(min.w, min.h * ratio));
  return { w, h: Math.max(Math.ceil(min.h), Math.round(w / ratio)) };
}

/**
 * Whether the shape can be had on this screen at all.
 *
 * It cannot always: an upright 9:16 clip needs 480×853 before the controls fit,
 * and a 13-inch laptop has less than that between the menu bar and the Dock.
 * `room` is null when the screen is unknown, which is not a reason to refuse.
 */
export function shapeFits(floor: Size, room: Size | null): boolean {
  return !room || (floor.w <= room.w && floor.h <= room.h);
}

export interface FitInput {
  shape: Size;
  /** How much picture the window should hold, as an area. */
  area: number;
  /** The window's own minimum, per axis. */
  min: Size;
  /** The largest content size the screen leaves, or null when unknown. */
  room: Size | null;
}

export interface Fit extends Size {
  /** The screen took some of the size away; the old position means nothing. */
  shrunk: boolean;
  /** The result has the picture's shape. False only where no size could. */
  held: boolean;
}

/**
 * The content size for a picture of `shape` given `area` to fill.
 *
 * Where the shape cannot be had (`shapeFits`), the answer is the old one —
 * shrunk into the screen, then clamped per axis — and says so with `held`:
 * bars beside a picture are better than a window that does not fit its screen.
 */
export function fitWindow({ shape, area, min, room }: FitInput): Fit {
  const ratio = shape.w / shape.h;
  let w = Math.sqrt(Math.max(area, 1) * ratio);
  let h = w / ratio;

  let shrunk = false;
  if (room) {
    const k = Math.min(1, room.w / w, room.h / h);
    shrunk = k < 1;
    w *= k;
    h *= k;
  }

  const floor = floorForShape(shape, min);
  if (!shapeFits(floor, room)) {
    return {
      w: Math.max(Math.ceil(min.w), Math.round(w)),
      h: Math.max(Math.ceil(min.h), Math.round(h)),
      shrunk,
      held: false,
    };
  }
  if (w < floor.w || h < floor.h) return { ...floor, shrunk, held: true };
  return { w: Math.round(w), h: Math.round(h), shrunk, held: true };
}

export interface Rect extends Size {
  x: number;
  y: number;
}

/**
 * Where a window of `size` goes when it takes the place of `from`: around the
 * same center, and inside the screen.
 *
 * The center is what the eye is on — the picture is in the middle of the
 * window — and a window resized from its top-left corner, which is what a bare
 * `setSize` does, throws the picture sideways by half of whatever the width
 * changed by: from a 2:1 film to a square clip, 200 px on an ordinary window.
 *
 * `area` is the screen's work area and `pad` the margin kept from its edges;
 * each axis is settled on its own, so a window the screen had to shrink in
 * height still keeps its center across. Where the size is more than the
 * margins leave, it is centered on the work area and overhangs evenly.
 */
export function placeAround(from: Rect, size: Size, area: Rect | null, pad: number): Rect {
  let x = Math.round(from.x + (from.w - size.w) / 2);
  let y = Math.round(from.y + (from.h - size.h) / 2);
  if (area) {
    x = keepInside(x, size.w, area.x, area.w, pad);
    y = keepInside(y, size.h, area.y, area.h, pad);
  }
  return { x, y, w: size.w, h: size.h };
}

function keepInside(at: number, length: number, start: number, extent: number, pad: number): number {
  const first = start + pad;
  const last = start + extent - pad - length;
  if (last < first) return start + Math.round((extent - length) / 2);
  return Math.min(Math.max(at, first), last);
}

// ---- What a pinch does -----------------------------------------------------

/**
 * What a trackpad pinch does — the viewer's setting, not the answer for any
 * one moment (that is `pinchResizes`).
 *
 * `resize` is the default: with the window in the picture's shape the window
 * *is* the picture, and "bigger" by pinching loses nothing of the frame, where
 * a zoom crops it. It is also what is asked for many times in a session, while
 * a look into one corner of the picture is rare — and that one keeps ⌥+pinch
 * and Ctrl+wheel whatever the setting says.
 */
export type PinchAction = 'resize' | 'zoom';
export const PINCH_CHOICES: PinchAction[] = ['resize', 'zoom'];

export interface PinchMoment {
  setting: PinchAction;
  /**
   * Fullscreen: the window cannot grow. Deliberately not "or maximized" — on
   * macOS that is not a mode but a frame (`isZoomed`: the window happens to
   * have the zoomed size), which a pinch reaches by itself and may leave
   * again; read as a mode, the pinch that took the window to the limit was
   * the last one that resized it.
   */
  fullscreen: boolean;
  /** The start screen is up, and the window is nobody's. */
  resting: boolean;
  /** The mini player: a size the viewer asked for, held to a corner. */
  mini: boolean;
  /**
   * Something fills the window that a resize around the center would draw
   * twice — a dialog with its backdrop, the casting screen (the web content is
   * pinned to the window's top-left corner and a frame or two behind it; what
   * floats over the picture is taken away for a resize, but these cannot be).
   */
  covered: boolean;
}

/**
 * Whether a pinch, right now, is the window's size rather than the picture's
 * zoom. The native side asks nothing: it is told this once per change and reads
 * ⌥ off the event itself.
 *
 * Every `false` here is a `true` for the zoom the gesture used to be, which is
 * the right answer wherever the window cannot grow: fullscreen, the mini
 * player. On the start screen and under a dialog the zoom has nothing to
 * act on either, and the gesture does nothing — better than a resize that
 * doubles what is on the screen.
 */
export function pinchResizes(now: PinchMoment): boolean {
  return now.setting === 'resize' && !now.fullscreen && !now.resting && !now.mini && !now.covered;
}
