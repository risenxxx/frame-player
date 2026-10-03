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
import { publishTrack, wire } from './sync/wire.svelte';
import { withFileDialog } from './chrome.svelte';
import {
  delaysFor,
  rememberDelay,
  rememberSubSpeed,
  rememberTrack,
  subSpeedFor,
  trackChoiceFor,
  trackWishFor,
} from './history.svelte';
import { locale, t } from './i18n.svelte';
import { showOsd } from './osd.svelte';
import { SUB_SPEED_PRESETS, isPreset, isUnitSpeed, type SubSpeedPreset } from './sub-speed';
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
import type { TrackKind } from './sync/protocol';

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
  // The graph went with the previous file (it is a file-local option); the
  // menu's mode goes with it, so every file opens on one track.
  audioMix.multi = false;
  audioMix.ticked = [];
  graphIds = [];
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
 * Start the standing restore. Must be called from a component's initialization
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
  // A pick of one audio track ends a mix: mpv ignores `aid` for as long as a
  // graph is set (measured — the write succeeds and nothing changes).
  if (kind === 'audio' && mixing()) {
    audioMix.multi = false;
    audioMix.ticked = [];
    graphIds = [];
    void (async () => {
      await clearMixGraph();
      await mpvSelectTrack(kind, track);
    })();
  } else {
    void mpvSelectTrack(kind, track);
  }
  const wish: TrackWish | null = player.filePath
    ? track
      ? describeTrack(track, kind === 'audio' ? player.audioTracks : player.subTracks)
      : 'no'
    : null;
  if (player.filePath && wish) rememberTrack(player.filePath, kind, wish);
  // Told to the room only for the kinds the *room* shares — a rule the host
  // sets beside "only the host controls playback", not a preference each viewer
  // keeps. `wire.shares` answers in both directions, which is what stops two
  // members disagreeing about what their own room does.
  //
  // A *description* goes over the wire, never the id: see `SharedTracks`.
  if (wish && wire.shares(kind)) publishTrack(kind, wish);
  // While the TV owns playback, an audio choice must reach it too: the prepared
  // file carries exactly one track, so the switch is a re-prepare (cached per
  // track — switching back is instant) plus a reload at the TV's position. The
  // local switch above still runs, so the handback and the menu's check mark
  // stay in agreement with what the TV plays.
  if (cast.remote && kind === 'audio' && track) void castSwitchAudio(track);
}

/**
 * Take a track a shared room has chosen.
 *
 * Deliberately routed through the same pending-restore machinery as the
 * remembered per-folder choice rather than straight to `aid`/`sid`, and for the
 * same reasons that machinery exists: the descriptor has to be *matched*
 * against this copy's own track list (two people rarely have the same rip), and
 * the list is still filling up for a few hundred milliseconds after a file
 * loads.
 *
 * Only the kind being followed is displaced. A viewer who shares audio but not
 * subtitles keeps their own subtitle memory while taking the room's dub, and a
 * restore still in flight for the other kind carries on.
 */
export function followRoomTrack(kind: TrackKind, wish: TrackWish) {
  if (!player.filePath) return;
  const mine = pending?.path === player.filePath ? pending : null;
  pending = {
    path: player.filePath,
    audio: kind === 'audio' ? wish : mine?.audio,
    sub: kind === 'sub' ? wish : mine?.sub,
    aid: kind === 'audio' ? undefined : mine?.aid,
    sid: kind === 'sub' ? undefined : mine?.sid,
    applied: {
      audio: kind === 'audio' ? 0 : (mine?.applied.audio ?? 0),
      sub: kind === 'sub' ? 0 : (mine?.applied.sub ?? 0),
    },
    until: Date.now() + RESTORE_WINDOW_MS,
  };
  applyPending(player.audioTracks, player.subTracks);
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

/// mpv keeps `sub-delay`, `audio-delay` and `sub-speed` across a file change
/// (all three measured), so a correction dialled in for one episode silently
/// applies to the next. Every file therefore gets an explicit value for each —
/// its own, or the default.
export function applyTiming() {
  if (!player.filePath) return;
  const saved = delaysFor(player.filePath);
  void setProperty('sub-delay', saved.sub).catch(() => {});
  void setProperty('audio-delay', saved.audio).catch(() => {});
  void setProperty('sub-speed', subSpeedFor(player.filePath)).catch(() => {});
}

// ---- Subtitle frame rate --------------------------------------------------

/**
 * Stretch the subtitles by `factor` (subtitle fps / video fps; see sub-speed.ts)
 * and remember it for this file.
 *
 * Written straight, not debounced like a delay: this is picked once from a
 * list, not dialled in. mpv applies it at render time, so a line already on
 * screen moves with it — measured, no seek or reload is needed.
 */
export function setSubSpeedHere(factor: number) {
  if (!player.hasFile) return;
  const value = isUnitSpeed(factor) ? 1 : factor;
  void setProperty('sub-speed', value).catch(() => {});
  if (player.filePath) rememberSubSpeed(player.filePath, value);
  showOsd(t('osc.sub_speed'), { sub: subSpeedLabel(value) });
}

/// A frame rate as a person writes it: 23.976 rather than 23.976023976…, with
/// the interface's own decimal separator.
export function formatFps(fps: number): string {
  return fps.toLocaleString(locale(), { maximumFractionDigits: 3 });
}

export function presetLabel(preset: SubSpeedPreset): string {
  return t('osc.sub_speed_pair', { from: formatFps(preset.from), to: formatFps(preset.to) });
}

/// What a factor mpv holds means, in the words the menu uses for it. A factor
/// that is none of the presets — fitted from a search result with an unusual
/// pair, or set in mpv.conf — is shown as the bare ratio.
export function subSpeedLabel(factor: number): string {
  if (isUnitSpeed(factor)) return t('osc.sub_speed_off');
  const preset = SUB_SPEED_PRESETS.find((p) => isPreset(factor, p));
  return preset
    ? presetLabel(preset)
    : t('osc.sub_speed_custom', {
        value: factor.toLocaleString(locale(), { maximumFractionDigits: 4 }),
      });
}

// ---- Several audio tracks at once ----------------------------------------
//
// A recording made for editing keeps the game, the microphone and the call in
// separate tracks, and a player that plays one of them at a time cannot be used
// to look through it. mpv can mix them: `lavfi-complex` takes the tracks by id
// and hands one stream to the output — measured on three test tones, each one
// arrives at its own level.
//
// What was measured, and what the code below does about each:
//
//   - **Every change of the graph is an exact seek to where playback is**
//     (`update_lavfi_complex` → `issue_refresh_seek`), and one that switches on
//     a stream the demuxer was not reading makes the picture jump forward;
//     one that does not, does not. So the graph takes **every** audio track of
//     the file from the moment the mode is switched on, and a tick only changes
//     `amix`'s weights — never which streams are read. What remains is one
//     jump, on the switch, rather than one per tick.
//   - With every track in the graph, every one reads `selected` and `aid`
//     reads `no`, so which tracks are *heard* is ours to keep (`ticked`).
//   - While a graph is set, `set aid` succeeds and does nothing. A single pick
//     has to clear the graph first (`selectTrack`).
//   - Clearing the graph leaves *no* audio at all — `aid` reads `no` — so going
//     back to one track always names it. And the option underneath still holds
//     the track picked before the mix, so naming that one again changes
//     nothing unless `aid` is set to `no` first (`clearMixGraph`).
//   - A graph left over from the previous file stops a file lacking one of its
//     tracks from opening at all. So it is written as a **file-local** option:
//     mpv drops it when the file changes, whichever way the next file arrives,
//     and there is no load path to remember.
//
// The mix is not remembered: it is a way of looking through one file, and the
// next file opens on its single track like any other.

class AudioMix {
  /// The menu is in "several tracks" mode: the graph is up, and a click on a
  /// track adds it to what is heard or takes it out.
  multi = $state(false);
  /// The tracks heard, by id, in the order they were ticked. Never empty while
  /// `multi` is on.
  ticked = $state<number[]>([]);
}

export const audioMix = new AudioMix();

/// The tracks the graph was built over. A track that turns up later (an
/// external file added while mixing) is not among them, and ticking it rebuilds
/// the graph — the one tick that costs a jump.
let graphIds: number[] = [];

/// `normalize=0` keeps every track at its own level — what was recorded is what
/// is heard — and the limiter is what makes that safe: three loud tracks summed
/// clip. `level=0` turns off the limiter's own make-up gain, which would
/// otherwise raise the whole mix. A weight of 0 is a track decoded and not
/// heard, which is the price of never changing the set of streams.
function mixGraph(ids: number[], heard: number[]): string {
  const weights = ids.map((id) => (heard.includes(id) ? 1 : 0)).join(' ');
  return `${ids.map((id) => `[aid${id}]`).join('')}amix=inputs=${ids.length}:normalize=0:weights=${weights},alimiter=limit=1:level=0[ao]`;
}

export function mixing(): boolean {
  return audioMix.multi;
}

async function clearMixGraph() {
  await command('set', ['file-local-options/lavfi-complex', '']);
  // `aid` still holds the track picked before the mix: the graph takes the
  // tracks over without writing the option, and a write of the value it
  // already holds is no change to mpv — `set aid 4` answers success and selects
  // nothing, so a viewer going back to the dub they had was left with no
  // track and no sound, every click on it included. Measured on mpv 0.41: a
  // track picked by hand, mixed, the graph cleared, the same id set → nothing;
  // any other id → selected. Through `no` first, the write that follows is
  // always a change.
  await command('set', ['aid', 'no']);
}

async function writeMix(heard: number[]) {
  // A restore still waiting for its track would otherwise land on top of this
  // as an `aid` write — ignored while the graph stands.
  pending = null;
  const all = player.audioTracks.map((track) => track.id);
  if (!heard.every((id) => graphIds.includes(id))) graphIds = all;
  await command('set', ['file-local-options/lavfi-complex', mixGraph(graphIds, heard)]);
  audioMix.ticked = heard;
}

/// Turn the menu's "several tracks" mode on or off. On keeps what is heard —
/// only the track that was playing is ticked. Off keeps the first track ticked.
export async function setAudioMulti(on: boolean) {
  if (!on) {
    if (!audioMix.multi) return;
    const keep = audioMix.ticked[0] ?? player.audioTracks[0]?.id;
    audioMix.multi = false;
    audioMix.ticked = [];
    graphIds = [];
    try {
      await clearMixGraph();
      if (keep !== undefined) await command('set', ['aid', String(keep)]);
      const track = player.audioTracks.find((candidate) => candidate.id === keep);
      if (track) showOsd(track.label);
    } catch {
      showOsd(t('osd.track_failed'));
    }
    setTimeout(() => void loadTracks(), RELIST_MS);
    return;
  }
  // Bitstream hands the receiver an undecoded stream; there is nothing to mix.
  const spdif = await getProperty('audio-spdif', 'string').catch(() => '');
  if (spdif) {
    showOsd(t('osd.mix_spdif'));
    return;
  }
  const current = player.audioTracks.find((track) => track.selected) ?? player.audioTracks[0];
  if (!current) return;
  graphIds = [];
  try {
    await writeMix([current.id]);
    audioMix.multi = true;
  } catch (e) {
    showOsd(t('osd.track_failed'));
    console.warn('audio mix failed:', e);
  }
}

/// Add a track to what is heard, or take it out. The last one stays: a mix of
/// nothing is silence, which nobody asked for with a tick box.
export async function toggleMixTrack(track: Track) {
  if (!audioMix.multi) return;
  const ticked = audioMix.ticked;
  const next = ticked.includes(track.id) ? ticked.filter((id) => id !== track.id) : [...ticked, track.id];
  if (!next.length) return;
  try {
    await writeMix(next);
    const only = player.audioTracks.find((candidate) => candidate.id === next[0]);
    showOsd(next.length > 1 ? t('osd.mix_on', { n: next.length }) : (only?.label ?? ''));
  } catch (e) {
    showOsd(t('osd.track_failed'));
    console.warn('audio mix failed:', e);
  }
}
