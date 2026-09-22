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
//
// What is left is still only a *guess*, and a latching one: it is written from
// the last mousemove, and the pointer stops sending those the moment it crosses
// onto a button — which is the whole reason the exception exists. So the box is
// the opening bid and `window_buttons` settles it: the native side measures the
// pointer against the buttons' real frames and `syncWindowButtons` writes the
// answer back over this flag. Without that, a pointer that merely passed
// through the corner on its way out of the window left the arrow on screen for
// good.
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
  /// The pointer is on the room chip. A third flag for the same idea as the two
  /// above, and it earns its place the moment the chip stopped being a readout:
  /// a control that fades out from under a resting cursor cannot be clicked, and
  /// the chip is where you press to see who is in the room.
  chipHover = $state(false);
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
      !this.chipHover &&
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
let winButtonsShown = true;
/// Settles when the last instruction sent to the traffic lights has actually
/// been carried out on the main thread — which is what the cursor waits for
/// (see `CURSOR_SETTLE_MS`), so it must always settle, including when the
/// instruction is superseded before it is sent.
let winButtonsSettled: Promise<void> = Promise.resolve();
/// Supersedes a delayed hide that has not been sent yet.
let winButtonsSeq = 0;
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
  const seq = ++winButtonsSeq;
  const apply = async (visible: boolean) => {
    if (winButtonsShown === visible) return;
    winButtonsShown = visible;
    // The command answers with the one thing only the native side can know:
    // whether the pointer is on the traffic lights *now*, measured against
    // their real frames. `pointerInTitlebar` is a latch on the last mousemove
    // and cannot answer it — the webview stops receiving them the moment the
    // pointer crosses onto a button, so the latch is right while it is there
    // and stale forever after, which is what left the cursor on screen with
    // the rest of the chrome gone. The reply is therefore the authority, and
    // correcting the latch from it is the whole point of awaiting the call.
    const onButtons = await invoke<boolean>('window_buttons', { visible }).catch(() => null);
    if (typeof onButtons === 'boolean' && onButtons !== chrome.pointerInTitlebar) {
      chrome.pointerInTitlebar = onButtons;
    }
  };
  if (!hidden) {
    winButtonsSettled = apply(true);
  } else if (immediate) {
    winButtonsSettled = apply(false);
  } else {
    winButtonsSettled = (async () => {
      await new Promise<void>((r) => setTimeout(r, WIN_BUTTONS_HIDE_MS));
      if (seq !== winButtonsSeq) return;
      await apply(false);
    })();
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
/// This used to be one flat 200 ms covering the 40 ms button timer, the command
/// round-trip, the hop to the main thread and the relayout — four unbounded
/// costs behind one guessed number, and a machine busy decoding is exactly
/// where it runs out: the arrow then stays until the pointer is moved, which is
/// how the bug was reported. So the wait is *causal* now — `winButtonsSettled`
/// resolves only once the main thread has run `set_buttons_visible` — and this
/// constant covers the one thing left afterwards, AppKit resetting the cursor
/// rectangles in its own layout pass.
const CURSOR_SETTLE_MS = 120;
/// …and a ceiling on the causal wait. The reply comes back over IPC and through
/// the main thread's event loop, and a cursor that never hides again because
/// one of those was lost would be a worse failure than the race this replaced.
const CURSOR_WAIT_CEILING_MS = 600;
let cursorTimer: ReturnType<typeof setTimeout> | undefined;
/// Only the newest attempt may hide the cursor (latest.ts's rule, hand-written
/// here because the wait is a promise the effect did not create).
let cursorRun = 0;

function cursorEffect() {
  $effect(() => {
    clearTimeout(cursorTimer);
    const run = ++cursorRun;
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
    // `winButtonsSettled` is read a microtask late, not here: the effect that
    // writes it runs in this same flush, and which of the two was created
    // first is not something the cursor should depend on.
    void Promise.resolve()
      .then(() =>
        Promise.race([
          winButtonsSettled,
          new Promise<void>((r) => setTimeout(r, CURSOR_WAIT_CEILING_MS)),
        ]),
      )
      .then(() => {
        if (run !== cursorRun) return;
        cursorTimer = setTimeout(() => {
          if (run === cursorRun) chrome.cursorHidden = true;
        }, CURSOR_SETTLE_MS);
      });
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
  // macOS: the native side takes the title-bar toolbar off before the
  // transition starts, not inside it — see `window_enter_fullscreen`.
  if (IS_MAC && (await invoke<boolean>('window_enter_fullscreen').catch(() => false))) return;
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

/// The surfaces the pointer may rest on without the chrome fading out from
/// under it, and the flag each one raises.
///
/// Every one of them sets its flag on `mouseenter` and clears it on
/// `mouseleave`, which is edge-triggered state: a single lost `mouseleave`
/// pins the chrome on screen **permanently**, because nothing else ever writes
/// the flag back. And they do get lost — the pointer crossing onto the macOS
/// traffic lights or out through the title bar, a native window drag taking
/// the mouse away mid-hover, the window changing space or application while a
/// control is hovered, an element leaving the DOM under the pointer (the room
/// chip is behind `{#if wire.on}`). The reported symptom is the interface
/// hanging there with the pointer nowhere near it, and the workaround people
/// find by themselves is telling: hover the bar and leave it again, which is
/// nothing but delivering the `mouseleave` by hand.
///
/// So the flags are reconciled against the mouse position on every move — the
/// one signal that is never stale. One `closest` for all three, since they do
/// not nest: the chip and the OSC are siblings of the bar, not children.
const HOVER_SURFACES = '.osc, .topbar, .roomchip';

function reconcileHover(e: MouseEvent) {
  const el = e.target instanceof Element ? e.target.closest(HOVER_SURFACES) : null;
  const osc = el?.classList.contains('osc') ?? false;
  const bar = el?.classList.contains('topbar') ?? false;
  const chip = el?.classList.contains('roomchip') ?? false;
  if (osc !== chrome.oscHover) chrome.oscHover = osc;
  if (bar !== chrome.barHover) chrome.barHover = bar;
  if (chip !== chrome.chipHover) chrome.chipHover = chip;
}

export function pokeUi(e?: MouseEvent) {
  if (e) {
    // A boolean, not a coordinate: it only changes when the border is
    // crossed, otherwise reactivity would churn on every mouse move.
    const inTop =
      IS_MAC && e.clientY <= TITLEBAR_STRIP && e.clientX <= MAC_BUTTONS_WIDTH;
    if (inTop !== chrome.pointerInTitlebar) chrome.pointerInTitlebar = inTop;
    reconcileHover(e);
  }
  chrome.uiVisible = true;
  clearTimeout(hideTimer);
  hideTimer = setTimeout(() => (chrome.uiVisible = false), UI_HIDE_MS);
}

/**
 * Coming back to the window counts as a poke.
 *
 * Switching spaces or activating the window makes AppKit rebuild what it draws
 * — and, as with the traffic-light relayout above, that puts the arrow back
 * without sending a single mousemove, so `cursor: none` is dropped and nothing
 * re-applies it: the cursor sits over the video until the pointer is moved.
 * Raising the chrome here restarts the idle cycle from the top, and the class
 * goes back on as a style change, which is a thing WebKit *does* answer with a
 * cursor update while the pointer stands still (it is how the cursor comes to
 * be hidden at all).
 *
 * The visible cost is the bars appearing for `UI_HIDE_MS` when you switch to
 * the player, which is the same thing that happens when you touch the mouse.
 */
function wakeEffect() {
  $effect(() => {
    const wake = () => {
      if (document.visibilityState === 'hidden') return;
      pokeUi();
    };
    window.addEventListener('focus', wake);
    document.addEventListener('visibilitychange', wake);
    return () => {
      window.removeEventListener('focus', wake);
      document.removeEventListener('visibilitychange', wake);
    };
  });
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
 * Start the shell's four standing effects. **Must be called from a component's
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
  wakeEffect();
  barSideEffect();
}
