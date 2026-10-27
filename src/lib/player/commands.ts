/**
 * The commands that drive playback: play/pause, volume and mute, repeat and
 * speed, the subtitle/audio delays, the A–B loop and the picture geometry.
 * Toggles go through `cycle` and delays through `add` — the mirrors go stale.
 */

import { command, getProperty, setProperty } from 'tauri-plugin-libmpv-api';

import { formatTime } from '../format';
import { t } from '../i18n.svelte';
import { showOsd } from '../osd.svelte';
import { resyncState } from './mirrors';
import { player, type LoopMode } from './state.svelte';

/// Amplification ceiling, in percent. Above 100 the player is amplifying and
/// can clip, so this stays well short of mpv's 1000 maximum. mpv's own default
/// is 130.
export const VOLUME_MAX = 150;

/// Step for the subtitle/audio delay nudges, in seconds. mpv's own is the same.
export const DELAY_STEP = 0.1;

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

/**
 * Mute, as a `cycle` and never as the mirror's opposite (gotcha 3): a mirror
 * that went stale sets the value mpv already has, and the popup says "sound
 * off" over sound that plays on.
 *
 * And not at all while the sound is a bitstream to a receiver: mpv's mute and
 * volume act on decoded audio, which passthrough does not have, so the button
 * would announce a change that cannot happen. The receiver's volume is the
 * only one there is then, and the popup says so.
 */
export async function toggleMute() {
  const out = await getProperty('audio-out-params/format', 'string').catch(() => null);
  if (out?.startsWith('spdif')) {
    showOsd(t('osd.volume_spdif'));
    return;
  }
  const was = await getProperty('mute', 'flag').catch(() => player.muted);
  await command('cycle', ['mute']).catch(() => {});
  showOsd(t(was ? 'osd.sound_on' : 'osd.sound_off'));
}

/**
 * Sound back on, because the volume was just raised: a muted player shows its
 * bar at zero, so a drag or a key up from there that left the mute on would
 * move a bar that snaps straight back. An absolute write, not a toggle — it
 * asks for one state rather than the mirror's opposite, so a stale mirror
 * costs at most a write of what mpv already has.
 */
export function unmute() {
  if (player.muted) void setProperty('mute', false);
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
export function loadLoopMode(): LoopMode {
  const saved = localStorage.getItem(LOOP_KEY);
  if (saved === 'off' || saved === 'all' || saved === 'one') return saved;
  if (saved === 'inf') return 'one';
  return 'off';
}

/// mpv options for a mode. `all` with a single entry in the playlist is the
/// same thing as `one` — `loop-playlist` restarts a one-entry playlist — so
/// nothing here has to special-case an open file with no queue behind it.
export function loopOptions(mode: LoopMode): { 'loop-file': string; 'loop-playlist': string } {
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
export function changeSpeed(factor: number, from = player.speed): number {
  const next = Math.round(Math.max(0.25, Math.min(4, from * factor)) * 100) / 100;
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

/// `base` is the part of mpv's value that is not this delay's — the output
/// device's offset, inside `audio-delay` (see tracks.svelte.ts). The popup
/// shows the delay without it, and a reset goes back to it rather than to zero.
export function nudgeDelay(kind: 'sub' | 'audio', delta: number, base = 0) {
  if (!player.hasFile) return;
  void command('add', [kind === 'sub' ? 'sub-delay' : 'audio-delay', delta]).catch(() => {});
  // Optimistic, like the seek popup: the property change arrives a frame or two
  // later, and a popup that lags one step behind reads as a stuck control.
  delayOsd(kind, (kind === 'sub' ? player.subDelay : player.audioDelay) - base + delta);
}

export function resetDelay(kind: 'sub' | 'audio', base = 0) {
  void setProperty(kind === 'sub' ? 'sub-delay' : 'audio-delay', base);
  delayOsd(kind, 0);
}

// ---- A–B loop -------------------------------------------------------------

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

export type PictureProp = 'video-rotate' | 'video-aspect-override' | 'panscan' | 'video-crop';

export function setPicture(prop: PictureProp, value: string) {
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
  void command('set', ['video-crop', '']).catch(() => {});
}
