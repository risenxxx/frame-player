<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import MediaInfoDialog from '$lib/components/MediaInfoDialog.svelte';
  import SubsDialog from '$lib/components/SubsDialog.svelte';
  import StartScreen from '$lib/components/StartScreen.svelte';
  import SettingsDialog from '$lib/components/SettingsDialog.svelte';
  import LicensesDialog from '$lib/components/LicensesDialog.svelte';
  import ReportDialog from '$lib/components/ReportDialog.svelte';
  import RoomDialog from '$lib/components/RoomDialog.svelte';
  import ContextMenu from '$lib/components/ContextMenu.svelte';
  import TopBar from '$lib/components/TopBar.svelte';
  import StepOverlay from '$lib/components/StepOverlay.svelte';
  import Tooltip from '$lib/components/Tooltip.svelte';
  import Osd from '$lib/components/Osd.svelte';
  import Curtain from '$lib/components/Curtain.svelte';
  import Dialogs from '$lib/components/Dialogs.svelte';
  import MiniButton from '$lib/components/MiniButton.svelte';
  import Osc from '$lib/components/Osc.svelte';
  import ResizeEdges from '$lib/components/ResizeEdges.svelte';
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
  import FeedPickDialog from '$lib/components/FeedPickDialog.svelte';
  import TorrentUpdateDialog from '$lib/components/TorrentUpdateDialog.svelte';
  import VpnRouteDialog from '$lib/components/VpnRouteDialog.svelte';
  import {
    initSync,
    sync,
  } from '$lib/sync/apply.svelte';
  import { invite } from '$lib/sync/link.svelte';
  import {
    togglePlayback,
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
    toggleFullscreen,
  } from '$lib/chrome.svelte';
  import { faded } from '$lib/faded.svelte';
  import { initStallWatch } from '$lib/stall.svelte';
  import { initSubShift } from '$lib/sub-shift.svelte';
  import {
    cancelAdvance,
    dismissEnd,
    endOfFile,
    initEndScreen,
    takeSkip,
  } from '$lib/endscreen.svelte';
  import {
    deleteTorrent,
    deleteWatchedFiles,
    openFileDialog,
    openLinkDialog,
    openRecent,
    openRememberedTorrent,
    checkFeedUpdate,
    opening,
  } from '$lib/open.svelte';
  import {
    catalog,
    checkTorrentUpdate,
    openCatalog,
  } from '$lib/catalog.svelte';
  import {
    initTracks,
  } from '$lib/tracks.svelte';
  import {
    clearCastClick,
    onCastScreenClick,
    onContextMenu,
    onVideoClick,
    onVideoDblClick,
    onVideoPointerDown,
    onVideoPointerMove,
    onVideoPointerUp,
    onWheel,
  } from '$lib/input.svelte';
  import {
    initOverlays,
    overlays,
  } from '$lib/overlays.svelte';
  import { boot, shutdown } from '$lib/boot';
  import { initThumbAspect } from '$lib/seekbar.svelte';
  import { tips } from '$lib/tooltip.svelte';
  import { initCastFollow } from '$lib/bridge.svelte';
  import {
    initVideoReady,
    startScreen,
  } from '$lib/start-screen.svelte';
  import { initWindowTitle, title } from '$lib/title.svelte';
  import { installUpdate, updater } from '$lib/updater.svelte';
  import { t } from '$lib/i18n.svelte';
  import { osd } from '$lib/osd.svelte';
  import {
    forgetRecent,
  } from '$lib/history.svelte';
  import { initNowPlaying } from '$lib/now-playing.svelte';
  import { initAudioOutput } from '$lib/audio-output.svelte';
  import { IS_MAC } from '$lib/platform';
  import {
    player,
  } from '$lib/player.svelte';
  import {
    curtain,
    curtainCovered,
    curtainSettled,
    initCurtain,
  } from '$lib/curtain.svelte';
  import {
    initWindowShape,
    mini,
  } from '$lib/window-prefs.svelte';
  import {
    initUpscaleWatch,
    upscale,
  } from '$lib/upscale.svelte';
  import {
    torrent,
    torrentResume,
    trackTorrentPlayback,
  } from '$lib/torrent.svelte';
  import {
    cast,
  } from '$lib/cast.svelte';
  import { maybeStartThumbs } from '$lib/thumbs.svelte';

  // Sidecar frame stepping (StepEngine) is off: stepping goes through mpv,
  // which shows the correct picture (HDR tone mapping included), made fast by
  // frame-step.svelte.ts. The sidecar code stays, kept working.

  // The dark fill under the UI until mpv paints — see start-screen.svelte.ts.
  initVideoReady();
  // The hover preview's shape, kept at the last real one — see seekbar.svelte.ts.
  initThumbAspect();

  const osdState = $derived(osd());

  onMount(boot);

  onDestroy(shutdown);

  // The window's own title, debounced — see title.svelte.ts.
  initWindowTitle();

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
  // The control bar, off the page once it has faded out — see `faded`.
  const oscGone = faded(() => chrome.idle);
  // Now Playing on macOS: what Control Center shows, and where the AirPods'
  // pause goes. Started here for the same reason as the chrome's effects.
  initNowPlaying();
  // Which device plays the sound, for its own offset (Bluetooth lag).
  initAudioOutput();
  // Keeps the window in the picture's shape while "match video aspect ratio"
  // is on. It is told when the shell owns the window's size, because a fit
  // that stood down for fullscreen has to be given its turn afterwards, and
  // when the start screen is up, where the window is nobody's.
  initWindowShape({
    sizeOwned: () => chrome.fullscreen || chrome.isMaximized,
    resting: () => startScreen.showEmpty,
    settled: curtainSettled,
    // For the pinch: fullscreen only — "maximized" is any window at the
    // zoomed frame on macOS, and a pinch has to be able to bring one back.
    fullscreen: () => chrome.fullscreen,
    // What a pinch may not resize under: a dialog (ours or the system's) and
    // the casting screen, which fill the window and would be drawn twice.
    covered: () => chrome.sheetOpen || chrome.fileDialogOpen || cast.remote,
  });
  // The dark between two pictures. It is told when a load is under way, which
  // is the one case where staying down past its cap is right.
  initCurtain({ loading: () => opening.busy });
  // The loading plate is up: from here the bars come up beside it, as they
  // always did — a slow source is not kept waiting for a window move that
  // may be many seconds off. See `curtain.bare`.
  // `curtain.bare` is read as well as answered: the plate can be up *before*
  // the curtain comes down (the room's own open, a torrent still resolving),
  // and an effect that only watched the plate had already run by then.
  $effect(() => {
    if (curtain.bare && (opening.busy || (sync.opening && !player.hasFile))) curtainCovered();
  });
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
  // Says so while mpv compiles the upscaler, which stops the picture for
  // seconds the first time on Windows — see upscale.svelte.ts.
  initUpscaleWatch();
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

  // The seekbar and the taskbar follow the TV while casting — see bridge.svelte.ts.
  initCastFollow();
</script>

<main
  class="player"
  class:mini={mini.on}
  class:idle-ui={chrome.idle}
  class:unsteady={chrome.unsteady}
  class:settling={chrome.settling}
  class:nocursor={chrome.cursorHidden}
  class:mac={IS_MAC}
  class:no-video={startScreen.showEmpty}
  class:backdrop={!startScreen.videoReady}
  style:--veil-color={startScreen.showEmpty ? '#101016' : '#000'}
  style:--chrome-fade={chrome.unsteady ? '50ms' : null}
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
  <Curtain />
  {#if player.initError}
    <div class="overlay">
      <div class="panel">
        <h2>{t('error.mpv')}</h2>
        <p class="error">{player.initError}</p>
        <p>{t('error.mpv_hint')}</p>
      </div>
    </div>
  {:else if startScreen.showEmpty}
    <StartScreen
      torrentRows={opening.rows}
      torrentTotal={opening.total}
      torrentBusy={opening.rowBusy}
      torrentOpening={opening.rowOpening}
      feedUpdates={torrent.feedUpdates}
      feedChecking={opening.feedChecking}
      {torrentResume}
      onOpenFile={openFileDialog}
      onOpenLink={openLinkDialog}
      onOpenCatalog={catalog.enabled ? () => void openCatalog() : null}
      onOpenRecent={(item) => void openRecent(item)}
      onForgetRecent={(item) => forgetRecent(item.path)}
      onOpenTorrent={(row) => void openRememberedTorrent(row)}
      onUpdateTorrent={(known) =>
        void (torrent.feedUpdates[known.infoHash] || known.feed
          ? checkFeedUpdate(known)
          : checkTorrentUpdate(known))}
      onDeleteTorrent={(row) => void deleteTorrent(row)}
      onDeleteWatched={(row) => void deleteWatchedFiles(row)}
    />
  {:else if endOfFile.shown}
    <EndScreen
      prev={endOfFile.prev}
      next={endOfFile.next}
      counting={endOfFile.advancing}
      seq={endOfFile.seq}
      oncancel={cancelAdvance}
      onreplay={togglePlayback}
      onhide={dismissEnd}
    />
  {/if}

  <!-- The second half of this condition is the room's own open, and it is the
       one wait in the player that had nothing on screen at all: joining a room
       that is watching a torrent is a magnet resolve of up to ninety seconds,
       during which `beforeLoad` has not run — mpv has not been handed anything
       yet — so `busy` was false and the viewer sat on the start screen with a
       single line in the room chip for the whole of it. Reported, correctly, as
       a player that does nothing.

       Gated on there being no file, which is the same trade the casting screen
       makes: while the room switches films this player is still legitimately
       playing the previous one, and covering a moving picture with a status
       plate for ninety seconds reads as the player dying. There the chip is
       enough, because there is something to look at. -->
  {#if opening.busy || (sync.opening && !player.hasFile)}
    <LoadingOverlay
      label={opening.label}
      sub={opening.swarmAdvice ?? opening.torrentLabel ?? (sync.opening ? t('sync.opening') : null)}
      since={opening.since}
    />
  {:else if upscale.preparingSince}
    <LoadingOverlay
      label={t('upscale.preparing')}
      sub={t('upscale.preparing_sub')}
      since={upscale.preparingSince}
      shade
    />
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
    title={title.displayTitle}
    onclick={onCastScreenClick}
    ondblclick={() => { clearCastClick(); void toggleFullscreen(); }}
  />
  {/if}

  <StepOverlay visible={step.on} pts={step.pts} bind:canvas={step.canvas} />

  <TopBar
    idle={chrome.idle}
    mini={mini.on}
    noVideo={startScreen.showEmpty}
    fullscreen={chrome.fullscreen}
    barSide={chrome.barSide}
    barTitleText={title.barTitleText}
    titleSlide={title.titleSlide}
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

  <ResizeEdges />

  <Dialogs />

  {#if tips.tooltip}
    <Tooltip tooltip={tips.tooltip} bind:el={tips.el} />
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

  <MiniButton />

  {#if !startScreen.showEmpty}
  <Osc gone={oscGone.on} />
  {/if}
</main>

<style>
  :global(html), :global(body) {
    margin: 0;
    padding: 0;
    height: 100%;
    background: transparent;
    overflow: hidden;
    /* Rubik has no Han glyphs: a Chinese character falls through to the first
       face that has one, named so it is the system's UI face rather than
       whatever `system-ui` resolves to for the glyph. YaHei UI before YaHei:
       the UI cut has the tighter line spacing Windows' own interface uses. */
    font-family:
      'Rubik', 'Segoe UI Variable', 'Segoe UI', 'PingFang SC', 'Microsoft YaHei UI',
      'Microsoft YaHei', system-ui, sans-serif;
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
</style>
