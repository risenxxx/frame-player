/**
 * Starting a file where the viewer left off, and keeping the seekbar honest
 * while that happens.
 *
 * Two things arrive at the same moment and neither is available when it is
 * needed. mpv reports `duration` only once it has opened the file, and the OSC
 * is on screen before that — so the knob is drawn as `seek.value / 0`, sits at
 * the far left, and then jumps. And `time-pos` reports zero for the whole of
 * that window, so anything written into the knob is written straight back out.
 * The history record the player is about to resume from already holds both
 * numbers — they are what the "resuming" popup prints — so the bar borrows them
 * until mpv has its own.
 *
 * There are three ways in and they differ only in where the position comes
 * from: the position store (the ordinary case), the snapshot left behind by an
 * update (`setPendingResume`), and nothing at all, which still has to *clear*
 * the knob or the previous file's position would be drawn against the new
 * file's duration.
 *
 * The seek itself is a load-time option rather than a seek after the fact.
 * `start` is set before `loadfile` so mpv decodes from the resume point; the old
 * "load, then exact seek on file-loaded" presented frame 0 for a beat before
 * jumping. `startPrimed` records that this happened, because then `applyResume`
 * must not seek a second time — it only has to release the knob.
 */

import { command, setProperty } from 'tauri-plugin-libmpv-api';

import { formatTime } from './format';
import { RESUME_OFFSET, positionsLoad } from './history.svelte';
import { t } from './i18n.svelte';
import { showOsd } from './osd.svelte';
import { player } from './player.svelte';
import { seek } from './seek.svelte';
import { sourceId } from './source';

/// Below this the file was barely started and there is nothing to resume.
const RESUME_FLOOR = 15;

/// How long the knob may be held at the primed position before it is released
/// regardless. A ceiling for a load that never completes, not a schedule: the
/// normal release is `applyResume`.
const HOLD_CEILING_MS = 8000;

class Resume {
  /// Position and duration from history, for the file currently opening. Null
  /// once mpv has its own numbers, or when there was nothing to resume.
  hint = $state<{ pos: number; dur: number } | null>(null);

  /// What the seekbar divides by. mpv's own duration the moment it exists, the
  /// remembered one before that — **display only**. Seek arithmetic must keep
  /// using `player.duration`: you cannot seek inside a file whose length is
  /// still a guess.
  barDuration = $derived(player.duration > 0 ? player.duration : (this.hint?.dur ?? 0));
}

export const resume = new Resume();

let holdTimer: ReturnType<typeof setTimeout> | undefined;
/// The snapshot an update left behind, consumed by the first load after it.
let pending: { pos: number; paused: boolean } | null = null;
/// The load went out with mpv's `start` pointing at the resume position, so
/// `applyResume` must not seek again.
let startPrimed = false;

/// Hand over the position an update restart is resuming from. It outranks the
/// store: the snapshot was written seconds ago and the store's last write may
/// be up to five seconds older.
export function setPendingResume(pos: number, paused: boolean) {
  pending = { pos: Math.max(0, pos - RESUME_OFFSET), paused };
}

/**
 * Everything that has to happen before `loadfile`.
 *
 * Awaited by the `beforeLoad` hook, and that is not incidental: `start` is a
 * load-time option, so it has to land first or mpv opens at zero and the seek
 * becomes visible again.
 */
export async function prepareResume(path: string) {
  primeResumeKnob(path);
  const target = pending?.pos ?? resume.hint?.pos;
  startPrimed = target !== undefined;
  await setProperty('start', startPrimed ? String(target) : 'none').catch(() => {});
}

/**
 * The knob alone, with no `start` and no mpv call.
 *
 * Launching with a file on the command line needs exactly this and no more: the
 * OSC is on screen from that moment, but mpv has not been initialized yet, so
 * setting `start` there would fail into a `catch` and — worse — record a
 * `startPrimed` that `prepareResume` is about to recompute from `beforeLoad`
 * anyway.
 */
export function primeResumeKnob(path: string) {
  resume.hint = null;
  clearTimeout(holdTimer);
  const saved = positionsLoad()[sourceId(path)];
  if (!saved || saved.pos <= RESUME_FLOOR) {
    // Nothing to resume: clear the knob rather than leaving the previous file's
    // position to be drawn against the new file's duration.
    seek.value = 0;
    return;
  }
  resume.hint = { pos: Math.max(0, saved.pos - RESUME_OFFSET), dur: saved.dur };
  seek.value = resume.hint.pos;
  // Hold the knob there: until the file is open, `time-pos` reports zero and the
  // observer would write that straight back.
  seek.settling = true;
  holdTimer = setTimeout(() => (seek.settling = false), HOLD_CEILING_MS);
}

/**
 * Seek to where the viewer left off, with the knob taken along.
 *
 * Only reached when `start` could not be used. Without moving `seek.value` here
 * the knob sits at zero until mpv reports the new position — a second or so on a
 * 4K file — and then jumps across the bar in full view; `seek.settling` is what
 * keeps it there, since the `time-pos` handler would otherwise write mpv's
 * still-zero position back.
 */
function seekTo(target: number) {
  seek.value = target;
  seek.settling = true;
  void command('seek', [target, 'absolute+exact'])
    .catch(() => {})
    .finally(() => {
      clearTimeout(holdTimer);
      seek.settling = false;
    });
}

/**
 * The file is open. Release the knob, and seek only if `start` did not already
 * do it.
 *
 * `start` is sticky, so it is cleared here: the next playlist entry loads
 * without going through `beforeLoad` and would otherwise begin at this file's
 * resume point.
 */
export function applyResume() {
  const primed = startPrimed;
  startPrimed = false;
  void setProperty('start', 'none').catch(() => {});
  if (primed) {
    // mpv is already decoding at the resume point — just release the knob.
    clearTimeout(holdTimer);
    seek.settling = false;
  }
  if (pending) {
    const p = pending;
    pending = null;
    if (!primed) seekTo(p.pos);
    if (p.paused) void setProperty('pause', true);
    return;
  }
  if (!player.filePath) return;
  const saved = positionsLoad()[sourceId(player.filePath)];
  if (!saved || saved.pos <= RESUME_FLOOR) return;
  if (!primed) seekTo(Math.max(0, saved.pos - RESUME_OFFSET));
  // `saved.dur` rather than `player.duration`: for a file that has only just
  // opened, mpv's property may not have arrived yet.
  const total = saved.dur > 0 ? saved.dur : player.duration;
  showOsd(t('osd.resume'), {
    sub: total > 0 ? `${formatTime(saved.pos)} / ${formatTime(total)}` : formatTime(saved.pos),
  });
}
