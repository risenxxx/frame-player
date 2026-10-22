/**
 * Frame stepping through mpv, made fast where mpv's own stepping is slow.
 *
 * A step back is an exact seek, so its cost is a decode from the previous
 * keyframe — tens of milliseconds on a film with two-second groups, a second
 * or more on the long-group files sites hand out. Two things change that, both
 * measured (docs/rules/mpv-playback.md, the frame-stepping line):
 *
 * 1. **The decoder goes to software for the session.** Hardware decoding makes
 *    a step back 2–10× slower, and switching costs ~10 ms. It comes back only
 *    while mpv is paused — just before our own controls resume playback, at
 *    the next pause after somebody else resumed it, or with a seek made while
 *    paused — because a decoder change re-initialises at once,
 *    and during playback the new decoder is fed the packets already in flight,
 *    from the middle of a group: a hardware AV1 or VP9 decoder fails on them
 *    and mpv settles on software for the rest of the file.
 * 2. **On a file where a step back is still slow, mpv decodes backward** from
 *    the second step back in a row (one alone, or steps that alternate with
 *    forward ones, stay native: turning costs more than a native step).
 *    `play-direction=backward` decodes the group once and keeps it, so the
 *    presses after it cost one frame period. It holds that group decoded, so
 *    it runs only inside a memory budget and only after a probe of the group
 *    length; and since an overflowing buffer drops frames *silently*, every
 *    step's landing is checked and a bad one ends it for the file.
 *
 * Both are written through `file-local-options/…`, so mpv itself puts the
 * direction and the decoder back when the file changes — a next episode that
 * opened playing backward from its last frame is the failure that rules out
 * anything else.
 *
 * Everything else that moves the playhead has to bring mpv forward first:
 * mpv's absolute seeks *replace* a queued one, and turning the direction queues
 * an exact seek to the current position, so a viewer's seek sent before the
 * turn would be silently lost. `frameStepBeforeSeek` is that gate.
 */

import { invoke } from '@tauri-apps/api/core';
import { command, getProperty, setProperty } from 'tauri-plugin-libmpv-api';

import {
  frameDuration,
  landedRight,
  reversalBudget,
  reversalNeed,
  shouldReverse,
  SLOW_BACKSTEP_MS,
  stretchInBands,
  swapsDecoder,
} from './frame-step-plan';
import { t } from './i18n.svelte';
import { dismissOsd, osdSeq, showOsd } from './osd.svelte';
import { isNetworkSource, player, waitPlaybackSettled, waitTimePosChange } from './player.svelte';
import { parseTorrentUrl } from './source';
import { torrent } from './torrent.svelte';

const PREFS_KEY = 'frameplayer.frameStep';
/// A step that never reports must not hold the keys for ever.
const STEP_TIMEOUT_MS = 4000;
/// Turning backward decodes a whole group before anything is shown: 2.3 s on a
/// 1080p60 AV1 file with ten-second groups.
const ENTER_TIMEOUT_MS = 8000;
/// A `pause` flicker of our own step can arrive just after the step resolved.
const OWN_FLICKER_MS = 150;
/// How often a step that plays a frame looks at the position.
const POLL_MS = 25;

interface GopProbe {
  start: number;
  next: number | null;
  prev: number | null;
  longest: number;
  ram: number;
}

class FrameStep {
  /// mpv is decoding backward for us.
  reverse = $state(false);
  /// The viewer's switch: Settings → Video → fast stepping back.
  enabled = $state(true);
}

export const frameStep = new FrameStep();

(() => {
  try {
    const saved = JSON.parse(localStorage.getItem(PREFS_KEY) ?? '{}') as { reverse?: boolean };
    if (typeof saved.reverse === 'boolean') frameStep.enabled = saved.reverse;
  } catch {
    // a damaged entry is the default
  }
})();

export function setFastBackStep(on: boolean) {
  frameStep.enabled = on;
  localStorage.setItem(PREFS_KEY, JSON.stringify({ reverse: on }));
  if (!on && frameStep.reverse) void leaveReverse(false);
}

/// What is known about the file being stepped through.
interface FileState {
  path: string;
  /// A native step back was slow (null: not measured yet).
  slow: boolean | null;
  /// Where the bytes sit on this disk, and whether all of them have arrived
  /// (see `diskFile`); null until asked, or when there is no such file.
  disk: DiskFile | null;
  probe: Promise<GopProbe | null> | null;
  /// The longest group the probe measured, kept for the checks on a torrent
  /// still downloading, which run at every step.
  longest: number;
  /// Backward playback misbehaved here; native from now on.
  broken: boolean;
}

let file: FileState | null = null;
/// The `hwdec` value to give back, while the session holds the decoder in
/// software.
let swappedFrom: string | null = null;
/// Playback has run since the decoder was taken: the session is over and the
/// decoder goes back at the next moment mpv is paused.
let resumedSinceSwap = false;
/// Giving it back is an exact seek in hardware; a step waits for it rather
/// than have mpv drop the step's seek while that one runs.
let restoring: Promise<void> | null = null;
/// The audio track to give back, while backward decoding has it switched off.
let audioFrom: string | null = null;
/// Steps back in a row, reset by a step forward.
let backRun = 0;
/// One step at a time; a held key drops presses instead of queueing them.
let busy = false;
/// A step of ours is in flight: its `pause` flicker and restart are ours.
let stepping = false;
let lastStepEnd = 0;
/// The viewer moved the playhead while a step was in flight: that step's
/// landing is no longer the step's to check, let alone to correct.
let superseded = false;
let leaving: Promise<void> | null = null;

function fileState(path: string): FileState {
  if (!file || file.path !== path) file = { path, slow: null, disk: null, probe: null, longest: 0, broken: false };
  return file;
}

async function fresh(name: string): Promise<number | null> {
  const v = await getProperty(name, 'double').catch(() => null);
  return typeof v === 'number' && Number.isFinite(v) ? v : null;
}

async function fps(): Promise<number | null> {
  return (await fresh('container-fps')) ?? (await fresh('estimated-vf-fps'));
}

/**
 * Until `time-pos` reads something other than `before`, or `ms` runs out.
 *
 * A step played forward (or backward, out of the reversal queue) is not a seek,
 * so no `playback-restart` says it is done, and the property event is not
 * enough on its own: mpv's observer folds changes together and compares with
 * the last value it *sent*, so a step back followed by a step forward — 34.450
 * → 34.433 → 34.450 — can arrive as no event at all. Measured in the app: the
 * step forward waited out its whole timeout. So the event only wakes this up,
 * and a fresh read decides.
 */
async function waitMovedFrom(before: number | null, ms: number) {
  for (let waited = 0; waited < ms; waited += POLL_MS) {
    await waitTimePosChange(POLL_MS);
    const now = await fresh('time-pos');
    if (now !== null && now !== before) return;
  }
}

function probe(path: string, pos: number): Promise<GopProbe | null> {
  return invoke<GopProbe>('frame_step_probe', { path, pos }).catch(() => null);
}

interface DiskFile {
  path: string;
  complete: boolean;
}

/**
 * Where the bytes of what mpv plays sit on this disk, or null.
 *
 * A local file is its own path, and whole. A torrent is played from the
 * loopback server, but its file is in the cache — `torrent_local_path` says
 * where and whether every byte has arrived. One still downloading is sparse: a
 * stretch that has not arrived reads back as **zeros** rather than failing, so
 * nothing may be read from it, and nothing decoded backward over it, without
 * the map of what has arrived saying that stretch is there (`stretchOnDisk`).
 * Any other network source has no bytes here at all.
 */
async function diskFile(path: string): Promise<DiskFile | null> {
  if (!isNetworkSource(path)) return { path, complete: true };
  const ref = parseTorrentUrl(path);
  if (!ref) return null;
  return invoke<DiskFile | null>('torrent_local_path', { infoHash: ref.infoHash, index: ref.index }).catch(
    () => null,
  );
}

/// Has the stretch `[from, to]` of the file, in seconds, arrived? Always on a
/// whole file; on a torrent still downloading, by its map of bytes.
function stretchOnDisk(f: FileState, from: number, to: number): boolean {
  if (!f.disk) return false;
  if (f.disk.complete) return true;
  const total = player.duration;
  if (!(total > 0)) return false;
  return stretchInBands(torrent.buffered, Math.max(0, from) / total, to / total);
}

/// What backward decoding from `here` would read: the group being stepped
/// through and the one before it, which mpv starts decoding as soon as the
/// current one is handed out — two of the longest groups, and a second over.
function reverseStretch(f: FileState, here: number): [number, number] {
  return [here - 2 * f.longest - 1, here];
}

/// Seconds behind the position the probe reads on a torrent still downloading
/// (the group back to its keyframe, and the one before for its length): wider
/// than any group backward decoding would be allowed to hold.
const PROBE_LOOKBACK_S = 20;

function probeFile(f: FileState, pos: number): Promise<GopProbe | null> {
  const run = (async () => {
    f.disk ??= await diskFile(f.path);
    if (!f.disk) return null;
    if (!stretchOnDisk(f, pos - PROBE_LOOKBACK_S, pos)) return null;
    const gop = await probe(f.disk.path, pos);
    if (gop) f.longest = gop.longest;
    return gop;
  })();
  // A torrent still downloading may simply not have this stretch yet: asked
  // again at the next step rather than given up on for the file.
  void run.then((gop) => {
    if (!gop && f.disk && !f.disk.complete && f.probe === run) f.probe = null;
  });
  return run;
}

/// After this long a step on a torrent still downloading says it is waiting:
/// mpv is reading from the loopback server, which holds a read of a piece that
/// has not arrived until it does, and a step that silently takes seconds reads
/// as the player having hung.
const WAITING_AFTER_MS = 1000;

export async function frameStepBy(delta: number) {
  const path = player.filePath;
  if (!player.hasFile || !path || busy) return;
  busy = true;
  stepping = true;
  superseded = false;
  const f = fileState(path);
  // Read, not the mirror: the mirror can lag a step, and this is compared
  // with a read after it.
  const before = await fresh('time-pos');
  const downloading = !!parseTorrentUrl(path) && !(f.disk ??= await diskFile(path))?.complete;
  let waitingSeq: number | null = null;
  const waiting = downloading
    ? setTimeout(() => {
        showOsd(t('step.waiting_data'), { sticky: true });
        waitingSeq = osdSeq();
      }, WAITING_AFTER_MS)
    : undefined;
  try {
    if (restoring) await restoring;
    backRun = delta > 0 ? 0 : backRun + 1;
    if (delta > 0) await stepForward();
    else await stepBack(f);
  } finally {
    clearTimeout(waiting);
    if (waitingSeq !== null) {
      // The sticky popup owes a replacement: nothing if the step got there,
      // a word if it gave up waiting.
      const now = await fresh('time-pos');
      if (now === null || now === before) showOsd(t('step.no_data'));
      else dismissOsd(waitingSeq);
    }
    stepping = false;
    busy = false;
    lastStepEnd = performance.now();
  }
}

async function stepForward() {
  // mpv parks on the last frame (`keep-open=always`); there is nothing after it
  // and a step would only wait out the timeout.
  if (player.eofReached && !frameStep.reverse) return;
  if (frameStep.reverse || leaving) await leaveReverse(true);
  const before = await fresh('time-pos');
  // `mute`: the step plays one frame, and with it a frame's worth of sound.
  await command('frame-step', [1, 'mute']).catch(() => {});
  await waitMovedFrom(before, STEP_TIMEOUT_MS);
}

async function stepBack(f: FileState) {
  const frame = frameDuration(await fps());
  const here = (await fresh('time-pos')) ?? player.timePos;
  // Nothing before the first frame — and backward, mpv would take the start of
  // the file for its end.
  if (here < frame * 0.5) return;
  await holdSoftware();
  if (frameStep.reverse) {
    // A torrent still downloading: the group mpv decodes next has to have
    // arrived, or the step waits on the network — so short of it, turn forward
    // and take this step natively, which reads only the group already here.
    if (stretchOnDisk(f, ...reverseStretch(f, here))) {
      await reverseStep(f, here, frame);
      return;
    }
    await leaveReverse(true);
  }
  const budget = backRun >= 2 ? await reverseBudgetFor(f, here) : null;
  if (budget !== null) {
    await enterReverse(f, here, frame, budget);
    return;
  }
  // The group length is asked for once per file, beside the first step, so
  // that by the second press the decision has what it needs.
  f.probe ??= probeFile(f, here);
  const settled = waitPlaybackSettled(STEP_TIMEOUT_MS);
  const startedAt = performance.now();
  await command('frame-back-step', []).catch(() => {});
  await settled;
  // The first measurement sets the verdict and later ones only tighten it, as
  // with the seek probe: a first step just after a keyframe decodes a handful
  // of frames and measured fast on a file whose later steps cost 280 ms.
  const slowNow = performance.now() - startedAt > SLOW_BACKSTEP_MS;
  f.slow = f.slow === null ? slowNow : f.slow || slowNow;
}

/// Take the decoder to software for the session, if it is in hardware and the
/// picture is small enough for that to pay (`swapsDecoder`).
///
/// The change queues an exact seek to the current frame, and **mpv drops a
/// `frame-back-step` that arrives while a precise seek is running**
/// (`add_step_frame` checks `hrseek_active`) — measured in the app: the first
/// step of every session left the picture where it was. So this waits for that
/// seek to land. It costs the first step of a session one software decode on
/// top, which is still well under one step in hardware.
async function holdSoftware() {
  resumedSinceSwap = false;
  if (swappedFrom !== null) return;
  const current = await getProperty('hwdec-current', 'string').catch(() => null);
  const w = await fresh('video-params/w');
  const h = await fresh('video-params/h');
  if (!swapsDecoder(current, w ?? 0, h ?? 0)) return;
  const original = await getProperty('hwdec', 'string').catch(() => null);
  if (!original) return;
  const settled = waitPlaybackSettled(STEP_TIMEOUT_MS);
  await setProperty('file-local-options/hwdec', 'no').catch(() => {});
  swappedFrom = original;
  await settled;
}

/// Only while paused (see the header). `untilShown` waits out the exact seek
/// the change queues; without it the caller's own seek replaces that one.
function restoreHardware(untilShown: boolean): Promise<void> {
  const original = swappedFrom;
  swappedFrom = null;
  resumedSinceSwap = false;
  if (!original) return Promise.resolve();
  const settled = untilShown ? waitPlaybackSettled(STEP_TIMEOUT_MS) : null;
  const done = setProperty('file-local-options/hwdec', original)
    .catch(() => {})
    .then(() => settled ?? undefined);
  if (untilShown) {
    restoring = done.finally(() => {
      restoring = null;
    });
  }
  return done;
}

/// The reversal buffer to ask for, or null when this step should stay native.
async function reverseBudgetFor(f: FileState, here: number): Promise<number | null> {
  if (!frameStep.enabled || f.slow !== true || f.broken || !f.probe) return null;
  const gop = await f.probe;
  const decoder = await getProperty('hwdec-current', 'string').catch(() => null);
  const graph = await getProperty('lavfi-complex', 'string').catch(() => null);
  const [w, h, bpp, rate] = await Promise.all([
    fresh('video-params/w'),
    fresh('video-params/h'),
    fresh('video-params/average-bpp'),
    fps(),
  ]);
  const need =
    gop && w && h && bpp && rate ? reversalNeed({ w, h, bpp, fps: rate }, gop.longest) : null;
  const budget = reversalBudget(gop?.ram ?? 0);
  const go = shouldReverse({
    enabled: frameStep.enabled,
    slow: f.slow,
    broken: f.broken,
    onDisk: !!gop && stretchOnDisk(f, ...reverseStretch(f, here)),
    abLoop: player.loopA !== null && player.loopB !== null,
    audioGraph: !!graph,
    software: !decoder || decoder === 'no',
    need,
    budget,
  });
  return go ? budget : null;
}

async function enterReverse(f: FileState, here: number, frame: number, budget: number) {
  await setProperty('file-local-options/video-reversal-buffer', String(budget)).catch(() => {});
  await silenceTrack();
  const settled = waitPlaybackSettled(ENTER_TIMEOUT_MS);
  // The turn is an exact seek to the current position, decoded backward: it
  // lands on the frame already shown, and the step proper follows.
  await setProperty('file-local-options/play-direction', 'backward').catch(() => {});
  frameStep.reverse = true;
  await settled;
  if (superseded) return;
  if (!landedRight(here, await fresh('time-pos'), frame)) {
    await abandonReverse(f, here);
    return;
  }
  await reverseStep(f, here, frame);
}

/**
 * Switch the audio track off for backward decoding, for the rest of the session.
 *
 * mpv decodes audio backward too, and measured on a 4K release with a 5.1 E-AC3
 * track that tripled the entry (2.0 s against 0.7 s without it) and made one
 * step out of forty skip five frames instead of one. The steps are muted
 * anyway (`frame-step … mute`), so nothing audible is lost. It stays off for
 * the forward steps after the turn too — giving it back is a seek, and a step
 * sent while one runs is undone — and comes back where the session ends: the
 * play controls, a seek, a resume by anyone (`restoreTrack`).
 */
async function silenceTrack() {
  if (audioFrom !== null) return;
  const aid = await getProperty('aid', 'string').catch(() => null);
  if (!aid || aid === 'no') return;
  await setProperty('file-local-options/aid', 'no').catch(() => {});
  audioFrom = aid;
}

/**
 * Give the audio track back — **only once mpv is turned forward**. Selecting a
 * track queues a seek to the current position, and written while mpv still
 * decodes backward that seek is resolved backward-style: measured, it landed on
 * the *next* keyframe (599.349 → 600.600, 600.809 → 602.602). After the turn's
 * `play-direction` write it lands where it should. Every caller turns first.
 */
async function restoreTrack() {
  const aid = audioFrom;
  audioFrom = null;
  if (aid) await setProperty('file-local-options/aid', aid).catch(() => {});
}

/// Backward, a step in the playing direction is a step back in time — played
/// out of the decoded group, not sought.
async function reverseStep(f: FileState, here: number, frame: number) {
  await command('frame-step', [1, 'mute']).catch(() => {});
  await waitMovedFrom(here, STEP_TIMEOUT_MS);
  if (superseded) return;
  const landed = await fresh('time-pos');
  if (landedRight(here - frame, landed, frame)) return;
  // Not moved at all on a torrent still downloading is the network, not the
  // file: turn forward, and leave backward decoding available for later.
  if (landed === here && f.disk && !f.disk.complete) {
    await leaveReverse(false);
    return;
  }
  await abandonReverse(f, here - frame);
}

/// The buffer overflowed or the file does not decode backward: go forward, put
/// the picture where the step should have landed, and stop trying on this file.
async function abandonReverse(f: FileState, target: number) {
  f.broken = true;
  console.warn('frame step: backward decoding misbehaved here, stepping natively');
  await leaveReverse(false);
  const settled = waitPlaybackSettled(STEP_TIMEOUT_MS);
  await command('seek', [Math.max(0, target), 'absolute+exact']).catch(() => {});
  await settled;
}

/// Turn mpv forward again. Resolves once the turn is queued — or, with
/// `untilShown`, once its seek has landed, which a forward step needs: a step
/// sent while that seek is pending is undone by it.
function leaveReverse(untilShown: boolean): Promise<void> {
  if (!frameStep.reverse) return leaving ?? Promise.resolve();
  frameStep.reverse = false;
  const settled = untilShown ? waitPlaybackSettled(STEP_TIMEOUT_MS) : null;
  leaving = setProperty('file-local-options/play-direction', 'forward')
    .catch(() => {})
    .then(() => settled ?? undefined)
    .finally(() => {
      leaving = null;
    });
  return leaving;
}

/**
 * Before any seek of the viewer's: bring mpv forward and give the decoder back,
 * in that order and *before* the seek is sent. Null — nothing to wait for — on
 * the common path, so callers stay synchronous outside a stepping session.
 */
export function frameStepBeforeSeek(): Promise<void> | null {
  if (!frameStep.reverse && !leaving && audioFrom === null && (swappedFrom === null || !player.paused)) {
    return null;
  }
  if (busy) {
    // Mid-step: turn forward so the seek is not lost, and leave the decoder to
    // the next resume — the step is still using it.
    superseded = true;
    return leaveReverse(false);
  }
  return (async () => {
    await leaveReverse(false);
    await restoreTrack();
    // Paused, the seek that follows replaces the exact seek the change queues,
    // so the decoder comes back for free. Playing, it waits for the next pause.
    if (swappedFrom !== null && player.paused) await restoreHardware(false);
  })();
}

/**
 * Before playback resumes through one of our own controls — the play button,
 * Space, a click on the picture, the media keys, all of which reach
 * `togglePlayback` — while mpv is still paused: turn forward and give the
 * decoder back, and wait for the seek that queues to land.
 *
 * Paused is the one moment the decoder change works (see the header), so this
 * is where a session ends cleanly. It costs the resume one exact seek in
 * hardware — measured 0.9 s on 1080p60 VP9 and AV1 with long groups, under
 * 0.2 s on a film — and it replaces the turn's own seek rather than adding to
 * it. A resume that does not come through here (the room, the end of a cast)
 * falls back to giving the decoder back at the next pause.
 */
export function frameStepBeforePlay(): Promise<void> | null {
  if (!frameStep.reverse && !leaving && swappedFrom === null && audioFrom === null) return null;
  return (async () => {
    if (restoring) await restoring;
    const hardware = swappedFrom !== null;
    // Before the commands, so the restart they cause cannot slip past.
    const settled = hardware ? waitPlaybackSettled(STEP_TIMEOUT_MS) : null;
    await leaveReverse(false);
    await restoreTrack();
    if (hardware) {
      await restoreHardware(false);
      await settled;
    }
  })();
}

/// A new file: mpv has already put the options back (they are file-local).
export function frameStepFileChanged() {
  frameStep.reverse = false;
  swappedFrom = null;
  audioFrom = null;
  resumedSinceSwap = false;
  backRun = 0;
  file = null;
}

/// From the page's `pause` hook.
export function frameStepNotePause() {
  if (player.paused) {
    // Our own steps pause mpv too; only a pause after playback ran is the
    // moment to give the decoder back.
    if (!stepping && !busy && resumedSinceSwap && swappedFrom !== null) void restoreHardware(true);
    return;
  }
  // A forward step unpauses mpv for one frame. The flicker is usually folded
  // away by mpv's observer, but not always, and may land just after the step.
  if (stepping) return;
  const sinceStep = performance.now() - lastStepEnd;
  if (sinceStep < OWN_FLICKER_MS) {
    setTimeout(() => {
      if (!player.paused && !stepping) resumed();
    }, OWN_FLICKER_MS - sinceStep);
    return;
  }
  resumed();
}

/// From the page's `playback-restart` hook: a seek of somebody else's (a
/// chapter, the room, the end of a cast) landed while mpv was going backward.
export function frameStepNoteRestart() {
  if (frameStep.reverse && !stepping) void leaveReverse(false);
}

/// Playback runs again. Through our controls the session is already over
/// (`frameStepBeforePlay`); anything else is turned forward here and keeps
/// software until the next pause.
function resumed() {
  backRun = 0;
  // Playing without its sound is not a state to leave anyone in; turned first,
  // for the reason in `restoreTrack`.
  void leaveReverse(false).then(restoreTrack);
  if (swappedFrom !== null) resumedSinceSwap = true;
}
