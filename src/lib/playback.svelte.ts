/**
 * Playback as one thing, whichever machine is decoding it.
 *
 * The player has two transports for the same film: mpv on this machine, or a
 * television that fetched the file for itself. Nearly everything above the
 * transport — the control row, the end-of-file offer, the chapter list, the
 * queue, the hotkeys, the taskbar — asks the same handful of questions of
 * either one: where is the position, how long is the file, is it paused, how
 * loud is it, and what is this session allowed to do at all.
 *
 * Asking them per call site is what this module replaces, and the reason is
 * measured rather than aesthetic. The rule was "while `cast.remote`, read
 * `cast.time` rather than mpv's, and act with `castSeek`", and it was written
 * down precisely because places kept being written without it: the chapter
 * list, the skip button, the skip guard and the Windows taskbar progress were
 * each found afterwards, by a viewer, one at a time. **A missed branch costs
 * no error, no warning and no failed check** — mpv is parked *paused* on the
 * frame it handed over, so its mirrors do not look wrong, they look frozen at
 * a plausible value, which is the same thing as a film that is not moving.
 *
 * What this deliberately does NOT cover is the seekbar gesture. A local drag
 * is a stream of preview seeks with an exact/keyframe probe behind it; a
 * remote one is a single command on release, because every preview would cost
 * the television seconds of buffering. Hiding that behind a shared `seek()`
 * would leave the caller unable to see what a gesture costs, which is the one
 * thing that code exists to reason about — so those branches stay written out
 * in `seek.svelte.ts`, and that module deliberately stays *below* this one in
 * the import graph (it is imported here, never the other way round; going the
 * other way is a cycle through `step-engine`).
 *
 * Dependency direction, as everywhere: this reads `player` and `cast`, and
 * neither of them may read it.
 */

import { command } from 'tauri-plugin-libmpv-api';

import {
  cast,
  castAdvance,
  castFollow,
  castNudgeVolume,
  castSeek,
  castSeekBy,
  castSetVolume,
  castToggleMute,
  castTogglePause,
} from './cast.svelte';
import { formatTime } from './format';
import { t } from './i18n.svelte';
import type { ActionId } from './keys.svelte';
import { showOsd } from './osd.svelte';
import {
  type Chapter,
  chapterAt,
  chapterTitle,
  jumpChapter,
  player,
  seekChapter,
  setVolume as mpvSetVolume,
  toggleMute as mpvToggleMute,
} from './player.svelte';
import { type PlaylistEntry, playEntry } from './playlist.svelte';
import { seekPercent, seekRelative } from './seek.svelte';
import { togglePlayback } from './step-engine.svelte';

/**
 * The scale the volume bar and the volume keys speak, on both transports.
 *
 * mpv's own ceiling is `VOLUME_MAX` (150) and the bar has always stopped at
 * 100; a receiver has no amplification range at all, so for it 100 *is* the
 * ceiling rather than a point on the way to anything. One scale is what lets
 * one slider and one key step serve both.
 */
export const VOLUME_SCALE = 100;

/**
 * Something a session can or cannot do.
 *
 * Two of these are genuine per-device facts — a receiver may declare its
 * volume fixed or never report one, and over DLNA the file went across with
 * every track in it, so which one plays is the television's own choice and
 * unreadable from here. The rest are the local decoder's: while the television
 * owns playback there is no decoded frame on this machine to step, zoom, save
 * or slow down, so acting on mpv would move a player nobody is watching.
 */
export type Feature =
  | 'volume'
  | 'trackChoice'
  | 'speed'
  | 'frameStep'
  | 'zoom'
  | 'loop'
  | 'delays'
  | 'screenshot'
  | 'mini';

class Playback {
  /// The television owns playback and this window is a remote. False while
  /// connecting or preparing — there local playback deliberately keeps
  /// running, and everything below must keep reading and driving mpv.
  get remote(): boolean {
    return cast.remote;
  }

  /// A session exists at all, handed over or not. The queue follows *this*,
  /// not `remote`: a session still preparing is still a session, and letting
  /// the next episode open locally under it is how the two ends come apart.
  get session(): boolean {
    return cast.active;
  }

  get position(): number {
    return cast.remote ? cast.time : player.timePos;
  }

  get duration(): number {
    return cast.remote ? cast.duration : player.duration;
  }

  /// Parked on the last frame counts as paused: mpv sets `pause=yes` itself at
  /// eof (`keep-open=always`), so the two are one state wearing two mirrors.
  get paused(): boolean {
    return cast.remote ? cast.paused : player.paused || player.eofReached;
  }

  /// 0..`VOLUME_SCALE` on either transport. Locally these are mpv's own volume
  /// units, where 100 is unity — a value above it can only arrive from the
  /// viewer's own mpv.conf, and the bar pins at its maximum as it always has.
  get volume(): number {
    return cast.remote ? cast.volume * VOLUME_SCALE : player.volume;
  }

  /// "Shows as silent", which locally includes a volume dialled to zero — that
  /// is what the icon has always meant and it is what the viewer sees.
  get muted(): boolean {
    return cast.remote ? cast.muted : player.muted || player.volume === 0;
  }

  /// Which chapter the film is actually in.
  ///
  /// mpv's `chapter` mirror is frozen at the handoff while casting, so the
  /// list highlighted whichever chapter the film happened to be on when the
  /// cast started and never moved again. The chapter *list* is local knowledge
  /// about the same file and stays valid; only the position has to come from
  /// whoever is playing.
  get chapterIndex(): number {
    return cast.remote ? (chapterAt(cast.time)?.index ?? -1) : player.chapterIndex;
  }

  /// What this session can do. Read by the controls to disable themselves and
  /// by the hotkey table to refuse out loud — one answer, so a button and its
  /// key cannot disagree about whether something is available.
  get can(): Record<Feature, boolean> {
    const remote = cast.remote;
    return {
      volume: !remote || cast.volumeAdjustable,
      trackChoice: !remote || cast.transport !== 'dlna',
      speed: !remote,
      frameStep: !remote,
      zoom: !remote,
      loop: !remote,
      delays: !remote,
      screenshot: !remote,
      mini: !remote,
    };
  }
}

export const playback = new Playback();

// ---- Verbs ------------------------------------------------------------------
// Each of these is the whole of a decision that used to be made at every call
// site. A caller says what the viewer asked for; who answers is not its
// business.

export function togglePause() {
  if (cast.remote) {
    castTogglePause();
    return;
  }
  void togglePlayback();
}

/// Relative seek. `precise` is a local concept — it asks for an exact seek
/// where the coarse step may land on a keyframe — and the television has no
/// such distinction, so it is simply ignored there.
export function seekBy(delta: number, precise = false) {
  if (cast.remote) {
    castSeekBy(delta);
    return;
  }
  seekRelative(delta, precise);
}

/// A jump to a fraction of the file: the digit keys, Home and End. Local
/// playback goes through `seekPercent`, which carries the exact/keyframe
/// probe; the remote one is a single command.
///
/// End stops a few seconds short on the television rather than landing on the
/// last frame, which a receiver reads as the file being over.
export function seekFraction(percent: number) {
  if (!cast.remote) {
    seekPercent(percent);
    return;
  }
  const total = cast.duration;
  if (percent >= 100) {
    castSeek(Math.max(0, total - 5));
    return;
  }
  castSeek((total * percent) / 100);
}

/// Go to a chapter by its entry in the list.
///
/// Locally this is mpv's `chapter` property rather than a seek to the
/// chapter's time, and that is not a detail: with ordered chapters (MKV) the
/// content can live in another file entirely and only mpv's own chapter seek
/// follows that. Remotely there is no such thing to follow — the television
/// was handed one file — so it is a seek to the timestamp, and the popup is
/// raised here because `seekChapter` raises its own and the two paths owe the
/// viewer the same feedback.
export function jumpToChapter(chapter: Chapter) {
  if (!cast.remote) {
    seekChapter(chapter.index);
    return;
  }
  castSeek(chapter.time);
  showOsd(chapterTitle(chapter), { sub: formatTime(chapter.time) });
}

/// Previous/next chapter, clamped at both ends: mpv's own `add chapter ±1`
/// walks off into the neighbouring playlist entry, and "previous chapter"
/// opening the previous episode is not what the key says.
export function stepChapter(dir: 1 | -1) {
  if (!cast.remote) {
    jumpChapter(dir);
    return;
  }
  const list = player.chapters;
  if (!list.length) return;
  // The half-second tolerance is the remote path's own: a position a moment
  // short of a boundary counts as being past it, so "next" from just before a
  // chapter start does not land back where it already is.
  const here = list.filter((c) => c.time <= cast.time + 0.5).length - 1;
  const target = dir === 1 ? here + 1 : here - 1;
  if (target >= list.length) return;
  jumpToChapter(list[Math.max(0, target)]);
}

/// Previous/next entry in the queue.
///
/// `session`, not `remote`: during the prepare rung local playback is still
/// what the viewer sees, but the queue is the *session's* — advancing it
/// locally there would leave the television being handed a file the player has
/// already moved off. That guard used to be spelled two different ways in
/// three places (the buttons said `active`, the keys said `remote`), which is
/// exactly the drift a verb exists to end.
export function advance(dir: 1 | -1) {
  if (cast.active) {
    void castAdvance(dir);
    return;
  }
  void command(dir === 1 ? 'playlist-next' : 'playlist-prev', []);
}

/// Open a specific queue entry — the panel's rows, and Enter on the end screen.
export function openEntry(entry: PlaylistEntry) {
  if (cast.active) {
    void castFollow(entry);
    return;
  }
  void playEntry(entry);
}

/**
 * The OSD every volume change raises.
 *
 * It lives beside the volume verbs rather than in `osd.svelte.ts` because it
 * is the *volume*'s presentation, and the wheel, the keys and both transports
 * want the same one.
 */
export function osdVolume(v: number) {
  showOsd(t('osd.volume', { value: Math.round(v) }), { progress: Math.min(1, v / VOLUME_SCALE) });
}

/// Set the volume outright: the slider. Silent on purpose — the bar is its own
/// readout, and a popup over a control the viewer is already looking at is
/// noise.
export function setVolume(v: number) {
  if (cast.remote) {
    castSetVolume(v / VOLUME_SCALE);
    return;
  }
  mpvSetVolume(v);
}

/// Nudge it: the keys and the wheel, which have no readout of their own and so
/// raise the popup. A receiver that declared its volume fixed says so instead
/// of silently doing nothing (`castNudgeVolume` owns that message).
export function nudgeVolume(delta: number) {
  if (cast.remote) {
    castNudgeVolume(delta / VOLUME_SCALE);
    return;
  }
  osdVolume(mpvSetVolume(player.volume + delta));
}

export function toggleMute() {
  if (cast.remote) {
    castToggleMute();
    return;
  }
  mpvToggleMute();
}

// ---- Hotkeys while a television owns playback -------------------------------

/**
 * What every hotkey action does while casting.
 *
 * `Record<ActionId, …>` on purpose: a new action is a **compile error** here
 * until somebody says which of the two kinds it is. That is the whole point of
 * the table — every casting bug this module exists for was an omission, and an
 * omission the compiler refuses is not one that reaches a viewer. Same device
 * as `SKIP_LABEL`, which makes a new `SkipKind` unbuildable until it has
 * something to print on the button.
 *
 * - `'local'` — the ordinary dispatch runs. Either the action is about this
 *   *window* rather than the film (fullscreen, the menus, opening a file), or
 *   its implementation is one of the verbs above and therefore routes itself.
 *   There is deliberately no third label for "the television answers it": that
 *   would be a second place routing is decided, which is what this module
 *   exists to end. An action the TV can answer earns a verb, and a verb serves
 *   the buttons and the menus as well as the key.
 * - a `Feature` — it needs the local decoder, which is parked. Refused out
 *   loud rather than acted on a player nobody is watching.
 */
const CAST_BEHAVIOR: Record<ActionId, 'local' | Feature> = {
  pause: 'local',
  seek_back: 'local',
  seek_fwd: 'local',
  seek_back_precise: 'local',
  seek_fwd_precise: 'local',
  seek_back_10: 'local',
  seek_fwd_10: 'local',
  seek_start: 'local',
  seek_end: 'local',
  frame_prev: 'frameStep',
  frame_next: 'frameStep',
  chapter_prev: 'local',
  chapter_next: 'local',
  playlist_prev: 'local',
  playlist_next: 'local',
  speed_down: 'speed',
  speed_up: 'speed',
  loop: 'loop',
  ab_loop: 'loop',
  ab_clear: 'loop',
  volume_up: 'local',
  volume_down: 'local',
  mute: 'local',
  audio_delay_down: 'delays',
  audio_delay_up: 'delays',
  sub_delay_down: 'delays',
  sub_delay_up: 'delays',
  fullscreen: 'local',
  mini: 'mini',
  info: 'local',
  reset_zoom: 'zoom',
  open_file: 'local',
  open_link: 'local',
  screenshot: 'screenshot',
  screenshot_subs: 'screenshot',
  copy_frame: 'screenshot',
};

/**
 * Refuse an action the session cannot carry. Returns true when it was dealt
 * with here and the ordinary dispatch must not also run.
 *
 * Phrased as a capability check rather than as "are we casting", so a feature
 * that becomes available on some devices and not others — the volume already
 * is one — needs no new branch here.
 */
export function refusedBySession(id: ActionId): boolean {
  const how = CAST_BEHAVIOR[id];
  if (how === 'local' || playback.can[how]) return false;
  showOsd(t('cast.not_while_casting'));
  return true;
}
