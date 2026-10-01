/**
 * Window geometry, the window's shape ("match video aspect ratio") and
 * always-on-top.
 *
 * Stored in localStorage next to volume and loop. Geometry is restored from the
 * frontend rather than natively: the window is created hidden and shown from
 * here too, so the size and position land before it becomes visible — restoring
 * an already visible window reads as a jump.
 */

import { invoke } from '@tauri-apps/api/core';
import { LogicalSize, PhysicalPosition, PhysicalSize } from '@tauri-apps/api/dpi';
import { currentMonitor, getCurrentWindow } from '@tauri-apps/api/window';

import { t } from './i18n.svelte';
import { latest, type Attempt } from './latest';
import { showOsd } from './osd.svelte';
import { IS_MAC } from './platform';
import { player } from './player.svelte';
import {
  fitWindow,
  floorForShape,
  PINCH_CHOICES,
  pictureShape,
  pinchResizes,
  placeAround,
  shapeFits,
  shapeKey,
  type PinchAction,
  type Rect,
  type Size,
} from './window-fit';

export { PINCH_CHOICES, type PinchAction };

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
/// v2: remember/fitToVideo forced back to off — both misbehaved. Payloads
/// without a version predate that reset, so their remember/fitToVideo are
/// ignored.
/// v3: fitToVideo is on by default, with what made it misbehave fixed (see
/// `fitToShape`). Every payload carries every field, so an "off" written under
/// v2 was the default of its day and not anybody's choice — it is not read.
/// One written under v3 is, and sticks.
const PREFS_VERSION = 3;

/**
 * When the controls hide themselves after the pointer stops.
 *
 * `always` is the player's own behaviour and the default. `fullscreen` is what
 * somebody coming from VLC or MPC expects — a bar that belongs to the window
 * and stays there, and a picture with nothing on it once the window *is* the
 * picture. `never` means never, fullscreen included; a setting that says
 * "never" and then hides something in some mode is lying.
 *
 * The mini player counts as fullscreen here, not as a window: it has no bar to
 * leave in place, only controls drawn over a 420 px picture that is usually
 * watched while the pointer is busy in another application — kept up, they
 * would cover a third of it for the whole film. `never` still keeps them.
 */
export type AutoHide = 'always' | 'fullscreen' | 'never';
export const AUTO_HIDE_CHOICES: AutoHide[] = ['always', 'fullscreen', 'never'];

/**
 * How long the pointer has to rest before the controls (and, with them, the
 * cursor) go, in milliseconds. Named values rather than a slider, as with the
 * seek steps: nobody is after 1.7 s, and five pills are quicker to hit. 1.2 s
 * is the player's own long-standing value and stays the default; the short end
 * is for a picture watched with the mouse in hand, the long end for somebody
 * who reads the readouts.
 *
 * `0` is "at once": the controls are up while the mouse moves and go the moment
 * it stops. It is not a zero timer — `pokeUi` floors it, see `STOP_MS` there.
 */
export const HIDE_DELAY_CHOICES = [0, 800, 1200, 2000, 3000, 5000] as const;
export type HideDelay = (typeof HIDE_DELAY_CHOICES)[number];

/**
 * When the cursor hides, asked separately from the controls because the two
 * answers are wanted apart: a bar that stays in the window (`AutoHide` at
 * `never` or `fullscreen`) over a picture the pointer should still get off.
 *
 * - `controls` — the default: the cursor goes exactly when the chrome does, so
 *   it follows `autoHide` wherever that is set.
 * - `always` — on the same idle timer whether or not the chrome is allowed to
 *   fade. Never while the pointer rests on a bar, a dialog is open or the
 *   seekbar is held: the pointer disappearing from under a control about to be
 *   clicked is a bug in either mode.
 * - `never` — the cursor stays.
 */
export type CursorHide = 'controls' | 'always' | 'never';
export const CURSOR_HIDE_CHOICES: CursorHide[] = ['controls', 'always', 'never'];

/**
 * What a click on the picture does.
 *
 * - `both` — the default: a click pauses, a double click goes fullscreen. The
 *   two are told apart without waiting (see `onVideoClick`), and the price of
 *   that is a pause that blinks under a double click.
 * - `fullscreen` — only the double click means anything; pausing is left to
 *   Space and the controls. Asked for by a viewer whose double clicks kept
 *   landing as pauses, and the honest answer to anyone who finds the blink
 *   under a double click too much.
 * - `pause` — only the click means anything; fullscreen is left to its key and
 *   its button.
 */
export type VideoClick = 'both' | 'fullscreen' | 'pause';
export const VIDEO_CLICK_CHOICES: VideoClick[] = ['both', 'fullscreen', 'pause'];

class WindowPrefs {
  /// Remember the geometry between runs. Off by default while restore is buggy.
  remember = $state(false);
  /// The window has the picture's shape: it takes it when a file opens and
  /// keeps it while it is resized by hand — see `initWindowShape`.
  fitToVideo = $state(true);
  alwaysOnTop = $state(false);
  /// Magnetic edges for the mini window. On by default: a corner window is put
  /// where it is meant to be out of the way, and pixel-accurate placement is
  /// not what anyone is trying to do with it.
  snapMini = $state(true);
  /// When the chrome may fade out on its own — see `AutoHide`.
  autoHide = $state<AutoHide>('always');
  /// How long the pointer rests before they do — see `HideDelay`.
  hideDelay = $state<HideDelay>(1200);
  /// When the cursor hides — see `CursorHide`.
  cursorHide = $state<CursorHide>('controls');
  /// What a trackpad pinch does (macOS) — see `PinchAction` in window-fit.
  pinch = $state<PinchAction>('resize');
  /// What a click on the picture does — see `VideoClick`.
  videoClick = $state<VideoClick>('both');
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
  // The minimum is about to be written from here, whichever way this goes.
  holding = '';
  if (mini.on) {
    const back = beforeMini;
    try {
      // First, so the awaited window calls below act as a barrier: they travel
      // the same main-thread queue, so by the time they resolve the window can
      // be made fullscreen again — which `toggleFullscreen` does immediately
      // after leaving mini.
      await floatOverFullscreen(false);
      // Logical, as the config states it. This read `PhysicalSize` once, which
      // on a Retina screen is half of it: one visit to the mini player left an
      // ordinary window that could be dragged down to 240×160.
      await win.setMinSize(new LogicalSize(MIN_WINDOW_W, MIN_WINDOW_H));
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

    await win.setMinSize(new LogicalSize(MINI_MIN_W, MINI_MIN_H));
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
      await win.setMinSize(new LogicalSize(MIN_WINDOW_W, MIN_WINDOW_H));
    } catch {
      // nothing further to do
    }
  }
}

/// How close to a resting place the window has to be dragged, in logical
/// pixels, for it to be taken the rest of the way.
const SNAP_DIST = 56;

/// How often the mouse button is asked about once the window stops moving.
///
/// A native drag runs the system's own loop and never tells the webview it has
/// ended, so this used to snap once the window had sat still for 300ms — and a
/// hand that paused mid-drag had the window pulled to an edge from under it.
/// Now "stopped moving" only starts the question the drag cannot answer itself:
/// is the button still down (`primary_button_down`)? The snap waits for no,
/// and asks often enough that letting go reads as the cause of it. A move with
/// no button behind it — the system placing the window, the glide itself —
/// finds it up on the first ask and settles as it always did.
const SNAP_POLL_MS = 60;

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

/// Called for every move event; the work happens once the window has been let go.
export function scheduleMiniSnap() {
  if (!mini.on || !windowPrefs.snapMini) return;
  clearTimeout(snapTimer);
  snapTimer = setTimeout(() => void snapOnRelease(), SNAP_POLL_MS);
}

async function snapOnRelease() {
  // A failed ask counts as released: the old behaviour rather than none.
  const held = await invoke<boolean>('primary_button_down').catch(() => false);
  if (held) {
    snapTimer = setTimeout(() => void snapOnRelease(), SNAP_POLL_MS);
    return;
  }
  await snapMiniToEdges();
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
    const v = typeof saved.v === 'number' ? saved.v : 0;
    // Each of the two is read only from a payload written while its default
    // was the one it has now — see `PREFS_VERSION`.
    if (v >= 2 && typeof saved.remember === 'boolean') windowPrefs.remember = saved.remember;
    if (v >= 3 && typeof saved.fitToVideo === 'boolean') windowPrefs.fitToVideo = saved.fitToVideo;
    if (typeof saved.alwaysOnTop === 'boolean') windowPrefs.alwaysOnTop = saved.alwaysOnTop;
    if (typeof saved.snapMini === 'boolean') windowPrefs.snapMini = saved.snapMini;
    if (saved.autoHide && AUTO_HIDE_CHOICES.includes(saved.autoHide)) windowPrefs.autoHide = saved.autoHide;
    const delay = HIDE_DELAY_CHOICES.find((d) => d === saved.hideDelay);
    // `!== undefined`, not truthiness: 0 is one of the choices.
    if (delay !== undefined) windowPrefs.hideDelay = delay;
    if (saved.cursorHide && CURSOR_HIDE_CHOICES.includes(saved.cursorHide)) {
      windowPrefs.cursorHide = saved.cursorHide;
    }
    if (saved.pinch && PINCH_CHOICES.includes(saved.pinch)) windowPrefs.pinch = saved.pinch;
    if (saved.videoClick && VIDEO_CLICK_CHOICES.includes(saved.videoClick)) {
      windowPrefs.videoClick = saved.videoClick;
    }
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
        autoHide: windowPrefs.autoHide,
        hideDelay: windowPrefs.hideDelay,
        cursorHide: windowPrefs.cursorHide,
        pinch: windowPrefs.pinch,
        videoClick: windowPrefs.videoClick,
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
 *
 * **Waited on with the `resize` event, never with animation frames.** It used to
 * poll with a double `requestAnimationFrame`, and a hidden window renders no
 * frames at all — so with "remember size and position" on, this never
 * returned, the window stayed hidden until the Rust safety net showed it three
 * seconds in (lib.rs, `fallback.show()`), and only then did frames and the rest
 * of startup resume. That was the whole of the slow launch. `resize` is
 * dispatched whether or not anything is painted, and a timer bounds the wait
 * for the case where the size was already right or never becomes it.
 */
async function settleLayout(targetW: number, targetH: number) {
  const dpr = window.devicePixelRatio || 1;
  // Compare with a tolerance: CSS pixels are rounded, and the outer window
  // size includes a frame we have exactly as much of as we do not.
  const settled = () =>
    Math.abs(window.innerWidth * dpr - targetW) <= 4 * dpr &&
    Math.abs(window.innerHeight * dpr - targetH) <= 48 * dpr;
  if (settled()) return;
  await new Promise<void>((resolve) => {
    const done = () => {
      clearTimeout(timer);
      window.removeEventListener('resize', onResize);
      resolve();
    };
    const onResize = () => {
      if (settled()) done();
    };
    const timer = setTimeout(done, 400);
    window.addEventListener('resize', onResize);
  });
}

// ---- The window's shape ---------------------------------------------------
//
// "Match video aspect ratio" is a standing constraint, not an event. It used to
// be one resize when a file opened, after which any edge could be dragged into
// a shape the picture does not have — the setting still ticked, over a window
// showing black bars. Three things keep it true now, and `adoptShape` is the
// one place that does them, in an order that is load-bearing:
//
//   1. the window is **fitted** once per new shape (`fitToShape`), around its
//      own center and over `MORPH_MS` rather than in one jump;
//   2. its **minimum** becomes the smallest size *of that shape*;
//   3. the platform is told to **hold** the shape while an edge is dragged
//      (`window_shape_lock`: `contentAspectRatio` on macOS, `WM_SIZING` on
//      Windows).
//
// Maximizing, fullscreen and the system's snap layouts are left alone: each is
// a shape asked for by name, and bars there are what was asked for.

/// The shape the picture had last, and for which file.
///
/// `dwidth`/`dheight` are reported *unavailable* whenever the VO reconfigures —
/// on a torrent, every time playback stalls for pieces — and a window that let
/// go of its shape for those frames could be caught mid-drag without one. So a
/// shape only ever moves to a real value while the file is the same, the rule
/// the hover preview's box already follows; a new file starts with none, or an
/// audio track would inherit the window of the film before it.
let known: { path: string | null; shape: Size } | null = null;

function currentShape(): Size | null {
  // A size is not yet a picture. `force-window` gives the VO a 960×540 field
  // of its own to paint black — before the first frame of every file, and for
  // the whole of a file that has no picture — and `dwidth`/`dheight` report
  // that field as readily as a film (measured: an audio file reads 960×540).
  // What says a picture exists is the decoder, which `sourceTransfer` mirrors;
  // taking the size alone reshaped the window twice on opening anything that
  // is not 16:9, once for the field and once for the film.
  const now =
    player.sourceTransfer !== null
      ? pictureShape(player.videoW, player.videoH, player.voRotate)
      : null;
  if (now) {
    known = { path: player.filePath, shape: now };
    return now;
  }
  return known && player.hasFile && known.path === player.filePath ? known.shape : null;
}

/// The shape the window was last fitted to. Fitting is once per shape: the next
/// episode at the same shape must not undo a size chosen during the previous
/// one. It is written once a fit is past standing down and about to move the
/// window — it used to be written when one was asked for, so a file that
/// changed in fullscreen was marked as fitted by a fit that had stood down, and
/// leaving fullscreen showed the new picture in the old one's window. Not as
/// late as the landing, either: whatever looks again while the window is in
/// the air would start a second fit from the middle of the first.
let fittedFor = '';

/// What the last fit left the window at, and the area it was aiming for. If the
/// window is still that size the next fit aims for the same area again, rather
/// than for whatever the screen or the minimum left of it: read from the window
/// each time, one upright clip on a small screen took the area down and every
/// film after it opened in the smaller window.
let lastFit: { w: number; h: number; area: number } | null = null;

class ShapeState {
  /// The window is on its way to a new frame, moved from here. The shell reads
  /// it: the bars cannot follow a frame that moves under them — see
  /// `chrome.unsteady`.
  gliding = $state(false);
}

export const shapeState = new ShapeState();

/// How many fits are on their way to the size in `lastFit`. While one is, the
/// window is between two sizes and is nobody's choice: a fit that starts then
/// aims for the same area as the one it interrupts.
let fitsInFlight = 0;

/// How long the window takes to get from one shape to the next. A window that
/// jumps reads as a glitch — the eye has nothing to attribute the new frame
/// to — and one that takes longer than this is in the way of the film that
/// has already started.
const MORPH_MS = 240;

/// What the page knows and this module may not import: the shell imports
/// *this*, and the start screen is the page's own.
export interface ShapeHooks {
  /// The window is in a mode that owns its size — fullscreen or maximized.
  sizeOwned: () => boolean;
  /// The start screen is up (the page's debounced flag, not `!hasFile`, which
  /// blinks between two entries of a playlist).
  resting: () => boolean;
  /// Fullscreen alone, for the pinch: `sizeOwned` also counts a maximized
  /// window, which on macOS is any window at the zoomed frame — one a pinch
  /// reaches by itself and has to be able to leave (see `PinchMoment`).
  fullscreen?: () => boolean;
  /// Something fills the window that cannot be taken away for a resize — a
  /// dialog with its backdrop, the casting screen. Read by the pinch alone:
  /// a fit happens under those too, since a new picture has to have its
  /// window whatever is over it.
  covered?: () => boolean;
  /// The picture is in its window, or will be in `ms`: a fit is about to move
  /// the window and takes that long, or none was needed and it is zero. Said
  /// whatever the setting is — a picture with the setting off is in its window
  /// the moment it exists. What lifts the dark between two files.
  settled?: (ms: number) => void;
}

let hooks: ShapeHooks = { sizeOwned: () => false, resting: () => true };

/// What the platform was last told — the shape held and the minimum — so that
/// the effect re-running for a reason that changes neither costs no round
/// trip. Starts as the window is created: nothing held, the config's minimum.
/// Emptied by anything else that writes the minimum.
let holding = holdKey(null, { w: MIN_WINDOW_W, h: MIN_WINDOW_H });

function holdKey(held: Size | null, floor: Size): string {
  return `${held ? shapeKey(held) : 'free'} over ${shapeKey(floor)}`;
}

const shapeRuns = latest();

/// `dwidth`, `dheight` and the rotation arrive as three events, and a fit
/// started on the first would be for a shape that never existed (1080×1080, on
/// the way from a film to an upright clip). mpv sends them together and they
/// land within a few milliseconds of each other; the rest of this is margin,
/// and it is kept short because the new picture is already on screen while it
/// runs, standing in the old one's window.
const SHAPE_SETTLE_MS = 40;

/// A fit finds the window still fullscreen while it is on its way out — the
/// mirror flips before the transition and the system's answer after it. Asked
/// again this often, this many times: the macOS animation is about 0.7 s.
const FIT_RETRY_MS = 250;
const FIT_RETRIES = 12;

let shapeTimer: ReturnType<typeof setTimeout> | undefined;

/**
 * Keep the window in the picture's shape for as long as the setting is on.
 *
 * A standing effect rather than a call on `dwidth`: the 1 s sweep repairs a
 * mirror without telling anyone (`resyncState` skips the `property` hook), so
 * anything hanging off the event misses exactly the change that was lost once
 * already.
 */
export function initWindowShape(page: ShapeHooks) {
  hooks = page;
  $effect(() => {
    // Read for the dependency, all of them: the setting, the picture, the
    // start screen, and the two modes — leaving one is when a fit that stood
    // down gets its turn, and the minimum is the mode's while mini is on.
    void windowPrefs.fitToVideo;
    void currentShape();
    void page.resting();
    void page.sizeOwned();
    void mini.on;
    scheduleShape();
    return () => clearTimeout(shapeTimer);
  });
  // What a pinch does, told to the native side whenever the answer changes.
  // Decided here rather than in the monitor that takes the gesture because
  // half of the answer is the shell's — the setting, the two modes, the start
  // screen, a dialog — and the other half (⌥ on the event) is read there.
  // macOS only: nowhere else does anything native see a pinch as a pinch.
  $effect(() => {
    if (!IS_MAC) return;
    const resize = pinchResizes({
      setting: windowPrefs.pinch,
      fullscreen: page.fullscreen?.() ?? page.sizeOwned(),
      resting: page.resting(),
      mini: mini.on,
      covered: page.covered?.() ?? false,
    });
    void invoke('window_pinch_mode', { resize }).catch(() => {});
  });
}

/**
 * Something the window's shape depends on has changed; look again shortly.
 *
 * The state is read when the timer fires rather than when it was set, so the
 * three events of one file change are one look at the file they add up to.
 * Exported for the tests, which are compiled the way a server build is and
 * have no effects to run.
 */
export function scheduleShape() {
  clearTimeout(shapeTimer);
  shapeTimer = setTimeout(() => {
    const resting = hooks.resting();
    // On the start screen the window is free, and whatever shape it is given
    // there is not the picture's: the next file is fitted even at the shape
    // of the last one.
    if (resting) fittedFor = '';
    const picture = resting ? null : currentShape();
    void adoptShape(windowPrefs.fitToVideo ? picture : null, picture !== null);
  }, SHAPE_SETTLE_MS);
}

async function adoptShape(shape: Size | null, picture: boolean, tries = 0) {
  const run = shapeRuns.begin();
  try {
    let moving = false;
    if (shape && shapeKey(shape) !== fittedFor) {
      const outcome = await fitToShape(shape, null, run, true);
      if (run.stale) return;
      moving = outcome === 'done';
      // Stood down with nothing of ours owning the size: a transition is in
      // flight. Anything else that stood it down ends with this effect
      // running again.
      if (outcome === 'away' && !mini.on && !hooks.sizeOwned() && tries < FIT_RETRIES) {
        shapeTimer = setTimeout(() => void adoptShape(shape, picture, tries + 1), FIT_RETRY_MS);
      }
    }
    // A fit that moved the window has said so itself, at the moment it began.
    if (picture && !moving) hooks.settled?.(0);
    await holdShape(shape, run);
  } catch (e) {
    console.warn('adoptShape failed:', e);
  }
}

/// The window, its screen and what is left of one around the other — all in
/// physical pixels.
async function measure(win: ReturnType<typeof getCurrentWindow>) {
  // Together rather than in turn: they are five reads of one window, and the
  // new picture waits in the old one's shape for as long as they take.
  const [mon, dpr, outer, inner, at] = await Promise.all([
    currentMonitor(),
    win.scaleFactor(),
    win.outerSize(),
    win.innerSize(),
    win.outerPosition(),
  ]);
  // Our chrome (title bar and OSC are drawn over the video) takes no height,
  // but the window frame does: take outer minus inner size.
  const frame = { w: outer.width - inner.width, h: outer.height - inner.height };
  const now: Rect = { x: at.x, y: at.y, w: outer.width, h: outer.height };
  // workArea, not size: it already excludes the Dock and the taskbar, whereas
  // the old "95% and 90% of the screen" was eyeballed guesswork — it undershot
  // on a monitor with a Dock on the left and overshot without one.
  const pad = SCREEN_PADDING * dpr;
  const work = mon?.workArea;
  const area: Rect | null = work
    ? { x: work.position.x, y: work.position.y, w: work.size.width, h: work.size.height }
    : null;
  const room = area ? { w: area.w - pad * 2 - frame.w, h: area.h - pad * 2 - frame.h } : null;
  // The lower bound is in PHYSICAL pixels, i.e. the config minimum times the
  // screen scale. Comparing a logical minimum against a physical size directly
  // does not work: on Retina (dpr = 2), "no smaller than 480" became 240
  // logical — half the real minimum — and vertical video at 50% shrank the
  // window until the bottom controls were cut off.
  const min = { w: MIN_WINDOW_W * dpr, h: MIN_WINDOW_H * dpr };
  return { dpr, inner, now, frame, pad, area, room, min };
}

/**
 * Take the window to a frame — position and size as one change, over time.
 *
 * Through the window API the two are separate calls, each queued on its own,
 * and nothing makes them land in one frame: the window grows from its corner
 * and is then pulled back. `window_frame_glide` sets both in one main-thread
 * turn per step. Where it cannot (it answers `false`: no way to on this
 * platform, or no window), the two calls are what is left — a jump, to the
 * right place. A glide that something newer took over — a later glide, a
 * pinch — answers `true`: the window is that one's to finish, and sending the
 * two calls after it would pull the window out of the viewer's fingers.
 */
/// Somebody who asked the system for less motion did not mean "except here".
function calm(): boolean {
  return (
    typeof window !== 'undefined' &&
    window.matchMedia?.('(prefers-reduced-motion: reduce)').matches === true
  );
}

async function moveFrame(win: ReturnType<typeof getCurrentWindow>, to: Rect, run: Attempt) {
  const landed = await invoke<boolean>('window_frame_glide', {
    x: to.x,
    y: to.y,
    width: to.w,
    height: to.h,
    ms: calm() ? 0 : MORPH_MS,
  }).catch(() => false);
  if (landed || run.stale) return;
  await win.setSize(new PhysicalSize(to.w, to.h));
  await win.setPosition(new PhysicalPosition(to.x, to.y));
}

/**
 * The minimum size and the resize constraint that go with a shape, or with
 * none.
 *
 * The minimum goes through tao like every other minimum here, so that it has
 * one owner: set natively, `contentMinSize` outranks the `minSize` tao writes,
 * and the mini player's own minimum would then be silently ignored — the
 * window that will not shrink, with nothing to say why.
 *
 * In the mini player the minimum stays the mode's. Its floor exists for the
 * seek row's width, and tao grows a window that is under a minimum it is
 * given (`set_min_inner_size`): raised to the floor of an upright clip's
 * shape, 240×427, it would stretch a 420×236 corner window on the spot.
 */
async function holdShape(shape: Size | null, run: Attempt) {
  const win = getCurrentWindow();
  let held = shape;
  let floor: Size = mini.on ? { w: MINI_MIN_W, h: MINI_MIN_H } : { w: MIN_WINDOW_W, h: MIN_WINDOW_H };
  if (shape && !mini.on) {
    const { dpr, room } = await measure(win);
    if (run.stale) return;
    const wanted = floorForShape(shape, floor);
    if (shapeFits(wanted, room && { w: room.w / dpr, h: room.h / dpr })) floor = wanted;
    // The shape cannot be had on this screen at a size the controls fit in,
    // and a constraint would hold the window to one it cannot take.
    else held = null;
  }
  const key = holdKey(held, floor);
  if (key === holding) return;
  // Unknown until both have landed: a run that goes stale between the two has
  // told the platform half of it.
  holding = '';
  await win.setMinSize(new LogicalSize(floor.w, floor.h));
  if (run.stale) return;
  await invoke('window_shape_lock', { width: held?.w ?? 0, height: held?.h ?? 0 });
  if (!run.stale) holding = key;
}

type FitOutcome = 'done' | 'away' | 'stale' | 'failed';

/**
 * Give the window the picture's shape.
 * `scale` is a fraction of the video's natural size (1 = pixel for pixel), or
 * null to keep the window's area and change only the shape.
 */
async function fitToShape(
  shape: Size,
  scale: number | null,
  run: Attempt,
  announce = false,
): Promise<FitOutcome> {
  // Mini is a size the viewer asked for; a new video's aspect must not undo it.
  if (mini.on) return 'away';
  const win = getCurrentWindow();
  try {
    if ((await win.isFullscreen()) || (await win.isMaximized())) return 'away';
    const { dpr, inner, now, frame, pad, area, room, min } = await measure(win);
    if (run.stale) return 'stale';

    let wanted: number;
    if (scale === null) {
      // Same area, new aspect — the window does not jump in size.
      const untouched =
        lastFit !== null &&
        (fitsInFlight > 0 ||
          (Math.abs(inner.width - lastFit.w) <= 2 && Math.abs(inner.height - lastFit.h) <= 2));
      wanted = untouched && lastFit ? lastFit.area : inner.width * inner.height;
    } else {
      wanted = shape.w * scale * dpr * (shape.h * scale * dpr);
    }
    const fit = fitWindow({ shape, area: wanted, min, room });
    // Around the window's own center, and inside the screen: a window grows
    // from its top-left corner if it is only told a size, which throws the
    // picture sideways and, near the right or bottom edge, off the screen.
    const to = placeAround(now, { w: fit.w + frame.w, h: fit.h + frame.h }, area, pad);

    // The minimum is still the previous shape's — 480×853 after an upright
    // clip, which no 16:9 size at this area is over. macOS takes a size under
    // the minimum as given (measured: `setContentSize` does not answer to
    // `minSize`); a Win32 window answers `SetWindowPos` with its tracking
    // minimum, the silent clamp the mini player was written around. So the
    // minimum goes down first and up to the new shape's once the size has
    // landed (`holdShape`) — raised before it, tao would grow the window to
    // meet it and the fit would then shrink it again, two jumps for one.
    holding = '';
    await win.setMinSize(new LogicalSize(MIN_WINDOW_W, MIN_WINDOW_H));
    if (run.stale) return 'stale';
    // Written before the window moves, not after it has: a fit that starts
    // while this one is in the air has to know what it was aiming for.
    lastFit = { w: fit.w, h: fit.h, area: wanted };
    fittedFor = shapeKey(shape);
    fitsInFlight++;
    shapeState.gliding = true;
    // Now, not once it has landed: whatever is waiting for the picture to have
    // its window takes its time from the way there.
    if (announce) hooks.settled?.(calm() ? 0 : MORPH_MS);
    try {
      await moveFrame(win, to, run);
    } finally {
      fitsInFlight--;
      if (fitsInFlight === 0) shapeState.gliding = false;
    }
    if (run.stale) return 'stale';
    void captureGeometry();
    return 'done';
  } catch (e) {
    console.warn('fitToShape failed:', e);
    return 'failed';
  }
}

/// The sizes in the window menu: 50 %, 100 %, 200 % of the picture.
export async function fitWindowToVideo(scale: number) {
  const shape = currentShape();
  if (!shape) return;
  const run = shapeRuns.begin();
  const outcome = await fitToShape(shape, scale, run);
  // The fit left the minimum at the window's own; what the setting adds to it
  // goes back on.
  if (outcome === 'done') {
    await holdShape(windowPrefs.fitToVideo ? shape : null, run).catch((e) =>
      console.warn('holdShape failed:', e),
    );
  }
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

export function setAutoHide(v: AutoHide) {
  windowPrefs.autoHide = v;
  saveWindowPrefs();
}

export function setHideDelay(v: HideDelay) {
  windowPrefs.hideDelay = v;
  saveWindowPrefs();
}

export function setCursorHide(v: CursorHide) {
  windowPrefs.cursorHide = v;
  saveWindowPrefs();
}

export function setPinch(v: PinchAction) {
  windowPrefs.pinch = v;
  saveWindowPrefs();
}

export function setVideoClick(v: VideoClick) {
  windowPrefs.videoClick = v;
  saveWindowPrefs();
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
    // Whichever way it went, the window has been anybody's since the last fit:
    // free to take any shape while the setting was off, and about to be. The
    // fit and the constraint themselves follow from the setting
    // (`initWindowShape`).
    fittedFor = '';
    showOsd(t(windowPrefs.fitToVideo ? 'osd.fit_on' : 'osd.fit_off'));
  }
}
