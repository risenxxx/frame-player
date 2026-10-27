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
    startResize,
    toggleFullscreen,
  } from '$lib/chrome.svelte';
  import { faded } from '$lib/faded.svelte';
  import { initStallWatch } from '$lib/stall.svelte';
  import { initSubShift, subShift } from '$lib/sub-shift.svelte';
  import {
    cancelAdvance,
    dismissEnd,
    endOfFile,
    initEndScreen,
    takeSkip,
  } from '$lib/endscreen.svelte';
  import {
    applyEncryption,
    applyProxy,
    applyRoute,
    clearTorrentCache,
    deleteTorrent,
    deleteWatchedFiles,
    dropLink,
    openFileDialog,
    fixYtdlp,
    openLinkDialog,
    openRecent,
    openRememberedTorrent,
    openTorrent,
    checkFeedUpdate,
    pickFeedItem,
    opening,
    pickTorrentFile,
    playTorrentFile,
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
    initTracks,
    nudgeDelayHere,
    resetDelayHere,
    selectTrack,
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
    closeSettings,
    initOverlays,
    openSettingsAt,
    overlays,
    toggleInfo,
    toggleMenu,
  } from '$lib/overlays.svelte';
  import {
    resume,
  } from '$lib/resume.svelte';
  import { boot, shutdown } from '$lib/boot';
  import { diag, runDiagnosis, copyDiagnosis } from '$lib/cast-diagnosis.svelte';
  import { initThumbAspect, seekbar } from '$lib/seekbar.svelte';
  import { tips } from '$lib/tooltip.svelte';
  import { initCastFollow } from '$lib/bridge.svelte';
  import {
    backToStart,
    initVideoReady,
    startScreen,
  } from '$lib/start-screen.svelte';
  import { initWindowTitle, title } from '$lib/title.svelte';
  import { installUpdate, updater } from '$lib/updater.svelte';
  import { subs } from '$lib/subs.svelte';
  import { t } from '$lib/i18n.svelte';
  import {
    withKey,
  } from '$lib/keys.svelte';
  import { osd } from '$lib/osd.svelte';
  import {
    forgetRecent,
  } from '$lib/history.svelte';
  import { initNowPlaying } from '$lib/now-playing.svelte';
  import { initAudioOutput } from '$lib/audio-output.svelte';
  import { IS_MAC } from '$lib/platform';
  import {
    cycleAbLoop,
    jumpChapter,
    player,
    setPicture,
    cycleLoop,
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
    toggleMini,
  } from '$lib/window-prefs.svelte';
  import {
    removeFromQueue,
  } from '$lib/playlist.svelte';
  import {
    initUpscaleWatch,
    upscale,
  } from '$lib/upscale.svelte';
  import {
    torrent,
    torrentPositions,
    torrentResume,
    watchedFiles,
    trackTorrentPlayback,
    answerVpnAsk,
    net,
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

  const castStateLabel = $derived(
    cast.state === 'connecting' ? t('cast.state_connecting')
    : cast.state === 'preparing' ? t('cast.state_preparing')
    : cast.state === 'loading' || cast.state === 'buffering' ? t('cast.state_loading')
    : cast.state === 'paused' ? t('cast.state_paused')
    : t('cast.state_playing'),
  );

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
    stateLabel={castStateLabel}
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

  {#if diag.open}
      <DiagnosisDialog
        device={diag.device}
        lines={diag.lines}
        busy={diag.busy}
        onclose={() => (diag.open = false)}
        onCopy={(device) => void copyDiagnosis(device)}
      />
    {/if}

    {#if opening.linkOpen}
      <LinkDialog
        link={opening.box}
        ytdlpBusy={opening.ytdlpBusy}
        ytdlpPct={opening.ytdlpPct}
        feedBusy={opening.feedBusy}
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
        feedItem={opening.updateFeedItem}
        note={opening.updateNote}
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
         downloaded until mpv reads one (see torrent/mod.rs). -->
    <!-- A feed holding more than one release. Picking one resolves it into an
         ordinary torrent, whose own file picker may follow. -->
    {#if opening.feedPick}
      {@const pick = opening.feedPick}
      <FeedPickDialog
        feed={pick.feed}
        items={pick.items}
        onclose={() => (opening.feedPick = null)}
        onPick={(item) => void pickFeedItem(pick.url, item)}
      />
    {/if}

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
      <CatalogDialog
        onclose={closeCatalog}
        onSettings={() => {
          closeCatalog();
          openSettingsAt('torrents');
        }}
      />
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
      initialTab={overlays.settingsTab}
      onclose={closeSettings}
      onToggleSeeding={() => void toggleSeeding()}
      onTogglePortForward={() => void togglePortForward()}
      onSetProxy={(url) => void applyProxy(url)}
      onSetEncryption={(mode) => void applyEncryption(mode)}
      onSetRoute={(route) => void applyRoute(route)}
      onClearTorrentCache={() => void clearTorrentCache()}
      onLicenses={() => (overlays.licenses = true)}
      onReport={() => (overlays.report = true)}
    />
  {/if}

  <!-- The one dialog an open *waits on*: `addTorrent` holds the resolve until
       it is answered, so it sits above everything, including the link box
       that may have raised it. -->
  {#if net.ask}
    <VpnRouteDialog
      via={net.ask.via}
      direct={net.ask.direct}
      onAnswer={(choice, remember) => void answerVpnAsk(choice, remember)}
    />
  {/if}

  <!-- Above the settings sheet rather than instead of it: closing the notices
       has to give the settings back, which is what the order in `closeTopmost`
       encodes. Rendered after, so it also paints on top. -->
  {#if overlays.licenses}
    <LicensesDialog onclose={() => (overlays.licenses = false)} />
  {/if}

  {#if overlays.report}
    <ReportDialog onclose={() => (overlays.report = false)} />
  {/if}

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

  {#if !startScreen.showEmpty}
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
    class:gone={oscGone.on}
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
        diagBusy={diag.busy}
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
      chapterMarks={seekbar.chapterMarks}
      abRegion={seekbar.abRegion}
      hoverChapter={seekbar.hoverChapter}
      hasThumbs={seekbar.hasThumbs}
      thumbAspect={seekbar.thumbAspect}
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
       extends the same 44px upwards and receives no events.
       20px at the sides is the window's one edge — see `.controls` in
       Controls.svelte, which is what else sits on it. */
    padding: 0 20px 12px;
    /* Its own stacking context, so that z-index: -1 on ::before lowers the
       gradient under the controls without dropping it behind the whole bar. */
    isolation: isolate;
    /* `--chrome-fade`: see `.topbar`, and `chrome.unsteady` for why. */
    transition: opacity var(--chrome-fade, 0.25s) ease, transform var(--chrome-fade, 0.25s) ease;
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

  /* Faded out and taken off the page: the seekbar under a transparent bar is
     still written on every video frame, and still painted. See `faded`. */
  .osc.gone {
    visibility: hidden;
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
