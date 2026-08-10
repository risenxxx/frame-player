/**
 * What is on top of the video, and what Escape takes away next.
 *
 * Four surfaces with one rule between them: at most one OSC menu, and a stack
 * that unwinds innermost first. It is a module rather than four `let`s in the
 * page because everything that *reacts* to an overlay needs to ask the same
 * question — the chrome must not fade out from under one, the skip offer must
 * not appear behind one, a click on the video closes one instead of pausing —
 * and each of those was reaching for a different variable.
 *
 * The link box is deliberately not here: it belongs to the flow that opens
 * things, and `closeTopmost` reaches for it rather than owning it. Which is the
 * whole distinction — this module knows the *order* of the surfaces, not what
 * any of them is for.
 */

import { catalog, closeCatalog, closeTitle } from './catalog.svelte';
import { chrome, exitFullscreen } from './chrome.svelte';
import { opening } from './open.svelte';
import { loadChapters, loadTracks } from './player.svelte';
import { ensureQueueTitles, loadPlaylist } from './playlist.svelte';

export type OscMenu = 'audio' | 'sub' | 'chapter' | 'queue' | 'cast';

class Overlays {
  /// Which OSC menu is open, one at a time — they all hang off the same bar and
  /// two of them at once would overlap.
  menu = $state<OscMenu | null>(null);
  /// Where the right-click landed, or null when no menu is up. Only the point:
  /// the menu places itself from its own measured size.
  ctxAt = $state<{ x: number; y: number } | null>(null);
  settings = $state(false);
  info = $state(false);
  /// Watching together: getting into a room, and the room you are in. A sheet
  /// like the settings rather than an OSC menu, because it is read rather than
  /// pointed at — and because it holds a text field.
  room = $state(false);
  /// The third-party notices. Opened from the settings footer, so it is a layer
  /// *above* the sheet rather than a replacement for it — Escape has to give the
  /// settings back rather than close everything.
  licenses = $state(false);

  /// The two track menus share their whole body (list + delay stepper), which
  /// the chapter menu does not — and every call in there takes 'audio' | 'sub',
  /// so the narrowing has to survive into the markup.
  trackMenu = $derived(this.menu === 'audio' || this.menu === 'sub' ? this.menu : null);

  /// Something is over the video, so the chrome must not fade out from under it.
  any = $derived(
    this.menu !== null || this.settings || this.licenses || this.room || catalog.open,
  );
}

export const overlays = new Overlays();

/**
 * Keep the window shell told. Two different questions and the narrower one is
 * not a detail: `overlayOpen` holds the chrome up, while `sheetOpen` also hides
 * the macOS traffic lights — which is right for a dialog that dims the window
 * and wrong for a menu hanging off the OSC.
 */
export function initOverlays() {
  $effect(() => {
    chrome.overlayOpen = overlays.any;
    chrome.sheetOpen =
      overlays.settings || overlays.licenses || overlays.room || catalog.open;
  });
}

/**
 * Open one OSC menu, or close it if it is the one already open.
 *
 * Each panel pulls what it needs on the way in rather than keeping a list warm:
 * the queue's titles are read from file headers and the chapter list is a long
 * chain of sub-property reads, neither of which is worth doing while the panel
 * is shut. Casting is the exception — the picker starts its own mDNS browse in
 * an effect, so its cleanup covers every way the panel closes (Escape, a click
 * outside, a session starting) and not only this one.
 */
export function toggleMenu(kind: OscMenu) {
  overlays.menu = overlays.menu === kind ? null : kind;
  if (overlays.menu === 'chapter') void loadChapters();
  else if (overlays.menu === 'queue') void openQueueMenu();
  else if (overlays.menu && overlays.menu !== 'cast') void loadTracks();
}

async function openQueueMenu() {
  // Awaited, unlike the fire-and-forget it used to be: the title read that
  // follows works off the entries this produces, and firing it against the
  // previous list would leave the panel on the 1.2 s timer — which is the whole
  // thing this call exists to skip.
  await loadPlaylist();
  ensureQueueTitles();
}

export function toggleInfo(hasFile: boolean) {
  if (!hasFile) return;
  overlays.info = !overlays.info;
}

/**
 * Escape unwinds one layer of whatever is open, innermost first, and only leaves
 * fullscreen once nothing else is left to close.
 *
 * Not a bindable action: it is about the surface stack rather than about
 * playback, which is also why `reservedReason()` refuses to hand the key to
 * anything else.
 */
export function closeTopmost() {
  if (opening.linkOpen) {
    opening.linkOpen = false;
    return;
  }
  // Two layers inside one sheet, unwound innermost first exactly as the stack
  // rule says: a title's page gives the grid back, and only the grid closes the
  // panel. Escape collapsing the whole catalog from a release list would throw
  // away a search as well as a choice.
  if (catalog.open) {
    if (catalog.picked) closeTitle();
    else closeCatalog();
    return;
  }
  if (overlays.info) {
    overlays.info = false;
    return;
  }
  if (overlays.room) {
    overlays.room = false;
    return;
  }
  if (overlays.licenses) {
    overlays.licenses = false;
    return;
  }
  if (overlays.settings) {
    overlays.settings = false;
    return;
  }
  if (overlays.ctxAt) {
    overlays.ctxAt = null;
    return;
  }
  if (overlays.menu) {
    overlays.menu = null;
    return;
  }
  void exitFullscreen();
}

/**
 * A pointerdown landed outside whatever is open. Returns true when it closed
 * something, which is what tells the caller to swallow the click that follows —
 * a click that dismisses a menu must not also toggle pause.
 */
export function dismissOnOutsideClick(target: HTMLElement | null): boolean {
  let closed = false;
  if (overlays.ctxAt && !target?.closest('.ctxmenu')) {
    overlays.ctxAt = null;
    closed = true;
  }
  if (overlays.menu && !target?.closest('.menu') && !target?.closest('.menu-toggle')) {
    overlays.menu = null;
    closed = true;
  }
  return closed;
}

/**
 * A click on the video itself. Same verdict, without the "outside" test: the
 * video is outside everything.
 */
export function dismissTopmostOnClick(): boolean {
  if (overlays.ctxAt) {
    overlays.ctxAt = null;
    return true;
  }
  if (overlays.menu) {
    overlays.menu = null;
    return true;
  }
  return false;
}
