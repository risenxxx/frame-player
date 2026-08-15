/**
 * What the currently open file actually is: container, codecs, color, decoder.
 *
 * Raw values only — every one of these is read straight from mpv and left
 * unformatted, so the panel can render them through `t()` and follow a language
 * switch. Formatting here would freeze the units into whatever locale was
 * current when the panel last refreshed.
 *
 * Everything is read as string/int64/double: `track-list` is not the only node
 * property in mpv, and none of them are safe (see architecture.md).
 */

import { getProperty } from 'tauri-plugin-libmpv-api';

export interface MediaInfo {
  container: string | null;
  size: number | null;
  duration: number | null;
  video: {
    codec: string | null;
    width: number | null;
    height: number | null;
    fps: number | null;
    bitrate: number | null;
    pixelFormat: string | null;
    primaries: string | null;
    gamma: string | null;
    matrix: string | null;
    maxLuma: number | null;
    hwdec: string | null;
  };
  audio: {
    codec: string | null;
    lang: string | null;
    channels: string | null;
    sampleRate: number | null;
    bitrate: number | null;
  };
  playback: Playback;
}

/**
 * How playback is actually going, as opposed to what the file is.
 *
 * These are the live numbers only; the drop counters that go beside them are
 * **not** here, and that is deliberate. A stall lasts a fraction of a second,
 * so anything that only reads while this panel is open has missed it — the
 * counters are observed continuously in `player.svelte.ts` and the panel merely
 * displays what was recorded. What is left here is what is worth *sampling*,
 * because it describes the present rather than an event.
 *
 * `avsync` is the one to watch: audio does not stop for a stalled picture, so a
 * freeze shows up there as a step even when it costs no counted drop at all.
 */
export interface Playback {
  avsync: number | null;
  /// What is actually reaching the screen, averaged over recent frames — it
  /// dips for seconds after a stall, which is what makes a rare one visible to
  /// somebody who opened the panel just afterwards.
  fpsActual: number | null;
  /// Seconds of demuxed data ahead of the playhead. For a torrent (or any
  /// other network source) this is what separates "the picture stalled" from
  /// "the bytes stopped arriving" — the second one takes the audio with it.
  cacheAhead: number | null;
}

const str = (name: string) =>
  getProperty(name, 'string')
    .then((v) => v?.trim() || null)
    .catch(() => null);
const num = (name: string) => getProperty(name, 'double').catch(() => null);
const int = (name: string) => getProperty(name, 'int64').catch(() => null);

/**
 * Read everything in one pass.
 *
 * Sequential rather than `Promise.all`: these are dozens of round-trips over
 * the same channel the player is using, and the panel is refreshed on a timer
 * while it is open — the same reasoning as the track and chapter lists.
 */
export async function loadMediaInfo(): Promise<MediaInfo> {
  return {
    container: await str('file-format'),
    size: await int('file-size'),
    duration: await num('duration'),
    video: {
      // The long name ("H.265 / HEVC (High Efficiency Video Coding)") rather
      // than `video-format` ("hevc"): this panel exists to be readable.
      codec: await str('video-codec'),
      width: await int('video-params/w'),
      height: await int('video-params/h'),
      // `container-fps` is what the file claims; `estimated-vf-fps` is what is
      // actually arriving. The claim is the useful one for identifying a file,
      // and it is missing on streams, where the estimate takes over.
      fps: (await num('container-fps')) ?? (await num('estimated-vf-fps')),
      // Averaged over the last few seconds of packets, so it is simply absent
      // for the first moments of playback — measured on a 4K HEVC file, audio
      // had a figure while video still had none.
      bitrate: await num('video-bitrate'),
      pixelFormat: await str('video-params/pixelformat'),
      primaries: await str('video-params/primaries'),
      gamma: await str('video-params/gamma'),
      matrix: await str('video-params/colormatrix'),
      maxLuma: await num('video-params/max-luma'),
      hwdec: await str('hwdec-current'),
    },
    audio: {
      codec: await str('audio-codec'),
      lang: await str('current-tracks/audio/lang'),
      // "5.1(side)" — more use than the bare channel count next to it.
      channels: await str('audio-params/channels'),
      sampleRate: await int('audio-params/samplerate'),
      bitrate: await num('audio-bitrate'),
    },
    playback: {
      avsync: await num('avsync'),
      fpsActual: await num('estimated-vf-fps'),
      cacheAhead: await num('demuxer-cache-duration'),
    },
  };
}

/**
 * Average bitrate over the whole file, from its size and duration.
 *
 * Worth having next to mpv's live figures because it is available immediately
 * and never disappears: the live ones are a rolling average of recent packets,
 * so they are missing right after a file opens and swing about afterwards.
 */
export function overallBitrate(info: MediaInfo): number | null {
  if (!info.size || !info.duration || info.duration <= 0) return null;
  return (info.size * 8) / info.duration;
}

/**
 * The color line: transfer, primaries, peak brightness.
 *
 * Kept as raw mpv vocabulary (`pq`, `bt.2020`) rather than translated — these
 * are the names on the spec sheet and on every other tool's screen, and a
 * translated "перцептивная квантизация" would help nobody identify a file.
 * Dolby Vision is worth calling out by name, since mpv reports it as the
 * color matrix and it is otherwise invisible here.
 */
export function colorLine(video: MediaInfo['video']): string | null {
  const parts: string[] = [];
  if (video.matrix === 'dolbyvision') parts.push('Dolby Vision');
  if (video.gamma) parts.push(video.gamma.toUpperCase());
  if (video.primaries) parts.push(video.primaries.toUpperCase());
  return parts.length ? parts.join(' · ') : null;
}

/** True when the file carries an HDR transfer function. */
export function isHdr(video: MediaInfo['video']): boolean {
  return video.gamma === 'pq' || video.gamma === 'hlg';
}
