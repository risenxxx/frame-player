/**
 * Booting mpv and handing it files: `initPlayer` (options, observers, events),
 * the hooks the page passes it, and `loadFiles`, which reads those hooks too —
 * the reason the two share a module.
 */

import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import {
  command,
  init,
  listenEvents,
  observeProperties,
  setProperty,
  type MpvConfig,
} from 'tauri-plugin-libmpv-api';

import { extensionOf } from '../format';
import { t } from '../i18n.svelte';
import { showOsd } from '../osd.svelte';
import { IS_MAC } from '../platform';
import { loadChapters } from './chapters';
import { applyLoopMode, loadLoopMode, loopOptions, VOLUME_MAX } from './commands';
import { AUDIO_EXTENSIONS, PATH_LIST_SEP, SUB_DIRS, SUBTITLE_EXTENSIONS, VIDEO_EXTENSIONS } from './files';
import { applyNormalize, applySdrColor, loadNormalize, loadSdrColor, setUserConfKeys } from './filters';
import { applyProperty, OBSERVED, resyncState, type ObservedName } from './mirrors';
import { player } from './state.svelte';
import { attachTrack, loadTracks } from './tracks';
import { applyYtdlpPath, refreshYtdlp } from './ytdlp.svelte';

export type PlayerHooks = {
  /// Runs for every observed property, after the mirror has been updated —
  /// so a handler reading `player.x` sees the new value. Kept as one callback
  /// rather than per-property hooks because the ordering inside a single
  /// property-change matters in a few places (see the seekbar rules).
  property?: (name: ObservedName, data: never) => void;
  fileLoaded?: () => void;
  playbackRestart?: () => void;
  /// Before a new file is loaded, with the path about to be opened, so overlays
  /// and gestures can stand down and the UI can pre-fill what it already knows
  /// about that file. Awaited: the page uses it to set load-time mpv options
  /// (`start`), which must land before the loadfile command.
  beforeLoad?: (path: string) => void | Promise<void>;
  /// After the videos of a drop have been handed to mpv, with the list that was
  /// opened. The queue-the-folder rule hangs off this rather than off
  /// `loadFiles` itself, because this module must not know that a playlist
  /// module exists — the dependency runs the other way.
  filesOpened?: (videos: string[]) => void;
  /// mpv gave up on the file it was opening.
  loadFailed?: () => void;
  /// A `.torrent` file was opened. mpv never sees it — the torrent client does —
  /// so this module classifies it and hands it over, exactly as `filesOpened`
  /// hands the queue-the-folder decision to someone who knows what a playlist is.
  openTorrentFile?: (path: string) => void;
};

let hooks: PlayerHooks = {};

/**
 * Why libmpv-2.dll could not be loaded, in place of the plugin's "Failed to
 * create mpv instance", which names neither the file nor the cause.
 * `libmpv_load.rs` loads the library before the plugin does in order to keep
 * this; it is Windows-only, and `null` whenever the library loaded.
 */
async function libmpvLoadError(): Promise<string | null> {
  try {
    const failure = await invoke<{ code: number; message: string; missing: string[] } | null>(
      'libmpv_load_error',
    );
    if (!failure) return null;
    const reason = t('error.mpv_load', { reason: failure.message, code: failure.code });
    if (failure.missing.length === 0) return reason;
    return `${reason} ${t('error.mpv_missing', { names: failure.missing.join(', ') })}`;
  } catch {
    return null;
  }
}

/**
 * Boots mpv and subscribes to it. Returns the teardown callbacks.
 *
 * The caller shows the window afterwards: `force-window=yes` gives mpv a black
 * field before the first frame, and the window is transparent until then.
 */
export async function initPlayer(config: PlayerHooks): Promise<Array<() => void>> {
  hooks = config;
  const unlisteners: Array<() => void> = [];

  const savedVolume = Number(localStorage.getItem('frameplayer.volume') ?? '100');
  // Where yt-dlp is, if anywhere. Asked for BEFORE init because the answer goes
  // into the initial options — mpv reads `ytdl_hook-ytdl_path` when the hook
  // loads, not when a URL is opened.
  // NOT awaited: nothing about starting mpv depends on it, and anything that
  // does hold up `init` holds up the whole window — a lesson learned the
  // expensive way when this call still ran `yt-dlp --version` (11 s a go).
  // The hook reads `ytdl_path` when it runs, not when it loads, so applying the
  // path a moment later is in time for any link a human could open.
  void refreshYtdlp().then(applyYtdlpPath);
  player.loopMode = loadLoopMode();

  const initialOptions: Record<string, string | number | boolean> = {
    'vo': 'gpu-next',
    'hwdec': 'auto-safe',
    'force-window': 'yes',
    'input-default-bindings': 'no',
    // mpv's own Now Playing integration is built for mpv.app and under libmpv
    // is half of itself: a Control Center entry reading "mpv" that never
    // learns a title or a pause, with buttons that do nothing. Off, so that it
    // does not compete with the player's own (now-playing.svelte.ts).
    ...(IS_MAC ? { 'input-media-keys': 'no' } : {}),
    'osc': 'no',
    'hr-seek': 'yes',
    'media-controls': 'yes',
    // Closed captions (eia_608/708) are decoded incrementally — roll-up and
    // paint-on modes build the on-screen text over many packets — so a seek
    // into the middle of a caption leaves residue behind. This player seeks far
    // harder than stock mpv (the scrub pump and drag previews fire a seek as
    // fast as the file allows), which compounds the residue into captions from
    // several different scenes stacked on top of each other. mpv's default is
    // `no`; for us the trade (a subtitle that began before the seek point stays
    // hidden until the next event) is plainly worth it.
    'sub-clear-on-seek': 'yes',
    // mpv's default is `exact`, i.e. only a subtitle file named exactly like
    // the video. Releases that keep subtitles in a `Subs/` subfolder, or name
    // them `<movie>.rus.srt`, therefore showed none at all.
    'sub-auto': 'fuzzy',
    // A path list, so the separator is the platform's own. Only the floor:
    // `pointAtExternalTracks` widens it per folder before each file opens.
    'sub-file-paths': SUB_DIRS.join(PATH_LIST_SEP),
    // External dubs, the same way: mpv's default is `no`, so a release that
    // ships its dub as `<episode>.mka` beside the video had to be added by hand
    // every episode. `fuzzy` for the reason `sub-auto` is: the file is named
    // after the episode, with a studio or a language after it.
    'audio-file-auto': 'fuzzy',
    // mpv's own default is 130; quiet films regularly need more than that.
    'volume-max': VOLUME_MAX,
    // mpv defaults to zlib level 7, which is a size-first choice that buys
    // nothing here. Measured on a 4K frame of photographic content (the same
    // libavcodec PNG encoder mpv uses for screenshots): level 7 took 1.46 s for
    // 16.4 MB, level 3 took 0.73 s for 16.4 MB — half the wait, identical size.
    // Level 0 is 0.21 s but 23.8 MB, which is the right trade only for the
    // throwaway file behind clipboard copy (see CLIPBOARD_SHOT_OPTIONS).
    // A 10-bit source doubles all of these, because screenshots of it are
    // written as 16-bit PNG.
    'screenshot-png-compression': 3,
    'volume': savedVolume,
    // Explicit rather than inherited. The CLI enables the ytdl hook by default;
    // for the client API the manual is ambiguous about it, and a feature that
    // silently depends on which side of that line libmpv falls is not worth the
    // saved line.
    'ytdl': 'yes',
    // The path is not known yet (the probe above runs in the background) and is
    // applied by `applyYtdlpPath` when it lands. The hook searches PATH on its
    // own, which a GUI process does not have — so on macOS it would never find
    // a Homebrew or ~/bin install without us.

    // The end of a file has to be a moment we own: the end screen, the "next
    // episode" countdown and the wrap of a repeating playlist all live there.
    // mpv's own `yes` stops only on the LAST playlist entry and advances
    // silently everywhere else, which is exactly the moment we want.
    'keep-open': 'always',
    ...loopOptions(player.loopMode),
  };

  // mpv's own log, for the issue report (report.svelte.ts). libmpv writes
  // nothing anywhere unless asked — `terminal` is off — so without this every
  // decoder fallback and every refused option is gone the moment it happens.
  // A file rather than log messages over the plugin: those would cross the
  // IPC bridge one event per line, and mpv's file writer has a thread of its
  // own. mpv writes a log file at `debug` whatever `msg-level` says (msg.c
  // raises every module to at least that while a file is open), truncates it
  // when it opens it, and a `log-file` in the viewer's mpv.conf wins, as every
  // line of theirs does.
  const logFile = await invoke<string>('diag_mpv_log_path').catch(() => null);
  if (logFile) initialOptions['log-file'] = logFile;

  // The compiled shaders, kept across sessions as the mpv CLI keeps them.
  // libmpv has nowhere to put them by default, so every session compiled every
  // shader afresh on the render thread, and the picture stands still for as
  // long as that takes: under D3D11 the upscaler's ArtCNN took 12 s the first
  // time a window grew past its threshold (FXC, ~2.2 s for each of its five
  // compute passes; measured on an RTX 5080), and 0 s from the cache. A
  // `gpu-shader-cache-dir` in the viewer's mpv.conf wins, as every line does.
  const shaderCache = await invoke<string>('mpv_shader_cache_dir').catch(() => null);
  if (shaderCache) initialOptions['gpu-shader-cache-dir'] = shaderCache;

  // The user's mpv.conf (%APPDATA%/<app>/mpv.conf) goes on top of the defaults,
  // mpv-style: the last line wins. Kept as a map of its own rather than merged
  // in place, because it is the half that may be wrong — see `start` below.
  let userOptions: Record<string, string> = {};
  try {
    const conf = await invoke<{ path: string; options: [string, string][] }>('user_mpv_conf');
    player.mpvConfPath = conf.path;
    userOptions = Object.fromEntries(conf.options);
  } catch (e) {
    console.warn('user_mpv_conf failed:', e);
  }

  /**
   * Start mpv, and do not let somebody's mpv.conf be the reason there is no
   * player.
   *
   * mpv refuses an option it does not know, and refuses *initialization* when
   * it does — so one bad line takes the whole player down. Calling that a typo
   * misses what actually happens: options are **removed between mpv versions**
   * (0.41 dropped `--alpha` in favour of `--background`), so a config that
   * worked for a year stops working on an upgrade the viewer never asked for,
   * and it takes the player with it.
   *
   * The failure is also the worst shape it could be. Nothing crashes: the
   * window opens, the start screen draws, everything looks alive — and then the
   * first file hangs for ever on "opening…", because it is being handed to an
   * mpv that was never initialized. Nothing on screen connects that to a line
   * in a text file.
   *
   * So the viewer's half is dropped and init is retried without it, with mpv's
   * own message kept: it names the offending option, which is the one thing
   * needed to fix the file. Our own defaults stay strict — a failure there is a
   * bug in this file rather than a configuration, and starting anyway would
   * hide it. Retrying after a failed `init` may itself not be allowed by the
   * plugin, and that costs nothing: the second failure lands on the same
   * `initError` the single attempt used to produce.
   */
  const start = async (): Promise<boolean> => {
    const attempt = async (options: Record<string, string | number | boolean>) => {
      const config: MpvConfig = { initialOptions: options, observedProperties: OBSERVED };
      await init(config);
    };

    if (Object.keys(userOptions).length > 0) {
      try {
        await attempt({ ...initialOptions, ...userOptions });
        setUserConfKeys(new Set(Object.keys(userOptions)));
        return true;
      } catch (e) {
        player.confError = String(e);
        console.warn('mpv refused something from mpv.conf; starting without it:', e);
      }
    }

    try {
      await attempt(initialOptions);
      return true;
    } catch (e) {
      player.initError = (await libmpvLoadError()) ?? String(e);
      return false;
    }
  };

  if (!(await start())) return unlisteners;
  player.ready = true;
  // Said out loud rather than left in the console: the viewer's settings are
  // silently not in force, and the alternative to a popup is finding out weeks
  // later that the subtitle size in that file never applied.
  if (player.confError) showOsd(t('osd.conf_ignored'), { sub: player.confError });
  // The user's mpv.conf is merged over `initialOptions`, so a `loop-file` line
  // in it would win — and the repeat button would then be lying about what the
  // player is doing. A control with its own persisted state has to be the
  // source of truth for the property behind it, so the mode is re-asserted here.
  applyLoopMode(player.loopMode);
  applyNormalize(loadNormalize());
  applySdrColor(loadSdrColor());

  unlisteners.push(
    await observeProperties(OBSERVED, (ev) => {
      applyProperty(ev);
      // Outside `applyProperty`, and that is the whole reason the two paths can
      // share it: this hook is where the *page* acts on a change — flushing a
      // position, releasing a torrent, resetting the seek probe — and the 1 s
      // sweep must move a stale mirror without pretending the event happened.
      hooks.property?.(ev.name, ev.data as never);
    }),
  );

  unlisteners.push(
    await listenEvents((event) => {
      if (event.event === 'file-loaded') {
        void loadTracks();
        void loadChapters();
        applyPendingAttachments();
        hooks.fileLoaded?.();
      }
      // A local file that fails to open at least leaves the start screen up.
      // A URL fails with no trace at all: nothing loads, nothing changes, and
      // the player looks like it ignored the paste.
      if (event.event === 'end-file' && event.reason === 'error') {
        hooks.loadFailed?.();
      }
      if (event.event === 'playback-restart') hooks.playbackRestart?.();
      if (event.event === 'queue-overflow') {
        void resyncState();
        void loadTracks();
        void loadChapters();
      }
    }),
  );

  // mpv sends the initial value of every observed property the moment it is
  // observed — and that happens inside init(), before the listener above
  // exists. Tauri does not queue events for late subscribers, so the whole
  // initial burst is lost: without this resync the mirrors kept their JS
  // defaults (pause, volume, loop, vo-configured) until the page's 1 s resync
  // timer made the first correction — visibly late.
  void resyncState();

  return unlisteners;
}

/// External tracks waiting for the file they belong to. `sub-add` before
/// `file-loaded` fails, so a drop of "movie + subtitles" has to park the
/// subtitles until the movie is actually open.
let pendingAttach: { kind: 'sub' | 'audio'; path: string }[] = [];

function applyPendingAttachments() {
  if (!pendingAttach.length) return;
  const waiting = pendingAttach;
  pendingAttach = [];
  void (async () => {
    for (const item of waiting) await attachTrack(item.kind, item.path);
  })();
}

/**
 * Several files become an mpv playlist: the first plays, the rest queue.
 * Subtitle and audio files are separated out and attached to the video instead
 * — the whole set usually arrives in one drop.
 */
export async function loadFiles(paths: string[]) {
  if (paths.length === 0) return;
  const videos: string[] = [];
  const subs: string[] = [];
  const audios: string[] = [];
  const torrents: string[] = [];
  for (const path of paths) {
    const ext = extensionOf(path);
    if (SUBTITLE_EXTENSIONS.includes(ext)) subs.push(path);
    else if (AUDIO_EXTENSIONS.includes(ext)) audios.push(path);
    else if (ext === 'torrent') torrents.push(path);
    else videos.push(path);
  }

  // A `.torrent` is a whole session rather than a file to play, so it is
  // handled here — where a drop is already sorted by extension — instead of at
  // each of the four entry points (drop, picker, argv, the Apple Event). Only
  // when nothing else in the drop is playable: a folder holding both a film and
  // the torrent it came from should play the film, which is the more certain
  // intent, and only one torrent can be opened at a time anyway.
  if (torrents.length && !videos.length) {
    hooks.openTorrentFile?.(torrents[0]);
    return;
  }

  // Audio with nothing to attach it to is a file to play, not a track to add —
  // dropping an album on an idle player should start it.
  if (!videos.length && !player.hasFile && audios.length) {
    videos.push(...audios.splice(0));
  }
  if (!videos.length) {
    if (!player.hasFile) {
      showOsd(t('osd.attach_no_file'));
      return;
    }
    for (const path of subs) await attachTrack('sub', path);
    for (const path of audios) await attachTrack('audio', path);
    return;
  }

  await hooks.beforeLoad?.(videos[0]);
  pendingAttach = [
    ...subs.map((path) => ({ kind: 'sub' as const, path })),
    ...audios.map((path) => ({ kind: 'audio' as const, path })),
  ];
  await command('loadfile', [videos[0]]);
  for (const p of videos.slice(1)) {
    await command('loadfile', [p, 'append']);
  }
  await setProperty('pause', false);
  hooks.filesOpened?.(videos);
}

/** File picker for videos. Returns once the selection has been handed to mpv. */
export async function pickAndLoadFiles() {
  const sel = await open({
    multiple: true,
    filters: [
      // `.torrent` rides in the first filter rather than getting one of its own:
      // it is another way to arrive at a video, and a picker that makes you
      // choose "видео" or "торренты" before you can see your file is asking you
      // to classify what you are looking for before you have found it.
      { name: t('dialog.video'), extensions: [...VIDEO_EXTENSIONS, 'torrent'] },
      { name: t('dialog.all_files'), extensions: ['*'] },
    ],
  });
  const paths = Array.isArray(sel) ? sel : sel ? [sel] : [];
  if (paths.length) await loadFiles(paths);
}
