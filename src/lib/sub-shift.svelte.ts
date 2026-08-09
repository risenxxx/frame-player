/**
 * Lift the subtitles clear of the control bar while it is on screen.
 *
 * The bar is drawn *over* the video and subtitles sit at the bottom of it, so
 * for the second or two the chrome is up the last line of dialogue is behind
 * the seekbar — exactly while the viewer is reading it, since what raised the
 * bar was their own hand on the mouse.
 *
 * The lever is `sub-pos`, and it is the only one this player has measured: mpv's
 * `sub-ass-override` defaults to `scale`, which is what makes `sub-pos` reach
 * ASS subtitles rather than only plain text ones (the same fact the settings
 * slider depends on). `sub-margin-y` would be additive and therefore need no
 * base at all, but nothing here has established that it survives ASS override,
 * and being wrong about it is silent — the subtitles simply do not move.
 *
 * What the lift has to be is the bar's height *minus where the subtitle already
 * sits*: mpv holds it `sub-margin-y` above the bottom edge, which is 33 px in a
 * 1080p window and scales with it. Adding a clearance to a clearance is what
 * left a whole scrim's height of empty picture between the text and the
 * controls, so that margin is read from mpv and subtracted (see the constant).
 *
 * Two things follow from `sub-pos` being the viewer's own setting as well as
 * ours. The base is **re-read from mpv on every apply** rather than remembered
 * across a session: the settings slider writes `sub-pos` directly and live, so
 * whatever it last wrote is the truth. And this module **stands down while a
 * dialog is up** (`chrome.sheetOpen`), which is what guarantees the slider can
 * only ever be dragged against an unshifted value — otherwise the viewer would
 * be dialling in a position measured from ours, and closing the sheet would
 * shift it again on top of that.
 *
 * The move is instant while the bar fades over 0.25 s. mpv has no animation for
 * a property, and driving one from here would be ~15 writes on its core thread
 * per fade; going first reads as the subtitles getting out of the way, which is
 * what they are doing.
 */

import { command, getProperty } from 'tauri-plugin-libmpv-api';

import { chrome } from './chrome.svelte';
import { player } from './player.svelte';

/// mpv's own default, and the value assumed when it declines to answer.
const DEFAULT_SUB_POS = 100;

/// Clearance between the subtitles and the top of the bar, in CSS pixels.
///
/// About the text not touching the controls, not about clearing the scrim. The
/// bar's gradient is drawn in the webview and therefore lies *over* mpv's
/// picture, reaching 44 px above the controls — so at this height the subtitle
/// carries roughly 0.18 of black over it, which reads as a slight dimming
/// against its own outline rather than as something covering it. Clearing the
/// scrim as well would mean giving up 44 px more, and that is the gap this
/// number was corrected down from.
const SUB_GAP = 8;

/// mpv's own bottom margin for subtitles, and the height it is expressed
/// against. `sub-margin-y` is in "scaled pixels" — 1/720 of the window — so the
/// subtitle already rests 33 px above the bottom edge of a 1080p window and
/// 66 px of a 4K one.
///
/// Subtracting it is what makes the lift land where it says it does: without it
/// the two clearances add, which put the subtitles a whole scrim's height above
/// the bar. It has to be read rather than assumed to be 22, because it is an
/// option the viewer's own mpv.conf may set — and it cannot be a constant in
/// pixels at all, since it scales with the window.
///
/// One thing it is only an estimate of: an ASS subtitle takes its margin from
/// its own style rather than from this option, and a script that sets one far
/// from mpv's default will sit correspondingly off. Both are in the same 10–30
/// range at the same 720 reference, which is why the estimate is worth making.
const DEFAULT_SUB_MARGIN_Y = 22;
const MARGIN_REFERENCE_H = 720;

/// Smallest change worth a property write. A window resize drags the measured
/// percentage around continuously, and each write re-renders the subtitle on
/// mpv's core thread.
const MIN_STEP = 0.2;

class SubShift {
  /// The control bar. Bound by the page; null whenever it is not rendered at
  /// all, which is the start screen.
  oscEl = $state<HTMLElement | null>(null);

  /// The bar's height and the window's, in CSS pixels. Both are state so the
  /// lift recomputes when either moves — `sub-pos` is a percentage of the
  /// window, so the ratio is what matters and the device scale cancels out.
  barPx = $state(0);
  windowPx = $state(0);

  /// mpv's `sub-margin-y`, read once the player is up. Until then its default,
  /// which is also what a refusal leaves in place.
  marginY = $state(DEFAULT_SUB_MARGIN_Y);

  /// How far the subtitles are lifted, in percent of window height. Zero means
  /// mpv holds the viewer's own value and nothing of ours is in force.
  lift = $derived.by(() => {
    if (!this.oscEl || !this.barPx || !this.windowPx) return 0;
    // The bar is faded out and no longer covers anything.
    if (chrome.idle) return 0;
    // A dialog dims the window and owns the screen; the bar behind it is not
    // covering anything the viewer is reading, and standing down here is what
    // keeps the settings slider writing against an unshifted `sub-pos`.
    if (chrome.sheetOpen || chrome.fileDialogOpen) return 0;
    // What the subtitle has to clear is the bar *beyond* where it already sits.
    const margin = (this.marginY * this.windowPx) / MARGIN_REFERENCE_H;
    const cover = this.barPx + SUB_GAP - margin;
    if (cover <= 0) return 0;
    return (cover / this.windowPx) * 100;
  });
}

export const subShift = new SubShift();

/// The viewer's own `sub-pos` while ours is in force, and null the rest of the
/// time — which is also the flag for "nothing to restore".
let base: number | null = null;
/// The lift last handed to `setLift`, whether or not its write has landed yet.
let applied = 0;
/// Writes are serialized: restoring the base and reading it back are the same
/// property, so a restore still in flight would otherwise be read as the base
/// by the apply that follows it, and the shift would compound.
let chain: Promise<unknown> = Promise.resolve();

function setLift(lift: number) {
  if (Math.abs(lift - applied) < MIN_STEP) return;
  applied = lift;
  chain = chain.then(() => write(lift)).catch(() => {});
}

async function write(lift: number) {
  if (lift === 0) {
    const previous = base;
    base = null;
    if (previous !== null) await command('set', ['sub-pos', String(previous)]);
    return;
  }
  if (base === null) {
    const value = await getProperty('sub-pos', 'double').catch(() => null);
    base = typeof value === 'number' ? value : DEFAULT_SUB_POS;
  }
  // mpv's own range. A viewer who has pushed the subtitles down past the frame
  // still gets them lifted, and a small window cannot drive the value negative.
  const pos = Math.min(150, Math.max(0, base - lift));
  await command('set', ['sub-pos', String(pos)]);
}

function measureEffect() {
  $effect(() => {
    const el = subShift.oscEl;
    const measure = () => {
      // `offsetHeight`, not the bounding rect: the bar is translated 10 px down
      // while it is hidden, and its border-box height is the distance from the
      // bottom of the window to its top edge either way.
      subShift.barPx = el ? el.offsetHeight : 0;
      subShift.windowPx = window.innerHeight;
    };
    measure();
    window.addEventListener('resize', measure);
    // The bar's height changes with the mode rather than with the window — the
    // mini player gives it less padding — so it is observed rather than assumed.
    const observer = new ResizeObserver(measure);
    if (el) observer.observe(el);
    return () => {
      window.removeEventListener('resize', measure);
      observer.disconnect();
    };
  });
}

function marginEffect() {
  // Read as soon as mpv is up rather than lazily beside the base: the margin
  // decides the *size* of the lift, so discovering it on the first apply would
  // make the very first appearance of the bar a two-step move — a jump to the
  // uncorrected height and a settle a round trip later.
  $effect(() => {
    if (!player.ready) return;
    void getProperty('sub-margin-y', 'int64')
      .then((v) => {
        if (typeof v === 'number' && v >= 0) subShift.marginY = v;
      })
      .catch(() => {});
  });
}

function applyEffect() {
  // A block body: an arrow returning a value hands `$effect` something it would
  // treat as a cleanup function.
  $effect(() => {
    setLift(subShift.lift);
  });
}

/**
 * Start the two standing effects. **Must be called from a component's
 * initialization** — see the note on `initChrome` for what a `$effect` at a
 * module's top level costs.
 */
export function initSubShift() {
  measureEffect();
  marginEffect();
  applyEffect();
}
