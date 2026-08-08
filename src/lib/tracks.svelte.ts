/**
 * What this file needs in order to play the way the viewer wants: which audio
 * and subtitle track, and how far either has to be nudged in time.
 *
 * Those two are one concern rather than two, which is also why they share a
 * store: both answer "how does this source have to be played", both outlive
 * finishing it, and both are things mpv keeps across a file change and would
 * otherwise leak into the next episode.
 *
 * **A remembered track is a description, not an id.** Track ids are positions
 * inside one file — the Russian dub that is #2 in episode 1 is routinely #3 in
 * episode 2 — so a choice is stored as a descriptor and resolved by scoring
 * against whatever the next file turns out to have (`matchTrack`). Below the
 * matcher's floor nothing is applied at all and mpv's own `alang`/`slang`
 * stands, which is the right answer for an episode that simply has no Russian
 * dub.
 *
 * **And it cannot be applied once, on `file-loaded`.** External subtitles found
 * by `sub-auto` turn up a beat later, and setting `sid` to an id that does not
 * exist yet silently selects nothing. So the restore is a standing effect that
 * re-runs as tracks appear, holding the score it already acted on so a better
 * candidate can still displace it — with a deadline, so a track that never
 * arrives (the file was re-encoded, the external subtitle is gone) cannot
 * hijack a choice the viewer made by hand in the meantime.
 */

import { command, getProperty, setProperty } from 'tauri-plugin-libmpv-api';

import { cast, castSwitchAudio } from './cast.svelte';
import { withFileDialog } from './chrome.svelte';
import {
  delaysFor,
  rememberDelay,
  rememberTrack,
  trackChoiceFor,
  trackWishFor,
} from './history.svelte';
import {
  describeTrack,
  loadTracks,
  matchTrack,
  nudgeDelay,
  pickAndAttachTrack,
  player,
  resetDelay,
  roundDelay,
  selectTrack as mpvSelectTrack,
  type Track,
  type TrackWish,
} from './player.svelte';

/// How long a pending restore may wait for its track to appear.
const RESTORE_WINDOW_MS = 5000;

/// A delay is dialled in by repeated presses; only where it settles is worth
/// writing down.
const DELAY_WRITE_MS = 400;

/// Re-read the lists this long after acting: the `selected` flags are what the
/// track menu ticks and they are stale until mpv has processed the change.
const RELIST_MS = 200;

type PendingTracks = {
  path: string;
  audio?: TrackWish;
  sub?: TrackWish;
  /// Legacy ids, exact in the file they were stored for.
  aid?: string;
  sid?: string;
  /// How good the match already acted on was, per kind. A better one can still
  /// turn up: external subtitles arrive a beat after `file-loaded`, and an .srt
  /// named like the episode should win over a generic embedded track.
  applied: { audio: number; sub: number };
  until: number;
};

let pending: PendingTracks | null = null;
let delayWriteTimer: ReturnType<typeof setTimeout> | undefined;

/// Arm the restore for the file that has just loaded.
export function restoreTrackChoice() {
  pending = null;
  if (!player.filePath) return;
  const legacy = trackChoiceFor(player.filePath);
  const audio = trackWishFor(player.filePath, 'audio');
  const sub = trackWishFor(player.filePath, 'sub');
  if (!legacy && !audio && !sub) return;
  pending = {
    path: player.filePath,
    audio: audio ?? undefined,
    sub: sub ?? undefined,
    aid: legacy?.aid,
    sid: legacy?.sid,
    applied: { audio: 0, sub: 0 },
    until: Date.now() + RESTORE_WINDOW_MS,
  };
  // Immediately, as well as reactively, and for two reasons. The race: `pending`
  // is a plain `let`, so arming it triggers nothing — the effect below next runs
  // when a track list *changes*, and if this file's list finished loading before
  // `file-loaded` reached us, no further change is coming and the restore would
  // silently never apply. And the delay: mpv has already picked a track by its
  // own `alang` and is decoding it, so every millisecond before the switch is a
  // moment of the wrong dub. There is nothing to wait for when the list is
  // already here.
  applyPending(player.audioTracks, player.subTracks);
}

/**
 * Act on the pending restore against the lists as they stand right now.
 *
 * The lists are **arguments rather than reads**, so the effect below cannot
 * fail to depend on them: a bare `void player.audioTracks` to "register the
 * dependency" is the shape that has caught this project before, since an
 * expression statement is exactly what a compiler or a minifier is entitled to
 * drop. An argument is evaluated.
 *
 * Returns without doing anything while there is nothing to act on — the caller
 * keeps waiting, because the list is still filling up.
 */
function applyPending(audioList: Track[], subList: Track[]) {
  const want = pending;
  if (!want) return;
  if (player.filePath !== want.path || Date.now() > want.until) {
    pending = null;
    return;
  }
  const audioScore = applySavedTrack('audio', want.audio, want.aid, audioList, want.applied.audio);
  const subScore = applySavedTrack('sub', want.sub, want.sid, subList, want.applied.sub);
  if (audioScore === want.applied.audio && subScore === want.applied.sub) return;
  want.applied = { audio: audioScore ?? want.applied.audio, sub: subScore ?? want.applied.sub };
  // The pending entry stays until its deadline: a better match can still arrive,
  // and a manual pick clears it in `selectTrack`.
  setTimeout(() => void loadTracks(), RELIST_MS);
}

/**
 * Resolve one remembered choice against the list this file actually has.
 *
 * Returns the score it acted on, or null while there is nothing to act on yet —
 * the caller keeps waiting, because the list is still filling up. A missing
 * track is not a failure: no candidate clears the matcher's floor, and mpv's own
 * choice stands.
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

/**
 * Start the standing restore. Must be called from a component's initialisation
 * — see the note on `initChrome` for what a top-level `$effect` costs.
 */
export function initTracks() {
  $effect(() => {
    // Reading both lists is what re-runs this as tracks appear — an external
    // subtitle found by `sub-auto` shows up a beat after `file-loaded`, and an
    // .srt named like the episode should displace a generic embedded track.
    applyPending(player.audioTracks, player.subTracks);
  });
}

/**
 * A deliberate choice. Worth remembering — and it cancels a restore still
 * waiting for its track to show up, which is the whole reason these two live in
 * one module.
 *
 * Closing the menu is the caller's: this is also reached from the hotkeys, where
 * there is no menu to close.
 */
export function selectTrack(kind: 'audio' | 'sub', track: Track | null) {
  pending = null;
  if (player.filePath) {
    const list = kind === 'audio' ? player.audioTracks : player.subTracks;
    rememberTrack(player.filePath, kind, track ? describeTrack(track, list) : 'no');
  }
  void mpvSelectTrack(kind, track);
  // While the TV owns playback, an audio choice must reach it too: the prepared
  // file carries exactly one track, so the switch is a re-prepare (cached per
  // track — switching back is instant) plus a reload at the TV's position. The
  // local switch above still runs, so the handback and the menu's check mark
  // stay in agreement with what the TV plays.
  if (cast.remote && kind === 'audio' && track) void castSwitchAudio(track);
}

/// Attach an external subtitle or audio file. The window dimming belongs to the
/// shell (`withFileDialog`); what is ours is that the result becomes a track of
/// this file.
export async function addTrackFile(kind: 'sub' | 'audio') {
  await withFileDialog(() => pickAndAttachTrack(kind));
}

// ---- Delays ---------------------------------------------------------------

/**
 * The value is read back rather than computed: `nudgeDelay` uses mpv's `add`
 * (for the same reason toggles use `cycle`), so what it lands on is mpv's
 * business and the mirror may be a beat behind.
 */
function rememberDelaySoon(kind: 'sub' | 'audio') {
  clearTimeout(delayWriteTimer);
  delayWriteTimer = setTimeout(async () => {
    if (!player.filePath) return;
    const value = await getProperty(kind === 'sub' ? 'sub-delay' : 'audio-delay', 'double').catch(
      () => null,
    );
    // Rounded before it is written: mpv hands back the raw float, and
    // `rememberDelay` only deletes the record on an exact zero (see
    // `DELAY_EPSILON`).
    if (typeof value === 'number') rememberDelay(player.filePath, kind, roundDelay(value));
  }, DELAY_WRITE_MS);
}

export function nudgeDelayHere(kind: 'sub' | 'audio', delta: number) {
  nudgeDelay(kind, delta);
  rememberDelaySoon(kind);
}

export function resetDelayHere(kind: 'sub' | 'audio') {
  resetDelay(kind);
  if (player.filePath) rememberDelay(player.filePath, kind, 0);
}

/// mpv keeps `sub-delay` across a file change (measured), so a correction
/// dialled in for one episode silently applies to the next. Every file therefore
/// gets an explicit value — its own, or zero.
export function applyDelays() {
  if (!player.filePath) return;
  const saved = delaysFor(player.filePath);
  void setProperty('sub-delay', saved.sub).catch(() => {});
  void setProperty('audio-delay', saved.audio).catch(() => {});
}
