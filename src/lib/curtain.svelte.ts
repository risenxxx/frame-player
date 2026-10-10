/**
 * The dark between two pictures.
 *
 * A file change used to be three things in a row, all of them on screen: the
 * previous film's frame, then the next film's first frame standing in the
 * previous one's window with bars beside it, then the window moving to the new
 * shape. The middle one is nobody's picture, and it is the one the eye catches.
 * So the change goes through black instead: the old frame goes dark *before*
 * mpv is handed the new file, and the dark comes off over exactly the time the
 * window takes to get to its new shape (`MORPH_MS`, from the window's side).
 *
 * What goes dark is the picture and nothing over it: the bars, the loading
 * plate and the dialogs stay lit. **How** it goes dark depends on the platform,
 * and the reason is a measurement. A fill in the web view is late — the web
 * content is a frame or two behind the window's frame — so while the window
 * grows on its way to the new shape there is a strip along the growing edge
 * the fill has not reached, and the picture shows through it at full
 * brightness. On macOS the video view's own opacity is faded instead
 * (`window_video_fade`): that view resizes in the same turn as the window.
 * Elsewhere, and wherever that view cannot be found, the fill is what there is
 * (`Curtain.svelte`, which draws only while `drawn` says so).
 *
 * **It must not be able to stay down.** What it covers is a film that is
 * playing, and every signal that lifts it arrives from somewhere that can lose
 * one: a property event dropped by a full queue, a file that never produces a
 * picture, a load that fails on its way. So it has four ways back, and only the
 * first is the intended one:
 *
 *  - the picture has its window, or is on its way there (`curtainSettled`);
 *  - the file is open and no picture came of it (`NO_PICTURE_MS` — an audio
 *    file, or a decoder that has nothing to show yet);
 *  - nothing was heard at all (`CAP_MS`), unless the page says a load is still
 *    under way, in which case there is nothing under it worth showing;
 *  - the start screen (`liftCurtain`, from the page).
 */

import { invoke } from '@tauri-apps/api/core';

import { IS_MAC } from './platform';
import { player } from './player.svelte';

/// Down. Short enough that opening a file does not feel held back — this much
/// is added to every open — and long enough to read as a fade rather than a
/// cut.
export const RAISE_MS = 80;

/// Up, where there is no window morph to take the time from.
export const LIFT_MS = 200;

/// One frame, give or take: the transition starts when the class has been
/// painted once, not when it was written.
const FRAME_MS = 20;

/// A file that is open and has shown no picture by now is not going to be
/// waited for in the dark.
export const NO_PICTURE_MS = 700;

/// The last way back. Longer than any local file takes to show its first
/// frame, shorter than anybody waits before deciding the player has hung.
export const CAP_MS = 2500;

class Curtain {
  /// Down over the picture.
  on = $state(false);
  /// How long the change under way takes.
  ms = $state(0);
  /// The web view draws it — everywhere but macOS, and on macOS for a dark the
  /// native fade could not make.
  drawn = $state(!IS_MAC);
  /// Came down over no picture — opening from the start screen — and nothing
  /// has been put up over it since. The window has no shape for what is coming
  /// and will move to one the moment it is known, so anything floating that
  /// appeared now would be on screen for a frame or two and then taken away
  /// for the move (`chrome.unsteady`). It waits for the window instead: the
  /// control bar and the "resuming" note came up, went at once and faded back
  /// in, which read as a blink on every open from "continue watching".
  bare = $state(false);
}

export const curtain = new Curtain();

let undrawTimer: ReturnType<typeof setTimeout> | undefined;

/**
 * Put what `on` and `ms` say on the screen, where that is not the component's
 * to do.
 *
 * The native fade fails where there is no video view to fade, which is not a
 * fault: mpv makes that view when it starts, and the first thing this does is
 * run before that. So a failure decides nothing beyond the dark it failed to
 * make — it used to decide the rest of the session, and the very first call,
 * made at launch to put a reloaded page's picture back, turned the native fade
 * off before it had been tried once.
 */
function present() {
  if (!IS_MAC) return;
  clearTimeout(undrawTimer);
  if (curtain.drawn) {
    // The web view made this dark and it is the one to lift it; the next one
    // is asked of the video view again.
    if (!curtain.on) {
      undrawTimer = setTimeout(() => {
        if (!curtain.on) curtain.drawn = false;
      }, curtain.ms + FRAME_MS);
    }
    return;
  }
  const dark = curtain.on;
  void invoke('window_video_fade', { to: dark ? 0 : 1, ms: curtain.ms }).catch(() => {
    if (dark && curtain.on) curtain.drawn = true;
  });
}

/// The file the curtain came down for is open. Until it is, a picture that
/// reports itself settled is the *previous* one, and lifting for it would show
/// exactly the frames this exists to cover.
let armed = false;

let capTimer: ReturnType<typeof setTimeout> | undefined;
let pictureTimer: ReturnType<typeof setTimeout> | undefined;

/// Whether the page has a load under way — the loading plate is up. A hook
/// because that flag is the page's, set in `beforeLoad` and cleared on
/// `file-loaded`.
let loading: () => boolean = () => false;

export function initCurtain(page: { loading: () => boolean }) {
  loading = page.loading;
  // The picture's opacity is the window's and outlives the page: a reload with
  // the dark down would come back to a film nobody can see.
  present();
}

function pictureShowing(): boolean {
  return player.hasFile && player.sourceTransfer !== null && player.videoW > 0 && player.videoH > 0;
}

function armCap() {
  clearTimeout(capTimer);
  capTimer = setTimeout(() => {
    if (loading()) armCap();
    else liftCurtain();
  }, CAP_MS);
}

function lower() {
  const showing = pictureShowing();
  curtain.ms = showing ? RAISE_MS : 0;
  curtain.on = true;
  // Not bare when the loading plate is already standing: a torrent opened from
  // the catalog or the history raises it for the magnet resolve, long before
  // there is a file to come down for, so nothing would say "covered" again —
  // and the bars, the traffic lights and the plate itself (`.afloat`) stayed
  // away until the swarm produced a first frame. A black window, for as long
  // as the slowest wait in the player takes.
  curtain.bare = !showing && !loading();
  present();
  return showing;
}

/**
 * Bring the dark down ahead of a file change, and answer once it is down.
 *
 * Awaited by whoever is about to hand mpv the next file: a fade that is still
 * on its way when the new frame arrives covers nothing. With no picture on
 * screen there is nothing to fade from and it is down at once.
 */
export function raiseCurtain(): Promise<void> {
  armed = false;
  clearTimeout(pictureTimer);
  armCap();
  if (curtain.on) return Promise.resolve();
  if (!lower()) return Promise.resolve();
  return new Promise((done) => setTimeout(done, RAISE_MS + FRAME_MS));
}

/**
 * A file has started that nobody raised the curtain for.
 *
 * The net under `raiseCurtain`: a change that reaches mpv by a way that does
 * not pass through one of our verbs still gets the dark, only later — mpv is
 * already opening the file, so the fade runs against the first frame rather
 * than ahead of it. Never touches `armed`: the order of this event and
 * `file-loaded` is mpv's, and re-arming here could undo the one that matters.
 */
export function curtainFileStarted() {
  if (curtain.on) return;
  armed = false;
  lower();
  armCap();
}

/// The file is open. From here a settled picture is the new one.
export function curtainFileLoaded() {
  if (!curtain.on) return;
  armed = true;
  clearTimeout(pictureTimer);
  pictureTimer = setTimeout(() => liftCurtain(), NO_PICTURE_MS);
}

/// Something stands over the dark — the loading plate, for a source that takes
/// its time. Waiting for the window is then no longer worth more than saying
/// what is happening, and what comes up next comes up beside it.
export function curtainCovered() {
  curtain.bare = false;
}

/// The picture is in its window, or will be in `ms` — the window's own report.
export function curtainSettled(ms: number) {
  if (!curtain.on || !armed) return;
  liftCurtain(Math.max(ms, LIFT_MS));
}

export function liftCurtain(ms = LIFT_MS) {
  clearTimeout(capTimer);
  clearTimeout(pictureTimer);
  armed = false;
  curtain.bare = false;
  if (!curtain.on) return;
  curtain.ms = ms;
  curtain.on = false;
  present();
}
