/**
 * Window geometry, the "fit to video" rule and always-on-top.
 *
 * Stored in localStorage next to volume and loop. Geometry is restored from the
 * frontend rather than natively: the window is created hidden and shown from
 * here too, so the size and position land before it becomes visible — restoring
 * an already visible window reads as a jump.
 */

import { invoke } from '@tauri-apps/api/core';
import { PhysicalPosition, PhysicalSize } from '@tauri-apps/api/dpi';
import { currentMonitor, getCurrentWindow } from '@tauri-apps/api/window';

import { t } from './i18n.svelte';
import { showOsd } from './osd.svelte';
import { IS_MAC } from './platform';
import { player } from './player.svelte';

/// Minimum window size in LOGICAL pixels. Duplicates minWidth/minHeight from
/// tauri.conf.json: there is no way to read them back from the window, and
/// fitting to the video has to respect them itself — otherwise the window runs
/// away on its shoulders. Changed together with the config.
const MIN_WINDOW_W = 480;
const MIN_WINDOW_H = 320;

/// Margin between the window and the edge of the work area when auto-fitting,
/// in logical pixels. A window flush against the screen edge looks cropped.
const SCREEN_PADDING = 24;

const WINDOW_PREFS_KEY = 'frameplayer.window';

/// Bumped when the defaults change in a way that must reach existing installs.
/// v2: remember/fitToVideo forced back to off — both misbehave and are disabled
/// by default until fixed. Payloads without a version predate the reset, so
/// their remember/fitToVideo are ignored; an explicit toggle after this build
/// saves with the current version and sticks.
const PREFS_VERSION = 2;

class WindowPrefs {
  /// Remember the geometry between runs. Off by default while restore is buggy.
  remember = $state(false);
  /// Fit the window to the video aspect when a file opens.
  fitToVideo = $state(false);
  alwaysOnTop = $state(false);
  /// Magnetic edges for the mini window. On by default: a corner window is put
  /// where it is meant to be out of the way, and pixel-accurate placement is
  /// not what anyone is trying to do with it.
  snapMini = $state(true);
  geometry = $state<{ x: number; y: number; w: number; h: number } | null>(null);
}

export const windowPrefs = new WindowPrefs();

// ---- Mini player ----------------------------------------------------------
//
// With `wid` embedding there is no true picture-in-picture to reach for: the
// video is a child view of our own window. A small always-on-top window with
// the chrome out of the way is the same thing by another route, and every part
// of it — geometry, always-on-top, hiding the bars — already exists here.

/// Logical width of the mini window. Height follows the video's aspect.
const MINI_WIDTH = 420;

/// The minimum the window may be dragged down to *while mini*, logical pixels.
/// Below this the seek row stops being usable.
const MINI_MIN_W = 240;
const MINI_MIN_H = 135;

/// Where the geometry came from, to put it back on the way out. Also the flag
/// the rest of the app reads: mini is a *mode*, and several things have to
/// stand down while it is on.
let beforeMini: { x: number; y: number; w: number; h: number; onTop: boolean } | null = null;

class MiniState {
  on = $state(false);
}
export const mini = new MiniState();

/**
 * Float over *other applications'* fullscreen spaces (macOS only).
 *
 * The one thing a system PiP panel does that always-on-top does not: a floating
 * level only puts the window above the others on its own space, and a
 * fullscreen app is a space of its own — so the mini player disappeared exactly
 * when it was most wanted. It takes more than a window flag (the window is
 * promoted to an NSPanel for the duration); what was measured, and what has to
 * be put back, is in `macos_chrome::set_float_over_fullscreen`.
 *
 * Deliberately not fatal to the mode: a mini window that does not float over
 * fullscreen is still a mini window, unlike a geometry call that fails and
 * leaves the chrome shuffled on a full-size window.
 */
async function floatOverFullscreen(on: boolean) {
  if (!IS_MAC) return;
  try {
    await invoke('window_float_over_fullscreen', { on });
  } catch (e) {
    console.warn('float over fullscreen failed:', e);
  }
}

/**
 * Shrink to a corner, or come back.
 *
 * The minimum window size has to be lifted first: `minWidth`/`minHeight` in
 * tauri.conf are 480×320, and a request below that is silently clamped — the
 * window would simply not shrink, with nothing to say why. It is put back on
 * the way out, so an ordinary window still cannot be dragged into uselessness.
 *
 * The whole geometry half runs *before* `mini.on` flips, and a failure leaves
 * the mode off. Flipping first meant a rejected window call — one missing
 * capability was enough — showed the mini chrome on a window that had not
 * moved or resized at all: the mode appeared to do nothing but shuffle the
 * controls, which is worse than not entering it.
 */
export async function toggleMini() {
  const win = getCurrentWindow();
  // Whatever a snap in flight was aiming at, this supersedes it.
  glideSeq++;
  if (mini.on) {
    const back = beforeMini;
    try {
      // First, so the awaited window calls below act as a barrier: they travel
      // the same main-thread queue, so by the time they resolve the window can
      // be made fullscreen again — which `toggleFullscreen` does immediately
      // after leaving mini.
      await floatOverFullscreen(false);
      await win.setMinSize(new PhysicalSize(MIN_WINDOW_W, MIN_WINDOW_H));
      if (back) {
        await win.setSize(new PhysicalSize(back.w, back.h));
        await win.setPosition(new PhysicalPosition(back.x, back.y));
        await win.setAlwaysOnTop(back.onTop);
      }
    } catch (e) {
      // Leave the mode anyway: being stuck in mini chrome is worse than a
      // window that kept the small size and can be resized by hand.
      console.warn('leaving mini failed:', e);
    }
    beforeMini = null;
    mini.on = false;
    return;
  }

  try {
    // Leaving fullscreen first: the two are the same control in opposite
    // directions, and a fullscreen window that "shrinks" stays fullscreen.
    if (await win.isFullscreen()) await win.setFullscreen(false);

    const pos = await win.outerPosition();
    const size = await win.outerSize();
    const dpr = await win.scaleFactor();
    const aspect =
      player.videoW > 0 && player.videoH > 0 ? player.videoW / player.videoH : 16 / 9;
    const w = Math.round(MINI_WIDTH * dpr);
    const h = Math.round((MINI_WIDTH / aspect) * dpr);

    await win.setMinSize(new PhysicalSize(Math.round(MINI_MIN_W * dpr), Math.round(MINI_MIN_H * dpr)));
    await win.setSize(new PhysicalSize(w, h));

    // Bottom-right of the work area, which already excludes the Dock and the
    // taskbar — the corner a small window is expected to go to.
    const mon = await currentMonitor();
    if (mon?.workArea) {
      const pad = Math.round(SCREEN_PADDING * dpr);
      const area = mon.workArea;
      await win.setPosition(
        new PhysicalPosition(
          area.position.x + area.size.width - w - pad,
          area.position.y + area.size.height - h - pad,
        ),
      );
    }
    await win.setAlwaysOnTop(true);
    // After the level: floating is what puts the window on top at all, the
    // collection behavior only decides which spaces it may appear on.
    await floatOverFullscreen(true);

    beforeMini = { x: pos.x, y: pos.y, w: size.width, h: size.height, onTop: windowPrefs.alwaysOnTop };
    mini.on = true;
  } catch (e) {
    console.warn('entering mini failed:', e);
    // Undo the one thing that may already have landed, so an ordinary window
    // is not left resizable down to nothing.
    try {
      await win.setMinSize(new PhysicalSize(MIN_WINDOW_W, MIN_WINDOW_H));
    } catch {
      // nothing further to do
    }
  }
}

/// How close to a resting place the window has to be dragged, in logical
/// pixels, for it to be taken the rest of the way.
const SNAP_DIST = 56;

/// How long the window has to sit still before it is snapped. A native drag
/// runs its own event loop and does not tell us when it ends, so "stopped
/// moving" is the only signal there is; long enough that a pause mid-drag
/// rarely reaches it, short enough to read as the window settling rather than
/// jumping later on its own.
const SNAP_SETTLE_MS = 300;

/// How long the glide to a resting place takes. The window teleporting there
/// reads as a glitch — the eye has nothing to attribute the new position to;
/// the same move animated reads as the window being pulled.
const GLIDE_MS = 190;

let snapTimer: ReturnType<typeof setTimeout> | undefined;
/// Bumped by anything that takes over the window's position, so a glide in
/// flight gives up instead of fighting it.
let glideSeq = 0;

/**
 * Move the window over time rather than in one jump.
 *
 * Stepped from the frontend because there is no animated `setPosition`: one
 * `setPosition` per frame, driven by elapsed time rather than by a frame count,
 * so a slow round trip stretches the steps instead of the animation. Ease-out,
 * because the window is being *caught* by the edge — the motion belongs at the
 * start, next to the gesture that caused it.
 */
async function glideTo(win: ReturnType<typeof getCurrentWindow>, fromX: number, fromY: number, x: number, y: number) {
  const dx = x - fromX;
  const dy = y - fromY;
  // Under a few pixels the animation is invisible while the round trips are not.
  if (Math.hypot(dx, dy) < 3) {
    await win.setPosition(new PhysicalPosition(x, y));
    return;
  }
  const seq = ++glideSeq;
  const start = performance.now();
  for (;;) {
    await new Promise((r) => requestAnimationFrame(r));
    if (seq !== glideSeq) return;
    const t = Math.min(1, (performance.now() - start) / GLIDE_MS);
    const k = 1 - (1 - t) ** 3;
    await win.setPosition(
      new PhysicalPosition(Math.round(fromX + dx * k), Math.round(fromY + dy * k)),
    );
    if (t >= 1) return;
  }
}

/// Called for every move event; the work happens once the window comes to rest.
export function scheduleMiniSnap() {
  if (!mini.on || !windowPrefs.snapMini) return;
  clearTimeout(snapTimer);
  snapTimer = setTimeout(() => void snapMiniToEdges(), SNAP_SETTLE_MS);
}

/**
 * Pull the mini window to the nearest edge, and never let it hang off one.
 *
 * The two halves answer different things and both are needed: snapping owns
 * the band near an edge (dragged to 5 px from it, the window means the corner,
 * and the gutter it lands in is the same one it opened in), while clamping
 * owns everything outside the work area — a window half off the screen is
 * never what was meant, and the mini player has no title bar to grab it back
 * by. The axes are independent, so a window dragged to the right edge keeps
 * the height it was put at.
 */
async function snapMiniToEdges() {
  if (!mini.on || !windowPrefs.snapMini) return;
  try {
    const win = getCurrentWindow();
    const mon = await currentMonitor();
    if (!mon?.workArea) return;
    const dpr = await win.scaleFactor();
    const pos = await win.outerPosition();
    const size = await win.outerSize();
    const area = mon.workArea;
    const pad = Math.round(SCREEN_PADDING * dpr);
    const snap = SNAP_DIST * dpr;

    const rest = {
      left: area.position.x + pad,
      right: area.position.x + area.size.width - size.width - pad,
      top: area.position.y + pad,
      bottom: area.position.y + area.size.height - size.height - pad,
    };
    let x = pos.x;
    let y = pos.y;
    if (Math.abs(x - rest.left) < snap) x = rest.left;
    else if (Math.abs(x - rest.right) < snap) x = rest.right;
    if (Math.abs(y - rest.top) < snap) y = rest.top;
    else if (Math.abs(y - rest.bottom) < snap) y = rest.bottom;

    // Fully inside the work area, with no margin of its own: the margin is
    // where snapping puts it, and forcing one here would make every position
    // within a screen edge's reach unreachable.
    const maxX = area.position.x + area.size.width - size.width;
    const maxY = area.position.y + area.size.height - size.height;
    x = Math.min(Math.max(x, area.position.x), Math.max(area.position.x, maxX));
    y = Math.min(Math.max(y, area.position.y), Math.max(area.position.y, maxY));

    // Each step of the glide schedules another pass; the last one finds nothing
    // to do and stops there.
    if (x !== pos.x || y !== pos.y) await glideTo(win, pos.x, pos.y, x, y);
  } catch (e) {
    console.warn('mini snap failed:', e);
  }
}

/// Leave mini if it is on — used when the player goes back to the start screen,
/// where a corner-sized window with nothing playing is a trap: the button that
/// leaves it is drawn over the video.
export async function exitMini() {
  if (mini.on) await toggleMini();
}

export function loadWindowPrefs() {
  try {
    const raw = localStorage.getItem(WINDOW_PREFS_KEY);
    if (!raw) return;
    const saved = JSON.parse(raw) as Partial<WindowPrefs> & { v?: number };
    // Pre-reset payloads (no version) keep only alwaysOnTop and the geometry
    // itself — remember/fitToVideo revert to the new off defaults once.
    if (saved.v === PREFS_VERSION) {
      if (typeof saved.remember === 'boolean') windowPrefs.remember = saved.remember;
      if (typeof saved.fitToVideo === 'boolean') windowPrefs.fitToVideo = saved.fitToVideo;
    }
    if (typeof saved.alwaysOnTop === 'boolean') windowPrefs.alwaysOnTop = saved.alwaysOnTop;
    if (typeof saved.snapMini === 'boolean') windowPrefs.snapMini = saved.snapMini;
    if (saved.geometry) windowPrefs.geometry = saved.geometry;
  } catch {
    // corrupt entry — the defaults stay
  }
}

function saveWindowPrefs() {
  try {
    localStorage.setItem(
      WINDOW_PREFS_KEY,
      JSON.stringify({
        v: PREFS_VERSION,
        remember: windowPrefs.remember,
        fitToVideo: windowPrefs.fitToVideo,
        alwaysOnTop: windowPrefs.alwaysOnTop,
        snapMini: windowPrefs.snapMini,
        geometry: windowPrefs.geometry,
      }),
    );
  } catch {
    // localStorage unavailable — not critical
  }
  syncMenuChecks();
}

/// The macOS menu bar check marks mirror these same settings. The frontend owns
/// the state and the menu only displays it, hence the one-way sync.
export function syncMenuChecks() {
  if (!IS_MAC) return;
  void invoke('sync_window_menu', {
    remember: windowPrefs.remember,
    fitToVideo: windowPrefs.fitToVideo,
    alwaysOnTop: windowPrefs.alwaysOnTop,
    snapMini: windowPrefs.snapMini,
  }).catch(() => {});
}

/// Save the current geometry. Called debounced from resize/move: writing to
/// localStorage for every pixel of a drag is pointless.
let geometrySaveTimer: ReturnType<typeof setTimeout> | undefined;

export function scheduleGeometrySave() {
  clearTimeout(geometrySaveTimer);
  geometrySaveTimer = setTimeout(() => void captureGeometry(), 400);
}

async function captureGeometry() {
  if (!windowPrefs.remember) return;
  try {
    const win = getCurrentWindow();
    // A fullscreen or maximized window must not be remembered: it would come
    // back as a monitor-sized window with no maximized flag. Nor a mini one,
    // which would come back as a thumbnail in the corner — same trap, and the
    // one the viewer would notice next launch rather than now.
    if (mini.on || (await win.isFullscreen()) || (await win.isMaximized())) return;
    const pos = await win.outerPosition();
    const size = await win.outerSize();
    if (size.width < 100 || size.height < 100) return;
    windowPrefs.geometry = { x: pos.x, y: pos.y, w: size.width, h: size.height };
    saveWindowPrefs();
  } catch {
    // the window may be gone — not critical
  }
}

/// Restore the geometry before showing the window. Anything off-screen is
/// caught by window_guard on the Rust side (clamp_to_visible_area).
export async function restoreGeometry() {
  const g = windowPrefs.geometry;
  if (!windowPrefs.remember || !g) return;
  try {
    const win = getCurrentWindow();
    await win.setSize(new PhysicalSize(g.w, g.h));
    await win.setPosition(new PhysicalPosition(g.x, g.y));
    await settleLayout(g.w, g.h);
  } catch {
    // did not work out — the window stays at its default position
  }
}

/**
 * Waits for the webview to relayout for the new window size.
 *
 * The resize reaches the webview asynchronously, and without waiting the window
 * was shown with the layout of its previous size: the content did not span the
 * full width (visible in the title-bar logo inset) and only settled once
 * something forced a repaint — a poster finishing loading, say. The window is
 * still hidden at this point, so the wait is invisible.
 */
async function settleLayout(targetW: number, targetH: number) {
  const dpr = window.devicePixelRatio || 1;
  const deadline = performance.now() + 400;
  while (performance.now() < deadline) {
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    // Compare with a tolerance: CSS pixels are rounded, and the outer window
    // size includes a frame we have exactly as much of as we do not.
    const w = window.innerWidth * dpr;
    const h = window.innerHeight * dpr;
    if (Math.abs(w - targetW) <= 4 * dpr && Math.abs(h - targetH) <= 48 * dpr) return;
  }
}

/**
 * Fit the window so the video area matches the frame's aspect ratio.
 * `scale` is a fraction of the video's natural size (1 = pixel for pixel), or
 * null to keep the current window area and change only the aspect.
 */
export async function fitWindowToVideo(scale: number | null) {
  if (player.videoW <= 0 || player.videoH <= 0) return;
  // Mini is a size the viewer asked for; a new video's aspect must not undo it.
  if (mini.on) return;
  const win = getCurrentWindow();
  try {
    if ((await win.isFullscreen()) || (await win.isMaximized())) return;
    const mon = await currentMonitor();
    const dpr = await win.scaleFactor();
    // Our chrome (title bar and OSC are drawn over the video) takes no height,
    // but the window frame does: take outer minus inner size.
    const outer = await win.outerSize();
    const inner = await win.innerSize();
    const chromeW = outer.width - inner.width;
    const chromeH = outer.height - inner.height;

    let targetW: number;
    let targetH: number;
    if (scale === null) {
      // Same area, new aspect — the window does not jump in size.
      const area = inner.width * inner.height;
      const k = Math.sqrt(area / (player.videoW * player.videoH));
      targetW = player.videoW * k;
      targetH = player.videoH * k;
    } else {
      targetW = player.videoW * scale * dpr;
      targetH = player.videoH * scale * dpr;
    }

    // Fit into the monitor's work area with margins. workArea, not size: it
    // already excludes the Dock and the taskbar, whereas the old "95% and 90%
    // of the screen" was eyeballed guesswork — it undershot on a monitor with a
    // Dock on the left and overshot without one.
    const pad = SCREEN_PADDING * dpr;
    const area = mon?.workArea;
    // Whether the requested size had to be shrunk to fit decides whether the
    // position is kept or the window is centered (see below).
    let shrunk = false;
    if (area) {
      const maxW = area.size.width - pad * 2 - chromeW;
      const maxH = area.size.height - pad * 2 - chromeH;
      const shrink = Math.min(1, maxW / targetW, maxH / targetH);
      shrunk = shrink < 1;
      targetW *= shrink;
      targetH *= shrink;
    }

    // The lower bound is in PHYSICAL pixels, i.e. the config minimum times the
    // screen scale. Comparing a logical minimum against a physical size
    // directly does not work: on Retina (dpr = 2), "no smaller than 480" became
    // 240 logical — half the real minimum — and vertical video at 50% shrank
    // the window until the bottom controls were cut off.
    const outW = Math.max(MIN_WINDOW_W * dpr, Math.round(targetW + chromeW));
    const outH = Math.max(MIN_WINDOW_H * dpr, Math.round(targetH + chromeH));
    await win.setSize(new PhysicalSize(outW, outH));

    // The size changed around the TOP-LEFT corner, so a window near the right
    // or bottom edge grows outwards. Pull it back into the work area, keeping
    // the same margins.
    if (area) {
      const pos = await win.outerPosition();
      const minX = area.position.x + pad;
      const minY = area.position.y + pad;
      const maxX = area.position.x + area.size.width - pad - outW;
      const maxY = area.position.y + area.size.height - pad - outH;
      const centerX = area.position.x + Math.round((area.size.width - outW) / 2);
      const centerY = area.position.y + Math.round((area.size.height - outH) / 2);
      // Two different situations:
      //  • the size was granted as requested — the window stays where it was
      //    and is only pulled back inside if it overflowed;
      //  • the size had to be shrunk to fit (or it hit the minimum, in which
      //    case maxX < minX) — the old position is meaningless, so center it.
      const x = shrunk || maxX < minX ? centerX : Math.min(Math.max(pos.x, minX), maxX);
      const y = shrunk || maxY < minY ? centerY : Math.min(Math.max(pos.y, minY), maxY);
      if (x !== pos.x || y !== pos.y) {
        await win.setPosition(new PhysicalPosition(x, y));
      }
    }
    void captureGeometry();
  } catch (e) {
    console.warn('fitWindowToVideo failed:', e);
  }
}

/// Fit for a new file — once per new resolution.
let fittedFor = '';

export function maybeFitWindow() {
  if (!windowPrefs.fitToVideo || player.videoW <= 0 || player.videoH <= 0) return;
  const key = `${player.videoW}x${player.videoH}`;
  if (fittedFor === key) return;
  fittedFor = key;
  void fitWindowToVideo(null);
}

export async function applyAlwaysOnTop() {
  // Mini owns this while it is on; the pref is restored on the way out.
  if (mini.on) return;
  try {
    await getCurrentWindow().setAlwaysOnTop(windowPrefs.alwaysOnTop);
  } catch {
    // not critical
  }
}

export function toggleWindowPref(key: 'remember' | 'fitToVideo' | 'alwaysOnTop' | 'snapMini') {
  windowPrefs[key] = !windowPrefs[key];
  saveWindowPrefs();
  if (key === 'snapMini') {
    // Turned on with the window already near an edge — take it there now,
    // rather than leaving the setting to prove itself on the next drag.
    if (windowPrefs.snapMini) void snapMiniToEdges();
    showOsd(t(windowPrefs.snapMini ? 'osd.snap_on' : 'osd.snap_off'));
  } else if (key === 'alwaysOnTop') {
    void applyAlwaysOnTop();
    showOsd(t(windowPrefs.alwaysOnTop ? 'osd.ontop_on' : 'osd.ontop_off'));
  } else if (key === 'remember') {
    // Turned on — capture the current placement right away, so the setting
    // takes effect on the next launch and not only after a resize.
    if (windowPrefs.remember) void captureGeometry();
    showOsd(t(windowPrefs.remember ? 'osd.remember_on' : 'osd.remember_off'));
  } else {
    if (windowPrefs.fitToVideo && player.hasFile) void fitWindowToVideo(null);
    showOsd(t(windowPrefs.fitToVideo ? 'osd.fit_on' : 'osd.fit_off'));
  }
}
