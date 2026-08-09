/**
 * mpv state mirrors and the commands that drive them.
 *
 * Everything here is "what mpv is doing"; nothing here knows about the seekbar,
 * the OSC or any gesture. The page wires the two together through `initPlayer`
 * hooks rather than this module reaching back into it — the observer callback
 * used to be the place where every concern in the app met.
 *
 * **The mirrors go stale.** Heavy seeking overflows mpv's client event queue and
 * property-change events are dropped (`queue-overflow`). Two rules follow, and
 * both are load-bearing: the target of a toggle is never computed from a mirror
 * (mpv inverts its own state via `cycle`), and the mirrors are resynced on a
 * timer the page owns.
 */

import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import {
  command,
  getProperty,
  init,
  listenEvents,
  observeProperties,
  setProperty,
  type MpvConfig,
  type MpvEventFromProperties,
  type MpvObservableProperty,
} from 'tauri-plugin-libmpv-api';

import { baseName, displayName, extensionOf, formatTime } from './format';
import { t } from './i18n.svelte';
import { LANG_ALIASES } from './languages';
import { latest } from './latest';
import { showOsd } from './osd.svelte';
import { IS_MAC } from './platform';

/// Amplification ceiling, in percent. Above 100 the player is amplifying and
/// can clip, so this stays well short of mpv's 1000 maximum. mpv's own default
/// is 130.
export const VOLUME_MAX = 150;

/// Step for the subtitle/audio delay nudges, in seconds. mpv's own is the same.
export const DELAY_STEP = 0.1;

export const VIDEO_EXTENSIONS = [
  'mp4', 'mkv', 'avi', 'mov', 'webm', 'wmv', 'flv', 'ts', 'm2ts', 'mts',
  'mpg', 'mpeg', 'vob', 'ogv', 'm4v', '3gp', 'rm', 'rmvb', 'y4m',
];

// Dropping one of these attaches a track to the current file instead of trying
// to play it: an .srt handed to `loadfile` used to be opened as if it were a movie.
export const SUBTITLE_EXTENSIONS = [
  'srt', 'ass', 'ssa', 'sub', 'idx', 'vtt', 'sup', 'smi', 'mks',
];
export const AUDIO_EXTENSIONS = [
  'mp3', 'aac', 'flac', 'wav', 'ogg', 'opus', 'm4a', 'ac3', 'eac3', 'dts',
  'mka', 'wma', 'alac', 'aiff',
];

/**
 * A source mpv can open that is not a file on this machine.
 *
 * The distinction earns its own helper because a surprising amount of the
 * player is quietly local-only: the thumbnail storyboard decodes the whole file
 * in the background, the queue is built by reading a directory, "show in
 * Finder" needs something to show. Run any of those against a URL and they
 * either fail confusingly or, worse, work — by pulling the entire stream down a
 * second time.
 *
 * Scheme rather than a regex: `://` shows up in file names often enough, and
 * mpv itself dispatches on the scheme.
 */
const NETWORK_SCHEME = /^(?:https?|rtmps?|rtsp|srt|udp|mmsh?|magnet):/i;

export function isNetworkSource(path: string | null): boolean {
  return !!path && NETWORK_SCHEME.test(path.trim());
}

export interface Track {
  id: number;
  selected: boolean;
  label: string;
  /// Raw fields, kept alongside the label because they are what identifies
  /// "the same track" in a DIFFERENT file — see TrackDesc.
  lang: string | null;
  title: string | null;
  codec: string | null;
  forced: boolean;
  /// Added by `sub-add`/`audio-add` (or found by `sub-auto`) rather than living
  /// in the container. **This is what decides whether a track can be removed at
  /// all**: mpv's `sub-remove` works on external files only, and an embedded
  /// track could not be taken out without rewriting the video file — which is
  /// not something a player does.
  external: boolean;
  /// The external file behind it, when there is one.
  path: string | null;
  /// Channel count from the demuxer, audio tracks only. The cast prepare rung
  /// reads it to decide whether an E-AC-3 transcode needs a 5.1 fold-down
  /// (above 6 channels) — and, crucially, that a stereo source must not get
  /// one, which would be an upmix.
  channels: number | null;
}

/**
 * What a chosen track was, in terms that survive into the next episode.
 *
 * Deliberately not the id: track ids are positions inside one file, and the
 * Russian dub that is #2 in episode 1 is routinely #3 in episode 2. The index
 * is kept anyway, but only to break ties between otherwise equal candidates.
 */
export interface TrackDesc {
  lang: string | null;
  title: string | null;
  codec: string | null;
  forced: boolean;
  index: number;
}

/// A remembered choice: a track to look for, or the explicit "none".
export type TrackWish = TrackDesc | 'no';

/**
 * Repeat, as music services shape it: off → everything → this one.
 *
 * Two mpv properties, and `all` is `loop-playlist` alone — on a one-entry
 * playlist that already means "repeat this video", so the state needs no
 * special case to cover a single open file.
 *
 * The mode is OUR state, not a reading of the mirrors, and it is written to mpv
 * rather than cycled there. That looks like a violation of the rule that
 * toggles must go through `cycle`, but the rule guards against computing a
 * target from a mirror that a dropped property-change may have left stale —
 * here the target comes from a value mpv never writes to.
 */
export type LoopMode = 'off' | 'all' | 'one';

export interface Chapter {
  /// Position in `chapter-list`, which is also what the `chapter` property holds.
  index: number;
  /// Start time in seconds.
  time: number;
  /// As written in the container; null when it has no title. Kept raw rather
  /// than pre-formatted, so switching the interface language renames the
  /// nameless ones (see `chapterTitle`).
  title: string | null;
}

const OBSERVED = [
  ['pause', 'flag'],
  ['time-pos', 'double', 'none'],
  ['duration', 'double', 'none'],
  ['filename', 'string', 'none'],
  ['path', 'string', 'none'],
  ['media-title', 'string', 'none'],
  ['volume', 'double'],
  ['mute', 'flag'],
  ['speed', 'double'],
  ['loop-file', 'string', 'none'],
  ['eof-reached', 'flag', 'none'],
  ['playlist-pos', 'int64'],
  ['playlist-count', 'int64'],
  ['dwidth', 'int64', 'none'],
  ['dheight', 'int64', 'none'],
  ['sub-delay', 'double', 'none'],
  ['audio-delay', 'double', 'none'],
  // The VO has been configured and paints its field (black before the first
  // frame, thanks to force-window). The page keeps an opaque fill over the
  // transparent window until this reports true — a timer was a guess that
  // lost the race on heavy files and flashed the desktop.
  ['vo-configured', 'flag'],
  // Tracks do not all appear at once: by the time file-loaded fires there may
  // be no subtitle tracks yet (external files via sub-auto, lazily parsed
  // containers), so reading the list once on that event is not enough — the
  // subtitles button stayed missing until something else refreshed the list.
  // The counter is what we observe: `track-list` itself is a node, and
  // observing node properties crashes the process (see architecture.md).
  ['track-list/count', 'int64', 'none'],
  // Same story for chapters, and for the same reason: the list is not final at
  // `file-loaded` for every container, and `chapter-list` itself is a node.
  ['chapter-list/count', 'int64', 'none'],
  ['chapter', 'int64', 'none'],
  // Picture geometry. Observed rather than read when the menu opens, so the
  // menu can tick the value that is actually in force — including one that came
  // from the user's own mpv.conf.
  // A–B loop. As STRINGS: an unset point reads back as the literal "no", and
  // asking for a double there gives nothing at all (measured).
  // Network buffering. `cache-buffering-state` is the fill of the initial cache
  // as a percentage; `paused-for-cache` is playback actually stalled waiting
  // for it. Both are absent for a local file, which is how the UI knows not to
  // say anything.
  ['cache-buffering-state', 'int64', 'none'],
  ['paused-for-cache', 'flag', 'none'],
  ['ab-loop-a', 'string', 'none'],
  ['ab-loop-b', 'string', 'none'],
  ['video-rotate', 'int64', 'none'],
  ['video-aspect-override', 'double', 'none'],
  ['panscan', 'double', 'none'],
] as const satisfies ReadonlyArray<MpvObservableProperty>;

export type ObservedName = (typeof OBSERVED)[number][0];

/// One property-change report, narrowed to what `OBSERVED` can produce — so the
/// switch in `applyProperty` gets `data` typed per property rather than as
/// `unknown`.
type PropertyChange = MpvEventFromProperties<(typeof OBSERVED)[number]>;

/**
 * Which mirrors the 1 s sweep re-reads, and it is a `Record<ObservedName, …>`
 * on purpose: **a newly observed property is a compile error here until
 * somebody says whether losing its event is survivable.**
 *
 * That question has a bad answer often enough to be worth forcing. mpv's event
 * queue overflows and drops property changes (gotcha 3), and the mirrors split
 * cleanly in two: the ones something re-reports anyway — a position, a
 * buffering percentage, anything mpv sends again on the next file — and the
 * ones that are simply *set* and then stand. A dropped event of the second kind
 * does not heal on the next tick; it stays wrong until the viewer touches the
 * thing again, which for a check mark in a menu can be never.
 *
 * `playlist-pos` is the one that proved it, and it was left out of a
 * hand-written list, which is why this is no longer a hand-written list.
 */
const RESYNC: Record<ObservedName, boolean> = {
  pause: true,
  volume: true,
  mute: true,
  speed: true,
  'vo-configured': true,
  'eof-reached': true,
  'sub-delay': true,
  'audio-delay': true,
  chapter: true,
  // Everything about *neighbours* is computed from this — the end screen's two
  // cards, the auto-advance, the skip button's "next episode" offer — and while
  // it is wrong, "the next entry" resolves to the file already playing.
  'playlist-pos': true,
  'playlist-count': true,
  // The A–B marks and the picture geometry are pure "set once and stand". The
  // picture is the sharper case: `setPicture` writes the property and does *not*
  // write the mirror, so these three depend entirely on the event coming back —
  // lose it and the context menu ticks the value the film had before.
  'ab-loop-a': true,
  'ab-loop-b': true,
  'video-rotate': true,
  'video-aspect-override': true,
  panscan: true,
  // A stall is entered and left by one event each, and it is the *leaving* one
  // that matters: `player.stalled` puts "waiting for data" in the top bar, so a
  // dropped `false` leaves that sentence standing over playing video with
  // nothing left to take it down.
  'paused-for-cache': true,

  // Re-reported by mpv on its own, so a dropped one costs a tick at most.
  'time-pos': false,
  duration: false,
  'cache-buffering-state': false,
  'dwidth': false,
  'dheight': false,
  // The identity of what is open. Deliberately NOT swept, and the reason is the
  // sweep's own shape rather than the value's: `resyncState` writes mirrors
  // without calling the `property` hook, and for these three the hook is where
  // the actual work lives — flushing the position, dropping posters, releasing
  // a torrent, resetting the seek probe, recording the title. Re-reading them
  // here would move the mirror and leave every one of those undone, which is a
  // worse state than the stale one it was meant to repair.
  filename: false,
  path: false,
  'media-title': false,
  // Ours, written to mpv and never read back: reading it would make `all` on a
  // one-entry playlist look like `one` and flip the button under the viewer.
  'loop-file': false,
  // Not mirrors at all — these are triggers for a list re-read, and sweeping
  // them would re-read the track and chapter lists once a second. A queue
  // overflow calls `loadTracks`/`loadChapters` directly instead.
  'track-list/count': false,
  'chapter-list/count': false,
};

class Player {
  ready = $state(false);
  initError = $state<string | null>(null);

  /// False, matching mpv: pause is never set at init, so an idle player is
  /// unpaused. The old `true` showed a Play icon over already-running video
  /// until the first resync corrected it.
  paused = $state(false);
  timePos = $state(0);
  duration = $state(0);
  filename = $state<string | null>(null);
  filePath = $state<string | null>(null);
  mediaTitle = $state<string | null>(null);
  volume = $state(100);
  muted = $state(false);
  speed = $state(1);
  subDelay = $state(0);
  audioDelay = $state(0);
  loopMode = $state<LoopMode>('off');
  eofReached = $state(false);
  playlistPos = $state(0);
  playlistCount = $state(0);

  /// dwidth/dheight — the video size including aspect.
  videoW = $state(0);
  videoH = $state(0);

  /// vo-configured — the VO exists and is painting (its field is black until
  /// the first frame). The page latches its dark backdrop off on this.
  voConfigured = $state(false);

  audioTracks = $state<Track[]>([]);
  subTracks = $state<Track[]>([]);

  /// Fill of the network cache, 0..100, or null when there is no cache — which
  /// is the normal state for a local file.
  cacheBuffering = $state<number | null>(null);
  /// Playback is stopped waiting for the network, not by the viewer.
  stalled = $state(false);

  /// A–B loop points in seconds, or null when unset.
  loopA = $state<number | null>(null);
  loopB = $state<number | null>(null);

  get hasAbLoop(): boolean {
    return this.loopA !== null && this.loopB !== null;
  }

  /// Volume normalization (a labeled `af` filter). Ours to remember: mpv
  /// answers `af` with an escaped form that cannot be compared against.
  normalize = $state(false);

  /// Picture geometry, per file. `aspectOverride` is negative for "from the
  /// container" — mpv's default is -2, and any negative value means auto.
  videoRotate = $state(0);
  aspectOverride = $state(-2);
  panscan = $state(0);

  chapters = $state<Chapter[]>([]);
  /// Index of the chapter being played, or -1 when the file has none.
  chapterIndex = $state(-1);

  /// Path of the user's mpv.conf, for the settings dialog footer.
  mpvConfPath = $state<string | null>(null);

  get hasFile(): boolean {
    return this.filename !== null;
  }

  /// A single chapter spanning the whole file is what containers write when
  /// they have nothing to say — there is nothing to navigate there.
  get hasChapters(): boolean {
    return this.chapters.length > 1;
  }

  get displayTitle(): string {
    // `media-title` is the container's own, or what yt-dlp resolved for a link.
    // Failing that the file name — tidied, because for a shared YouTube link
    // mpv's `filename` is the video id with its tracking query attached.
    if (this.mediaTitle) return this.mediaTitle;
    return this.filename ? displayName(this.filename) : 'Frame Player';
  }
}

export const player = new Player();

/**
 * When `time-pos` last arrived, on the monotonic clock.
 *
 * Deliberately NOT `$state`: mpv reports the position several times a second,
 * and a reactive timestamp would re-run every effect that reads it at that rate
 * for no benefit — nothing renders from it.
 */
let timePosAt = 0;

/**
 * Where playback is *now*, rather than where it was when mpv last said so.
 *
 * `player.timePos` is a mirror of an event, so by the time anything reads it, it
 * is as old as the gap since that event — tens of milliseconds, and more on a
 * busy machine. For anything the viewer sees that is irrelevant; for comparing
 * this player against a shared timeline it is a systematic error in a
 * measurement whose whole job is to be small, and it sat inside the drift
 * correction's own deadband where it could never be observed.
 *
 * Extrapolated at the current *playback* speed, which is the right rate even
 * while drift correction is bending it: the question is where the film has got
 * to, not where it was supposed to.
 */
export function positionNow(): number {
  if (player.paused || player.eofReached || !timePosAt) return player.timePos;
  const ahead = (performance.now() - timePosAt) / 1000;
  // A mirror that has not been refreshed for a second is not stale, it is
  // wrong — playback stalled, or the event queue overflowed — and extrapolating
  // across it would invent a position. The resync sweep runs at 1 s, so this is
  // the point past which something else is going on.
  if (ahead > 1) return player.timePos;
  return player.timePos + ahead * player.speed;
}

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
 * Where yt-dlp is, and whether it is ours.
 *
 * `managed` decides what the UI may offer: our own copy in the app data
 * directory can be updated in place (that is why it lives there), while one
 * from Homebrew or pip belongs to its package manager and `-U` there would
 * either refuse or fight it.
 */
class Ytdlp {
  path = $state<string | null>(null);
  managed = $state(false);
  version = $state<string | null>(null);

  get present(): boolean {
    return this.path !== null;
  }
}

export const ytdlp = new Ytdlp();

export async function refreshYtdlp() {
  const status = await invoke<{ path: string | null; managed: boolean; version: string | null }>(
    'ytdlp_status',
  ).catch(() => null);
  ytdlp.path = status?.path ?? null;
  ytdlp.managed = status?.managed ?? false;
  ytdlp.version = status?.version ?? null;
}

/// Tell mpv where it is. Set rather than passed at init when the binary arrives
/// later — installing it mid-session must not need a restart.
export function applyYtdlpPath() {
  if (!ytdlp.path) return;
  void command('set', ['script-opts', `ytdl_hook-ytdl_path=${ytdlp.path}`]).catch(() => {});
}

export function hasYtdlp(): boolean {
  return ytdlp.present;
}

/**
 * Boots mpv and subscribes to it. Returns the teardown callbacks.
 *
 * The caller shows the window afterwards: `force-window=yes` gives mpv a black
 * field before the first frame, and the window is transparent until then.
 */
/**
 * Resolve when mpv reports that playback has actually restarted, or give up
 * after `ms`.
 *
 * `command('seek', …)` resolves when mpv *accepts* the command, not when it has
 * performed it, and `time-pos` at that moment is still the position before the
 * jump. `playback-restart` is the event that means the seek is done. The
 * timeout is there so a seek that never completes cannot leave a caller waiting
 * on a promise for ever.
 */
export function waitPlaybackSettled(ms: number): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, ms);
    playbackRestartWaiters.push(() => {
      clearTimeout(timer);
      resolve();
    });
  });
}

let playbackRestartWaiters: Array<() => void> = [];

/// Called from the `playbackRestart` hook: releases everyone waiting above.
export function notePlaybackRestart() {
  const waiting = playbackRestartWaiters;
  playbackRestartWaiters = [];
  for (const w of waiting) w();
}

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
    'osc': 'no',
    'hr-seek': 'yes',
    'demuxer-max-back-bytes': '512MiB',
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
    // A path list, so the separator is the platform's own.
    'sub-file-paths': ['Subs', 'subs', 'Subtitles', 'subtitles'].join(IS_MAC ? ':' : ';'),
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

  // The user's mpv.conf (%APPDATA%/<app>/mpv.conf) goes on top of the defaults,
  // mpv-style: the last line wins. Applied at startup.
  try {
    const conf = await invoke<{ path: string; options: [string, string][] }>('user_mpv_conf');
    player.mpvConfPath = conf.path;
    for (const [k, v] of conf.options) initialOptions[k] = v;
  } catch (e) {
    console.warn('user_mpv_conf failed:', e);
  }

  const mpvConfig: MpvConfig = { initialOptions, observedProperties: OBSERVED };

  try {
    await init(mpvConfig);
  } catch (e) {
    player.initError = String(e);
    return unlisteners;
  }
  player.ready = true;
  // The user's mpv.conf is merged over `initialOptions`, so a `loop-file` line
  // in it would win — and the repeat button would then be lying about what the
  // player is doing. A control with its own persisted state has to be the
  // source of truth for the property behind it, so the mode is re-asserted here.
  applyLoopMode(player.loopMode);
  applyNormalize(loadNormalize());

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

/**
 * Write one property-change into its mirror.
 *
 * **Both paths into the mirrors go through here**: the observer, and the 1 s
 * sweep below. They used to be two hand-written copies of the same mapping, and
 * that is the shape the `playlist-pos` bug had — not a wrong line, a missing
 * one, in the copy nobody reads while writing the other.
 */
function applyProperty(ev: PropertyChange) {
  switch (ev.name) {
    case 'pause': player.paused = ev.data; break;
    case 'time-pos':
      player.timePos = ev.data ?? 0;
      timePosAt = performance.now();
      break;
    case 'duration': player.duration = ev.data ?? 0; break;
    case 'filename': player.filename = ev.data; break;
    case 'path': player.filePath = ev.data; break;
    case 'media-title': player.mediaTitle = ev.data; break;
    case 'volume': player.volume = ev.data; break;
    case 'mute': player.muted = ev.data; break;
    case 'speed': player.speed = ev.data; break;
    case 'sub-delay': player.subDelay = ev.data ?? 0; break;
    case 'audio-delay': player.audioDelay = ev.data ?? 0; break;
    // The mode is ours; mpv is only told about it. Reading it back would
    // make `all` on a one-entry playlist (which also sets loop-file) look
    // like `one` and flip the button under the viewer.
    case 'loop-file': break;
    case 'eof-reached': player.eofReached = ev.data ?? false; break;
    case 'playlist-pos': player.playlistPos = ev.data; break;
    case 'playlist-count': player.playlistCount = ev.data; break;
    case 'dwidth': player.videoW = ev.data ?? 0; break;
    case 'dheight': player.videoH = ev.data ?? 0; break;
    case 'vo-configured': player.voConfigured = ev.data; break;
    case 'track-list/count': void loadTracks(); break;
    case 'chapter-list/count': void loadChapters(); break;
    case 'chapter': player.chapterIndex = ev.data ?? -1; break;
    case 'cache-buffering-state': player.cacheBuffering = ev.data; break;
    case 'paused-for-cache': player.stalled = ev.data ?? false; break;
    case 'ab-loop-a': player.loopA = parseAbPoint(ev.data); break;
    case 'ab-loop-b': player.loopB = parseAbPoint(ev.data); break;
    case 'video-rotate': player.videoRotate = ev.data ?? 0; break;
    case 'video-aspect-override': player.aspectOverride = ev.data ?? -2; break;
    case 'panscan': player.panscan = ev.data ?? 0; break;
  }
}

/**
 * Re-read the mirrors a dropped property-change could have left stale.
 *
 * Called on a timer and after a queue overflow. What it sweeps is `RESYNC`, and
 * where it puts the value is `applyProperty` — so neither the set nor the
 * assignment is written twice, and the two ways to get this wrong (forget a
 * property, or mirror it into the wrong field) are both gone.
 *
 * One property failing no longer ends the sweep. It used to: the reads sat in a
 * single `try`, so an unavailable property early in the list — routine between
 * files — silently skipped every mirror after it.
 */
export async function resyncState() {
  if (!player.ready) return;
  for (const [name, format, missing] of OBSERVED) {
    if (!RESYNC[name]) continue;
    let data: unknown;
    try {
      data = await getProperty(name, format);
    } catch {
      // Unavailable. A property declared `none` says so with a null and its
      // own fallback below is the right answer (an absent `chapter` really is
      // "no chapter"); anything else keeps what it had rather than being told
      // the file has no volume.
      if (missing !== 'none') continue;
      data = null;
    }
    // The pair is correlated by construction — `format` came off the same row
    // as `name` — but that is not something the checker can follow across a
    // union of tuples, so the correlation is asserted once, here, instead of
    // being re-stated per property in a table that could drift.
    applyProperty({ event: 'property-change', id: 0, name, data } as PropertyChange);
  }
}

// ---- Playback -------------------------------------------------------------

/** Play/pause. Restarts from the beginning when the file has already ended. */
export async function togglePause() {
  if (!player.hasFile) return;
  // A fresh read rather than the mirror: eof decides between "resume" and
  // "start over", and getting it wrong is very visible.
  const eof = (await getProperty('eof-reached', 'flag').catch(() => false)) ?? false;
  if (eof) {
    await command('seek', [0, 'absolute']);
    await setProperty('pause', false);
  } else {
    await command('cycle', ['pause']);
  }
  void resyncState();
}

export function setVolume(v: number): number {
  const clamped = Math.max(0, Math.min(VOLUME_MAX, Math.round(v)));
  void setProperty('volume', clamped);
  localStorage.setItem('frameplayer.volume', String(clamped));
  return clamped;
}

export function toggleMute() {
  void setProperty('mute', !player.muted);
  showOsd(t(player.muted ? 'osd.sound_on' : 'osd.sound_off'));
}

// ---- Repeat ---------------------------------------------------------------

const LOOP_KEY = 'frameplayer.loop';

/**
 * The stored mode, migrated from the two-state era.
 *
 * The old key held mpv's own `inf`/`no` for `loop-file`, and it was only ever
 * written by the toggle — so its ABSENCE means "never touched", which used to
 * read as `inf` through a `?? 'inf'` default. That default is what made an
 * untouched player repeat the current file forever, and therefore never reach
 * the end of a playlist entry: with the queue arriving, off is the only sane
 * starting point. An explicit `inf` still becomes `one`, because that was a
 * deliberate choice to repeat the file.
 */
function loadLoopMode(): LoopMode {
  const saved = localStorage.getItem(LOOP_KEY);
  if (saved === 'off' || saved === 'all' || saved === 'one') return saved;
  if (saved === 'inf') return 'one';
  return 'off';
}

/// mpv options for a mode. `all` with a single entry in the playlist is the
/// same thing as `one` — `loop-playlist` restarts a one-entry playlist — so
/// nothing here has to special-case an open file with no queue behind it.
function loopOptions(mode: LoopMode): { 'loop-file': string; 'loop-playlist': string } {
  return {
    'loop-file': mode === 'one' ? 'inf' : 'no',
    'loop-playlist': mode === 'all' ? 'inf' : 'no',
  };
}

export function applyLoopMode(mode: LoopMode) {
  player.loopMode = mode;
  const opts = loopOptions(mode);
  void setProperty('loop-file', opts['loop-file']).catch(() => {});
  void setProperty('loop-playlist', opts['loop-playlist']).catch(() => {});
  try {
    localStorage.setItem(LOOP_KEY, mode);
  } catch {
    // not critical: the choice simply will not survive a restart
  }
}

/// off → all → one → off, the order every music service uses.
export function cycleLoop() {
  const next: LoopMode =
    player.loopMode === 'off' ? 'all' : player.loopMode === 'all' ? 'one' : 'off';
  applyLoopMode(next);
  showOsd(t(next === 'off' ? 'osd.loop_off' : next === 'all' ? 'osd.loop_all' : 'osd.loop_one'));
}

/// Returns what it settled on, so a caller that has to report the change —
/// `playback.changeSpeed`, which tells a shared room — does not have to repeat
/// the clamping and rounding and end up disagreeing with what mpv was given.
export function changeSpeed(factor: number): number {
  const next = Math.round(Math.max(0.25, Math.min(4, player.speed * factor)) * 100) / 100;
  void setProperty('speed', next);
  showOsd(t('osd.speed', { value: next }), { progress: (next - 0.25) / 3.75 });
  return next;
}

// ---- Subtitle / audio delay ----------------------------------------------
// `add` rather than `set` for the same reason toggles use `cycle`: mpv owns the
// value, and the local mirror can be stale after a queue overflow.

/**
 * Delays are dialled in with mpv's `add`, which is floating point: ten presses
 * forward and ten back leave **2.7755575615628914e-17**, not zero. Everything
 * downstream has to agree on what zero means or the residue leaks in two ways,
 * both of which it did — the reset button stayed enabled over a readout saying
 * `+0,00 с`, and `rememberDelay` stores a record whenever the value is not
 * `=== 0`, so a delay that had been dialled back to nothing was written to disk
 * and re-applied to that file for ever.
 *
 * Half of the last digit shown: anything the readout rounds to zero *is* zero,
 * which keeps the button, the store and the text answering the same question.
 */
const DELAY_EPSILON = 0.005;

export function delayIsZero(seconds: number): boolean {
  return Math.abs(seconds) < DELAY_EPSILON;
}

/// mpv's value, snapped to what the player is willing to say about it. Used
/// before anything is written down — see `DELAY_EPSILON`.
export function roundDelay(seconds: number): number {
  return delayIsZero(seconds) ? 0 : Math.round(seconds * 100) / 100;
}

export function formatDelay(seconds: number): string {
  const rounded = roundDelay(seconds);
  return t('osc.delay_value', { value: (rounded > 0 ? '+' : '') + rounded.toFixed(2) });
}

function delayOsd(kind: 'sub' | 'audio', value: number) {
  showOsd(
    t(kind === 'sub' ? 'osd.sub_delay' : 'osd.audio_delay', {
      value: (value > 0 ? '+' : '') + (Math.round(value * 100) / 100).toFixed(2),
    }),
  );
}

export function nudgeDelay(kind: 'sub' | 'audio', delta: number) {
  if (!player.hasFile) return;
  void command('add', [kind === 'sub' ? 'sub-delay' : 'audio-delay', delta]).catch(() => {});
  // Optimistic, like the seek popup: the property change arrives a frame or two
  // later, and a popup that lags one step behind reads as a stuck control.
  delayOsd(kind, (kind === 'sub' ? player.subDelay : player.audioDelay) + delta);
}

export function resetDelay(kind: 'sub' | 'audio') {
  void setProperty(kind === 'sub' ? 'sub-delay' : 'audio-delay', 0);
  delayOsd(kind, 0);
}

// ---- List properties ------------------------------------------------------

/**
 * Reads a list property through its sub-properties.
 *
 * IMPORTANT: the 'node' format in libmpv-wrapper 0.1.1 is broken (it writes an
 * mpv_node into a pointer variable -> stack corruption -> ACCESS_VIOLATION), so
 * a list is never read as a whole. Every list property in mpv is implemented
 * through the same helper on its side, which exposes `<prop>/count` and
 * `<prop>/N/<field>` in simple formats — so `chapter-list` and `playlist` are
 * readable exactly like `track-list`, and none of them needs a node.
 *
 * Entries the reader rejects (a missing mandatory field, a type we do not care
 * about) drop out; the caller keeps the sequence guard, because a read is dozens
 * of sequential round-trips and there are several reasons to start one.
 */
export async function readList<T>(
  prop: string,
  readItem: (base: string, index: number) => Promise<T | null>,
): Promise<T[]> {
  const count = (await getProperty(`${prop}/count`, 'int64').catch(() => null)) ?? 0;
  const items: T[] = [];
  for (let i = 0; i < count; i++) {
    const item = await readItem(`${prop}/${i}`, i);
    if (item) items.push(item);
  }
  return items;
}

// ---- Tracks ---------------------------------------------------------------

/// Bumped on every read of the track list. A read is dozens of sequential
/// getProperty calls, there are several reasons to start one (file-loaded, a
/// track-list/count change, opening the menu, switching a track), and the
/// counter changes several times in a row as tracks keep appearing. Without the
/// marker an earlier read can finish last and overwrite a fresh list.
const trackReads = latest();

export async function loadTracks() {
  const run = trackReads.begin();
  try {
    const count = (await getProperty('track-list/count', 'int64')) ?? 0;
    const audio: Track[] = [];
    const subs: Track[] = [];
    for (let i = 0; i < count; i++) {
      const base = `track-list/${i}`;
      const type = await getProperty(`${base}/type`, 'string').catch(() => null);
      if (type !== 'audio' && type !== 'sub') continue;
      const id = (await getProperty(`${base}/id`, 'int64').catch(() => null)) ?? 0;
      const selected = (await getProperty(`${base}/selected`, 'flag').catch(() => false)) ?? false;
      const title = await getProperty(`${base}/title`, 'string').catch(() => null);
      const lang = await getProperty(`${base}/lang`, 'string').catch(() => null);
      // Closed captions carry no title and no language, so they used to show up
      // as a bare "Track 1" — with no hint that picking it means captions
      // positioned on a 32-column grid (hence the off-center, indented text)
      // that also smear across seeks. Naming them is the honest minimum.
      const codec = await getProperty(`${base}/codec`, 'string').catch(() => null);
      const forced = (await getProperty(`${base}/forced`, 'flag').catch(() => false)) ?? false;
      const named = [title, lang].filter(Boolean);
      const label =
        codec === 'eia_608' || codec === 'eia_708'
          ? [t('track.cc'), ...named].join(' · ')
          : named.join(' · ') || t('track.generic', { id });
      const external = (await getProperty(`${base}/external`, 'flag').catch(() => false)) ?? false;
      const path = external
        ? await getProperty(`${base}/external-filename`, 'string').catch(() => null)
        : null;
      const channels =
        type === 'audio'
          ? await getProperty(`${base}/demux-channel-count`, 'int64').catch(() => null)
          : null;
      (type === 'audio' ? audio : subs).push({
        id,
        selected,
        label,
        lang,
        title,
        codec,
        forced,
        external,
        path,
        channels,
      });
    }
    if (run.stale) return;
    player.audioTracks = audio;
    player.subTracks = subs;
  } catch {
    if (run.stale) return;
    player.audioTracks = [];
    player.subTracks = [];
  }
}

/**
 * ISO-639-2 to -1 for the languages that actually turn up in releases.
 *
 * MKV stores three-letter codes, plenty of files use two — and comparing by
 * prefix is not enough, because `jpn` and `ja` share only one letter. Anything
 * unlisted falls through lower-cased, which still matches like against like.
 *
 * The table now comes from `languages.ts`, so the codes the track scoring
 * compares and the codes the settings picker writes into `alang`/`slang` are
 * one list rather than two that drift. That is a data import with no state and
 * no imports of its own, so it does not bend the "player depends on nothing"
 * rule the *stateful* modules follow.
 */
function normLang(lang: string | null): string | null {
  if (!lang) return null;
  const code = lang.trim().toLowerCase().split(/[-_]/)[0];
  return LANG_ALIASES[code] ?? code;
}

export function describeTrack(track: Track, list: Track[]): TrackDesc {
  return {
    lang: track.lang,
    title: track.title,
    codec: track.codec,
    forced: track.forced,
    index: Math.max(0, list.indexOf(track)),
  };
}

/// Below this, the candidate shares nothing that identifies a track — neither
/// the language nor the title — and guessing from a codec alone is worse than
/// leaving mpv's own `alang`/`slang` choice alone.
const MATCH_FLOOR = 60;

/**
 * Best match for a remembered track in another file's list, with its score.
 *
 * Scored rather than compared for equality, because not one of these fields is
 * guaranteed to be present: releases exist with languages and no titles, with
 * titles and no languages ("Rus"/"Jap" as names), and with two tracks of the
 * same language distinguished only by codec. Equality on any single field
 * would therefore fail on a large share of real files.
 */
export function matchTrack(list: Track[], want: TrackDesc): { track: Track; score: number } | null {
  let best: { track: Track; score: number } | null = null;
  for (const [index, track] of list.entries()) {
    let score = 0;
    const a = normLang(want.lang);
    const b = normLang(track.lang);
    if (a && b && a === b) score += 100;
    if (want.title && track.title && want.title === track.title) score += 60;
    if (want.codec && track.codec === want.codec) score += 12;
    // A forced track is a different thing from full subtitles — one carries the
    // alien dialogue, the other the whole script. Never silently swap them.
    if (want.forced !== track.forced) score -= 50;
    // Position only breaks ties; on its own it means nothing across files.
    if (index === want.index) score += 5;
    if (score >= MATCH_FLOOR && (!best || score > best.score)) best = { track, score };
  }
  return best;
}

export async function selectTrack(kind: 'audio' | 'sub', track: Track | null) {
  try {
    // Through command('set', ...): mpv's string parser is guaranteed to
    // understand choice properties (aid/sid = number | "no" | "auto").
    await command('set', [kind === 'audio' ? 'aid' : 'sid', track ? String(track.id) : 'no']);
    showOsd(track ? track.label : t('osd.subs_off'));
    setTimeout(() => void loadTracks(), 200);
  } catch (e) {
    showOsd(t('osd.track_failed'));
    console.warn('selectTrack failed:', e);
  }
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

export async function attachTrack(kind: 'sub' | 'audio', path: string) {
  try {
    // 'select' makes the new track the active one — dragging a subtitle file
    // onto the window means "show these", not "add them to a list".
    await command(kind === 'sub' ? 'sub-add' : 'audio-add', [path, 'select']);
    showOsd(t(kind === 'sub' ? 'osd.sub_added' : 'osd.audio_added'), { sub: baseName(path) });
    setTimeout(() => void loadTracks(), 200);
  } catch (e) {
    showOsd(t('osd.track_failed'));
    console.warn('attachTrack failed:', e);
  }
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

// ---- A–B loop -------------------------------------------------------------

/// mpv answers an unset loop point with the literal "no", so the mirrors parse
/// rather than cast: `Number('no')` is NaN and would read as a position.
function parseAbPoint(raw: string | null): number | null {
  if (!raw || raw === 'no') return null;
  const value = Number(raw);
  return Number.isFinite(value) ? value : null;
}

/**
 * One key drives the whole feature: mark the start, mark the end, clear.
 *
 * mpv loops the segment by itself once both points exist, so nothing here has
 * to watch the position. B is refused before A — mpv would accept the pair and
 * simply never loop, which looks like the feature is broken rather than like
 * the marks are the wrong way round.
 */
export function cycleAbLoop() {
  if (!player.hasFile) return;
  if (player.loopA === null) {
    const at = player.timePos;
    void command('set', ['ab-loop-a', String(at)]).catch(() => {});
    player.loopA = at;
    showOsd(t('osd.ab_a', { time: formatTime(at) }));
    return;
  }
  if (player.loopB === null) {
    const at = player.timePos;
    if (at <= player.loopA) {
      showOsd(t('osd.ab_after_a'));
      return;
    }
    void command('set', ['ab-loop-b', String(at)]).catch(() => {});
    player.loopB = at;
    showOsd(t('osd.ab_on', { time: formatTime(at - player.loopA) }));
    return;
  }
  clearAbLoop();
}

export function clearAbLoop() {
  if (player.loopA === null && player.loopB === null) return;
  void command('set', ['ab-loop-a', 'no']).catch(() => {});
  void command('set', ['ab-loop-b', 'no']).catch(() => {});
  player.loopA = null;
  player.loopB = null;
  showOsd(t('osd.ab_off'));
}

// ---- Volume normalization -------------------------------------------------

// The stored key keeps its British spelling on purpose, alone in this file.
// It is not a word here, it is an address: renaming it would leave every
// existing installation's volume-leveling setting behind under a name nothing
// reads any more — a silent reset, on upgrade, for a preference the viewer set
// once and would not think to check.
const NORMALIZE_KEY = 'frameplayer.normalise';

/**
 * Night mode: even out quiet dialogue against loud effects.
 *
 * A LABELED filter, so it can be removed again without touching an `af` line
 * the user put in their own mpv.conf — which is also why the setting is kept in
 * localStorage rather than written to mpv.conf like the rest of the playback
 * settings: `mpv_conf_set` replaces the last uncommented line of a key, and for
 * `af` that would silently eat someone's own filter chain.
 *
 * The state cannot be read back from mpv either. `af` answers with its own
 * escaping (measured: `@fpnorm:lavfi=graph=%27%dynaudnorm=f=250:g=15:p=0.9`),
 * so ours is the record — the same arrangement as the repeat mode.
 *
 * Parameters are dynaudnorm's frame length, gaussian window and target peak:
 * gentle enough that music does not visibly pump, strong enough to matter.
 */
const NORMALIZE_FILTER = '@fpnorm:lavfi=[dynaudnorm=f=250:g=15:p=0.9]';

export function loadNormalize(): boolean {
  return localStorage.getItem(NORMALIZE_KEY) === 'on';
}

export function applyNormalize(on: boolean) {
  player.normalize = on;
  // `af remove` on a filter that is not there is harmless, and it is what makes
  // this idempotent enough to call at startup.
  void command('af', [on ? 'add' : 'remove', on ? NORMALIZE_FILTER : '@fpnorm']).catch(() => {});
  try {
    localStorage.setItem(NORMALIZE_KEY, on ? 'on' : 'off');
  } catch {
    // not critical: the choice simply will not survive a restart
  }
}

// ---- Picture geometry -----------------------------------------------------

/// Whether the viewer has touched any of it this session.
///
/// These are global mpv options, so a rotation applied to one phone clip would
/// otherwise still be in force for the next episode. They are reset when a file
/// loads — but only if we were the ones who changed them, or the reset would
/// quietly overwrite a `video-aspect-override` line in the user's mpv.conf on
/// every single file.
let pictureTouched = false;

/// mpv reports an aspect override as a decimal (`4:3` reads back as 1.333333),
/// so the menu has to compare numerically rather than by the string it sent.
export const ASPECT_AUTO = '-2';

/// Repeat state, named for the context menu and the OSC tooltip. Here rather
/// than in either of them because both draw it, and a second copy would be the
/// one that drifts.
export const LOOP_LABEL = { off: 'loop.off', all: 'loop.all', one: 'loop.one' } as const;

export function setPicture(prop: 'video-rotate' | 'video-aspect-override' | 'panscan', value: string) {
  pictureTouched = true;
  void command('set', [prop, value]).catch(() => {});
}

/// Drop the loop when the file changes. mpv keeps the points across a load, and
/// a segment measured in the previous episode would trap playback in a random
/// stretch of the next one — with no visible cause, since the marks would be
/// nowhere near where the viewer put them.
export function resetAbLoop() {
  if (player.loopA === null && player.loopB === null) return;
  void command('set', ['ab-loop-a', 'no']).catch(() => {});
  void command('set', ['ab-loop-b', 'no']).catch(() => {});
  player.loopA = null;
  player.loopB = null;
}

export function resetPicture() {
  if (!pictureTouched) return;
  pictureTouched = false;
  void command('set', ['video-rotate', '0']).catch(() => {});
  void command('set', ['video-aspect-override', ASPECT_AUTO]).catch(() => {});
  void command('set', ['panscan', '0']).catch(() => {});
}

// ---- Chapters -------------------------------------------------------------

/// Same role as `trackReads`: `chapter-list` is not final at `file-loaded` for
/// every container, so there are several reasons to start a read, and a slow
/// earlier one must not finish last and overwrite a fresh list.
const chapterReads = latest();

/// mpv's placeholder for a chapter with no title in the container.
///
/// The two list properties disagree here, so the track pattern cannot simply be
/// copied. Measured on mpv 0.41: `track-list/N/title` of an untitled track is
/// *unavailable* (the read fails and the catch below produces null), while
/// `chapter-list/N/title` is available and reads back as this literal. Left
/// alone it would put an English placeholder in the chapter list of a Russian
/// interface, instead of the numbered fallback.
const MPV_UNNAMED = '(unnamed)';

export async function loadChapters() {
  const run = chapterReads.begin();
  const list = await readList('chapter-list', async (base, index) => {
    // A chapter without a time is not navigable, and mpv does not produce one —
    // but the read can also fail simply because the file went away underneath.
    const time = await getProperty(`${base}/time`, 'double').catch(() => null);
    if (time === null) return null;
    const raw = (await getProperty(`${base}/title`, 'string').catch(() => null))?.trim();
    const title = !raw || raw === MPV_UNNAMED ? null : raw;
    return { index, time, title } satisfies Chapter;
  }).catch(() => [] as Chapter[]);
  if (run.stale) return;
  player.chapters = list;
}

/// Formatted at the call site rather than stored, so nameless chapters are
/// renamed when the interface language changes.
export function chapterTitle(chapter: Chapter): string {
  return chapter.title ?? t('chapter.generic', { n: chapter.index + 1 });
}

/// The chapter containing `time`, or null when the file has none. Used by the
/// seekbar hover: the list is short (tens of entries), so a scan is cheaper
/// than keeping an index in sync with it.
export function chapterAt(time: number): Chapter | null {
  const list = player.chapters;
  for (let i = list.length - 1; i >= 0; i--) {
    if (time >= list[i].time) return list[i];
  }
  return list[0] ?? null;
}

/// Chapters a viewer normally skips. Split by kind, because the button has to
/// name what it skips: "Skip credits" over an opening is worse than no button.
export type SkipKind = 'intro' | 'recap' | 'preview' | 'trailer' | 'credits' | 'ad';

/**
 * Titles that announce a chapter as skippable.
 *
 * Matched against the **whole** title, not searched inside it. A substring
 * search reads "Ending the war for good" as the closing credits and "Opening
 * the vault" as a title sequence — measured against a corpus of real chapter
 * names, that is not a rare edge case but the common one, because these words
 * are ordinary English verbs. Announcement chapters, by contrast, are named
 * exactly what they are.
 *
 * Anchoring also removes the "субтитры" contains "титры" collision for free,
 * and it makes the failure direction the harmless one: an unusual name
 * ("Opening theme (1:45)") simply offers no button, rather than a button
 * appearing over a scene the viewer wanted to watch.
 *
 * The only decorations allowed are the ones rips actually use: a leading index
 * ("01. Intro") and trailing punctuation ("Previously on…").
 */
const SKIP_ALTERNATIVES: ReadonlyArray<readonly [SkipKind, string]> = [
  // Ordered intro-first, which **is no longer load-bearing** and is kept as
  // defense rather than as a rule. It mattered while the patterns were
  // substring searches: "Opening Credits" matched both. Anchoring them to the
  // whole title made the alternatives disjoint — verified by enumerating every
  // title they match from a corpus of their own literals and diffing the
  // verdicts under both orders, which are identical. Re-order freely; but if a
  // pattern ever loses its anchors, this is what stops the pair colliding.
  [
    'intro',
    String.raw`intro(?:duction)?|opening(?:\s+(?:credits|titles?|sequence|theme|song))?|op\s*\d*|main\s+titles?|title\s+sequence|заставка|опенинг|вступление|(?:вступительные|начальные)\s+титры`,
  ],
  // The "on <show name>" tails are the only open-ended alternatives here, and
  // they are bounded twice: by a character budget, and by the word cap in
  // `skipKind`. Note `\b` is useless after a Cyrillic letter (it is ASCII-only
  // in JS, so "ранее в\b" never matched at all) — those need an explicit \s.
  [
    'recap',
    // "re-cap" is the same word hyphenated, and rips spell it both ways; the
    // optional hyphen costs nothing because the pattern is anchored anyway.
    String.raw`re-?cap|previously(?:\s+on(?:\s+.{0,40})?)?|ранее\s+в(?:\s+.{0,40})?|в\s+предыдущих\s+сериях?|краткое\s+содержание|рекап`,
  ],
  [
    'preview',
    String.raw`(?:next\s+episode\s+)?preview|teaser|next\s+time(?:\s+on(?:\s+.{0,40})?)?|анонс|в\s+следующей\s+серии`,
  ],
  // A kind of its own rather than a spelling of "preview", because the kind is
  // only ever read to label the button: an anime episode's preview announces
  // the next episode, while a chapter called "Trailer" on a disc rip is a promo
  // for another film entirely, and "Пропустить анонс" over one of those is the
  // "Skip credits over an opening" mistake in miniature. The button names what
  // it skips, so the pattern that knows the difference keeps it.
  ['trailer', String.raw`trailers?|трейлеры?`],
  [
    'credits',
    String.raw`(?:end|ending|final|closing)?\s*credits|ending(?:\s+(?:song|theme))?|outro|ed\s*\d*|(?:финальные|заключительные|конечные)?\s*титры|концовка|эндинг`,
  ],
  [
    'ad',
    String.raw`ads?|advert(?:isement)?s?|sponsor(?:ed|ship)?|commercials?|реклама|спонсор`,
  ],
];

const SKIP_PATTERNS: ReadonlyArray<readonly [SkipKind, RegExp]> = SKIP_ALTERNATIVES.map(
  ([kind, alternatives]) =>
    [kind, new RegExp(String.raw`^\s*(?:\d{1,2}\s*[.)\-–—:]\s*)?(?:${alternatives})\s*[.!…]*\s*$`, 'i')] as const,
);

/// An announcement chapter names itself and stops; a title of half a sentence
/// is describing a scene. The outer bound on top of the anchoring, so that the
/// open-ended tails ("Previously on <show>") and any pattern added later cannot
/// quietly start matching prose.
const SKIP_MAX_WORDS = 5;

/// What kind of skippable chapter this is, or null. A chapter mpv had to name
/// itself (no title in the container) can never match — which is right: with no
/// title there is nothing to base the claim on.
export function skipKind(chapter: Chapter): SkipKind | null {
  const title = chapter.title;
  if (!title || title.split(/\s+/).length > SKIP_MAX_WORDS) return null;
  for (const [kind, pattern] of SKIP_PATTERNS) {
    if (pattern.test(title)) return kind;
  }
  return null;
}

/**
 * Jump to the start of a chapter, by index.
 *
 * Through the `chapter` property rather than a seek to `chapter.time`: with
 * ordered chapters (MKV) a chapter's content can live in a different file
 * entirely, which only mpv's own chapter seek handles. It is an exact seek —
 * the same contract as a seekbar click, since this is a deliberate "take me
 * there", not a preview.
 */
export function seekChapter(index: number) {
  const chapter = player.chapters[index];
  if (!chapter) return;
  // `set` with a string: mpv's own parser is guaranteed to understand the
  // property, same as with aid/sid.
  void command('set', ['chapter', String(index)]).catch(() => {});
  player.chapterIndex = index;
  showOsd(chapterTitle(chapter), { sub: formatTime(chapter.time) });
}

/**
 * Previous/next chapter.
 *
 * Clamped at both ends on purpose: mpv's own `add chapter ±1` walks off into
 * the neighbouring playlist entry, and "previous chapter" silently opening the
 * previous episode is not what the key says it does. At the first chapter,
 * "previous" restarts it — which is what every other player does too.
 */
export function jumpChapter(delta: number) {
  if (!player.hasFile || player.chapters.length === 0) return;
  // The mirror can be stale (a dropped property-change), and it is -1 until mpv
  // reports the first one — derive the current chapter from the position.
  const current = player.chapterIndex >= 0
    ? player.chapterIndex
    : (chapterAt(player.timePos)?.index ?? 0);
  const target = current + delta;
  if (target >= player.chapters.length) return;
  seekChapter(Math.max(0, target));
}

/**
 * Detach an external subtitle track from the session.
 *
 * mpv's `sub-remove` works on external files only — an embedded track lives in
 * the container and taking it out would mean rewriting the video, which is not
 * a player's job. The caller is expected to have hidden the control for
 * anything else; this refuses as well, because the two would otherwise have to
 * be kept in step by hand.
 */
export async function removeSubTrack(track: Track): Promise<boolean> {
  if (!track.external) return false;
  try {
    await command('sub-remove', [String(track.id)]);
    await loadTracks();
    return true;
  } catch (e) {
    console.warn('sub-remove failed:', e);
    return false;
  }
}

/** File picker for an external subtitle or audio track. */
export async function pickAndAttachTrack(kind: 'sub' | 'audio') {
  if (!player.hasFile) return;
  const sel = await open({
    multiple: true,
    filters: [
      {
        name: t(kind === 'sub' ? 'dialog.subtitles' : 'dialog.audio'),
        extensions: kind === 'sub' ? SUBTITLE_EXTENSIONS : AUDIO_EXTENSIONS,
      },
      { name: t('dialog.all_files'), extensions: ['*'] },
    ],
  });
  const paths = Array.isArray(sel) ? sel : sel ? [sel] : [];
  for (const path of paths) await attachTrack(kind, path);
}
