/**
 * The page's start-up and teardown: everything that used to be its `onMount`
 * and `onDestroy`, in the same order — the order is the point. What has to
 * happen before the window is shown (the files it was opened with, the
 * restored geometry, a complete start screen) comes first; mpv comes up next,
 * then the listeners. See the comments along the way for why each step sits
 * where it does.
 *
 * Out of the page's script. A plain module: it reads and writes the runes
 * modules' state but declares none of its own.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { join, resourceDir } from '@tauri-apps/api/path';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { command, destroy } from 'tauri-plugin-libmpv-api';
import { runMenuAction } from './app-menu';
import { playerHooks } from './bridge.svelte';
import { chrome, pokeUi, scheduleVeilRelease } from './chrome.svelte';
import { flushPosition, loadHistoryPrefs, loadRecent, maybeRecordPosition } from './history.svelte';
import { locale } from './i18n.svelte';
import {
  armClickSuppression,
  clearClickSuppression,
  initInput,
  loadDoubleClickInterval,
  onKeydown,
  resetWheelGesture,
} from './input.svelte';
import { opening, openResumeSnapshot, refreshTorrents } from './open.svelte';
import { dismissMenus, dismissOnOutsideClick } from './overlays.svelte';
import { IS_MAC } from './platform';
import { initPlayer, loadFiles, player, resyncState } from './player.svelte';
import { loadPlaylistPrefs } from './playlist.svelte';
import { initPortable } from './portable.svelte';
import { syncDebugLog } from './report.svelte';
import { primeResumeKnob, setPendingResume } from './resume.svelte';
import { loadSeekSteps } from './seek-steps.svelte';
import { initSeek, scheduleScrubEnd, seek } from './seek.svelte';
import { startScreen } from './start-screen.svelte';
import {
  cancelStep,
  flushStepThenCancel,
  schedulePrewarm,
  step,
  USE_STEP_ENGINE,
} from './step-engine.svelte';
import { initDeepLinks } from './sync/link.svelte';
import { loadThumbPrefs } from './thumbs.svelte';
import { clearTitleTimer } from './title.svelte';
import { hideTooltip, initTooltips } from './tooltip.svelte';
import { loadTorrentPrefs } from './torrent.svelte';
import { checkForUpdate } from './updater.svelte';
import { initUpscale } from './upscale.svelte';
import {
  applyAlwaysOnTop,
  loadWindowPrefs,
  restoreGeometry,
  scheduleGeometrySave,
  scheduleMiniSnap,
  syncMenuChecks,
} from './window-prefs.svelte';
import { markZoomLuaLoaded, reclampPan } from './zoom.svelte';

/// Everything that has to be torn down with the page.
const unlisteners: Array<() => void> = [];

/// Drains the "Open with" files buffered in Rust and opens them. Called from
/// several places (the signal, window focus), hence the re-entry guard: two
/// drains at once would split the list between them.
let drainingFiles = false;
async function drainPendingFiles() {
  if (drainingFiles) return;
  drainingFiles = true;
  try {
    const paths = await invoke<string[]>('take_pending_files').catch(() => []);
    if (paths.length) await loadFiles(paths);
  } finally {
    drainingFiles = false;
  }
}

export async function boot() {
  // The frame-stepper lives here, so the seek module reaches it through
  // hooks: a seek has to cancel a step in progress, and this keeps
  // seek.svelte.ts from knowing the FFmpeg sidecar exists.
  initSeek({
    stepMode: () => step.on,
    cancelStep,
    flushStepThenCancel,
    schedulePrewarm,
    useStepEngine: USE_STEP_ENGINE,
  });

  // The system's own double-click interval, used by the cast click path. Not
  // awaited and not on the critical path: the module's 500 ms default is right
  // on both platforms if the call is slow or fails.
  void loadDoubleClickInterval();
  // When the window was activated, for the click that did it (see
  // `activation` in input.svelte.ts).
  unlisteners.push(initInput());
  // Invitations to a shared viewing. Not awaited, for the same reason: a link
  // that opened the player is picked up by `getCurrent` inside, and the
  // dialog it raises is a decision the viewer has to make anyway — nothing
  // before the first frame depends on it.
  void initDeepLinks();
  // Whether the verbose log is on: `FP_CAST_DEBUG` may have turned it on
  // before the window existed, and the console ring follows it.
  void syncDebugLog();
  // "Open with" file and the post-update resume: both are learned BEFORE the
  // window is shown — if a file is on its way, the start screen must not
  // flash in front of the video (a black background holds until it loads).
  const initial = await invoke<string[]>('take_pending_files').catch(() => []);
  const resumeRaw = localStorage.getItem('frameplayer.resume');
  localStorage.removeItem('frameplayer.resume');
  if (initial.length || resumeRaw) {
    startScreen.showEmpty = false;
    // The OSC is on screen from here on, so prime it before mpv even starts.
    if (initial.length) primeResumeKnob(initial[0]);
    // safety net: the file may be gone or fail to open — bring the picker back.
    // Not while something is still being opened: a torrent restored after an
    // update has to start its session and re-add itself first, which routinely
    // takes longer than this, and the picker flashed in front of a video that
    // was on its way. Re-checked instead, until the attempt settles.
    const safetyNet = () => {
      if (player.filename !== null) return;
      if (opening.busy) {
        setTimeout(safetyNet, 1000);
        return;
      }
      startScreen.showEmpty = true;
      void loadRecent();
    };
    setTimeout(safetyNet, 5000);
  }

  // Geometry strictly before show(): restoring an already visible window
  // reads as a jump. Anything off-screen is caught by window_guard.
  loadHistoryPrefs();
  loadPlaylistPrefs();
  loadSeekSteps();
  loadThumbPrefs();
  loadTorrentPrefs();
  // **Awaited**, unlike everything else started here, and both are cheap: the
  // recents list settles after one batched `stat` (its posters keep filling in
  // afterwards on their own) and the torrent list is one directory read. What
  // it buys is the start screen being *complete* in its first frame. Started
  // un-awaited, the window appeared holding only the panel and the buttons and
  // then grew a rail and a torrent list under them a moment later — a reflow
  // on every launch, and the one moment where the player has nothing else to
  // do but be ready.
  if (startScreen.showEmpty) await Promise.all([loadRecent(), refreshTorrents()]);
  loadWindowPrefs();
  await restoreGeometry();
  void applyAlwaysOnTop();
  // The native menu is built in English and only learns the real language
  // here. The check marks are synced after that on purpose: a rebuild
  // replaces the items, so syncing first would leave them all unchecked.
  // Both commands hop to the main thread through the same queue, so they run
  // in the order they were sent.
  if (IS_MAC) {
    void invoke('set_menu_locale', { locale: locale() })
      .catch(() => {})
      .finally(() => syncMenuChecks());
  }

  // The window was created hidden (visible: false in tauri.conf.json) — show
  // it only now that the webview has painted a black background: no white
  // flash, no position jump, no see-through window.
  // ...and take focus straight away: after an update restart the new process
  // comes up behind whatever app the user switched to meanwhile, and the
  // player had to be dug out by hand.
  void getCurrentWindow()
    .show()
    .then(() => getCurrentWindow().setFocus())
    .catch(() => {});

  // mpv comes up here; the module owns the option surface and the mirrors,
  // and hands back everything that has to be torn down.
  unlisteners.push(
    await listen<number>('frameplayer://ytdlp-progress', (e) => {
      if (opening.ytdlpBusy) opening.ytdlpPct = e.payload;
    }),
  );

  unlisteners.push(
    ...(await initPlayer(playerHooks)),
  );
  if (player.initError) return;

  // Atomic zoom+pan: load-script at runtime (unlike the init option it
  // returns an error if the build has no Lua or the path is wrong)
  try {
    const luaPath = await join(await resourceDir(), 'lua', 'zoompan.lua');
    await command('load-script', [luaPath]);
    markZoomLuaLoaded();
  } catch (e) {
    console.warn('zoompan.lua not loaded, zoom falls back:', e);
  }
  initUpscale(await join(await resourceDir(), 'shaders'));

  // Position bookkeeping rides the same timer as the resync: it needs fresh
  // mirrors, and one interval is easier to reason about than two.
  const resyncTimer = setInterval(() => {
    void resyncState().then(() => maybeRecordPosition());
  }, 1000);
  unlisteners.push(() => clearInterval(resyncTimer));

  unlisteners.push(
    await getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === 'drop' && event.payload.paths.length > 0) {
        void loadFiles(event.payload.paths);
      }
    }),
  );

  // Which kind of copy this is, and — for a portable one — taking the shell
  // registration it has no installer to do for it. Not awaited: nothing on
  // screen waits for either answer.
  void initPortable();

  // update check: shortly after start and every 6 hours
  setTimeout(() => void checkForUpdate(), 3000);
  const updTimer = setInterval(() => void checkForUpdate(), 6 * 3600 * 1000);
  unlisteners.push(() => clearInterval(updTimer));

  const win = getCurrentWindow();
  chrome.isMaximized = await win.isMaximized().catch(() => false);
  unlisteners.push(
    await win.onResized(() => {
      void win
        .isMaximized()
        .then((m) => (chrome.isMaximized = m))
        .catch(() => {});
      // fullscreen can also change from outside — keep the mirror honest
      void win
        .isFullscreen()
        .then((f) => (chrome.fullscreen = f))
        .catch(() => {});
      // adaptive veil release: a little after the last resize event
      // (~2 frames, which is what mpv's child window needs to catch up)
      if (chrome.fsTransition) scheduleVeilRelease(60);
      // resizing the window changes the pan bounds
      reclampPan();
    }),
  );

  window.addEventListener('keydown', onKeydown);
  unlisteners.push(() => window.removeEventListener('keydown', onKeydown));

  // After a mouse click, do not leave focus on the button: a stray Enter
  // would "press" it again. Keyboard focus (Tab) is left alone — pointerup
  // only comes from a mouse or touch.
  const blurOnPointerUp = (e: PointerEvent) => {
    const btn = (e.target as HTMLElement | null)?.closest('button');
    if (btn) btn.blur();
  };
  window.addEventListener('pointerup', blurOnPointerUp);
  unlisteners.push(() => window.removeEventListener('pointerup', blurOnPointerUp));

  // Custom tooltips instead of native title — see tooltip.svelte.ts.
  unlisteners.push(initTooltips());

  // Playback position on window close (plus the periodic 5 s write)
  const saveOnUnload = () => flushPosition();
  window.addEventListener('beforeunload', saveOnUnload);
  unlisteners.push(() => window.removeEventListener('beforeunload', saveOnUnload));

  // Closing with a video open (Alt+F4, taskbar): the libmpv plugin
  // intercepts CloseRequested and destroys mpv FIRST, with the window still
  // on screen — the transparent window showed the desktop for the whole
  // fade-out. This listener races it: the API wrapper calls destroy() as
  // soon as the handler returns without preventDefault, and destroying the
  // window is milliseconds while the mpv teardown is tens of them — the
  // window gets cloaked while the last frame is still composed, so the
  // close animation keeps the video frame. mpv itself is reclaimed by the
  // process exit that follows Destroyed (lib.rs). The ✕ button does not
  // even get here — closeWindow() destroys directly, no race at all.
  unlisteners.push(
    await getCurrentWindow().onCloseRequested(() => {
      // destroy() skips beforeunload — flush the position by hand
      flushPosition();
    }),
  );

  pokeUi();

  // "Open with" files. The event is only a "look in the buffer" signal; the
  // list itself is fetched by a command (see deliver_files in lib.rs).
  unlisteners.push(
    await listen('frameplayer://open-file', () => {
      void drainPendingFiles();
    }),
  );
  // A second, independent trigger: opening a file from Finder or Explorer
  // always activates the app, and activation gives the webview focus. If the
  // signal failed to arrive for any reason, the file is still picked up here
  // — a single delivery channel is exactly why opening a second file in an
  // already running player silently did nothing.
  const onWindowFocus = () => void drainPendingFiles();
  window.addEventListener('focus', onWindowFocus);
  unlisteners.push(() => window.removeEventListener('focus', onWindowFocus));

  unlisteners.push(
    await listen<string>('frameplayer://menu', (e) => runMenuAction(e.payload)),
  );

  // Geometry persistence. onMoved/onResized fire for every pixel of a drag,
  // so the localStorage write is debounced.
  unlisteners.push(
    await getCurrentWindow().onMoved(() => {
      scheduleGeometrySave();
      scheduleMiniSnap();
    }),
  );
  unlisteners.push(await getCurrentWindow().onResized(() => scheduleGeometrySave()));
  void invoke('open_file_ready').catch(() => {});
  // Restore after an update restart: same file, same position with the
  // offset. An explicit file from the arguments takes priority.
  // (initial/resumeRaw were fetched at the top of onMount, before show.)
  if (initial.length) {
    void loadFiles(initial);
  } else if (resumeRaw) {
    try {
      const r = JSON.parse(resumeRaw) as { path: string; pos: number; paused: boolean };
      setPendingResume(r.pos, r.paused);
      void openResumeSnapshot(r.path).then((opened) => {
        if (!opened) {
          startScreen.showEmpty = true;
          void loadRecent();
        }
      });
    } catch {
      // corrupt entry — ignore
    }
  }

  // Menus close on a click anywhere outside them: capture phase, because
  // clicks on OSC/title-bar buttons are swallowed by their stopPropagation.
  const closeMenusOnOutsideClick = (e: PointerEvent) => {
    hideTooltip();
    // The earliest event of a gesture (capture on window), which is why the
    // previous flag is cleared here: a native drag loop may have swallowed
    // the click, and without a reset the suppression would leak into the next
    // press.
    clearClickSuppression();
    // This handler runs in the pointerdown capture phase, i.e. before the
    // click on the video. Without the flag, onVideoClick would see an already
    // closed menu, fail to recognize a "closing" click and toggle pause.
    if (dismissOnOutsideClick(e.target as HTMLElement | null)) armClickSuppression();
  };
  // Trackpad scroll gesture phase (macOS only, see macos_chrome.rs).
  unlisteners.push(
    await listen<boolean>('frameplayer://window-resizing', (e) => {
      chrome.resizing = e.payload;
      // Letting go of the edge counts as a mouse move, which the web view did
      // not see one of for the whole of the drag: the bars come back, and
      // from there the "hide the controls" setting has them again — its
      // timer runs from now, not from before the resize.
      if (!e.payload) pokeUi();
    }),
  );
  unlisteners.push(
    await listen<boolean>('frameplayer://scroll-phase', (e) => {
      seek.fingersDown = e.payload;
      if (!seek.fingersDown) {
        // Fingers lifted — the gesture is now allowed to end.
        scheduleScrubEnd();
        // ...and the axis may be picked afresh without waiting for the timeout.
        resetWheelGesture();
      }
    }),
  );

  window.addEventListener('pointerdown', closeMenusOnOutsideClick, true);
  unlisteners.push(() => window.removeEventListener('pointerdown', closeMenusOnOutsideClick, true));

  // A click outside the window — the desktop, another app — is outside the
  // menus too, and the pointerdown above never sees it. The window's own
  // `blur`, not an element's: no capture, so a focus moving between controls
  // inside the page does not reach it. The system file dialog takes the focus
  // as well, and that one is ours: "Add file…" leaves its menu open so the new
  // track shows up in the list.
  const closeMenusOnBlur = () => {
    if (chrome.fileDialogOpen) return;
    hideTooltip();
    dismissMenus();
  };
  window.addEventListener('blur', closeMenusOnBlur);
  unlisteners.push(() => window.removeEventListener('blur', closeMenusOnBlur));
}

export function shutdown() {
  for (const un of unlisteners) un();
  clearTitleTimer();
  if (player.ready) void destroy();
}
