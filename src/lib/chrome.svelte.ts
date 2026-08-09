/**
 * The window shell: fullscreen and its veil, the custom title bar, the window
 * buttons, the resize edges, and the idle rules that decide when all of it
 * fades away.
 *
 * These are one concern rather than two, and trying to split them shows why:
 * the chrome's visibility *is* the idle state, `pokeUi` drives the macOS
 * traffic lights, and hiding the cursor has to trail the native button hide
 * rather than race it (see `CURSOR_HIDE_MS`).
 *
 * What this module deliberately does not know is which dialog is open. It takes
 * one input for that — `chrome.overlayOpen` — set by whoever owns the dialogs,
 * so the shell never has to import the settings sheet or the OSC menus.
 */

import { invoke } from '@tauri-apps/api/core';
import { emitTo } from '@tauri-apps/api/event';
import { currentMonitor, getCurrentWindow } from '@tauri-apps/api/window';
import { PhysicalPosition, PhysicalSize } from '@tauri-apps/api/dpi';
import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import { tick } from 'svelte';

import { playback } from './playback.svelte';
import { IS_MAC } from './platform';
import { flushPosition } from './history.svelte';
import { player } from './player.svelte';
import { seek } from './seek.svelte';
import { mini, toggleMini } from './window-prefs.svelte';

/// How long the chrome stays up after the pointer stops.
const UI_HIDE_MS = 1200;

// Title bar height, and the width of the macOS traffic lights within it.
//
// The cursor is kept visible over that corner and nowhere else. Hovering the
// system window buttons stops the webview from receiving mousemove — the cursor
// "sticks" there, idle sets in on the timer, and hiding it would be wrong,
// because the user is working with the native popup on the green button and we
// cannot see that they are.
//
// It used to be the whole 48px band across the full width, on both platforms,
// which is far more than the reason justifies: in a window the pointer lands in
// that band constantly, and there it looked as though the cursor simply never
// hid in windowed mode. Nothing on Windows needs the exception at all — the
// window buttons there are our own HTML.
const TITLEBAR_STRIP = 48;
const MAC_BUTTONS_WIDTH = 110;

class Chrome {
  fullscreen = $state(false);
  isMaximized = $state(false);
  /// The black shutter that masks a fullscreen transition.
  fsTransition = $state(false);

  /// The pointer has moved recently, so the bars are up.
  uiVisible = $state(true);
  oscHover = $state(false);
  barHover = $state(false);
  /// Parked on the title bar: like `oscHover`, keeps the UI from going idle.
  pointerInTitlebar = $state(false);
  cursorHidden = $state(false);

  /// Something is on top of the video — a dialog, a menu, the file picker — so
  /// the chrome must not fade out from under it. Set by the page.
  overlayOpen = $state(false);
  /// A *dimming* overlay specifically: one of the seven dialogs. Narrower than
  /// `overlayOpen` on purpose — an OSC menu keeps the chrome up without dimming
  /// the window, and only the dimming case has to hide the macOS traffic lights
  /// (see the window-buttons effect below).
  sheetOpen = $state(false);
  /// The OS file dialog, which dims the window the same way but is not ours to
  /// draw.
  fileDialogOpen = $state(false);

  /// The start screen is showing. Set by the page, which debounces it — a
  /// playlist transition blanks the filename for a moment and the picker screen
  /// must not flash — so it is deliberately not `!player.hasFile`. The shutter
  /// color is the one thing here that depends on it.
  startScreen = $state(true);

  /// Room for the centered title, measured from whichever side cluster is wider.
  barSide = $state(0);
  brandEl = $state<HTMLElement | null>(null);
  chromeEl = $state<HTMLElement | null>(null);

  idle = $derived(
    player.hasFile &&
      !seek.dragging &&
      !this.oscHover &&
      !this.barHover &&
      !this.uiVisible &&
      !this.overlayOpen &&
      // While casting the window is a remote control and a status display —
      // there is no picture being watched under the chrome, so hiding it buys
      // nothing and hiding the controls of a remote is actively wrong.
      !playback.session,
  );
}

export const chrome = new Chrome();

let hideTimer: ReturnType<typeof setTimeout> | undefined;
let fsTransitionTimer: ReturnType<typeof setTimeout> | undefined;


// macOS: the system window buttons live outside the DOM, so they have to be
// dimmed by a separate command in step with the title bar's CSS fade (0.25 s)
// — otherwise they hang around after the rest of the UI is gone. Shown
// immediately, hidden once the animation is over.
let winButtonsTimer: ReturnType<typeof setTimeout> | undefined;
let winButtonsShown = true;
// The system file dialog: while it is open, the window is dimmed.

// Well inside the CSS fade (0.25 s): the native buttons cannot be faded, they
// vanish in one step, so matching the *end* of the animation makes them a
// lagging tail. Even mid-fade read as late — a hard cut is noticed at once
// while a fade is still visibly on its way out, so it has to land early to
// feel simultaneous. This is a taste knob; lower it further if they still
// linger, but not to 0, or they lead instead.
const WIN_BUTTONS_HIDE_MS = 40;

/**
 * Run something behind the system file dialog.
 *
 * The dialog dims the window while it is up, and on macOS the traffic lights
 * have to be hidden outright — they are native views *above* the webview, so no
 * HTML can cover them and they stay bright on top of the dimming. Every picker
 * in the player needs that, so the flag is set here rather than handed to each
 * of them: it was a hook on `open.svelte.ts` first, and the moment the track
 * pickers needed it too that would have been two hooks for one flag.
 */
export async function withFileDialog<T>(fn: () => Promise<T>): Promise<T> {
  chrome.fileDialogOpen = true;
  try {
    return await fn();
  } finally {
    chrome.fileDialogOpen = false;
  }
}

export function syncWindowButtons(hidden: boolean, immediate = false) {
  if (!IS_MAC) return;
  clearTimeout(winButtonsTimer);
  const apply = (visible: boolean) => {
    if (winButtonsShown === visible) return;
    winButtonsShown = visible;
    void invoke('window_buttons', { visible }).catch(() => {});
  };
  if (!hidden) {
    apply(true);
  } else if (immediate) {
    apply(false);
  } else {
    winButtonsTimer = setTimeout(() => apply(false), WIN_BUTTONS_HIDE_MS);
  }
}

/// Hiding the cursor trails the native button hide instead of racing it.
///
/// `set_buttons_visible` hides the traffic lights with `setHidden`, which asks
/// AppKit for a title-bar relayout — and a relayout resets the window's cursor
/// rectangles, which puts the arrow back. Applied together with `idle`, our
/// `cursor: none` was therefore undone ~40 ms after it landed, and with the
/// pointer standing still nothing re-applied it: the cursor blinked once and
/// then simply stayed. It reads as a windowed-only bug because in fullscreen
/// `set_buttons_visible` declines to act at all, so there is no relayout to
/// undo it — but nothing here ever looked at fullscreen.
///
/// The margin covers the 40 ms timer, the command round-trip, the hop to the
/// main thread and the relayout itself, and still lands well inside the
/// 0.25 s fade of the bars.
const CURSOR_HIDE_MS = 200;
let cursorTimer: ReturnType<typeof setTimeout> | undefined;

function cursorEffect() {
  $effect(() => {
    clearTimeout(cursorTimer);
    if (!chrome.idle || chrome.pointerInTitlebar) {
      // Showing it again is never delayed: that half is a response to the
      // pointer moving, and any lag there is felt immediately.
      chrome.cursorHidden = false;
      return;
    }
    // Windows has no native chrome to relay out, so there is nothing to wait for.
    if (!IS_MAC) {
      chrome.cursorHidden = true;
      return;
    }
    cursorTimer = setTimeout(() => (chrome.cursorHidden = true), CURSOR_HIDE_MS);
  });
}

function windowButtonsEffect() {
  $effect(() => {
    // Overlays dim the window, but the traffic lights are native views ABOVE
    // the webview and no HTML can cover them: they stay bright on top of the
    // dimming and read as a z-index bug. So hide them explicitly, immediately.
    const overlay = chrome.sheetOpen || chrome.fileDialogOpen;
    // Fullscreen is deliberately *not* excluded here, though macOS owns the
    // chrome there. This call is the only record of whether the buttons are
    // meant to be on screen, and the native side replays that record after
    // rebuilding the title bar on the way out of fullscreen (`BUTTONS_VISIBLE`
    // in macos_chrome.rs). Excluding fullscreen left the record saying "visible"
    // the whole time, so leaving fullscreen while idle flashed the traffic
    // lights on and this effect hid them again a frame later.
    //
    // Sending it is safe because the native side keeps the two apart: it stores
    // the intent, then declines to *act* on a hide while fullscreen — otherwise
    // the idle timer emptied the strip the user had just pulled down.
    // Mini hides them outright: in a 420px window the traffic lights sit on top
    // of the picture, and the way out is the button drawn in the corner.
    syncWindowButtons(overlay || chrome.idle || mini.on, overlay || mini.on);
  });
}

// While the window is resizing, mpv's child window relayouts a frame or two
// behind its parent — the transparent area falls through to the desktop. The
// veil (above the whole UI, so control jumps are hidden too) masks the
// artifact; it is released adaptively, shortly after the last resize event,
// with the timeout only as a safety ceiling.
// Release order: the shutter hides first (the main window beneath it already
// wears the black veil), then the veil dissolves — the seam is invisible.
export function releaseVeil() {
  clearTimeout(fsTransitionTimer);
  void hideShutter();
  chrome.fsTransition = false;
}

/// Re-arm the release. The resize listener calls this on every event so the
/// veil comes down ~2 frames after the last one; the 500 ms armed by `fsVeil`
/// is only a ceiling for a transition that produces no resize at all.
///
/// It has to be the *same* timer, which is why this is exported rather than
/// left to the caller: while the page kept a timer of its own, canceling the
/// adaptive release did not cancel the ceiling, so a resize that ran long had
/// its veil taken away from underneath it.
export function scheduleVeilRelease(ms: number) {
  clearTimeout(fsTransitionTimer);
  fsTransitionTimer = setTimeout(releaseVeil, ms);
}

function fsVeil() {
  chrome.fsTransition = true;
  scheduleVeilRelease(500);
}

// Wait until the veil frame is ACTUALLY on screen before resizing the window.
// Otherwise the resize starts before the veil's first paint and Windows
// briefly shows a stretched buffer with the old layout (controls "jump" up
// and to the left). A black frame stretches into black — no visible artifact.
async function presentVeil() {
  fsVeil();
  await new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r())));
  await new Promise<void>((r) => setTimeout(r, 30));
}

// Monitor-sized shutter window: the last 1-2 DWM composition frames (the old
// window buffer blended with new content) cannot be hidden from inside the
// window at all — so a separate black always-on-top window covers them for
// the duration of the resize.
async function showShutter() {
  try {
    const shutter = await WebviewWindow.getByLabel('veil');
    if (!shutter) return;
    const mon = await currentMonitor();
    // While we were fetching the monitor (several IPC calls), a resize event
    // may already have released the veil. Showing the shutter now is not an
    // option: releaseVeil will not come again, and a monitor-sized black
    // window (alwaysOnTop, focusable: false) would hang there forever with
    // nothing left to close it.
    if (!chrome.fsTransition) return;
    if (mon) {
      await shutter.setPosition(new PhysicalPosition(mon.position.x, mon.position.y));
      await shutter.setSize(new PhysicalSize(mon.size.width, mon.size.height));
    }
    await shutter.show();
    // ...and once more after show(), for the same reason.
    if (!chrome.fsTransition) {
      await shutter.hide();
      return;
    }
    await shutter.setAlwaysOnTop(true);
  } catch (e) {
    console.warn('shutter show failed:', e);
  }
}

async function hideShutter() {
  try {
    const shutter = await WebviewWindow.getByLabel('veil');
    await shutter?.hide();
  } catch {
    // not critical
  }
}

// The veil and shutter exist for DWM composition artifacts — on macOS the
// system animates the fullscreen transition and there is nothing to hide.
async function maskFullscreenTransition() {
  if (IS_MAC) return;
  // The shutter color must match the veil (the release relies on an
  // invisible seam between them) — announce it before show().
  await emitTo('veil', 'veil-color', chrome.startScreen ? '#101016' : '#000').catch(() => {});
  await presentVeil();
  await showShutter();
}

export async function toggleFullscreen() {
  // Leaving mini on the way in: `toggleMini` does the same in reverse, and
  // without it fullscreen would restore to a thumbnail in the corner.
  if (mini.on) await toggleMini();
  if (chrome.fullscreen) {
    await exitFullscreen();
    return;
  }
  await maskFullscreenTransition();
  chrome.fullscreen = true;
  await getCurrentWindow().setFullscreen(true);
}

export async function exitFullscreen() {
  if (!chrome.fullscreen) return;
  await maskFullscreenTransition();
  chrome.fullscreen = false;
  await getCurrentWindow().setFullscreen(false);
}

/// Seek popup: "24:01 / 53:34" plus a thin progress bar. The position is
/// optimistic rather than read from timePos (which mpv only updates a frame
/// or two after the seek), otherwise the popup lags one step behind.

// Title bar height, and the width of the macOS traffic lights within it.
//
// The cursor is kept visible over that corner and nowhere else. Hovering the
// system window buttons stops the webview from receiving mousemove — the
// cursor "sticks" there, idle sets in on the timer, and hiding it would be
// wrong, because the user is working with the native popup on the green
// button and we cannot see that they are.
//
// It used to be the whole 48px band across the full width, on both platforms,
// which is far more than the reason justifies: in a window the pointer lands
// in that band constantly (after dragging the window, after using the
// buttons), and there it looked as though the cursor simply never hid in
// windowed mode. Nothing on Windows needs the exception at all — the window
// buttons there are our own HTML, so they report mousemove like everything
// else, and once the bar has faded they are not even hit-testable.

export function pokeUi(e?: MouseEvent) {
  if (e) {
    // A boolean, not a coordinate: it only changes when the border is
    // crossed, otherwise reactivity would churn on every mouse move.
    const inTop =
      IS_MAC && e.clientY <= TITLEBAR_STRIP && e.clientX <= MAC_BUTTONS_WIDTH;
    if (inTop !== chrome.pointerInTitlebar) chrome.pointerInTitlebar = inTop;
  }
  chrome.uiVisible = true;
  clearTimeout(hideTimer);
  hideTimer = setTimeout(() => (chrome.uiVisible = false), UI_HIDE_MS);
}

// ---- Custom title bar (window without system decorations) ----

// Windows: titlebar drag/maximize is ours, instead of data-tauri-drag-region
// (macOS keeps the stock attribute). Tauri's built-in script calls
// internal_toggle_maximize on double click, and tao's set_maximized does not
// clear the fullscreen state: the window visibly leaves fullscreen, but
// isFullscreen() keeps answering true — the mirror and the fullscreen button
// get stuck. Our handler exits fullscreen honestly via exitFullscreen (and
// does not let a fullscreen window be dragged by the titlebar).
export function onTitlebarMouseDown(e: MouseEvent) {
  if (IS_MAC || e.button !== 0 || e.target !== e.currentTarget) return;
  if (e.detail !== 1 && e.detail !== 2) return;
  e.preventDefault();
  if (e.detail === 2) {
    if (chrome.fullscreen) void exitFullscreen();
    else void getCurrentWindow().toggleMaximize();
  } else if (!chrome.fullscreen) {
    void getCurrentWindow().startDragging();
  }
}

export function minimizeWindow() {
  void getCurrentWindow().minimize();
}

export function closeWindow() {
  // destroy(), not close(): close() raises CloseRequested, where the libmpv
  // plugin tears mpv down while the window is still on screen (see the
  // onCloseRequested note in onMount). Destroying directly keeps the last
  // video frame alive into the DWM close animation.
  flushPosition();
  void getCurrentWindow().destroy();
}

export function startResize(
  dir: 'East' | 'North' | 'NorthEast' | 'NorthWest' | 'South' | 'SouthEast' | 'SouthWest' | 'West',
) {
  void getCurrentWindow().startResizeDragging(dir);
}

/// Breathing space between the title and whichever cluster it reaches first.
/// Added to the measurement rather than to the CSS, so the number lives in
/// one language only.
const BAR_TITLE_GAP = 16;


function barSideEffect() {
  $effect(() => {
    const sides = [chrome.brandEl, chrome.chromeEl].filter((el): el is HTMLElement => !!el);
    if (!sides.length) return;
    const measure = () => {
      chrome.barSide = Math.max(...sides.map((el) => el.getBoundingClientRect().width)) + BAR_TITLE_GAP;
    };
    measure();
    const observer = new ResizeObserver(measure);
    for (const el of sides) observer.observe(el);
    return () => observer.disconnect();
  });
}

/**
 * Start the shell's three standing effects. **Must be called from a component's
 * initialization.**
 *
 * A bare `$effect` at the top level of a `.svelte.ts` throws `effect_orphan`
 * the moment the module is imported, and `svelte-check` says nothing about it —
 * the module type-checks perfectly and the app dies on load. (Measured against
 * the real compiler: `compileModule` emits `$.user_effect(…)` at module scope,
 * and importing the result throws.) `scripts/check-runes.mjs` is the gate.
 *
 * A block body rather than `const f = () => $effect(…)`: the compiler rejects
 * that outright with "`$effect()` can only be used as an expression statement".
 */
export function initChrome() {
  cursorEffect();
  windowButtonsEffect();
  barSideEffect();
}
