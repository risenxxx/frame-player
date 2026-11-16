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
 * The lift is a **ceiling**, not a distance: while the bar is up the subtitles
 * may sit no lower than where their line box ends `SUB_GAP` above it, and
 * anywhere higher they stay put (`liftCeiling` in main-line.ts, from libass's
 * own placement — measured exact to 2 px). It used to be a fixed shift worked
 * out for a line at the bottom, which also lifted a line the viewer had put in
 * the middle of the picture, where no bar was covering it. The margin mpv
 * already keeps (`sub-margin-y`, 34 on 0.41, scaled with the window) is read
 * from mpv rather than assumed: adding a clearance to a clearance is what once
 * left a whole scrim's height of empty picture between the text and the bar.
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

import { invoke } from '@tauri-apps/api/core';

import { chrome } from './chrome.svelte';
import { DEFAULT_SUB_MARGIN_Y, liftCeiling } from './main-line';
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

/// Smallest change worth a property write. A window resize drags the measured
/// percentage around continuously, and each write re-renders the subtitle on
/// mpv's core thread.
const MIN_STEP = 0.2;

const ENABLED_KEY = 'frameplayer.subLift';

function loadEnabled(): boolean {
  try {
    return localStorage.getItem(ENABLED_KEY) !== '0';
  } catch {
    return true;
  }
}

class SubShift {
  /// The viewer's switch (Settings → Subtitles). Off, the subtitles stay where
  /// `sub-pos` puts them and the bar covers them while it is up — which some
  /// viewers prefer to a line of text that moves every time the mouse does.
  enabled = $state(loadEnabled());

  /// The control bar. Bound by the page; null whenever it is not rendered at
  /// all, which is the start screen.
  oscEl = $state<HTMLElement | null>(null);

  /// The bar's height and the window's, in CSS pixels. Both are state so the
  /// lift recomputes when either moves — `sub-pos` is a percentage of the
  /// window, so the ratio is what matters and the device scale cancels out.
  barPx = $state(0);
  windowPx = $state(0);

  /// mpv's `sub-margin-y`, in 720-high units, read once the player is up.
  /// Until then its default, which is also what a refusal leaves in place.
  marginY = $state(DEFAULT_SUB_MARGIN_Y);

  /// While the pointer is over the main line or drags it (MainSubLine.svelte):
  /// the ceiling in force when it arrived. The pointer raises the bar, the bar
  /// would move the line out from under the pointer, and the line would then
  /// flicker between the two places.
  held = $state<{ ceiling: number | null } | null>(null);

  /// The lowest the subtitles may sit, as a `sub-pos`, or null when nothing of
  /// ours is in force and mpv holds the viewer's own value.
  ceiling = $derived.by((): number | null => {
    if (this.held) return this.held.ceiling;
    if (!this.enabled) return null;
    if (!this.oscEl || !this.barPx || !this.windowPx) return null;
    // The bar is faded out and no longer covers anything.
    if (chrome.idle) return null;
    // A dialog dims the window and owns the screen; the bar behind it is not
    // covering anything the viewer is reading, and standing down here is what
    // keeps the settings slider writing against an unshifted `sub-pos`.
    if (chrome.sheetOpen || chrome.fileDialogOpen) return null;
    return liftCeiling(this.windowPx, this.barPx, this.marginY, SUB_GAP);
  });
}

export const subShift = new SubShift();

export function setSubLift(on: boolean) {
  subShift.enabled = on;
  try {
    localStorage.setItem(ENABLED_KEY, on ? '1' : '0');
  } catch {
    // the choice simply will not survive a restart
  }
}

/// The viewer's own `sub-pos` while ours is in force, and null the rest of the
/// time — which is also the flag for "nothing to restore".
let base: number | null = null;
/// The ceiling last handed to `setCeiling`, whether or not its write has landed
/// yet; `undefined` forces the next one through.
let applied: number | null | undefined = null;
/// Writes are serialized: restoring the base and reading it back are the same
/// property, so a restore still in flight would otherwise be read as the base
/// by the apply that follows it, and the shift would compound.
let chain: Promise<unknown> = Promise.resolve();

function setCeiling(ceiling: number | null) {
  if (ceiling === applied) return;
  if (ceiling !== null && typeof applied === 'number' && Math.abs(ceiling - applied) < MIN_STEP) return;
  applied = ceiling;
  chain = chain.then(() => write(ceiling)).catch(() => {});
}

async function write(ceiling: number | null) {
  if (ceiling === null) {
    const previous = base;
    base = null;
    if (previous !== null) await command('set', ['sub-pos', String(previous)]);
    return;
  }
  if (base === null) {
    const value = await getProperty('sub-pos', 'double').catch(() => null);
    base = typeof value === 'number' ? value : DEFAULT_SUB_POS;
  }
  // A line the viewer put higher than the bar reaches stays where it is; one
  // pushed down past the frame comes up like any other.
  await command('set', ['sub-pos', String(Math.min(base, ceiling))]);
}

/// Hold the subtitles where they are while the pointer is over the main line,
/// and let the bar have its say again when it leaves.
export function holdSubtitles(on: boolean) {
  if (on) {
    subShift.held = { ceiling: subShift.ceiling };
    return;
  }
  subShift.held = null;
  // The ceiling may come back as the same number it was held at, which no
  // effect would see as a change — but the line may have been dragged under it.
  applied = undefined;
  setCeiling(subShift.ceiling);
}

/// The viewer moved the subtitles: `pos` is where they are now, and where they
/// stay once the bar is gone — the base the ceiling is applied to.
export function moveSubtitles(pos: number) {
  chain = chain
    .then(async () => {
      if (base !== null) base = pos;
      await command('set', ['sub-pos', String(pos)]);
    })
    .catch(() => {});
}

/// …and, once a drag is over, kept: in mpv.conf, where the settings slider
/// keeps the same value, so the slider shows it next time and it outlives the
/// session.
export function keepSubtitlesPos(pos: number) {
  void invoke('mpv_conf_set', { key: 'sub-pos', value: String(pos) }).catch((e) => {
    console.warn('mpv_conf_set failed:', e);
  });
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
  // decides the ceiling, so discovering it on the first apply would
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
    setCeiling(subShift.ceiling);
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
