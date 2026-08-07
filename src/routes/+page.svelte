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
  import { command, destroy, getProperty, setProperty } from 'tauri-plugin-libmpv-api';
  import { flipAxis, shiftAxis } from '$lib/floating';
  import { parseEpisode, type EpisodeRef } from '$lib/format';
  import { locale, setLocale, t, type Locale } from '$lib/i18n.svelte';
  import {
    ACTIONS,
    GROUP_ORDER,
    actionFor,
    actionLabel,
    assign,
    chordLabel,
    chordOf,
    chordsOf,
    hasCustomBindings,
    hint,
    hintPair,
    isCustom,
    isDigitJump,
    isMenuAccelerator,
    isModifierCode,
    reservedReason,
    resetAction,
    resetAll,
    unassign,
    type ActionId,
  } from '$lib/keys.svelte';
  import {
    baseName,
    displayName,
    extensionOf,
    fileStem,
    formatTime,
    formatTimeMs,
    readableLink,
    shotStamp,
  } from '$lib/format';
  import {
    languageName,
    mpvLangValue,
    parseLangList,
    searchLanguages,
  } from '$lib/languages';
  import { osd, osdSeq, showOsd } from '$lib/osd.svelte';
  import {
    RESUME_OFFSET,
    addExcludedFolder,
    clearHistory,
    dropResumeSnapshot,
    flushPosition,
    forgetDownloadedSub,
    forgetLink,
    forgetRecent,
    history,
    purgeTorrentHistory,
    isDownloadedSub,
    loadHistoryPrefs,
    loadRecent,
    recentLinks,
    rememberTitle,
    resolveTitle,
    titleFor,
    rememberLink,
    maybeRecordPosition,
    positionsLoad,
    delaysFor,
    rememberDelay,
    rememberDownloadedSub,
    rememberTrack,
    removeExcludedFolder,
    saveResumeSnapshot,
    toggleHistory,
    trackChoiceFor,
    trackWishFor,
    type RecentItem,
  } from '$lib/history.svelte';
  import { IS_MAC } from '$lib/platform';
  import {
    AUDIO_EXTENSIONS,
    DELAY_STEP,
    SUBTITLE_EXTENSIONS,
    ASPECT_AUTO,
    applyNormalise,
    VOLUME_MAX,
    attachTrack,
    chapterAt,
    clearAbLoop,
    cycleAbLoop,
    chapterTitle,
    describeTrack,
    changeSpeed,
    formatDelay,
    applyYtdlpPath,
    refreshYtdlp,
    ytdlp,
    initPlayer,
    isNetworkSource,
    jumpChapter,
    loadChapters,
    loadFiles,
    loadTracks,
    matchTrack,
    nudgeDelay,
    pickAndAttachTrack,
    removeSubTrack,
    pickAndLoadFiles,
    player,
    readList,
    resetDelay,
    resetAbLoop,
    resetPicture,
    resyncState,
    seekChapter,
    selectTrack as mpvSelectTrack,
    setPicture,
    setVolume,
    skipKind,
    cycleLoop,
    toggleMute,
    togglePause as mpvTogglePause,
    type ObservedName,
    type Track,
    type TrackWish,
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
    windowPrefs,
  } from '$lib/window-prefs.svelte';
  import {
    dropPosters,
    ensurePoster,
    ensureQueueTitles,
    loadPlaylist,
    loadPlaylistPrefs,
    neighbour,
    playEntry,
    playlist,
    queueFolder,
    queueTorrent,
    setPlaylistPref,
    type PlaylistEntry,
  } from '$lib/playlist.svelte';
  import {
    addTorrent,
    attachTorrentSubtitles,
    releaseTorrent,
    torrent,
    findSupersededTorrent,
    rememberedTorrent,
    forgetTorrent,
    listTorrents,
    torrentIsPlaying,
    rememberTorrent,
    resolveTorrentFile,
    positionBuffered,
    torrentPrefs,
    torrentVideos,
    trackTorrentPlayback,
    loadTorrentPrefs,
    setSeeding,
    type TorrentFile,
    updateTorrent,
    type RememberedTorrent,
    type TorrentInfo,
    type TorrentRow,
  } from '$lib/torrent.svelte';
  import {
    colourLine,
    isHdr,
    loadMediaInfo,
    overallBitrate,
    type MediaInfo,
  } from '$lib/mediainfo';
  import { copyScreenshot, saveScreenshot } from '$lib/screenshot';
  import {
    cast,
    castCacheCapGb,
    castCurrentFile,
    castMode,
    castNudgeVolume,
    castSeek,
    castSeekBy,
    castSetVolume,
    castSwitchAudio,
    castToggleMute,
    castTogglePause,
    disconnectCast,
    endCast,
    setCastCacheCapGb,
    setCastMode,
    startCastDiscovery,
    stopCastDiscovery,
  } from '$lib/cast.svelte';
  import { isMagnet, isTorrentLink, magnetFor, parseTorrentUrl, sourceId } from '$lib/source';
  import { maybeStartThumbs, requestThumb, thumbs } from '$lib/thumbs.svelte';
  import { isZoomed, markZoomLuaLoaded, panBy, reclampPan, resetZoom, zoomAt } from '$lib/zoom.svelte';

  // Sidecar frame stepping (StepEngine) is off: mpv's native steps give the
  // correct picture (proper HDR tone mapping) and are fast enough. The code
  // stays for a possible comeback behind an option.
  const USE_STEP_ENGINE: boolean = false;

  // Interactive settings editor: descriptions of the options mirrored into
  // mpv.conf. v = null means "player default" (the line is commented out).
  /// One row of the settings dialog. Two shapes, and the difference is what the
  /// value IS: a pill picks one of several named values, a slider covers a
  /// continuous range. Mixing them (a pill for "0.8 / 1.0 / 1.2") was the wrong
  /// answer for subtitle size — the useful setting is "a bit bigger than that",
  /// which a list of three cannot say.
  interface SettingBase {
    /// Which section it appears in. Only the three that hold mpv settings.
    tab: 'video' | 'audio' | 'subs';
    key: string;
    label: string;
    hint?: string;
    /// Value applied live when the setting is reset to the default.
    liveDefault: string;
  }
  interface SegmentedDef extends SettingBase {
    kind: 'segmented';
    options: { v: string | null; label: string }[];
  }
  /// Same control as a pill, laid out vertically because the labels are device
  /// names ("MacBook Pro Speakers") rather than words, and because the list is
  /// discovered at runtime and can be any length.
  interface DeviceDef extends SettingBase {
    kind: 'devices';
  }
  /// An **ordered** list of content languages (`alang`, `slang`).
  ///
  /// Not a pill row, and that is the whole reason this kind exists: `.segmented`
  /// picks one of a few *named* values, and there are sixty-odd languages. It is
  /// also not one value — mpv takes a priority list, and "Japanese, else
  /// English" is the ordinary case for anyone watching anime, not an exotic one.
  interface LangsDef extends SettingBase {
    kind: 'langs';
  }
  interface SliderDef extends SettingBase {
    kind: 'slider';
    min: number;
    max: number;
    step: number;
    /// Digits in the readout. Also what makes the default reachable again: the
    /// step has to divide the distance from `min` to `liveDefault` exactly, or
    /// dragging back to it is impossible and the mpv.conf line never clears.
    decimals: number;
    suffix?: string;
  }
  type SettingDef = SegmentedDef | DeviceDef | LangsDef | SliderDef;

  // $derived, so switching the language re-renders the labels; the option
  // values that are not words (spline, AC3/EAC3) stay literal.
  const SETTINGS: SettingDef[] = $derived([
    {
      kind: 'segmented',
      tab: 'video',
      key: 'target-colorspace-hint',
      label: t('vset.hdr'),
      liveDefault: 'no',
      options: [
        { v: null, label: t('vset.hdr_sdr') },
        { v: 'auto', label: t('vset.hdr_auto') },
      ],
      hint: t('vset.hdr_hint'),
    },
    {
      kind: 'segmented',
      tab: 'video',
      key: 'tone-mapping',
      label: t('vset.tonemap'),
      liveDefault: 'spline',
      options: [
        { v: null, label: 'spline' },
        { v: 'bt.2446a', label: 'bt.2446a' },
        { v: 'bt.2390', label: 'bt.2390' },
      ],
      hint: t('vset.tonemap_hint'),
    },
    {
      kind: 'segmented',
      tab: 'video',
      key: 'ytdl-format',
      label: t('vset.quality'),
      // Empty is the hook's own choice, which on YouTube means the best
      // available — 4K wherever there is 4K, and every seek then pays for it.
      liveDefault: '',
      options: [
        { v: null, label: t('vset.quality_best') },
        { v: 'bestvideo[height<=?1080]+bestaudio/best', label: '1080p' },
        { v: 'bestvideo[height<=?720]+bestaudio/best', label: '720p' },
      ],
      hint: t('vset.quality_hint'),
    },
    {
      kind: 'segmented',
      tab: 'video',
      key: 'hwdec',
      label: t('vset.hwdec'),
      liveDefault: 'auto-safe',
      // "Hardware" is hwdec=auto-safe, i.e. "take the hardware, but only where
      // mpv is confident about the decoder+output pairing". On macOS in our
      // build (output into an external NSView through --wid) auto-safe gives up
      // on VideoToolbox for some codecs and silently falls back to software —
      // on 4K60 that is seconds per exact seek. "VideoToolbox only" bypasses
      // that caution: mpv still falls back to software if the pairing fails.
      // What was actually chosen is shown at the bottom of this dialog
      // (`hwdec-current`).
      options: IS_MAC
        ? [
            { v: null, label: t('vset.hwdec_auto') },
            { v: 'videotoolbox', label: t('vset.hwdec_vt') },
            { v: 'no', label: t('vset.hwdec_sw') },
          ]
        : [
            { v: null, label: t('vset.hwdec_auto') },
            { v: 'no', label: t('vset.hwdec_sw') },
          ],
    },
    {
      kind: 'segmented',
      tab: 'audio',
      key: 'audio-spdif',
      label: t('vset.spdif'),
      liveDefault: '',
      options: [
        { v: null, label: t('vset.spdif_off') },
        { v: 'ac3,eac3', label: 'AC3/EAC3' },
        { v: 'ac3,eac3,truehd', label: '+ TrueHD' },
      ],
      hint: t('vset.spdif_hint'),
    },
    {
      kind: 'devices',
      tab: 'audio',
      key: 'audio-device',
      label: t('vset.device'),
      liveDefault: 'auto',
      hint: t('vset.device_hint'),
    },
    {
      kind: 'langs',
      tab: 'audio',
      key: 'alang',
      label: t('vset.alang'),
      liveDefault: '',
      hint: t('vset.lang_hint'),
    },
    {
      kind: 'langs',
      tab: 'subs',
      key: 'slang',
      label: t('vset.slang'),
      liveDefault: '',
      hint: t('vset.lang_hint'),
    },
    {
      kind: 'segmented',
      // Sits directly under `slang` because it modifies it, and it is mpv's
      // own option rather than selection logic of ours — reimplementing track
      // selection would fight `select_default_track` and the remembered-track
      // restore at once. Read from mpv 0.41's source rather than assumed: the
      // choices are `no`/`forced`/`yes` and the default is **yes**, so `null`
      // is the "показывать" pill and clearing the line restores it.
      tab: 'subs',
      key: 'subs-with-matching-audio',
      label: t('vset.subs_matching'),
      liveDefault: 'yes',
      options: [
        { v: null, label: t('vset.subs_matching_yes') },
        { v: 'forced', label: t('vset.subs_matching_forced') },
        { v: 'no', label: t('vset.subs_matching_no') },
      ],
      hint: t('vset.subs_matching_hint'),
    },
    {
      kind: 'segmented',
      // `sid=no` rather than an `slang` value: turning subtitles off has to
      // survive external files too, and sub-auto selects those regardless of
      // language preferences.
      tab: 'subs',
      key: 'sid',
      label: t('vset.subs_default'),
      liveDefault: 'auto',
      options: [
        { v: null, label: t('vset.subs_on') },
        { v: 'no', label: t('vset.subs_off') },
      ],
    },
    {
      kind: 'slider',
      tab: 'subs',
      key: 'sub-scale',
      label: t('vset.sub_scale'),
      // mpv's own default, and its `sub-ass-override` default is `scale`, so
      // this reaches ASS subtitles too rather than only plain text ones.
      liveDefault: '1',
      min: 0.5,
      max: 2,
      step: 0.05,
      decimals: 2,
      suffix: '×',
    },
    {
      kind: 'slider',
      tab: 'subs',
      key: 'sub-pos',
      label: t('vset.sub_pos'),
      liveDefault: '100',
      min: 0,
      max: 150,
      step: 1,
      decimals: 0,
      hint: t('vset.sub_pos_hint'),
    },
    {
      kind: 'slider',
      tab: 'subs',
      key: 'sub-border-size',
      label: t('vset.sub_border'),
      // 1.65 is mpv's default here — not the 3 it used to be, and the step has
      // to land on it exactly for "back to default" to be reachable.
      liveDefault: '1.65',
      min: 0,
      max: 5,
      step: 0.05,
      decimals: 2,
    },
    {
      kind: 'segmented',
      tab: 'subs',
      key: 'sub-border-style',
      label: t('vset.sub_style'),
      liveDefault: 'outline-and-shadow',
      options: [
        { v: null, label: t('vset.sub_style_outline') },
        { v: 'background-box', label: t('vset.sub_style_box') },
      ],
      hint: t('vset.sub_style_hint'),
    },
  ]);

  /// Menu items do nothing on their own — Rust sends the id here and the same
  /// code as the context menu performs the action.
  function runMenuAction(id: string) {
    switch (id) {
      case 'settings': void openSettings(); break;
      case 'open': void openFileDialog(); break;
      case 'open_link': void openLinkDialog(); break;
      case 'info': toggleInfo(); break;
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

  let seekValue = $state(0);
  let dragging = $state(false);
  let uiVisible = $state(true);
  let oscHover = $state(false);
  /// Parked on the title bar: like oscHover, keeps the UI from going idle.
  let barHover = $state(false);
  let fullscreen = $state(false);
  let hoverTime = $state<number | null>(null);
  let hoverX = $state(0);
  let seekWrapEl = $state<HTMLElement | null>(null);
  let fsTransition = $state(false);
  let openMenu = $state<'audio' | 'sub' | 'chapter' | 'queue' | 'cast' | null>(null);
  let stepMode = $state(false);
  let stepPts = $state(0);
  let stepCanvas = $state<HTMLCanvasElement | null>(null);
  let isMaximized = $state(false);
  let updateAvail = $state<Update | null>(null);
  let updatePct = $state<number | null>(null);
  let ctxMenu = $state<{ x: number; y: number; maxH: number } | null>(null);
  /// Which submenu of the context menu is open. An id rather than a boolean
  /// since there are two of them, and only ever one at a time — so the panel
  /// itself still needs a single ref, while the ANCHORS need one each: every
  /// head is always in the DOM, and sharing a ref would leave it pointing at
  /// whichever rendered last rather than at the open one.
  type CtxSub = 'file' | 'frame' | 'playback' | 'picture' | 'window';
  let ctxSubmenu = $state<CtxSub | null>(null);
  let ctxSubEl = $state<HTMLDivElement | undefined>();
  /// One anchor per head, keyed by submenu. A plain object rather than $state:
  /// it is read imperatively by placeSubmenu after a tick, never rendered.
  const ctxWraps: Partial<Record<CtxSub, HTMLDivElement>> = {};
  /// Submenu offset from its parent item, in pixels. Computed after render
  /// from the actual size: eyeballed estimates go stale as items are added —
  /// the same scheme as the clamping of the context menu itself.
  let ctxSubPos = $state<{ left: number; top: number; maxH: number } | null>(null);

  /// Below this window width a menu and its submenu cannot stand side by side
  /// (two 256px menus plus their margins, with room for content that pushes
  /// one of them wider), so the submenu covers the parent instead.
  const SIDE_BY_SIDE_MIN = 600;

  /// Drill-down mode: the submenu takes the parent's place instead of standing
  /// beside it. Captured when the menu opens, because it decides how submenus
  /// are *opened* and not only where they land.
  let ctxDrill = $state(false);

  let ctxSubTimer: ReturnType<typeof setTimeout> | undefined;

  function openSubmenu(id: CtxSub) {
    clearTimeout(ctxSubTimer);
    if (ctxSubmenu === id) return;
    ctxSubmenu = id;
    void placeSubmenu();
  }

  /// Delayed close — otherwise the submenu collapses right under the cursor.
  /// The path from the item to the submenu is diagonal and briefly crosses the
  /// gap between them, which belongs to neither, so mouseleave concludes the
  /// cursor left entirely. The delay is enough for the cursor to arrive;
  /// entering either area cancels it.
  function closeSubmenuSoon() {
    clearTimeout(ctxSubTimer);
    ctxSubTimer = setTimeout(() => (ctxSubmenu = null), 220);
  }

  /// Hover opens a submenu only when there is room for it beside the menu.
  /// In the mini player the submenu covers the parent, so opening on hover
  /// means the parent is unusable: every pointer move across it lands on
  /// another submenu head, and the menu cannot even be scrolled. There the
  /// submenu is a place you go to deliberately and come back from.
  function hoverSubmenu(which: CtxSub) {
    if (!ctxDrill) openSubmenu(which);
  }

  function hoverLeaveSubmenu() {
    if (!ctxDrill) closeSubmenuSoon();
  }

  function closeSubmenuNow() {
    clearTimeout(ctxSubTimer);
    ctxSubmenu = null;
  }

  async function placeSubmenu() {
    ctxSubPos = null;
    await tick();
    const wrapEl = ctxSubmenu ? ctxWraps[ctxSubmenu] : undefined;
    if (!ctxSubEl || !wrapEl) return;
    const sub = ctxSubEl.getBoundingClientRect();
    const wrap = wrapEl.getBoundingClientRect();

    // Viewport coordinates, not the wrapper's: the parent menu scrolls when it
    // is taller than the window, and `overflow` clips descendants — an
    // absolutely positioned submenu inside it would be cut off at the parent's
    // own edge. Fixed positioning takes it out of that box, and the parent
    // closes it on scroll rather than dragging it along.
    const h = flipAxis({
      near: wrap.left,
      far: wrap.right,
      size: sub.width,
      limit: window.innerWidth,
      gap: 4,
      preferBefore: false,
    });
    // Neither side had room — which is the normal case in the mini player,
    // where the window is narrower than two menus side by side. Half-overlapping
    // the parent there reads as a menu growing out of its own middle and being
    // clipped by it; so the submenu covers the parent instead, aligned to it,
    // which is the drill-down every small-screen menu uses. `room` is what the
    // chosen side could actually give.
    const menu = ctxEl?.getBoundingClientRect();
    const cramped = (ctxDrill || h.room < sub.width) && !!menu;
    ctxSubPos = {
      left: cramped ? shiftAxis(menu.left, sub.width, window.innerWidth) : h.pos,
      // Level with the item, slid up only as far as it takes to fit — or with
      // the parent's own top when it is standing in for the parent.
      top: shiftAxis(cramped ? menu.top : wrap.top - 6, sub.height, window.innerHeight),
      maxH: window.innerHeight - 16,
    };
  }
  // ---- Open link ----------------------------------------------------------

  let linkOpen = $state(false);
  let linkValue = $state('');
  let linkInputEl = $state<HTMLInputElement | undefined>();
  /// A link failed to open. Kept until the next attempt, because the reason is
  /// worth reading twice — especially the yt-dlp one.
  let linkFailed = $state(false);
  /// The link that failed, so the fix can retry it without retyping.
  let lastLink = $state('');
  /// What the player is currently trying to open. NOT `lastLink`, which only
  /// ever remembers the last thing typed into the dialog and is never cleared:
  /// judging a failure by that meant a stray `end-file` error long afterwards —
  /// and they happen routinely, e.g. when a load is abandoned because another
  /// file was opened — reopened the link dialog over the start screen. Its
  /// backdrop then swallowed the next click, so opening a file "did nothing"
  /// until enough clicks had dismissed enough dialogs.
  let attempting = $state('');
  /// An install or update is running.
  let ytdlpBusy = $state(false);
  /// Download progress, 0..100, or null when there is nothing to report — an
  /// update runs inside yt-dlp itself, which does not tell us how far along it
  /// is, so the button falls back to a label without a figure.
  let ytdlpPct = $state<number | null>(null);

  /// Install or update yt-dlp, then have another go at the link that failed.
  ///
  /// Only our own copy is ever updated — one from Homebrew or pip belongs to
  /// its package manager, so there the dialog explains instead of offering a
  /// button it has no right to press.
  async function fixYtdlp() {
    if (ytdlpBusy) return;
    ytdlpBusy = true;
    ytdlpPct = null;
    try {
      const version = await invoke<string>(ytdlp.present ? 'ytdlp_update' : 'ytdlp_install');
      await refreshYtdlp();
      // mpv learns the path without a restart: installing it mid-session is
      // exactly the case this whole flow exists for.
      applyYtdlpPath();
      showOsd(t('link.ytdlp_ready', { version }));
      linkFailed = false;
      if (lastLink) {
        linkOpen = false;
        void loadFile(lastLink);
      }
    } catch (e) {
      showOsd(t('link.ytdlp_failed'));
      console.warn('yt-dlp install/update failed:', e);
    } finally {
      ytdlpBusy = false;
      ytdlpPct = null;
    }
  }

  /// A network source is being opened and nothing is on screen yet.
  ///
  /// Between `loadfile` and the first frame a URL can take many seconds — yt-dlp
  /// resolving the page, then the stream buffering — and all of it used to be a
  /// black window with no sign that anything was happening. Local files never
  /// get here: they open faster than the indicator would fade in.
  let opening = $state(false);

  /// What the indicator says. mpv reports the cache fill only while there is a
  /// cache, so an absent figure means "still resolving", not "0%".
  const openingLabel = $derived.by(() => {
    if (player.stalled) return t('load.stalled');
    const pct = player.cacheBuffering;
    return pct !== null && pct < 100 ? t('load.buffering', { percent: pct }) : t('load.opening');
  });

  /// The live readout, or null when there is nothing to report.
  ///
  /// Only while a torrent is actually feeding playback: a finished download has
  /// no rate worth watching, and a local file has no swarm. `opening` is
  /// excluded because the loading overlay is already saying all of this, and two
  /// copies of the same numbers on screen at once is worse than one.
  const torrentChip = $derived.by(() => {
    const s = torrent.status;
    if (!s || !player.hasFile || opening) return null;
    if (s.state !== 'live' && s.state !== 'initializing') return null;
    // Nothing left to fetch for this file — the chip has served its purpose and
    // would otherwise sit at 100% for the rest of the film.
    if (s.file_size > 0 && s.file_done >= s.file_size) return null;
    return s;
  });

  /// The second line, when a torrent is what is slow.
  ///
  /// mpv's own answer to "why is nothing happening" over a torrent is
  /// `paused-for-cache`, which says *that* it is waiting and nothing about why.
  /// Whether the swarm is empty or simply slow is the difference between "this
  /// is broken" and "give it ten seconds", and it is the only thing that makes
  /// waiting bearable — so the peer count and the rate go under the label
  /// whenever they are what the viewer is waiting on.
  const torrentLabel = $derived.by(() => {
    const s = torrent.status;
    if (!s || s.state === 'gone') return null;
    if (s.state === 'error') return s.error ?? t('torrent.error');
    // Not a swarm problem at all: librqbit is hashing what is already on disk.
    // Measured at 3.1 s for a season with 46 MB fetched, and it is the whole of
    // the wait when reopening a half-watched one — while `peers` is legitimately
    // zero, so without this the popup announced "no peers" for something that
    // was not looking for any.
    if (s.state === 'initializing') return t('torrent.verifying');
    // No peers at all is a different problem from a slow one, and the advice
    // ("this torrent has no seeders") is different too.
    if (s.peers === 0) {
      return s.peers_seen > 0 ? t('torrent.connecting') : t('torrent.no_peers');
    }
    return t('torrent.rate', {
      peers: s.peers,
      speed: fmtSpeed(s.down_bps),
    });
  });

  async function openLinkDialog() {
    linkFailed = false;
    links = recentLinks();
    for (const url of links) {
      const known = titleFor(url);
      if (known) linkTitles = { ...linkTitles, [url]: known };
    }
    linkOpen = true;
    void fillLinkTitles();
    await tick();
    // Focused so the paste shortcut works with nothing else to click.
    linkInputEl?.focus();
    linkInputEl?.select();
  }

  // ---- Subtitle search (OpenSubtitles) ------------------------------------
  //
  // Two things shape this panel. A hash match and a title match are *not* the
  // same result — the first is the same release and drops in already in sync,
  // the second is a guess about which rip you have — so which one produced the
  // list is said out loud rather than left for the viewer to discover by
  // watching lips. And the hash is only sent for a file outside the private
  // folders: it names what is being watched as surely as a stored position
  // does. There the panel searches by typed title instead, which is the
  // viewer's own words rather than something we volunteered.

  type SubHit = {
    file_id: number;
    file_name: string;
    language: string;
    release: string;
    movie: string;
    year: number | null;
    parent: string;
    season: number | null;
    episode: number | null;
    downloads: number;
    hash_match: boolean;
    hearing_impaired: boolean;
    from_trusted: boolean;
    ai_translated: boolean;
    fps: number | null;
    uploader: string;
  };

  /// The subtitle languages this viewer has asked for, read from mpv when the
  /// panel opens (see `openSubsDialog`).
  let subsLangPrefs = $state<string[]>([]);

  /// The language filter, which is those preferences plus "any".
  ///
  /// It was a hardcoded Russian/English/any, which is the same leak ROADMAP 25
  /// is about: those are the two languages the *interface* is translated into,
  /// and they had become the only ones a search could filter by. Deriving it
  /// costs no second picker and no second preference — someone who has said
  /// "subtitles in Polish, else English" has already answered this question —
  /// and it degrades to the interface language, which is what the old list gave
  /// that person anyway. "Любые" stays last and always, or the panel would
  /// become unable to find a subtitle in a language nobody thought to configure.
  const SUBS_LANGS: { value: string; label: string }[] = $derived([
    ...(subsLangPrefs.length ? subsLangPrefs : [locale()]).map((code) => ({
      value: code,
      label: languageName(code),
    })),
    { value: '', label: t('subs.lang_any') },
  ]);

  let subsOpen = $state(false);
  let subsQuery = $state('');
  let subsLang = $state('');
  let subsBusy = $state(false);
  let subsError = $state<string | null>(null);
  let subsHits = $state<SubHit[] | null>(null);
  let subsMatchKind = $state<'hash' | 'title'>('title');
  /// The file's hash was looked up and matched nothing — the useful negative,
  /// and the difference between "here is a guess" and "OpenSubtitles has
  /// nothing for this release".
  let subsHashTried = $state(false);
  let subsHashBlocked = $state(false);
  let subsQuota = $state<{ remaining: number | null; reset: string | null } | null>(null);
  let subsBusyId = $state<number | null>(null);
  let subsInputEl = $state<HTMLInputElement | null>(null);
  /// The video's own frame rate, read once when the panel opens.
  ///
  /// OpenSubtitles carries **no duration** for a subtitle — measured, there is
  /// no such field anywhere in a search result, in `files[]`, in
  /// `feature_details` or in the feature record — so the question "will this
  /// one fit" has to be answered by what there is. `fps` is it: a 25 fps
  /// subtitle on a 23.976 fps file does not merely start late, it drifts
  /// further out the longer you watch, which is the failure people actually
  /// hit and cannot diagnose.
  let subsVideoFps = $state<number | null>(null);
  /// Season and episode read off the file name, when it says so. Sent with a
  /// title search and shown in the panel, because otherwise a list of episodes
  /// appearing for a one-word query looks like magic.
  let subsEpisode = $state<EpisodeRef | null>(null);

  /// A tenth of a frame is far tighter than any real pair (25 vs 23.976 is the
  /// case that matters) and loose enough to survive mpv reporting 23.976024.
  function subsFpsOff(fps: number | null): boolean {
    return fps !== null && subsVideoFps !== null && Math.abs(fps - subsVideoFps) > 0.1;
  }

  // ---- The OpenSubtitles account ------------------------------------------
  //
  // Signing in is an upgrade, not a gate: downloading works signed out (5 a day
  // per IP), and an account raises that to 10–1000. So the form is folded away
  // until asked for, and the panel never blocks on it.

  type SubsAccount = {
    signed_in: boolean;
    username: string | null;
    remembered_password: boolean;
    allowed_downloads: number | null;
    remaining_downloads: number | null;
    level: string | null;
    keychain_failed: boolean;
  };

  let subsAccount = $state<SubsAccount | null>(null);
  let subsAuthOpen = $state(false);
  let subsUser = $state('');
  let subsPass = $state('');
  let subsRemember = $state(false);
  let subsAuthBusy = $state(false);
  let subsAuthError = $state<string | null>(null);
  let subsAuthEl = $state<HTMLDivElement | null>(null);
  let subsAuthUserEl = $state<HTMLInputElement | null>(null);

  /// Registration. The locale prefix follows the pattern the API documentation
  /// uses for the consumers page (`/en/consumers`); the path itself is Devise's
  /// convention and could not be verified from here — the site sits behind
  /// Cloudflare, which answers every programmatic request with 403, including
  /// for pages known to exist.
  const OPENSUBTITLES_SIGNUP = 'https://www.opensubtitles.com/en/users/sign_up';

  /**
   * Open the sign-in form and bring it into view.
   *
   * The panel is a scroll container with a bounded height (`.settings` is
   * `max-height: 100%; overflow-y: auto`), and the form unfolds at the very
   * bottom, below a list that is usually taller than the window — so without
   * this the button appears to do nothing at all. Smooth rather than instant on
   * purpose: the movement is what connects the press to what appeared.
   */
  /// Bring the sign-in form fully into view, buttons included.
  ///
  /// Deliberately instant and deliberately done twice. Measured in a WKWebView
  /// harness against this exact structure (a `max-height` box with
  /// `overflow-y: auto`, the form inserted on click): every scroll API reaches
  /// the bottom — `scrollIntoView`, `scrollTo`, `scrollTop`, smooth or not, with
  /// or without waiting a frame — **except** when a `focus()` call is nearby,
  /// which trims a *smooth* animation to 710 px of the 717 needed and does so
  /// even with `preventScroll: true`. Instant lands exactly, so the animation is
  /// not worth its one failure mode; and the second pass on the next frame
  /// covers the box still settling as the form unfolds.
  function revealSubsAuth() {
    subsAuthEl?.scrollIntoView({ block: 'end' });
  }

  async function openSubsAuth() {
    subsAuthOpen = true;
    subsAuthError = null;
    await tick();
    // Still `preventScroll`: focusing an input scrolls it into view by itself,
    // which lands the field near the top and leaves the buttons under the fold
    // — the half that mattered.
    subsAuthUserEl?.focus({ preventScroll: true });
    revealSubsAuth();
    requestAnimationFrame(revealSubsAuth);
  }

  async function loadSubsAccount(refresh: boolean) {
    try {
      subsAccount = await invoke<SubsAccount>('subs_account', { refresh });
    } catch (e) {
      console.warn('account state failed:', e);
    }
  }

  async function subsSignIn() {
    if (subsAuthBusy || !subsUser.trim() || !subsPass) return;
    subsAuthBusy = true;
    subsAuthError = null;
    try {
      subsAccount = await invoke<SubsAccount>('subs_login', {
        username: subsUser.trim(),
        password: subsPass,
        rememberPassword: subsRemember,
      });
      subsAuthOpen = false;
      // Kept nowhere on this side: the password's only home is the keychain,
      // and only when it was asked for.
      subsPass = '';
      // `/login` reports the account's allowance but not what is left of it
      // today; `/infos/user` does. Asked for straight away, because the line
      // that says "12 of 20 left" is also the proof that the token works.
      void loadSubsAccount(true);
    } catch (e) {
      subsAuthError = t('subs.sign_in_failed', { reason: String(e) });
    } finally {
      subsAuthBusy = false;
    }
  }

  /**
   * Take an external subtitle out of the session — and off the disk when it is
   * one we downloaded.
   *
   * The second half is not tidiness: a downloaded subtitle is named after the
   * video precisely so `sub-auto=fuzzy` finds it by itself next time, so
   * detaching without deleting means it is back at the next launch and the
   * removal looks broken. A file we did not create is only detached, because a
   * subtitle the viewer made or corrected is not ours to delete.
   */
  async function removeSubtitle(track: Track) {
    const path = track.path;
    if (!(await removeSubTrack(track))) return;
    if (path && isDownloadedSub(path)) {
      try {
        await invoke('subs_delete_file', { path });
        forgetDownloadedSub(path);
        showOsd(t('osd.sub_deleted'));
        return;
      } catch (e) {
        // The track is already gone from the session, so this is not a failure
        // of the action the viewer asked for — only of the cleanup.
        console.warn('deleting the subtitle file failed:', e);
      }
    }
    showOsd(t('osd.sub_removed'));
  }

  async function subsSignOut() {
    subsAuthBusy = true;
    try {
      subsAccount = await invoke<SubsAccount>('subs_logout');
      subsPass = '';
    } catch (e) {
      console.warn('sign out failed:', e);
    } finally {
      subsAuthBusy = false;
    }
  }

  async function openSubsDialog() {
    subsError = null;
    subsHits = null;
    subsBusyId = null;
    // **Asked of mpv, not of `settingsValues`** — that map is filled when the
    // settings dialog opens and is empty until then, so reading it here would
    // make the filter depend on whether the viewer had been into settings this
    // session. mpv always knows: it has merged mpv.conf over `initialOptions`,
    // and every change in that dialog is applied to it live.
    subsLangPrefs = parseLangList(await getProperty('slang', 'string').catch(() => null));
    // The first preferred subtitle language, which is where a search should
    // start; with none set, the language the UI is in. "Any" stays one click
    // away either way.
    subsLang = subsLangPrefs[0] ?? locale();
    // Prefilled with the title the player is showing, which for a file mpv
    // named is already the right query — and is what the viewer would type.
    // A release name carries the show and the episode in one string, and both
    // halves are wanted: the numbers reach the right work (a title alone finds
    // the 1973 film rather than the 2024 series), and the title without them is
    // a far better query than the whole file name.
    subsEpisode = player.filePath ? parseEpisode(player.filePath) : null;
    subsQuery = subsEpisode?.title || (player.hasFile ? player.displayTitle : '');
    subsVideoFps = player.hasFile
      ? await getProperty('container-fps', 'double').catch(() => null)
      : null;
    subsOpen = true;
    subsAuthOpen = false;
    subsAuthError = null;
    // The live quota, which is worth knowing before spending one to find out.
    void loadSubsAccount(true);
    await tick();
    subsInputEl?.focus();
    subsInputEl?.select();
    // A local file can be searched by hash with nothing typed, so the panel
    // opens with an answer instead of an empty box.
    if (player.filePath && !player.filePath.includes('://')) void runSubsSearch();
  }

  async function runSubsSearch() {
    if (subsBusy) return;
    subsBusy = true;
    subsError = null;
    try {
      const result = await invoke<{
        items: SubHit[];
        match_kind: 'hash' | 'title';
        hash_tried: boolean;
        hash_blocked: boolean;
        total: number;
      }>('subs_search', {
        path: player.filePath ?? null,
        query: subsQuery.trim() || null,
        languages: subsLang,
        season: subsEpisode?.season ?? null,
        episode: subsEpisode?.episode ?? null,
      });
      subsHits = result.items;
      subsMatchKind = result.match_kind;
      subsHashTried = result.hash_tried;
      subsHashBlocked = result.hash_blocked;
    } catch (e) {
      console.warn('subtitle search failed:', e);
      subsHits = null;
      subsError = String(e).includes('no API key') ? t('subs.no_key') : t('subs.failed');
    } finally {
      subsBusy = false;
    }
  }

  async function downloadSub(hit: SubHit) {
    if (subsBusyId !== null) return;
    subsBusyId = hit.file_id;
    subsError = null;
    try {
      const result = await invoke<{
        path: string;
        remaining: number | null;
        reset: string | null;
      }>('subs_download', {
        fileId: hit.file_id,
        videoPath: player.filePath ?? null,
        language: hit.language || 'sub',
      });
      subsQuota = { remaining: result.remaining, reset: result.reset };
      // Recorded as ours before it is attached: this is the only thing that
      // later tells it apart from a subtitle the viewer made, and it is what
      // lets removing it delete the file instead of leaving one `sub-auto`
      // brings straight back.
      rememberDownloadedSub(result.path);
      // Straight through the same path a dropped .srt takes, so it is selected
      // and the track list refreshes without a second mechanism.
      await attachTrack('sub', result.path);
      subsOpen = false;
    } catch (e) {
      console.warn('subtitle download failed:', e);
      // A spent quota is a state with a way out, not a breakage: measured, the
      // API answers HTTP 406 with the time it renews. Anonymous callers get 5 a
      // day per IP, so this is the wall a regular viewer hits — and signing in
      // is what moves it, which is why the form is opened right here rather
      // than left to be discovered.
      const failure = e as { kind?: string; message?: string; reset?: string | null };
      if (failure?.kind === 'quota') {
        subsError = failure.reset
          ? t('subs.quota_spent_reset', { time: failure.reset })
          : t('subs.quota_spent');
        if (!subsAccount?.signed_in) void openSubsAuth();
      } else {
        subsError = t('subs.download_failed_hint', {
          reason: failure?.message ?? String(e),
        });
      }
    } finally {
      subsBusyId = null;
    }
  }

  /// An `end-file` error does NOT mean the attempt failed.
  ///
  /// mpv's ytdl hook routinely raises one on the way to succeeding — it opens
  /// the original URL, gives up on it, and loads what yt-dlp resolved instead —
  /// and a playlist transition can do the same. Reporting on the event itself
  /// therefore threw an error dialog over videos that were about to play, which
  /// is worse than saying nothing. So the verdict waits: if anything loads in
  /// the meantime, `onFileLoaded` cancels it and nobody hears about it.
  const LOAD_FAIL_GRACE = 2500;
  let loadFailTimer: ReturnType<typeof setTimeout> | undefined;

  function reportLoadFailure() {
    clearTimeout(loadFailTimer);
    loadFailTimer = setTimeout(() => {
      opening = false;
      // A link that fails deserves more than a popup that fades: the fix is
      // usually one button, and the dialog is where the user just was. Judged
      // by what was being opened, and only while nothing has opened since.
      if (isNetworkSource(attempting) && !player.hasFile) {
        lastLink = attempting;
        linkFailed = true;
        linkOpen = true;
        return;
      }
      showOsd(t('osd.open_failed'));
    }, LOAD_FAIL_GRACE);
  }

  function submitLink(url = linkValue.trim()) {
    if (!url) return;
    // Dispatched by shape, not by a separate box: a magnet and a YouTube link
    // are the same gesture — the viewer has a link — and which protocol reaches
    // the video is the player's problem, not theirs.
    if (isTorrentLink(url)) {
      // A re-upload usually arrives here rather than through the update button,
      // because pasting a link into the box that takes links is the obvious
      // move. Opening it as a separate torrent would silently re-download a
      // season and reset its history, so the resemblance is worth a question —
      // asked before anything is added, while both outcomes are still cheap.
      const superseded = findSupersededTorrent(url);
      if (superseded) {
        linkOpen = false;
        void openUpdateDialog(superseded, url);
        return;
      }
      void openTorrent(url);
      return;
    }
    linkOpen = false;
    linkFailed = false;
    lastLink = url;
    rememberLink(url);
    links = recentLinks();
    void loadFile(url);
  }

  // ---- Torrent streaming --------------------------------------------------
  //
  // Resolving a magnet is a DHT lookup that takes ten seconds when it works, so
  // the dialog stays up saying so rather than closing on a black window — the
  // same rule as everywhere else here: a slow operation announces itself before
  // it starts, not after it finishes.
  //
  // What comes back is a *list*. A torrent with one film plays it; a torrent
  // holding a season is a question only the viewer can answer, so it gets a
  // picker. Either way every video in it becomes a queue entry, which costs
  // nothing until one is played (see torrent.rs).

  /// A resolved torrent waiting for the viewer to choose a file from it.
  let torrentPick = $state<TorrentInfo | null>(null);
  /// Why the last magnet did not open, as a finished sentence.
  let torrentError = $state<string | null>(null);

  async function openTorrent(source: string) {
    torrentError = null;
    torrentPick = null;
    lastLink = source;
    try {
      const info = await addTorrent(source);
      // **What is remembered is a magnet, whatever was handed over.** A dropped
      // `.torrent` file is a path that can be moved or deleted and a `.torrent`
      // URL is a page that can go down, while the info hash names the torrent
      // for good — and the metadata cache beside it (torrent.rs) makes
      // reopening from a bare magnet cost no lookup at all. So a file opened
      // once keeps working after it is thrown away, which is what people do
      // with `.torrent` files.
      const key = isMagnet(source) ? source : magnetFor(info.info_hash, info.name);
      rememberLink(key);
      // The torrent's own name against the magnet, so the recents list reads as
      // a film rather than a hash. `displayName` would fall back to the `dn`
      // parameter, but that is the name whoever made the link chose to put in
      // it; this is the one in the torrent.
      if (info.name) rememberTitle(key, info.name);
      // How to reopen this torrent later, which is what makes the start-screen
      // list and the history cards work after a restart.
      rememberTorrent(info, key);
      links = recentLinks();
      const videos = torrentVideos(info);
      if (videos.length === 0) {
        torrentError = t('torrent.no_video');
        return;
      }
      if (videos.length === 1) {
        await playTorrentFile(info, videos[0]);
        return;
      }
      torrentPick = info;
    } catch (e) {
      // The one failure worth naming: a swarm that never answered is a fact
      // about the torrent, and "try a different magnet" is the actual advice.
      torrentError =
        String(e) === 'resolve_timeout' ? t('torrent.timeout') : t('torrent.failed', { reason: String(e) });
      // A torrent does not only arrive from the box that shows this message: a
      // dropped `.torrent` reaches here with no dialog open, and a corrupt file
      // would then fail in complete silence. Raising it is the same answer the
      // start-screen rows give, and it is a no-op when the box is already up.
      linkOpen = true;
    }
  }

  /// Pick a `.torrent` from disk. The same dimming flag as the other pickers:
  /// the system dialog dims the window while it is up.
  async function pickTorrentFile() {
    fileDialogOpen = true;
    try {
      const sel = await open({
        multiple: false,
        filters: [{ name: t('dialog.torrent'), extensions: ['torrent'] }],
      });
      if (typeof sel === 'string') await openTorrent(sel);
    } finally {
      fileDialogOpen = false;
    }
  }

  /// Turning seeding off has to take effect now, not at the next magnet — a
  /// switch that reads "off" while pieces are still going out would be a lie
  /// about the one behaviour here with legal weight attached. That costs the
  /// running torrent, so the popup says so rather than leaving a stream that
  /// silently died.
  async function toggleSeeding() {
    const stopped = await setSeeding(!torrentPrefs.seeding);
    if (stopped) {
      torrent.info = null;
      torrent.status = null;
      showOsd(t('torrent.seed_restarted'));
    }
  }

  /**
   * Open a card from "continue watching".
   *
   * A local file is its own address and opens directly. A **torrent entry is
   * not**: what the record holds is a loopback URL, and its port died with the
   * session that issued it — so after a restart every torrent card led to a
   * refused connection. It has to be re-derived from the magnet instead, which
   * is what `resolveTorrentFile` does, and which is the reason torrents are
   * remembered at all. With history off nothing was recorded and the honest
   * answer is to say so rather than fail silently.
   */
  async function openRecent(item: RecentItem) {
    if (!item.id.startsWith('torrent:')) {
      await loadFile(item.path);
      return;
    }
    opening = true;
    const url = await resolveTorrentFile(item.id);
    if (!url) {
      opening = false;
      showOsd(t('start.torrent_lost'));
      return;
    }
    await loadFile(url);
  }

  // ---- The torrent list on the start screen -------------------------------

  let torrentRows = $state<TorrentRow[]>([]);
  /// The folder being deleted, so its row can grey out — `remove_dir_all` over
  /// 9 GB is not instant and a row that keeps looking clickable invites a
  /// second press.
  let torrentBusy = $state<string | null>(null);

  const torrentTotal = $derived(torrentRows.reduce((sum, r) => sum + r.size, 0));

  async function refreshTorrents() {
    torrentRows = await listTorrents();
  }

  /// Where the viewer left off inside this torrent — the single most useful
  /// line on the row, and the reason it is worth a lookup: "продолжить: S01E03"
  /// answers what the torrent is *for*, where a size does not.
  function torrentResume(
    row: TorrentRow,
  ): { name: string; pos: number; dur: number; index: number } | null {
    if (!row.info_hash) return null;
    const prefix = `torrent:${row.info_hash}/`;
    // Straight from the position store rather than `history.recent`, which holds
    // only the twelve newest entries across everything — a torrent watched last
    // week has fallen out of that long before its data has fallen off the disk.
    const positions = positionsLoad();
    let best: { name: string; pos: number; dur: number; index: number; ts: number } | null = null;
    for (const [id, rec] of Object.entries(positions)) {
      if (!id.toLowerCase().startsWith(prefix)) continue;
      const ts = rec.ts ?? 0;
      if (best && ts <= best.ts) continue;
      best = {
        name: rec.title || displayName(rec.src ?? id),
        pos: rec.pos,
        dur: rec.dur,
        index: Number(id.slice(prefix.length)),
        ts,
      };
    }
    return best;
  }

  /// Which file of a torrent has a saved position, by index — for the picker.
  function torrentPositions(infoHash: string): Record<number, { pos: number; dur: number }> {
    const prefix = `torrent:${infoHash.toLowerCase()}/`;
    const out: Record<number, { pos: number; dur: number }> = {};
    for (const [id, rec] of Object.entries(positionsLoad())) {
      if (!id.toLowerCase().startsWith(prefix)) continue;
      out[Number(id.slice(prefix.length))] = { pos: rec.pos, dur: rec.dur };
    }
    return out;
  }

  /// The folder currently being resolved, so its row can say so. Resolving a
  /// magnet is a DHT lookup — a second at best — and the row used to sit there
  /// looking untouched for all of it.
  let torrentOpening = $state<string | null>(null);

  /**
   * Reopen a torrent from the start screen.
   *
   * **Goes straight to where the viewer left off**, which is what the row says
   * it will do — it prints "продолжить: S01E03 · 23:14" — and what it used to
   * ignore, dropping the same file picker a freshly pasted magnet gets. Landing
   * on a list of nine identical file names to hunt for the episode you were
   * already watching is the player forgetting something it plainly knows.
   *
   * The picker is still what happens when there is nothing to continue: a
   * torrent opened but never watched, or one whose positions have aged out.
   */
  async function openRememberedTorrent(row: TorrentRow) {
    if (!row.known || torrentOpening) return;
    const resume = torrentResume(row);
    torrentOpening = row.folder;
    torrentError = null;
    try {
      const info = await addTorrent(row.known.magnet);
      rememberTorrent(info, row.known.magnet);
      const videos = torrentVideos(info);
      if (!videos.length) {
        torrentError = t('torrent.no_video');
        linkOpen = true;
        return;
      }
      const wanted =
        (resume && videos.find((f) => f.index === resume.index)) ??
        (videos.length === 1 ? videos[0] : null);
      if (wanted) {
        await playTorrentFile(info, wanted);
        return;
      }
      torrentPick = info;
    } catch (e) {
      torrentError =
        String(e) === 'resolve_timeout'
          ? t('torrent.timeout')
          : t('torrent.failed', { reason: String(e) });
      linkOpen = true;
    } finally {
      torrentOpening = null;
    }
  }

  /// Delete a torrent completely — data, history, magnet. One action, because a
  /// half-deleted torrent is the confusing state: files gone but the season
  /// still listed, or the reverse.
  async function deleteTorrent(row: TorrentRow) {
    if (torrentIsPlaying(row) || torrentBusy) return;
    torrentBusy = row.folder;
    try {
      const freed = await forgetTorrent(row);
      if (row.info_hash) {
        purgeTorrentHistory(row.info_hash);
        if (row.known) forgetLink(row.known.magnet);
      }
      await refreshTorrents();
      links = recentLinks();
      showOsd(t('torrent.cache_cleared', { size: fmtSize(freed) }));
    } finally {
      torrentBusy = null;
    }
  }

  // ---- Replacing a torrent with its re-upload ------------------------------
  //
  // BitTorrent cannot add a file to a torrent, so an uploader who adds an
  // episode publishes a different one. The player cannot discover that — the new
  // magnet lives on a tracker page we have no access to — but once the viewer
  // brings it, everything else is ours to do: keep the episodes already on disk,
  // and keep the watch history that is keyed by the old info hash.

  /// The torrent being replaced, and the link for it. Null when the dialog is shut.
  let updateFor = $state<RememberedTorrent | null>(null);
  let updateValue = $state('');
  let updateBusy = $state(false);
  let updateError = $state<string | null>(null);
  let updateInputEl = $state<HTMLInputElement | undefined>();
  /// Set when a link typed into the ordinary "Open link…" box looks like a newer
  /// release of something already in the list — see `submitLink`.
  let updateSuggested = $state(false);

  async function openUpdateDialog(known: RememberedTorrent, magnet = '') {
    updateFor = known;
    updateValue = magnet;
    updateError = null;
    updateSuggested = !!magnet;
    await tick();
    updateInputEl?.focus();
    updateInputEl?.select();
  }

  async function submitUpdate() {
    const magnet = updateValue.trim();
    const known = updateFor;
    if (!magnet || !known || updateBusy) return;
    if (!isTorrentLink(magnet)) {
      updateError = t('torrent.update_not_magnet');
      return;
    }
    updateBusy = true;
    updateError = null;
    try {
      const { info, matched, added } = await updateTorrent(known, magnet);
      updateFor = null;
      links = recentLinks();
      await refreshTorrents();
      showOsd(t('torrent.updated', { matched, added }));
      // Straight into the picker: the viewer came here for the new episode, and
      // making them click the row again to reach it would be the second half of
      // the same errand.
      const videos = torrentVideos(info);
      if (videos.length) torrentPick = info;
    } catch (e) {
      const reason = String(e);
      updateError = reason.includes('same_torrent')
        ? t('torrent.update_same')
        : reason.includes('no_hash')
          ? t('torrent.update_not_magnet')
          : t('torrent.failed', { reason });
    } finally {
      updateBusy = false;
    }
  }

  /// Close what is playing and return to the start screen.
  ///
  /// `stop` rather than `playlist-remove` or a flag of our own: it unloads the
  /// file *and* empties the playlist, which is what "back to the start screen"
  /// has to mean — leaving a queue behind would have the next episode start
  /// itself the moment anything nudged playback.
  async function backToStart() {
    await command('stop').catch((e) => console.warn('stop failed:', e));
  }

  async function clearTorrentCache() {
    const freed = await invoke<number>('torrent_clear_cache').catch(() => 0);
    showOsd(t('torrent.cache_cleared', { size: fmtSize(freed) }));
  }

  async function playTorrentFile(info: TorrentInfo, file: TorrentFile) {
    torrentPick = null;
    linkOpen = false;
    linkFailed = false;
    torrentError = null;
    await loadFile(file.url);
    // After the load, like the folder queue: the entry being played is already
    // in the playlist and the rest go around it.
    await queueTorrent(torrentVideos(info), file.url);
  }

  /// Read when the dialog opens rather than kept live: the list only changes
  /// when a link is opened, which closes the dialog.
  let links = $state<string[]>([]);
  /// Titles for the rows, by link. A row shows its id until one arrives.
  let linkTitles = $state<Record<string, string>>({});

  /// Fill in whatever names are missing, one request at a time — this is a
  /// network call per unknown link, and a list of eight should not open eight
  /// connections at once.
  async function fillLinkTitles() {
    for (const url of links) {
      if (linkTitles[url]) continue;
      const title = await resolveTitle(url);
      if (title) linkTitles = { ...linkTitles, [url]: title };
    }
  }

  /// Drop one row of the recent-links list.
  ///
  /// The list is the *only* thing touched: a magnet in it may name a torrent
  /// whose episodes are on disk, and "I do not want this link offered again" is
  /// not "delete the season" — that is what the torrent list's own cross is for.
  function dropLink(url: string) {
    forgetLink(url);
    links = recentLinks();
  }

  /// The name of the video being played, when mpv has none.
  ///
  /// mpv normally supplies it (yt-dlp resolves the page), so this is the
  /// fallback for the case the viewer actually sees: a link whose title never
  /// arrived, leaving a video id in the title bar.
  let resolvedTitle = $state<string | null>(null);



  // ---- Media info ---------------------------------------------------------

  let infoOpen = $state(false);
  let mediaInfo = $state<MediaInfo | null>(null);

  $effect(() => {
    if (!infoOpen) return;
    void refreshInfo();
    // Bitrate is a rolling average of recent packets: it is absent for the
    // first seconds of a file and drifts afterwards, so the panel is a live
    // readout rather than a snapshot.
    const timer = setInterval(() => void refreshInfo(), 1000);
    return () => clearInterval(timer);
  });

  async function refreshInfo() {
    if (!player.hasFile) {
      mediaInfo = null;
      return;
    }
    mediaInfo = await loadMediaInfo().catch(() => null);
  }

  function toggleInfo() {
    if (!player.hasFile) return;
    infoOpen = !infoOpen;
  }

  // Units go through t() at render time rather than being baked into the
  // values, so switching the language relabels them without waiting for the
  // next refresh.
  function fmtSize(bytes: number): string {
    const gb = bytes / 1024 ** 3;
    return gb >= 1
      ? t('info.gb', { value: gb.toFixed(2) })
      : t('info.mb', { value: Math.round(bytes / 1024 ** 2) });
  }

  function fmtRate(bitsPerSecond: number): string {
    return bitsPerSecond >= 1_000_000
      ? t('info.mbps', { value: (bitsPerSecond / 1_000_000).toFixed(1) })
      : t('info.kbps', { value: Math.round(bitsPerSecond / 1000) });
  }

  /// A download rate, in bytes — the unit a torrent client is read in, and
  /// deliberately not the bits `fmtRate` uses for a video bitrate. The two are
  /// answering different questions and an eightfold difference between them
  /// would be read as one of the figures being wrong.
  function fmtSpeed(bytesPerSecond: number): string {
    return bytesPerSecond >= 1024 ** 2
      ? t('info.mbs', { value: (bytesPerSecond / 1024 ** 2).toFixed(1) })
      : t('info.kbs', { value: Math.round(bytesPerSecond / 1024) });
  }

  function fmtFps(fps: number): string {
    // 23.976 must survive; 25 must not become "25.000".
    const rounded = Math.round(fps * 1000) / 1000;
    return t('info.fps', { value: Number.isInteger(rounded) ? rounded : rounded.toFixed(3) });
  }

  let settingsOpen = $state(false);
  let settingsValues = $state<Record<string, string | null>>({});

  // HDR state of the display under the window: while it is in SDR, HDR output
  // is impossible (target-colorspace-hint is a no-op, mpv always tone-maps) —
  // which the settings dialog spells out.
  let displayHdr = $state<{ supported: boolean; enabled: boolean } | null>(null);
  /// Output devices, read when the dialog opens. mpv builds this list lazily —
  /// the hotplug monitor starts on the first read — so there is nothing to
  /// observe and no reason to keep it fresh while the dialog is closed.
  let audioDevices = $state<{ name: string; description: string }[]>([]);

  /**
   * Output devices, deduplicated by what they are called.
   *
   * mpv lists a device once per audio output that can reach it, and the macOS
   * build carries three — `coreaudio`, `coreaudio_exclusive` and
   * `avfoundation` (verified in the bundled libmpv). The `name` differs by that
   * prefix while the `description` is identical, so the picker showed every
   * speaker two or three times with nothing to tell the copies apart.
   *
   * The first entry for a description wins, which is mpv's own preferred
   * output. What that gives up is exclusive mode, and deliberately: it is an
   * audiophile setting that takes the device away from every other app, it
   * cannot be labelled in this dialog without inventing jargon, and anyone who
   * wants it can name the device in mpv.conf — which is what the footer of this
   * dialog already says is where mpv's own settings live.
   */
  async function loadAudioDevices() {
    const all = await readList('audio-device-list', async (base) => {
      const name = await getProperty(`${base}/name`, 'string').catch(() => null);
      if (!name) return null;
      const description = await getProperty(`${base}/description`, 'string').catch(() => null);
      return { name, description: description?.trim() || name };
    }).catch(() => []);
    const seen = new Set<string>();
    audioDevices = all.filter((device) => {
      if (seen.has(device.description)) return false;
      seen.add(device.description);
      return true;
    });
  }

  /// Position and duration from history, for the file currently opening.
  let resumeHint = $state<{ pos: number; dur: number } | null>(null);
  /// Backstop for the knob hold below: if the file never loads, nothing else
  /// would ever release it.
  let resumeHoldTimer: ReturnType<typeof setTimeout> | undefined;

  /// What the seekbar measures against. Falls back to the remembered duration
  /// while mpv is still opening the file, and hands over the moment it reports
  /// its own — a stale hint (the file was re-encoded) corrects itself.
  const barDuration = $derived(player.duration > 0 ? player.duration : (resumeHint?.dur ?? 0));
  /// The OSC menu when it is one of the two track menus. They share their whole
  /// body (list + delay stepper), which the chapter menu does not — and every
  /// call in there takes 'audio' | 'sub', so the narrowing has to survive into
  /// the event handlers.
  const trackMenu = $derived(openMenu === 'audio' || openMenu === 'sub' ? openMenu : null);
  /// Chapter boundaries as percentages of the bar. The one at zero is dropped:
  /// containers write it for the first chapter almost every time, and a notch
  /// under the very end of the track reads as a rendering artifact rather than
  /// as a boundary.
  const chapterMarks = $derived(
    barDuration > 0
      ? player.chapters
          .filter((c) => c.time > 0 && c.time < barDuration)
          .map((c) => (c.time / barDuration) * 100)
      : [],
  );
  /// The looping segment as a band on the bar. Drawn while only A is set too —
  /// a mark that appears the moment you press the key is what tells you the key
  /// did anything, and the band then grows with the playhead.
  const abRegion = $derived.by(() => {
    if (player.loopA === null || barDuration <= 0) return null;
    const from = Math.max(0, Math.min(player.loopA, barDuration));
    const to = Math.max(from, Math.min(player.loopB ?? player.timePos, barDuration));
    // mpv only loops while the position is inside the segment; past B it keeps
    // the marks but stops acting on them, and re-arms when playback returns.
    // The band says which of the two it is, or it would be claiming that
    // something is looping when nothing is.
    const armed = player.loopB === null || player.timePos < player.loopB;
    return {
      left: (from / barDuration) * 100,
      width: ((to - from) / barDuration) * 100,
      armed,
    };
  });

  /// Whether the hover popup can show a frame at all. The storyboard is off for
  /// a stream (it would pull the whole thing again), so the popup must not
  /// reserve space for a picture that is never coming — an empty grey rectangle
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

  // ---- The recents rail ---------------------------------------------------

  let railEl = $state<HTMLDivElement | undefined>();
  /// Both true when the row fits, which is what hides the arrows entirely.
  let railAtStart = $state(true);
  let railAtEnd = $state(true);

  function updateRailEnds() {
    const el = railEl;
    if (!el) return;
    railAtStart = el.scrollLeft <= 1;
    // A pixel of slack: `scrollWidth` and the sum of the fractional layout
    // widths do not always agree to the last unit, and a permanently lit arrow
    // that scrolls nowhere is worse than one that vanishes a pixel early.
    railAtEnd = el.scrollLeft + el.clientWidth >= el.scrollWidth - 1;
  }

  function scrollRail(dir: -1 | 1) {
    const el = railEl;
    if (!el) return;
    // Just under a full view, so the card at the edge stays visible and gives
    // the eye something to carry across — a full-width jump loses the thread.
    el.scrollBy({ left: dir * el.clientWidth * 0.8, behavior: 'smooth' });
  }

  /// Recompute when the list changes: with a short list the rail does not
  /// scroll at all and both arrows have to stay hidden.
  $effect(() => {
    void history.recent.length;
    void tick().then(updateRailEnds);
  });

  /// And when the rail itself is resized — a narrower window turns a row that
  /// fitted into one that scrolls. Observing the element rather than the window
  /// because that is the measurement the arrows actually depend on, and it also
  /// covers the overlay's own scrollbar appearing beside it.
  $effect(() => {
    const el = railEl;
    if (!el) return;
    const ro = new ResizeObserver(updateRailEnds);
    ro.observe(el);
    updateRailEnds();
    return () => ro.disconnect();
  });

  /// Whether the hover popup has a frame to show. Read from the thumbnail
  /// module rather than from the source kind: a torrent earns previews part-way
  /// through playback, when its file finishes downloading, which no test of the
  /// URL could tell you.
  const hasThumbs = $derived(thumbs.available);

  const hoverChapter = $derived(
    hoverTime !== null && player.hasChapters ? chapterAt(hoverTime) : null,
  );

  // ---- "Skip intro" -------------------------------------------------------
  // A chapter that names itself an opening, a recap or the credits is offered
  // for skipping — but only for the first few seconds of it, because after that
  // the viewer has evidently decided to watch.

  /// The button's deadline, in seconds of playback — bounded at both ends on
  /// purpose, and the shorter of the two always wins.
  ///
  /// Longer than the few seconds it started at, because the decision to skip
  /// lands when you recognise the music rather than at the first frame, and a
  /// window that short took the offer away while its reason was still on
  /// screen. Shorter than the chapter itself, because an offer that sits over
  /// the picture for a whole minute stops reading as an offer. And never longer
  /// than the chapter: a five-second bumper must not leave a button hanging for
  /// three times as long as the thing it offers to skip — which also keeps a
  /// title the detector misreads ("Introduction" on a two-hour lecture) from
  /// producing anything worse than a brief button.
  const SKIP_MAX_WINDOW = 20;

  /// Aspect overrides offered in the picture submenu. The label is what mpv is
  /// told; the ratio is what it answers with, which is why both are here.
  const ASPECTS = [
    { label: '16:9', ratio: 16 / 9 },
    { label: '4:3', ratio: 4 / 3 },
    { label: '2.35:1', ratio: 2.35 },
  ];

  /// Repeat state, named for the context menu and the tooltip.
  const LOOP_LABEL = { off: 'loop.off', all: 'loop.all', one: 'loop.one' } as const;

  // ---- End of file --------------------------------------------------------
  // `keep-open=always` parks mpv on the last frame of every entry rather than
  // only the last one, so the moment a file ends belongs to us: the end screen,
  // the countdown, and the wrap of a repeating queue (which mpv's own
  // loop-playlist can no longer perform, because mpv never advances).
  //
  // Repeat-one never gets here at all — mpv loops the file internally and eof
  // is never reached.

  /// How long the next entry waits before starting itself. Long enough to see
  /// what is coming and stop it, short enough not to feel like a stall.
  const ADVANCE_MS = 5000;
  /// Fetch the poster this many seconds before the end, so the card is not
  /// blank at the moment it appears. One decode, and only when a queue exists.
  const POSTER_LEAD = 20;

  // `player.hasFile` rather than the `hasFile` derived below it: this one is
  // evaluated eagerly, and that alias is declared further down the file.
  const ended = $derived(player.hasFile && player.eofReached);
  const nextEntry = $derived(ended ? neighbour(1) : null);
  const prevEntry = $derived(ended ? neighbour(-1) : null);

  let advancing = $state(false);
  /// Bumped per countdown so the CSS bar restarts instead of inheriting a
  /// finished animation.
  let advanceSeq = $state(0);
  let advanceTimer: ReturnType<typeof setTimeout> | undefined;
  /// This end has been dealt with. `eof-reached` can surface more than once for
  /// one ending — the property event and `resyncState` both write the mirror —
  /// and a second pass would silently restart the countdown from five.
  let endHandled = false;

  /// Take the skip offer — from the button or from Enter, which is why it is a
  /// function and not an inline handler.
  ///
  /// The hint is read ONCE, into a local, before anything it depends on is
  /// touched. A $derived is re-evaluated the moment it is read again rather
  /// than at the end of the tick, so `skipUsed = skipHint.from` followed by
  /// `skipHint.chapter` threw on that second read: the button vanished (its
  /// block had unmounted) and nothing else happened at all.
  function takeSkip() {
    const hint = skipHint;
    if (!hint) return;
    skipUsed = hint.from;
    if (hint.chapter) seekChapter(hint.chapter.index);
    else if (hint.entry) void playEntry(hint.entry);
  }

  function cancelAdvance() {
    clearTimeout(advanceTimer);
    advancing = false;
  }

  /// The file ended. Decide between waiting and rolling on, and get the cards
  /// something to show either way.
  function onReachedEnd() {
    if (endHandled) return;
    endHandled = true;
    const next = neighbour(1);
    const prev = neighbour(-1);
    // Sequential, because a poster is a keyframe decode and two at once is the
    // greed already cured in the storyboard.
    void (async () => {
      if (next) await ensurePoster(next.path);
      if (prev) await ensurePoster(prev.path);
    })();
    if (!next || !playlist.autoAdvance) return;
    advancing = true;
    advanceSeq++;
    clearTimeout(advanceTimer);
    advanceTimer = setTimeout(() => {
      advancing = false;
      // Re-read rather than closing over `next`: the countdown outlives several
      // frames, and the queue can be edited from the panel while it runs.
      const target = neighbour(1);
      if (target) void playEntry(target);
    }, ADVANCE_MS);
  }
  /// The last moments are faded out rather than cut, so the button leaves the
  /// way it arrived. In seconds, matched by the CSS transition.
  const SKIP_FADE = 0.6;
  /// Chapter whose button has already been used, muted until playback is
  /// genuinely elsewhere (see the time-pos handler). Without it the button
  /// lingers for the ~100 ms until mpv reports the new position, which is long
  /// enough for a second click to skip a chapter the viewer never saw. It is a
  /// race guard and nothing more: coming back to an opening on purpose has to
  /// offer the button again.
  let skipUsed = $state(-1);
  const SKIP_LABEL = {
    intro: 'skip.intro',
    recap: 'skip.recap',
    preview: 'skip.preview',
    credits: 'skip.credits',
    ad: 'skip.ad',
  } as const;

  const skipHint = $derived.by(() => {
    if (!hasFile || !player.hasChapters || stepMode) return null;
    if (openMenu !== null || settingsOpen) return null;
    // From the position rather than the `chapter` mirror: the mirror lags a
    // seek, and the elapsed time below has to be measured against the same
    // chapter the button would skip.
    const here = chapterAt(player.timePos);
    if (!here || here.index === skipUsed) return null;
    const kind = skipKind(here);
    if (!kind) return null;
    // Where the offer leads. Inside the file it is the next chapter; on the
    // LAST chapter — closing credits, nearly always — there is nowhere left to
    // seek, so the offer becomes the next entry in the queue. Without one there
    // is nothing to propose and no button.
    //
    // **And the next entry has to be a different file.** `neighbour` answers
    // "what plays next", which under repeat is legitimately *this* file again:
    // with one entry and repeat-all it wraps straight back to index 0, and it
    // does the same from a stale `playlist-pos` of -1 (mirrors go stale, see
    // gotcha 3). Either way the credits of a lone episode grew a "Следующая
    // серия" button that restarted what was already playing. The two questions
    // are genuinely different — repeating one file is what "repeat all" on a
    // one-entry queue means, so the fix belongs here and not in `neighbour` —
    // and it is settled two ways because they fail differently: the index is
    // the thing that is stale in the second case, and `filePath` is mpv's own
    // answer to "what is open". Either of them calling it the same file is
    // enough to withdraw the offer — a queue holding one file twice is the only
    // thing that costs, and suppressing a button that would reopen an identical
    // file is not a loss.
    const chapter = player.chapters[here.index + 1] ?? null;
    const next = chapter ? null : neighbour(1);
    const sameFile = next && (next.path === player.filePath || next.index === player.playlistPos);
    const entry = next && !sameFile ? next : null;
    if (!chapter && !entry) return null;
    // The last chapter ends where the file does.
    const ends = chapter ? chapter.time : player.duration;
    if (ends <= here.time) return null;
    const span = Math.min(ends - here.time, SKIP_MAX_WINDOW);
    const left = here.time + span - player.timePos;
    if (left <= 0 || left > span) return null;
    // Both readouts come from the position rather than from a CSS clock, so
    // they survive a seek into the middle of the chapter: the bar shows what is
    // actually left, and pausing freezes it for free (time-pos simply stops).
    return {
      kind,
      from: here.index,
      chapter,
      entry,
      left: left / span,
      fade: Math.min(1, left / SKIP_FADE),
    };
  });
  const osdState = $derived(osd());
  const hasFile = $derived(player.hasFile);
  /// mpv's own title when it has one; failing that, the name the site gave us
  /// (`resolvedTitle`), and only then the tidied file name — which for a link
  /// is a video id and tells the viewer nothing.
  const displayTitle = $derived(
    player.mediaTitle ? player.displayTitle : (resolvedTitle ?? player.displayTitle),
  );
  const idle = $derived(
    hasFile && !dragging && !oscHover && !barHover && !uiVisible && openMenu === null && !settingsOpen
      // While casting the window is a remote control and a status display —
      // there is no picture being watched under the chrome, so hiding it
      // buys nothing and hiding the controls of a remote is actively wrong.
      && !cast.active,
  );

  // macOS: the system window buttons live outside the DOM, so they have to be
  // dimmed by a separate command in step with the title bar's CSS fade (0.25 s)
  // — otherwise they hang around after the rest of the UI is gone. Shown
  // immediately, hidden once the animation is over.
  let winButtonsTimer: ReturnType<typeof setTimeout> | undefined;
  let winButtonsShown = true;
  // The system file dialog: while it is open, the window is dimmed.
  let fileDialogOpen = $state(false);

  // Well inside the CSS fade (0.25 s): the native buttons cannot be faded, they
  // vanish in one step, so matching the *end* of the animation makes them a
  // lagging tail. Even mid-fade read as late — a hard cut is noticed at once
  // while a fade is still visibly on its way out, so it has to land early to
  // feel simultaneous. This is a taste knob; lower it further if they still
  // linger, but not to 0, or they lead instead.
  const WIN_BUTTONS_HIDE_MS = 40;

  function syncWindowButtons(hidden: boolean, immediate = false) {
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
  let cursorHidden = $state(false);
  let cursorTimer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    clearTimeout(cursorTimer);
    if (!idle || pointerInTitlebar) {
      // Showing it again is never delayed: that half is a response to the
      // pointer moving, and any lag there is felt immediately.
      cursorHidden = false;
      return;
    }
    // Windows has no native chrome to relay out, so there is nothing to wait for.
    if (!IS_MAC) {
      cursorHidden = true;
      return;
    }
    cursorTimer = setTimeout(() => (cursorHidden = true), CURSOR_HIDE_MS);
  });

  $effect(() => {
    // Overlays dim the window, but the traffic lights are native views ABOVE
    // the webview and no HTML can cover them: they stay bright on top of the
    // dimming and read as a z-index bug. So hide them explicitly, immediately.
    const overlay = settingsOpen || fileDialogOpen;
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
    syncWindowButtons(overlay || idle || mini.on, overlay || mini.on);
  });

  let unlisteners: Array<() => void> = [];
  let hideTimer: ReturnType<typeof setTimeout> | undefined;
  let fsTransitionTimer: ReturnType<typeof setTimeout> | undefined;
  let preSeekTimer: ReturnType<typeof setTimeout> | undefined;
  let prewarmTimer: ReturnType<typeof setTimeout> | undefined;
  let seekMoved = false;
  /// We paused playback for the duration of the drag — resume it afterwards.
  let dragResume = false;
  /// The final seek is still in flight: the knob stays at the released position.
  let seekSettling = false;
  let stepBusy = false;
  let playbackRestartWaiters: Array<() => void> = [];
  let lastTaskbarUpdate = 0;
  let resumePending: { pos: number; paused: boolean } | null = null;
  /// The load went out with mpv's `start` option pointing at the resume
  /// position, so onFileLoaded must not seek again (see beforeLoad).
  let startPrimed = false;
  let videoDragStart: { x: number; y: number } | null = null;
  let suppressNextClick = false;

  // Pan gesture bookkeeping; the zoom maths itself lives in lib/zoom.svelte.ts.
  let panning = false;
  let panMoved = false;
  let panLast: { x: number; y: number } | null = null;
  // Debounced "empty" state: switching files in the playlist blanks filename
  // for a moment, and the file-picker screen must not flash.
  let showEmpty = $state(true);
  /// The decoder mpv actually picked (`hwdec-current`), shown in the settings:
  /// exact-seek speed depends on it — on 4K60 the difference between hardware
  /// and software decoding is plainly visible, and a silent fallback to
  /// software was previously impossible to notice.
  let hwdecCurrent = $state<string | null>(null);
  /// Settings dialog tab, split by who owns the setting: "Video" is what ends
  /// up in mpv.conf, "General" is what the player keeps itself. "General" by
  /// default: video options are fine-tuning, reached deliberately, whereas the
  /// first tab is more often opened by accident.
  /// Which section of the settings dialog is showing. Topical rather than the
  /// old two-way split by who owns the value — the ownership boundary is still
  /// real (only some of these end up in mpv.conf) but it is a poor map for
  /// someone looking for "subtitle size", which is what a settings dialog is
  /// searched by. What the boundary still owns is the footer: it now names mpv's
  /// settings rather than claiming everything on the page is one of them.
  type SettingsTab = 'general' | 'video' | 'playback' | 'audio' | 'subs' | 'torrents' | 'tv' | 'keys';
  let settingsTab = $state<SettingsTab>('general');
  // The tab row is nowrap with hidden overflow, and the sheet's width is sized
  // from it (598px fits seven Russian labels with ~50px slack) — which is why
  // this tab is «ТВ», not «Трансляция»: two characters ride in the slack, a
  // ten-character label overflows and the eighth tab is silently gone.
  const SETTINGS_TABS: { id: SettingsTab; label: string }[] = $derived([
    { id: 'general', label: t('set.tab_general') },
    { id: 'video', label: t('set.tab_video') },
    { id: 'playback', label: t('set.tab_playback') },
    { id: 'audio', label: t('set.tab_audio') },
    { id: 'subs', label: t('set.tab_subs') },
    { id: 'torrents', label: t('set.tab_torrents') },
    { id: 'tv', label: t('set.tab_tv') },
    { id: 'keys', label: t('set.tab_keys') },
  ]);

  // ---- The TV (casting) tab ----
  let castModeVal = $state(castMode());
  let castCapVal = $state(castCacheCapGb());
  let castCacheBytes = $state<number | null>(null);
  const CAST_CAP_CHOICES = [0, 5, 20, 50];

  function setCastModeHere(mode: 'prepare' | 'hls') {
    castModeVal = mode;
    setCastMode(mode);
  }

  function setCastCapHere(gb: number) {
    castCapVal = gb;
    setCastCacheCapGb(gb);
  }

  async function refreshCastCacheSize() {
    castCacheBytes = await invoke<number>('cast_cache_size').catch(() => null);
  }

  $effect(() => {
    if (settingsOpen && settingsTab === 'tv') void refreshCastCacheSize();
  });

  async function clearCastCacheHere() {
    const freed = await invoke<number>('cast_clear_cache').catch(() => 0);
    showOsd(t('cast.cache_cleared', { size: fmtSize(freed) }));
    void refreshCastCacheSize();
  }
  // Popup elements: measured after render (before paint) to clamp to the window
  let ctxEl = $state<HTMLDivElement | undefined>();
  let tipEl = $state<HTMLDivElement | undefined>();
  let chapterMenuEl = $state<HTMLDivElement | undefined>();
  let queueMenuEl = $state<HTMLDivElement | undefined>();
  async function openSettings() {
    void loadAudioDevices();
    try {
      const conf = await invoke<{ path: string; options: [string, string][] }>('user_mpv_conf');
      player.mpvConfPath = conf.path;
      const map: Record<string, string | null> = {};
      for (const s of SETTINGS) map[s.key] = null;
      for (const [k, v] of conf.options) if (k in map) map[k] = v;
      settingsValues = map;
      void refreshHdrStatus();
      hwdecCurrent = hasFile
        ? ((await getProperty('hwdec-current', 'string').catch(() => null)) ?? null)
        : null;
      settingsOpen = true;
    } catch (e) {
      showOsd(t('osd.settings_failed'));
      console.warn('openSettings failed:', e);
    }
  }

  async function refreshHdrStatus() {
    const st = await invoke<{ supported: boolean; enabled: boolean }>('hdr_status').catch(
      () => null,
    );
    if (!st) return;
    const prev = displayHdr;
    displayHdr = st;
    if (prev && prev.enabled !== st.enabled) {
      showOsd(t(st.enabled ? 'osd.display_hdr' : 'osd.display_sdr'));
      // Embedded through wid, mpv may not notice the HDR switch
      // (WM_DISPLAYCHANGE is only broadcast to top-level windows) — re-apply
      // the hint so the VO re-reads the output colour space.
      const hint =
        (await getProperty('target-colorspace-hint', 'string').catch(() => null)) ?? 'no';
      if (hasFile && hint !== 'no') {
        try {
          await command('set', ['target-colorspace-hint', 'no']);
          await command('set', ['target-colorspace-hint', hint]);
        } catch (e) {
          console.warn('hdr re-hint failed:', e);
        }
      }
    }
  }

  /// Pending mpv.conf writes, one timer per key. A slider fires on every pixel
  /// of the drag: applying that live is free, but writing the file each time is
  /// not — and a single shared timer would let a second slider cancel the first
  /// one's write.
  const confWrites = new Map<string, ReturnType<typeof setTimeout>>();
  const CONF_WRITE_MS = 400;

  function setSetting(def: SettingDef, v: string | null, debounce = false) {
    settingsValues = { ...settingsValues, [def.key]: v };
    // apply live, without a restart
    void command('set', [def.key, v ?? def.liveDefault]).catch(() => {});
    // persist into mpv.conf (surgically, leaving the rest alone)
    clearTimeout(confWrites.get(def.key));
    const write = () => {
      confWrites.delete(def.key);
      void invoke('mpv_conf_set', { key: def.key, value: v }).catch((e) => {
        showOsd(t('osd.setting_failed'));
        console.warn('mpv_conf_set failed:', e);
      });
    };
    if (debounce) confWrites.set(def.key, setTimeout(write, CONF_WRITE_MS));
    else write();
  }

  // ---- Content languages (ROADMAP 25) --------------------------------------
  //
  // `alang`/`slang` are ordered priority lists, and what is stored is a plain
  // mpv.conf line — so the list is *derived* from the line rather than kept
  // beside it. That is what makes a value someone typed into their own mpv.conf
  // show up here correctly, and it is why nothing needs migrating: the two
  // values this dialog used to write, `rus,ru` and `eng,en`, parse back to
  // exactly [ru] and [en].

  /// Which setting's language list is open for adding, and what has been typed.
  /// One at a time: two open search panels in a dialog this size is noise.
  let langPickerFor = $state<string | null>(null);
  let langQuery = $state('');
  let langQueryEl = $state<HTMLInputElement | undefined>();

  function langsOf(def: SettingDef): string[] {
    return parseLangList(settingsValues[def.key] ?? null);
  }

  /// Write a language list back. An empty list clears the mpv.conf line rather
  /// than writing an empty one — "авто" is the absence of the setting, the same
  /// meaning "по умолчанию" has in every pill in this dialog.
  function setLangs(def: SettingDef, codes: string[]) {
    setSetting(def, codes.length ? mpvLangValue(codes) : null);
  }

  async function openLangPicker(def: SettingDef) {
    langPickerFor = langPickerFor === def.key ? null : def.key;
    langQuery = '';
    if (!langPickerFor) return;
    await tick();
    langQueryEl?.focus();
  }

  function addLang(def: SettingDef, code: string) {
    setLangs(def, [...langsOf(def), code]);
    langPickerFor = null;
    langQuery = '';
  }

  function removeLang(def: SettingDef, code: string) {
    setLangs(
      def,
      langsOf(def).filter((c) => c !== code),
    );
  }

  /// Clicking a chip makes it the preferred language.
  ///
  /// Order is priority, and without this the only way to change it is to remove
  /// every language above the one you want and add them back — which also
  /// appends them, so it does not even work. One click on the thing you want
  /// beats a pair of arrows on every chip, in a row that is usually two items
  /// long.
  function promoteLang(def: SettingDef, code: string) {
    const codes = langsOf(def);
    if (codes[0] === code) return;
    setLangs(def, [code, ...codes.filter((c) => c !== code)]);
  }

  /// Current value of a slider: what mpv.conf says, or mpv's own default.
  function sliderValue(def: SliderDef): number {
    const saved = settingsValues[def.key];
    const value = Number(saved ?? def.liveDefault);
    return Number.isFinite(value) ? Math.min(def.max, Math.max(def.min, value)) : Number(def.liveDefault);
  }

  function setSlider(def: SliderDef, value: number) {
    // Landing exactly on mpv's own default removes the line from mpv.conf
    // rather than pinning the default in writing — the same meaning "по
    // умолчанию" has in every pill above.
    // Snapped to the declared precision before it is written: a range input
    // can hand back 1.2000000000000002, and that would go into mpv.conf as is.
    const snapped = Number(value.toFixed(def.decimals));
    const isDefault = Math.abs(snapped - Number(def.liveDefault)) < 1e-9;
    setSetting(def, isDefault ? null : String(snapped), true);
  }

  /// The language also lives in the native macOS menu, which is built in Rust
  /// and cannot reach the dictionary — push the choice over and let it rebuild.
  function changeLocale(next: Locale) {
    setLocale(next);
    if (IS_MAC) {
      void invoke('set_menu_locale', { locale: next })
        .then(() => syncMenuChecks())
        .catch(() => {});
    }
  }

  // Custom tooltips: one delegated mechanism for every [data-tip]
  let tooltip = $state<{ text: string; pos: { x: number; y: number } | null } | null>(null);
  let tipTimer: ReturnType<typeof setTimeout> | undefined;
  let emptyTimer: ReturnType<typeof setTimeout> | undefined;
  // The video title (centred in the title bar) is driven by hand rather than
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
    // "Open with" file and the post-update resume: both are learned BEFORE the
    // window is shown — if a file is on its way, the start screen must not
    // flash in front of the video (a black background holds until it loads).
    const initial = await invoke<string[]>('take_pending_files').catch(() => []);
    const resumeRaw = localStorage.getItem('frameplayer.resume');
    localStorage.removeItem('frameplayer.resume');
    if (initial.length || resumeRaw) {
      showEmpty = false;
      // The OSC is on screen from here on, so prime it before mpv even starts.
      if (initial.length) primeResume(initial[0]);
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
    if (showEmpty) { void loadRecent(); void refreshTorrents(); }
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
        if (ytdlpBusy) ytdlpPct = e.payload;
      }),
    );

    unlisteners.push(
      ...(await initPlayer({
        beforeLoad: async (path) => {
          // Opening another file while casting ends the session first: mpv is
          // about to start playing locally, and two playbacks at once — one
          // here, one on the TV — is never what a viewer meant. (Casting the
          // new file instead is phase 2; see casting.md.)
          if (cast.active) await endCast({ osd: t('cast.stopped'), resumeLocal: false });
          cancelStep();
          attempting = path;
          clearTimeout(loadFailTimer);
          opening = isNetworkSource(path);
          primeResume(path);
          // Decode straight from the resume point: the old "load, then exact
          // seek on file-loaded" presented frame 0 for a beat before jumping.
          // `start` is a load-time option, so it must land before loadfile —
          // the hook is awaited for exactly this reason. Reset in onFileLoaded.
          const target = resumePending?.pos ?? resumeHint?.pos;
          startPrimed = target !== undefined;
          await setProperty('start', startPrimed ? String(target) : 'none').catch(() => {});
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
          for (const w of playbackRestartWaiters) w();
          playbackRestartWaiters = [];
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

    // Display HDR status: at start and every 3 s (reacts to the Windows HDR
    // toggle and to moving the window to another monitor; the call is microseconds)
    void refreshHdrStatus();
    const hdrTimer = setInterval(() => void refreshHdrStatus(), 3000);
    unlisteners.push(() => clearInterval(hdrTimer));

    unlisteners.push(
      await getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type === 'drop' && event.payload.paths.length > 0) {
          void loadFiles(event.payload.paths);
        }
      }),
    );

    // update check: shortly after start and every 6 hours
    const checkUpdates = async () => {
      try {
        updateAvail = await check();
      } catch {
        // offline or the manifest is unreachable — stay quiet
      }
    };
    setTimeout(() => void checkUpdates(), 3000);
    const updTimer = setInterval(() => void checkUpdates(), 6 * 3600 * 1000);
    unlisteners.push(() => clearInterval(updTimer));

    const win = getCurrentWindow();
    isMaximized = await win.isMaximized().catch(() => false);
    unlisteners.push(
      await win.onResized(() => {
        void win
          .isMaximized()
          .then((m) => (isMaximized = m))
          .catch(() => {});
        // fullscreen can also change from outside — keep the mirror honest
        void win
          .isFullscreen()
          .then((f) => (fullscreen = f))
          .catch(() => {});
        // adaptive veil release: a little after the last resize event
        // (~2 frames, which is what mpv's child window needs to catch up)
        if (fsTransition) {
          clearTimeout(fsTransitionTimer);
          fsTransitionTimer = setTimeout(releaseVeil, 60);
        }
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
        resumePending = { pos: Math.max(0, r.pos - RESUME_OFFSET), paused: r.paused };
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
      suppressNextClick = false;
      const target = e.target as HTMLElement | null;
      // This handler runs in the pointerdown capture phase, i.e. before the
      // click on the video. Without the flag, onVideoClick would see an already
      // closed menu, fail to recognise a "closing" click and toggle pause.
      if (ctxMenu && !target?.closest('.ctxmenu')) {
        ctxMenu = null;
        closeSubmenuNow();
        suppressNextClick = true;
      }
      if (openMenu && !target?.closest('.menu') && !target?.closest('.menu-toggle')) {
        openMenu = null;
        suppressNextClick = true;
      }
    };
    // Trackpad scroll gesture phase (macOS only, see macos_chrome.rs).
    unlisteners.push(
      await listen<boolean>('frameplayer://scroll-phase', (e) => {
        wheelFingersDown = e.payload;
        if (!wheelFingersDown) {
          // Fingers lifted — the gesture is now allowed to end.
          scheduleScrubEnd();
          // ...and the axis may be picked afresh without waiting for the timeout.
          wheelAxis = null;
          wheelAccum = 0;
          wheelSeekAccum = 0;
        }
      }),
    );

    window.addEventListener('pointerdown', closeMenusOnOutsideClick, true);
    unlisteners.push(() => window.removeEventListener('pointerdown', closeMenusOnOutsideClick, true));
  });

  onDestroy(() => {
    for (const un of unlisteners) un();
    clearTimeout(hideTimer);
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
        if (player.paused && USE_STEP_ENGINE && !stepMode) schedulePrewarm();
        updateTaskbarProgress();
        if (player.paused) maybeRecordPosition(true);
        break;
      case 'time-pos':
        // While a gesture owns the position, the gesture sets the knob.
        // Otherwise it jitters: mpv still reports the old position (a seek is
        // in flight) or one that landed on a keyframe, and the knob bounces
        // between the finger and the lagging value.
        if (!dragging && !stepMode && !scrubbing && !seekSettling) seekValue = player.timePos;
        // Release the skip button's mute once the position has actually left
        // the chapter it was used on. In the `property` hook rather than an
        // $effect for the usual reason: this must run on a position report, not
        // whenever one of the values it reads happens to change.
        if (skipUsed >= 0 && chapterAt(player.timePos)?.index !== skipUsed) skipUsed = -1;
        // Decode the next entry's poster shortly before it is needed: it is a
        // keyframe decode, and starting it when the end screen is already up
        // means the card appears empty and fills in afterwards. `ensurePoster`
        // is a no-op once cached, so calling it per report costs nothing.
        if (player.duration > 0 && player.duration - player.timePos < POSTER_LEAD) {
          const soon = neighbour(1);
          if (soon) void ensurePoster(soon.path);
        }
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
          emptyTimer = setTimeout(() => {
            showEmpty = true;
            void loadRecent();
            void refreshTorrents();
            barTitleText = '';
            titleSlide = '';
          }, 300);
        } else {
          showEmpty = false;
          // the pause lets media-title update before the slide
          setTimeout(() => slideTitle(), 120);
        }
        break;
      case 'eof-reached':
        if (player.eofReached) {
          onReachedEnd();
        } else {
          endHandled = false;
          cancelAdvance();
        }
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
        // Chapter indices mean nothing across files — a stale one would
        // silently suppress the skip button in the new file.
        skipUsed = -1;
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

  // ---- Priming the seekbar for a file that is still opening ---------------
  // mpv reports `duration` only once it has opened the file, and the OSC is on
  // screen before that — so the knob is drawn as `seekValue / 0` and sits at the
  // far left for a moment, then jumps. The history record we are about to resume
  // from already holds both numbers (the "resuming" popup prints them), so the
  // bar uses them until mpv has its own.

  function primeResume(path: string) {
    resumeHint = null;
    clearTimeout(resumeHoldTimer);
    const saved = positionsLoad()[sourceId(path)];
    if (!saved || saved.pos <= 15) {
      // Nothing to resume: clear the knob instead of leaving the previous
      // file's position to be drawn against the new file's duration.
      seekValue = 0;
      return;
    }
    resumeHint = { pos: Math.max(0, saved.pos - RESUME_OFFSET), dur: saved.dur };
    seekValue = resumeHint.pos;
    // Hold the knob there: until the file is open, `time-pos` reports zero for
    // it and would write that straight back.
    seekSettling = true;
    resumeHoldTimer = setTimeout(() => (seekSettling = false), 8000);
  }

  /// Seek to where the viewer left off, with the knob taken along.
  ///
  /// Without moving `seekValue` here the knob sits at zero until mpv reports
  /// the new position — a second or so on a 4K file — and then jumps across the
  /// bar in full view. `seekSettling` is what keeps it there: the time-pos
  /// handler would otherwise write mpv's still-zero position straight back.
  function seekOnResume(target: number) {
    seekValue = target;
    seekSettling = true;
    void command('seek', [target, 'absolute+exact'])
      .catch(() => {})
      .finally(() => {
        clearTimeout(resumeHoldTimer);
        seekSettling = false;
      });
  }

  function onFileLoaded() {
    // Something opened, so whatever failed on the way here was a step and not
    // an outcome.
    clearTimeout(loadFailTimer);
    attempting = '';
    opening = false;
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
    endHandled = false;
    cancelAdvance();
    void loadPlaylist();
    // Only when mpv came up empty-handed: normally yt-dlp has already told it
    // the name, and asking the site again would be a request for nothing.
    resolvedTitle = null;
    const src = player.filePath;
    if (src && isNetworkSource(src)) {
      setTimeout(() => {
        if (player.filePath !== src || player.mediaTitle) return;
        void resolveTitle(src).then((title) => {
          if (title && player.filePath === src) resolvedTitle = title;
        });
      }, 1500);
    }
    // `start` is sticky: reset it so the next playlist entry (which loads
    // without going through beforeLoad) starts from zero, not from here.
    const primed = startPrimed;
    startPrimed = false;
    void setProperty('start', 'none').catch(() => {});
    if (primed) {
      // mpv is already decoding at the resume point — just release the knob.
      clearTimeout(resumeHoldTimer);
      seekSettling = false;
    }
    if (resumePending) {
      const r = resumePending;
      resumePending = null;
      if (!primed) seekOnResume(r.pos);
      if (r.paused) void setProperty('pause', true);
    } else if (player.filePath) {
      // resume playback: the same position minus the offset
      const saved = positionsLoad()[sourceId(player.filePath)];
      if (saved && saved.pos > 15) {
        if (!primed) seekOnResume(Math.max(0, saved.pos - RESUME_OFFSET));
        // saved.dur is more reliable than duration: for a file that just
        // opened, the mpv property may not have arrived yet.
        const total = saved.dur > 0 ? saved.dur : player.duration;
        showOsd(t('osd.resume'), {
          sub: total > 0
            ? `${formatTime(saved.pos)} / ${formatTime(total)}`
            : formatTime(saved.pos),
        });
      }
    }
  }

  // ---- Remembered audio / subtitle track ----------------------------------
  // Applied once the track it names actually exists. It cannot be a one-shot on
  // file-loaded: external subtitles found by sub-auto turn up a beat later, and
  // setting `sid` to an id that does not exist yet simply selects nothing.

  /// Pending restore, with a deadline so a track that never appears (the file
  /// was re-encoded, the external subtitle is gone) does not leave us waiting
  /// and then hijacking a choice the viewer made by hand in the meantime.
  type PendingTracks = {
    path: string;
    audio?: TrackWish;
    sub?: TrackWish;
    /// Legacy ids, exact in the file they were stored for.
    aid?: string;
    sid?: string;
    /// How good the match we already acted on was, per kind. A better one can
    /// still turn up: external subtitles arrive a beat after `file-loaded`, and
    /// an .srt named like the episode should win over a generic embedded track.
    applied: { audio: number; sub: number };
    until: number;
  };
  let pendingTracks: PendingTracks | null = null;

  function restoreTrackChoice() {
    pendingTracks = null;
    if (!player.filePath) return;
    const legacy = trackChoiceFor(player.filePath);
    const audio = trackWishFor(player.filePath, 'audio');
    const sub = trackWishFor(player.filePath, 'sub');
    if (!legacy && !audio && !sub) return;
    pendingTracks = {
      path: player.filePath,
      audio: audio ?? undefined,
      sub: sub ?? undefined,
      aid: legacy?.aid,
      sid: legacy?.sid,
      applied: { audio: 0, sub: 0 },
      until: Date.now() + 5000,
    };
  }

  /**
   * Resolve one remembered choice against the list this file actually has.
   *
   * Returns the score it acted on, or null while there is nothing to act on
   * yet — the caller keeps waiting, because the list is still filling up.
   * A missing track is not a failure: no candidate clears the matcher's floor,
   * mpv's own `alang`/`slang` choice stands, and that is the right answer for
   * an episode that simply has no Russian dub.
   */
  function applySavedTrack(
    kind: 'audio' | 'sub',
    wish: TrackWish | undefined,
    legacyId: string | undefined,
    list: Track[],
    already: number,
  ): number | null {
    const prop = kind === 'audio' ? 'aid' : 'sid';
    if (wish === 'no') {
      if (already > 0) return already;
      void command('set', [prop, 'no']).catch(() => {});
      return Number.MAX_SAFE_INTEGER;
    }
    if (wish) {
      const found = matchTrack(list, wish);
      if (!found || found.score <= already) return already || null;
      if (!found.track.selected) {
        void command('set', [prop, String(found.track.id)]).catch(() => {});
      }
      return found.score;
    }
    // No descriptor: an entry written before choices described themselves.
    if (!legacyId) return Number.MAX_SAFE_INTEGER;
    if (legacyId !== 'no' && !list.some((track) => String(track.id) === legacyId)) return null;
    void command('set', [prop, legacyId]).catch(() => {});
    return Number.MAX_SAFE_INTEGER;
  }

  $effect(() => {
    // Reading both lists is what re-runs this as tracks appear.
    const audio = player.audioTracks;
    const subs = player.subTracks;
    const want = pendingTracks;
    if (!want) return;
    if (player.filePath !== want.path || Date.now() > want.until) {
      pendingTracks = null;
      return;
    }
    const audioScore = applySavedTrack('audio', want.audio, want.aid, audio, want.applied.audio);
    const subScore = applySavedTrack('sub', want.sub, want.sid, subs, want.applied.sub);
    if (audioScore === want.applied.audio && subScore === want.applied.sub) return;
    want.applied = { audio: audioScore ?? want.applied.audio, sub: subScore ?? want.applied.sub };
    // The `selected` flags in the lists are now stale, and they are what the
    // track menu ticks — re-read once mpv has acted on the change. The pending
    // entry stays until its deadline: a better match can still arrive, and a
    // manual pick clears it in selectTrack.
    setTimeout(() => void loadTracks(), 200);
  });

  // Poll a torrent's peers and rate while one is playing, and stop the moment
  // it is not. Driven by `path` inside the module rather than started when the
  // magnet was opened, because moving to the next episode of a torrent is a
  // playlist transition nothing else would report.
  trackTorrentPlayback();

  /// Play/pause with the frame-step overlay taken into account: while it is up,
  /// mpv is parked behind it and the first press means "come back to playing".
  async function togglePause() {
    if (!player.hasFile) return;
    if (stepMode) {
      await exitStep(true);
      return;
    }
    await mpvTogglePause();
  }

  /// Nudging a delay, with the result remembered for this source.
  ///
  /// The value is read back rather than computed: `nudgeDelay` uses mpv's `add`
  /// (for the same reason toggles use `cycle`), so what it lands on is mpv's
  /// business, and the mirror may be a beat behind. Debounced because a delay
  /// is dialled in by repeated presses and only the value it settles on is
  /// worth writing.
  let delayWriteTimer: ReturnType<typeof setTimeout> | undefined;

  function rememberDelaySoon(kind: 'sub' | 'audio') {
    clearTimeout(delayWriteTimer);
    delayWriteTimer = setTimeout(async () => {
      if (!player.filePath) return;
      const value = await getProperty(kind === 'sub' ? 'sub-delay' : 'audio-delay', 'double').catch(
        () => null,
      );
      if (typeof value === 'number') rememberDelay(player.filePath, kind, value);
    }, 400);
  }

  function nudgeDelayHere(kind: 'sub' | 'audio', delta: number) {
    nudgeDelay(kind, delta);
    rememberDelaySoon(kind);
  }

  function resetDelayHere(kind: 'sub' | 'audio') {
    resetDelay(kind);
    if (player.filePath) rememberDelay(player.filePath, kind, 0);
  }

  /// mpv keeps `sub-delay` across a file change (measured), so a correction
  /// dialled in for one episode silently applies to the next. Every file
  /// therefore gets an explicit value — its own, or zero.
  function applyDelays() {
    if (!player.filePath) return;
    const saved = delaysFor(player.filePath);
    void setProperty('sub-delay', saved.sub).catch(() => {});
    void setProperty('audio-delay', saved.audio).catch(() => {});
  }

  function selectTrack(kind: 'audio' | 'sub', track: Track | null) {
    openMenu = null;
    // A deliberate choice, so it is worth remembering — and it also cancels a
    // restore still waiting for its track to show up.
    pendingTracks = null;
    if (player.filePath) {
      const list = kind === 'audio' ? player.audioTracks : player.subTracks;
      rememberTrack(player.filePath, kind, track ? describeTrack(track, list) : 'no');
    }
    void mpvSelectTrack(kind, track);
    // While the TV owns playback, an audio choice must reach it too: the
    // prepared file carries exactly one track, so the switch is a re-prepare
    // (cached per track — switching back is instant) plus a reload at the
    // TV's position. The local switch above still runs, so the handback and
    // the menu's check mark stay in agreement with what the TV plays.
    if (cast.remote && kind === 'audio' && track) void castSwitchAudio(track);
  }

  // The system file dialog dims the window while it is up, so both pickers go
  // through the flag the overlay effect watches.
  async function openFileDialog() {
    fileDialogOpen = true;
    try {
      await pickAndLoadFiles();
    } finally {
      fileDialogOpen = false;
    }
  }

  async function addTrackFile(kind: 'sub' | 'audio') {
    fileDialogOpen = true;
    try {
      await pickAndAttachTrack(kind);
    } finally {
      fileDialogOpen = false;
    }
  }

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
    if (!hasFile || player.duration <= 0) {
      void win.setProgressBar({ status: ProgressBarStatus.None }).catch(() => {});
      return;
    }
    void win
      .setProgressBar({
        status: player.paused ? ProgressBarStatus.Paused : ProgressBarStatus.Normal,
        progress: Math.min(100, Math.max(0, Math.round((player.timePos / player.duration) * 100))),
      })
      .catch(() => {});
  }

  // ---- Auto-update (signed releases, latest.json manifest on R2) ----------

  async function installUpdate() {
    if (!updateAvail || updatePct !== null) return;
    try {
      let total = 0;
      let done = 0;
      updatePct = 0;
      saveResumeSnapshot();
      await updateAvail.downloadAndInstall((e) => {
        if (e.event === 'Started') total = e.data.contentLength ?? 0;
        else if (e.event === 'Progress') {
          done += e.data.chunkLength;
          if (total > 0) updatePct = Math.round((done / total) * 100);
        } else if (e.event === 'Finished') {
          updatePct = 100;
          // the download may have taken minutes — refresh the position first
          saveResumeSnapshot();
        }
      });
      await relaunch();
    } catch (e) {
      // do not leave the snapshot behind, or a later ordinary launch would
      // suddenly open a video
      dropResumeSnapshot();
      updatePct = null;
      showOsd(t('osd.update_failed'));
      console.warn('update failed:', e);
    }
  }

  /// Builds the list and pulls in posters. Entries whose file vanished drop out
  /// silently: poster_frame returns an error, and keeping something unopenable
  /// in the list is pointless.
  ///
  /// The sequence number is bumped on every call: the list reloads on every
  /// return to the start screen, and posters from the previous pass must not
  /// append themselves to it.

  // Custom seekbar (a native input[type=range] is unusable: the click is mapped
  // accounting for the thumb width — diverging from the preview — and clicking
  // the thumb itself is a dead zone). Click and preview use one formula: the
  // fraction of the track width.
  function seekFrac(e: PointerEvent): number {
    const el = seekWrapEl ?? (e.currentTarget as HTMLElement);
    const rect = el.getBoundingClientRect();
    return Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
  }

  // ---- Seek mode selection (shared by the seekbar and the wheel scrub) -----
  // Whether an exact seek is expensive is a property of the FILE (codec, GOP
  // length, resolution), not of the jump distance. Measured over mpv IPC,
  // 30 steps of 2 s:
  //
  //   HEVC 4K 25fps, GOP 2 s   exact 25 ms    keyframe 25 ms  ← no difference
  //   h264 4K 60fps, GOP 2 s   exact 36 ms    keyframe 37 ms  ← none here either
  //   VP9  4K 60fps, GOP 5 s   exact 1219 ms  keyframe 50 ms  ← 24x
  //
  // On a normal file keyframe mode buys nothing, while SWITCHING modes between
  // the previews and the final seek is plainly visible as a jerk: the preview
  // sits on a keyframe, the final frame on the exact position. So the mode is
  // chosen once per file: a fast file gets exact seeks everywhere (no jerks), a
  // slow one gets keyframes for previews and exactness only where a frame is
  // actually being asked for. IINA works the same way
  // (useExactSeekForCurrentFile, 50 ms threshold).
  const SLOW_SEEK_MS = 250;
  /// The file is deemed "slow" for exact seeks. Reset when the file changes.
  let fileSlowSeek = false;
  let fileSeekProbed = false;

  function resetSeekProbe() {
    fileSlowSeek = false;
    fileSeekProbed = false;
  }

  /// Seek with measurement: the first exact seek on a file also measures
  /// whether it is affordable here. Returns the completion promise — the
  /// follow-up actions (unpausing, unfreezing the knob) hang off it.
  /// Record what an exact seek cost on this file. The first measurement sets
  /// the verdict; after that it only tightens — one fast seek (a cache hit)
  /// must not undo a proven expense.
  function noteSeekCost(startedAt: number) {
    const slow = performance.now() - startedAt > SLOW_SEEK_MS;
    if (!fileSeekProbed) {
      fileSeekProbed = true;
      fileSlowSeek = slow;
    } else if (slow) {
      fileSlowSeek = true;
    }
  }

  function issueSeek(pos: number, exact: boolean): Promise<unknown> {
    const startedAt = performance.now();
    return command('seek', [pos, exact ? 'absolute+exact' : 'absolute+keyframes'])
      .catch(() => {})
      .finally(() => {
        if (exact) noteSeekCost(startedAt);
      });
  }

  /// Whether an exact seek is appropriate right now. On a fast file, always; on
  /// a slow one only for "show me this frame", never for a preview mid-gesture.
  function wantExact(isPreview: boolean): boolean {
    return !fileSlowSeek || !isPreview;
  }

  function onSeekDown(e: PointerEvent) {
    if (!hasFile || player.duration <= 0) return;
    if (stepMode) cancelStep();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    dragging = true;
    seekMoved = false;
    dragShownAt = null;
    dragShownExact = false;
    seekValue = seekFrac(e) * player.duration;
  }

  /// Pause for the duration of the drag. Playing video drifts forward between
  /// throttled preview seeks and the next preview yanks it back — visible as
  /// forward/backward judder, especially on files with sparse keyframes (where
  /// the preview keeps returning to the same frame).
  ///
  /// Armed on the first move rather than on pointer-down: a plain click on the
  /// knob must not touch playback at all. The wheel scrub, by contrast, does
  /// not pause — that gesture self-terminates after 180 ms, and continuing
  /// playback is exactly what adds motion between seek landings.
  function beginDragPause() {
    // While casting, local playback is already parked; the drag drives the TV
    // and there is nothing here to pause.
    if (cast.remote) return;
    // A fresh value, not the mirror: paused may have gone stale after an mpv
    // event queue overflow.
    void getProperty('pause', 'flag')
      .then((p) => {
        if (!p && dragging) {
          dragResume = true;
          void setProperty('pause', true);
        }
      })
      .catch(() => {});
  }

  // ---- Drag previews ------------------------------------------------------
  // One seek in flight (as in the wheel scrub), not "once every 80 ms". A time
  // throttle left the shown frame behind the cursor: on a 57-minute video a
  // ~900 px seekbar is 3.8 s per pixel, so lagging even a few pixels means
  // seconds, and releasing then jumped the video by that gap. With a pump the
  // preview keeps up with the cursor exactly as well as the file allows (25 ms
  // on HEVC 4K, 50 ms on "slow" VP9).
  let dragInFlight = false;
  let dragPending = false;
  /// Where the displayed frame actually sits, and how exact it is.
  let dragShownAt: number | null = null;
  let dragShownExact = false;
  let dragSettleTimer: ReturnType<typeof setTimeout> | undefined;
  /// Positions equal to within float jitter.
  const SAME_POS = 1e-6;

  function pumpDragSeek(exact: boolean) {
    // No previews on a cast drag: every preview would be a seek on the TV,
    // seconds of buffering each. The remote gesture is release-only.
    if (cast.remote) return;
    if (dragInFlight) {
      dragPending = true;
      return;
    }
    const target = seekValue;
    // The frame is already here and exact enough — a repeat seek would only blink.
    if (dragShownAt !== null && Math.abs(dragShownAt - target) < SAME_POS && (dragShownExact || !exact)) {
      return;
    }
    dragInFlight = true;
    void issueSeek(target, exact).finally(() => {
      dragShownAt = target;
      dragShownExact = exact;
      dragInFlight = false;
      const again = dragPending;
      dragPending = false;
      if (again && dragging) pumpDragSeek(wantExact(true));
    });
  }

  /// The cursor stopped — show the EXACT frame of the position it rests on.
  /// While the cursor moves, a "slow" file cannot afford this (seconds per
  /// seek), but stopping is precisely "I am aiming here": this is the frame the
  /// user is studying and expects to see after releasing. It also removes the
  /// jump on release — there will be nothing left to settle.
  function scheduleDragSettle() {
    clearTimeout(dragSettleTimer);
    dragSettleTimer = setTimeout(() => {
      if (dragging) pumpDragSeek(true);
    }, 120);
  }

  function onSeekMove(e: PointerEvent) {
    onSeekHover(e);
    if (!dragging) return;
    if (!seekMoved) beginDragPause();
    seekMoved = true;
    seekValue = seekFrac(e) * player.duration;
    pumpDragSeek(wantExact(true));
    scheduleDragSettle();
  }

  function onSeekUp(e: PointerEvent) {
    if (!dragging) return;
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    seekValue = seekFrac(e) * player.duration;
    // The remote path: one SEEK to the TV on release, none of the local
    // machinery — the probe, the exact/keyframe split and the settle are all
    // about local decode cost. cast.time is set optimistically on send, so the
    // knob holds without a seekSettling equivalent.
    if (cast.remote) {
      dragging = false;
      clearTimeout(dragSettleTimer);
      castSeek(seekValue);
      if (seekWrapEl && !seekWrapEl.matches(':hover')) hoverTime = null;
      return;
    }
    // Releasing means "I want this timestamp", so the final seek is always
    // exact — same as a single click. On a fast file the previews were exact
    // too, so the frame is already there and nothing jumps; on a slow one this
    // is the single keyframe → exact transition of the whole gesture.
    //
    // NOTE (architecture.md, the seekbar lesson): it was exactly this trailing
    // exact seek after keyframe previews that once caused a frame blink, and
    // the lesson read "drag is keyframes-only, including the final one". It is
    // deliberately reinstated here: on fast files there is no mode mixing left
    // at all, and on slow ones the price is one transition instead of the knob
    // bouncing backwards.
    dragging = false;
    clearTimeout(dragSettleTimer);
    // Hold the knob at the released position until the seek lands: otherwise
    // time-pos writes the old (preview) position into it and the knob jitters
    // back and forth.
    seekSettling = true;
    // If the exact frame of this position is already shown (the cursor rested
    // before release, so scheduleDragSettle fired), there is nothing to settle:
    // a superfluous seek right here is what looked like "jumping to a different
    // frame".
    const alreadyThere =
      dragShownExact && dragShownAt !== null && Math.abs(dragShownAt - seekValue) < SAME_POS;
    void (alreadyThere ? Promise.resolve() : issueSeek(seekValue, true)).finally(() => {
      seekSettling = false;
      if (dragResume) {
        dragResume = false;
        void setProperty('pause', false);
      }
    });
    if (player.paused && USE_STEP_ENGINE) schedulePrewarm();
    if (seekWrapEl && !seekWrapEl.matches(':hover')) hoverTime = null;
  }

  function onSeekHover(e: MouseEvent) {
    if (player.duration <= 0) return;
    const wrap = e.currentTarget as HTMLElement;
    const rect = wrap.getBoundingClientRect();
    const frac = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
    hoverTime = frac * player.duration;
    // Clamped by half the popup's width, which depends on whether it carries a
    // thumbnail: keeping the 92px margin for a bare timestamp would stop it
    // following the cursor well before either end of the bar.
    const half = hasThumbs ? 92 : 30;
    hoverX = Math.min(rect.width - half, Math.max(half, e.clientX - rect.left));
    if (hasThumbs) requestThumb(hoverTime, () => hoverTime);
  }

  // ---- Frame-step mode (StepEngine): frames come from the FFmpeg sidecar,
  // mpv sits paused behind the canvas overlay and is walked to the displayed
  // frame by a background pre-seek, so resuming playback is instant.
  function drawStepFrame(buf: ArrayBuffer) {
    const dv = new DataView(buf);
    const w = dv.getUint32(0, true);
    const h = dv.getUint32(4, true);
    const pts = dv.getFloat64(8, true);
    if (stepCanvas) {
      if (stepCanvas.width !== w) stepCanvas.width = w;
      if (stepCanvas.height !== h) stepCanvas.height = h;
      const px = new Uint8ClampedArray(buf, 16);
      stepCanvas.getContext('2d')!.putImageData(new ImageData(px, w, h), 0, 0);
    }
    stepPts = pts;
    seekValue = pts;
  }

  function schedulePreSeek() {
    clearTimeout(preSeekTimer);
    preSeekTimer = setTimeout(() => {
      void command('seek', [stepPts, 'absolute+exact']).catch(() => {});
    }, 150);
  }

  function schedulePrewarm() {
    clearTimeout(prewarmTimer);
    prewarmTimer = setTimeout(() => {
      if (player.filePath && !stepMode) {
        void invoke('step_prewarm', { path: player.filePath, pos: player.timePos }).catch(() => {});
      }
    }, 150);
  }

  async function stepBy(delta: number) {
    if (!hasFile || stepBusy) return;
    if (!USE_STEP_ENGINE) {
      void command(delta < 0 ? 'frame-back-step' : 'frame-step', []);
      return;
    }
    stepBusy = true;
    try {
      if (!stepMode) {
        if (!player.filePath) throw new Error('no path');
        await setProperty('pause', true);
        const pos = (await getProperty('time-pos', 'double').catch(() => null)) ?? player.timePos;
        const entry = await invoke<ArrayBuffer>('step_enter', { path: player.filePath, pos });
        stepMode = true;
        drawStepFrame(entry);
      }
      const buf = await invoke<ArrayBuffer>('step_move', { delta });
      drawStepFrame(buf);
      schedulePreSeek();
    } catch {
      // the sidecar could not cope (exotic format etc.) — fall back to mpv
      cancelStep();
      void command(delta < 0 ? 'frame-back-step' : 'frame-step', []);
    } finally {
      stepBusy = false;
    }
  }

  /** Exit without seeking: used when the user is about to set the position. */
  function cancelStep() {
    if (!stepMode) return;
    stepMode = false;
    clearTimeout(preSeekTimer);
    void invoke('step_exit').catch(() => {});
  }

  /** Syncs mpv to the displayed frame and exits (before a relative seek). */
  function flushStepThenCancel() {
    clearTimeout(preSeekTimer);
    void command('seek', [stepPts, 'absolute+exact']).catch(() => {});
    cancelStep();
  }

  function waitPlaybackSettled(ms: number): Promise<void> {
    return new Promise((resolve) => {
      const timer = setTimeout(resolve, ms);
      playbackRestartWaiters.push(() => {
        clearTimeout(timer);
        resolve();
      });
    });
  }

  /** Exit and resume: the canvas hides only once mpv is on the frame. */
  async function exitStep(unpause: boolean) {
    if (!stepMode) return;
    clearTimeout(preSeekTimer);
    try {
      const settled = waitPlaybackSettled(250);
      await command('seek', [stepPts, 'absolute+exact']);
      await settled;
    } catch {
      // ignore
    }
    stepMode = false;
    void invoke('step_exit').catch(() => {});
    if (unpause) await setProperty('pause', false).catch(() => {});
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

  function onWheel(e: WheelEvent) {
    const onSurface = !!(e.target as HTMLElement | null)?.closest(WHEEL_SURFACES);
    if (e.ctrlKey) {
      // Ctrl+wheel zooms the video around the point under the cursor. On a
      // trackpad this is the pinch gesture, whose sign the device sets itself —
      // "natural scrolling" does not flip it, so there is nothing to invert.
      // preventDefault regardless of where the pointer is: without it the
      // webview zooms the whole UI, which there is no way back from.
      e.preventDefault();
      if (hasFile && !onSurface && !cast.remote) zoomAt(e.clientX, e.clientY, e.deltaY < 0 ? 0.1 : -0.1);
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
      if (!hasFile || player.duration <= 0) return;
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
  // While the window is resizing, mpv's child window relayouts a frame or two
  // behind its parent — the transparent area falls through to the desktop. The
  // veil (above the whole UI, so control jumps are hidden too) masks the
  // artefact; it is released adaptively, shortly after the last resize event,
  // with the timeout only as a safety ceiling.
  // Release order: the shutter hides first (the main window beneath it already
  // wears the black veil), then the veil dissolves — the seam is invisible.
  function releaseVeil() {
    void hideShutter();
    fsTransition = false;
  }

  function fsVeil() {
    fsTransition = true;
    clearTimeout(fsTransitionTimer);
    fsTransitionTimer = setTimeout(releaseVeil, 500);
  }

  // Wait until the veil frame is ACTUALLY on screen before resizing the window.
  // Otherwise the resize starts before the veil's first paint and Windows
  // briefly shows a stretched buffer with the old layout (controls "jump" up
  // and to the left). A black frame stretches into black — no visible artefact.
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
      if (!fsTransition) return;
      if (mon) {
        await shutter.setPosition(new PhysicalPosition(mon.position.x, mon.position.y));
        await shutter.setSize(new PhysicalSize(mon.size.width, mon.size.height));
      }
      await shutter.show();
      // ...and once more after show(), for the same reason.
      if (!fsTransition) {
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

  // The veil and shutter exist for DWM composition artefacts — on macOS the
  // system animates the fullscreen transition and there is nothing to hide.
  async function maskFullscreenTransition() {
    if (IS_MAC) return;
    // The shutter colour must match the veil (the release relies on an
    // invisible seam between them) — announce it before show().
    await emitTo('veil', 'veil-color', showEmpty ? '#101016' : '#000').catch(() => {});
    await presentVeil();
    await showShutter();
  }

  async function toggleFullscreen() {
    // Leaving mini on the way in: `toggleMini` does the same in reverse, and
    // without it fullscreen would restore to a thumbnail in the corner.
    if (mini.on) await toggleMini();
    if (fullscreen) {
      await exitFullscreen();
      return;
    }
    await maskFullscreenTransition();
    fullscreen = true;
    await getCurrentWindow().setFullscreen(true);
  }

  async function exitFullscreen() {
    if (!fullscreen) return;
    await maskFullscreenTransition();
    fullscreen = false;
    await getCurrentWindow().setFullscreen(false);
  }

  /// Seek popup: "24:01 / 53:34" plus a thin progress bar. The position is
  /// optimistic rather than read from timePos (which mpv only updates a frame
  /// or two after the seek), otherwise the popup lags one step behind.
  function showSeekOsd(target: number) {
    if (player.duration <= 0) return;
    const pos = Math.min(player.duration, Math.max(0, target));
    showOsd(`${formatTime(pos)} / ${formatTime(player.duration)}`, { progress: pos / player.duration });
  }

  /// Relative seek with a popup — the shared entry point for arrows and trackpad.
  ///
  /// The coarse step (arrows) goes by keyframes deliberately. A bare `relative`
  /// flag obeys `hr-seek=yes` from initialOptions, i.e. it was exact, and an
  /// exact seek costs decoding a whole GOP: on 4K60 VP9 with a 5 s keyframe
  /// interval that is 1.9 s against 0.05 s (measured over IPC) — the arrow key
  /// responded two seconds later. Precision where it is asked for:
  /// Shift+arrow (a 1 s step) stays exact.
  /**
   * Where a relative seek lands when an A–B loop is running.
   *
   * mpv disarms the loop while the position is past B and re-arms it when the
   * position comes back inside (measured — the marks stay set the whole time),
   * so an arrow key near the end of a segment quietly drops out of it. That is
   * right for a destination ("take me to 42:00") and wrong for a nudge, which
   * means "a bit further along from here" — and here is inside a loop.
   *
   * Modulo rather than a clamp to the start, because a loop is a circle: moving
   * five seconds forward through it should still move five seconds, however
   * short the segment is.
   */
  /**
   * Bounds for the wheel scrub: the file, or the A–B segment while one is set.
   *
   * A continuous gesture is CLAMPED where a key press wraps, and the difference
   * is not arbitrary — it is what the two gestures are. Fingers travelling right
   * mean "later", and wrapping would break that correspondence: a steady swipe
   * would spin the picture around a short loop instead of moving through it. A
   * key press has no such correspondence to keep; it is a step, and a step that
   * clamped would go dead at the edge under repeats.
   *
   * Clamping is also what this gesture already does at the ends of the file, so
   * a segment is the same behaviour with tighter bounds rather than a new rule
   * to learn.
   *
   * The upper bound stops just short of B: landing exactly on it disarms the
   * loop (mpv arms on `position < B` at seek time), and the gesture would end by
   * silently switching the loop off. A frame short is invisible, and playback
   * wraps from there on its own.
   */
  function scrubRange(): { min: number; max: number } {
    if (player.loopA !== null && player.loopB !== null && player.loopB > player.loopA) {
      return { min: player.loopA, max: Math.max(player.loopA, player.loopB - 0.05) };
    }
    return { min: 0, max: player.duration };
  }

  function wrapInAbLoop(target: number): number | null {
    if (player.loopA === null || player.loopB === null) return null;
    const span = player.loopB - player.loopA;
    if (span <= 0) return null;
    if (target >= player.loopA && target < player.loopB) return null;
    return player.loopA + (((target - player.loopA) % span) + span) % span;
  }

  function seekRelative(delta: number, exact = false) {
    if (!hasFile) return;
    if (stepMode) flushStepThenCancel();
    const wrapped = wrapInAbLoop(player.timePos + delta);
    if (wrapped !== null) {
      // An absolute seek: the point is precisely that it is no longer `delta`
      // away from here. Exact, like every other "put me on this frame".
      void issueSeek(wrapped, true);
      showSeekOsd(wrapped);
      return;
    }
    // The coarse step used to be keyframes ALWAYS, which is right on a file
    // where an exact seek costs seconds and wrong everywhere else: a keyframe
    // landing is up to a GOP away, so a 5 s arrow moved 6 on an ordinary file
    // and 10 on a network stream with long fragments. It was always doing that
    // — only the popup, which reported the promise rather than the landing,
    // hid it. Now it follows the same probe as the seekbar and the scrub: exact
    // where that is free, keyframes where it is not.
    const wantsExact = exact || !fileSlowSeek;
    const startedAt = performance.now();
    const seek = command('seek', [delta, wantsExact ? 'relative+exact' : 'relative+keyframes'])
      .catch(() => {})
      .finally(() => {
        // Arrows are often the first seek on a file, so they carry the probe
        // too — otherwise a slow file is never diagnosed until the seekbar is
        // touched.
        if (wantsExact) noteSeekCost(startedAt);
      });
    showSeekOsd(player.timePos + delta);
    // A keyframe seek does not land where the optimistic popup promised — fix
    // it up with the actual position once mpv reports it. If the popup changed
    // meanwhile (another key press), the refinement is dropped.
    if (!wantsExact) refineSeekOsd(seek);
  }

  /**
   * Correct the optimistic popup once the seek has actually landed.
   *
   * A keyframe seek does not stop where the popup promised, so the figure has
   * to be read back — but `command('seek', …)` resolves when mpv ACCEPTS the
   * command, not when it has performed it, and `time-pos` at that moment is
   * still the position before the jump. Reading there made the popup show the
   * target, then flick back to where it started, and stay there: three numbers
   * for one key press. `playback-restart` is the event that means the seek is
   * done, with a timeout so a seek that never completes cannot leave the popup
   * waiting on a promise for ever.
   *
   * The sequence guard stays: with the key held down, an earlier correction
   * must not overwrite a newer press's popup.
   */
  function refineSeekOsd(seek: Promise<unknown>) {
    const shown = osdSeq();
    void seek
      .then(() => waitPlaybackSettled(500))
      .then(() => getProperty('time-pos', 'double'))
      .then((pos) => {
        if (typeof pos === 'number' && osdSeq() === shown) showSeekOsd(pos);
      })
      .catch(() => {});
  }

  /// Jump to a percentage of the file (digit keys, Home/End). Same seek-mode
  /// contract as the arrows: exact where it is free, keyframes on a file where
  /// an exact seek costs seconds.
  function seekPercent(percent: number) {
    if (!hasFile || player.duration <= 0) return;
    if (stepMode) flushStepThenCancel();
    const target = (player.duration * Math.min(100, Math.max(0, percent))) / 100;
    const exact = wantExact(true);
    const seek = issueSeek(target, exact);
    showSeekOsd(target);
    if (!exact) refineSeekOsd(seek);
  }

  // ---- Horizontal-gesture scrub -------------------------------------------
  // The rate is not set by a timer: the next seek only goes out after the
  // previous one finished (scrubInFlight). A fixed interval on an expensive
  // file built a queue of seeks, and the gesture kept "playing out" after the
  // fingers had stopped.
  //
  // The seek mode is shared with the seekbar (wantExact/issueSeek). On a slow
  // file previews go by keyframes: exact ones cost seconds there and turn the
  // gesture into 0.8 frames per second. On a fast file there is no difference
  // in cost, so everything is exact.
  let scrubbing = false;
  let scrubTarget: number | null = null;
  /// We muted the sound for the gesture — restore it afterwards.
  let scrubUnmute = false;
  let scrubInFlight = false;
  let scrubPendingSeek = false;
  /// Target of the last seek sent, so the same one is not sent twice.
  let scrubSeekedTo: number | null = null;

  let scrubEndTimer: ReturnType<typeof setTimeout> | undefined;
  // Fingers on the trackpad (from NSEvent.phase in Rust, see macos_chrome.rs).
  // While true the gesture cannot end by timeout: stopping the fingers mid-seek
  // must not resume playback.
  let wheelFingersDown = false;

  function scheduleScrubEnd() {
    clearTimeout(scrubEndTimer);
    if (wheelFingersDown) return;
    // The timeout is needed after lifting the fingers too: inertial scrolling
    // follows, and cutting the seek off halfway through it would be wrong.
    scrubEndTimer = setTimeout(endScrub, 180);
  }

  function scrubBy(deltaSeconds: number) {
    if (!hasFile || player.duration <= 0) return;
    const base = scrubTarget ?? player.timePos;
    const range = scrubRange();
    scrubTarget = Math.min(range.max, Math.max(range.min, base + deltaSeconds));

    if (!scrubbing) {
      scrubbing = true;
      // Playback is NOT stopped. Pausing for the gesture was the cause of the
      // "slideshow": the picture froze between seek landings, and on a file
      // with sparse keyframes the gesture came down to a dozen static frames.
      // With the video playing, frames between seeks keep coming and the motion
      // reads as continuous — which is apparently the difference from IINA,
      // which seeks by keyframes on such a file in exactly the same way.
      //
      // The sound is muted for the gesture: 10 seeks per second machine-gun it.
      // A fresh value, not the mirror: mute may have drifted after an mpv event
      // queue overflow.
      void getProperty('mute', 'flag')
        .then((m) => {
          if (!m && scrubbing) {
            scrubUnmute = true;
            void setProperty('mute', true);
          }
        })
        .catch(() => {});
    }

    showSeekOsd(scrubTarget);
    // The gesture drives the knob, not the lagging time-pos (see the 'time-pos'
    // handler).
    seekValue = scrubTarget;
    pumpScrubSeek();
    scheduleScrubEnd();
  }

  /// Keeps exactly one seek in flight, always aimed at the freshest target.
  function pumpScrubSeek() {
    if (scrubInFlight) {
      scrubPendingSeek = true;
      return;
    }
    if (scrubTarget === null || scrubTarget === scrubSeekedTo) {
      // Nothing to catch up with. If the gesture is over, the target is no
      // longer needed: the next one starts from the actual position.
      if (!scrubbing) scrubTarget = null;
      return;
    }
    scrubInFlight = true;
    scrubSeekedTo = scrubTarget;
    // While the gesture runs these are previews (keyframes on a slow file); the
    // last seek after it ends must be exact, since that is the frame that stays.
    void issueSeek(scrubTarget, wantExact(scrubbing))
      .finally(() => {
        scrubInFlight = false;
        scrubPendingSeek = false;
        // The target may have moved while the seek was in flight — and this is
        // also what finishes the last position if the gesture ended mid-seek.
        pumpScrubSeek();
      });
  }

  function endScrub() {
    if (!scrubbing) return;
    // The frame blink from architecture.md came from MIXING modes (keyframe
    // previews plus an exact tail); here the whole gesture is uniform, and
    // pumpScrubSeek does not replay a target already reached.
    scrubbing = false;
    pumpScrubSeek();
    scrubSeekedTo = null;
    if (scrubUnmute) {
      scrubUnmute = false;
      void setProperty('mute', false);
    }
  }

  function osdVolume(v: number) {
    showOsd(t('osd.volume', { value: v }), { progress: Math.min(1, v / 100) });
  }

  const UI_HIDE_MS = 1200;
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
  const TITLEBAR_STRIP = 48;
  const MAC_BUTTONS_WIDTH = 110;
  let pointerInTitlebar = $state(false);

  function pokeUi(e?: MouseEvent) {
    if (e) {
      // A boolean, not a coordinate: it only changes when the border is
      // crossed, otherwise reactivity would churn on every mouse move.
      const inTop =
        IS_MAC && e.clientY <= TITLEBAR_STRIP && e.clientX <= MAC_BUTTONS_WIDTH;
      if (inTop !== pointerInTitlebar) pointerInTitlebar = inTop;
    }
    uiVisible = true;
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => (uiVisible = false), UI_HIDE_MS);
  }

  // ---- Custom title bar (window without system decorations) ----

  // Windows: titlebar drag/maximize is ours, instead of data-tauri-drag-region
  // (macOS keeps the stock attribute). Tauri's built-in script calls
  // internal_toggle_maximize on double click, and tao's set_maximized does not
  // clear the fullscreen state: the window visibly leaves fullscreen, but
  // isFullscreen() keeps answering true — the mirror and the fullscreen button
  // get stuck. Our handler exits fullscreen honestly via exitFullscreen (and
  // does not let a fullscreen window be dragged by the titlebar).
  function onTitlebarMouseDown(e: MouseEvent) {
    if (IS_MAC || e.button !== 0 || e.target !== e.currentTarget) return;
    if (e.detail !== 1 && e.detail !== 2) return;
    e.preventDefault();
    if (e.detail === 2) {
      if (fullscreen) void exitFullscreen();
      else void getCurrentWindow().toggleMaximize();
    } else if (!fullscreen) {
      void getCurrentWindow().startDragging();
    }
  }

  function minimizeWindow() {
    void getCurrentWindow().minimize();
  }

  function closeWindow() {
    // destroy(), not close(): close() raises CloseRequested, where the libmpv
    // plugin tears mpv down while the window is still on screen (see the
    // onCloseRequested note in onMount). Destroying directly keeps the last
    // video frame alive into the DWM close animation.
    flushPosition();
    void getCurrentWindow().destroy();
  }

  function startResize(
    dir: 'East' | 'North' | 'NorthEast' | 'NorthWest' | 'South' | 'SouthEast' | 'SouthWest' | 'West',
  ) {
    void getCurrentWindow().startResizeDragging(dir);
  }

  // IMPORTANT: the 'node' format in libmpv-wrapper 0.1.1 is broken (it writes
  // an mpv_node into a pointer variable -> stack corruption ->
  // STATUS_ACCESS_VIOLATION), so track-list and chapter-list are only read
  // through sub-properties of simple formats (see readList in player.svelte.ts).
  function toggleMenu(kind: 'audio' | 'sub' | 'chapter' | 'queue' | 'cast') {
    openMenu = openMenu === kind ? null : kind;
    if (openMenu === 'chapter') void openChapterMenu();
    else if (openMenu === 'queue') void openQueueMenu();
    else if (openMenu === 'cast') {
      // Discovery is handled by the $effect below — it must also stop when
      // the panel closes by any route (Escape, outside click), not only here.
    } else if (openMenu) void loadTracks();
  }

  /// Discovery runs exactly while the picker is open. mDNS browse is
  /// on-demand, like the torrent session and the yt-dlp probe: nothing opens
  /// sockets at startup, and the effect's cleanup covers every way the panel
  /// closes (Escape, outside click, casting starting).
  let castSearchLong = $state(false);
  $effect(() => {
    if (openMenu === 'cast' && !cast.active) {
      startCastDiscovery();
      castSearchLong = false;
      const timer = setTimeout(() => (castSearchLong = true), 6000);
      return () => {
        clearTimeout(timer);
        stopCastDiscovery();
      };
    }
  });

  const castStateLabel = $derived(
    cast.state === 'connecting' ? t('cast.state_connecting')
    : cast.state === 'preparing' ? t('cast.state_preparing')
    : cast.state === 'loading' || cast.state === 'buffering' ? t('cast.state_loading')
    : cast.state === 'paused' ? t('cast.state_paused')
    : t('cast.state_playing'),
  );

  /// The seekbar follows the TV while casting. Guarded on `dragging` so the
  /// gesture owns the knob, exactly like the time-pos observer's guard — and
  /// safe as an $effect (which the observer guard is not) because on a drag
  /// release `cast.time` is already the optimistic release position, so the
  /// re-run cannot yank the knob backwards. `remote`, not `active`: while the
  /// prepare rung runs, local playback continues and time-pos owns the knob —
  /// two writers on one seekbar is the jitter bug in a new costume.
  $effect(() => {
    const time = cast.time;
    if (cast.remote && !dragging) seekValue = time;
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
  // The title is centred in the WINDOW, not in what is left between the side
  // clusters, so both sides have to be reserved equally or it runs into the
  // narrower one first. On macOS that is the logo and the app name (the traffic
  // lights are the system's and live outside this DOM); on Windows it is the
  // brand on one side and the window buttons on the other.
  //
  // Measured rather than assumed, because no constant is right for long: the
  // right cluster grows by a whole button when an update is waiting, and the
  // app name is localised. Reserving 46vw and hoping left about 12px of gap at
  // a 480px window — which is what "the title is touching the logo" was.

  /// Breathing space between the title and whichever cluster it reaches first.
  /// Added to the measurement rather than to the CSS, so the number lives in
  /// one language only.
  const BAR_TITLE_GAP = 16;

  let brandEl = $state<HTMLElement | null>(null);
  let chromeEl = $state<HTMLElement | null>(null);
  let barSide = $state(0);

  $effect(() => {
    const sides = [brandEl, chromeEl].filter((el): el is HTMLElement => !!el);
    if (!sides.length) return;
    const measure = () => {
      barSide = Math.max(...sides.map((el) => el.getBoundingClientRect().width)) + BAR_TITLE_GAP;
    };
    measure();
    const observer = new ResizeObserver(measure);
    for (const el of sides) observer.observe(el);
    return () => observer.disconnect();
  });

  // ---- Reordering the queue -----------------------------------------------
  //
  // Dragged with pointer events rather than HTML5 drag-and-drop: the window is
  // already a drop target for files, handled natively by Tauri, and a DOM drag
  // over the same surface is not a fight worth having for a list of a dozen
  // rows.
  //
  // The gesture has to share the row with a click that plays the entry, so
  // nothing happens until the pointer has actually travelled `DRAG_SLOP`.

  /// How far the pointer must move before a press becomes a drag. Below this a
  /// press is a click, and clicking a row is how you play it.
  const DRAG_SLOP = 4;

  let dragFrom = $state<number | null>(null);
  /// Where it would land if released now. Same number as `dragFrom` until the
  /// pointer crosses a neighbouring row.
  let dragTo = $state<number | null>(null);
  let dragStartY = 0;
  let dragRowHeight = 0;
  /// Read from the markup, so it has to be state — the row's lifted styling
  /// and the shift of its neighbours both key off it.
  let dragArmed = $state(false);

  function onQueueDown(event: PointerEvent, index: number) {
    // The × is a control of its own; a press there is never a drag.
    if ((event.target as HTMLElement).closest('.queue-remove')) return;
    const row = (event.currentTarget as HTMLElement).closest('.queue-row');
    if (!row) return;
    dragStartY = event.clientY;
    dragRowHeight = row.getBoundingClientRect().height || 1;
    dragFrom = index;
    dragTo = index;
    dragArmed = false;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }

  function onQueueMove(event: PointerEvent) {
    if (dragFrom === null) return;
    const travelled = event.clientY - dragStartY;
    if (!dragArmed && Math.abs(travelled) < DRAG_SLOP) return;
    dragArmed = true;
    const steps = Math.round(travelled / dragRowHeight);
    const last = playlist.entries.length - 1;
    dragTo = Math.min(last, Math.max(0, dragFrom + steps));
  }

  async function onQueueUp(event: PointerEvent) {
    const from = dragFrom;
    const to = dragTo;
    const wasDrag = dragArmed;
    dragFrom = null;
    dragTo = null;
    dragArmed = false;
    try {
      (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
    } catch {
      // The capture is gone already — nothing to release.
    }
    // A press that never travelled is a click, and the click handler on the row
    // does the rest.
    if (!wasDrag || from === null || to === null || from === to) return;
    // mpv moves the entry so that it *takes the place of* index2, computed on
    // the list as it stands. Moving down therefore needs one more: in
    // [A,B,C,D], putting A at index 2 means `playlist-move 0 3` — with 2 it
    // would land before C, at index 1.
    const target = to > from ? to + 1 : to;
    await command('playlist-move', [String(from), String(target)]).catch((e) => {
      console.warn('playlist-move failed:', e);
    });
    await loadPlaylist();
  }

  /// How far a row is displaced while a drag is in flight. The dragged row
  /// follows the pointer in whole rows; the ones it passes step aside by one.
  function queueRowShift(index: number): number {
    if (dragFrom === null || dragTo === null || !dragArmed) return 0;
    if (index === dragFrom) return (dragTo - dragFrom) * dragRowHeight;
    if (dragFrom < dragTo && index > dragFrom && index <= dragTo) return -dragRowHeight;
    if (dragFrom > dragTo && index < dragFrom && index >= dragTo) return dragRowHeight;
    return 0;
  }

  /// Same reasoning as the chapter list: a season has dozens of entries and the
  /// one playing is the only row the viewer has a reference point for.
  async function openQueueMenu() {
    // Awaited, unlike the fire-and-forget it used to be: the title read that
    // follows works off the entries this produces, and firing it against the
    // previous list would leave the panel on the 1.2 s timer — which is the
    // whole thing this call exists to skip.
    await loadPlaylist();
    ensureQueueTitles();
    await tick();
    queueMenuEl?.querySelector('.menu-item.sel')?.scrollIntoView({ block: 'nearest' });
  }

  /// A film has dozens of chapters and the list opens scrolled to the top, so
  /// the one being played is usually out of sight — which is the only entry the
  /// viewer has a reference point for. `nearest` so an item already visible
  /// does not scroll anything at all.
  async function openChapterMenu() {
    void loadChapters();
    await tick();
    chapterMenuEl?.querySelector('.menu-item.sel')?.scrollIntoView({ block: 'nearest' });
  }

  // Dragging the video area: without zoom it moves the window (5 px threshold,
  // so a click stays a click); zoomed in, it pans the magnified region.
  function onVideoPointerDown(e: PointerEvent) {
    if (e.button !== 0 || !hasFile) return;
    if (e.target !== e.currentTarget) return;
    if (isZoomed()) {
      panning = true;
      panMoved = false;
      panLast = { x: e.clientX, y: e.clientY };
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
      return;
    }
    if (fullscreen || isMaximized) return;
    videoDragStart = { x: e.clientX, y: e.clientY };
  }

  function onVideoPointerMove(e: PointerEvent) {
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

  function onVideoPointerUp(e: PointerEvent) {
    if (panning) {
      panning = false;
      panLast = null;
      (e.currentTarget as HTMLElement).releasePointerCapture?.(e.pointerId);
      // panning must not toggle pause with a trailing click
      if (panMoved) suppressNextClick = true;
    }
    videoDragStart = null;
  }

  function onVideoClick(e: MouseEvent) {
    if (suppressNextClick) {
      suppressNextClick = false;
      return;
    }
    if (ctxMenu) {
      ctxMenu = null;
      return;
    }
    if (openMenu) {
      openMenu = null;
      return;
    }
    if (e.target !== e.currentTarget) return;
    // Pause fires immediately, without waiting to see whether this is a double
    // click. Waiting for anything is not an option: the macOS double-click
    // threshold defaults to 500 ms, and any delay within it is exactly that
    // much delay on the player's most frequent action. The double click still
    // works: the browser sends click, click, dblclick, so the second click
    // cancels the first one's pause (togglePause goes through `cycle`, i.e. it
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
    // "resume" costs more than an artefact on a double click. The instant path
    // without this ambiguity is Space and K. Do not "fix" without discussing.
    void togglePause();
  }

  /// True when the event started inside something editable, where the system's
  /// own menu (Cut/Copy/Paste) is the one that belongs and ours has nothing to
  /// offer. Suppressing it there is how the link field ended up with no way to
  /// paste at all.
  function inTextField(e: Event): boolean {
    const target = e.target as HTMLElement | null;
    return !!target?.closest('input, textarea, [contenteditable="true"]');
  }

  /// Dialog backdrops swallow the context menu so the player's does not open
  /// behind them — but not over a text field, for the reason above.
  function blockContextMenu(e: MouseEvent) {
    if (inTextField(e)) return;
    e.preventDefault();
    e.stopPropagation();
  }

  async function onContextMenu(e: MouseEvent) {
    if (inTextField(e)) return;
    e.preventDefault();
    pokeUi();
    // The submenu always opens collapsed: it can only live inside
    // {#if ctxMenu}, so resetting the flag here is enough and it does not have
    // to be done everywhere the menu closes. closeSubmenuNow rather than the
    // bare flag: otherwise a pending close timer from the previous menu
    // collapses the one just opened.
    closeSubmenuNow();
    ctxDrill = window.innerWidth < SIDE_BY_SIDE_MIN;
    ctxMenu = { x: e.clientX, y: e.clientY, maxH: window.innerHeight };
    // Placed from the menu's actual size (eyeballed estimates go stale as items
    // are added): render → measure → correct, all before paint. The height cap
    // is the part a clamp alone cannot do — in the mini window the menu is
    // taller than the whole player, so no position shows all of it and it has
    // to scroll instead.
    await tick();
    if (ctxEl) {
      const v = flipAxis({
        near: e.clientY,
        far: e.clientY,
        size: ctxEl.offsetHeight,
        limit: window.innerHeight,
        preferBefore: false,
      });
      ctxMenu = {
        x: shiftAxis(e.clientX, ctxEl.offsetWidth, window.innerWidth),
        y: v.pos,
        maxH: v.room,
      };
    }
  }

  function onVideoDblClick() {
    // The pause was already undone by the second click — fullscreen only here.
    void toggleFullscreen();
  }

  // ---- Key bindings editor -------------------------------------------------

  /// The action whose next keystroke is being captured, or null.
  let recordingAction = $state<ActionId | null>(null);
  /// One line under one row: what a rebind took away, or why a chord was
  /// refused. Cleared by the next attempt, so it reads as a reply rather than
  /// as a status the panel keeps.
  let keyNote = $state<{ id: ActionId; text: string } | null>(null);

  const RESERVED_NOTE = {
    digits: 'keys.taken_digits',
    contextual: 'keys.taken_contextual',
    menu: 'keys.taken_menu',
  } as const;

  /// Capture phase, on the window: the player's own handler is a *bubble*
  /// listener on the same window, and stopping propagation on the way down
  /// means the keystroke never reaches it. Without that, recording a binding
  /// for "полный экран" would go fullscreen while recording it. `preventDefault`
  /// is the other half — Space would otherwise re-click the button that started
  /// the recording, and Tab would move focus out of the row.
  function recordChord(e: KeyboardEvent) {
    e.preventDefault();
    e.stopPropagation();
    // A modifier on its own is not a chord: the viewer is still reaching for
    // the key, so keep waiting rather than binding ⇧ to anything.
    if (isModifierCode(e.code)) return;
    const id = recordingAction;
    if (!id) return;
    const chord = chordOf(e);
    // Bare Escape cancels. It is reserved anyway, so without this the panel
    // would answer an attempt to back out by explaining why Escape is taken.
    if (chord === 'Escape') {
      stopRecording();
      return;
    }
    const reason = reservedReason(chord);
    if (reason) {
      keyNote = { id, text: t(RESERVED_NOTE[reason]) };
      stopRecording();
      return;
    }
    const stolen = assign(id, chord);
    keyNote = stolen ? { id, text: t('keys.stolen', { action: actionLabel(stolen) }) } : null;
    stopRecording();
  }

  function startRecording(id: ActionId) {
    keyNote = null;
    recordingAction = id;
    window.addEventListener('keydown', recordChord, true);
  }

  function stopRecording() {
    recordingAction = null;
    window.removeEventListener('keydown', recordChord, true);
  }

  $effect(() => {
    // Recording must not outlive the panel that started it: a capture listener
    // left on the window would swallow the next key pressed at the video, which
    // reads as the player having frozen rather than as a dialog left open.
    void settingsTab;
    void settingsOpen;
    return stopRecording;
  });

  /// Same rule for the language search, and for the same reason: a panel left
  /// open belongs to the tab that opened it. Reopening the dialog onto a
  /// half-typed query would also be a state nobody asked for.
  $effect(() => {
    void settingsTab;
    void settingsOpen;
    return () => {
      langPickerFor = null;
      langQuery = '';
    };
  });

  /// A label with its current hotkey after it, for tooltips. An unbound action
  /// yields the bare label: "Полный экран ()" is worse than a tooltip that
  /// simply says what the button does, and every action here can be unbound now.
  function withKey(label: string, id: ActionId): string {
    const key = hint(id);
    return key ? t('osc.with_key', { label, key }) : label;
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
      case 'frame_prev':
      case 'frame_next':
      case 'speed_down':
      case 'speed_up':
      case 'loop':
      case 'ab_loop':
      case 'ab_clear':
      case 'playlist_prev':
      case 'playlist_next':
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

  function runAction(id: ActionId) {
    // `remote`, not `active`: while the prepare rung runs, local playback is
    // still what the viewer sees, and the keys must keep driving it.
    if (cast.remote && runCastAction(id)) return;
    switch (id) {
      case 'pause': void togglePause(); break;
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
      case 'mini': if (hasFile || mini.on) void toggleMini(); break;
      case 'info': toggleInfo(); break;
      case 'reset_zoom': resetZoom(); break;
      case 'open_file': void openFileDialog(); break;
      case 'open_link': void openLinkDialog(); break;
      case 'screenshot': void saveScreenshot(false); break;
      case 'screenshot_subs': void saveScreenshot(true); break;
      case 'copy_frame': void copyScreenshot(); break;
    }
  }

  /// Escape unwinds one layer of whatever is open, innermost first, and only
  /// leaves fullscreen once nothing else is left to close. Not a bindable
  /// action: it is about the dialog stack rather than about playback, which is
  /// also why `reservedReason()` refuses to hand the key to anything else.
  function closeTopmost() {
    if (linkOpen) {
      linkOpen = false;
      return;
    }
    if (infoOpen) {
      infoOpen = false;
      return;
    }
    if (settingsOpen) {
      settingsOpen = false;
      return;
    }
    if (ctxMenu) {
      ctxMenu = null;
      return;
    }
    if (openMenu) {
      openMenu = null;
      return;
    }
    void exitFullscreen();
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
    if (skipHint) {
      e.preventDefault();
      takeSkip();
    } else if (ended && nextEntry) {
      e.preventDefault();
      cancelAdvance();
      void playEntry(nextEntry);
    }
  }

  function onKeydown(e: KeyboardEvent) {
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
    if (e.code === 'Escape') closeTopmost();
    pokeUi();
  }

</script>

<main
  class="player"
  class:mini={mini.on}
  class:idle-ui={idle}
  class:nocursor={cursorHidden}
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
    <div class="overlay start scrollable">
      <!-- The padding lives on the inner wrapper, not on the scroll container:
           browsers do not include a scrollable block's padding-bottom in the
           scrollable area, so at the height where the content is flush but no
           scrollbar has appeared yet, the bottom gap vanished and the card text
           ran into the window edge. -->
      <div class="start-inner">
      <div class="panel">
        <h1>Frame Player</h1>
        <p>{t('start.hint')}</p>
        <div class="start-actions">
          <button class="primary" onclick={openFileDialog}>{withKey(t('start.open'), 'open_file')}</button>
          <!-- Secondary on purpose: a local file is what the player is for, and
               a link is the other way in rather than an equal one. -->
          <button class="btn-outline" onclick={openLinkDialog}>
            <!-- The chain drawn DIAGONALLY, which is not decoration: the flat
                 horizontal version occupies 10 of the 24 units vertically, so
                 at any box small enough to sit beside a 15px label it renders
                 about 8px tall against the label's ~10.7px cap height and reads
                 as undersized. The diagonal one nearly fills its box, so 14px
                 of box gives ~11px of glyph — level with the words. -->
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
                <path d="M10 13.5a4.6 4.6 0 0 0 6.9.5l2.7-2.7a4.6 4.6 0 0 0-6.5-6.5L11.5 6.4"/>
                <path d="M14 10.5a4.6 4.6 0 0 0-6.9-.5l-2.7 2.7a4.6 4.6 0 0 0 6.5 6.5l1.6-1.6"/>
              </g>
            </svg>
            {t('start.link')}
          </button>
        </div>
      </div>
      {#if history.recent.length}
        <div class="recent">
          <div class="recent-head">{t('start.recent')}</div>
          <div class="recent-railwrap">
          <div class="recent-rail" bind:this={railEl} onscroll={updateRailEnds}>
            {#each history.recent as item (item.path)}
              <div class="card">
                <button
                  class="card-open"
                  aria-label={item.name}
                  onclick={() => void openRecent(item)}
                >
                  <span class="card-poster" class:empty={!item.poster}>
                    {#if item.poster}
                      <img src={item.poster} alt="" />
                    {:else if isNetworkSource(item.path)}
                      <!-- No frame is coming: a poster is decoded from a file
                           and this entry is a URL. The end-of-file cards learned
                           the same thing — an empty outlined rectangle reads as
                           a preview that failed, so the card says what it is. -->
                      <span class="card-link">
                        <svg viewBox="0 0 24 24" aria-hidden="true">
                          <g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
                            <path d="M10 13.5a4.6 4.6 0 0 0 6.9.5l2.7-2.7a4.6 4.6 0 0 0-6.5-6.5L11.5 6.4"/>
                            <path d="M14 10.5a4.6 4.6 0 0 0-6.9-.5l-2.7 2.7a4.6 4.6 0 0 0 6.5 6.5l1.6-1.6"/>
                          </g>
                        </svg>
                      </span>
                    {/if}
                    <span
                      class="card-progress"
                      style="width: {item.dur > 0
                        ? Math.min(100, (item.pos / item.dur) * 100)
                        : 0}%"
                    ></span>
                  </span>
                  <!-- The tip is on the caption, not on the whole card. It
                       names what the card already shows in short — the full
                       path, or the torrent this episode came out of — so it is
                       an answer to "which one is this", asked by reading the
                       truncated name. Hung on the poster it was answering a
                       question nobody had, and covering the frame to do it. -->
                  <span class="card-meta" data-tip={recentTip(item)} data-tip-below>
                    <span class="card-name">{item.name}</span>
                    <span class="card-left">
                      {item.dur > 0
                        ? t('start.remaining', { time: formatTime(item.dur - item.pos) })
                        : formatTime(item.pos)}
                    </span>
                  </span>
                </button>
                <button
                  class="card-forget"
                  data-tip={t('start.forget')}
                  aria-label={t('start.forget')}
                  onclick={() => forgetRecent(item.path)}
                >
                  <!-- An SVG rather than the "×" glyph: the multiplication sign
                       has no guaranteed vertical centre in the em square, it
                       sits above the middle of the line, and no place-items
                       fixes that — what gets centred is the line, not the sign. -->
                  <svg viewBox="0 0 10 10" aria-hidden="true">
                    <path
                      stroke="currentColor"
                      stroke-width="1.4"
                      stroke-linecap="round"
                      d="M1.2 1.2l7.6 7.6M8.8 1.2l-7.6 7.6"
                    />
                  </svg>
                </button>
              </div>
            {/each}
          </div>
          <!-- Shown only where there is something to reach. Arrows rather than a
               scrollbar because this is a row of targets, not a document — and
               because a trackpad already swipes it (the start overlay is in
               WHEEL_SURFACES, so `onWheel` hands wheel events straight to the
               browser and horizontal ones scroll this natively). -->
          <button
            class="rail-arrow prev"
            class:on={!railAtStart}
            aria-label={t('start.rail_prev')}
            tabindex={railAtStart ? -1 : 0}
            onclick={() => scrollRail(-1)}
          >
            <span>
              <svg viewBox="0 0 16 16" aria-hidden="true">
                <path d="M10 3.5 6 8 10 12.5" fill="none" stroke="currentColor"
                  stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
              </svg>
            </span>
          </button>
          <button
            class="rail-arrow next"
            class:on={!railAtEnd}
            aria-label={t('start.rail_next')}
            tabindex={railAtEnd ? -1 : 0}
            onclick={() => scrollRail(1)}
          >
            <span>
              <svg viewBox="0 0 16 16" aria-hidden="true">
                <path d="M6 3.5 10 8 6 12.5" fill="none" stroke="currentColor"
                  stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
              </svg>
            </span>
          </button>
          </div>
        </div>
      {/if}

      <!-- Torrents, one row per RAW TORRENT rather than per file — which is the
           whole reason this is not a duplicate of the grid above. That grid
           holds individual episodes with positions; a torrent is the container,
           and its episodes that were never started have no representation
           anywhere else. So this is how you get back into a season without the
           magnet, and the disk figure is secondary to that rather than the point
           of it: leading with a size would turn the start screen into a
           download manager, which is precisely what this player is not. -->
      {#if torrentRows.length}
        <div class="recent torrents">
          <div class="recent-head">
            <span>{t('start.torrents')}</span>
            <span class="torrents-total">{fmtSize(torrentTotal)}</span>
          </div>
          <div class="torrent-list">
            {#each torrentRows as row (row.folder)}
              {@const resume = torrentResume(row)}
              {@const opening = torrentOpening === row.folder}
              <div class="torrow" class:busy={torrentBusy === row.folder}>
                <button
                  class="torrow-open"
                  disabled={!row.known || opening}
                  data-tip={row.known
                    ? resume
                      ? t('start.torrent_continue_tip', { name: resume.name })
                      : t('start.torrent_open')
                    : t('start.torrent_unknown')}
                  onclick={() => void openRememberedTorrent(row)}
                >
                  <span class="torrow-name">{row.name ?? t('start.torrent_unnamed')}</span>
                  <span class="torrow-meta">
                    <!-- Resolving a magnet is a DHT lookup — a second at best,
                         and the row used to sit there looking untouched for all
                         of it. It answers the click immediately instead. -->
                    {#if opening}
                      <span class="torrow-working">
                        <span class="torrow-spin"></span>
                        {t('torrent.resolving')}
                      </span>
                    {:else}
                      {#if row.known}
                        <span>{t('start.torrent_files', { count: row.known.videos })}</span>
                      {/if}
                      <span>{fmtSize(row.size)}</span>
                      {#if resume}
                        <span class="torrow-resume">
                          {t('start.torrent_resume', {
                            name: resume.name,
                            time: formatTime(Math.max(0, resume.dur - resume.pos)),
                          })}
                        </span>
                      {/if}
                    {/if}
                  </span>
                </button>
                <!-- Grouped, so `.torrow`'s 11px gap falls once — between the
                     text and the actions — rather than between every pair and
                     undoing the centring it was computed for. -->
                <div class="torrow-actions">
                {#if row.known}
                  {@const known = row.known}
                  <button
                    class="card-forget torrow-forget torrow-update"
                    data-tip={t('torrent.update_tip')}
                    aria-label={t('torrent.update')}
                    onclick={() => void openUpdateDialog(known)}
                  >
                    <!-- The same plus as "add an excluded folder", deliberately
                         one glyph rather than two near-identical ones. A ring was
                         here first and was the only curve in a 16px set of
                         chevrons, crosses and strokes — next to the delete cross
                         it read as borrowed from elsewhere. This is built exactly
                         as that cross is (two straight strokes, round caps, same
                         weight), which also makes the pair legible as what it is:
                         one adds episodes, the other removes the lot. A download
                         arrow was the other candidate and lost on meaning — that
                         glyph already says "fetching right now" in `.torchip`. -->
                    <svg viewBox="0 0 16 16" aria-hidden="true">
                      <path
                        d="M8 3.5v9M3.5 8h9"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="1.5"
                        stroke-linecap="round"
                      />
                    </svg>
                  </button>
                {/if}
                <button
                  class="card-forget torrow-forget"
                  disabled={torrentIsPlaying(row)}
                  data-tip={torrentIsPlaying(row)
                    ? t('start.torrent_playing')
                    : t('start.torrent_delete')}
                  aria-label={t('start.torrent_delete')}
                  onclick={() => void deleteTorrent(row)}
                >
                  <svg viewBox="0 0 10 10" aria-hidden="true">
                    <path
                      stroke="currentColor"
                      stroke-width="1.4"
                      stroke-linecap="round"
                      d="M1.2 1.2l7.6 7.6M8.8 1.2l-7.6 7.6"
                    />
                  </svg>
                </button>
                </div>
              </div>
            {/each}
          </div>
        </div>
      {/if}
      </div>
    </div>
  {:else if ended}
    <!-- keep-open=always parks mpv on the last frame of EVERY entry, not only
         the last one, so this is the end of an episode as much as the end of a
         film: previous / replay / next, with the countdown on next. Clicking
         the backdrop cancels the countdown — it is the largest possible target
         for "wait". -->
    <div class="overlay clickthrough-bg endscreen" role="presentation" onclick={cancelAdvance}>
      <div class="endrow">
        {#if prevEntry}
          {@render endCard(prevEntry, 'prev')}
        {/if}
        <button class="replay" onclick={togglePause} aria-label={t('osc.replay')}>
          <svg viewBox="0 0 24 24" width="48" height="48"><path fill="currentColor" d="M12 5V1L7 6l5 5V7a6 6 0 1 1-6 6H4a8 8 0 1 0 8-8z"/></svg>
        </button>
        {#if nextEntry}
          {@render endCard(nextEntry, 'next')}
        {/if}
      </div>
      {#if advancing}
        <button class="endcancel" onclick={cancelAdvance}>{t('end.cancel')}</button>
      {/if}
    </div>
  {/if}

  {#snippet endCard(entry: PlaylistEntry, side: 'prev' | 'next')}
    <button
      class="endcard"
      class:prev={side === 'prev'}
      onclick={(e) => {
        e.stopPropagation();
        cancelAdvance();
        void playEntry(entry);
      }}
    >
      <span class="card-poster" class:empty={!playlist.posters[entry.path]}>
        {#if playlist.posters[entry.path]}
          <img src={playlist.posters[entry.path]} alt="" />
        {:else if isNetworkSource(entry.path)}
          <!-- No frame is coming: a poster is decoded from the file, and this
               entry is a URL. An empty outlined rectangle reads as a preview
               that failed, so the card says what it is instead. -->
          <span class="card-link">
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
                <path d="M10 13.5a4.6 4.6 0 0 0 6.9.5l2.7-2.7a4.6 4.6 0 0 0-6.5-6.5L11.5 6.4"/>
                <path d="M14 10.5a4.6 4.6 0 0 0-6.9-.5l-2.7 2.7a4.6 4.6 0 0 0 6.5 6.5l1.6-1.6"/>
              </g>
            </svg>
          </span>
        {/if}
        <!-- The countdown, keyed so a restarted one gets a fresh element and
             therefore replays its animation instead of inheriting a finished
             one. Indigo like .card-progress, and for the same reason: it lies
             over an arbitrary video frame, where white disappears. -->
        {#if side === 'next' && advancing}
          {#key advanceSeq}
            <span class="endprogress" style="--advance: {ADVANCE_MS}ms"></span>
          {/key}
        {/if}
      </span>
      <span class="card-left">{t(side === 'next' ? 'end.next' : 'end.prev')}</span>
      <span class="card-name">{entry.title}</span>
    </button>
  {/snippet}

  {#if opening}
    <div class="overlay loading-overlay">
      <div class="loading-box">
        <span class="loading-spin"></span>
        <!-- The two lines are ONE flex item, not two siblings of the spinner.
             That is what makes them share a left edge structurally instead of
             by an offset someone has to keep correct. -->
        <span class="loading-text">
          <span class="loading-title">{openingLabel}</span>
          <!-- Why it is waiting, when the answer is a swarm. Without it a
               torrent stall is indistinguishable from a hung player. -->
          {#if torrentLabel}
            <span class="loading-sub">{torrentLabel}</span>
          {/if}
        </span>
      </div>
    </div>
  {/if}

  <!-- The casting screen: while the TV plays, this window is a remote, and
       what it shows must never be the stale paused frame pretending to be
       playback — nor a hole to the desktop (gotcha 10), hence the opaque
       fill in its CSS rather than a bare overlay. Gated on `remote`, not
       `active`: during the prepare rung local playback deliberately keeps
       running, and covering a playing picture with a status card would read
       as the player dying for the length of the remux. -->
  {#if cast.remote}
    <div class="overlay castscreen">
      <div class="castscreen-box">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M21 3H3c-1.1 0-2 .9-2 2v3h2V5h18v14h-7v2h7c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2zM1 18v3h3c0-1.66-1.34-3-3-3zm0-4v2c2.76 0 5 2.24 5 5h2c0-3.87-3.13-7-7-7zm0-4v2c4.97 0 9 4.03 9 9h2c0-6.08-4.93-11-11-11z"/></svg>
        <span class="castscreen-title">{t('cast.casting_on', { name: cast.deviceName ?? '' })}</span>
        <span class="castscreen-sub">{displayTitle}</span>
        {#if cast.busy}
          <span class="castscreen-state">{castStateLabel}</span>
        {/if}
      </div>
    </div>
  {/if}

  <div class="stepwrap" class:visible={stepMode}>
    <canvas bind:this={stepCanvas}></canvas>
    <div class="stepbadge">{t('step.badge')} · {formatTimeMs(stepPts)}</div>
  </div>

  {#snippet brandMark()}
    <svg class="logo" viewBox="0 0 24 24"><path fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" d="M3.2 7.6V5.9c0-1.5 1.2-2.7 2.7-2.7h1.7M16.4 3.2h1.7c1.5 0 2.7 1.2 2.7 2.7v1.7M20.8 16.4v1.7c0 1.5-1.2 2.7-2.7 2.7h-1.7M7.6 20.8H5.9c-1.5 0-2.7-1.2-2.7-2.7v-1.7"/><path fill="#e8e8ec" stroke="#e8e8ec" stroke-width="1.6" stroke-linejoin="round" d="M10 9v6l5.4-3z"/></svg>
    <span class="appname">Frame Player</span>
  {/snippet}

  <div
    class="topbar"
    class:hidden={idle}
    style="--bar-side: {barSide}px"
    data-tauri-drag-region={IS_MAC || undefined}
    role="presentation"
    onmousedown={onTitlebarMouseDown}
    onmouseenter={() => (barHover = true)}
    onmouseleave={() => (barHover = false)}
    onclick={(e) => e.stopPropagation()}
    ondblclick={(e) => e.stopPropagation()}
  >
    {#if !IS_MAC}
      <div class="brand" bind:this={brandEl}>{@render brandMark()}</div>
    {/if}
    <div class="titlecenter">
      <span
        class="title"
        class:slide-out-next={titleSlide === 'out-next'}
        class:slide-out-prev={titleSlide === 'out-prev'}
        class:slide-prep-next={titleSlide === 'prep-next'}
        class:slide-prep-prev={titleSlide === 'prep-prev'}>{barTitleText}</span>
    </div>
    <div class="chrome" bind:this={chromeEl}>
    {#if updateAvail}
      <button
        class="updbtn"
        class:progressing={updatePct !== null}
        style="--pct: {updatePct ?? 0}%"
        onclick={installUpdate}
        disabled={updatePct !== null}
      >
        {updatePct === null
          ? t('bar.update', { version: updateAvail.version })
          : t('bar.downloading', { percent: updatePct })}
      </button>
    {/if}
    <!-- macOS: minimize/close/fullscreen are the system traffic lights on the
         left, and fullscreen is left with the OSC button (bottom right), Esc or
         F — there are no title-bar buttons of our own there at all. -->
    {#if !IS_MAC}
      <div class="winbtns">
        <button class="winbtn" data-tip={t('bar.minimize')} aria-label={t('bar.minimize')} onclick={minimizeWindow}>
          <svg viewBox="0 0 10 10"><path stroke="currentColor" d="M0 5h10"/></svg>
        </button>
        {#if fullscreen}
          <button class="winbtn fsx" data-tip={t('bar.fullscreen_exit_esc')} aria-label={t('bar.fullscreen_exit_esc')} onclick={() => void exitFullscreen()}>
            <svg viewBox="0 0 24 24"><path fill="currentColor" d="M5 16h3v3h2v-5H5v2zm3-8H5v2h5V5H8v3zm6 11h2v-3h3v-2h-5v5zm2-11V5h-2v5h5V8h-3z"/></svg>
          </button>
        {:else}
          <button class="winbtn fsx" data-tip={withKey(t('bar.fullscreen'), 'fullscreen')} aria-label={t('bar.fullscreen')} onclick={() => void toggleFullscreen()}>
            <svg viewBox="0 0 24 24"><path fill="currentColor" d="M7 14H5v5h5v-2H7v-3zm-2-4h2V7h3V5H5v5zm12 7h-3v2h5v-5h-2v3zM14 5v2h3v3h2V5h-5z"/></svg>
          </button>
        {/if}
        <button class="winbtn close" data-tip={t('bar.close')} aria-label={t('bar.close')} onclick={closeWindow}>
          <svg viewBox="0 0 10 10"><path stroke="currentColor" d="M0 0l10 10M10 0L0 10"/></svg>
        </button>
      </div>
    {/if}
    {#if IS_MAC}
      <div class="brand brand-right">{@render brandMark()}</div>
    {/if}
    </div>
  </div>

  <!-- Live torrent readout, and it closes a real gap rather than decorating:
       `opening` is cleared on `file-loaded`, so a torrent that runs out of
       pieces *mid-playback* used to freeze the picture with nothing on screen
       to say why. Deliberately OUTSIDE `.topbar` — like the skip button and for
       the same reason, it has to survive the UI going idle, which is exactly
       when a stall happens. It follows the chrome the rest of the time, so the
       picture stays clean while the swarm is keeping up.

       Top-right, because top-left is where `.osd` appears — the popup for every
       volume change and seek — and it would cover this outright. -->
  {#if torrentChip}
    <div
      class="torchip"
      class:hidden={idle && !player.stalled}
      class:waiting={player.stalled}
    >
      <div class="torchip-line">
        <svg class="torchip-icon" viewBox="0 0 16 16" aria-hidden="true">
          <path
            d="M8 2.5v8m0 0L4.8 7.3M8 10.5l3.2-3.2M3 13.5h10"
            fill="none"
            stroke="currentColor"
            stroke-width="1.6"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
        <!-- The same sentence the loading overlay uses, not a second copy of
             the logic: it already separates "no peers at all" from "connecting"
             from a rate, and those distinctions are worth exactly as much here.
             Stalling outranks it, because that is what the viewer is looking
             at the chip to find out. -->
        <span>{player.stalled ? t('load.stalled') : (torrentLabel ?? '')}</span>
      </div>
      {#if torrentChip.file_size > 0}
        <div class="torchip-bar">
          <span style="width: {(torrentChip.file_done / torrentChip.file_size) * 100}%"></span>
        </div>
      {/if}
    </div>
  {/if}

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
  {#if !IS_MAC && !fullscreen && !isMaximized}
    <div class="rz rz-n" role="presentation" onpointerdown={() => startResize('North')}></div>
    <div class="rz rz-s" role="presentation" onpointerdown={() => startResize('South')}></div>
    <div class="rz rz-e" role="presentation" onpointerdown={() => startResize('East')}></div>
    <div class="rz rz-w" role="presentation" onpointerdown={() => startResize('West')}></div>
    <div class="rz rz-ne" role="presentation" onpointerdown={() => startResize('NorthEast')}></div>
    <div class="rz rz-nw" role="presentation" onpointerdown={() => startResize('NorthWest')}></div>
    <div class="rz rz-se" role="presentation" onpointerdown={() => startResize('SouthEast')}></div>
    <div class="rz rz-sw" role="presentation" onpointerdown={() => startResize('SouthWest')}></div>
  {/if}

  {#if ctxMenu}
    <div
      class="ctxmenu scrollable"
      class:masked={ctxDrill && !!ctxSubmenu}
      bind:this={ctxEl}
      style="left: {ctxMenu.x}px; top: {ctxMenu.y}px; max-height: {ctxMenu.maxH}px"
      role="menu"
      tabindex="-1"
      onscroll={closeSubmenuNow}
      onclick={(e) => e.stopPropagation()}
      ondblclick={(e) => e.stopPropagation()}
      oncontextmenu={blockContextMenu}
    >
      <!-- Grouped rather than listed: anything with more than two related
           entries becomes a submenu, which takes the menu from ~19 rows to 11
           and makes it fit windows it used to have to scroll in. What stays at
           the top level is what has no group and is reached often — the two
           size modes, the settings. -->
      {@render submenuHead('file', t('ctx.file'), false)}
      {@render submenuHead('frame', t('ctx.frame'), !hasFile)}
      {@render submenuHead('playback', t('ctx.playback'), !hasFile)}
      {@render submenuHead('picture', t('ctx.picture'), !hasFile)}
      <div class="menu-sep"></div>
      <button class="menu-item" onclick={() => { ctxMenu = null; void toggleFullscreen(); }}>
        {t('ctx.fullscreen')} <span class="hint">{hint('fullscreen')}</span>
      </button>
      <button class="menu-item" class:active={mini.on} disabled={!hasFile} onclick={() => { ctxMenu = null; void toggleMini(); }}>
        {t('ctx.mini')} <span class="hint">{hint('mini')}</span>
      </button>
      {@render submenuHead('window', t('ctx.window'), false)}
      <div class="menu-sep"></div>
      <button class="menu-item" onclick={() => { ctxMenu = null; void openSettings(); }}>
        {t('ctx.settings')}
      </button>
      <!-- Windows only: ms-settings: is a Windows scheme, and on macOS this
           item silently did nothing. macOS has no single-link equivalent (the
           default app is chosen in a specific file's Get Info panel). -->
      {#if !IS_MAC}
        <button
          class="menu-item"
          onclick={() => { ctxMenu = null; void openUrl('ms-settings:defaultapps'); }}
        >
          {t('ctx.default_apps')}
        </button>
      {/if}
    </div>
    <!-- The panels are rendered OUTSIDE the menu, not inside the item that
         opens them. The menu scrolls when it is taller than the window, and a
         scroll container is a clip: a panel inside it can only be as wide as
         what is left of the menu. Placement is in viewport coordinates either
         way (placeSubmenu), so nothing but the clip depended on the nesting —
         and the hover bridge is geometric, so it does not either. -->
    {@render submenuPanel('file', fileItems)}
    {@render submenuPanel('frame', frameItems)}
    {@render submenuPanel('playback', playbackItems)}
    {@render submenuPanel('picture', pictureItems)}
    {@render submenuPanel('window', windowItems)}
  {/if}
    {#if linkOpen}
      <div
        class="settings-backdrop"
        role="presentation"
        onclick={() => (linkOpen = false)}
        ondblclick={(e) => e.stopPropagation()}
        oncontextmenu={blockContextMenu}
      >
        <div class="settings link-dialog" role="dialog" aria-label={t('link.title')} tabindex="-1" onclick={(e) => e.stopPropagation()}>
          <div class="settings-head">
            <span>{t('link.title')}</span>
            <button class="settings-close" data-tip={t('set.close')} aria-label={t('bar.close')} onclick={() => (linkOpen = false)}>
              <svg viewBox="0 0 10 10"><path stroke="currentColor" d="M0 0l10 10M10 0L0 10"/></svg>
            </button>
          </div>
          <input
            class="link-input"
            type="text"
            spellcheck="false"
            autocapitalize="off"
            autocorrect="off"
            placeholder={t('link.placeholder')}
            bind:this={linkInputEl}
            bind:value={linkValue}
            onkeydown={(e) => {
              // Enter submits and Escape closes from inside the field, where the
              // window-level handler never sees them.
              if (e.key === 'Enter') { e.preventDefault(); submitLink(); }
              if (e.key === 'Escape') { e.preventDefault(); linkOpen = false; }
              e.stopPropagation();
            }}
          />
          <!-- Resolving a magnet is a DHT lookup, routinely ten seconds and
               occasionally a minute, so the dialog stays up and says what it is
               waiting for rather than closing on a black window. -->
          {#if torrent.resolving}
            <div class="link-progress">
              <span class="loading-spin"></span>
              <span>{t('torrent.resolving')}</span>
            </div>
            <div class="setting-hint">{t('torrent.resolving_hint')}</div>
          {:else if torrentError}
            <div class="link-error">{torrentError}</div>
          {:else if linkFailed}
            <div class="link-error">
              {t(ytdlp.present ? 'link.failed_stale' : 'link.failed_missing')}
            </div>
          {:else}
            <div class="setting-hint">{t(ytdlp.present ? 'link.hint_ytdlp' : 'link.hint_plain')}</div>
          {/if}
          {#if links.length && !linkValue.trim()}
            <div class="link-recent">
              {#each links as url (url)}
                <!-- Two controls, so the row is a flex container: opening the
                     link is the row, forgetting it is the cross. Every ancestor
                     between the ellipsised name and the dialog's fixed width
                     needs `min-width: 0`, or the row grows to the name instead. -->
                <div class="link-recent-row">
                  <button
                    class="link-recent-item"
                    data-tip={readableLink(url)}
                    onclick={() => submitLink(url)}
                  >
                    {linkTitles[url] ?? displayName(url)}
                  </button>
                  <button
                    class="link-recent-forget"
                    data-tip={t('link.forget')}
                    aria-label={t('link.forget')}
                    onclick={() => dropLink(url)}
                  >
                    <svg viewBox="0 0 10 10" aria-hidden="true">
                      <path
                        stroke="currentColor"
                        stroke-width="1.4"
                        stroke-linecap="round"
                        d="M1.2 1.2l7.6 7.6M8.8 1.2l-7.6 7.6"
                      />
                    </svg>
                  </button>
                </div>
              {/each}
            </div>
          {/if}
          <div class="link-actions">
            <!-- The one thing this dialog could not do, and the reason torrent
                 support was invisible in it: a magnet is pasted, but a
                 `.torrent` is a *file* — there is nothing to type, so a box that
                 only takes text silently excluded half of how torrents arrive.
                 Sits at the left, away from the primary action, because it is
                 the other route rather than a variant of this one; the tooltip
                 names dropping, which is the cheaper gesture of the two and
                 which nothing on screen would otherwise reveal. -->
            <button
              class="btn-outline link-torrent"
              data-tip={t('link.torrent_file_tip')}
              disabled={torrent.resolving}
              onclick={pickTorrentFile}
            >{t('link.torrent_file')}</button>
            <!-- Offered only for the copy we installed: someone else's yt-dlp is
                 their package manager's to update, and pressing -U on it would
                 either fail or start a fight. -->
            {#if !ytdlp.present || (linkFailed && ytdlp.managed)}
              <!-- The progress is the button's own fill rather than a separate
                   bar: a 36 MB download over a slow link otherwise leaves a
                   button saying "downloading…" for a minute, which is
                   indistinguishable from one that has hung. -->
              <button
                class="btn-outline"
                class:progressing={ytdlpPct !== null}
                style="--pct: {ytdlpPct ?? 0}%"
                disabled={ytdlpBusy}
                onclick={fixYtdlp}
              >
                {ytdlpBusy
                  ? ytdlpPct !== null
                    ? t('link.ytdlp_pct', { percent: ytdlpPct })
                    : t('link.ytdlp_working')
                  : t(ytdlp.present ? 'link.ytdlp_update' : 'link.ytdlp_install')}
              </button>
            {/if}
            <button
              class="primary"
              disabled={!linkValue.trim() || torrent.resolving}
              onclick={() => submitLink()}
            >{t('link.open')}</button>
          </div>
        </div>
      </div>
    {/if}

    <!-- The dialog leads with the *reason*, not the request. "Update" over an
         input box is mysterious; "BitTorrent cannot add a file to a torrent" is
         the fact that makes the whole errand make sense, and it is why the
         player cannot do this by itself. -->
    {#if updateFor}
      {@const known = updateFor}
      <div
        class="settings-backdrop"
        role="presentation"
        onclick={() => { if (!updateBusy) updateFor = null; }}
        ondblclick={(e) => e.stopPropagation()}
        oncontextmenu={blockContextMenu}
      >
        <div
          class="settings link-dialog"
          role="dialog"
          aria-label={t('torrent.update')}
          tabindex="-1"
          onclick={(e) => e.stopPropagation()}
        >
          <div class="settings-head">
            <span>{t('torrent.update')}</span>
            <button
              class="settings-close"
              data-tip={t('set.close')}
              aria-label={t('bar.close')}
              disabled={updateBusy}
              onclick={() => (updateFor = null)}
            >
              <svg viewBox="0 0 10 10"><path stroke="currentColor" d="M0 0l10 10M10 0L0 10"/></svg>
            </button>
          </div>
          <div class="setting-hint">
            {updateSuggested
              ? t('torrent.update_suggested', { name: known.name ?? displayName(known.magnet) })
              : t('torrent.update_why')}
          </div>
          <input
            class="link-input"
            type="text"
            spellcheck="false"
            autocapitalize="off"
            autocorrect="off"
            placeholder="magnet:?xt=urn:btih:…"
            disabled={updateBusy}
            bind:this={updateInputEl}
            bind:value={updateValue}
            onkeydown={(e) => {
              if (e.key === 'Enter') { e.preventDefault(); void submitUpdate(); }
              if (e.key === 'Escape') { e.preventDefault(); updateFor = null; }
              e.stopPropagation();
            }}
          />
          {#if updateError}
            <div class="link-error">{updateError}</div>
          {:else}
            <div class="setting-hint">{t('torrent.update_keeps')}</div>
          {/if}
          <div class="link-actions">
            <!-- The way out for a link that only LOOKS like an update: opening
                 it separately has to stay one click away, or a wrong guess by
                 `findSupersededTorrent` becomes a wall. -->
            {#if updateSuggested}
              <button
                class="btn-outline"
                disabled={updateBusy}
                onclick={() => {
                  const magnet = updateValue.trim();
                  updateFor = null;
                  void openTorrent(magnet);
                }}
              >
                {t('torrent.update_as_new')}
              </button>
            {/if}
            <button
              class="primary"
              disabled={!updateValue.trim() || updateBusy}
              onclick={() => void submitUpdate()}
            >
              {updateBusy ? t('torrent.resolving') : t('torrent.update_go')}
            </button>
          </div>
        </div>
      </div>
    {/if}

    <!-- A torrent holding more than one video is a question only the viewer can
         answer. The rest still become queue entries, which is free: nothing is
         downloaded until mpv reads one (see torrent.rs). -->
    {#if torrentPick}
      <!-- Positions per file, because a season is nine names that differ by two
           characters and "which one was I on" is the actual question being asked
           of this list. Read once when the panel opens rather than per row, and
           hoisted to here because `{@const}` may only be an immediate child of a
           block. -->
      {@const watched = torrentPositions(torrentPick.info_hash)}
      <div
        class="settings-backdrop"
        role="presentation"
        onclick={() => (torrentPick = null)}
        ondblclick={(e) => e.stopPropagation()}
        oncontextmenu={blockContextMenu}
      >
        <div
          class="settings link-dialog"
          role="dialog"
          aria-label={t('torrent.pick_title')}
          tabindex="-1"
          onclick={(e) => e.stopPropagation()}
        >
          <div class="settings-head">
            <span>{torrentPick.name ?? t('torrent.pick_title')}</span>
            <button
              class="settings-close"
              data-tip={t('set.close')}
              aria-label={t('bar.close')}
              onclick={() => (torrentPick = null)}
            >
              <svg viewBox="0 0 10 10"><path stroke="currentColor" d="M0 0l10 10M10 0L0 10"/></svg>
            </button>
          </div>
          <div class="setting-hint">{t('torrent.pick_hint')}</div>
          <div class="torrent-files scrollable">
            {#each torrentVideos(torrentPick) as file (file.index)}
              {@const info = torrentPick}
              {@const seen = watched[file.index]}
              <button
                class="menu-item torrent-file"
                class:started={!!seen}
                data-tip={file.path}
                onclick={() => void playTorrentFile(info, file)}
              >
                <span class="torrent-file-name">{displayName(file.path)}</span>
                {#if seen && seen.dur > 0}
                  <span class="torrent-file-left">
                    {t('start.remaining', { time: formatTime(Math.max(0, seen.dur - seen.pos)) })}
                  </span>
                {/if}
                <span class="torrent-file-size">{fmtSize(file.size)}</span>
              </button>
            {/each}
          </div>
        </div>
      </div>
    {/if}

    {#if subsOpen}
      <div
        class="settings-backdrop"
        role="presentation"
        onclick={() => (subsOpen = false)}
        ondblclick={(e) => e.stopPropagation()}
        oncontextmenu={blockContextMenu}
      >
        <div
          class="settings subs-dialog"
          role="dialog"
          aria-label={t('subs.title')}
          tabindex="-1"
          onclick={(e) => e.stopPropagation()}
        >
          <div class="settings-head">
            <span>{t('subs.title')}</span>
            <button
              class="settings-close"
              data-tip={t('set.close')}
              aria-label={t('bar.close')}
              onclick={() => (subsOpen = false)}
            >
              <svg viewBox="0 0 10 10"><path stroke="currentColor" d="M0 0l10 10M10 0L0 10"/></svg>
            </button>
          </div>

          <div class="subs-query">
            <input
              class="link-input"
              type="text"
              spellcheck="false"
              autocapitalize="off"
              autocorrect="off"
              placeholder={t('subs.placeholder')}
              bind:this={subsInputEl}
              bind:value={subsQuery}
              onkeydown={(e) => {
                if (e.key === 'Enter') { e.preventDefault(); void runSubsSearch(); }
                if (e.key === 'Escape') { e.preventDefault(); subsOpen = false; }
                e.stopPropagation();
              }}
            />
            <button class="primary" disabled={subsBusy} onclick={() => void runSubsSearch()}>
              {subsBusy ? t('subs.searching') : t('subs.search')}
            </button>
          </div>

          <!-- Said out loud: without it, a list of episodes coming back for a
               two-word query reads as the search guessing. -->
          {#if subsEpisode}
            <div class="setting-hint">
              {t('subs.episode_from_name', {
                season: subsEpisode.season,
                episode: subsEpisode.episode,
              })}
            </div>
          {/if}

          <!-- A pill picks one of several named values, which is exactly what a
               language filter is. -->
          <div class="segmented">
            {#each SUBS_LANGS as option (option.value)}
              <button
                class="segopt"
                class:sel={subsLang === option.value}
                onclick={() => { subsLang = option.value; void runSubsSearch(); }}
              >
                {option.label}
              </button>
            {/each}
          </div>

          <!-- Which of the two searches produced this list is the single most
               load-bearing sentence in the panel: an exact match drops in
               already in sync, a title match may be a different release. The
               "hash matched nothing" case is stated on its own, because that is
               a fact about the file rather than about the search. -->
          {#if subsHashBlocked}
            <div class="setting-hint">{t('subs.hash_blocked')}</div>
          {:else if subsHits?.length && subsMatchKind === 'hash'}
            <div class="setting-hint">{t('subs.by_hash')}</div>
          {:else if subsHits?.length}
            <div class="setting-hint">
              {t(subsHashTried ? 'subs.no_exact_then_title' : 'subs.by_title')}
            </div>
          {:else if subsHits && subsHashTried && !subsBusy}
            <div class="setting-hint">{t('subs.no_exact')}</div>
          {/if}

          {#if subsError}
            <div class="link-error">{subsError}</div>
          {/if}

          {#if subsHits}
            {#if subsHits.length}
              <div class="subs-list scrollable">
                {#each subsHits as hit (hit.file_id)}
                  <div class="subs-row">
                    <div class="subs-main">
                      <div class="subs-name" data-tip={hit.release || hit.file_name}>
                        {hit.release || hit.file_name}
                      </div>
                      <div class="subs-meta">
                        <span class="subs-lang">{hit.language.toUpperCase()}</span>
                        <!-- An episode's own title is usually "Episode 3", so
                             the series and the number are what identify it. -->
                        {#if hit.parent && hit.season !== null && hit.episode !== null}
                          <span class="subs-movie">
                            {hit.parent} · S{hit.season}E{hit.episode}
                          </span>
                        {:else if hit.movie}
                          <span class="subs-movie">{hit.movie}{hit.year ? ` (${hit.year})` : ''}</span>
                        {/if}
                        {#if hit.fps !== null}
                          <span
                            class="subs-fps"
                            class:warn={subsFpsOff(hit.fps)}
                            data-tip={subsFpsOff(hit.fps) ? t('subs.fps_off') : undefined}
                          >
                            {t('info.fps', { value: hit.fps })}
                          </span>
                        {/if}
                        <span>{t('subs.downloads', { count: hit.downloads })}</span>
                      </div>
                      <div class="subs-badges">
                        {#if hit.hash_match}<span class="subs-badge match">{t('subs.badge_hash')}</span>{/if}
                        {#if hit.from_trusted}<span class="subs-badge">{t('subs.badge_trusted')}</span>{/if}
                        {#if hit.hearing_impaired}<span class="subs-badge">{t('subs.badge_hi')}</span>{/if}
                        {#if hit.ai_translated}<span class="subs-badge warn">{t('subs.badge_ai')}</span>{/if}
                      </div>
                    </div>
                    <button
                      class="btn-outline subs-sm subs-get"
                      disabled={subsBusyId !== null}
                      onclick={() => void downloadSub(hit)}
                    >
                      {subsBusyId === hit.file_id ? t('subs.downloading') : t('subs.download')}
                    </button>
                  </div>
                {/each}
              </div>
            {:else if !subsBusy}
              <div class="setting-hint">{t('subs.empty')}</div>
            {/if}
          {/if}

          {#if subsQuota && subsQuota.remaining !== null}
            <div class="setting-hint">
              {t('subs.quota', { count: subsQuota.remaining })}
              {#if subsQuota.reset}· {t('subs.quota_reset', { time: subsQuota.reset })}{/if}
            </div>
          {/if}

          <!-- The account, folded away until asked for: downloading works
               without one, and signing in only raises the daily ceiling. -->
          <div class="subs-account">
            {#if subsAccount?.signed_in}
              <div class="subs-account-line">
                <span>{t('subs.signed_in', { name: subsAccount.username ?? '' })}</span>
                {#if subsAccount.remaining_downloads !== null && subsAccount.allowed_downloads !== null}
                  <span class="subs-account-quota">
                    {t('subs.quota_account', {
                      remaining: subsAccount.remaining_downloads,
                      allowed: subsAccount.allowed_downloads,
                    })}
                  </span>
                {/if}
                <button
                  class="btn-outline subs-sm"
                  disabled={subsAuthBusy}
                  onclick={() => void subsSignOut()}
                >
                  {t('subs.sign_out')}
                </button>
              </div>
              {#if subsAccount.keychain_failed}
                <div class="setting-hint">{t('subs.keychain_failed')}</div>
              {/if}
            {:else if subsAuthOpen}
              <div class="subs-auth" bind:this={subsAuthEl}>
                <input
                  class="link-input"
                  type="text"
                  autocomplete="username"
                  spellcheck="false"
                  autocapitalize="off"
                  autocorrect="off"
                  placeholder={t('subs.username')}
                  bind:this={subsAuthUserEl}
                  bind:value={subsUser}
                  onkeydown={(e) => e.stopPropagation()}
                />
                <input
                  class="link-input"
                  type="password"
                  autocomplete="current-password"
                  placeholder={t('subs.password')}
                  bind:value={subsPass}
                  onkeydown={(e) => {
                    if (e.key === 'Enter') { e.preventDefault(); void subsSignIn(); }
                    e.stopPropagation();
                  }}
                />
                <!-- A boolean is a switch in this UI, never a pill. Labelled
                     the way the settings rows are: the text is a sibling and
                     the switch carries its own `aria-label`, because a `<label>`
                     around a `<button>` labels nothing and does not forward
                     clicks either. -->
                <div class="subs-remember">
                  <button
                    class="switch"
                    class:on={subsRemember}
                    role="switch"
                    aria-checked={subsRemember}
                    aria-label={t('subs.remember')}
                    onclick={() => (subsRemember = !subsRemember)}
                  ><span class="switch-knob"></span></button>
                  <span>{t('subs.remember')}</span>
                </div>
                <div class="setting-hint">{t('subs.remember_hint')}</div>
                <!-- A link, not a button: this genuinely navigates — it opens
                     the registration page in the browser. -->
                <div class="setting-hint">
                  {t('subs.no_account')}
                  <button class="settings-link" onclick={() => void openUrl(OPENSUBTITLES_SIGNUP)}>
                    {t('subs.register')}
                  </button>
                </div>
                {#if subsAuthError}
                  <div class="link-error">{subsAuthError}</div>
                {/if}
                <div class="link-actions">
                  <button class="btn-outline" onclick={() => { subsAuthOpen = false; subsPass = ''; }}>
                    {t('subs.cancel')}
                  </button>
                  <button
                    class="primary"
                    disabled={subsAuthBusy || !subsUser.trim() || !subsPass}
                    onclick={() => void subsSignIn()}
                  >
                    {subsAuthBusy ? t('subs.signing_in') : t('subs.sign_in')}
                  </button>
                </div>
              </div>
            {:else}
              <div class="subs-account-line">
                <span class="subs-account-hint">{t('subs.sign_in_hint')}</span>
                <button class="btn-outline subs-sm" onclick={() => void openSubsAuth()}>
                  {t('subs.sign_in')}
                </button>
              </div>
            {/if}
          </div>
        </div>
      </div>
    {/if}

    {#if infoOpen}
      <div
        class="settings-backdrop"
        role="presentation"
        onclick={() => (infoOpen = false)}
        ondblclick={(e) => e.stopPropagation()}
        oncontextmenu={blockContextMenu}
      >
        <div
          class="settings scrollable"
          role="dialog"
          aria-label={t('info.title')}
          tabindex="-1"
          onclick={(e) => e.stopPropagation()}
        >
          <div class="settings-head">
            <span>{t('info.title')}</span>
            <button class="settings-close" data-tip={t('set.close')} aria-label={t('bar.close')} onclick={() => (infoOpen = false)}>
              <svg viewBox="0 0 10 10"><path stroke="currentColor" d="M0 0l10 10M10 0L0 10"/></svg>
            </button>
          </div>
          {#if mediaInfo}
            {@const info = mediaInfo}
            <div class="info-section">{t('info.file')}</div>
            {@render infoRow(t('info.name'), player.filename)}
            {@render infoRow(t('info.container'), info.container?.toUpperCase() ?? null)}
            {@render infoRow(t('info.size'), info.size ? fmtSize(info.size) : null)}
            {@render infoRow(t('info.duration'), info.duration ? formatTime(info.duration) : null)}
            {@render infoRow(
              t('info.overall'),
              overallBitrate(info) ? fmtRate(overallBitrate(info)!) : null,
            )}

            <div class="info-section">{t('info.video')}</div>
            {@render infoRow(t('info.codec'), info.video.codec)}
            {@render infoRow(
              t('info.resolution'),
              info.video.width && info.video.height ? `${info.video.width} × ${info.video.height}` : null,
            )}
            {@render infoRow(t('info.fps_label'), info.video.fps ? fmtFps(info.video.fps) : null)}
            {@render infoRow(t('info.bitrate'), info.video.bitrate ? fmtRate(info.video.bitrate) : null)}
            {@render infoRow(t('info.pixfmt'), info.video.pixelFormat)}
            {@render infoRow(
              isHdr(info.video) ? t('info.hdr') : t('info.colour'),
              colourLine(info.video),
            )}
            {@render infoRow(
              t('info.peak'),
              info.video.maxLuma ? t('info.nits', { value: Math.round(info.video.maxLuma) }) : null,
            )}
            {@render infoRow(
              t('info.decoding'),
              info.video.hwdec === 'no'
                ? t('info.decode_sw')
                : info.video.hwdec
                  ? t('info.decode_hw', { name: info.video.hwdec })
                  : null,
            )}

            <div class="info-section">{t('info.audio')}</div>
            {@render infoRow(t('info.codec'), info.audio.codec)}
            {@render infoRow(t('info.lang'), info.audio.lang)}
            {@render infoRow(t('info.channels'), info.audio.channels)}
            {@render infoRow(
              t('info.samplerate'),
              info.audio.sampleRate ? t('info.khz', { value: info.audio.sampleRate / 1000 }) : null,
            )}
            {@render infoRow(t('info.bitrate'), info.audio.bitrate ? fmtRate(info.audio.bitrate) : null)}

            <!-- Torrent state lives here rather than in chrome of its own: this
                 panel already answers "what is this and how is it being played",
                 and where the bytes are coming from is the same question. -->
            {#if torrent.status && torrent.status.state !== 'gone'}
              {@const ts = torrent.status}
              <div class="info-section">{t('torrent.info')}</div>
              {@render infoRow(t('torrent.info_peers'), String(ts.peers))}
              {@render infoRow(t('torrent.info_speed'), fmtSpeed(ts.down_bps))}
              {@render infoRow(
                t('torrent.info_downloaded'),
                ts.file_size ? `${fmtSize(ts.file_done)} / ${fmtSize(ts.file_size)}` : null,
              )}
            {/if}
          {/if}
        </div>
      </div>
    {/if}

    {#snippet infoRow(label: string, value: string | null)}
      <!-- A row with nothing in it is worse than a missing row: half of these
           properties are simply absent depending on the container, the codec and
           how long the file has been playing. -->
      {#if value}
        <div class="info-row">
          <span class="info-label">{label}</span>
          <span class="info-value">{value}</span>
        </div>
    {/if}
  {/snippet}

  {#snippet submenuHead(key: CtxSub, label: string, disabled: boolean)}
    <!-- Opens on hover and on click — the click is for keyboard and for
         trackpads without hover, and it is the ONLY way in when the window is
         too narrow for a panel beside the menu (see hoverSubmenu). -->
    <div
      class="submenu-wrap"
      role="presentation"
      bind:this={ctxWraps[key]}
      onmouseenter={() => hoverSubmenu(key)}
      onmouseleave={hoverLeaveSubmenu}
    >
      <button
        class="menu-item submenu-head"
        class:open={ctxSubmenu === key}
        {disabled}
        onclick={() => { if (ctxSubmenu === key) closeSubmenuNow(); else openSubmenu(key); }}
      >
        <span>{label}</span>
        <svg class="caret" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
          <path
            d="M6 3.5 10.5 8 6 12.5"
            fill="none"
            stroke="currentColor"
            stroke-width="1.6"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
      </button>
    </div>
  {/snippet}

  {#snippet submenuPanel(key: CtxSub, body: Snippet)}
    {#if ctxSubmenu === key}
      <div
        class="ctxmenu submenu scrollable"
        role="menu"
        tabindex="-1"
        bind:this={ctxSubEl}
        onmouseenter={() => hoverSubmenu(key)}
        onmouseleave={hoverLeaveSubmenu}
        onclick={(e) => e.stopPropagation()}
        ondblclick={(e) => e.stopPropagation()}
        oncontextmenu={blockContextMenu}
        style={ctxSubPos
          ? `left: ${ctxSubPos.left}px; top: ${ctxSubPos.top}px; max-height: ${ctxSubPos.maxH}px`
          : 'left: 0; top: 0; visibility: hidden'}
      >
        {@render submenuBack()}
        {@render body()}
      </div>
    {/if}
  {/snippet}

  {#snippet fileItems()}
    <button class="menu-item" onclick={() => { ctxMenu = null; void openFileDialog(); }}>
      {t('ctx.open')} <span class="hint">{hint('open_file')}</span>
    </button>
    <button class="menu-item" onclick={() => { ctxMenu = null; void openLinkDialog(); }}>
      {t('ctx.open_link')} <span class="hint">{hint('open_link')}</span>
    </button>
    <button
      class="menu-item"
      disabled={!player.filePath || isNetworkSource(player.filePath)}
      onclick={() => { ctxMenu = null; if (player.filePath) void revealItemInDir(player.filePath); }}
    >
      {t(IS_MAC ? 'ctx.reveal_mac' : 'ctx.reveal_win')}
    </button>
    <button class="menu-item" disabled={!hasFile} onclick={() => { ctxMenu = null; toggleInfo(); }}>
      {t('ctx.info')} <span class="hint">{hint('info')}</span>
    </button>
    <!-- Named for where it goes rather than what it does to mpv: "stop" is the
         mechanism, and the reason anyone reaches for it is to get back to the
         list of what they were watching. Everything else follows from unloading
         the file — the `filename === null` branch of the property hook already
         commits the position, drops the posters, leaves the mini player and
         releases the torrent. -->
    <button class="menu-item" disabled={!hasFile} onclick={() => { ctxMenu = null; void backToStart(); }}>
      {t('ctx.back_to_start')}
    </button>
  {/snippet}

  {#snippet frameItems()}
    <button class="menu-item" disabled={!hasFile} onclick={() => { ctxMenu = null; void saveScreenshot(false); }}>
      {t('ctx.shot')} <span class="hint">{hint('screenshot')}</span>
    </button>
    <!-- Only worth offering while subtitles are actually on: otherwise the
         two variants would save byte-identical files. -->
    {#if player.subTracks.some((track) => track.selected)}
      <button class="menu-item" disabled={!hasFile} onclick={() => { ctxMenu = null; void saveScreenshot(true); }}>
        {t('ctx.shot_subs')} <span class="hint">{hint('screenshot_subs')}</span>
      </button>
    {/if}
    <button class="menu-item" disabled={!hasFile} onclick={() => { ctxMenu = null; void copyScreenshot(); }}>
      {t('ctx.shot_copy')} <span class="hint">{hint('copy_frame')}</span>
    </button>
  {/snippet}

  {#snippet playbackItems()}
    <button class="menu-item" class:active={player.loopMode !== 'off'} disabled={!hasFile} onclick={() => { ctxMenu = null; cycleLoop(); }}>
      {t(LOOP_LABEL[player.loopMode])} <span class="hint">{hint('loop')}</span>
    </button>
    <!-- Only for files that have chapters, which is a minority — but on
         Windows there is no menu bar, so this is the only place the shortcut
         is ever spelled out. -->
    {#if player.hasChapters}
      <button class="menu-item" onclick={() => { ctxMenu = null; jumpChapter(-1); }}>
        {t('ctx.chapter_prev')} <span class="hint">{hint('chapter_prev')}</span>
      </button>
      <button class="menu-item" onclick={() => { ctxMenu = null; jumpChapter(1); }}>
        {t('ctx.chapter_next')} <span class="hint">{hint('chapter_next')}</span>
      </button>
    {/if}
    <!-- Stays open: A and B are set one after the other, and closing after the
         first mark would mean opening the menu again for the second. -->
    <button class="menu-item" class:active={player.loopA !== null} disabled={!hasFile} onclick={() => cycleAbLoop()}>
      {t(player.loopA === null ? 'ctx.ab_set_a' : player.loopB === null ? 'ctx.ab_set_b' : 'ctx.ab_clear')}
      <span class="hint">{hint('ab_loop')}</span>
    </button>
    <div class="menu-title">{t('ctx.speed')}</div>
    <div class="speedrow">
      {#each [0.5, 1, 1.25, 1.5, 2] as s (s)}
        <button
          class="speedopt"
          class:sel={Math.abs(player.speed - s) < 0.01}
          onclick={() => { ctxMenu = null; void setProperty('speed', s); showOsd(t('osd.speed', { value: s }), { progress: (s - 0.25) / 3.75 }); }}
        >
          {s}×
        </button>
      {/each}
    </div>
  {/snippet}

  {#snippet pictureItems()}
    <!-- Every option here stays open: these are tried against the picture and
         compared, not chosen once from a list — the same reasoning as the
         delay stepper in the track menus. -->
    <div class="menu-title">{t('ctx.rotate')}</div>
    <div class="speedrow">
      {#each [0, 90, 180, 270] as deg (deg)}
        <button
          class="speedopt"
          class:sel={player.videoRotate === deg}
          onclick={() => setPicture('video-rotate', String(deg))}
        >
          {deg}°
        </button>
      {/each}
    </div>
    <div class="menu-title">{t('ctx.aspect')}</div>
    <div class="speedrow">
      <!-- Compared numerically: mpv answers "4:3" with 1.333333, so the
           string that was sent is not what comes back. -->
      <button
        class="speedopt"
        class:sel={player.aspectOverride < 0}
        onclick={() => setPicture('video-aspect-override', ASPECT_AUTO)}
      >
        {t('ctx.aspect_auto')}
      </button>
      {#each ASPECTS as a (a.label)}
        <button
          class="speedopt"
          class:sel={Math.abs(player.aspectOverride - a.ratio) < 0.01}
          onclick={() => setPicture('video-aspect-override', a.label)}
        >
          {a.label}
        </button>
      {/each}
    </div>
    <div class="menu-sep"></div>
    <button
      class="menu-item"
      class:active={player.panscan > 0}
      onclick={() => setPicture('panscan', player.panscan > 0 ? '0' : '1')}
    >
      {t('ctx.panscan')}
    </button>
  {/snippet}

  {#snippet windowItems()}
    <button class="menu-item" class:active={windowPrefs.remember} onclick={() => toggleWindowPref('remember')}>
      {t('ctx.win_remember')}
    </button>
    <button class="menu-item" class:active={windowPrefs.fitToVideo} onclick={() => toggleWindowPref('fitToVideo')}>
      {t('ctx.win_fit')}
    </button>
    <button class="menu-item" class:active={windowPrefs.alwaysOnTop} onclick={() => toggleWindowPref('alwaysOnTop')}>
      {t('ctx.win_ontop')}
    </button>
    <button class="menu-item" class:active={windowPrefs.snapMini} onclick={() => toggleWindowPref('snapMini')}>
      {t('ctx.win_snap')}
    </button>
    <div class="menu-sep"></div>
    <div class="menu-title">{t('ctx.win_size')}</div>
    <div class="speedrow">
      {#each [0.5, 1, 2] as z (z)}
        <button
          class="speedopt"
          disabled={!hasFile || player.videoW <= 0}
          onclick={() => { ctxMenu = null; closeSubmenuNow(); void fitWindowToVideo(z); }}
        >
          {z * 100}%
        </button>
      {/each}
    </div>
  {/snippet}

  {#snippet submenuBack()}
    <!-- Only in drill-down: the parent is hidden behind this panel, so without
         a way back the only exit is closing the menu and opening it again. -->
    {#if ctxDrill}
      <button class="menu-item back" onclick={closeSubmenuNow}>
        <svg class="caret" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
          <path d="M10 3.5 5.5 8 10 12.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
        <span>{t('ctx.back')}</span>
      </button>
      <div class="menu-sep"></div>
    {/if}
  {/snippet}

  {#if settingsOpen}
    <div
      class="settings-backdrop"
      role="presentation"
      onclick={() => (settingsOpen = false)}
      ondblclick={(e) => e.stopPropagation()}
      oncontextmenu={blockContextMenu}
    >
      <div class="settings scrollable" role="dialog" aria-label={t('set.title')} tabindex="-1" onclick={(e) => e.stopPropagation()}>
        <div class="settings-head">
          <span>{t('set.title')}</span>
          <button class="settings-close" data-tip={t('set.close')} aria-label={t('bar.close')} onclick={() => (settingsOpen = false)}>
            <svg viewBox="0 0 10 10"><path stroke="currentColor" d="M0 0l10 10M10 0L0 10"/></svg>
          </button>
        </div>
        <div class="tabs" role="tablist">
          {#each SETTINGS_TABS as tab (tab.id)}
            <button
              class="tab"
              class:sel={settingsTab === tab.id}
              role="tab"
              aria-selected={settingsTab === tab.id}
              onclick={() => (settingsTab = tab.id)}
            >
              {tab.label}
            </button>
          {/each}
        </div>
        {#if settingsTab === 'general'}
          <div class="setting">
            <div class="setting-label">{t('set.language')}</div>
            <div class="segmented">
              <button class="segopt" class:sel={locale() === 'ru'} onclick={() => changeLocale('ru')}>
                Русский
              </button>
              <button class="segopt" class:sel={locale() === 'en'} onclick={() => changeLocale('en')}>
                English
              </button>
            </div>
          </div>

          <div class="setting">
            <div class="row-toggle">
              <div class="row-text">
                <div class="setting-label">{t('set.history')}</div>
                <div class="setting-hint">{t('set.history_hint')}</div>
              </div>
              <button
                class="switch"
                class:on={history.prefs.enabled}
                role="switch"
                aria-checked={history.prefs.enabled}
                aria-label={t('set.history')}
                onclick={toggleHistory}
              >
                <span class="switch-knob"></span>
              </button>
            </div>
          </div>

          <!-- Dimmed together with disabled history: when nothing is written,
               the exclusions affect nothing, and that should be visible. -->
          <div class="setting" class:muted={!history.prefs.enabled}>
            <div class="setting-label">{t('set.excluded')}</div>
            <div class="setting-hint">{t('set.excluded_hint')}</div>
            <div class="folders">
              {#each history.prefs.excluded as dir (dir)}
                <div class="folder-row">
                  <svg class="folder-ico" viewBox="0 0 16 16" aria-hidden="true">
                    <path
                      d="M1.75 3.5h3.9l1.2 1.6h7.4v7.4H1.75z"
                      fill="none"
                      stroke="currentColor"
                      stroke-width="1.3"
                      stroke-linejoin="round"
                    />
                  </svg>
                  <!-- bdi isolates the text direction: the container is rtl (so
                       the ellipsis lands on the left, since the meaningful part
                       of a path is its tail), and without isolation the slashes,
                       being neutral characters, move about and "/Users/…" is
                       drawn as "Users/…/". -->
                  <span class="folder-path" title={dir}><bdi>{dir}</bdi></span>
                  <button
                    class="folder-remove"
                    data-tip={t('set.excluded_remove')}
                    aria-label={t('set.excluded_remove')}
                    disabled={!history.prefs.enabled}
                    onclick={() => removeExcludedFolder(dir)}
                  >
                    <!-- Same coordinates as the card's cross: round-capped ends
                         must stay inside the viewBox, or they get clipped at
                         the edge. -->
                    <svg viewBox="0 0 10 10" aria-hidden="true">
                      <path stroke="currentColor" stroke-width="1.4" stroke-linecap="round" d="M1.2 1.2l7.6 7.6M8.8 1.2l-7.6 7.6" />
                    </svg>
                  </button>
                </div>
              {:else}
                <div class="folders-empty">{t('set.excluded_empty')}</div>
              {/each}
              <button class="folder-add" disabled={!history.prefs.enabled} onclick={addExcludedFolder}>
                <svg viewBox="0 0 16 16" aria-hidden="true">
                  <path stroke="currentColor" stroke-width="1.5" stroke-linecap="round" d="M8 3.5v9M3.5 8h9" />
                </svg>
                {t('set.excluded_add')}
              </button>
            </div>
          </div>

          <div class="setting">
            <div class="setting-label">{t('set.clear')}</div>
            <div class="setting-hint">{t('set.clear_hint')}</div>
            <button class="btn-danger" onclick={() => void clearHistory()}>{t('set.clear_btn')}</button>
          </div>

        {:else if settingsTab === 'torrents'}
          <!-- Seeding was under the privacy controls, on the argument that it is
               the same kind of decision as an excluded folder — what leaves this
               machine. That argument still holds and is not why it moved: a
               viewer looking for anything about torrents had no section to look
               in, and found the switch and the cache button by reading a tab
               named "Основные" to the bottom. The two are here together because
               they are the whole of what this player decides about a torrent —
               what goes out, and what stays on the disk. A switch, because it is
               on/off; the hint carries the legal weight. -->
          <div class="setting">
            <div class="row-toggle">
              <div class="row-text">
                <div class="setting-label">{t('torrent.seed')}</div>
                <div class="setting-hint">{t('torrent.seed_hint')}</div>
              </div>
              <button
                class="switch"
                class:on={torrentPrefs.seeding}
                role="switch"
                aria-checked={torrentPrefs.seeding}
                aria-label={t('torrent.seed')}
                onclick={toggleSeeding}
              >
                <!-- `.switch-knob`, like every other switch in this dialog. A
                     bare `.knob` has no rule anywhere, so this one rendered as a
                     track with nothing in it — on/off told apart only by the
                     background. -->
                <span class="switch-knob"></span>
              </button>
            </div>
          </div>

          <!-- Streaming a torrent writes the pieces to disk, so a few films fill
               a directory the viewer never chose to fill. -->
          <div class="setting">
            <div class="setting-label">{t('torrent.cache_clear')}</div>
            <div class="setting-hint">{t('torrent.cache_hint')}</div>
            <button class="btn-danger" onclick={clearTorrentCache}>{t('torrent.cache_clear')}</button>
          </div>
        {:else if settingsTab === 'tv'}
          <!-- The mode is the transport, and the hint switches with the pill:
               each value explains its own consequences in one breath, because
               "prepare vs HLS" means nothing to a viewer until it is said in
               terms of start-up wait, seeking, sound and disk. -->
          <div class="setting">
            <div class="setting-label">{t('cast.set_mode')}</div>
            <div class="segmented">
              <button
                class="segopt"
                class:sel={castModeVal === 'prepare'}
                onclick={() => setCastModeHere('prepare')}
              >
                {t('cast.mode_prepare')}
              </button>
              <button
                class="segopt"
                class:sel={castModeVal === 'hls'}
                onclick={() => setCastModeHere('hls')}
              >
                {t('cast.mode_hls')}
              </button>
            </div>
            <div class="setting-hint">
              {castModeVal === 'hls' ? t('cast.mode_hls_hint') : t('cast.mode_prepare_hint')}
            </div>
            <div class="setting-hint">{t('cast.mode_torrent_note')}</div>
          </div>
          <div class="setting">
            <div class="setting-label">{t('cast.set_cache')}</div>
            <div class="segmented">
              {#each CAST_CAP_CHOICES as cap (cap)}
                <button
                  class="segopt"
                  class:sel={castCapVal === cap}
                  onclick={() => setCastCapHere(cap)}
                >
                  {cap === 0 ? t('cast.cache_none') : t('cast.cap_gb', { n: cap })}
                </button>
              {/each}
            </div>
            <div class="setting-hint">{t('cast.cache_hint')}</div>
            {#if castCacheBytes !== null}
              <div class="setting-hint">{t('cast.cache_size', { size: fmtSize(castCacheBytes) })}</div>
            {/if}
            <button class="btn-danger" onclick={() => void clearCastCacheHere()}>
              {t('cast.cache_clear')}
            </button>
          </div>
        {:else if settingsTab === 'playback'}
          <div class="setting">
            <div class="row-toggle">
              <div class="row-text">
                <div class="setting-label">{t('set.queue')}</div>
                <div class="setting-hint">{t('set.queue_hint')}</div>
              </div>
              <button
                class="switch"
                class:on={playlist.queueFolder}
                role="switch"
                aria-checked={playlist.queueFolder}
                aria-label={t('set.queue')}
                onclick={() => setPlaylistPref('queueFolder', !playlist.queueFolder)}
              >
                <span class="switch-knob"></span>
              </button>
            </div>
          </div>

          <div class="setting">
            <div class="row-toggle">
              <div class="row-text">
                <div class="setting-label">{t('set.autoadvance')}</div>
                <div class="setting-hint">{t('set.autoadvance_hint')}</div>
              </div>
              <button
                class="switch"
                class:on={playlist.autoAdvance}
                role="switch"
                aria-checked={playlist.autoAdvance}
                aria-label={t('set.autoadvance')}
                onclick={() => setPlaylistPref('autoAdvance', !playlist.autoAdvance)}
              >
                <span class="switch-knob"></span>
              </button>
            </div>
          </div>

        {:else if settingsTab === 'keys'}
          {#each GROUP_ORDER as group (group)}
            <div class="keys-group">{t(`kgroup.${group}`)}</div>
            {#each ACTIONS.filter((a) => a.group === group) as def (def.id)}
              {@const chords = chordsOf(def.id)}
              <div class="setting keyrow">
                <div class="keyrow-name">{actionLabel(def.id)}</div>
                <div class="keyrow-chords">
                  {#each chords as chord (chord)}
                    <!-- A chord the macOS menu bar owns is shown but not
                         removable: taking it off here would change nothing,
                         since the system delivers it from the menu either way. -->
                    <span class="kbadge" class:fixed={isMenuAccelerator(chord)}>
                      {chordLabel(chord)}
                      {#if !isMenuAccelerator(chord)}
                        <button
                          class="kbadge-x"
                          data-tip={t('keys.remove')}
                          aria-label={t('keys.remove')}
                          onclick={() => unassign(def.id, chord)}
                        >
                          <svg viewBox="0 0 10 10"><path stroke="currentColor" d="M0 0l10 10M10 0L0 10"/></svg>
                        </button>
                      {/if}
                    </span>
                  {/each}
                  {#if IS_MAC && def.menuMac && !chords.includes(def.menuMac)}
                    <span class="kbadge fixed" data-tip={t('keys.menu_note')}>
                      {chordLabel(def.menuMac)}
                    </span>
                  {/if}
                  {#if chords.length === 0 && !(IS_MAC && def.menuMac)}
                    <span class="kbadge empty">{t('keys.none')}</span>
                  {/if}
                  {#if recordingAction === def.id}
                    <span class="kbadge recording">{t('keys.press')}</span>
                  {:else}
                    <button
                      class="kadd"
                      data-tip={t('keys.add')}
                      aria-label={t('keys.add')}
                      onclick={() => startRecording(def.id)}
                    >+</button>
                  {/if}
                  {#if isCustom(def.id)}
                    <button class="settings-link kreset" onclick={() => { keyNote = null; resetAction(def.id); }}>
                      {t('keys.restore')}
                    </button>
                  {/if}
                </div>
              </div>
              {#if recordingAction === def.id}
                <div class="setting-hint keynote">{t('keys.press_hint')}</div>
              {:else if keyNote?.id === def.id}
                <div class="setting-hint keynote">{keyNote.text}</div>
              {/if}
            {/each}
          {/each}
          <div class="settings-foot">{t('keys.footer')}</div>
          <button class="btn-danger" disabled={!hasCustomBindings()} onclick={() => { keyNote = null; resetAll(); }}>
            {t('keys.reset_all')}
          </button>

        {:else}
        {#each SETTINGS.filter((s) => s.tab === settingsTab) as s (s.key)}
          <div class="setting">
            <div class="setting-label">{s.label}</div>
            {#if s.kind === 'devices'}
              <div class="segmented vertical">
                {#each audioDevices as device (device.name)}
                  <!-- mpv's own list starts with `auto`, and that is also its
                       default — mapped to null so choosing it CLEARS the
                       mpv.conf line instead of pinning "auto" in writing, the
                       same meaning "по умолчанию" has everywhere else here. -->
                  {@const value = device.name === 'auto' ? null : device.name}
                  <button
                    class="segopt"
                    class:sel={(settingsValues[s.key] ?? null) === value}
                    onclick={() => setSetting(s, value)}
                  >
                    {device.description}
                  </button>
                {/each}
              </div>
            {:else if s.kind === 'langs'}
              {@const codes = langsOf(s)}
              <!-- Chips in priority order, then the way to add one. The empty
                   state says "авто" as a word rather than showing an empty
                   strip, which reads as a control that failed to render. -->
              <div class="langs">
                {#if !codes.length}
                  <span class="langs-auto">{t('vset.lang_auto')}</span>
                {/if}
                {#each codes as code, i (code)}
                  <!-- Two buttons in a chip, never nested: a button inside a
                       button is invalid and the inner one stops being clicked.
                       The first chip is the preferred language and says so by
                       being filled, which is also why clicking it does nothing
                       — it is already what it would become. -->
                  <span class="lang-chip" class:first={i === 0}>
                    <button
                      class="lang-name"
                      data-tip={i === 0 ? t('vset.lang_primary') : t('vset.lang_promote')}
                      disabled={i === 0}
                      onclick={() => promoteLang(s, code)}
                    >{languageName(code)}</button>
                    <button
                      class="lang-drop"
                      aria-label={t('vset.lang_remove')}
                      data-tip={t('vset.lang_remove')}
                      onclick={() => removeLang(s, code)}
                    >
                      <svg viewBox="0 0 10 10" aria-hidden="true">
                        <path stroke="currentColor" stroke-width="1.4" stroke-linecap="round" d="M1.2 1.2l7.6 7.6M8.8 1.2l-7.6 7.6" />
                      </svg>
                    </button>
                  </span>
                {/each}
                <button
                  class="lang-add"
                  class:open={langPickerFor === s.key}
                  data-tip={t('vset.lang_add')}
                  aria-label={t('vset.lang_add')}
                  onclick={() => void openLangPicker(s)}
                >
                  <svg viewBox="0 0 16 16" aria-hidden="true">
                    <path d="M8 3.5v9M3.5 8h9" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
                  </svg>
                </button>
              </div>
              {#if langPickerFor === s.key}
                <!-- Sixty languages behind a search field, which is the whole
                     shape of this control: the common case is two clicks and the
                     long tail is one word of typing away, without either being
                     in the other's way. -->
                <div class="lang-picker">
                  <input
                    class="lang-search"
                    type="text"
                    spellcheck="false"
                    autocapitalize="off"
                    autocorrect="off"
                    placeholder={t('vset.lang_search')}
                    bind:this={langQueryEl}
                    bind:value={langQuery}
                    onkeydown={(e) => {
                      // Enter takes the first hit — with a filtered list of one,
                      // reaching for the mouse is the whole cost of the feature.
                      if (e.key === 'Enter') {
                        e.preventDefault();
                        const hit = searchLanguages(langQuery, codes)[0];
                        if (hit) addLang(s, hit.code);
                      }
                      if (e.key === 'Escape') { e.preventDefault(); langPickerFor = null; }
                      e.stopPropagation();
                    }}
                  />
                  <div class="lang-list">
                    {#each searchLanguages(langQuery, codes) as lang (lang.code)}
                      <button class="lang-option" onclick={() => addLang(s, lang.code)}>
                        <span class="lang-option-name">{lang.name}</span>
                        <span class="lang-option-code">{lang.code}</span>
                      </button>
                    {:else}
                      <div class="folders-empty">{t('vset.lang_none')}</div>
                    {/each}
                  </div>
                </div>
              {/if}
            {:else if s.kind === 'slider'}
              {@const value = sliderValue(s)}
              <div class="slider-row">
                <input
                  type="range"
                  class="setting-slider"
                  min={s.min}
                  max={s.max}
                  step={s.step}
                  {value}
                  style="--progress: {((value - s.min) / (s.max - s.min)) * 100}%"
                  aria-label={s.label}
                  oninput={(e) => setSlider(s, Number(e.currentTarget.value))}
                  onchange={(e) => e.currentTarget.blur()}
                />
                <span class="slider-value">{value.toFixed(s.decimals)}{s.suffix ?? ''}</span>
              </div>
            {:else}
              <div class="segmented">
                {#each s.options as o (o.label)}
                  <button
                    class="segopt"
                    class:sel={settingsValues[s.key] === o.v}
                    disabled={s.key === 'target-colorspace-hint' &&
                      o.v !== null &&
                      displayHdr !== null &&
                      !displayHdr.supported}
                    onclick={() => setSetting(s, o.v)}
                  >
                    {o.label}
                  </button>
                {/each}
              </div>
            {/if}
            {#if s.key === 'target-colorspace-hint' && displayHdr}
              <div class="setting-hint">
                {#if !displayHdr.supported}
                  {t('set.hdr_unsupported')}
                {:else if !displayHdr.enabled}
                  {t(IS_MAC ? 'set.hdr_off_mac' : 'set.hdr_off_win')}
                {:else if settingsValues[s.key] === null}
                  {t(IS_MAC ? 'set.hdr_forced_mac' : 'set.hdr_forced_win')}
                {:else}
                  {t(IS_MAC ? 'set.hdr_on_mac' : 'set.hdr_on_win')}
                {/if}
              </div>
            {:else if s.hint}<div class="setting-hint">{s.hint}</div>{/if}
          </div>
        {/each}
          {#if settingsTab === 'audio'}
          <!-- Bitstream passthrough hands the receiver an undecoded stream, and a
               filter has nothing to attach to — so the switch says why it is
               unavailable rather than silently doing nothing. Kept by the player
               rather than written to mpv.conf (see the module), which is why it
               sits below the footer's claim about mpv's own settings. -->
          {@const bitstream = Boolean(settingsValues['audio-spdif'])}
          <div class="setting" class:muted={bitstream}>
            <div class="row-toggle">
              <div class="row-text">
                <div class="setting-label">{t('set.normalise')}</div>
                <div class="setting-hint">
                  {t(bitstream ? 'set.normalise_spdif' : 'set.normalise_hint')}
                </div>
              </div>
              <button
                class="switch"
                class:on={player.normalise && !bitstream}
                role="switch"
                aria-checked={player.normalise && !bitstream}
                disabled={bitstream}
                aria-label={t('set.normalise')}
                onclick={() => applyNormalise(!player.normalise)}
              >
                <span class="switch-knob"></span>
              </button>
            </div>
          </div>

          {/if}
          {#if settingsTab === 'video'}
          {#if hwdecCurrent}
            <div class="settings-foot">
              {t('set.hwdec_foot')}
              {hwdecCurrent === 'no'
                ? t('set.hwdec_sw')
                : t('set.hwdec_hw', { name: hwdecCurrent })}
            </div>
          {/if}
          {/if}
          <div class="settings-foot">
            {t('set.conf_foot')}
            <button
              class="settings-link"
              disabled={!player.mpvConfPath}
              onclick={() => { if (player.mpvConfPath) void revealItemInDir(player.mpvConfPath); }}
            >
              {t(IS_MAC ? 'set.conf_reveal_mac' : 'set.conf_reveal_win')}
            </button>
          </div>
        {/if}
      </div>
    </div>
  {/if}

  {#if tooltip}
    <div
      class="tip"
      bind:this={tipEl}
      style={tooltip.pos
        ? `transform: translate3d(${tooltip.pos.x}px, ${tooltip.pos.y}px, 0)`
        : 'visibility: hidden'}
    >
      {tooltip.text}
    </div>
  {/if}

  <div class="veil" class:on={fsTransition}></div>

  {#if osdState}
    <div class="osd" class:with-bar={osdState.progress !== undefined}>
      <span class="osd-text">{osdState.text}</span>
      {#if osdState.progress !== undefined}
        <div class="osd-bar"><div class="osd-bar-fill" style="width: {osdState.progress * 100}%"></div></div>
      {/if}
      {#if osdState.sub}
        <span class="osd-sub">{osdState.sub}</span>
      {/if}
    </div>
  {/if}

  <!-- Deliberately outside .osc: this is the one control that has to stay up
       while the UI is idle, since the whole point is to appear over video the
       viewer is not touching. Its offset is fixed rather than tied to the bar,
       so it does not hop when the OSC comes and goes. -->
  {#if skipHint}
    <button
      class="skipbtn"
      style="--skip-left: {skipHint.left}; opacity: {skipHint.fade}"
      onclick={(e) => {
        e.stopPropagation();
        takeSkip();
      }}
      ondblclick={(e) => e.stopPropagation()}
    >
      <!-- On the last chapter the button leaves the file entirely, and saying
           "skip credits" for something that starts the next episode would be a
           lie about where the click leads. -->
      <span class="skip-label">
        {t(skipHint.entry ? 'skip.next_episode' : SKIP_LABEL[skipHint.kind])}
      </span>
      <!-- Filled, like every other glyph in this player. The stroked chevrons
           that were here read as a web-page affordance among a set of solid
           Material-shaped icons. Not the OSC's next-file glyph (▶|) either:
           that one already means "next entry in the playlist". -->
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path fill="currentColor" d="M4 18l8.5-6L4 6v12zm9-12v12l8.5-6L13 6z" />
      </svg>
    </button>
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
  <div
    class="osc"
    class:hidden={idle}
    role="toolbar"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    ondblclick={(e) => e.stopPropagation()}
    onmouseenter={() => (oscHover = true)}
    onmouseleave={() => (oscHover = false)}
  >
    {#if openMenu === 'queue'}
      <div class="menu chapters queue scrollable" bind:this={queueMenuEl}>
        <div class="menu-title">{t('osc.queue')}</div>
        {#each playlist.entries as entry (entry.index)}
          <!-- A row and its remove button, not a button inside a button: nested
               interactive elements are invalid and the inner one stops being
               reachable. Same shape as the recents card and its × on the start
               screen. -->
          <div
            class="queue-row"
            class:dragging={dragArmed && dragFrom === entry.index}
            style="transform: translateY({queueRowShift(entry.index)}px)"
          >
            <button
              class="menu-item chapter-item"
              class:sel={entry.index === player.playlistPos}
              data-tip={entry.title}
              onpointerdown={(e) => onQueueDown(e, entry.index)}
              onpointermove={onQueueMove}
              onpointerup={(e) => void onQueueUp(e)}
              onpointercancel={(e) => void onQueueUp(e)}
              onclick={() => {
                // A drag ends with a click on the same element; the guard is
                // the flag the pointer handlers leave behind.
                if (dragArmed) return;
                openMenu = null;
                void playEntry(entry);
              }}
            >
              <span class="chapter-name">{entry.title}</span>
            </button>
            <button
              class="queue-remove"
              data-tip={t('osc.queue_remove')}
              aria-label={t('osc.queue_remove')}
              onclick={() => void removeFromQueue(entry.index)}
            >
              <svg viewBox="0 0 10 10" aria-hidden="true">
                <path stroke="currentColor" stroke-width="1.4" stroke-linecap="round" d="M1.2 1.2l7.6 7.6M8.8 1.2l-7.6 7.6"/>
              </svg>
            </button>
          </div>
        {/each}
      </div>
    {:else if openMenu === 'chapter'}
      <div class="menu chapters scrollable" bind:this={chapterMenuEl}>
        <div class="menu-title">{t('osc.chapters')}</div>
        {#each player.chapters as chapter (chapter.index)}
          <button
            class="menu-item chapter-item"
            class:sel={chapter.index === player.chapterIndex}
            onclick={() => {
              openMenu = null;
              seekChapter(chapter.index);
            }}
          >
            <span class="chapter-name">{chapterTitle(chapter)}</span>
            <span class="hint">{formatTime(chapter.time)}</span>
          </button>
        {/each}
      </div>
    {:else if openMenu === 'cast'}
      <div class="menu castmenu scrollable">
        <div class="menu-title">{t('cast.title')}</div>
        {#if cast.active}
          <div class="cast-current">
            <span class="cast-name">{cast.deviceName}</span>
            <span class="cast-state">{castStateLabel}</span>
          </div>
          <button
            class="menu-item"
            onclick={() => {
              openMenu = null;
              void disconnectCast();
            }}
          >
            {t('cast.disconnect')}
          </button>
        {:else}
          {#each cast.devices as device (device.id)}
            <button
              class="menu-item cast-device"
              onclick={() => {
                openMenu = null;
                void castCurrentFile(device);
              }}
            >
              <span class="cast-name">{device.name}</span>
              {#if device.model && device.model !== device.name}
                <span class="cast-model">{device.model}</span>
              {/if}
            </button>
          {:else}
            <div class="cast-empty">
              {castSearchLong ? t('cast.empty') : t('cast.searching')}
            </div>
          {/each}
          <!-- The number-one cause of "casting doesn't work" is the network,
               not the code, so the panel says so up front: the Defender prompt
               before the first cast, and the usual reasons a TV is invisible
               once the search has clearly come up dry. -->
          <div class="cast-hint">
            {castSearchLong && cast.devices.length === 0
              ? t('cast.empty_hint')
              : t('cast.firewall_warn')}
          </div>
        {/if}
      </div>
    {:else if trackMenu}
      <div class="menu scrollable">
        <div class="menu-title">{t(trackMenu === 'audio' ? 'osc.audio' : 'osc.subs')}</div>
        {#each trackMenu === 'audio' ? player.audioTracks : player.subTracks as track (track.id)}
          <!-- The × appears for external subtitle tracks only. An embedded one
               cannot be removed without rewriting the video file, and mpv's
               `sub-remove` refuses it too. -->
          {#if trackMenu === 'sub' && track.external}
            <div class="queue-row">
              <button
                class="menu-item chapter-item"
                class:sel={track.selected}
                data-tip={track.label}
                onclick={() => selectTrack('sub', track)}
              >
                <span class="chapter-name">{track.label}</span>
              </button>
              <button
                class="queue-remove"
                data-tip={t('osc.sub_remove')}
                aria-label={t('osc.sub_remove')}
                onclick={() => void removeSubtitle(track)}
              >
                <svg viewBox="0 0 10 10" aria-hidden="true">
                  <path stroke="currentColor" stroke-width="1.4" stroke-linecap="round" d="M1.2 1.2l7.6 7.6M8.8 1.2l-7.6 7.6"/>
                </svg>
              </button>
            </div>
          {:else}
            <button class="menu-item" class:sel={track.selected} onclick={() => selectTrack(trackMenu!, track)}>
              {track.label}
            </button>
          {/if}
        {/each}
        {#if trackMenu === 'sub'}
          <button
            class="menu-item"
            class:sel={!player.subTracks.some((track) => track.selected)}
            onclick={() => selectTrack('sub', null)}
          >
            {t('osc.subs_off')}
          </button>
        {/if}
        <button class="menu-item" onclick={() => void addTrackFile(trackMenu!)}>
          {t('osc.add_file')}
        </button>
        {#if trackMenu === 'sub'}
          <button class="menu-item" onclick={() => { openMenu = null; void openSubsDialog(); }}>
            {t('subs.find')}
          </button>
        {/if}
        <div class="menu-sep"></div>
        <div class="menu-title">{t('osc.delay')}</div>
        <!-- Stays open on click: a delay is dialled in by repeated nudges while
             watching the result, not chosen once from a list. -->
        <div class="delayrow">
          <button class="speedopt" aria-label="-0.1" onclick={() => nudgeDelayHere(trackMenu!, -DELAY_STEP)}>−</button>
          <span class="delayval">{formatDelay(trackMenu === 'audio' ? player.audioDelay : player.subDelay)}</span>
          <button class="speedopt" aria-label="+0.1" onclick={() => nudgeDelayHere(trackMenu!, DELAY_STEP)}>+</button>
        </div>
        <!-- Always rendered, only disabled: the menu is anchored by its bottom
             edge, so an item appearing here would push the −/+ row up by its
             own height the moment the delay leaves zero — right under the
             cursor that is clicking it. -->
        <button
          class="menu-item"
          disabled={(trackMenu === 'audio' ? player.audioDelay : player.subDelay) === 0}
          onclick={() => resetDelayHere(trackMenu!)}
        >
          {t('osc.delay_reset')}
        </button>
      </div>
    {/if}

    <div class="seekrow">
      <span class="time">{formatTime(seekValue)}</span>
      <div
        class="seekwrap"
        role="presentation"
        bind:this={seekWrapEl}
        onpointerdown={onSeekDown}
        onpointermove={onSeekMove}
        onpointerup={onSeekUp}
        onmouseleave={() => {
          if (!dragging) hoverTime = null;
        }}
      >
        {#if hoverTime !== null && hasFile && player.duration > 0}
          <div class="hovertip" style="left: {hoverX}px">
            {#if hasThumbs}
            <!-- The box keeps the VIDEO's aspect, not a hardcoded 16:9. mpv
                 reports `dwidth`/`dheight` — the size with the aspect already
                 applied — as soon as a file opens, so this is exact and needs no
                 frame to have arrived. Without it the placeholder was 16:9 and
                 the first real frame resized the whole popup, which a 4:3 film
                 or a phone clip did on every single hover. -->
            {@const aspect = thumbAspect}
            <!-- A position of a torrent that has not been downloaded cannot be
                 decoded at all (see `positionBuffered`). An empty outlined box
                 there reads as a preview that BROKE rather than one that was
                 never coming — the mistake the end-of-file cards already learned
                 not to make — so it says which it is. -->
            {@const previewable =
              !thumbs.partial || positionBuffered(hoverTime / Math.max(1e-9, player.duration))}
            <div class="thumbwrap" class:loading={thumbs.loading && previewable}>
              {#if previewable && thumbs.src}
                <img class="thumb" src={thumbs.src} alt="" style="aspect-ratio: {aspect}" />
              {:else if previewable}
                <div class="thumb placeholder" style="aspect-ratio: {aspect}"></div>
              {:else}
                <div class="thumb placeholder pending" style="aspect-ratio: {aspect}">
                  <svg viewBox="0 0 16 16" aria-hidden="true">
                    <path
                      d="M8 2.5v8m0 0L4.8 7.3M8 10.5l3.2-3.2M3 13.5h10"
                      fill="none"
                      stroke="currentColor"
                      stroke-width="1.6"
                      stroke-linecap="round"
                      stroke-linejoin="round"
                    />
                  </svg>
                </div>
              {/if}
              {#if previewable && thumbs.fading}
                <!-- Keyed so a fade following another gets a fresh element and
                     therefore restarts the animation instead of inheriting a
                     finished one. -->
                {#key thumbs.fading}
                  <img class="thumb fading" src={thumbs.fading} alt="" />
                {/key}
              {/if}
              <div class="spinner" class:on={thumbs.loading && previewable}></div>
            </div>
            {/if}
            <span>{formatTime(hoverTime)}</span>
            {#if hoverChapter}
              <span class="hover-chapter">{chapterTitle(hoverChapter)}</span>
            {/if}
          </div>
        {/if}
        <div class="seektrack">
          <!-- What is on disk, drawn UNDER the played fill: the bar's job here
               is to answer "will a jump there land, or wait", which is the one
               question a torrent's seekbar is really read for. -->
          {#each torrent.buffered as [from, to] (from)}
            <div
              class="seekbuffered"
              style="left: {from * 100}%; width: {(to - from) * 100}%"
            ></div>
          {/each}
          <div class="seekfill" style="width: {barDuration > 0 ? (seekValue / barDuration) * 100 : 0}%"></div>
          {#if abRegion}
            <div
              class="abregion"
              class:disarmed={!abRegion.armed}
              style="left: {abRegion.left}%; width: {abRegion.width}%"
            ></div>
          {/if}
          {#each chapterMarks as mark, i (i)}
            <div class="chapmark" style="left: {mark}%"></div>
          {/each}
          <div class="seekknob" style="left: {barDuration > 0 ? (seekValue / barDuration) * 100 : 0}%"></div>
        </div>
      </div>
      <span class="time">{formatTime(barDuration)}</span>
    </div>
    <div class="controls">
      <div class="cluster cl-left">
      <button data-tip={withKey(t('osc.sound'), 'mute')} aria-label={t('osc.sound')} onclick={() => (cast.remote ? castToggleMute() : toggleMute())}>
        {#if cast.remote ? cast.muted : player.muted || player.volume === 0}
          <svg viewBox="0 0 24 24"><path fill="currentColor" d="M16.5 12A4.5 4.5 0 0 0 14 8v2.2l2.5 2.5v-.7zM19 12a7 7 0 0 1-1.2 3.9l1.5 1.5A9 9 0 0 0 21 12a9 9 0 0 0-7-8.8v2.1A7 7 0 0 1 19 12zM4.3 3L3 4.3 7.7 9H3v6h4l5 5v-6.7l4.3 4.2a6.9 6.9 0 0 1-2.3 1.2v2.1a9 9 0 0 0 3.7-1.8L20 21.7 21.3 20 4.3 3zM12 4L9.9 6.1 12 8.2V4z"/></svg>
        {:else}
          <svg viewBox="0 0 24 24"><path fill="currentColor" d="M3 9v6h4l5 5V4L7 9H3zm13.5 3A4.5 4.5 0 0 0 14 8v8a4.5 4.5 0 0 0 2.5-4zM14 3.2v2.1a7 7 0 0 1 0 13.4v2.1a9 9 0 0 0 0-17.6z"/></svg>
        {/if}
      </button>
      <!-- While casting the bar is the TV's volume — the receiver's own 0..1
           shown as percent, with no amplification range: 100 is the device's
           ceiling, not a point on the way to 150. -->
      <input
        class="volumebar"
        type="range"
        min="0"
        max={cast.remote ? 100 : VOLUME_MAX}
        step="1"
        disabled={cast.remote && !cast.volumeAdjustable}
        value={cast.remote ? Math.round(cast.volume * 100) : player.volume}
        oninput={(e) =>
          cast.remote
            ? castSetVolume(Number(e.currentTarget.value) / 100)
            : setVolume(Number(e.currentTarget.value))}
        onchange={(e) => e.currentTarget.blur()}
        style="--progress: {cast.remote ? cast.volume * 100 : (player.volume / VOLUME_MAX) * 100}%"
        data-tip={t('osd.volume', { value: Math.round(cast.remote ? cast.volume * 100 : player.volume) })}
        aria-label={t('osd.volume', { value: Math.round(cast.remote ? cast.volume * 100 : player.volume) })}
      />
      {#if player.speed !== 1 && !cast.remote}
        <span class="speed" data-tip={t('osc.speed')} aria-label={t('osc.speed')}>{player.speed}×</span>
      {/if}
      </div>
      <div class="cluster cl-center">
      <button data-tip={withKey(t('osc.prev'), 'playlist_prev')} aria-label={t('osc.prev')} disabled={player.playlistPos <= 0 || cast.active} onclick={() => void command('playlist-prev', [])}>
        <svg viewBox="0 0 24 24"><path fill="currentColor" d="M6 6h2v12H6zm3.5 6l8.5 6V6z"/></svg>
      </button>
      <button class="play" data-tip={withKey(t('osc.play'), 'pause')} aria-label={t('osc.play')} disabled={showEmpty} onclick={() => (cast.remote ? castTogglePause() : void togglePause())}>
        {#if cast.remote ? cast.paused : player.paused || player.eofReached}
          <svg viewBox="0 0 24 24"><path fill="currentColor" d="M8 5v14l11-7z"/></svg>
        {:else}
          <svg viewBox="0 0 24 24"><path fill="currentColor" d="M6 19h4V5H6v14zm8-14v14h4V5h-4z"/></svg>
        {/if}
      </button>
      <button data-tip={withKey(t('osc.next'), 'playlist_next')} aria-label={t('osc.next')} disabled={player.playlistPos >= player.playlistCount - 1 || cast.active} onclick={() => void command('playlist-next', [])}>
        <svg viewBox="0 0 24 24"><path fill="currentColor" d="M16 6h2v12h-2zM6 18l8.5-6L6 6z"/></svg>
      </button>
      </div>
      <div class="cluster cl-right">
      <button
        data-tip={withKey(t(LOOP_LABEL[player.loopMode]), 'loop')}
        aria-label={t(LOOP_LABEL[player.loopMode])}
        class:active={player.loopMode !== 'off'}
        onclick={cycleLoop}
      >
        <svg viewBox="0 0 24 24">
          <path fill="currentColor" d="M17 7H7v3l-4-4 4-4v3h12v6h-2V7zM7 17h10v-3l4 4-4 4v-3H5v-6h2v4z"/>
          <!-- The digit goes in the gap between the two arrows (y 7–17 of the
               viewBox), which is exactly where Material's own repeat_one puts
               it — drawn as a path rather than a "1" glyph, since a glyph has
               no dependable centre in the em square. -->
          {#if player.loopMode === 'one'}
            <path fill="currentColor" d="M13 15V9h-1l-2 1v1h1.5v4H13z"/>
          {/if}
        </svg>
      </button>
      {#if playlist.hasQueue}
        <button
          class="menu-toggle"
          data-tip={t('osc.queue')}
          aria-label={t('osc.queue')}
          class:active={openMenu === 'queue'}
          onclick={() => toggleMenu('queue')}
        >
          <!-- The bars share the chapter icon's rhythm exactly — same rows,
               same thickness — because the two sit side by side and any
               difference in pitch reads as a mistake rather than as a
               distinction. Two earlier attempts got the vertical placement
               wrong in opposite directions: a play mark hanging BELOW the last
               bar dropped the glyph's centre to y=14 (against the box's 12),
               and a tighter pitch then lifted the bar group above its
               neighbour's.
               Everything horizontal here is on a 1.2-unit grid, which is what
               keeps the bars sharp: the 24-unit box renders into 20 px, so one
               unit is 5/6 px and only multiples of 1.2 land on a whole pixel —
               at 1x and, being multiples of 0.6 device px, at 2x as well.
               Off-grid edges bleed across two rows of pixels, which reads as
               bars that are both blurry and too thick, and a glyph in a
               different sub-pixel phase from its neighbour looks like a
               different weight. Hence 2.4 thick on rows 3.6 / 10.8 / 18, shared
               verbatim with the chapter icon.
               The play mark is centred on the last row and kept NARROW (4.8
               against 5.6 tall). A wide one read as another arrowhead beside
               the repeat icon, which carries one in the same corner — two
               right-pointing wedges side by side stop looking like two
               different controls. The ink it gives up is handed back to the
               bottom bar, whose row has to weigh as much as a full-width one or
               the whole glyph floats above its neighbour: measured 11.89
               against the chapter icon's 11.94, where the same mark over a
               short bar gave 11.73. -->
          <svg viewBox="0 0 24 24" fill="currentColor">
            <path d="M2.4 3.6h19.2v2.4H2.4zM2.4 10.8h19.2v2.4H2.4zM2.4 18h13.2v2.4H2.4zM16.8 16.4v5.6l4.8-2.8z"/>
          </svg>
        </button>
      {/if}
      {#if player.hasChapters}
        <button
          class="menu-toggle"
          data-type="chapters"
          data-tip={t('osc.with_key', { label: t('osc.chapters'), key: hintPair('chapter_prev', 'chapter_next') })}
          aria-label={t('osc.chapters')}
          class:active={openMenu === 'chapter'}
          onclick={() => toggleMenu('chapter')}
        >
          <!-- Drawn to fill the box like its neighbours rather than to the
               proportions of Material's own list glyph: that one occupies
               16×12 of the 24×24 viewBox where the loop is 18×20 and the mic
               18×18, and at a shared 20px it visibly sat smaller than both.
               Bars are 2.4 thick on rows 3.6 / 10.8 / 18 — the 1.2-unit grid
               explained on the queue icon, which shares these rows exactly so
               the two read as one family standing side by side. -->
          <svg viewBox="0 0 24 24" fill="currentColor">
            <path d="M8.4 3.6h13.2v2.4H8.4zM8.4 10.8h13.2v2.4H8.4zM8.4 18h13.2v2.4H8.4z"/>
            <circle cx="4.2" cy="4.8" r="1.8"/>
            <circle cx="4.2" cy="12" r="1.8"/>
            <circle cx="4.2" cy="19.2" r="1.8"/>
          </svg>
        </button>
      {/if}
      {#if player.audioTracks.length > 1}
        <button class="menu-toggle" data-tip={t('osc.audio')} aria-label={t('osc.audio')} class:active={openMenu === 'audio'} onclick={() => toggleMenu('audio')}>
          <svg viewBox="0 0 24 24"><path fill="currentColor" d="M12 3a9 9 0 0 0-9 9v7a2 2 0 0 0 2 2h3v-8H5v-1a7 7 0 1 1 14 0v1h-3v8h3a2 2 0 0 0 2-2v-7a9 9 0 0 0-9-9z"/></svg>
        </button>
      {/if}
      {#if hasFile}
        <button class="menu-toggle" data-tip={t('osc.subs')} aria-label={t('osc.subs')} class:active={openMenu === 'sub'} onclick={() => toggleMenu('sub')}>
          <svg viewBox="0 0 24 24"><path fill="currentColor" d="M20 4H4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V6a2 2 0 0 0-2-2zM6 10h2v2H6v-2zm8 6H6v-2h8v2zm4 0h-2v-2h2v2zm0-4H10v-2h8v2z"/></svg>
        </button>
      {/if}
      {#if (hasFile && (!isNetworkSource(player.filePath) || !!parseTorrentUrl(player.filePath ?? ''))) || cast.active}
        <!-- Indigo while connected: the accent means on/selected, and a live
             cast session is a boolean "on". Local files and torrents (whose
             data may be a complete file on this disk — resolveCastSource
             answers, and an incomplete one refuses with the reason); a plain
             network stream cannot be served to the TV from a disk it is
             not on. -->
        <button
          class="menu-toggle"
          class:cast-on={cast.active}
          data-tip={t('cast.tip')}
          aria-label={t('cast.tip')}
          class:active={openMenu === 'cast'}
          onclick={() => toggleMenu('cast')}
        >
          <svg viewBox="0 0 24 24"><path fill="currentColor" d="M21 3H3c-1.1 0-2 .9-2 2v3h2V5h18v14h-7v2h7c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2zM1 18v3h3c0-1.66-1.34-3-3-3zm0-4v2c2.76 0 5 2.24 5 5h2c0-3.87-3.13-7-7-7zm0-4v2c4.97 0 9 4.03 9 9h2c0-6.08-4.93-11-11-11z"/></svg>
        </button>
      {/if}
      <button
        class="fs"
        data-tip={withKey(t(fullscreen ? 'bar.fullscreen_exit' : 'bar.fullscreen'), 'fullscreen')}
        aria-label={t(fullscreen ? 'bar.fullscreen_exit' : 'bar.fullscreen')}
        onclick={toggleFullscreen}
      >
        {#if fullscreen}
          <svg viewBox="0 0 24 24"><path fill="currentColor" d="M5 16h3v3h2v-5H5v2zm3-8H5v2h5V5H8v3zm6 11h2v-3h3v-2h-5v5zm2-11V5h-2v5h5V8h-3z"/></svg>
        {:else}
          <svg viewBox="0 0 24 24"><path fill="currentColor" d="M7 14H5v5h5v-2H7v-3zm-2-4h2V7h3V5H5v5zm12 7h-3v2h5v-5h-2v3zM14 5v2h3v3h2V5h-5z"/></svg>
        {/if}
      </button>
      </div>
    </div>
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

  /* Fill for the time while mpv is not painting yet. Black, not the start
     screen tint: launching with a file (Open with) must bring the window up
     already looking like the player — it used to flash #101016 and snap to
     black mid-appearance. A separate class rather than no-video: that one
     also removes the title-bar gradient, which is not wanted here. */
  .player.backdrop {
    background: #000;
  }

  /* Until a file is open the window is opaque: neither the desktop before mpv
     initialises nor the startup stages are visible. After .backdrop on
     purpose: the start screen keeps its tint while mpv is still warming up. */
  .player.no-video {
    background: #101016;
  }

  .player.nocursor {
    cursor: none;
  }

  .overlay {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: #e8e8ec;
  }

  /* 484 = the 420 this was written as plus its own 64px of padding. An outer
     figure now that boxes are border boxes, and the same width on screen. */
  .panel {
    text-align: center;
    max-width: 484px;
    padding: 32px;
  }

  .panel h1 {
    font-size: 42px;
    font-weight: 600;
    letter-spacing: 0.06em;
    margin: 0 0 8px;
  }

  .panel p {
    color: #9a9aa5;
    margin: 8px 0 20px;
  }

  .panel .error {
    color: #ff8a8a;
    word-break: break-word;
  }

  /* Form controls do not inherit the font by default (UA styles), which is why
     buttons were drawn in the system font instead of Rubik */
  button,
  input {
    font-family: inherit;
  }

  button.primary {
    background: #6366f1;
    color: #ffffff;
    border: none;
    border-radius: 8px;
    padding: 10px 22px;
    font-size: 15px;
    /* Rubik 600 clumps at small sizes — 500 plus a little tracking is softer */
    font-weight: 400;
    letter-spacing: 0.01em;
    cursor: pointer;
  }

  /* The secondary action next to .primary. A real button rather than a link:
     a link promises navigation, and these open a dialog or download a binary —
     the same reasoning that makes the destructive actions real buttons. */
  button.btn-outline {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    background: transparent;
    border: 1px solid rgba(255, 255, 255, 0.18);
    border-radius: 8px;
    padding: 9px 20px;
    color: #d6d6de;
    font-size: 15px;
    letter-spacing: 0.01em;
    cursor: pointer;
  }

  button.btn-outline:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.08);
    border-color: rgba(255, 255, 255, 0.3);
    color: #e8e8ec;
  }

  /* A button that starts with an icon needs less padding in front of it than
     one that starts with a word. Measured: the glyph itself is symmetric in its
     box (1.25px of clearance each side), so the gap is not arithmetic — a
     sparse two-stroke mark simply reads as beginning further right than dense
     text does, and the leading space then looks larger than the trailing one.
     Scoped by `:has`, because the same class is used without an icon in the
     link dialog, where trimming would introduce the very asymmetry it fixes. */
  button.btn-outline:has(svg) {
    padding-left: 16px;
  }

  button.btn-outline svg {
    width: 14px;
    height: 14px;
    /* Slightly below the label's own weight: the glyph is decoration, the
       words are the button. */
    opacity: 0.8;
  }

  button.btn-outline:hover:not(:disabled) svg {
    opacity: 1;
  }

  /* Fills left to right as the bytes arrive. Indigo, because this reports
     progress of something the viewer asked for and is waiting on — the same
     reason .card-progress is indigo over a video frame. */
  button.btn-outline.progressing {
    background-image: linear-gradient(rgba(99, 102, 241, 0.35), rgba(99, 102, 241, 0.35));
    background-repeat: no-repeat;
    background-size: var(--pct, 0%) 100%;
    transition: background-size 0.25s linear;
  }

  button.btn-outline:disabled {
    opacity: 0.5;
    cursor: default;
  }

  button.primary:hover {
    background: #818cf8;
  }

  .clickthrough-bg {
    background: rgba(0, 0, 0, 0.35);
  }

  button.replay {
    background: rgba(20, 20, 28, 0.75);
    border: none;
    border-radius: 50%;
    width: 96px;
    height: 96px;
    display: grid;
    place-items: center;
    color: #e8e8ec;
    cursor: pointer;
  }

  button.replay:hover {
    background: rgba(40, 40, 54, 0.85);
  }

  .topbar {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    /* Three layers: brand on the left, video title centred, window buttons in
       the corner. The height is exactly the 48px of content, and that is the
       HIT AREA: the gradient tail lives in ::before, which catches no events.
       The bar used to occupy all 76px and swallowed clicks in the bottom 28px
       (nothing but a faint shadow there) — to the user that looked like
       clicking the video and the player ignoring it. The children
       (.brand/.titlecenter/.chrome) are absolutely positioned within 48px
       anyway, so they do not depend on the bar's height. */
    height: 48px;
    transition: opacity 0.25s ease;
  }

  .topbar::before {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    height: 76px;
    background: linear-gradient(to bottom, rgba(0, 0, 0, 0.6), transparent);
    pointer-events: none;
  }

  .brand {
    position: absolute;
    top: 0;
    left: 18px;
    height: 48px;
    display: flex;
    align-items: center;
    gap: 8px;
    pointer-events: none;
  }

  /* macOS: the window buttons are the system ones on the left, so the brand
     moves to the right corner and flips: caption first, then the mark. */
  .brand.brand-right {
    position: static;
    height: auto;
    flex-direction: row-reverse;
    padding-right: 18px;
  }

  .brand .logo {
    width: 18px;
    height: 18px;
    color: #818cf8;
    filter: var(--ui-shadow-drop);
  }

  .brand .appname {
    /* The same colour as the video title: the muted #b9b9c3 read as
       semi-transparent and disappeared on a bright frame. */
    color: #e8e8ec;
    font-size: 13px;
    font-weight: 500;
    letter-spacing: 0.02em;
    text-shadow: var(--ui-shadow);
  }

  .titlecenter {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: 48px;
    display: flex;
    align-items: center;
    justify-content: center;
    pointer-events: none;
  }

  .topbar.hidden {
    opacity: 0;
    pointer-events: none;
  }

  /* **Right**, and that is not a preference: `.osd` — the popup every volume
     change, seek and speed change raises — is `top: 60px; left: 18px`, which is
     within two pixels of where this sat. A status readout that the next volume
     nudge covers completely is worse than no readout, because it is there right
     up until the moment you look. Nothing else occupies this corner: the window
     buttons and the macOS logo end at the bar's 48px, and the skip button is
     bottom-right.

     Never interactive — it reports, it does not offer anything, and a readout
     that eats clicks over video is the gotcha the top bar's own gradient
     already taught once. */
  .torchip {
    position: absolute;
    top: 58px;
    right: 18px;
    display: flex;
    flex-direction: column;
    gap: 5px;
    /* Outer, like every size here: 148 of content plus 22 of padding and 2 of
       border. Wide enough that the rate and the peer count changing width does
       not make the chip breathe. */
    min-width: 172px;
    padding: 7px 11px 8px;
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 9px;
    background: rgba(16, 16, 22, 0.82);
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.4);
    color: #d6d6de;
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
    pointer-events: none;
    transition: opacity 0.25s ease;
  }

  .torchip.hidden {
    opacity: 0;
  }

  /* Stalled is the one state that stays up through idle, so it says so in more
     than words: the accent is what the rest of the UI uses for "this is the
     thing to look at". */
  .torchip.waiting {
    color: #e8e8ec;
    border-color: rgba(129, 140, 248, 0.45);
  }

  .torchip-line {
    display: flex;
    align-items: center;
    gap: 7px;
    white-space: nowrap;
  }

  .torchip-icon {
    /* An SVG has no border, so the box-sizing reset is a no-op here — but
       `flex: none` still matters, or a long label squashes the arrow into an
       oval. */
    flex: none;
    width: 12px;
    height: 12px;
    color: #8f8f9c;
  }

  .torchip.waiting .torchip-icon {
    color: #818cf8;
    /* The arrow stops meaning "downloading" when nothing is arriving. */
    animation: torchip-pulse 1.4s ease-in-out infinite;
  }

  @keyframes torchip-pulse {
    0%, 100% { opacity: 0.35; }
    50% { opacity: 1; }
  }

  .torchip-bar {
    height: 2px;
    border-radius: 1px;
    background: rgba(255, 255, 255, 0.14);
    overflow: hidden;
  }

  .torchip-bar span {
    display: block;
    height: 100%;
    border-radius: 1px;
    /* White, not the accent: this reports a value rather than offering one —
       the same reason `.seekfill` is white. */
    background: rgba(255, 255, 255, 0.75);
    transition: width 0.3s ease;
  }

  /* With no video the window background is dark anyway — the bar needs no gradient */
  /* With no video the gradient is pointless — there is nothing to darken. It
     has to be targeted through ::before now: .topbar itself has no background. */
  .player.no-video .topbar::before {
    background: none;
  }

  .topbar .title {
    transition:
      opacity 0.25s ease,
      transform 0.25s ease;
  }

  /* Slide: "next" sends the old title left and brings the new one in from the
     right; "prev" is mirrored */
  .topbar .title.slide-out-next {
    opacity: 0;
    transform: translateX(-28px);
  }

  .topbar .title.slide-out-prev {
    opacity: 0;
    transform: translateX(28px);
  }

  .topbar .title.slide-prep-next {
    opacity: 0;
    transform: translateX(28px);
    transition: none;
  }

  .topbar .title.slide-prep-prev {
    opacity: 0;
    transform: translateX(-28px);
    transition: none;
  }

  .topbar .title {
    /* Bounded by what is actually free: the window minus the wider of the two
       side clusters on BOTH sides (the title is centred in the window, so the
       reservation has to be symmetric), minus a gap so it never arrives flush
       against the logo. `--bar-side` is measured in JS and already carries that
       gap — see the comment there for why a constant does not survive an update
       button or a translation. A calc() that goes negative is clamped to 0, so
       a window too narrow for both clusters hides the title rather than
       dropping the bound and letting it overlap them. */
    max-width: min(46vw, calc(100vw - 2 * var(--bar-side, 0px)));
    color: #e8e8ec;
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    /* overflow: hidden is needed for the ellipsis, but it also clipped the
       shadow at the ends of the line. Vertical padding gives it room inside the
       block; the element is centred vertically by flex, so the text does not
       shift. */
    padding: 8px 0;
    text-shadow: var(--ui-shadow);
  }

  .chrome {
    position: absolute;
    top: 0;
    right: 0;
    height: 48px;
    display: flex;
    align-items: center;
  }

  /* Full-height title-bar buttons (Spotify style) */
  .winbtns {
    display: flex;
    height: 100%;
  }

  /* Fills as the download lands. The percentage is in the label already; the
     fill is what makes it readable at a glance on a bar the eye passes over. */
  .updbtn.progressing {
    background-image: linear-gradient(rgba(255, 255, 255, 0.28), rgba(255, 255, 255, 0.28));
    background-repeat: no-repeat;
    background-size: var(--pct, 0%) 100%;
    transition: background-size 0.25s linear;
  }

  .updbtn {
    align-self: center;
    margin-right: 8px;
    background: #6366f1;
    color: #ffffff;
    border: none;
    border-radius: 999px;
    padding: 4px 12px;
    font-size: 12px;
    font-weight: 500;
    letter-spacing: 0.02em;
    cursor: pointer;
    white-space: nowrap;
  }

  .updbtn:hover:not(:disabled) {
    background: #818cf8;
  }

  .updbtn:disabled {
    opacity: 0.75;
    cursor: default;
  }

  .winbtn {
    /* a square gives equal gaps around the icon on every side */
    width: 48px;
    height: 100%;
    display: grid;
    place-items: center;
    background: transparent;
    border: none;
    color: #d6d6de;
    padding: 0;
  }

  .winbtn svg {
    width: 10px;
    height: 10px;
    /* The same shadow as the logo and the titles: the glyphs sit directly over
       the frame, and without it the light grey lines are lost on bright video.
       drop-shadow rather than text-shadow — the shadow must follow the shape's
       outline, not the element's rectangle. */
    filter: var(--ui-shadow-drop);
  }

  /* On hover the button gets a background of its own, which provides the
     contrast — a shadow on top of it only muddies the glyph (especially on the
     red close button). */
  .winbtn:hover svg {
    filter: none;
  }

  /* The enter/exit fullscreen glyphs fill ~60% of their viewBox — compensate
     with size so they match their neighbours optically */
  .winbtn.fsx svg {
    width: 17px;
    height: 17px;
  }

  .winbtn:hover {
    background: rgba(255, 255, 255, 0.12);
    color: #fff;
  }

  .winbtn.close:hover {
    background: #c42b1c;
    color: #fff;
  }

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

  /* Frosted panel, IINA style: no border, soft radius, deep shadow. A real
     backdrop-filter is impossible here — the video is NOT in the DOM but a
     native layer under the transparent webview, so the web content has nothing
     to blur. The "frost" is therefore made of background density and shadow. */
  /* IINA-style panel: top left under the title bar, no border, dense backing.
     A real backdrop-filter is impossible here — the video is NOT in the DOM but
     a native layer under the transparent webview, so the web content has
     nothing to blur. The "frost" is made of background density and shadow. */
  .osd {
    position: absolute;
    top: 60px;
    left: 18px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    /* Outer: 168 of content plus 32 of padding and 2 of border. Wide enough
       that "70%" and "1.25×" raise a popup of the same size. */
    min-width: 202px;
    /* The same surface as the context menu, settings and tooltips — floating
       layers should come from one scale, not one each. */
    background: rgba(16, 16, 22, 0.96);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 12px;
    padding: 12px 16px;
    pointer-events: none;
    box-shadow:
      0 10px 34px rgba(0, 0, 0, 0.5),
      inset 0 1px 0 rgba(255, 255, 255, 0.06);
    animation: osd-in 0.15s ease;
    z-index: 30;
  }

  /* The bar must not press against the bottom edge */
  .osd.with-bar {
    padding-bottom: 16px;
  }

  .osd-bar {
    height: 4px;
    border-radius: 2px;
    background: rgba(255, 255, 255, 0.18);
    overflow: hidden;
  }

  /* White, like the seekbar fill. Indigo here would mean "selected / on /
     primary action" — that is all it means everywhere else in this UI — while
     the OSD bar selects nothing and is not interactive at all: it merely
     reports a value. It came out inverted: the thing you drag (the seekbar) was
     white, and a passive readout wore the accent. */
  .osd-bar-fill {
    height: 100%;
    border-radius: 2px;
    background: #fff;
  }

  /* Second line: for "resuming" this is the position, smaller and dimmer */
  .osd-sub {
    color: #b9b9c3;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .osd-text {
    color: #e8e8ec;
    font-size: 15px;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  @keyframes osd-in {
    from {
      opacity: 0;
      transform: translateX(-50%) scale(0.95);
    }
  }

  .ctxmenu {
    position: fixed;
    z-index: 60;
    /* One minimum for every menu in the app; content widens them from there.
       Wide enough for the five-option speed row, which at 230px gave each
       option 37px and let "1.25×" fill it edge to edge. */
    min-width: 256px;
    /* Taller than the window in the mini player, and on a short screen with a
       long chapter list. The cap comes from the placement (floating.ts), which
       computes it from the room on screen and therefore means the whole box —
       the global border-box reset is what makes the `max-height` it hands over
       mean the same thing. Before the reset this menu rendered 14px taller than
       it was told (6+6 padding, 1+1 border) and ate the 8px margin it was
       meant to keep from the window edge.
       `overflow-x` must be set too: leaving it `visible` next to a scrolling
       axis computes it to `auto`, and a row a fraction of a pixel too wide then
       raises a horizontal bar that eats a row's worth of height. Nothing here
       is meant to scroll sideways. */
    overflow-y: auto;
    overflow-x: hidden;
    background: rgba(16, 16, 22, 0.96);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 10px;
    padding: 6px;
  }

  .menu-sep {
    height: 1px;
    margin: 5px 8px;
    background: rgba(255, 255, 255, 0.09);
  }

  /* Nested menu. Opens to the right; with no room there it flips left (the
     browser clamps to the window edge itself through right: 100% on .flip). */
  .submenu-wrap {
    position: relative;
  }

  /* Mantine style: the item is a flex row, the chevron pushed to the right by
     space-between and centred on the line. `margin-left: auto` would not work
     here at all — .menu-item is display: block (the neighbouring .hint gets by
     with float, but float gives an icon a crooked baseline).
     The selector must use two classes: the rule `.menu-item { display: block }`
     is declared LOWER in the file, and at equal specificity it would win — flex
     silently did not apply, the chevron stayed right after the text and sat on
     the baseline, i.e. above the centre of the line. */
  .menu-item.back {
    display: flex;
    align-items: center;
    gap: 8px;
    color: #a8a8b3;
  }

  .menu-item.back .caret {
    flex: none;
  }

  .menu-item.submenu-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .menu-item.submenu-head .caret {
    flex: none;
    color: #77777f;
    transition: color 0.12s ease;
  }

  .menu-item.submenu-head:hover .caret,
  .menu-item.submenu-head.open .caret {
    color: #d6d6de;
  }

  .menu-item.submenu-head.open {
    background: rgba(255, 255, 255, 0.09);
  }

  /* Drill-down: the submenu stands in for the parent rather than beside it, so
     the parent gets out of the way. `visibility` and not `display`, because the
     placement still measures it — and it works only because the submenu is no
     longer one of its descendants. */
  .ctxmenu.masked {
    visibility: hidden;
  }

  .ctxmenu.submenu {
    /* Fixed, not absolute: the parent scrolls, and a scroll container clips
       what sticks out of it. Placed in viewport coordinates by placeSubmenu. */
    position: fixed;
    /* left/top come from JS, computed from the actual size (placeSubmenu) */
    min-width: 256px;
  }

  /* An invisible bridge across the gap between the item and the submenu:
     extends the hover area by 10px on both sides, so ordinary cursor movement
     never leaves the submenu and the close delay is not even needed. It goes
     under the block itself (z-index: -1) so it does not intercept clicks on the
     items. Works both ways — the submenu can also open to the left when there
     is no room on the right. */
  .ctxmenu.submenu::before {
    content: '';
    position: absolute;
    inset: 0 -10px;
    z-index: -1;
  }

  .menu-item .hint {
    float: right;
    color: #77777f;
    font-size: 12px;
  }

  .menu-item:disabled {
    color: #6a6a74;
    cursor: default;
  }

  .menu-item:disabled:hover {
    background: transparent;
  }

  .menu-item.active {
    color: #818cf8;
  }

  /* segmented control */
  .speedrow {
    display: flex;
    gap: 2px;
    margin: 2px 8px 6px;
    padding: 3px;
    background: rgba(255, 255, 255, 0.07);
    border-radius: 8px;
  }

  .speedopt {
    flex: 1;
    background: transparent;
    border: none;
    border-radius: 6px;
    color: #d6d6de;
    font-size: 12px;
    padding: 6px 0;
    cursor: pointer;
  }

  /* Delay stepper: the same pill row as the speed presets, because it is the
     same job — a compact group of small actions. The value between them is a
     readout, not a control, so it stays plain text rather than a third pill. */
  .delayrow {
    display: flex;
    align-items: center;
    gap: 2px;
    margin: 2px 8px 6px;
    padding: 3px;
    background: rgba(255, 255, 255, 0.07);
    border-radius: 8px;
  }

  .delayrow .speedopt {
    flex: 0 0 34px;
    font-size: 15px;
    line-height: 1;
  }

  .delayval {
    flex: 1;
    text-align: center;
    color: #e8e8ec;
    font-size: 12px;
    /* Tabular figures: the value changes under the cursor, and proportional
       digits make the whole row twitch sideways with every nudge. */
    font-variant-numeric: tabular-nums;
  }

  .speedopt:hover {
    background: rgba(255, 255, 255, 0.12);
  }

  .speedopt.sel {
    color: #ffffff;
    background: #6366f1;
    font-weight: 600;
  }

  .settings-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    display: grid;
    place-items: center;
    /* The dialog's inset from the window, kept here rather than as a magic
       number inside the dialog's own max-height: the padding bounds the grid
       area, so `max-height: 100%` on the child follows it automatically. It
       needs to be generous — the settings list grew tall enough to reach the
       window edges at an ordinary player height, and a panel flush against the
       frame reads as a menu, not a dialog. The top figure clears the title bar
       (48px of content) and the bottom matches it, so centring stays centred.
       No `safe` needed unlike the start screen: this child scrolls internally,
       so it never overflows the grid area in the first place. */
    padding: 56px 24px;
    z-index: 70;
  }

  /* The tab row is what sets this number, and the Russian labels are the
     binding case. Measured against the built stylesheet rather than a
     hand-written copy of it (see the queue-row note): the seven of them render
     to **510px** at the real 13px font and 13px gap, English to 408.

     598 is that 560 plus the sheet's own 36px of padding and 2px of border,
     because this is an outer width now — the number changed, the sheet did
     not. Content is still 560, so the tab row still has 50px of slack, or 40
     once this sheet is tall enough to grow its 10px scrollbar.

     The old 428 was already 13px short of *six* tabs, so "Клавиши" was being
     clipped by a row whose overflow scroll is invisible by design
     (`scrollbar-width: none`). The seventh tab did not create that; it made it
     impossible to miss. */
  .settings {
    width: min(598px, 100%);
    /* Against the backdrop's padded grid area, and now honestly: as a
       content-box height this let the sheet stand 32px taller than the area it
       was centred in, so at full height it reached 39px from the top of the
       window and did not in fact clear the 48px title bar the backdrop's 56px
       of padding was chosen to clear. */
    max-height: 100%;
    overflow-y: auto;
    background: rgba(16, 16, 22, 0.97);
    border: 1px solid rgba(255, 255, 255, 0.09);
    border-radius: 14px;
    padding: 16px 18px 14px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.5);
  }

  /* Narrower than the settings sheet: it holds one field. Outer, like the
     sheet's own width — 520 of content plus the same 38 of padding and
     border. */
  .link-dialog {
    width: min(558px, 100%);
  }

  .link-input {
    width: 100%;
    margin: 4px 0 8px;
    padding: 10px 12px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.06);
    color: #e8e8ec;
    font-size: 13px;
  }

  .link-input:focus {
    outline: none;
    border-color: #6366f1;
  }

  /* The two ways in, side by side: the file picker is the primary action and
     the link sits next to it as a plain one. */
  .start-actions {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 16px;
  }

  /* Shown only while the field is empty: once something is typed the list is
     about the past and the field is about the present. */
  .link-recent {
    display: flex;
    flex-direction: column;
    margin-top: 8px;
    max-height: 168px;
    overflow-y: auto;
    /* Part of scrolling vertically, not an extra: setting one axis computes the
       other from `visible` to `auto`, and a row a fraction of a pixel too wide
       would otherwise raise a horizontal bar and eat a row's height. */
    overflow-x: hidden;
  }

  .link-recent-row {
    display: flex;
    align-items: center;
    /* The dialog is a fixed width and the name is arbitrary. Without this the
       row grows to the name instead of letting it ellipsise — `min-width: auto`
       is the default on a flex item, and it has to be cleared on every ancestor
       between the label and the fixed box, not only on the label. */
    min-width: 0;
    border-radius: 6px;
  }

  .link-recent-row:hover {
    background: rgba(255, 255, 255, 0.08);
  }

  .link-recent-item {
    flex: 1;
    min-width: 0;
    text-align: left;
    padding: 7px 8px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: #b9b9c3;
    font-size: 12.5px;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .link-recent-row:hover .link-recent-item {
    color: #e8e8ec;
  }

  /* Always on screen, never faded in: taking a 1.4px stroke off full opacity
     makes the engine re-rasterise it and the glyph visibly twitches in
     WKWebView. The three strengths are colour, exactly as on `.torrow-forget`. */
  .link-recent-forget {
    flex: none;
    width: 24px;
    height: 24px;
    margin-right: 4px;
    display: grid;
    place-items: center;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: transparent;
    color: rgba(255, 255, 255, 0.22);
    cursor: pointer;
    transition: color 0.12s ease;
  }

  .link-recent-row:hover .link-recent-forget {
    color: rgba(255, 255, 255, 0.5);
  }

  /* Written to win rather than left to source order: the rule above is a
     descendant selector and would otherwise out-weigh a bare
     `.link-recent-forget:hover`, so the cross would never reach full strength
     under the pointer — and it is only ever hovered while the row is. */
  .link-recent-row:hover .link-recent-forget:hover {
    background: rgba(255, 255, 255, 0.1);
    color: #f0f0f4;
  }

  .link-recent-forget svg {
    width: 10px;
    height: 10px;
  }

  .link-actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 14px;
    margin-top: 12px;
  }

  /* Pushed to the far left, so the row reads as "the other way in" on one side
     and "do the thing" on the other. `margin-right: auto` rather than
     `space-between` on the parent, which would also spread the yt-dlp button
     away from the primary one it belongs beside. */
  .link-torrent {
    margin-right: auto;
  }

  .link-error {
    margin-top: 2px;
    color: #f0a0a0;
    font-size: 12px;
    line-height: 1.45;
  }

  .link-progress {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 2px;
    color: #e8e8ec;
    font-size: 12.5px;
  }

  /* The hint's own 5px glues it to the line above, which here is a live status
     rather than a label — the same problem the language pills had, and fixed the
     same way: a sibling rule, so it holds wherever the pair occurs. */
  .link-progress + .setting-hint {
    margin-top: 11px;
  }

  .torrent-files {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-top: 10px;
    /* Bounded rather than free: a season is dozens of rows and a dialog that
       grows past the window has nowhere to put its own close button. */
    max-height: min(360px, 50vh);
    overflow-y: auto;
    overflow-x: hidden;
  }

  /* Two classes of equal weight decide `display`, and which one wins is source
     order — the queue rows lost that fight once already (see CLAUDE.md). Written
     to win rather than left to chance. */
  .menu-item.torrent-file {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
  }

  .torrent-file-name {
    /* Both halves are needed: a flex item defaults to `min-width: auto` and so
       never shrinks below its content, and the ellipsis then never fires. */
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .torrent-file-size {
    flex: none;
    color: #8a8a95;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }

  /* A file with a position is the one being looked for, so it reads brighter
     than the rest rather than merely carrying extra text. */
  .torrent-file.started .torrent-file-name {
    color: #ffffff;
  }

  .torrent-file-left {
    flex: none;
    color: #818cf8;
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
  }

  /* No progress bar here, and that was tried: a full-width rule under the name
     reads as an underline rather than a value — at 91% it is indistinguishable
     from a border, and a list of them turns the panel into stripes. The card
     grid needs one because a poster leaves no room for words; here there are
     words, and "осталось 4:12" says the same thing without competing with the
     name it sits under. */

  /* Wider than the link dialog: a row here carries a release name, which is
     the one field that must not be truncated to uselessness — it is how you
     tell one rip from another. */
  .subs-dialog {
    width: min(680px, 100%);
  }

  /* The row owns the vertical spacing, not the field inside it: `.link-input`
     carries `margin: 4px 0 8px` for the link dialog, where it is the only
     thing in its row, and an asymmetric margin on a centred flex item tilts
     the pair — the field sat 2px high and the button looked dropped. */
  .subs-query {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 4px 0 8px;
  }

  .subs-query .link-input {
    flex: 1;
    min-width: 0;
    margin: 0;
  }

  .subs-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-top: 10px;
    /* Bounded so the dialog cannot outgrow a small window; the rows scroll. */
    max-height: min(46vh, 340px);
    overflow-y: auto;
    overflow-x: hidden;
  }

  .subs-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 8px 8px 10px;
    border-radius: 8px;
  }

  .subs-row:hover {
    background: rgba(255, 255, 255, 0.06);
  }

  /* min-width: 0 on the column AND on the label below it, or the flex row
     grows to the longest release name instead of ellipsising it. */
  .subs-main {
    flex: 1;
    min-width: 0;
  }

  .subs-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #e8e8ec;
    font-size: 13px;
  }

  .subs-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 3px;
    color: #9a9aa5;
    font-size: 11.5px;
    white-space: nowrap;
  }

  .subs-lang {
    color: #b9b9c3;
    font-weight: 600;
    letter-spacing: 0.04em;
  }

  .subs-movie {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* The one answer available to "will this fit": OpenSubtitles carries no
     duration for a subtitle, and a frame-rate mismatch is the failure that
     drifts further out the longer you watch. */
  .subs-fps {
    flex: none;
  }

  .subs-fps.warn {
    color: #f0a0a0;
  }

  .subs-badges {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 5px;
  }

  .subs-badge {
    padding: 1px 7px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    border-radius: 999px;
    color: #9a9aa5;
    font-size: 10.5px;
  }

  /* Indigo means one thing in this UI — selected / on / primary — and "this is
     the one that matches your file" is exactly that. */
  .subs-badge.match {
    border-color: transparent;
    background: #6366f1;
    color: #fff;
  }

  .subs-badge.warn {
    border-color: rgba(240, 160, 160, 0.4);
    color: #f0a0a0;
  }

  /* A control inside a row, not a dialog action: `.btn-outline`'s own 9/20
     padding and 15px type are sized for the footer of a dialog, where there is
     one of them. In a list there is one per row, and at that size they dominate
     what they are attached to. */
  button.subs-sm {
    padding: 5px 13px;
    border-radius: 7px;
    font-size: 12.5px;
  }

  .subs-get {
    flex: none;
  }

  .subs-account {
    margin-top: 12px;
    padding-top: 10px;
    border-top: 1px solid rgba(255, 255, 255, 0.08);
  }

  .subs-account-line {
    display: flex;
    align-items: center;
    gap: 10px;
    color: #b9b9c3;
    font-size: 12.5px;
  }

  /* The hint gives up its width first: the button must not be pushed off. */
  .subs-account-hint {
    flex: 1;
    min-width: 0;
  }

  .subs-account-quota {
    flex: 1;
    min-width: 0;
    color: #9a9aa5;
    text-align: right;
  }

  .subs-auth {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .subs-auth .link-input {
    margin: 0;
  }

  .subs-remember {
    display: flex;
    align-items: center;
    gap: 10px;
    color: #d6d6de;
    font-size: 13px;
  }

  .info-section {
    margin: 16px 0 6px;
    color: #9a9aa5;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .info-section:first-of-type {
    margin-top: 4px;
  }

  .info-row {
    display: flex;
    gap: 16px;
    align-items: baseline;
    padding: 3px 0;
    font-size: 12.5px;
  }

  .info-label {
    flex: 0 0 38%;
    color: #8f8f9c;
  }

  /* Codec names and file names are long and arbitrary; the dialog has a fixed
     width, so this is the column that has to give. */
  .info-value {
    flex: 1;
    min-width: 0;
    color: #e0e0e6;
    overflow-wrap: anywhere;
  }

  .settings-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 12px;
    color: #e8e8ec;
    font-size: 15px;
    font-weight: 500;
  }

  .settings-close {
    width: 30px;
    height: 30px;
    display: grid;
    place-items: center;
    background: transparent;
    border: none;
    border-radius: 7px;
    color: #d6d6de;
    cursor: pointer;
  }

  .settings-close svg {
    width: 9px;
    height: 9px;
  }

  .settings-close:hover {
    background: rgba(255, 255, 255, 0.1);
  }

  .setting {
    margin-bottom: 12px;
  }

  /* ---- Dialog tabs ----
     Deliberately underlines rather than pills: pills are already taken by the
     segmented value switch (.segmented), and giving navigation and a setting
     the same look reads as the same control. */
  .tabs {
    display: flex;
    /* Seven sections rather than two: the Russian labels need every pixel here,
       and the row must not wrap — a second line of tabs reads as two rows of
       something else. The scroll is a last resort and, with no scrollbar to
       show for it, an *invisible* one: a tab past the edge is simply gone. So
       adding a section means re-measuring `.settings`' width, which is sized
       from this row for exactly that reason — the six-tab row had quietly been
       overflowing it. */
    flex-wrap: nowrap;
    overflow-x: auto;
    scrollbar-width: none;
    gap: 13px;
    margin: 4px 0 18px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.09);
  }

  .tab {
    position: relative;
    flex: 0 0 auto;
    padding: 6px 2px 10px;
    white-space: nowrap;
    border: none;
    background: transparent;
    color: #8f8f9c;
    font-size: 13px;
    cursor: pointer;
    transition: color 0.12s ease;
  }

  .tab:hover {
    color: #d6d6de;
  }

  .tab.sel {
    color: #e8e8ec;
  }

  .tab.sel::after {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    bottom: -1px;
    height: 2px;
    border-radius: 2px;
    background: #6366f1;
  }

  /* ---- Toggle row ---- */
  .row-toggle {
    display: flex;
    align-items: flex-start;
    gap: 16px;
  }

  .row-text {
    flex: 1;
    min-width: 0;
  }

  /* Inside the row the label is already offset by the toggle itself */
  .row-text .setting-label {
    margin-bottom: 3px;
  }

  .row-text .setting-hint {
    margin-top: 0;
  }

  .switch {
    flex: none;
    position: relative;
    width: 38px;
    height: 22px;
    margin-top: 1px;
    padding: 0;
    border: none;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.16);
    cursor: pointer;
    transition: background 0.15s ease;
  }

  .switch.on {
    background: #6366f1;
  }

  .switch:hover {
    background: rgba(255, 255, 255, 0.24);
  }

  .switch.on:hover {
    background: #818cf8;
  }

  .switch-knob {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    transition: transform 0.15s ease;
  }

  .switch.on .switch-knob {
    transform: translateX(16px);
  }

  /* A section that has lost its meaning under the current settings: visible
     but dimmed */
  .setting.muted {
    opacity: 0.45;
  }

  /* ---- Content languages (ROADMAP 25) ---- */

  .langs {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin-top: 8px;
  }

  .langs-auto {
    padding: 4px 2px;
    color: #6a6a74;
    font-size: 12.5px;
  }

  /* The chip is the container and both controls sit inside it, because a button
     inside a button is invalid markup and the inner one stops receiving clicks.
     Its own background carries the state; neither child paints one. */
  .lang-chip {
    display: inline-flex;
    align-items: center;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.1);
    transition: background 0.12s ease;
  }

  /* Indigo means one thing here as everywhere else — this is the selected one,
     the language mpv reaches for first. */
  .lang-chip.first {
    background: #6366f1;
  }

  .lang-name {
    padding: 5px 4px 5px 11px;
    border: none;
    background: transparent;
    color: #e0e0e6;
    font-size: 12.5px;
    cursor: pointer;
  }

  .lang-chip.first .lang-name {
    color: #fff;
  }

  /* The first chip is already what clicking it would make it, so it is inert —
     and says so by not changing under the pointer rather than by being dimmed,
     which would read as unavailable. */
  .lang-name:disabled {
    cursor: default;
  }

  .lang-drop {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    margin-right: 4px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: transparent;
    /* Colour, never opacity: this is a 1.4px stroke that is always on screen,
       and taking it off full opacity makes WKWebView re-rasterise and wobble it
       (the same finding as the torrent list's delete cross). */
    color: rgba(255, 255, 255, 0.4);
    cursor: pointer;
    transition: color 0.12s ease, background 0.12s ease;
  }

  .lang-chip.first .lang-drop {
    color: rgba(255, 255, 255, 0.62);
  }

  /* Written to win, not left to source order — measured in the built bundle,
     `.lang-chip.first .lang-drop` weighs (0,4,0) against a bare
     `.lang-drop:hover`'s (0,3,0), so on the preferred chip the cross would have
     stayed at its resting colour under the pointer. The same arithmetic the
     link-history cross ran into. */
  .lang-chip .lang-drop:hover {
    background: rgba(0, 0, 0, 0.25);
    color: #fff;
  }

  .lang-drop svg {
    width: 9px;
    height: 9px;
  }

  /* 26 outer, border included — and it always was, because a <button> is
     border-box in both engines' own stylesheets, reset or no reset. */
  .lang-add {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    padding: 0;
    border: 1px dashed rgba(255, 255, 255, 0.18);
    border-radius: 50%;
    background: transparent;
    color: #b9b9c3;
    cursor: pointer;
    transition: border-color 0.12s ease, color 0.12s ease, background 0.12s ease;
  }

  .lang-add:hover,
  .lang-add.open {
    border-color: #818cf8;
    background: rgba(129, 140, 248, 0.1);
    color: #e8e8ec;
  }

  .lang-add svg {
    width: 13px;
    height: 13px;
  }

  .lang-picker {
    margin-top: 8px;
    border: 1px solid rgba(255, 255, 255, 0.09);
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.03);
    overflow: hidden;
  }

  .lang-search {
    display: block;
    width: 100%;
    padding: 8px 10px;
    border: none;
    border-bottom: 1px solid rgba(255, 255, 255, 0.07);
    background: transparent;
    color: #e8e8ec;
    font-size: 12.5px;
  }

  .lang-search:focus {
    outline: none;
  }

  .lang-list {
    max-height: 176px;
    overflow-y: auto;
    /* Part of scrolling vertically rather than an extra: setting one axis
       computes the other from `visible` to `auto`. */
    overflow-x: hidden;
  }

  /* `width: 100%` is the row filling the list rather than overhanging it by its
     own 20px of padding, and that held before the global reset as well: this is
     a <button>, and both engines' stylesheets already make one border-box. The
     reset changes nothing here — worth knowing before "fixing" a row that was
     never broken. */
  .lang-option {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
    width: 100%;
    padding: 6px 10px;
    border: none;
    background: transparent;
    color: #d0d0d8;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
  }

  .lang-option:hover {
    background: rgba(255, 255, 255, 0.08);
    color: #fff;
  }

  .lang-option-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* The code is here for the person who knows their file is tagged `jpn` and
     cannot read 日本語 — the same reason the search matches on codes. */
  .lang-option-code {
    flex: none;
    color: #6a6a74;
    font-size: 11px;
  }

  /* ---- Excluded folder list ---- */
  .folders {
    margin-top: 10px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .folder-row {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 7px 8px 7px 10px;
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.05);
  }

  .folder-ico {
    flex: none;
    width: 15px;
    height: 15px;
    color: #8f8f9c;
  }

  .folder-path {
    flex: 1;
    min-width: 0;
    /* Truncate the start, not the tail: for paths the meaningful part is the
       last components. The text direction is isolated by <bdi> in the markup. */
    direction: rtl;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #d6d6de;
    font-size: 12px;
  }

  .folder-remove {
    flex: none;
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: #77777f;
    cursor: pointer;
    transition: background 0.12s ease, color 0.12s ease;
  }

  .folder-remove svg {
    width: 9px;
    height: 9px;
    fill: none;
    display: block;
  }

  .folder-row:hover .folder-remove {
    color: #d6d6de;
  }

  .folder-remove:hover {
    background: rgba(255, 255, 255, 0.1);
    color: #fff;
  }

  .folders-empty {
    padding: 7px 10px;
    color: #6a6a74;
    font-size: 12px;
  }

  .folder-add {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    padding: 8px 10px;
    border: 1px dashed rgba(255, 255, 255, 0.16);
    border-radius: 8px;
    background: transparent;
    color: #b9b9c3;
    font-size: 12.5px;
    cursor: pointer;
    transition: border-color 0.12s ease, color 0.12s ease, background 0.12s ease;
  }

  .folder-add svg {
    width: 14px;
    height: 14px;
    fill: none;
  }

  .folder-add:hover:not(:disabled) {
    border-color: #818cf8;
    background: rgba(129, 140, 248, 0.08);
    color: #e8e8ec;
  }

  .folder-add:disabled,
  .folder-remove:disabled {
    cursor: default;
  }

  .folder-remove:disabled:hover {
    background: transparent;
    color: #77777f;
  }

  /* A destructive action: an ordinary button, but red — it must not be styled
     as a link, since a link promises navigation, not deletion. */
  .btn-danger {
    margin-top: 10px;
    padding: 8px 14px;
    border: 1px solid rgba(248, 113, 113, 0.35);
    border-radius: 8px;
    background: transparent;
    color: #f87171;
    font-size: 12.5px;
    cursor: pointer;
    transition: background 0.12s ease, border-color 0.12s ease;
  }

  .btn-danger:hover {
    background: rgba(248, 113, 113, 0.12);
    border-color: rgba(248, 113, 113, 0.6);
  }

  /* ---- Start screen: continue watching ----
     flex, not the grid inherited from .overlay: that one has `place-items:
     center`, i.e. the column is sized from its content. The card grid's width
     then depends on the cards, and they change as posters finish loading —
     hence the layout jump. In a flex column with align-items: center the
     children's width is computed from the container, not the other way round,
     and the images can no longer affect it.
     scrollbar-gutter reserves the space for the scrollbar in advance —
     otherwise its appearance after loading would shift the content sideways
     (on Windows the bar takes layout space, unlike macOS). */
  .overlay.start {
    /* The scroll area starts below the title bar (48px of content height):
       otherwise the scrollbar reaches the top of the window and runs into the
       window controls. Centring then happens within the remaining area, which
       is arguably more correct for the start screen anyway — the bar occupies
       the top regardless. */
    top: 48px;
    display: flex;
    flex-direction: column;
    align-items: center;
    /* `safe` is mandatory: in a centred scrollable flex container the overflow
       goes BOTH ways, and the top of the content is cut off — it cannot be
       scrolled to, the scrollbar does not go there. With `safe`, centring
       applies only while the content fits, and beyond that the layout is
       pinned to the start. */
    justify-content: safe center;
    overflow-y: auto;
    /* both-edges, not plain stable: the content here is centred, and reserving
       space only on the right would shift it left by half the bar width —
       noticeable next to the centred title in the bar. With a mirrored reserve
       the width is constant and the centre stays the centre, whether the bar
       appears or not. */
    scrollbar-gutter: stable both-edges;
  }

  .recent {
    width: 100%;
    max-width: 900px;
    flex: none;
  }

  /* Padding and layout live here rather than on the scroll container: a
     scrollable block's padding-bottom is not part of its scrollable area, and
     the bottom gap vanished at exactly the height where the content was flush
     but no scrollbar had appeared yet. For an ordinary in-flow element the
     padding is part of its height, so it always works. */
  .start-inner {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 26px;
    padding: 32px 24px;
    width: 100%;
    /* 900px for .recent plus the horizontal padding */
    max-width: 948px;
  }

  /* The content does not shrink when height runs short — the container scrolls
     instead. */
  .overlay.start > *,
  .start-inner > * {
    flex: none;
  }

  .recent-head {
    color: #b9b9c3;
    font-size: 12.5px;
    margin-bottom: 16px;
    text-align: center;
  }

  /* Secondary to the grid above, and deliberately so: this section is how you
     get back into a season, not the primary "what do I watch" answer. A second
     grid of poster cards would compete with the first and turn the start screen
     into two equal halves. */
  .torrents {
    margin-top: 34px;
  }

  .torrents .recent-head {
    display: flex;
    align-items: baseline;
    justify-content: center;
    gap: 10px;
  }

  .torrents-total {
    color: #77777f;
    font-variant-numeric: tabular-nums;
  }

  .torrent-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    /* Matches the recents grid at its narrowest so the two sections line up
       rather than each finding its own width from its content. */
    width: min(560px, 100%);
    margin: 0 auto;
  }

  /* The row is a container, not a control — exactly like `.queue-row`, which
     also carries no hover and no fill of its own. Highlighting the whole row
     said "this is one clickable thing" when it is two with different actions,
     and it lit up even for a row whose open button is disabled (a torrent from
     the older layout, which cannot be reopened at all). Putting the hover on the
     button instead makes the disabled case correct for free.

     The resting fill is what gives the list structure on a start screen that
     has no panel around it. Keeping it and highlighting the button was what
     produced a visible seam in the first attempt — the highlight stopped where
     the cross began and the row's own fill carried on past it in a different
     shade. The fix is not to drop the fill but to **inset** the highlight: the
     row pads itself, so the button's pill floats inside the fill and touches
     none of its edges. Rendering the two settled it. */
  .torrow {
    display: flex;
    align-items: stretch;
    /* 11px, and it is arithmetic rather than taste: the cross has 8px of margin
       plus the row's 3px of padding to its right, so 11px is what sits between
       it and the row's edge. Matching that on the left centres it in the space
       the highlight leaves — measured, it was 2px against 11px, and the cross
       visibly hugged the highlight instead of sitting between the two edges. */
    gap: 11px;
    padding: 3px;
    border-radius: 11px;
    background: rgba(255, 255, 255, 0.035);
  }

  .torrow.busy {
    opacity: 0.5;
    pointer-events: none;
  }

  .torrow-open {
    flex: 1;
    /* Both halves, or the ellipsis below never fires: a flex item defaults to
       `min-width: auto` and refuses to shrink below its content. */
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 3px;
    padding: 10px 12px;
    border: none;
    /* Rounded on every side: the highlight is a pill of its own, not a slab cut
       off where the cross begins. */
    border-radius: 8px;
    background: transparent;
    color: #e8e8ec;
    font-size: 13px;
    text-align: left;
    cursor: pointer;
    transition: background 120ms ease;
  }

  /* 0.06 rather than `.menu-item:hover`'s 0.09 because this one composites over
     the row's own 0.035: 0.035 + 0.06·0.965 ≈ 0.093, so what the eye gets is the
     same step the queue gives. */
  .torrow-open:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.06);
  }

  /* A torrent from the older name-based layout, or one opened with history off:
     the data is there and can be removed, but there is no magnet to reopen it
     with. Dimmed rather than hidden — it is occupying disk either way. */
  .torrow-open:disabled {
    cursor: default;
    color: #9a9aa6;
  }

  .torrow-name {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .torrow-meta {
    display: flex;
    align-items: baseline;
    gap: 8px;
    max-width: 100%;
    overflow: hidden;
    color: #77777f;
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .torrow-resume {
    overflow: hidden;
    text-overflow: ellipsis;
    color: #9a9aa6;
  }

  .torrow-working {
    display: flex;
    align-items: center;
    gap: 7px;
    color: #b9b9c3;
  }

  .torrow-spin {
    flex: none;
    width: 11px;
    height: 11px;
    border-radius: 50%;
    border: 1.5px solid rgba(255, 255, 255, 0.25);
    border-top-color: #b9b9c3;
    animation: spin 0.8s linear infinite;
  }

  /* `.card-forget` is built to sit on top of a poster and stay invisible until
     the card is hovered. Both of those have to be undone here: the row is not a
     `.card`, so its hover rule never fires and the button would be permanently
     invisible — a delete control nobody can find.

     **Written to win, not left to source order.** `.card-forget` and a bare
     `.torrow-forget` are both one class, so the later rule takes it — and
     `.card-forget` is ~200 lines further down this file. That is not a
     hypothetical: it kept `position: absolute`, and with no positioned ancestor
     in the row the button escaped to `top: 4px; right: 4px` of the overlay and
     sat under the title-bar logo. Exactly the fight `.menu-item.torrent-file`
     above already documents losing once. */
  /* `.card-forget`'s own dark disc is for sitting on top of a poster, where a
     scrim is what makes a white glyph readable over an arbitrary frame. Over
     this row there is no frame to fight, and it inverted the app's one
     direction: every hover here lightens.

     **Visible at rest, unlike the recents cards and the queue** — and that is a
     difference in what the list is for, not an inconsistency. There, deletion
     is a rare correction to a list whose point is playing something, so hiding
     it until hover keeps the common case clean. Here it is half the reason the
     section exists: this is the only place disk usage is shown and the only
     place it can be reclaimed. And a row from the older layout has its open
     button disabled, so with the cross hidden it offers *no* action at all and
     reads as something the player is showing you for no reason. Quiet at rest,
     full strength when the row is under the cursor, lit when it is. */
  /* **Opacity is fixed at 1 and never animated**, and the three strengths come
     from colour instead. Since the cross is now permanently visible there is
     nothing opacity buys, and animating it on this element costs something:
     taking a layer off full opacity makes the engine re-rasterise it and switch
     how it antialiases, which on a 1.4px stroke reads as the glyph twitching as
     the pointer arrives — the "jumps and comes back" this replaced. A colour
     transition changes no geometry and no compositing. */
  .card-forget.torrow-forget {
    position: static;
    flex: none;
    align-self: center;
    /* Larger than `.card-forget`'s 22px, which is sized for sitting on a poster
       thumbnail. These are the only controls on their row and the row is 58px
       tall; at 22px with an 8px glyph they read as afterthoughts, and a ~1px
       ring at that size antialiases unevenly enough to look crooked. */
    width: 28px;
    height: 28px;
    background: transparent;
    color: #6f6f7a;
    opacity: 1;
    transition: background 120ms ease, color 120ms ease;
  }

  .torrow:hover .card-forget.torrow-forget,
  .card-forget.torrow-forget:focus-visible {
    color: #b9b9c3;
  }

  .card-forget.torrow-forget:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.1);
    color: #e8e8ec;
  }

  /* The row's two actions as one group, so `.torrow`'s 11px gap falls between
     the text and them rather than between every pair — which is what keeps the
     cross centred against the row's edge (see `.torrow`). The 8px that used to
     sit on the cross moves here for the same reason. */
  .torrow-actions {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-right: 8px;
  }

  .card-forget.torrow-forget svg {
    width: 10px;
    height: 10px;
  }

  /* A shade larger than the cross: a plus and a cross are the same two strokes
     forty-five degrees apart, but diagonals cover more of their box, so at equal
     size the plus reads lighter. Measured, this lands the glyph at ~8.1 against
     the cross's 7.60 — the small bump that makes them look equal.

     Written to beat `.torrow-forget svg` above rather than left to source order,
     since the update button carries both classes. */
  .card-forget.torrow-forget.torrow-update svg {
    width: 14px;
    height: 14px;
  }

  /* Written to beat `.torrow-forget:hover` (four classes), not left to source
     order — the lesson this file has now learned twice. Indigo rather than the
     cross's red-ish white: this one adds episodes. */
  .card-forget.torrow-forget.torrow-update:hover:not(:disabled) {
    color: #a5b4fc;
  }

  /* The torrent playing right now. Clearly below the resting strength above, or
     "cannot be deleted" and "can be deleted" look the same until clicked. More
     specific than the rule above on purpose — that one would otherwise brighten
     it on row hover. */
  .torrow:hover .card-forget.torrow-forget:disabled,
  .card-forget.torrow-forget:disabled {
    color: #3d3d45;
    cursor: default;
  }


  /* auto-FIT, not auto-fill: fill keeps empty columns in place, so a single
     card was pinned to the left of a wide empty grid. fit collapses the unused
     columns (together with their gaps) and a group of 1-3 cards is centred by
     justify-content.
     The 220px upper bound is mandatory: without it (at 1fr) a lone card would
     stretch across the whole list. In a full row it is never reached — four
     columns in 900px give 214px each — so the card size is unchanged.
     Caveat: an incomplete LAST row (a 5th card under four, say) is still pinned
     left — grid cannot centre a partial row in principle. That is ordinary grid
     behaviour, and it was not what the complaint was about. */
  /* **One row, always.** As a wrapping grid this was the only part of the start
     screen that grew with its content, and once network entries stopped being
     silently dropped it went three rows deep and pushed the torrents section
     220px below the fold at 1280x800 — measured. A rail keeps the page a fixed
     height whatever the history holds, so the list below it is never missed.

     **`safe center`, never a bare `center`.** A few cards should sit centred as
     the grid did, but a centred flex container that overflows does so in BOTH
     directions and the start becomes unreachable — no scrollbar goes there.
     Rendered with a bare `center`, twelve cards began at the fifth with the
     first four clipped off the left edge for good. `safe` falls back to
     flex-start the moment the content stops fitting, which is the same fix the
     start overlay itself already carries. */
  .recent-rail {
    display: flex;
    justify-content: safe center;
    gap: 14px;
    overflow-x: auto;
    /* Room for the hover outline. `.card-poster` draws a 2px ring at
       `outline-offset: 1px`, which lives 3px outside the card's box and was
       being cut off by this container's own clipping — visible as the top edge
       of the ring disappearing. The negative margin gives the padding back so
       nothing moves, and `scroll-padding` keeps a snapped card clear of it. */
    overflow-y: hidden;
    padding: 5px;
    margin: -5px;
    scroll-padding-inline: 5px;
    scroll-behavior: smooth;
    /* Proximity, not mandatory: a flick that lands between cards should settle
       on the nearer one, not be dragged back for the sake of alignment. */
    scroll-snap-type: x proximity;
    /* The one place the universal scrollbar skin is turned off on purpose. The
       arrows and the trackpad are the affordance here, and a bar under a single
       row of cards reads as the page having a second scroller. */
    scrollbar-width: none;
  }

  .recent-rail::-webkit-scrollbar {
    display: none;
  }

  /* `flex: 0 0 200px` is only half of a fixed width: **`min-width: auto` is the
     default and it wins**, so a card whose name is one long unbreakable token —
     `Dutton.Ranch.S01E02.WEB-DLRip-AVC.x264.seleZen`, i.e. every torrent file —
     grew to fit it. Measured at 312px against its neighbours' 200, and since the
     poster's height comes from its width that card was also half again as tall.
     The same rule the queue rows already carry, missed here. */
  .recent-rail .card {
    flex: 0 0 200px;
    min-width: 0;
    scroll-snap-align: start;
  }

  /* And on the ancestors between the flex item and the ellipsis, or the label
     never gets the chance to truncate. */
  .recent-rail .card-open,
  .recent-rail .card-meta,
  .recent-rail .card-name,
  .recent-rail .card-left {
    min-width: 0;
    max-width: 100%;
  }

  .recent-railwrap {
    position: relative;
  }

  /* Over the rail's edges rather than beside it: the row is already as wide as
     the screen allows, and taking 40px off each side for arrows would cost a
     card. Circular for the same reason the mini player's close button is —
     nothing to disagree with the card corners underneath. */
  /* Centred ON the rail's edge, half outside it. Inside, it covered the first
     card it was meant to reveal; fully outside, it drifted away from the row it
     belongs to. Straddling costs nothing here because `.start-inner` carries
     24px of horizontal padding — more than the 16px overhang — so nothing
     reaches the scroll container and no horizontal scrollbar can appear. */
  .rail-arrow {
    position: absolute;
    /* Level with the posters rather than the whole card: the name and the
       remaining time sit below and would pull the arrow off centre. */
    top: 0;
    bottom: 26px;
    width: 32px;
    display: grid;
    place-items: center;
    padding: 0;
    border: none;
    background: none;
    color: #e8e8ec;
    cursor: pointer;
    opacity: 0;
    transition: opacity 140ms ease;
  }

  .rail-arrow.on {
    opacity: 1;
  }

  .rail-arrow.prev {
    left: -16px;
  }

  /* 21, not 16, and the extra 5 is not a fudge. A scroll container's content
     *begins* at its content edge but is *clipped* at its padding edge, so the
     rail's 5px of horizontal padding — there to give the hover outline room —
     puts the row's start and its cut-off 5px apart. Measured: at scrollLeft 0
     the first card sits at the content edge, while cards on the right run all
     the way to the padding edge. So the left arrow lines up on the first card
     and this one on the clip boundary, and each is on the edge it actually
     shares with the row. */
  .rail-arrow.next {
    right: -21px;
  }

  /* 32 flat, border included — the circle has to be the button it sits in, or
     the glyph lands a pixel off centre and the chevron visibly leans. That is
     the border-box reset doing it rather than a line here. */
  .rail-arrow span {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 50%;
    background: rgba(16, 16, 22, 0.92);
    border: 1px solid rgba(255, 255, 255, 0.12);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.45);
  }

  .rail-arrow:hover span {
    background: rgba(32, 32, 42, 0.96);
  }

  /* **Optically centred, which is not the same as geometrically centred.** A
     chevron's ink sits in its open arms while the eye reads its vertex, so a
     bbox-centred one leans away from the point: measured on the rendered
     pixels, the centre of mass of `‹` is 0.18px right of the button's centre
     and `›` 0.23px left — exactly the directions they look wrong in. The nudge
     is toward the point, the same correction a play triangle needs. */
  .rail-arrow svg {
    width: 15px;
    height: 15px;
  }

  .rail-arrow.prev svg {
    transform: translateX(-0.5px);
  }

  .rail-arrow.next svg {
    transform: translateX(0.5px);
  }

  .card {
    position: relative;
  }

  .card-open {
    display: block;
    width: 100%;
    padding: 0;
    background: none;
    border: none;
    text-align: left;
    cursor: pointer;
    color: inherit;
  }

  .card-poster {
    position: relative;
    display: block;
    aspect-ratio: 16 / 9;
    border-radius: 8px;
    overflow: hidden;
  }

  /* No frame to show — either none has been decoded yet, or none ever will
     (a stream). Darker than everything around it rather than lighter: the old
     white wash made an empty card the BRIGHTEST thing in a dark grid, which is
     precisely backwards for something that has nothing in it. A recess reads as
     "no picture here"; a pale block reads as a picture that failed to load.
     The hairline keeps the area defined, and is scoped to the empty state so a
     card with a poster is not framed. */
  .card-poster.empty {
    background: rgba(0, 0, 0, 0.45);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.07);
  }

  .card-poster img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .card-open:hover .card-poster,
  .endcard:hover .card-poster {
    outline: 2px solid #818cf8;
    outline-offset: 1px;
  }

  /* End of an entry: previous / replay / next. The cards are a fixed width, so
     the replay button stays put whichever of them is missing — a queue has a
     first and a last entry, and the middle control jumping sideways between
     episodes would be the most visible thing on the screen. */
  .endscreen {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 20px;
  }

  .endrow {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 32px;
  }

  .endcard {
    width: 220px;
    display: flex;
    flex-direction: column;
    padding: 0;
    background: none;
    border: none;
    text-align: left;
    color: inherit;
    cursor: pointer;
  }

  /* Stands in for the frame a stream cannot provide. Dim on purpose: it is a
     statement about the entry, not a thing to look at. */
  .card-link {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: #7b7b8a;
  }

  .card-link svg {
    width: 30px;
    height: 30px;
  }

  /* The pair reads outwards from the replay button: "next" under the card on
     the left, "previous" under the card on the right, each aligned to the edge
     it sits against. Both labels were left-aligned before, which put them at
     the same side of the screen and made the row look like one thing pushed
     off-centre rather than two choices either side of a middle. */
  .endcard.prev {
    text-align: right;
  }

  /* These sit directly on the final frame of the film — a 0.35 scrim is not a
     background — so both lines carry the shadow every other caption over the
     video does. And the label is the start screen's grey lifted to something
     that survives a bright frame: at #8f8f9c on a snow scene it was invisible,
     which is exactly where the viewer is looking for "what is next". A rule
     that has to beat `.card-name`/`.card-left` is written to win rather than
     left to source order (see the queue rows and the torrent list). */
  .endcard .card-name {
    margin-top: 2px;
    color: #f2f2f6;
    text-shadow: var(--ui-shadow);
  }

  .endcard .card-left {
    color: #d2d2dc;
    text-shadow: var(--ui-shadow);
  }

  /* The countdown drains rather than fills: what it reports is the time left,
     and it lies over an arbitrary video frame, which is why it is indigo like
     .card-progress and not white like every other readout. */
  .endprogress {
    position: absolute;
    left: 0;
    bottom: 0;
    height: 3px;
    background: #6366f1;
    animation: end-advance var(--advance) linear forwards;
  }

  @keyframes end-advance {
    from {
      width: 100%;
    }
    to {
      width: 0;
    }
  }

  .endcancel {
    padding: 8px 18px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    border-radius: 999px;
    background: rgba(16, 16, 22, 0.92);
    color: #e8e8ec;
    font-size: 13px;
    cursor: pointer;
  }

  .endcancel:hover {
    background: rgba(32, 32, 42, 0.94);
    border-color: rgba(255, 255, 255, 0.24);
  }

  .card-progress {
    position: absolute;
    left: 0;
    bottom: 0;
    height: 3px;
    background: #6366f1;
  }

  /* The card's caption, and with it the tooltip's hit area: the name and the
     remaining time, never the poster above them. It gets no box of its own —
     `.card-name`'s 6px top margin collapses straight through it, so the wrapper
     covers the two line boxes and not the gap between the caption and the
     frame, and the card is exactly as tall as before. Measured in both engines:
     the card and both lines land on the same pixel with and without it. */
  .card-meta {
    display: block;
  }

  .card-name {
    display: block;
    margin-top: 6px;
    font-size: 12.5px;
    color: #e0e0e6;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .card-left {
    display: block;
    font-size: 11.5px;
    color: #8f8f9c;
  }

  .card-forget {
    position: absolute;
    top: 4px;
    right: 4px;
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: rgba(0, 0, 0, 0.6);
    color: #fff;
    cursor: pointer;
    opacity: 0;
    /* Only the fade-out stays animated — see the rule below. */
    transition: opacity 0.12s ease;
  }

  .card-forget svg {
    width: 8px;
    height: 8px;
    fill: none;
    display: block;
  }

  /* Appearance is instant (transition: none), like the fullscreen veil: the
     card outline is not animated at all, and any delay on the cross reads as
     "the card highlight is fading in slowly". */
  .card:hover .card-forget,
  .card-forget:focus-visible {
    opacity: 1;
    transition: none;
  }

  .setting-label {
    color: #b9b9c3;
    font-size: 12.5px;
    margin-bottom: 6px;
  }

  .segmented {
    display: flex;
    gap: 2px;
    padding: 3px;
    background: rgba(255, 255, 255, 0.07);
    border-radius: 8px;
  }

  /* Same control, stacked: device names are phrases, and a row of pills would
     either overflow the dialog or truncate them to uselessness. */
  .segmented.vertical {
    flex-direction: column;
    gap: 1px;
  }

  .segmented.vertical .segopt {
    text-align: left;
    white-space: normal;
  }

  .segopt {
    flex: 1;
    background: transparent;
    border: none;
    border-radius: 6px;
    color: #d6d6de;
    font-size: 12px;
    padding: 7px 6px;
    cursor: pointer;
    white-space: nowrap;
  }

  .segopt:hover {
    background: rgba(255, 255, 255, 0.1);
  }

  .segopt.sel {
    background: #6366f1;
    color: #ffffff;
    font-weight: 500;
  }

  .segopt:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .segopt:disabled:hover {
    background: transparent;
  }

  /* A slider and its readout. The readout is fixed-width and tabular so the
     digits do not shuffle the slider sideways while it is being dragged. */
  .slider-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .setting-slider {
    flex: 1;
    min-width: 0;
  }

  .slider-value {
    flex: 0 0 52px;
    text-align: right;
    color: #d6d6de;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }

  .setting-hint {
    color: #77777f;
    font-size: 11.5px;
    margin-top: 5px;
  }

  /* A hint explains what is above it; a control below it needs air, or the two
     read as one block — which is how "from the file name…" ended up glued to
     the language pills. Written as a sibling rule rather than a modifier class,
     so it holds wherever the pair occurs. */
  .setting-hint + .segmented {
    margin-top: 12px;
  }

  .settings-foot {
    color: #77777f;
    font-size: 11.5px;
    line-height: 1.45;
    margin-top: 4px;
    border-top: 1px solid rgba(255, 255, 255, 0.07);
    padding-top: 10px;
  }

  .settings-link {
    background: none;
    border: none;
    padding: 0;
    font-size: inherit;
    color: #818cf8;
    cursor: pointer;
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .settings-link:hover {
    color: #a5b4fc;
  }

  .settings-link:disabled {
    color: #77777f;
    cursor: default;
    text-decoration: none;
  }

  /* ---- Key bindings editor ----
     A row is a flex pair rather than the label-above-control shape the rest of
     the dialog uses: thirty-six actions at two lines each is a page of
     scrolling, and a chord is short enough to sit beside its own name. */
  /* Its own class rather than a borrowed .menu-title. That one is a *menu*
     heading and carries `padding: 6px 10px` for it, which set every section
     title 10px right of the rows it labels — and the `padding: 0` written here
     lost, because the two selectors weigh the same and .menu-title is defined
     later in the file (measured in the shipped bundle: byte 22325 against
     27825). Exactly the trap .menu-item.chapter-item is on record for. Same
     look, no collision to lose. */
  .keys-group {
    /* 28 above against 8 below, and the asymmetry is the whole point: a heading
       has to bind to the section it labels far more strongly than to the one it
       follows, or thirty-six rows read as one undifferentiated list. The row
       above contributes nothing — adjoining sibling margins collapse, so the
       gap is this 28 and not 28 plus the row's 6. No :first-child case either:
       the heading is the dialog's third child (head, tabs, then this). */
    margin: 28px 0 8px;
    color: #9a9aa5;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }


  .setting.keyrow {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 28px;
    margin-bottom: 6px;
  }

  /* `min-width: 0` on the item AND on the row above it, or a long action name
     refuses to shrink below its content and the dialog grows a horizontal
     scrollbar instead of ellipsising — the same trap the queue rows hit. */
  .keyrow-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #b9b9c3;
    font-size: 12.5px;
  }

  .keyrow-chords {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: 6px;
  }

  .kbadge {
    display: inline-flex;
    align-items: center;
    /* Measured against the shipped stylesheet: the glyph sits 9px from the
       chord text and 9px from the badge's padding box (10px from the border
       itself, which is the extra pixel of the border and reads as balanced).
       With the button flush against the text (gap: 0) its own 5px of internal
       padding was all that separated the two, against 8px on the outside — so
       the glyph read as pushed into the text it sat next to. */
    gap: 4px;
    padding: 3px 4px 3px 8px;
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.06);
    color: #e6e6ec;
    font-size: 12px;
    line-height: 1.3;
    white-space: nowrap;
  }

  /* Nothing to take off these two, so no × and no room held for one. */
  .kbadge.fixed,
  .kbadge.empty {
    padding-right: 8px;
    color: #77777f;
  }

  .kbadge.empty {
    border-style: dashed;
    background: none;
  }

  /* Indigo means one thing here as everywhere else: this is the live one. */
  .kbadge.recording {
    border-color: #6366f1;
    background: rgba(99, 102, 241, 0.18);
    color: #fff;
  }

  /* An 18px square, not a 7px glyph: the icon is the *mark*, the button is the
     target. As a bare <svg> the hit area was the strokes themselves, which on a
     badge this size is a few pixels of diagonal line — missable even when aimed
     at deliberately. The square is padding, so the glyph keeps its size. */
  .kbadge-x {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 18px;
    height: 18px;
    padding: 0;
    border: none;
    border-radius: 4px;
    background: none;
    color: #77777f;
    cursor: pointer;
    transition: background 0.12s ease, color 0.12s ease;
  }

  .kbadge-x svg {
    width: 8px;
    height: 8px;
    fill: none;
    stroke-width: 1.6;
  }

  /* Red on hover for the same reason .btn-danger is red: this one removes. */
  .kbadge-x:hover {
    background: rgba(248, 113, 113, 0.16);
    color: #f87171;
  }

  .kadd {
    width: 22px;
    height: 22px;
    padding: 0;
    border: 1px dashed rgba(255, 255, 255, 0.18);
    border-radius: 6px;
    background: none;
    color: #9a9aa5;
    font-size: 14px;
    line-height: 1;
    cursor: pointer;
  }

  .kadd:hover {
    border-color: #818cf8;
    border-style: solid;
    color: #fff;
  }

  .kreset {
    font-size: 11.5px;
  }

  /* Sits under the row it answers, not as a hint about the next one. */
  .keynote {
    margin: -2px 0 10px;
  }

  .tip {
    position: fixed;
    /* Placed by a transform from the window's origin, never by left/top: a box
       laid out at `left: x` may only use the width remaining to its right, so
       near the right edge the text wrapped one word per line and the measure
       that followed saw the wrapped width as the natural one. See floating.ts. */
    left: 0;
    top: 0;
    /* A path is the one tip that can be longer than the window, and it is never
       truncated: the useful part is its tail, so an ellipsis would hide exactly
       what is being looked for. It wraps instead.
       360px rather than 520: at the wider size a long path made one enormous
       line that read as a paragraph rather than a label. `anywhere` rather than
       `break-word` because a path or a release name is frequently a single
       unbroken token with no space to break at — `break-word` would leave it
       overflowing the box it was given.
       380 is that 360 plus the tip's own 18px of padding and 2px of border:
       the same box on screen, written as its outer size. The viewport term
       becomes honest at the same time — it was meant to keep 16px clear of
       each edge and, as a content width, kept 6px. */
    max-width: min(380px, calc(100vw - 32px));
    overflow-wrap: anywhere;
    background: rgba(14, 14, 20, 0.95);
    border: 1px solid rgba(255, 255, 255, 0.09);
    border-radius: 7px;
    color: #d6d6de;
    font-size: 12px;
    padding: 5px 9px;
    pointer-events: none;
    z-index: 90;
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.4);
  }


  /* Above the entire UI: layout jumps during the transition stay invisible.
     Colour: on the start screen — its background (so the transition does not
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

  .stepwrap {
    position: absolute;
    inset: 0;
    display: none;
    place-items: center;
    background: #000;
    pointer-events: none;
  }

  .stepwrap.visible {
    display: grid;
  }

  .stepwrap canvas {
    max-width: 100%;
    max-height: 100%;
  }

  .stepbadge {
    position: absolute;
    bottom: 128px;
    left: 16px;
    background: rgba(16, 16, 22, 0.85);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 8px;
    color: #e8e8ec;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    padding: 5px 10px;
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

  .menu {
    position: absolute;
    bottom: 100px;
    right: 24px;
    min-width: 256px;
    /* The panel sizes itself to its longest row, and without a ceiling that is
       unbounded in the one direction nobody watches: it is positioned by its
       RIGHT edge with `left: auto`, so it grows leftwards and simply walks off
       the window. Measured with a long release name in a 700px player — the
       box came out 972px wide at `offsetLeft: -297`, clipped by `.player`, with
       the name intact but unreachable and the × sitting over it. The text was
       not overflowing anything, which is why the ellipsis (correct all along)
       never fired: the panel kept growing instead.
       So: grow with the content, stop before the window edge. The viewport term
       is what makes the mini player work, where 520px does not exist. */
    max-width: min(520px, calc(100vw - 48px));
    /* 300px was written for a full-sized window; in the mini player the whole
       player is shorter than that, and the panel would run off the top edge. */
    max-height: min(300px, calc(100vh - 132px));
    overflow-y: auto;
    /* Same reason as .ctxmenu: sideways is never a direction this scrolls. */
    overflow-x: hidden;
    background: rgba(16, 16, 22, 0.94);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 10px;
    padding: 6px;
  }

  /* Sits where the track menus do (same right edge, just above the bar), and is
     hidden whenever one of them is open rather than fighting it for the space.
     A pill like .updbtn, which is the same species of control: a suggested
     action that turns up when it is relevant and leaves on its own. The surface
     is the shared floating-surface value, not an indigo fill — over arbitrary
     video the accent shouts, and it means "on/selected" everywhere else. */
  .skipbtn {
    position: absolute;
    right: 24px;
    bottom: 108px;
    /* Above the bar, below everything in the popup scale (.osd is 30). The OSC
       is the reason this is needed at all: its scrim reaches 44px above the
       controls, and .osc comes later in DOM order with a stacking context of
       its own — so without a z-index the gradient washes over the top half of
       the button, tinting it unevenly against its own shadow. */
    z-index: 20;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 14px 22px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    border-radius: 999px;
    /* The remaining time is the background IMAGE, drained by animating
       background-size. It was a scaled pseudo-element, and a transform distorts
       what it scales: the wash's own corners flattened as it narrowed, and its
       increasingly square left edge crept out from under the pill's rounded
       one. A background is clipped by the border-radius by definition, so there
       is nothing left to distort or to escape — and it removes the ::before,
       and with it the z-index/isolation pair that gotcha 9 forces on every
       absolutely positioned pseudo-element in this file.
       White rather than indigo: this reports a value, it does not offer a
       choice — the rule that keeps .seekfill white. */
    background-color: rgba(16, 16, 22, 0.92);
    background-image: linear-gradient(rgba(255, 255, 255, 0.1), rgba(255, 255, 255, 0.1));
    background-repeat: no-repeat;
    background-position: left center;
    background-size: calc(var(--skip-left, 1) * 100%) 100%;
    /* The width comes from the playback position, which arrives a handful of
       times a second — the transition is what turns those steps into a glide.
       It also means the bar cannot lie after a seek, and that a paused film
       freezes it without anyone arranging for that. */
    transition:
      background-size 0.3s linear,
      opacity 0.3s ease;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.45);
    color: #e8e8ec;
    font-size: 15px;
    letter-spacing: 0.01em;
    cursor: pointer;
    /* Entrance only, and a fixed duration: the button's lifetime is now the
       chapter's, so anything keyed to percentages of it would stretch a 0.2 s
       fade into three seconds on a minute-long opening. The exit is the fade
       carried by the inline opacity, which is driven by the same clock as the
       bar. */
    animation: skip-in 0.18s ease;
  }

  /* Only the colour: `background` shorthand here would drop the drain gradient. */
  .skipbtn:hover {
    background-color: rgba(32, 32, 42, 0.94);
    border-color: rgba(255, 255, 255, 0.24);
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.5);
  }

  /* The label never wraps: the button is anchored to the right edge, and a
     second line would push the chevron off its baseline. */
  .skip-label {
    white-space: nowrap;
  }

  .skipbtn svg {
    width: 17px;
    height: 17px;
    opacity: 0.75;
  }

  .skipbtn:hover svg {
    opacity: 1;
  }

  @keyframes skip-in {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
  }

  /* The row: the entry fills it, the × sits at the end and appears on hover —
     a permanent one in every row would read as a list of delete buttons. */
  .queue-row {
    display: flex;
    align-items: center;
    min-width: 0;
    /* Rows step aside while one is dragged over them. Not transitioned on the
       dragged row itself — that one has to keep up with the finger. */
    transition: transform 0.12s ease;
    touch-action: none;
  }

  .queue-row.dragging {
    position: relative;
    z-index: 1;
    transition: none;
    background: rgba(255, 255, 255, 0.06);
    border-radius: 6px;
  }

  .queue-row .menu-item {
    flex: 1;
    min-width: 0;
  }

  .queue-remove {
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    margin-left: 2px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: transparent;
    color: #8f8f9c;
    opacity: 0;
    cursor: pointer;
  }

  .queue-remove svg {
    width: 8px;
    height: 8px;
  }

  .queue-row:hover .queue-remove,
  .queue-remove:focus-visible {
    opacity: 1;
  }

  .queue-remove:hover {
    background: rgba(255, 255, 255, 0.1);
    color: #e8e8ec;
  }

  /* Wider than the track menus: chapter titles are sentences, not track names. */
  .menu.chapters {
    min-width: 260px;
    max-width: 380px;
  }

  /* Wider than the chapter list it shares its markup with: those are sentences,
     these are release names, and the panel grows with them until the window
     says no. */
  .menu.queue {
    max-width: min(520px, calc(100vw - 48px));
  }

  /* `.menu-item.chapter-item`, not `.chapter-item`: the element carries both
     classes, `.menu-item` sets `display: block`, and the two rules weigh the
     same — so the winner was decided by which came later in the file, and
     `.menu-item` does. Measured in the compiled bundle: `display:flex` at byte
     27352, `display:block` at 27753. The row was therefore never a flex
     container, which made `flex: 1` and `min-width: 0` below inert and left the
     name an INLINE span — and `overflow: hidden` does not apply to those, so it
     ran past the button and under the × instead of ellipsising. A rule that has
     to win is written to win rather than left to source order. */
  .menu-item.chapter-item {
    display: flex;
    align-items: baseline;
    gap: 12px;
    /* Same reason as .chapter-name, one level up: the row is itself a flex item
       of .queue-row and has to be allowed to shrink. */
    min-width: 0;
  }

  .chapter-name {
    flex: 1;
    /* Without this the label never ellipsizes: a flex item's `min-width` is
       `auto`, i.e. its content, so a long file name pushed the row wider than
       the panel and the panel grew a horizontal scrollbar instead. */
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* A column of timestamps, so the digits have to line up. (`float: right` from
     .menu-item .hint does nothing here — the row is a flex container.) */
  .chapter-item .hint {
    font-variant-numeric: tabular-nums;
  }

  .menu-title {
    color: #9a9aa5;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    padding: 6px 10px;
  }

  /* `width: 100%` is the row filling the menu, not overhanging it by its own
     20px of padding — and it always was: a <button> is border-box in both
     engines' own stylesheets, which is why every one of the nine local
     `box-sizing` declarations this project accumulated sat on something that
     was NOT a button. Measured before and after the reset, this row and the
     `.hint` at its right edge do not move by a pixel. */
  .menu-item {
    display: block;
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    border-radius: 6px;
    color: #d6d6de;
    font-size: 13px;
    padding: 8px 10px;
    cursor: pointer;
  }

  .menu-item:hover {
    background: rgba(255, 255, 255, 0.09);
  }

  .menu-item.sel {
    color: #818cf8;
    font-weight: 600;
  }

  /* Timecodes above the bar (current on the left, duration on the right), the
     bar full-width below: the DOM is unchanged, flex order does the arranging. */
  .seekrow {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
  }

  .seekrow .time:first-child {
    order: 1;
    margin-right: auto;
    margin-left: 8px;
    text-align: left;
  }

  .seekrow .time:last-child {
    order: 2;
    margin-right: 8px;
    text-align: right;
  }

  .seekwrap {
    order: 3;
    /* slightly narrower than the row: the track edges line up with the visible
       edges of the icons below */
    flex: 1 0 calc(100% - 16px);
    margin: 6px 8px 0;
    position: relative;
    display: flex;
    align-items: center;
    padding: 6px 0;
    cursor: pointer;
    touch-action: none;
  }

  .seektrack {
    position: relative;
    width: 100%;
    height: 4px;
    border-radius: 2px;
    background: rgba(255, 255, 255, 0.28);
  }

  /* Below the played fill in DOM order, so the white overtakes it rather than
     the other way round. Dimmer than the fill and brighter than the track: it
     is neither what has been watched nor empty bar, and the three have to be
     distinguishable at a glance on a 4px strip. */
  .seekbuffered {
    position: absolute;
    top: 0;
    bottom: 0;
    border-radius: 2px;
    background: rgba(255, 255, 255, 0.3);
  }

  .seekfill {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    border-radius: 2px;
    background: #fff;
  }

  /* The A–B segment. Indigo because it lies over the bar's own white fill and
     the video behind it, where another white would simply vanish — the same
     exception .card-progress makes. Under the chapter notches and the knob in
     DOM order, so neither is swallowed by it. */
  .abregion {
    position: absolute;
    top: -2px;
    bottom: -2px;
    background: rgba(99, 102, 241, 0.55);
    border-radius: 2px;
    pointer-events: none;
    transition: background 0.15s ease;
  }

  /* Marks still set, loop not running (playback is past B). Faint rather than
     gone: the segment is still there to come back to, and mpv re-arms it by
     itself the moment playback is inside it again. */
  .abregion.disarmed {
    background: rgba(99, 102, 241, 0.2);
  }

  /* Mini: a 420px window is mostly picture, so the chrome has to earn its
     pixels. The title bar goes entirely (its own drag region goes with it, but
     an always-on-top window in a corner is moved by dragging the video, which
     already works), the bar keeps play/pause and the seek row, and everything
     that belongs to a full-sized session — clusters of toggles, menus,
     timecodes — stands down. Nothing is hidden that cannot be reached another
     way: the context menu is untouched. */
  .player.mini .topbar {
    display: none;
  }

  .player.mini .osc {
    padding: 0 10px 6px;
  }

  .player.mini .cluster.cl-left,
  .player.mini .cluster.cl-right {
    display: none;
  }

  .player.mini .seekrow .time {
    font-size: 10.5px;
  }

  .player.mini .seekwrap {
    margin: 2px 6px 0;
  }

  /* `.controls` is a `1fr auto 1fr` grid, so hiding the side clusters does not
     centre what is left — the remaining cluster auto-places into the first
     column and the row sits against the left edge. The mini row is one item,
     so it gets a one-column grid. */
  .player.mini .controls {
    grid-template-columns: 1fr;
    justify-items: center;
    margin-top: 4px;
  }

  .player.mini .controls button.play {
    width: 38px;
    height: 34px;
  }

  .player.mini .controls button.play svg {
    width: 24px;
    height: 24px;
  }

  /* Everything that floats over the picture is sized for a full window, and in
     a 420px one that is a third of the frame. Scaled to the window rather than
     shrunk to nothing — these are read at arm's length, which is the whole
     point of a corner window. The popup also moves up: `top: 60px` was there
     to clear the title bar, and in mini there is no title bar. */
  .player.mini .osd {
    top: 12px;
    left: 12px;
    min-width: 0;
    gap: 6px;
    /* Concentric with the window's own corner: the inner radius should be the
       outer one minus the gap, and the outer one is not knowable — macOS picks
       it, Windows 11 picks another, and macOS 26 rounds windows considerably
       more than macOS 11 did. So it errs *tight* on purpose: an inner corner
       slightly squarer than the rule wants passes unnoticed, while one rounder
       than the curve behind it reads as a mistake. 8 + the 12px gap assumes an
       outer radius near 20; drop it further if the window ever looks rounder
       than the popup inside it. */
    border-radius: 8px;
    padding: 8px 12px;
  }

  .player.mini .osd-text {
    font-size: 13px;
  }

  .player.mini .osd-sub {
    font-size: 11px;
  }

  .player.mini .skipbtn {
    right: 12px;
    /* Clears the shortened bar, which is ~77px tall: 6px of padding, a 34px
       play row with its 4px margin, and a seek row of a 14px timecode line
       above a ~19px track. At 72 the pill sat on the timecodes. */
    bottom: 88px;
    gap: 8px;
    padding: 9px 16px;
    font-size: 13px;
  }

  .player.mini .skipbtn svg {
    width: 15px;
    height: 15px;
  }

  /* The way out has to be visible in the mode that hides everything else. */
  .player.mini .minibtn {
    display: grid;
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

  /* Fades with the rest of the UI: in a corner window the picture is the point,
     and a button pinned over it forever is one more thing in the way. */
  .player.mini.idle-ui .minibtn {
    opacity: 0;
    pointer-events: none;
  }

  /* Chapter boundaries: a notch cut through both the fill and the empty part of
     the track, so it reads the same on either side of the playhead. Not part of
     the hit area — every pointer event on the bar belongs to .seekwrap, and a
     marker that swallowed a click would be a dead pixel column in the middle of
     the seekbar. */
  .chapmark {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
    margin-left: -1px;
    border-radius: 1px;
    background: rgba(16, 16, 22, 0.9);
    pointer-events: none;
  }

  .seekknob {
    position: absolute;
    top: 50%;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: #fff;
    transform: translate(-50%, -50%);
    transition: transform 0.12s ease;
    pointer-events: none;
  }

  .seekwrap:hover .seekknob {
    transform: translate(-50%, -50%) scale(1.3);
  }

  /* No container: a thumbnail with a white outline and a shadowed timestamp
     straight over the video */
  .hovertip {
    position: absolute;
    bottom: 18px;
    transform: translateX(-50%);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    color: #fff;
    font-size: 13px;
    font-variant-numeric: tabular-nums;
    pointer-events: none;
    white-space: nowrap;
    text-shadow: var(--ui-shadow);
  }

  .hovertip .thumbwrap {
    position: relative;
  }

  /* Capped at the thumbnail width on purpose: hoverX is clamped assuming the
     tip is exactly that wide, so a long chapter title would push the whole
     popup past the window edge. */
  .hover-chapter {
    max-width: 184px;
    margin-top: -4px;
    overflow: hidden;
    text-overflow: ellipsis;
    color: #d6d6de;
    font-size: 12px;
  }

  /* 184, border included — the outer width of the popup, which is the figure
     everything around it is written against: `.hover-chapter` caps itself at
     the same number, and `onSeekHover` clamps hoverX by half of it (92). Those
     agreed with the rendered box while this said 180 and grew by its outline;
     with the border-box reset they agree with the declaration too.
     One consequence of the box changing meaning: `aspect-ratio` follows
     box-sizing, so the video's shape is now the shape of the outlined frame
     rather than of the picture inside it. That is a 2px inset on each side —
     under 2% of the ratio, taken by `object-fit: cover` — and the frame is the
     part anyone sees. */
  .hovertip .thumb {
    display: block;
    width: 184px;
    /* Set inline from the video's own dimensions; this is the fallback for the
       moment before mpv has reported them. */
    aspect-ratio: 16 / 9;
    object-fit: cover;
    border-radius: 10px;
    border: 2px solid rgba(255, 255, 255, 0.85);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.45);
  }

  .hovertip .thumb.placeholder {
    background: rgba(16, 16, 22, 0.8);
  }

  /* Not "the preview failed" but "this part has not arrived". The arrow is the
     one already used by the download readout, so the two say the same thing in
     the same shape. */
  .hovertip .thumb.pending {
    display: grid;
    place-items: center;
    color: #6f6f7a;
  }

  .hovertip .thumb.pending svg {
    width: 22px;
    height: 22px;
  }

  /* The exact frame fetched when the cursor comes to rest is a different scene
     from the grid frame it replaces in roughly 40% of hovers, so it crosses
     over instead of cutting. This layer is the OUTGOING frame fading out — the
     one underneath is always the newest, which is what keeps stale content from
     showing through while the browser decodes it. Frames that merely track a
     moving cursor never get here and stay instant.
     Keep the duration under FADE_HOLD_MS in thumbs.svelte.ts, which drops this
     element once the animation has run. */
  .hovertip .thumb.fading {
    position: absolute;
    inset: 0;
    /* **Takes its size from the box it covers, never from a ratio of its own.**
       It inherits `.thumb`'s width and aspect-ratio, and the inline aspect that
       corrects those lives only on the in-flow frame — so this layer computed
       16:9 and, on anything wider (a 1920x816 rip is ordinary), came out 105px
       tall over an 80px box: it overflowed 25px downward onto the timestamp for
       the length of the crossfade and snapped back. Measured, and it explains
       why arriving from an un-downloaded position never blinked — there is no
       outgoing frame there, so this element is never created. */
    /* 100%, not `auto`: an <img> is a REPLACED element, so `width: auto` takes
       its intrinsic size rather than resolving from the insets — measured, that
       made the layer 484px tall. Percentages against the positioned wrapper are
       what actually make it cover exactly, and they only do so because boxes
       are border boxes: `.thumb`'s own 2px border used to be added on top of
       the 100% and left this 4px bigger, the same overflow in miniature. */
    width: 100%;
    height: 100%;
    aspect-ratio: auto;
    object-fit: cover;
    animation: thumb-cross 0.14s ease forwards;
  }

  @keyframes thumb-cross {
    to {
      opacity: 0;
    }
  }

  /* The dimming and the spinner appear with a ~150 ms delay: on fast responses
     (a cache hit) the loader does not flash at all. */
  .thumbwrap.loading .thumb {
    transition: opacity 0.15s ease 0.15s;
    opacity: 0.45;
  }

  /* Over the black field mpv shows before its first frame. No backdrop of its
     own: there is nothing underneath to dim. */
  .loading-overlay {
    pointer-events: none;
  }

  /* The shared floating-surface fill comes with a hairline and a shadow, and
     both halves are load-bearing: this fill is the start screen's own colour,
     so without the border the plate is invisible exactly when a link has just
     been pasted — which is the moment it exists for. The shadow does the same
     job over video, where the border alone would be lost against a bright
     frame. */
  /* Icon column, text column — the layout every notification with a subtitle
     uses, and the reason is not taste. The first version wrapped a second line
     onto a new flex row and re-derived its left offset by hand; measured, that
     put the two lines 4px apart horizontally and 16px apart vertically, because
     `flex-wrap` makes `gap` a ROW gap as well and because the spinner is not the
     width it is declared to be (see .loading-spin). With the text as one item
     there is no offset left to compute, and nothing to keep in sync when the
     spinner or the gap changes. */
  .loading-box {
    display: flex;
    align-items: center;
    gap: 13px;
    padding: 13px 17px;
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 12px;
    background: rgba(16, 16, 22, 0.92);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.45);
    color: #e8e8ec;
    font-size: 13px;
    /* The figures refresh every second — without this the box twitches as the
       digits change width. */
    font-variant-numeric: tabular-nums;
  }

  .loading-text {
    display: flex;
    flex-direction: column;
    /* Title to subtitle, not paragraph spacing: they are one statement. */
    gap: 2px;
    min-width: 0;
    /* A librqbit error can be a sentence. Wrapping it inside the column keeps
       the wrapped lines aligned under the first, where a wrap at the box level
       would not be. */
    max-width: min(420px, calc(100vw - 96px));
  }

  .loading-title {
    line-height: 1.3;
  }

  /* The state is the title ("waiting for data"); this is the evidence for it.
     Below rather than beside, because on one line the two read as a single
     sentence and stop being a state and its reason. */
  .loading-sub {
    color: #9a9aa6;
    font-size: 12px;
    line-height: 1.35;
  }

  /* 20 including the 2.5px ring — a spinner written as its content is a circle
     5px bigger than the number beside it, which is what threw the old two-line
     alignment off. The border-box reset is what holds this now. */
  .loading-spin {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    border: 2.5px solid rgba(255, 255, 255, 0.3);
    border-top-color: #fff;
    animation: spin 0.8s linear infinite;
    /* A flex item shrinks to fit by default, and a squashed circle is an
       ellipse. */
    flex: none;
  }

  .spinner {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    opacity: 0;
  }

  .spinner.on {
    opacity: 1;
    transition: opacity 0.12s ease 0.18s;
  }

  /* 27, which is the 22 this was written as plus its 2.5px ring on each side:
     the same circle over the preview as before, now as an outer size. */
  .spinner::after {
    content: '';
    width: 27px;
    height: 27px;
    border-radius: 50%;
    border: 2.5px solid rgba(255, 255, 255, 0.35);
    border-top-color: #fff;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .time {
    color: #e0e0e6;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    min-width: 44px;
    text-align: center;
    text-shadow: var(--ui-shadow);
  }

  .controls {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    margin-top: 8px;
  }

  .cluster {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .cl-left {
    justify-self: start;
  }

  .cl-center {
    justify-self: center;
    gap: 14px;
  }

  .cl-right {
    justify-self: end;
  }

  .controls button {
    background: transparent;
    border: none;
    border-radius: 8px;
    width: 36px;
    height: 34px;
    display: grid;
    place-items: center;
    color: #e8e8ec;
    cursor: pointer;
    padding: 0;
  }

  .controls button svg {
    width: 20px;
    height: 20px;
  }

  .controls button.play {
    width: 46px;
    height: 42px;
  }

  /* 32 rather than 30: every other box in the row is a multiple of 4 (20 for
     the ordinary controls, 24 for fullscreen), and rounding up also widens the
     gap to prev/next, which is the point of them being small. Those stay at 20
     — their glyph fills 12 of its 24 units, so a 24px box would render it 12px
     wide against play's 14.7 and the two would read as the same size. */
  .controls button.play svg {
    width: 32px;
    height: 32px;
  }

  /* The "corners" glyph is drawn more compactly than the rest — even out the
     optical size */
  .controls button.fs svg {
    width: 24px;
    height: 24px;
  }

  .controls button:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.12);
  }

  .controls button:disabled {
    color: #6a6a74;
    cursor: default;
  }

  .controls button.active {
    color: #818cf8;
  }

  .speed {
    color: #818cf8;
    font-size: 12px;
    font-weight: 600;
    padding: 0 6px;
    text-shadow: var(--ui-shadow);
  }

  input[type='range'] {
    appearance: none;
    -webkit-appearance: none;
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(
      to right,
      #ffffff var(--progress, 0%),
      rgba(255, 255, 255, 0.28) var(--progress, 0%)
    );
    outline: none;
    cursor: pointer;
  }

  input[type='range']::-webkit-slider-thumb {
    appearance: none;
    -webkit-appearance: none;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: #ffffff;
    border: none;
    transition: transform 0.12s ease;
  }

  input[type='range']:hover::-webkit-slider-thumb {
    transform: scale(1.25);
  }

  input[type='range']:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .volumebar {
    width: 90px;
    margin: 0 6px 0 2px;
  }
  /* ---- Casting ------------------------------------------------------------ */

  /* Indigo means on: a live cast session is a boolean "on", same rule as
     .switch.on and the focus rings. */
  .controls button.cast-on {
    color: #818cf8;
  }

  .menu.castmenu {
    min-width: 280px;
  }

  /* Written to win against .menu-item's display: block (the two-class lesson:
     a lone modifier class of equal weight loses to source order). */
  .menu-item.cast-device {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
  }

  .cast-model {
    font-size: 11px;
    color: rgba(232, 232, 236, 0.45);
  }

  .cast-current {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 4px 14px 8px;
    /* Explicit: this is a bare div, not a .menu-item button, so it inherits
       the document's default (black) rather than the menu's text colour —
       which is exactly how the device name shipped unreadable. */
    color: #e8e8ec;
  }

  .cast-current .cast-name {
    font-weight: 600;
  }

  /* The TV declared its volume fixed (or never reported one): the slider is
     disabled rather than silently inert, and keys/wheel raise the OSD saying
     volume lives on the TV's own remote. */
  .volumebar:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .cast-state {
    font-size: 11px;
    color: rgba(232, 232, 236, 0.55);
  }

  .cast-empty {
    padding: 10px 14px;
    font-size: 12.5px;
    color: rgba(232, 232, 236, 0.55);
  }

  .cast-hint {
    padding: 8px 14px 10px;
    font-size: 11px;
    line-height: 1.45;
    color: rgba(232, 232, 236, 0.4);
    max-width: 300px;
  }

  /* Nearly opaque, deliberately not fully: mpv sits paused behind on the frame
     the cast started from, and letting it ghost through says "this is what is
     on the TV" — while the alpha is high enough that even if nothing painted
     there would be a dark field, not the desktop (gotcha 10: mpv always paints
     its field once the VO is configured, so this is belt and braces). */
  .overlay.castscreen {
    background: rgba(11, 11, 16, 0.88);
  }

  .castscreen-box {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 24px;
    color: #e8e8ec;
    text-align: center;
  }

  .castscreen-box > svg {
    width: 56px;
    height: 56px;
    opacity: 0.85;
  }

  .castscreen-title {
    font-size: 15px;
    font-weight: 600;
  }

  .castscreen-sub {
    font-size: 13px;
    color: rgba(232, 232, 236, 0.6);
    max-width: min(440px, 80vw);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .castscreen-state {
    font-size: 12px;
    color: rgba(232, 232, 236, 0.5);
  }
</style>
