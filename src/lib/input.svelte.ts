/**
 * Everything the viewer does with a key, a wheel or a pointer, and where it
 * goes.
 *
 * Three surfaces, one rule between them: **while casting, the window is a
 * remote**. Playback keys drive the television, the wheel is its volume, and
 * the local-only concepts — the frame, the zoom, the loop, the speed, the
 * delays — refuse out loud rather than silently acting on a player nobody is
 * watching. Every one of those was a separate bug when the routing was decided
 * per call site.
 *
 * It needs no hooks into the page, and that is what the surrounding modules are
 * for: the overlay stack (`overlays`), the shell (`chrome`), the opening flow
 * (`open`) and the end-of-file offer (`endscreen`) all answer for themselves, so
 * this file only has to decide *who* is asked.
 */

import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { command } from 'tauri-plugin-libmpv-api';

import {
  cast,
  castAdvance,
  castNudgeVolume,
  castSeek,
  castSeekBy,
  castToggleMute,
  castTogglePause,
} from './cast.svelte';
import { chrome, exitFullscreen, pokeUi, toggleFullscreen } from './chrome.svelte';
import { inTextField } from './dom';
import { cancelAdvance, endOfFile, takeSkip } from './endscreen.svelte';
import { formatTime } from './format';
import { t } from './i18n.svelte';
import { actionFor, chordOf, isDigitJump, type ActionId } from './keys.svelte';
import { openFileDialog, openLinkDialog } from './open.svelte';
import { closeTopmost, dismissTopmostOnClick, overlays, toggleInfo } from './overlays.svelte';
import { showOsd } from './osd.svelte';
import {
  DELAY_STEP,
  changeSpeed,
  chapterTitle,
  clearAbLoop,
  cycleAbLoop,
  cycleLoop,
  jumpChapter,
  player,
  setVolume,
  toggleMute,
} from './player.svelte';
import { playEntry } from './playlist.svelte';
import { copyScreenshot, saveScreenshot } from './screenshot';
import { scrubBy, seekPercent, seekRelative } from './seek.svelte';
import { stepBy, togglePlayback } from './step-engine.svelte';
import { nudgeDelayHere } from './tracks.svelte';
import { mini, toggleMini } from './window-prefs.svelte';
import { isZoomed, panBy, resetZoom, zoomAt } from './zoom.svelte';
import { IS_MAC } from './platform';

/// The OSD every volume change raises. Here rather than in `osd.svelte.ts`
/// because it is the *volume*'s presentation, and both the wheel and the keys
/// want the same one.
export function osdVolume(v: number) {
  showOsd(t('osd.volume', { value: v }), { progress: Math.min(1, v / 100) });
}

/// Set once from the system rather than guessed: the cast click path waits it
/// out, and a constant would be wrong on a machine where the interval was
/// changed. 500 ms is the right default on both platforms if the call fails.
let doubleClickMs = 500;

export async function loadDoubleClickInterval() {
  const ms = await invoke<number>('double_click_time').catch(() => 0);
  if (ms > 0) doubleClickMs = ms;
}

/// A drag or a pan just ended, so the click that follows is its tail rather than
/// a press on the video. Cleared by the next pointerdown rather than by a timer:
/// a drag longer than the timer cleared it before the click arrived, and the
/// window toggled pause at the end of the drag.
let suppressNextClick = false;

export function armClickSuppression() {
  suppressNextClick = true;
}

export function clearClickSuppression() {
  suppressNextClick = false;
}

// Dragging the video area: without zoom it moves the window (5 px threshold, so
// a click stays a click); zoomed in, it pans the magnified region.
let videoDragStart: { x: number; y: number } | null = null;
let panning = false;
let panMoved = false;
let panLast: { x: number; y: number } | null = null;

/// The pending single-click toggle while casting, if any.
let castClickTimer: ReturnType<typeof setTimeout> | null = null;

export function onVideoPointerDown(e: PointerEvent) {
  if (e.button !== 0 || !player.hasFile) return;
  if (e.target !== e.currentTarget) return;
  if (isZoomed()) {
    panning = true;
    panMoved = false;
    panLast = { x: e.clientX, y: e.clientY };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    return;
  }
  if (chrome.fullscreen || chrome.isMaximized) return;
  videoDragStart = { x: e.clientX, y: e.clientY };
}

export function onVideoPointerMove(e: PointerEvent) {
  if (panning && panLast) {
    const mx = e.clientX - panLast.x;
    const my = e.clientY - panLast.y;
    panLast = { x: e.clientX, y: e.clientY };
    if (mx === 0 && my === 0) return;
    panMoved = true;
    panBy(mx, my);
    return;
  }
  if (!videoDragStart) return;
  const dx = e.clientX - videoDragStart.x;
  const dy = e.clientY - videoDragStart.y;
  if (dx * dx + dy * dy > 25) {
    videoDragStart = null;
    // The native drag loop swallows pointerup, but the click can still
    // arrive. The flag is cleared by the next pointerdown rather than by a
    // timer: a drag longer than the timer cleared it before the click
    // arrived, and the window toggled pause at the end of the drag.
    suppressNextClick = true;
    void getCurrentWindow().startDragging();
  }
}

export function onVideoPointerUp(e: PointerEvent) {
  if (panning) {
    panning = false;
    panLast = null;
    (e.currentTarget as HTMLElement).releasePointerCapture?.(e.pointerId);
    // panning must not toggle pause with a trailing click
    if (panMoved) suppressNextClick = true;
  }
  videoDragStart = null;
}

export function onVideoClick(e: MouseEvent) {
  if (suppressNextClick) {
    suppressNextClick = false;
    return;
  }
  // A click that dismisses a menu does not also pause: closing is what the
  // click was for.
  if (dismissTopmostOnClick()) return;
  if (e.target !== e.currentTarget) return;
  // Pause fires immediately, without waiting to see whether this is a double
  // click. Waiting for anything is not an option: the macOS double-click
  // threshold defaults to 500 ms, and any delay within it is exactly that
  // much delay on the player's most frequent action. The double click still
  // works: the browser sends click, click, dblclick, so the second click
  // cancels the first one's pause (togglePlayback goes through `cycle`, i.e. it
  // is idempotent in pairs) and dblclick is left to do fullscreen alone. The
  // old scheme with a 175 ms timer was worse twice over: both a delay and a
  // false trigger — 175 ms is below the system threshold, so an ordinary
  // double click toggled pause one extra time.
  //
  // A DELIBERATE TRADE-OFF, not an oversight. The trick is free only in the
  // "pause it" direction: the frame freezes and moves on, nothing is lost.
  // The other way round it is not: a double click on paused video really does
  // play 150-300 ms (the frame blinks, the audio clicks), and the position
  // after a pair of `cycle pause` does NOT come back — that is a start and a
  // stop, not an undo. So a double click on a paused video nudges the frame
  // forward. The alternative (waiting out the system threshold before
  // unpausing) was discussed and rejected: half a second of delay on every
  // "resume" costs more than an artifact on a double click. The instant path
  // without this ambiguity is Space and K. Do not "fix" without discussing.
  //
  // **While casting, that trade inverts and so does the code.** The canceling
  // trick relies on a pair of toggles costing nothing; on a television a
  // pause and a resume 100 ms apart is not a frozen frame but a decode
  // restart — two remote commands, a buffer flush and a visible hiccup. And
  // the delay that was too expensive locally is free here: the remote already
  // answers in a few hundred milliseconds and nobody frame-steps a TV. So the
  // click waits out the *system's own* double-click interval (never a guessed
  // constant — the reason `double_click_time` exists) and the double click
  // cancels it outright.
  if (cast.remote) {
    clearCastClick();
    castClickTimer = setTimeout(() => {
      castClickTimer = null;
      castTogglePause();
    }, doubleClickMs);
    return;
  }
  void togglePlayback();
}

/// The casting screen's click: the same deferral as the video area, without
/// the target check (the card is one box and every part of it is "the
/// picture" as far as the gesture is concerned).
export function onCastScreenClick() {
  if (dismissTopmostOnClick()) return;
  clearCastClick();
  castClickTimer = setTimeout(() => {
    castClickTimer = null;
    castTogglePause();
  }, doubleClickMs);
}

export function clearCastClick() {
  if (castClickTimer !== null) {
    clearTimeout(castClickTimer);
    castClickTimer = null;
  }
}

export function onContextMenu(e: MouseEvent) {
  if (inTextField(e)) return;
  e.preventDefault();
  pokeUi();
  // Only where it was asked for. Placing it — flipping, shifting and capping
  // it against the window — is the menu's own business and happens once it
  // has a size to measure, which is after it renders.
  overlays.ctxAt = { x: e.clientX, y: e.clientY };
}

export function onVideoDblClick() {
  // The pause was already undone by the second click — fullscreen only here.
  // Casting is the exception: there the first click is still pending, and
  // this is what stops it reaching the television at all.
  clearCastClick();
  void toggleFullscreen();
}

// Volume accumulates by delta rather than a fixed step per event: a trackpad
// fires dozens of tiny events per gesture, and a step for each drove the
// volume to the ceiling.
// The quantum is small (20 px = 1 unit) — otherwise the trackpad stays silent
// until a whole step accumulates, which feels like lag. A mouse wheel notch
// in Chromium is 100 px, so the familiar 5 units per notch is preserved.
const PX_PER_VOLUME_UNIT = 20;
// Seeking by horizontal gesture: 20 px = 1 s, so a wheel notch tilted
// sideways (100 px) gives the same 5 s as an arrow key.
const PX_PER_SEEK_SECOND = 20;
let wheelAccum = 0;
let wheelSeekAccum = 0;
// The axis is locked for the whole gesture. Otherwise diagonal finger motion
// flipped the axis from event to event and seeking bled into volume.
let wheelAxis: 'x' | 'y' | null = null;
let wheelAxisTimer: ReturnType<typeof setTimeout> | undefined;
const WHEEL_AXIS_RESET_MS = 400;

/// Surfaces that own their own wheel: menus, panels, dialogs and the start
/// and end screens. The player reads a vertical wheel as volume and a
/// horizontal one as a scrub, which is right over the video and wrong
/// everywhere else — scrolling the queue was changing the volume. One list
/// here rather than a stopPropagation on each surface, because that is a
/// list nobody sees until the one surface missing from it is found by hand.
const WHEEL_SURFACES = '.ctxmenu, .menu, .settings, .settings-backdrop, .overlay';

/**
 * The gesture is over — fingers lifted off the trackpad, which macOS reports
 * and Windows does not. Lets the axis be picked afresh without waiting out
 * `WHEEL_AXIS_RESET_MS`, so a swipe immediately after a scrub is not still
 * locked to the previous direction.
 */
export function resetWheelGesture() {
  wheelAxis = null;
  wheelAccum = 0;
  wheelSeekAccum = 0;
}

export function onWheel(e: WheelEvent) {
  const onSurface = !!(e.target as HTMLElement | null)?.closest(WHEEL_SURFACES);
  if (e.ctrlKey) {
    // Ctrl+wheel zooms the video around the point under the cursor. On a
    // trackpad this is the pinch gesture, whose sign the device sets itself —
    // "natural scrolling" does not flip it, so there is nothing to invert.
    // preventDefault regardless of where the pointer is: without it the
    // webview zooms the whole UI, which there is no way back from.
    e.preventDefault();
    if (player.hasFile && !onSurface && !cast.remote) zoomAt(e.clientX, e.clientY, e.deltaY < 0 ? 0.1 : -0.1);
    return;
  }
  if (onSurface) return;
  // deltaMode: 0 — pixels, 1 — lines, 2 — pages.
  const scale = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? 400 : 1;

  // While casting the wheel drives the TV: vertical is its volume (the
  // receiver's own 0..1), and there is no scrub — keyframe previews are a
  // local concept and every landing would cost the TV seconds of buffering.
  if (cast.remote) {
    const px = e.deltaY * scale;
    wheelAccum += IS_MAC ? px : -px;
    const steps = Math.trunc(wheelAccum / PX_PER_VOLUME_UNIT);
    if (!steps) return;
    wheelAccum -= steps * PX_PER_VOLUME_UNIT;
    castNudgeVolume(steps * 0.02);
    return;
  }

  // The axis is picked once at the start of the gesture and held to its end.
  clearTimeout(wheelAxisTimer);
  wheelAxisTimer = setTimeout(() => {
    wheelAxis = null;
    wheelAccum = 0;
    wheelSeekAccum = 0;
  }, WHEEL_AXIS_RESET_MS);
  if (wheelAxis === null) {
    const ax = Math.abs(e.deltaX);
    const ay = Math.abs(e.deltaY);
    // While the motion is too small, do not pick an axis: the first events of
    // a gesture are almost always noisy and can point the wrong way.
    if (Math.max(ax, ay) < 2) return;
    wheelAxis = ax > ay ? 'x' : 'y';
  }

  if (wheelAxis === 'x') {
    if (!player.hasFile || player.duration <= 0) return;
    // On macOS the sign is inverted for the same reason as for volume: the
    // system already applied "natural scrolling" to the event, so a rightward
    // gesture arrives as deltaX < 0 while it feels like "forward".
    wheelSeekAccum += IS_MAC ? -e.deltaX * scale : e.deltaX * scale;
    const secs = Math.trunc(wheelSeekAccum / PX_PER_SEEK_SECOND);
    if (!secs) return;
    wheelSeekAccum -= secs * PX_PER_SEEK_SECOND;
    scrubBy(secs);
    return;
  }

  const px = e.deltaY * scale;
  // macOS already applied the "natural scrolling" setting to the event, so an
  // upward gesture there is deltaY > 0 — the opposite sign to a Windows wheel.
  wheelAccum += IS_MAC ? px : -px;

  const steps = Math.trunc(wheelAccum / PX_PER_VOLUME_UNIT);
  if (!steps) return;
  wheelAccum -= steps * PX_PER_VOLUME_UNIT;
  osdVolume(setVolume(player.volume + steps));
}

/// Every hotkey action, by id. The chord that reached it is deliberately not
/// passed in: an action reading `e.shiftKey` would be two actions wearing one
/// name, which is exactly what an editor cannot express — "rebind screenshot"
/// would have to mean rebinding a key *and* a modifier convention. So the old
/// overloads (Shift+S, Shift+Z, Shift+A, the three things the arrows did) are
/// rows of their own, and this switch has no branches in it.
/// While casting the window is a remote: playback keys drive the TV, and
/// the local-only concepts — the frame, the zoom, the loop, the speed, the
/// delays — say so instead of silently acting on a player nobody is
/// watching. Returns true when the action was consumed.
function runCastAction(id: ActionId): boolean {
  switch (id) {
    case 'pause': castTogglePause(); return true;
    case 'seek_back': castSeekBy(-5); return true;
    case 'seek_fwd': castSeekBy(5); return true;
    case 'seek_back_precise': castSeekBy(-1); return true;
    case 'seek_fwd_precise': castSeekBy(1); return true;
    case 'seek_back_10': castSeekBy(-10); return true;
    case 'seek_fwd_10': castSeekBy(10); return true;
    case 'seek_start': castSeek(0); return true;
    case 'seek_end': castSeek(Math.max(0, cast.duration - 5)); return true;
    case 'volume_up': castNudgeVolume(0.05); return true;
    case 'volume_down': castNudgeVolume(-0.05); return true;
    case 'mute': castToggleMute(); return true;
    case 'chapter_prev':
    case 'chapter_next': {
      // The chapter list is local knowledge about the same file; the jump is
      // a remote seek, never mpv's `chapter` property — that would move the
      // paused local player out from under the session.
      const list = player.chapters;
      if (!list.length) return true;
      const current = list.filter((c) => c.time <= cast.time + 0.5).length - 1;
      const target = id === 'chapter_next' ? current + 1 : current - 1;
      if (target >= list.length) return true;
      const chapter = list[Math.max(0, target)];
      castSeek(chapter.time);
      showOsd(chapterTitle(chapter), { sub: formatTime(chapter.time) });
      return true;
    }
    case 'playlist_prev':
    case 'playlist_next':
      // The session follows the queue: the local player opens the neighbour
      // and the television is handed it, without dropping the session.
      void castAdvance(id === 'playlist_next' ? 1 : -1);
      return true;
    case 'frame_prev':
    case 'frame_next':
    case 'speed_down':
    case 'speed_up':
    case 'loop':
    case 'ab_loop':
    case 'ab_clear':
    case 'screenshot':
    case 'screenshot_subs':
    case 'copy_frame':
    case 'mini':
    case 'audio_delay_down':
    case 'audio_delay_up':
    case 'sub_delay_down':
    case 'sub_delay_up':
    case 'reset_zoom':
      showOsd(t('cast.not_while_casting'));
      return true;
    default:
      return false;
  }
}

export function runAction(id: ActionId) {
  // `remote`, not `active`: while the prepare rung runs, local playback is
  // still what the viewer sees, and the keys must keep driving it.
  if (cast.remote && runCastAction(id)) return;
  switch (id) {
    case 'pause': void togglePlayback(); break;
    case 'seek_back': seekRelative(-5); break;
    case 'seek_fwd': seekRelative(5); break;
    case 'seek_back_precise': seekRelative(-1, true); break;
    case 'seek_fwd_precise': seekRelative(1, true); break;
    case 'seek_back_10': seekRelative(-10); break;
    case 'seek_fwd_10': seekRelative(10); break;
    case 'seek_start': seekPercent(0); break;
    case 'seek_end': seekPercent(100); break;
    case 'frame_prev': void stepBy(-1); break;
    case 'frame_next': void stepBy(1); break;
    case 'chapter_prev': jumpChapter(-1); break;
    case 'chapter_next': jumpChapter(1); break;
    case 'playlist_prev': void command('playlist-prev', []); break;
    case 'playlist_next': void command('playlist-next', []); break;
    case 'speed_down': changeSpeed(1 / 1.25); break;
    case 'speed_up': changeSpeed(1.25); break;
    case 'loop': cycleLoop(); break;
    case 'ab_loop': cycleAbLoop(); break;
    case 'ab_clear': clearAbLoop(); break;
    case 'volume_up': osdVolume(setVolume(player.volume + 5)); break;
    case 'volume_down': osdVolume(setVolume(player.volume - 5)); break;
    case 'mute': toggleMute(); break;
    case 'audio_delay_down': nudgeDelayHere('audio', -DELAY_STEP); break;
    case 'audio_delay_up': nudgeDelayHere('audio', DELAY_STEP); break;
    case 'sub_delay_down': nudgeDelayHere('sub', -DELAY_STEP); break;
    case 'sub_delay_up': nudgeDelayHere('sub', DELAY_STEP); break;
    case 'fullscreen': void toggleFullscreen(); break;
    case 'mini': if (player.hasFile || mini.on) void toggleMini(); break;
    case 'info': toggleInfo(player.hasFile); break;
    case 'reset_zoom': resetZoom(); break;
    case 'open_file': void openFileDialog(); break;
    case 'open_link': void openLinkDialog(); break;
    case 'screenshot': void saveScreenshot(false); break;
    case 'screenshot_subs': void saveScreenshot(true); break;
    case 'copy_frame': void copyScreenshot(); break;
  }
}

/// Enter takes whatever the player is currently offering: the skip button
/// while it is up, otherwise the next entry on the end screen. Both are the
/// same gesture — "yes, that one" — and neither exists unless something is
/// actually being proposed, so Enter stays free the rest of the time rather
/// than becoming a second play/pause. Contextual in the same way Escape is,
/// and reserved for the same reason.
function takeOffer(e: KeyboardEvent) {
  // A focused button already answers to Enter by itself; acting here too
  // would fire two things at once.
  if (e.repeat || document.activeElement instanceof HTMLButtonElement) return;
  if (endOfFile.hint) {
    e.preventDefault();
    takeSkip();
  } else if (endOfFile.ended && endOfFile.next) {
    e.preventDefault();
    cancelAdvance();
    void playEntry(endOfFile.next);
  }
}

export function onKeydown(e: KeyboardEvent) {
  // e.code is the physical key, so hotkeys work in any keyboard layout — and
  // that is what the editor stores, so the property survives a rebind.
  const action = actionFor(chordOf(e));
  if (action) {
    if (e.repeat && action.noRepeat) return;
    // A bound chord belongs to the player, so the webview never gets its
    // default for it. That used to be a per-case decision and was wrong the
    // moment keys became movable: Ctrl+= is the webview's zoom whichever
    // action a viewer puts on it, and Space scrolls whatever it lands on.
    e.preventDefault();
    runAction(action.id);
    // `quiet` rides on the action rather than on the key, because "already
    // has a popup of its own" is a fact about what happens, not about which
    // button was pressed.
    if (!action.quiet) pokeUi();
    return;
  }
  // The reserved families, which have no row in the editor. Digits and Enter
  // are quiet; Escape closes something, and the OSC coming back is how the
  // player says it did.
  if (isDigitJump(e)) {
    seekPercent(Number(e.code.slice(5)) * 10);
    return;
  }
  if (e.code === 'Enter' || e.code === 'NumpadEnter') {
    takeOffer(e);
    return;
  }
  if (e.code === 'Escape') {
    // **The one reserved key that was not consuming its event**, and the only
    // one where that showed. A bound chord gets `preventDefault` unconditionally
    // (see the note in CLAUDE.md); Escape skipped it, so on macOS it was handled
    // twice — once here, closing the settings sheet, and once by AppKit, which
    // answers `cancelOperation:` in a fullscreen window by leaving fullscreen.
    // Both happened, so closing a dialog also dropped the window out of
    // fullscreen. Nothing showed on Windows, where the window is undecorated and
    // Escape means nothing to the system.
    //
    // Consuming it also keeps the no-dialog case ours: `exitFullscreen` is what
    // keeps mpv's mirror honest, and letting AppKit do it instead is the path
    // that leaves `isFullscreen()` disagreeing with the button (macos_chrome.rs).
    e.preventDefault();
    closeTopmost();
  }
  pokeUi();
}
