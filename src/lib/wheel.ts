/**
 * What a wheel event over the video asks for: volume, a seek, or nothing yet.
 *
 * Pure — no DOM, no timers, no mpv — because the rules are the kind that break
 * quietly. The gesture's channel is chosen once and held, or diagonal finger
 * motion flips it from event to event and seeking bleeds into volume; Shift
 * pressed or released part-way is the one thing that re-decides it; and Shift
 * only counts when the event brings no horizontal delta of its own.
 * `input.svelte.ts` owns the timer that ends a gesture and the verbs the
 * result is handed to.
 */

/// What the vertical wheel does. Shift+wheel does the other one.
export type WheelMode = 'volume' | 'seek';

/// `scrub` is a real horizontal delta — a tilt wheel, a trackpad swipe — at the
/// continuous rate. `notch` is a vertical wheel asked to seek (in seek mode, or
/// with Shift in volume mode), at the viewer's seconds per notch.
export type WheelChannel = 'volume' | 'scrub' | 'notch';

export type WheelResult = { volume: number } | { seek: number } | null;

export interface WheelInput {
  deltaX: number;
  deltaY: number;
  /// deltaMode: 0 — pixels, 1 — lines, 2 — pages.
  deltaMode: number;
  shiftKey: boolean;
}

// Volume accumulates by delta rather than a fixed step per event: a trackpad
// fires dozens of tiny events per gesture, and a step for each drove the
// volume to the ceiling.
// The quantum is small (20 px = 1 unit) — otherwise the trackpad stays silent
// until a whole step accumulates, which feels like lag. A mouse wheel notch
// in Chromium is 100 px, so the familiar 5 units per notch is preserved.
export const PX_PER_VOLUME_UNIT = 20;
// Seeking by horizontal gesture: 20 px = 1 s, so a wheel notch tilted
// sideways (100 px) gives the same 5 s as an arrow key.
export const PX_PER_SEEK_SECOND = 20;
/// One notch of a mouse wheel, which the seconds-per-notch setting is counted in.
export const NOTCH_PX = 100;

/// Shift held over a vertical wheel: a mouse has one wheel, so this is its
/// horizontal scroll — and it does not arrive as one. Both engines report it
/// as `deltaY` with `shiftKey` set and do the remap further down their own
/// scrolling code, which never runs over the video, because nothing there
/// scrolls. So the remap is ours. Only when the event brings no horizontal
/// delta of its own, since a tilt wheel, a Windows precision touchpad's
/// two-finger swipe and a mac trackpad are all already right, and Shift is
/// routinely held while scrolling one.
function shiftedOf(e: WheelInput) {
  return e.shiftKey && Math.abs(e.deltaY) > Math.abs(e.deltaX);
}

/**
 * Which channel an event opens. `null` while the motion is too small to tell:
 * the first events of a gesture are almost always noisy and can point the
 * wrong way.
 */
export function wheelChannel(e: WheelInput, mode: WheelMode): WheelChannel | null {
  if (shiftedOf(e)) return mode === 'seek' ? 'volume' : 'notch';
  const ax = Math.abs(e.deltaX);
  const ay = Math.abs(e.deltaY);
  if (Math.max(ax, ay) < 2) return null;
  if (ax > ay) return 'scrub';
  return mode === 'seek' ? 'notch' : 'volume';
}

/**
 * One gesture's state: the locked channel and what has been collected towards
 * the next whole unit. `reset()` is the end of a gesture — the timer in
 * `input`, or the fingers leaving a trackpad.
 */
export class WheelGesture {
  channel: WheelChannel | null = null;
  /// Whether the lock came from Shift rather than from the event's own deltas,
  /// so that letting Shift go can unlock it again.
  shifted = false;
  private accum = 0;

  reset() {
    this.channel = null;
    this.shifted = false;
    this.accum = 0;
  }

  /**
   * `mac` because macOS has already applied "natural scrolling" to the event.
   * `notchSeconds` is how far one notch seeks. `canSeek` is false with nothing
   * to seek in — the gesture still locks, so it does not turn into volume.
   */
  feed(e: WheelInput, mode: WheelMode, notchSeconds: number, mac: boolean, canSeek: boolean): WheelResult {
    const scale = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? 400 : 1;
    const shifted = shiftedOf(e);
    // Shift going down — or coming back up — part-way through a gesture is an
    // explicit ask rather than the diagonal noise the lock exists for, so it
    // re-decides the channel instead of waiting the gesture out. The
    // accumulator goes with it, or the fraction of a second already collected
    // lands as volume, and the other way round.
    if (shifted !== this.shifted) {
      this.shifted = shifted;
      this.channel = null;
      this.accum = 0;
    }
    if (this.channel === null) {
      this.channel = wheelChannel(e, mode);
      if (this.channel === null) return null;
    }

    if (this.channel === 'scrub') {
      if (!canSeek) return null;
      // A *real* horizontal delta is inverted on macOS for the same reason as
      // volume: the system already applied "natural scrolling", so a rightward
      // gesture arrives as deltaX < 0 while it feels like "forward".
      this.accum += (mac ? -e.deltaX : e.deltaX) * scale;
      const secs = Math.trunc(this.accum / PX_PER_SEEK_SECOND);
      if (!secs) return null;
      this.accum -= secs * PX_PER_SEEK_SECOND;
      return { seek: secs };
    }

    const py = e.deltaY * scale;
    if (this.channel === 'notch') {
      if (!canSeek) return null;
      // Two directions, chosen on purpose. Shift+wheel is a horizontal scroll,
      // so it goes the browsers' way — down becomes right, hence forward — and
      // `deltaY > 0` means "down" on both platforms whatever "natural
      // scrolling" says, so it needs no platform branch. A plain vertical wheel
      // asked to seek goes the way it goes for volume — up is more — which is
      // also mpv's own binding (WHEEL_UP is a seek forward).
      this.accum += shifted ? py : mac ? py : -py;
      const pxPerSecond = NOTCH_PX / notchSeconds;
      const secs = Math.trunc(this.accum / pxPerSecond);
      if (!secs) return null;
      this.accum -= secs * pxPerSecond;
      return { seek: secs };
    }

    // macOS already applied the "natural scrolling" setting to the event, so an
    // upward gesture there is deltaY > 0 — the opposite sign to a Windows wheel.
    this.accum += mac ? py : -py;
    const steps = Math.trunc(this.accum / PX_PER_VOLUME_UNIT);
    if (!steps) return null;
    this.accum -= steps * PX_PER_VOLUME_UNIT;
    return { volume: steps };
  }
}
