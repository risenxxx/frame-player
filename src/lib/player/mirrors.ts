/**
 * What mpv is observed for, and the one way a report reaches a mirror.
 *
 * `OBSERVED` is the list handed to mpv (its order is the order of
 * registration), `RESYNC` is typed against it, and `applyProperty` is shared by
 * the observer and the 1 s sweep. The freshness helpers live here because
 * `applyProperty` is what stamps and releases them.
 */

import {
  getProperty,
  type MpvEventFromProperties,
  type MpvObservableProperty,
} from 'tauri-plugin-libmpv-api';

import { loadChapters } from './chapters';
import { syncSdrColor } from './filters';
import { player } from './state.svelte';
import { loadTracks } from './tracks';

export const OBSERVED = [
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
  // How far the VO turns the picture as it draws it. `dwidth`/`dheight` are
  // the size *before* that turn (measured on 0.41 with a rotating VO: a clip
  // carrying a 90° display matrix reports 1280x720 and is drawn upright), so
  // anything that needs the shape on screen needs this as well. It is the sum
  // of the file's own rotation and `video-rotate`, which is why the window's
  // shape reads this one and not the option.
  ['video-out-params/rotate', 'int64', 'none'],
  ['sub-delay', 'double', 'none'],
  ['audio-delay', 'double', 'none'],
  // Observed so the subtitle menu can tick the factor in force, including one
  // from the user's own mpv.conf.
  ['sub-speed', 'double', 'none'],
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
  // `WxH+X+Y`, or empty for none. dwidth/dheight follow it (measured), which is
  // what lets the hover preview take the cropped shape; the window applies it
  // to the decoder's size itself (`decodedShape`).
  ['video-crop', 'string', 'none'],
  // Frames that were lost, and where. Observed rather than read when the media
  // info panel opens, and that is the whole point of them: a stall lasts a
  // fraction of a second and happens while the viewer is watching the video,
  // so a counter that only runs while a dialog is up has already missed every
  // event worth recording by the time it is read. What the panel needs is not
  // the total — a few drops are the ordinary cost of a seek — but *when the
  // last one landed*, which only something running the whole time can answer.
  //
  // `vo-delayed-frame-count` is deliberately not here: it only ever moves under
  // `video-sync=display-*`, and with mpv's default `audio` it would be a row
  // reading zero whatever went wrong.
  ['frame-drop-count', 'int64', 'none'],
  ['decoder-frame-drop-count', 'int64', 'none'],
  // The picture's transfer function as it *enters* the filter chain — the
  // file's own, or mpv's guess for a file that declares none (`bt.1886` for
  // video, `srgb` for an RGB image). Before the chain matters: the SDR color
  // mode's own `format=gamma=…` changes `video-out-params`, never this, so the
  // mode cannot end up deciding about itself. It decides whether that mode may
  // touch the picture at all (`syncSdrColor`), and it is observed rather than
  // read on `file-loaded` because the decoder has reported nothing by then.
  ['video-params/gamma', 'string', 'none'],
  // The picture as the decoder hands it to the filter chain: the frame, its
  // display size (aspect applied) and its turn. The window's shape is built
  // from these and not from `dwidth`/`dheight`, which are the *VO's* and lag
  // behind: a file opened mid-way decodes from the keyframe up to the start
  // position and throws those frames away, and for all that time the VO is
  // still painting `force-window`'s 960×540 field while these — and `gamma`
  // with them — already describe the film (measured on 0.41, `start=30`: 160
  // ms between the two). One struct on mpv's side, so the five arrive in one
  // batch. `w`/`h` are reported before the first frame too, from the codec;
  // `dw`, `dh` and `rotate` are not, and are what says a frame exists.
  ['video-params/w', 'int64', 'none'],
  ['video-params/h', 'int64', 'none'],
  ['video-params/dw', 'int64', 'none'],
  ['video-params/dh', 'int64', 'none'],
  ['video-params/rotate', 'int64', 'none'],
  // The VO's surface in device pixels and the picture's margins inside it:
  // how far the picture is enlarged on screen, which is what decides whether
  // an upscaling shader runs at all (upscale-plan.ts). Reported on every
  // resize, so the six arrive together as the window moves.
  ['osd-dimensions/w', 'int64', 'none'],
  ['osd-dimensions/h', 'int64', 'none'],
  ['osd-dimensions/ml', 'int64', 'none'],
  ['osd-dimensions/mr', 'int64', 'none'],
  ['osd-dimensions/mt', 'int64', 'none'],
  ['osd-dimensions/mb', 'int64', 'none'],
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
  'sub-speed': true,
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
  'video-crop': true,
  // A stall is entered and left by one event each, and it is the *leaving* one
  // that matters: `player.stalled` puts "waiting for data" in the top bar, so a
  // dropped `false` leaves that sentence standing over playing video with
  // nothing left to take it down.
  'paused-for-cache': true,
  // Swept, even though the sweep dates a drop to the tick that noticed it
  // rather than to the moment it happened. The error is under a second and the
  // readout is measured in "was that hitch just now"; against it, a lost event
  // leaves the count standing until the *next* drop, and the events are most
  // likely to be lost during exactly the load being diagnosed.
  'frame-drop-count': true,
  'decoder-frame-drop-count': true,
  // A lost change leaves the previous file's curve standing: an HDR film after
  // an SDR one would be drawn through a filter that re-labels it as gamma 2.2.
  // The sweep is cheap here because `syncSdrColor` only acts on a change.
  'video-params/gamma': true,
  // Set once per file and then stand, and the window's shape is held to them:
  // lost, the window keeps the previous file's shape until the next one.
  'video-params/w': true,
  'video-params/h': true,
  'video-params/dw': true,
  'video-params/dh': true,
  'video-params/rotate': true,
  // Re-reported at the next resize, but a window left alone after the lost
  // one never resizes again: the panel would go on saying the upscaler is not
  // needed in a window that has since been made full screen.
  'osd-dimensions/w': true,
  'osd-dimensions/h': true,
  'osd-dimensions/ml': true,
  'osd-dimensions/mr': true,
  'osd-dimensions/mt': true,
  'osd-dimensions/mb': true,

  // Re-reported by mpv on its own, so a dropped one costs a tick at most.
  'time-pos': false,
  duration: false,
  'cache-buffering-state': false,
  'dwidth': false,
  'dheight': false,
  // Set once per file and then stands, and what hangs off it is the shape the
  // window is held to: lost, an upright clip keeps a landscape window until
  // the next file.
  'video-out-params/rotate': true,
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

/**
 * Resolve when mpv next reports a *different* `time-pos`, or give up after `ms`.
 *
 * A frame step that plays a frame is not a seek: there is no `playback-restart`
 * for it, and the position moving is the only sign it happened. Released from
 * the event path only when the value changed, so the 1 s resync sweep
 * rewriting the same number does not count. A wake-up, not a verdict: mpv's
 * observer folds changes together, so a position that went away and came back
 * sends nothing — `waitMovedFrom` in frame-step.svelte.ts confirms with a read.
 */
export function waitTimePosChange(ms: number): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, ms);
    timePosWaiters.push(() => {
      clearTimeout(timer);
      resolve();
    });
  });
}

let timePosWaiters: Array<() => void> = [];

/**
 * Write one property-change into its mirror.
 *
 * **Both paths into the mirrors go through here**: the observer, and the 1 s
 * sweep below. They used to be two hand-written copies of the same mapping, and
 * that is the shape the `playlist-pos` bug had — not a wrong line, a missing
 * one, in the copy nobody reads while writing the other.
 */
export function applyProperty(ev: PropertyChange) {
  switch (ev.name) {
    case 'pause': player.paused = ev.data; break;
    case 'time-pos': {
      const before = player.timePos;
      player.timePos = ev.data ?? 0;
      timePosAt = performance.now();
      if (player.timePos !== before && timePosWaiters.length) {
        const waiting = timePosWaiters;
        timePosWaiters = [];
        for (const w of waiting) w();
      }
      break;
    }
    case 'duration': player.duration = ev.data ?? 0; break;
    case 'filename': player.filename = ev.data; break;
    case 'path': player.filePath = ev.data; break;
    case 'media-title': player.mediaTitle = ev.data; break;
    case 'volume': player.volume = ev.data; break;
    case 'mute': player.muted = ev.data; break;
    case 'speed': player.speed = ev.data; break;
    case 'sub-delay': player.subDelay = ev.data ?? 0; break;
    case 'audio-delay': player.audioDelay = ev.data ?? 0; break;
    case 'sub-speed': player.subSpeed = ev.data ?? 1; break;
    // The mode is ours; mpv is only told about it. Reading it back would
    // make `all` on a one-entry playlist (which also sets loop-file) look
    // like `one` and flip the button under the viewer.
    case 'loop-file': break;
    case 'eof-reached': player.eofReached = ev.data ?? false; break;
    case 'playlist-pos': player.playlistPos = ev.data; break;
    case 'playlist-count': player.playlistCount = ev.data; break;
    case 'dwidth': player.videoW = ev.data ?? 0; break;
    case 'dheight': player.videoH = ev.data ?? 0; break;
    case 'video-out-params/rotate': player.voRotate = ev.data ?? 0; break;
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
    case 'video-crop': player.videoCrop = ev.data ?? ''; break;
    case 'frame-drop-count': player.dropVo.note(ev.data); break;
    case 'decoder-frame-drop-count': player.dropDecoder.note(ev.data); break;
    case 'video-params/gamma':
      player.sourceTransfer = ev.data;
      syncSdrColor();
      break;
    case 'video-params/w': player.decoded.w = ev.data ?? 0; break;
    case 'video-params/h': player.decoded.h = ev.data ?? 0; break;
    case 'video-params/dw': player.decoded.dw = ev.data ?? 0; break;
    case 'video-params/dh': player.decoded.dh = ev.data ?? 0; break;
    case 'video-params/rotate': player.decoded.rotate = ev.data ?? 0; break;
    case 'osd-dimensions/w': player.osd.w = ev.data ?? 0; break;
    case 'osd-dimensions/h': player.osd.h = ev.data ?? 0; break;
    case 'osd-dimensions/ml': player.osd.ml = ev.data ?? 0; break;
    case 'osd-dimensions/mr': player.osd.mr = ev.data ?? 0; break;
    case 'osd-dimensions/mt': player.osd.mt = ev.data ?? 0; break;
    case 'osd-dimensions/mb': player.osd.mb = ev.data ?? 0; break;
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

/// mpv answers an unset loop point with the literal "no", so the mirrors parse
/// rather than cast: `Number('no')` is NaN and would read as a position.
function parseAbPoint(raw: string | null): number | null {
  if (!raw || raw === 'no') return null;
  const value = Number(raw);
  return Number.isFinite(value) ? value : null;
}
