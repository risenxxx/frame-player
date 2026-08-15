<script lang="ts">
  import { onDestroy, onMount, tick, type Snippet } from 'svelte';
  import { open } from '@tauri-apps/plugin-dialog';
  import { invoke } from '@tauri-apps/api/core';
  import { PhysicalPosition, PhysicalSize } from '@tauri-apps/api/dpi';
  import { emitTo, listen } from '@tauri-apps/api/event';
  import { join, resourceDir } from '@tauri-apps/api/path';
  import { currentMonitor, getCurrentWindow } from '@tauri-apps/api/window';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { ProgressBarStatus } from '@tauri-apps/api/window';
  import { openUrl, revealItemInDir } from '@tauri-apps/plugin-opener';
  import { relaunch } from '@tauri-apps/plugin-process';
  import { check, type Update } from '@tauri-apps/plugin-updater';
  import { command, destroy } from 'tauri-plugin-libmpv-api';
  import Dialog from '$lib/components/Dialog.svelte';
  import MediaInfoDialog from '$lib/components/MediaInfoDialog.svelte';
  import SubsDialog from '$lib/components/SubsDialog.svelte';
  import StartScreen from '$lib/components/StartScreen.svelte';
  import SettingsDialog from '$lib/components/SettingsDialog.svelte';
  import LicensesDialog from '$lib/components/LicensesDialog.svelte';
  import RoomDialog from '$lib/components/RoomDialog.svelte';
  import ContextMenu from '$lib/components/ContextMenu.svelte';
  import TopBar from '$lib/components/TopBar.svelte';
  import StepOverlay from '$lib/components/StepOverlay.svelte';
  import Tooltip from '$lib/components/Tooltip.svelte';
  import Osd from '$lib/components/Osd.svelte';
  import LoadingOverlay from '$lib/components/LoadingOverlay.svelte';
  import CastScreen from '$lib/components/CastScreen.svelte';
  import SkipButton from '$lib/components/SkipButton.svelte';
  import EndScreen from '$lib/components/EndScreen.svelte';
  import QueueMenu from '$lib/components/QueueMenu.svelte';
  import ChapterMenu from '$lib/components/ChapterMenu.svelte';
  import CastMenu from '$lib/components/CastMenu.svelte';
  import TrackMenu from '$lib/components/TrackMenu.svelte';
  import SeekBar from '$lib/components/SeekBar.svelte';
  import Controls from '$lib/components/Controls.svelte';
  import DiagnosisDialog from '$lib/components/DiagnosisDialog.svelte';
  import LinkDialog from '$lib/components/LinkDialog.svelte';
  import CatalogDialog from '$lib/components/CatalogDialog.svelte';
  import TorrentPickDialog from '$lib/components/TorrentPickDialog.svelte';
  import TorrentUpdateDialog from '$lib/components/TorrentUpdateDialog.svelte';
  import { blockContextMenu, inTextField } from '$lib/dom';
  import { initSync, syncNoteFileLoaded } from '$lib/sync/apply.svelte';
  import { initDeepLinks, invite } from '$lib/sync/link.svelte';
  import {
    initSeek,
    resetSeekProbe,
    scheduleScrubEnd,
    seek,
  } from '$lib/seek.svelte';
  import {
    USE_STEP_ENGINE,
    cancelStep,
    togglePlayback,
    flushStepThenCancel,
    schedulePrewarm,
    step,
  } from '$lib/step-engine.svelte';
  import {
    chrome,
    closeWindow,
    exitFullscreen,
    initChrome,
    minimizeWindow,
    onTitlebarMouseDown,
    pokeUi,
    scheduleVeilRelease,
    startResize,
    toggleFullscreen,
  } from '$lib/chrome.svelte';
  import { initStallWatch } from '$lib/stall.svelte';
  import { initSubShift, subShift } from '$lib/sub-shift.svelte';
  import {
    cancelAdvance,
    endOfFile,
    initEndScreen,
    noteLocalPosition,
    onReachedEnd,
    resetEnd,
    resetSkipGuard,
    takeSkip,
  } from '$lib/endscreen.svelte';
  import {
    abandonOpening,
    cancelLoadFailure,
    clearTorrentCache,
    deleteTorrent,
    deleteWatchedFiles,
    dropLink,
    openFileDialog,
    fixYtdlp,
    noteOpened,
    openLinkDialog,
    openRecent,
    openRememberedTorrent,
    openTorrent,
    openUpdateDialog,
    opening,
    pickTorrentFile,
    playTorrentFile,
    refreshTorrents,
    reportLoadFailure,
    resolveTitleIfMissing,
    submitLink,
    submitUpdate,
    togglePortForward,
    toggleSeeding,
  } from '$lib/open.svelte';
  import {
    catalog,
    checkTorrentUpdate,
    closeCatalog,
    openCatalog,
  } from '$lib/catalog.svelte';
  import {
    addTrackFile,
    applyDelays,
    initTracks,
    nudgeDelayHere,
    resetDelayHere,
    restoreTrackChoice,
    selectTrack,
  } from '$lib/tracks.svelte';
  import {
    clearCastClick,
    clearClickSuppression,
    armClickSuppression,
    loadDoubleClickInterval,
    onCastScreenClick,
    onContextMenu,
    onKeydown,
    onVideoClick,
    onVideoDblClick,
    onVideoPointerDown,
    onVideoPointerMove,
    onVideoPointerUp,
    onWheel,
    resetWheelGesture,
  } from '$lib/input.svelte';
  import {
    dismissOnOutsideClick,
    initOverlays,
    overlays,
    toggleInfo,
    toggleMenu,
  } from '$lib/overlays.svelte';
  import {
    applyResume,
    prepareResume,
    primeResumeKnob,
    resume,
    setPendingResume,
  } from '$lib/resume.svelte';
  import { checkForUpdate, installUpdate, updater } from '$lib/updater.svelte';
  import { openSubsDialog, removeSubtitle, subs } from '$lib/subs.svelte';
  import { LOOP_LABEL } from '$lib/player.svelte';
  import { playback } from '$lib/playback.svelte';
  import { flipAxis, shiftAxis } from '$lib/floating';
  import { locale, setLocale, t, type Locale } from '$lib/i18n.svelte';
  import {
    hint,
    withKey,
  } from '$lib/keys.svelte';
  import { osd, osdSeq, showOsd } from '$lib/osd.svelte';
  import {
    flushPosition,
    forgetRecent,
    history,
    loadHistoryPrefs,
    loadRecent,
    rememberTitle,
    maybeRecordPosition,
    type RecentItem,
  } from '$lib/history.svelte';
  import { IS_MAC } from '$lib/platform';
  import {
    chapterAt,
    cycleAbLoop,
    ytdlp,
    initPlayer,
    isNetworkSource,
    notePlaybackRestart,
    jumpChapter,
    loadFiles,
    player,
    resetAbLoop,
    resetPicture,
    resyncState,
    setPicture,
    cycleLoop,
    type ObservedName,
  } from '$lib/player.svelte';
  import {
    applyAlwaysOnTop,
    fitWindowToVideo,
    loadWindowPrefs,
    maybeFitWindow,
    mini,
    restoreGeometry,
    scheduleGeometrySave,
    scheduleMiniSnap,
    syncMenuChecks,
    exitMini,
    toggleMini,
    toggleWindowPref,
  } from '$lib/window-prefs.svelte';
  import {
    dropPosters,
    loadPlaylist,
    loadPlaylistPrefs,
    playlist,
    queueFolder,
  } from '$lib/playlist.svelte';
  import {
    attachTorrentSubtitles,
    releaseTorrent,
    torrent,
    rememberedTorrent,
    torrentPositions,
    torrentResume,
    watchedFiles,
    trackTorrentPlayback,
    loadTorrentPrefs,
  } from '$lib/torrent.svelte';
  import { copyScreenshot, saveScreenshot } from '$lib/screenshot';
  import {
    cast,
    diagnoseDevice,
    diagnosisText,
    type CheckLine,
    type TvDevice,
    castFollowing,
    endCast,
  } from '$lib/cast.svelte';
  import { parseTorrentUrl } from '$lib/source';
  import { maybeStartThumbs, requestThumb, thumbs } from '$lib/thumbs.svelte';
  import { isZoomed, markZoomLuaLoaded, panBy, reclampPan, resetZoom, zoomAt } from '$lib/zoom.svelte';

  // Sidecar frame stepping (StepEngine) is off: mpv's native steps give the
  // correct picture (proper HDR tone mapping) and are fast enough. The code
  // stays for a possible comeback behind an option.

  function runMenuAction(id: string) {
    switch (id) {
      case 'settings': overlays.settings = true; break;
      case 'open': void openFileDialog(); break;
      case 'open_link': void openLinkDialog(); break;
      case 'info': toggleInfo(player.hasFile); break;
      case 'mini': void toggleMini(); break;
      case 'chapter_prev': jumpChapter(-1); break;
      case 'chapter_next': jumpChapter(1); break;
      case 'reveal':
        if (player.filePath && !isNetworkSource(player.filePath)) {
          void revealItemInDir(player.filePath);
        }
        break;
      case 'fullscreen': void toggleFullscreen(); break;
      case 'win_remember': toggleWindowPref('remember'); break;
      case 'win_fit': toggleWindowPref('fitToVideo'); break;
      case 'win_ontop': toggleWindowPref('alwaysOnTop'); break;
      case 'win_snap': toggleWindowPref('snapMini'); break;
      case 'win_size_50': void fitWindowToVideo(0.5); break;
      case 'win_size_100': void fitWindowToVideo(1); break;
      case 'win_size_200': void fitWindowToVideo(2); break;
    }
  }

  /// mpv is up and paints its own area — until then a dark fill sits under the
  /// UI. Separate from showEmpty: launching with a file has to hide the start
  /// screen but not the background, or the transparent window shows the desktop.
  let videoReady = $state(false);

  /// Drops the dark fill. Driven by vo-configured (the mirror is fed by the
  /// property event and by resyncState — the initial event races the listener
  /// registration and can be lost), with playback-restart as the backstop: once
  /// playback started, mpv is definitely painting. This used to be a fixed
  /// delay after init(), which is a guess — init() resolves when libmpv is
  /// initialized, not when its VO has presented anything, and on a heavy 4K
  /// open the first present came later than the slack: the fill dropped while
  /// nobody painted and the desktop flashed through the transparent window.
  /// The double-rAF + delay waits from a correct anchor instead: a couple of
  /// frames for the VO to present the field it just configured. Erring long is
  /// invisible (our black over mpv's black field), erring short is the desktop.
  function armVideoReady() {
    if (videoReady) return;
    requestAnimationFrame(() =>
      requestAnimationFrame(() => setTimeout(() => (videoReady = true), 50)),
    );
  }
  $effect(() => {
    if (player.voConfigured) armVideoReady();
  });


  /// Put the picker on screen and give it something current to show. Reached
  /// two ways, and the difference is only *when*: on a debounce from the
  /// `filename` handler, because a playlist transition blanks the name for a
  /// moment and the picker must not flash through it; and immediately from
  /// `backToStart`, where there is nothing to wait and see about.
  function enterStartScreen() {
    clearTimeout(emptyTimer);
    showEmpty = true;
    void loadRecent();
    void refreshTorrents();
    barTitleText = '';
    titleSlide = '';
  }

  /// Close what is playing and return to the start screen.
  ///
  /// `stop` rather than `playlist-remove` or a flag of our own: it unloads the
  /// file *and* empties the playlist, which is what "back to the start screen"
  /// has to mean — leaving a queue behind would have the next episode start
  /// itself the moment anything nudged playback.
  async function backToStart() {
    // The picker goes up BEFORE mpv is told to stop, rather than 300 ms after
    // it already has. That debounce exists for a playlist transition, where the
    // blank is involuntary and momentary; here the destination is known, and
    // waiting it out left the player's own chrome standing over a stopped VO —
    // a black frame under a seekbar reading 00:00, which is what going back
    // used to look like. `.player.no-video` is opaque, so raising it first
    // covers the video and nothing in between is ever seen.
    //
    // The position is flushed first so the card this file is about to appear on
    // carries the moment it was actually left at, not the previous write.
    flushPosition();
    // Nothing is being opened any more, and saying so is this path's job: the
    // indicator goes up in `beforeLoad` and comes down on `file-loaded`, so a
    // load abandoned in between — a torrent still looking for peers, which is
    // exactly when somebody gives up on it — has nothing else to take it down.
    // The `stop` below does not: its `end-file` carries the reason `stop`, and
    // only `error` reaches the failure path.
    abandonOpening();
    // And the torrent is let go here rather than left to the `filename === null`
    // handler, for the same reason the picker is raised here: this path *knows*
    // it is leaving, while that one is inferring it from an event that may be
    // dropped (a queue overflow costs property changes — see the mirrors' 1 s
    // resync). Getting it wrong leaves the swarm running and the download
    // readout standing over the start screen, which is exactly what it looked
    // like. Idempotent: the handler's own call finds nothing left to release.
    if (torrent.info) void releaseTorrent();
    enterStartScreen();
    await command('stop').catch((e) => console.warn('stop failed:', e));
  }

  /// The name of the video being played, when mpv has none.
  ///
  /// mpv normally supplies it (yt-dlp resolves the page), so this is the
  /// fallback for the case the viewer actually sees: a link whose title never
  /// arrived, leaving a video id in the title bar.



  /// Chapter boundaries as percentages of the bar. The one at zero is dropped:
  /// containers write it for the first chapter almost every time, and a notch
  /// under the very end of the track reads as a rendering artifact rather than
  /// as a boundary.
  const chapterMarks = $derived(
    resume.barDuration > 0
      ? player.chapters
          .filter((c) => c.time > 0 && c.time < resume.barDuration)
          .map((c) => (c.time / resume.barDuration) * 100)
      : [],
  );
  /// The looping segment as a band on the bar. Drawn while only A is set too —
  /// a mark that appears the moment you press the key is what tells you the key
  /// did anything, and the band then grows with the playhead.
  const abRegion = $derived.by(() => {
    if (player.loopA === null || resume.barDuration <= 0) return null;
    const from = Math.max(0, Math.min(player.loopA, resume.barDuration));
    const to = Math.max(from, Math.min(player.loopB ?? player.timePos, resume.barDuration));
    // mpv only loops while the position is inside the segment; past B it keeps
    // the marks but stops acting on them, and re-arms when playback returns.
    // The band says which of the two it is, or it would be claiming that
    // something is looping when nothing is.
    const armed = player.loopB === null || player.timePos < player.loopB;
    return {
      left: (from / resume.barDuration) * 100,
      width: ((to - from) / resume.barDuration) * 100,
      armed,
    };
  });

  /// Whether the hover popup can show a frame at all. The storyboard is off for
  /// a stream (it would pull the whole thing again), so the popup must not
  /// reserve space for a picture that is never coming — an empty gray rectangle
  /// reads as a broken preview rather than as an absent one.
  /// The preview box's shape, and it **only ever moves to a real value**.
  ///
  /// `dwidth`/`dheight` are observed with `none`, so mpv reports them as
  /// *unavailable* whenever the VO reconfigures — which on a torrent happens
  /// every time playback stalls for pieces. Read directly, the box fell back to
  /// 16:9 for those frames and snapped back, which is the blink seen while
  /// hovering. Same rule as `media-title`, where a transient null must not erase
  /// what is already known: keep the last real one.
  let lastAspect = $state('16 / 9');
  $effect(() => {
    if (player.videoW > 0 && player.videoH > 0) {
      lastAspect = `${player.videoW} / ${player.videoH}`;
    }
  });
  const thumbAspect = $derived(lastAspect);

  /**
   * What a recents card says on hover.
   *
   * For a local file the full path, which is the useful thing a card cannot
   * show: it tells you *which copy*. For a torrent the path is a loopback URL —
   * `http://127.0.0.1:53864/t/<40 hex>/3/Silo.S03E04.mkv` — where the port is
   * this run's, the hash is unreadable and neither survives a restart. What
   * identifies the episode there is its name inside the torrent, and the
   * torrent's own name is the context worth adding.
   */
  function recentTip(item: RecentItem): string {
    const ref = parseTorrentUrl(item.path);
    if (!ref) return item.path;
    const file = decodeURIComponent(item.path.split('/').pop() ?? '');
    const known = rememberedTorrent(ref.infoHash);
    return known?.name ? `${known.name} · ${file}` : file;
  }

  /// Whether the hover popup has a frame to show. Read from the thumbnail
  /// module rather than from the source kind: a torrent earns previews part-way
  /// through playback, when its file finishes downloading, which no test of the
  /// URL could tell you.
  const hasThumbs = $derived(thumbs.available);

  const hoverChapter = $derived(
    seek.hoverTime !== null && player.hasChapters ? chapterAt(seek.hoverTime) : null,
  );

  const osdState = $derived(osd());
  const hasFile = $derived(player.hasFile);
  /// mpv's own title when it has one; failing that, the name the site gave us
  /// (`resolvedTitle`), and only then the tidied file name — which for a link
  /// is a video id and tells the viewer nothing.
  const displayTitle = $derived(
    player.mediaTitle ? player.displayTitle : (opening.resolvedTitle ?? player.displayTitle),
  );

  let unlisteners: Array<() => void> = [];
  /// Throttles the Windows taskbar progress bar to about once a second.
  let lastTaskbarUpdate = 0;
  // Debounced "empty" state: switching files in the playlist blanks filename
  // for a moment, and the file-picker screen must not flash. The debounce is
  // for that involuntary blank only — a deliberate close goes through
  // `enterStartScreen` at once, see `backToStart`.
  let showEmpty = $state(true);
  let tipEl = $state<HTMLDivElement | undefined>();
  // Custom tooltips: one delegated mechanism for every [data-tip]
  let tooltip = $state<{ text: string; pos: { x: number; y: number } | null } | null>(null);
  let tipTimer: ReturnType<typeof setTimeout> | undefined;
  let emptyTimer: ReturnType<typeof setTimeout> | undefined;
  // The video title (centered in the title bar) is driven by hand rather than
  // straight from displayTitle — the text only changes inside the slide.
  let barTitleText = $state('');
  // Slide phases: out (leaving) / prep (starting pose of the new one) / ''
  let titleSlide = $state('');
  let slideDir: 'next' | 'prev' = 'next';
  /// The playlist moved, so the next title change is a step through it rather
  /// than a file being opened.
  let playlistStepped = false;
  let slideTimer: ReturnType<typeof setTimeout> | undefined;

  /// The title slide says "the playlist moved" — one name leaves in the
  /// direction of travel and the next arrives from the other side. Opening a
  /// file is not that: there is nothing to push out of the way, and the slide
  /// both misreads as a playlist step and delays the name by a quarter second.
  /// So it only runs when the playlist actually stepped and there is an old
  /// name on screen to replace.
  function slideTitle() {
    clearTimeout(slideTimer);
    if (!playlistStepped || !barTitleText) {
      playlistStepped = false;
      titleSlide = '';
      barTitleText = displayTitle;
      return;
    }
    playlistStepped = false;
    const dir = slideDir;
    titleSlide = dir === 'next' ? 'out-next' : 'out-prev';
    slideTimer = setTimeout(() => {
      barTitleText = displayTitle;
      titleSlide = dir === 'next' ? 'prep-next' : 'prep-prev';
      requestAnimationFrame(() => {
        requestAnimationFrame(() => (titleSlide = ''));
      });
    }, 260);
  }

  onMount(async () => {
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
    // Invitations to a shared viewing. Not awaited, for the same reason: a link
    // that opened the player is picked up by `getCurrent` inside, and the
    // dialog it raises is a decision the viewer has to make anyway — nothing
    // before the first frame depends on it.
    void initDeepLinks();
    // "Open with" file and the post-update resume: both are learned BEFORE the
    // window is shown — if a file is on its way, the start screen must not
    // flash in front of the video (a black background holds until it loads).
    const initial = await invoke<string[]>('take_pending_files').catch(() => []);
    const resumeRaw = localStorage.getItem('frameplayer.resume');
    localStorage.removeItem('frameplayer.resume');
    if (initial.length || resumeRaw) {
      showEmpty = false;
      // The OSC is on screen from here on, so prime it before mpv even starts.
      if (initial.length) primeResumeKnob(initial[0]);
      // safety net: the file may be gone or fail to open — bring the picker back
      setTimeout(() => {
        if (player.filename === null) {
          showEmpty = true;
          void loadRecent();
        }
      }, 5000);
    }

    // Geometry strictly before show(): restoring an already visible window
    // reads as a jump. Anything off-screen is caught by window_guard.
    loadHistoryPrefs();
    loadPlaylistPrefs();
    loadTorrentPrefs();
    // **Awaited**, unlike everything else started here, and both are cheap: the
    // recents list settles after one batched `stat` (its posters keep filling in
    // afterwards on their own) and the torrent list is one directory read. What
    // it buys is the start screen being *complete* in its first frame. Started
    // un-awaited, the window appeared holding only the panel and the buttons and
    // then grew a rail and a torrent list under them a moment later — a reflow
    // on every launch, and the one moment where the player has nothing else to
    // do but be ready.
    if (showEmpty) await Promise.all([loadRecent(), refreshTorrents()]);
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
      ...(await initPlayer({
        beforeLoad: async (path) => {
          // Opening another file while casting ends the session first: mpv is
          // about to start playing locally, and two playbacks at once — one
          // here, one on the TV — is never what a viewer meant. The exception
          // is the session moving itself along the queue, which opens the next
          // episode on purpose and hands it over (`castFollowing`).
          if (cast.active && !castFollowing()) {
            await endCast({ osd: t('cast.stopped'), resumeLocal: false });
          }
          cancelStep();
          opening.attempting = path;
          cancelLoadFailure();
          opening.busy = isNetworkSource(path);
          // Primes the knob and points mpv's `start` at the resume position,
          // which is why this hook is awaited: `start` is a load-time option and
          // has to land before `loadfile`.
          await prepareResume(path);
        },
        property: onPlayerProperty,
        fileLoaded: onFileLoaded,
        // Opening ONE file queues the rest of its folder; opening several is an
        // explicit selection and is left exactly as it was given.
        filesOpened: (videos) => {
          if (videos.length === 1) void queueFolder(videos[0]);
          else void loadPlaylist();
        },
        loadFailed: reportLoadFailure,
        openTorrentFile: (path) => void openTorrent(path),
        playbackRestart: () => {
          armVideoReady();
          notePlaybackRestart();
        },
      })),
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

    // Custom tooltips instead of native title: delegated via [data-tip]
    //
    // **A tooltip is cleared by the pointer not being over an anchor, never by
    // being told it left one.** `pointerout` is not delivered when the hovered
    // node is *removed* (measured, both engines), and the transition from the
    // start screen into playback removes anchors under a stationary cursor by
    // the handful — the torrent file picker being the one that gets noticed.
    // Relying on that event alone left a tooltip nothing could dismiss: moving
    // the mouse over the video hit the early return below, so only a click
    // (which clears it in the pointerdown capture handler) or landing on
    // another anchor would take it away.
    const onTipOver = (e: PointerEvent) => {
      const el = (e.target as HTMLElement | null)?.closest('[data-tip]') as HTMLElement | null;
      const text = el?.getAttribute('data-tip');
      if (!el || !text) {
        // The pointer is over something that has no tooltip, which is the
        // authoritative "nothing to show" — including the pointerover the
        // browser re-dispatches for whatever is revealed when an element under
        // the cursor disappears.
        clearTimeout(tipTimer);
        tooltip = null;
        return;
      }
      clearTimeout(tipTimer);
      tipTimer = setTimeout(async () => {
        // The anchor may have been removed during the wait, and a detached
        // element's rect is **all zeros** (measured) — which `flipAxis` and
        // `shiftAxis` then faithfully place in the top-left corner of the
        // window, where it sat with no anchor left to send a pointerout.
        if (!el.isConnected) {
          tooltip = null;
          return;
        }
        // Rendered unplaced first, then measured, then moved — all before
        // paint. Measuring has to happen at the window's origin, because a box
        // positioned with `left` has only the space to its right to lay out
        // in: a tip near the right edge wrapped to one word per line, and the
        // clamp that followed then read that wrapped width and called it a
        // fit. It is placed with a transform for the same reason — a transform
        // moves the box after layout instead of deciding it.
        tooltip = { text, pos: null };
        await tick();
        // `tooltip` already null means something else cleared it — a click, or
        // the pointer leaving — and this pass has simply been overtaken.
        if (!tipEl || !tooltip) return;
        // Checked again on this side of the await, because `tick()` is exactly
        // when a pending unmount flushes: the anchor can be connected when the
        // timer fires and gone one line later.
        if (!el.isConnected) {
          tooltip = null;
          return;
        }
        const r = el.getBoundingClientRect();
        const w = tipEl.offsetWidth;
        const h = tipEl.offsetHeight;
        // Above by default, which is right for anything in a bar: the tip
        // appears over the picture rather than over the next control. An
        // element opts out with `data-tip-below` when the thing it names is
        // *above* it and would be covered — a recents card, where the tip
        // belongs to the poster it would otherwise hide. Only the preference
        // moves; flipAxis still puts the tip on whichever side has the room.
        const v = flipAxis({
          near: r.top,
          far: r.bottom,
          size: h,
          limit: window.innerHeight,
          gap: 7,
          preferBefore: !el.hasAttribute('data-tip-below'),
        });
        tooltip = {
          text,
          pos: { x: shiftAxis(r.left + r.width / 2 - w / 2, w, window.innerWidth), y: v.pos },
        };
      }, 450);
    };
    const onTipOut = (e: PointerEvent) => {
      if (!(e.target as HTMLElement | null)?.closest('[data-tip]')) return;
      clearTimeout(tipTimer);
      tooltip = null;
    };
    window.addEventListener('pointerover', onTipOver);
    window.addEventListener('pointerout', onTipOut);
    unlisteners.push(() => {
      window.removeEventListener('pointerover', onTipOver);
      window.removeEventListener('pointerout', onTipOut);
    });

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
        void loadFile(r.path);
      } catch {
        // corrupt entry — ignore
      }
    }

    // Menus close on a click anywhere outside them: capture phase, because
    // clicks on OSC/title-bar buttons are swallowed by their stopPropagation.
    const closeMenusOnOutsideClick = (e: PointerEvent) => {
      clearTimeout(tipTimer);
      tooltip = null;
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
  });

  onDestroy(() => {
    for (const un of unlisteners) un();
    clearTimeout(titleTimer);
    if (player.ready) void destroy();
  });

  /// Debounced on purpose: opening a file changes the title two or three times
  /// in quick succession (cleared, then the filename, then the container's own
  /// title), and each one is a native title-bar relayout on the main thread
  /// while it is already busy opening the file. One call after things settle is
  /// all the window menu and the Dock ever needed.
  ///
  /// This used to matter far more — every setTitle knocked the macOS traffic
  /// lights back to their default spots and the correction was visible. That is
  /// handled structurally now (macos_chrome.rs), so the debounce is a courtesy,
  /// not a workaround.
  let titleTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const next = hasFile ? `${displayTitle} — Frame Player` : 'Frame Player';
    clearTimeout(titleTimer);
    titleTimer = setTimeout(() => {
      void getCurrentWindow().setTitle(next).catch(() => {});
    }, 250);
  });

  // ---- Bridge between mpv state and the page ------------------------------

  /// Previous playlist position, kept because the slide direction is decided by
  /// comparing against it — and by the time this runs the mirror is already the
  /// new value.
  let lastPlaylistPos = 0;

  /// Reactions to mpv property changes that are not the player module's own
  /// business: the seekbar knob, the taskbar, the start screen, the title
  /// slide. Runs after the mirror is updated, so `player.x` is already fresh.
  function onPlayerProperty(name: ObservedName) {
    switch (name) {
      case 'pause':
        if (player.paused && USE_STEP_ENGINE && !step.on) schedulePrewarm();
        updateTaskbarProgress();
        if (player.paused) maybeRecordPosition(true);
        break;
      case 'time-pos':
        // While a gesture owns the position, the gesture sets the knob.
        // Otherwise it jitters: mpv still reports the old position (a seek is
        // in flight) or one that landed on a keyframe, and the knob bounces
        // between the finger and the lagging value.
        if (!seek.dragging && !step.on && !seek.scrubbing && !seek.settling) seek.value = player.timePos;
        // Releases the skip button's mute and pre-decodes the next poster. In
        // the `property` hook rather than an $effect for the usual reason: both
        // must run on a position report, not whenever one of the values they
        // read happens to change.
        noteLocalPosition();
        if (performance.now() - lastTaskbarUpdate > 1000) updateTaskbarProgress();
        break;
      case 'duration':
        maybeStartThumbs();
        break;
      case 'filename':
        clearTimeout(emptyTimer);
        if (player.filename === null) {
          updateTaskbarProgress();
          // the file is gone (switched or closed) — commit the last position
          flushPosition();
          dropPosters();
          void exitMini();
          // Nothing is playing, so nothing may still be talking to a swarm:
          // uploading to strangers after the film is closed is not something a
          // video player gets to do quietly. The pieces stay on disk, so
          // re-opening the magnet continues rather than starting over.
          if (torrent.info) void releaseTorrent();
          // Not when the picker is already up: `backToStart` raises it before
          // issuing the stop, and re-running this would rebuild the recents
          // list a second time — visible, because the cards would reshuffle
          // under the cursor as the posters arrive again.
          if (!showEmpty) emptyTimer = setTimeout(enterStartScreen, 300);
        } else {
          showEmpty = false;
          // the pause lets media-title update before the slide
          setTimeout(() => slideTitle(), 120);
        }
        break;
      case 'eof-reached':
        if (player.eofReached) onReachedEnd();
        else resetEnd();
        break;
      case 'playlist-count':
        void loadPlaylist();
        break;
      case 'media-title':
        // Free: yt-dlp resolved it, mpv passed it on. Recording it here is what
        // keeps the recent-links list readable without asking anyone anything.
        if (player.mediaTitle && player.filePath) rememberTitle(player.filePath, player.mediaTitle);
        break;
      case 'path':
        resetSeekProbe();
        resetSkipGuard();
        maybeStartThumbs();
        break;
      case 'playlist-pos':
        if (player.playlistPos !== lastPlaylistPos) {
          slideDir = player.playlistPos > lastPlaylistPos ? 'next' : 'prev';
          // Consumed by the next slideTitle: mpv reports the new position
          // before the new filename, so the flag is always set in time.
          playlistStepped = true;
          lastPlaylistPos = player.playlistPos;
        }
        break;
      // dwidth/dheight arrive separately, so the window fit hangs off both and
      // guards against repeats: recomputing it on every event (including a
      // track change at the same resolution) is pointless.
      case 'dwidth':
      case 'dheight':
        maybeFitWindow();
        break;
    }
  }

  function onFileLoaded() {
    noteOpened();
    resetZoom(true);
    // Rotation and aspect are global mpv options: a phone clip turned upright
    // would still be turned for the next episode. Loop points and track delays
    // likewise.
    resetPicture();
    resetAbLoop();
    applyDelays();
    // Subtitles the torrent itself carries. `sub-auto` finds nothing here —
    // there is no local file to look beside — so without this they sit in the
    // torrent unreachable while the viewer searches OpenSubtitles for something
    // already on their own disk.
    void attachTorrentSubtitles();
    restoreTrackChoice();
    resetEnd();
    void loadPlaylist();
    resolveTitleIfMissing();
    // Releases the knob, clears mpv's sticky `start`, and seeks only if the
    // load did not already begin at the resume point.
    applyResume();
    // Last, and deliberately: it reads the title and the duration, which are
    // what the room shows, and both are worth more once the rest has run.
    void syncNoteFileLoaded();
  }

  // Poll a torrent's peers and rate while one is playing, and stop the moment
  // it is not. Driven by `path` inside the module rather than started when the
  // magnet was opened, because moving to the next episode of a torrent is a
  // playlist transition nothing else would report.
  // The callback is what breaks the one import cycle `src/` had: a completed
  // torrent file is an ordinary file on disk, so the seekbar can have previews —
  // but the torrent client has no business knowing that a thumbnail service
  // exists. `maybeStartThumbs` is idempotent, which this needs, since the poll
  // reports completion every second from then on.
  trackTorrentPlayback(maybeStartThumbs);

  // The window shell's standing effects — the cursor hide, the macOS traffic
  // lights, and the title bar's side measurement. Started from here rather than
  // left at the module's top level: see the note on `initChrome`.
  initChrome();
  // Lifts the subtitles clear of the control bar while it is up. Its own
  // effects rather than the chrome's: what it measures is the bar, but what it
  // writes is an mpv option, and the shell has no business knowing about those.
  initSubShift();
  // Releases the skip guard from the *television's* position: while casting,
  // mpv is paused and `noteLocalPosition` never fires.
  initEndScreen();
  // Keeps the window shell told what is over the video.
  initOverlays();
  // Measures how late this process's own timer runs, which is the only way to
  // tell a block of the whole machine from one inside mpv — see stall.svelte.ts.
  initStallWatch();
  // Restores the remembered audio/subtitle choice as the track list fills up —
  // a standing effect, not a one-shot, because external subtitles arrive after
  // `file-loaded`.
  initTracks();
  // Watching together: applies what arrives from the room, keeps this player
  // in step with it, and reports whether it is ready to be played to. Started
  // from here like every other module's standing effects, and for the same
  // reason — a `$effect` at a module's top level throws `effect_orphan`.
  initSync();
  // An invitation is *offered*, never obeyed: a custom scheme is a surface any
  // page on the internet can aim at, and a link that silently pulled a viewer
  // out of their film and into a stranger's room would be a far worse bargain
  // than one extra click. The dialog is also where the room server's address
  // is set, which a link naming an unfamiliar relay is exactly the reason to
  // look at.
  $effect(() => {
    if (invite.code) overlays.room = true;
  });

  async function loadFile(path: string) {
    await loadFiles([path]);
  }

  /// Save the current geometry. Called debounced from resize/move: writing to
  /// localStorage for every pixel of a drag is pointless.
  /// Fit for a new file — once per new resolution.
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

  function updateTaskbarProgress() {
    // On macOS this is a bar over the Dock icon — not done there, just noise.
    if (IS_MAC) return;
    lastTaskbarUpdate = performance.now();
    const win = getCurrentWindow();
    // **While casting the position lives on the television.** mpv sits paused
    // on the file it handed over, so reading its mirrors here pinned the bar to
    // the moment of the handoff and painted it "paused" for the whole session —
    // `player.paused` being true is what makes the handover work in the first
    // place. `playback` is the answer to that for every reader at once.
    const time = playback.position;
    const total = playback.duration;
    const paused = playback.paused;
    if (!hasFile || total <= 0) {
      void win.setProgressBar({ status: ProgressBarStatus.None }).catch(() => {});
      return;
    }
    void win
      .setProgressBar({
        status: paused ? ProgressBarStatus.Paused : ProgressBarStatus.Normal,
        progress: Math.min(100, Math.max(0, Math.round((time / total) * 100))),
      })
      .catch(() => {});
  }

  // ---- Frame export -------------------------------------------------------
  // mpv writes the file itself, so the saved frame is exactly what the VO
  // decoded — including HDR tone mapping, which is what made the StepEngine
  // canvas path unusable for this. `video` is the clean frame (no OSD, no
  // subtitles); `subtitles` burns them in, which is only worth offering when
  // subtitles are actually on.

  /// Settings for the temp file behind clipboard copy. It exists for a
  /// fraction of a second and reaches the clipboard as 8-bit RGBA regardless,
  /// so every byte of compression work is thrown away — and 16-bit output
  /// (mpv's default for a 10-bit source) doubles both the encode and the decode
  /// on the way back, for precision the clipboard cannot carry.
  let diagBusy = $state<string | null>(null);
  let diagOpen = $state(false);
  let diagDevice = $state<TvDevice | null>(null);
  let diagLines = $state<CheckLine[]>([]);

  async function runDiagnosis(device: TvDevice) {
    diagBusy = device.key;
    diagDevice = device;
    diagLines = [];
    // The dialog opens at once and fills in: the checks take a couple of
    // seconds against a device in standby, and a button that does nothing
    // visible for that long reads as a button that did nothing.
    diagOpen = true;
    overlays.menu = null;
    try {
      diagLines = await diagnoseDevice(device);
    } finally {
      diagBusy = null;
    }
  }

  async function copyDiagnosis(device: TvDevice) {
    // The webview's own clipboard: this is text, and the Rust path next door
    // exists for an image the clipboard cannot take any other way.
    try {
      await navigator.clipboard.writeText(diagnosisText(device, diagLines));
      showOsd(t('cast.diagnose_copied'));
    } catch (e) {
      console.warn('clipboard write failed:', e);
      showOsd(t('cast.diagnose_copy_failed'));
    }
  }
  const castStateLabel = $derived(
    cast.state === 'connecting' ? t('cast.state_connecting')
    : cast.state === 'preparing' ? t('cast.state_preparing')
    : cast.state === 'loading' || cast.state === 'buffering' ? t('cast.state_loading')
    : cast.state === 'paused' ? t('cast.state_paused')
    : t('cast.state_playing'),
  );

  /// The seekbar follows the TV while casting. Guarded on `seek.dragging` so the
  /// gesture owns the knob, exactly like the time-pos observer's guard — and
  /// safe as an $effect (which the observer guard is not) because on a drag
  /// release `cast.time` is already the optimistic release position, so the
  /// re-run cannot yank the knob backwards. `remote`, not `active`: while the
  /// prepare rung runs, local playback continues and time-pos owns the knob —
  /// two writers on one seekbar is the jitter bug in a new costume.
  $effect(() => {
    const time = cast.time;
    if (!cast.remote) return;
    if (!seek.dragging) seek.value = time;
    // **The taskbar has no other driver while casting.** It is refreshed from
    // mpv's property events, and with the local player parked `time-pos` never
    // arrives — so the bar was set once, at the handoff, and sat there for the
    // whole session. This poll is the only thing that knows the television
    // moved. Unthrottled on purpose: it runs at the cast poll's 2 Hz, which is
    // what the observer's 1 s throttle exists to cut a ~10 Hz local report
    // down to, and a skipped run would leave the bar the wrong colour across a
    // pause. Inside the `remote` guard for a reason that is easy to miss: the
    // update reads mpv's mirrors on the local branch, so calling it here
    // unconditionally would make this effect depend on `time-pos` and re-run
    // ten times a second all through ordinary playback. Leaving the session
    // hands the bar back to those same property events, which is where it
    // belongs — the handback's seek and unpause both fire one.
    updateTaskbarProgress();
  });

  /// Drop an entry. mpv renumbers what follows, so the list is re-read rather
  /// than patched — and removing the entry that is playing is allowed, because
  /// mpv handles it by moving on, which is what the viewer asked for.
  async function removeFromQueue(index: number) {
    await command('playlist-remove', [String(index)]).catch(() => {});
    void loadPlaylist();
  }

  // ---- Room for the title in the top bar ----------------------------------
  //
  // The title is centered in the WINDOW, not in what is left between the side
  // clusters, so both sides have to be reserved equally or it runs into the
  // narrower one first. On macOS that is the logo and the app name (the traffic
  // lights are the system's and live outside this DOM); on Windows it is the
  // brand on one side and the window buttons on the other.
  //
  // Measured rather than assumed, because no constant is right for long: the
  // right cluster grows by a whole button when an update is waiting, and the
  // app name is localised. Reserving 46vw and hoping left about 12px of gap at
  // a 480px window — which is what "the title is touching the logo" was.


</script>

<main
  class="player"
  class:mini={mini.on}
  class:idle-ui={chrome.idle}
  class:nocursor={chrome.cursorHidden}
  class:mac={IS_MAC}
  class:no-video={showEmpty}
  class:backdrop={!videoReady}
  style:--veil-color={showEmpty ? '#101016' : '#000'}
  onmousemove={pokeUi}
  onwheel={onWheel}
  ondblclick={onVideoDblClick}
  onclick={onVideoClick}
  oncontextmenu={onContextMenu}
  onpointerdown={onVideoPointerDown}
  onpointermove={onVideoPointerMove}
  onpointerup={onVideoPointerUp}
  role="presentation"
>
  {#if player.initError}
    <div class="overlay">
      <div class="panel">
        <h2>{t('error.mpv')}</h2>
        <p class="error">{player.initError}</p>
        <p>{t('error.mpv_hint')}</p>
      </div>
    </div>
  {:else if showEmpty}
    <StartScreen
      torrentRows={opening.rows}
      torrentTotal={opening.total}
      torrentBusy={opening.rowBusy}
      torrentOpening={opening.rowOpening}
      {torrentResume}
      onOpenFile={openFileDialog}
      onOpenLink={openLinkDialog}
      onOpenCatalog={catalog.enabled ? () => void openCatalog() : null}
      onOpenRecent={(item) => void openRecent(item)}
      onForgetRecent={(item) => forgetRecent(item.path)}
      onOpenTorrent={(row) => void openRememberedTorrent(row)}
      onUpdateTorrent={(known) => void checkTorrentUpdate(known)}
      onDeleteTorrent={(row) => void deleteTorrent(row)}
      onDeleteWatched={(row) => void deleteWatchedFiles(row)}
    />
  {:else if endOfFile.ended}
    <EndScreen
      prev={endOfFile.prev}
      next={endOfFile.next}
      counting={endOfFile.advancing}
      seq={endOfFile.seq}
      oncancel={cancelAdvance}
      onreplay={togglePlayback}
    />
  {/if}

  {#if opening.busy}
    <LoadingOverlay label={opening.label} torrentLabel={opening.torrentLabel} />
  {/if}

  <!-- The casting screen: while the TV plays, this window is a remote, and
       what it shows must never be the stale paused frame pretending to be
       playback — nor a hole to the desktop (gotcha 10), hence the opaque
       fill in its CSS rather than a bare overlay. Gated on `remote`, not
       `active`: during the prepare rung local playback deliberately keeps
       running, and covering a playing picture with a status card would read
       as the player dying for the length of the remux. -->
  {#if cast.remote}
    <CastScreen
    stateLabel={castStateLabel}
    title={displayTitle}
    onclick={onCastScreenClick}
    ondblclick={() => { clearCastClick(); void toggleFullscreen(); }}
  />
  {/if}

  <StepOverlay visible={step.on} pts={step.pts} bind:canvas={step.canvas} />

  <TopBar
    idle={chrome.idle}
    mini={mini.on}
    noVideo={showEmpty}
    fullscreen={chrome.fullscreen}
    barSide={chrome.barSide}
    {barTitleText}
    {titleSlide}
    bind:brandEl={chrome.brandEl}
    bind:chromeEl={chrome.chromeEl}
    updateAvail={updater.available}
    updatePct={updater.percent}
    torrentChip={opening.chip}
    torrentLabel={opening.torrentLabel}
    {onTitlebarMouseDown}
    onInstallUpdate={() => void installUpdate()}
    onMinimize={minimizeWindow}
    onClose={closeWindow}
    onToggleFullscreen={() => void toggleFullscreen()}
    onExitFullscreen={() => void exitFullscreen()}
    onBarHover={(over) => (chrome.barHover = over)}
    onOpenRoom={() => (overlays.room = true)}
    onChipHover={(over) => (chrome.chipHover = over)}
  />

  <!-- Windows only, and that is not a tidy-up: `drag_resize_window` is
       **unimplemented on macOS** — tao returns `NotSupported` and Tauri
       discards the error (`let _ = ...`), so these strips caught the press and
       then did nothing at all. Worse than nothing, in fact: they are 5px wide,
       sit inside the window above everything (`z-index: 50`) and draw their own
       resize cursors, so they took over the band AppKit uses for its own edge
       resizing and turned it into a dead zone that still looked live. The
       symptom was a resize that worked only when the pointer happened to land
       in the system's band outside the frame. macOS needs none of this: the
       window is `Titled | Resizable`, and AppKit resizes it from edges and
       corners the DOM cannot reach anyway. -->
  {#if !IS_MAC && !chrome.fullscreen && !chrome.isMaximized}
    <div class="rz rz-n" role="presentation" onpointerdown={() => startResize('North')}></div>
    <div class="rz rz-s" role="presentation" onpointerdown={() => startResize('South')}></div>
    <div class="rz rz-e" role="presentation" onpointerdown={() => startResize('East')}></div>
    <div class="rz rz-w" role="presentation" onpointerdown={() => startResize('West')}></div>
    <div class="rz rz-ne" role="presentation" onpointerdown={() => startResize('NorthEast')}></div>
    <div class="rz rz-nw" role="presentation" onpointerdown={() => startResize('NorthWest')}></div>
    <div class="rz rz-se" role="presentation" onpointerdown={() => startResize('SouthEast')}></div>
    <div class="rz rz-sw" role="presentation" onpointerdown={() => startResize('SouthWest')}></div>
  {/if}

    {#if overlays.ctxAt}
    <ContextMenu
      at={overlays.ctxAt}
      fullscreen={chrome.fullscreen}
      close={() => (overlays.ctxAt = null)}
      actions={{
        openFile: () => void openFileDialog(),
        openLink: () => void openLinkDialog(),
        backToStart: () => void backToStart(),
        toggleInfo: () => toggleInfo(player.hasFile),
        openSettings: () => (overlays.settings = true),
        openRoom: () => (overlays.room = true),
        toggleFullscreen: () => void toggleFullscreen(),
        cycleLoop,
        cycleAbLoop,
        jumpChapter,
        setPicture,
      }}
    />
  {/if}

  {#if diagOpen}
      <DiagnosisDialog
        device={diagDevice}
        lines={diagLines}
        busy={diagBusy}
        onclose={() => (diagOpen = false)}
        onCopy={(device) => void copyDiagnosis(device)}
      />
    {/if}

    {#if opening.linkOpen}
      <LinkDialog
        link={opening.box}
        ytdlpBusy={opening.ytdlpBusy}
        ytdlpPct={opening.ytdlpPct}
        onclose={() => (opening.linkOpen = false)}
        onSubmit={(url) => submitLink(url)}
        onForget={(url) => dropLink(url)}
        onPickTorrentFile={pickTorrentFile}
        onFixYtdlp={fixYtdlp}
      />
    {/if}

    <!-- The dialog leads with the *reason*, not the request. "Update" over an
         input box is mysterious; "BitTorrent cannot add a file to a torrent" is
         the fact that makes the whole errand make sense, and it is why the
         player cannot do this by itself. -->
    {#if opening.updateFor}
      {@const known = opening.updateFor}
      <TorrentUpdateDialog
        {known}
        suggested={opening.updateSuggested}
        busy={opening.updateBusy}
        error={opening.updateError}
        value={opening.updateValue}
        onValue={(v) => (opening.updateValue = v)}
        onclose={() => { if (!opening.updateBusy) opening.updateFor = null; }}
        onSubmit={() => void submitUpdate()}
        onOpenAsNew={(magnet) => { opening.updateFor = null; void openTorrent(magnet); }}
      />
    {/if}

    <!-- A torrent holding more than one video is a question only the viewer can
         answer. The rest still become queue entries, which is free: nothing is
         downloaded until mpv reads one (see torrent.rs). -->
    {#if opening.pick}
      {@const info = opening.pick}
      <TorrentPickDialog
        {info}
        positions={torrentPositions(info.info_hash)}
        finished={watchedFiles(info.info_hash)}
        onclose={() => (opening.pick = null)}
        onPick={(i, file) => void playTorrentFile(i, file)}
      />
    {/if}

    <!-- Above the start screen and below everything a chosen release raises:
         `playRelease` shuts this before handing the magnet over, so the torrent
         picker that follows never has to sit on top of it. -->
    {#if catalog.open}
      <CatalogDialog onclose={closeCatalog} />
    {/if}

    {#if subs.open}
      <SubsDialog />
    {/if}

    {#if overlays.info}
      <MediaInfoDialog onclose={() => (overlays.info = false)} />
    {/if}



  {#if overlays.room}
    <RoomDialog onclose={() => (overlays.room = false)} />
  {/if}

  {#if overlays.settings}
    <SettingsDialog
      onclose={() => (overlays.settings = false)}
      onToggleSeeding={() => void toggleSeeding()}
      onTogglePortForward={() => void togglePortForward()}
      onClearTorrentCache={() => void clearTorrentCache()}
      onLicenses={() => (overlays.licenses = true)}
    />
  {/if}

  <!-- Above the settings sheet rather than instead of it: closing the notices
       has to give the settings back, which is what the order in `closeTopmost`
       encodes. Rendered after, so it also paints on top. -->
  {#if overlays.licenses}
    <LicensesDialog onclose={() => (overlays.licenses = false)} />
  {/if}

  {#if tooltip}
    <Tooltip {tooltip} bind:el={tipEl} />
  {/if}

  <div class="veil" class:on={chrome.fsTransition}></div>

  {#if osdState}
    <Osd state={osdState} mini={mini.on} />
  {/if}

  <!-- Deliberately outside .osc: this is the one control that has to stay up
       while the UI is chrome.idle, since the whole point is to appear over video the
       viewer is not touching. Its offset is fixed rather than tied to the bar,
       so it does not hop when the OSC comes and goes. -->
  {#if endOfFile.hint}
    <SkipButton hint={endOfFile.hint} mini={mini.on} onskip={takeSkip} />
  {/if}

  <button
    class="minibtn"
    data-tip={withKey(t('ctx.mini_exit'), 'mini')}
    aria-label={t('ctx.mini_exit')}
    onclick={(e) => {
      e.stopPropagation();
      void toggleMini();
    }}
  >
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path fill="currentColor" d="M7 14H5v5h5v-2H7v-3zm-2-4h2V7h3V5H5v5zm12 7h-3v2h5v-5h-2v3zM14 5v2h3v3h2V5h-5z"/>
    </svg>
  </button>

  {#if !showEmpty}
  <!-- The click handler is a guard, not an affordance: it stops a click on the
       bar reaching the video's play/pause. There is nothing here to activate
       from the keyboard — every control inside is a real button, reachable on
       its own — so a keydown handler would add a focus stop that leads nowhere.
       `role="toolbar"` already says what this is. The directive has to be a
       comment of its own and start with the word: prose in front of it and it
       is not seen at all. -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="osc"
    class:hidden={chrome.idle}
    bind:this={subShift.oscEl}
    role="toolbar"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    ondblclick={(e) => e.stopPropagation()}
    onmouseenter={() => (chrome.oscHover = true)}
    onmouseleave={() => (chrome.oscHover = false)}
  >
    {#if overlays.menu === 'queue'}
      <QueueMenu
        close={() => (overlays.menu = null)}
        onRemove={(index) => void removeFromQueue(index)}
      />
    {:else if overlays.menu === 'chapter'}
      <ChapterMenu close={() => (overlays.menu = null)} />
    {:else if overlays.menu === 'cast'}
      <CastMenu
        close={() => (overlays.menu = null)}
        {diagBusy}
        onDiagnose={(device) => void runDiagnosis(device)}
      />
    {:else if overlays.trackMenu}
      <TrackMenu
        kind={overlays.trackMenu}
        close={() => (overlays.menu = null)}
        onSelect={(k, tr) => { overlays.menu = null; selectTrack(k, tr); }}
        onAddFile={(k) => void addTrackFile(k)}
        onNudgeDelay={nudgeDelayHere}
        onResetDelay={resetDelayHere}
      />
    {/if}

    <SeekBar
      barDuration={resume.barDuration}
      {chapterMarks}
      {abRegion}
      {hoverChapter}
      {hasThumbs}
      {thumbAspect}
      mini={mini.on}
    />
    <Controls
      mini={mini.on}
      fullscreen={chrome.fullscreen}
      openMenu={overlays.menu}
      onToggleMenu={toggleMenu}
      onCycleLoop={cycleLoop}
      onToggleFullscreen={() => void toggleFullscreen()}
    />
  </div>
  {/if}
</main>

<style>
  :global(html), :global(body) {
    margin: 0;
    padding: 0;
    height: 100%;
    background: transparent;
    overflow: hidden;
    font-family: 'Rubik', 'Segoe UI Variable', 'Segoe UI', system-ui, sans-serif;
    user-select: none;
    -webkit-user-select: none;
  }

  /* Fill for the time while mpv is not painting yet. Black, not the start
     screen tint: launching with a file (Open with) must bring the window up
     already looking like the player — it used to flash #101016 and snap to
     black mid-appearance. A separate class rather than no-video: that one
     also removes the title-bar gradient, which is not wanted here. */
  .player.backdrop {
    background: #000;
  }

  .player.nocursor {
    cursor: none;
  }

  .panel .error {
    color: #ff8a8a;
    word-break: break-word;
  }

  /* Form controls do not inherit the font by default (UA styles), which is why
     buttons were drawn in the system font instead of Rubik */
  .rz {
    position: fixed;
    z-index: 50;
  }

  .rz-n { top: 0; left: 10px; right: 10px; height: 5px; cursor: n-resize; }
  .rz-s { bottom: 0; left: 10px; right: 10px; height: 5px; cursor: s-resize; }
  .rz-e { right: 0; top: 10px; bottom: 10px; width: 5px; cursor: e-resize; }
  .rz-w { left: 0; top: 10px; bottom: 10px; width: 5px; cursor: w-resize; }
  .rz-ne { top: 0; right: 0; width: 10px; height: 10px; cursor: ne-resize; }
  .rz-nw { top: 0; left: 0; width: 10px; height: 10px; cursor: nw-resize; }
  .rz-se { bottom: 0; right: 0; width: 10px; height: 10px; cursor: se-resize; }
  .rz-sw { bottom: 0; left: 0; width: 10px; height: 10px; cursor: sw-resize; }

  /* Windows: the top-right corner belongs entirely to the window buttons — a
     flick of the cursor into the screen corner of an edge-snapped window must
     hit the close button, not a resize zone. Corner resizing is not lost: it
     stays on the invisible native rim (~7px) just outside the window edge.
     144px = three 48px buttons. */
  .player:not(.mac) .rz-n { right: 144px; }
  .player:not(.mac) .rz-e { top: 48px; }
  .player:not(.mac) .rz-ne { display: none; }

  /* No progress bar here, and that was tried: a full-width rule under the name
     reads as an underline rather than a value — at 91% it is indistinguishable
     from a border, and a list of them turns the panel into stripes. The card
     grid needs one because a poster leaves no room for words; here there are
     words, and "осталось 4:12" says the same thing without competing with the
     name it sits under. */

  /* ---- Content languages (ROADMAP 25) ---- */

  /* Above the entire UI: layout jumps during the transition stay invisible.
     Color: on the start screen — its background (so the transition does not
     blink black), with video — black; set via --veil-color on .player and
     mirrored to the shutter window. */
  .veil {
    position: absolute;
    inset: 0;
    background: var(--veil-color, #000);
    opacity: 0;
    pointer-events: none;
    z-index: 100;
    transition: opacity 0.15s ease;
  }

  /* Switching on is instant (no transition): by the time the resize happens
     the veil must be fully opaque; only the dissolve is animated */
  .veil.on {
    opacity: 1;
    transition: none;
  }

  .osc {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    /* No top padding: those 44px were pure gradient with no controls, yet they
       caught clicks (see .topbar). The gradient moved into ::before, which
       extends the same 44px upwards and receives no events. */
    padding: 0 24px 12px;
    /* Its own stacking context, so that z-index: -1 on ::before lowers the
       gradient under the controls without dropping it behind the whole bar. */
    isolation: isolate;
    transition: opacity 0.25s ease, transform 0.25s ease;
  }

  .osc::before {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    top: -44px;
    background: linear-gradient(to top, rgba(0, 0, 0, 0.72), rgba(0, 0, 0, 0.32) 55%, transparent);
    pointer-events: none;
    /* MANDATORY. By the painting rules an absolutely positioned ::before lands
       ABOVE ordinary in-flow children (.seekrow, .controls), and the gradient
       darkened the buttons — they looked disabled. This did not happen before,
       because the gradient was the bar's own background and backgrounds are
       painted first. The same scheme works on .topbar without z-index only
       because its children are themselves absolutely positioned and come after
       ::before in DOM order. */
    z-index: -1;
  }

  .osc.hidden {
    opacity: 0;
    transform: translateY(10px);
    pointer-events: none;
  }

  .player.mini .osc {
    padding: 0 10px 6px;
  }

  /* ---- Casting ------------------------------------------------------------ */

  .player {
    position: fixed;
    inset: 0;
    background: transparent;

    /* One shadow for everything that sits over the video: video title, app
       name, timecodes, logo. Weaker than the old one, which read as a black
       outline on bright frames. Changed in one place. */
    --ui-shadow:
      0 1px 2px rgba(0, 0, 0, 0.55),
      0 0 6px rgba(0, 0, 0, 0.3);
    /* The same for shapes: drop-shadow follows the alpha channel rather than
       the element box, so it traces the SVG logo and not its rectangle. */
    --ui-shadow-drop:
      drop-shadow(0 1px 2px rgba(0, 0, 0, 0.55))
      drop-shadow(0 0 6px rgba(0, 0, 0, 0.3));
  }

  /* Until a file is open the window is opaque: neither the desktop before mpv
     initializes nor the startup stages are visible. After .backdrop on
     purpose: the start screen keeps its tint while mpv is still warming up. */
  .player.no-video {
    background: #101016;
  }

  /* The way out has to be visible in the mode that hides everything else. */
  .player.mini .minibtn {
    display: grid;
  }

  /* Fades with the rest of the UI: in a corner window the picture is the point,
     and a button pinned over it forever is one more thing in the way. */
  .player.mini.idle-ui .minibtn {
    opacity: 0;
    pointer-events: none;
  }

  /* Round, and that is the whole reason: a rounded rectangle 8px inside the
     window's own rounded corner has to obey the concentric rule (inner radius
     = outer minus the gap) to look right, and the outer radius is not ours to
     know — macOS decides it, Windows 11 decides a different one, and both can
     change with the OS. A circle has no corner to disagree with, which is why
     the system's own corner controls (the traffic lights) are circles too. */
  .minibtn {
    display: none;
    position: absolute;
    top: 12px;
    right: 12px;
    z-index: 20;
    place-items: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 999px;
    background: rgba(16, 16, 22, 0.82);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.45);
    color: #d6d6de;
    cursor: pointer;
    transition: opacity 0.25s ease;
  }

  .minibtn svg {
    width: 14px;
    height: 14px;
  }

  .minibtn:hover {
    background: rgba(32, 32, 42, 0.9);
    color: #fff;
  }
</style>
