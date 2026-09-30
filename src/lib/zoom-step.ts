/**
 * How far one Ctrl+wheel event zooms the picture, in the log2 units
 * `zoom.svelte.ts` keeps (0 is 100 %, 1 is 200 %).
 *
 * Two gestures arrive as the same event and want different answers. A wheel
 * notch is a click: a fixed step, 0.1 (about 7 %), and the wheel decides how
 * many. A trackpad pinch is a stream — every 8 ms at 120 Hz, dozens per gesture
 * — and the same step for each drove any pinch to the 800 % ceiling: measured,
 * a full spread is 30–60 events. What tells them apart is the size of the
 * delta: WebKit writes a pinch as `deltaY = −100 × magnification` (measured, a
 * real pinch stays under 15 per event), Chromium's synthesized pinch wheel is
 * in the same range, and a mouse notch is 100 or 120 pixels in Chromium and
 * a line or a page in the other modes. So a pinch zooms by what the fingers
 * did — `log2(1 + magnification)`, the same product the window would scale
 * by — and anything the size of a notch is a notch.
 *
 * Pure so that the line between the two can be pinned by a test; the sign is
 * the browser's (negative is "in") whichever gesture it was.
 */

/// One notch of a wheel, log2.
export const WHEEL_ZOOM_STEP = 0.1;

/// The smallest `deltaY` that is a notch rather than a pinch, in pixels.
const NOTCH_PX = 40;

export function zoomStep(deltaY: number, deltaMode: number): number {
  if (!Number.isFinite(deltaY) || deltaY === 0) return 0;
  if (deltaMode !== 0 || Math.abs(deltaY) >= NOTCH_PX) {
    return deltaY < 0 ? WHEEL_ZOOM_STEP : -WHEEL_ZOOM_STEP;
  }
  // |deltaY| < 40 here, so the magnification is inside (−0.4, 0.4) and the
  // logarithm is finite.
  return Math.log2(1 - deltaY / 100);
}
