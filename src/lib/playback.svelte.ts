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
 * **The same verbs are where a shared room is told what happened**, and that is
 * the whole of "watching together picks up new features by design". A verb is
 * already the mandated route for a button, a menu row, a hotkey and a gesture —
 * that is why it exists — so publishing from here means a control added next
 * year is shared without anybody remembering to share it. What guards the
 * *other* direction, an action that must never leave this machine, is
 * `SYNC_BEHAVIOR` at the bottom: a `Record<ActionId, …>`, so a new action does
 * not compile until somebody has said which kind it is. Exactly the device
 * `CAST_BEHAVIOR` already is, for exactly the same failure — every casting bug
 * this module exists for was an omission.
 *
 * Applying what *arrives* is deliberately not here. It goes to mpv directly,
 * from `sync/apply.svelte.ts`, and it must: a remote timeline is not a viewer's
 * gesture, so it may not raise an OSD, pause a drag, or be published back. Same
 * distinction the player already draws between `applyProperty` sweeping a stale
 * mirror and the `property` hook acting on a real event.
 *
 * Dependency direction, as everywhere: this reads `player`, `cast` and the sync
 * *leaf*, and none of them may read it.
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
import { publishState, wire } from './sync/wire.svelte';
import { formatTime } from './format';
import { t } from './i18n.svelte';
import type { ActionId } from './keys.svelte';
import { showOsd } from './osd.svelte';
import {
  type Chapter,
  changeSpeed as mpvChangeSpeed,
  chapterAt,
  chapterTitle,
  jumpChapter,
  player,
  positionNow,
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
// business — and, when a room is watching, who else hears about it is not its
// business either.

/**
 * Tell the room where this gesture put the film.
 *
 * Every caller passes what it *intends*, never what the mirrors currently say:
 * an mpv command is asynchronous, so at the moment a verb runs `player.paused`
 * and `player.timePos` still describe the state the gesture is about to leave.
 * Publishing those would send the room the previous position — and, since the
 * relay stamps whatever arrives, would then drag everybody back to it.
 *
 * Where no position is given — a pause, a speed change — the *extrapolated* one
 * is used rather than the mirror, for the same reason the reconciler reads it:
 * `time-pos` is an event, so the mirror is tens of milliseconds behind, and
 * publishing it tells the room the film is further back than it is.
 */
function share(next: { paused?: boolean; position?: number; speed?: number }) {
  if (!wire.on) return;
  publishState(
    next.paused ?? playback.paused,
    clampPosition(next.position ?? (cast.remote ? playback.position : positionNow())),
    next.speed ?? player.speed,
  );
}

function clampPosition(pos: number): number {
  const end = playback.duration;
  return Math.max(0, end > 0 ? Math.min(end, pos) : pos);
}

export function togglePause() {
  if (refusedByRoom()) return;
  const paused = !playback.paused;
  if (cast.remote) castTogglePause();
  else void togglePlayback();
  share({ paused });
}

/// Relative seek. `precise` is a local concept — it asks for an exact seek
/// where the coarse step may land on a keyframe — and the television has no
/// such distinction, so it is simply ignored there.
export function seekBy(delta: number, precise = false) {
  if (refusedByRoom()) return;
  if (cast.remote) castSeekBy(delta);
  else seekRelative(delta, precise);
  // The intended landing rather than the observed one: a coarse arrow may land
  // on a keyframe up to a GOP away, and telling the room about *that* would
  // make everyone else's position depend on this machine's codec.
  share({ position: playback.position + delta });
}

/// A jump to a fraction of the file: the digit keys, Home and End. Local
/// playback goes through `seekPercent`, which carries the exact/keyframe
/// probe; the remote one is a single command.
///
/// End stops a few seconds short on the television rather than landing on the
/// last frame, which a receiver reads as the file being over.
export function seekFraction(percent: number) {
  if (refusedByRoom()) return;
  if (!cast.remote) {
    seekPercent(percent);
    share({ position: (playback.duration * Math.min(100, Math.max(0, percent))) / 100 });
    return;
  }
  const total = cast.duration;
  const target = percent >= 100 ? Math.max(0, total - 5) : (total * percent) / 100;
  castSeek(target);
  share({ position: target });
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
  if (refusedByRoom()) return;
  if (!cast.remote) {
    seekChapter(chapter.index);
  } else {
    castSeek(chapter.time);
    showOsd(chapterTitle(chapter), { sub: formatTime(chapter.time) });
  }
  // The chapter's timestamp, not its index. Ordered chapters (MKV) can put the
  // content in another file entirely, so an index means something only to a
  // player holding this exact release — and the room may not be.
  share({ position: chapter.time });
}

/// Previous/next chapter, clamped at both ends: mpv's own `add chapter ±1`
/// walks off into the neighbouring playlist entry, and "previous chapter"
/// opening the previous episode is not what the key says.
export function stepChapter(dir: 1 | -1) {
  if (refusedByRoom()) return;
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
  if (refusedByRoom()) return;
  if (cast.active) {
    void castAdvance(dir);
    return;
  }
  void command(dir === 1 ? 'playlist-next' : 'playlist-prev', []);
}

/// Open a specific queue entry — the panel's rows, and Enter on the end screen.
export function openEntry(entry: PlaylistEntry) {
  if (refusedByRoom()) return;
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

/**
 * Playback speed — a verb rather than a straight call into `player` because it
 * is one of the four things a room shares.
 *
 * The roadmap's rule is "sync the timeline, not the presentation", and speed is
 * on the timeline side of that line: it decides where everybody will be in ten
 * seconds. Volume, tracks and subtitle appearance are on the other.
 *
 * Note this deliberately publishes the speed the *viewer* asked for. Drift
 * correction also writes mpv's `speed`, and it must not come through here —
 * that is this machine catching up with the room, not a change to what the room
 * is doing, and publishing it would have every peer chasing every other peer.
 */
export function changeSpeed(factor: number) {
  if (refusedByRoom()) return;
  const next = mpvChangeSpeed(factor);
  share({ speed: next });
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

// ---- Hotkeys while a room is watching along ---------------------------------

/**
 * What every hotkey action means to a shared room.
 *
 * The same device as `CAST_BEHAVIOR`, and it is the answer to the request this
 * feature was built under: that actions be picked up by a shared session **by
 * design** rather than by somebody remembering. A `Record<ActionId, …>` makes a
 * new action a compile error until it has been classified, and the verbs above
 * make the classification true — `shared` needs no wiring here, because the verb
 * that implements it has already told the room.
 *
 * - `'shared'` — it moves the timeline, and the verb publishes. Includes the
 *   ones that move it by changing the *file*: `apply` publishes the new content
 *   when it loads, so the queue keys need nothing of their own.
 * - `'personal'` — it is about this machine. Volume, tracks, subtitle delays,
 *   zoom, the window, saving a frame. Sharing any of them would be sharing the
 *   presentation, which is precisely what the roadmap's rule says not to do:
 *   *sync the timeline, not the presentation*.
 * - `'solo'` — it would make the shared timeline mean two different things at
 *   once, so it is refused out loud while a room is on. Only the A–B loop
 *   qualifies: it holds playback inside a segment, which drift correction would
 *   fight once a second for as long as the loop lasts.
 *
 * Frame stepping is deliberately `personal` rather than `solo`: it moves the
 * position by a frame, which is an order of magnitude under the drift threshold,
 * and it is done on a paused film that the room has paused too. Repeat mode is
 * `personal` for a subtler reason — a viewer whose file loops while the room
 * moves on ends up on different content, and that heals itself, because
 * `apply` opens whatever the room is watching.
 */
const SYNC_BEHAVIOR: Record<ActionId, 'shared' | 'personal' | 'solo'> = {
  pause: 'shared',
  seek_back: 'shared',
  seek_fwd: 'shared',
  seek_back_precise: 'shared',
  seek_fwd_precise: 'shared',
  seek_back_10: 'shared',
  seek_fwd_10: 'shared',
  seek_start: 'shared',
  seek_end: 'shared',
  chapter_prev: 'shared',
  chapter_next: 'shared',
  playlist_prev: 'shared',
  playlist_next: 'shared',
  speed_down: 'shared',
  speed_up: 'shared',
  ab_loop: 'solo',
  ab_clear: 'solo',
  frame_prev: 'personal',
  frame_next: 'personal',
  loop: 'personal',
  volume_up: 'personal',
  volume_down: 'personal',
  mute: 'personal',
  audio_delay_down: 'personal',
  audio_delay_up: 'personal',
  sub_delay_down: 'personal',
  sub_delay_up: 'personal',
  fullscreen: 'personal',
  mini: 'personal',
  info: 'personal',
  reset_zoom: 'personal',
  open_file: 'personal',
  open_link: 'personal',
  screenshot: 'personal',
  screenshot_subs: 'personal',
  copy_frame: 'personal',
};

/**
 * Refuse an action the session cannot carry. Returns true when it was dealt
 * with here and the ordinary dispatch must not also run.
 *
 * Phrased as a capability check rather than as "are we casting", so a feature
 * that becomes available on some devices and not others — the volume already
 * is one — needs no new branch here.
 *
 * Two sessions can be running at once (a television *and* a room), so both
 * tables are consulted. The casting one goes first because it is the one that
 * would act on a player nobody is watching, which is the worse failure.
 */
export function refusedBySession(id: ActionId): boolean {
  const how = CAST_BEHAVIOR[id];
  if (how !== 'local' && !playback.can[how]) {
    showOsd(t('cast.not_while_casting'));
    return true;
  }
  if (wire.on && SYNC_BEHAVIOR[id] === 'solo') {
    showOsd(t('sync.not_in_room'));
    return true;
  }
  return false;
}

/**
 * The viewer asked for something shared but the room has handed control to its
 * host. Returns true when the gesture was refused and must not run locally.
 *
 * Checked at the *gesture*, not inside `share`: acting locally and then not
 * telling anybody is the one outcome worse than refusing, because the viewer
 * ends up somewhere else in the film with nothing on screen to say why.
 */
export function refusedByRoom(): boolean {
  if (!wire.on || wire.mayDrive) return false;
  showOsd(t('sync.host_only'));
  return true;
}
