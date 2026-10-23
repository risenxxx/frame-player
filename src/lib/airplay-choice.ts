/**
 * What AirPlay sends a television, decided before and after the viewer picks
 * one — pure, so every branch is tested (airplay-choice.test.ts).
 *
 * Two ways exist (docs/airplay.md): an HLS rendition carrying every audio
 * track and every text subtitle, switched on the receiver without a reload,
 * and the MP4 copy — the original picture with one chosen audio track and no
 * subtitles. HLS is the better one wherever the receiver takes it, and one
 * receiver measured does not take it for 4K or HDR: it fetches the master
 * playlist and nothing after it. Whether another does is a property of that
 * receiver, so "auto" tries HLS, falls back on that refusal, and remembers it
 * per receiver and per kind of picture.
 */

import type { Track } from './player.svelte';

export type AirPlayMode = 'auto' | 'tracks' | 'original';

const MODE_KEY = 'frameplayer.airplay-mode';
const REFUSALS_KEY = 'frameplayer.airplay-refusals';

export function airplayMode(): AirPlayMode {
  const v = typeof localStorage === 'undefined' ? null : localStorage.getItem(MODE_KEY);
  return v === 'tracks' || v === 'original' ? v : 'auto';
}

export function setAirPlayMode(mode: AirPlayMode): void {
  try {
    localStorage.setItem(MODE_KEY, mode);
  } catch {
    // not critical: the choice simply will not survive a restart
  }
}

/// The kinds of picture a receiver can turn an HLS rendition down for.
export type PictureKind = 'sd' | 'uhd' | 'hdr';

/// What each receiver refused over HLS, by the name it announces itself under.
export type Refusals = Record<string, Partial<Record<PictureKind, true>>>;

export function airplayRefusals(): Refusals {
  try {
    const parsed = JSON.parse(localStorage.getItem(REFUSALS_KEY) ?? '{}');
    return parsed && typeof parsed === 'object' ? (parsed as Refusals) : {};
  } catch {
    return {};
  }
}

function saveRefusals(all: Refusals): void {
  try {
    localStorage.setItem(REFUSALS_KEY, JSON.stringify(all));
  } catch {
    // a full storage costs only the six seconds the next attempt waits
  }
}

export function rememberRefusal(receiver: string, kind: PictureKind): void {
  const all = airplayRefusals();
  all[receiver] = { ...all[receiver], [kind]: true };
  saveRefusals(all);
}

export function forgetRefusals(receiver: string): void {
  const all = airplayRefusals();
  delete all[receiver];
  saveRefusals(all);
}

/**
 * The picture's kind, for the refusal memory: HDR outranks resolution (a 4K
 * HDR file that is refused says nothing about 4K SDR), and anything above
 * 1920×1080 is UHD. The measured receiver refused 1080p HDR and 4K SDR alike
 * and played 1080p SDR, so neither attribute is enough on its own.
 */
export function pictureKind(width: number, height: number, transfer: string | null): PictureKind {
  if (transfer === 'pq' || transfer === 'hlg') return 'hdr';
  if (width > 1920 || height > 1080) return 'uhd';
  return 'sd';
}

/// The `VIDEO-RANGE` an HLS master must declare — AVFoundation refuses an HDR
/// rendition declared without one.
export function videoRange(transfer: string | null): 'PQ' | 'HLG' | null {
  return transfer === 'pq' ? 'PQ' : transfer === 'hlg' ? 'HLG' : null;
}

/// How long a receiver may sit on an HLS master before its silence is a
/// refusal. Measured: an accepting set asks for the variant within a second,
/// a refusing one never does.
export const HLS_REFUSE_MS = 6000;

/**
 * Whether the receiver turned the rendition down: live long enough, and never
 * asked for anything past the master playlist (one request).
 */
export function hlsRefused(liveForMs: number, receiverRequests: number): boolean {
  return liveForMs > HLS_REFUSE_MS && receiverRequests <= 1;
}

/**
 * Which way a file goes. `tracks` forces HLS and `original` the copy; `auto`
 * takes HLS when it buys anything — more than one audio track, or a subtitle
 * the viewer can turn on — unless this receiver already refused this kind of
 * picture, which is known only once a receiver has been chosen (the queue
 * moving on along one route).
 */
export function wantsHls(
  mode: AirPlayMode,
  worthIt: boolean,
  refusals: Refusals,
  receiver: string | null,
  kind: PictureKind,
): boolean {
  if (mode === 'original') return false;
  if (mode === 'tracks') return true;
  if (!worthIt) return false;
  return !(receiver && refusals[receiver]?.[kind]);
}

/// Subtitle codecs that become WebVTT — text, whatever its dialect. An image
/// subtitle (PGS, VobSub, DVB) has no text to convert, and burning it into
/// the picture would be a video re-encode.
const TEXT_SUBS = [
  'subrip', 'srt', 'ass', 'ssa', 'webvtt', 'mov_text', 'text', 'microdvd',
  'subviewer', 'subviewer1', 'mpl2', 'vplayer', 'jacosub', 'sami', 'realtext', 'pjs', 'stl',
];
/// Captions carried inside the video stream: listed by mpv as a subtitle
/// track, not a stream of the container, so they are neither counted nor sent.
const IN_VIDEO_SUBS = ['eia_608', 'eia_708'];
/// What AVFoundation plays as it is; anything else becomes E-AC-3.
export const AIRPLAY_AUDIO_COPY = ['aac', 'mp3', 'flac', 'alac', 'ac3', 'eac3'];

export function isTextSub(track: Track): boolean {
  return !!track.codec && TEXT_SUBS.includes(track.codec.toLowerCase());
}

export interface TrackSource {
  file: string | null;
  index: number;
  lang: string | null;
  title: string | null;
  transcode: boolean;
  channels: number;
}

export interface TracksPlan {
  audio: TrackSource[];
  subs: TrackSource[];
  /// The mpv track id behind each rendition, in the master's order — what
  /// turns a pick in the track menu into an index for `airplay_select`.
  audioIds: number[];
  subIds: number[];
}

/**
 * Every audio track and every text subtitle, as the rendition will carry
 * them. An embedded track is addressed by its position among the container's
 * streams of its kind — counting the ones that are not sent (an image
 * subtitle still occupies its `s:N`), and not counting captions inside the
 * video. An external track is its own file's first stream.
 */
export function tracksPlan(audioTracks: Track[], subTracks: Track[]): TracksPlan {
  const plan: TracksPlan = { audio: [], subs: [], audioIds: [], subIds: [] };
  let embedded = 0;
  for (const tr of audioTracks) {
    const source = tr.external ? (tr.path ? { file: tr.path, index: 0 } : null) : { file: null, index: embedded++ };
    if (!source) continue;
    const codec = tr.codec?.toLowerCase() ?? '';
    plan.audio.push({
      ...source,
      lang: tr.lang,
      title: tr.title,
      transcode: !!codec && !AIRPLAY_AUDIO_COPY.includes(codec),
      channels: tr.channels ?? 6,
    });
    plan.audioIds.push(tr.id);
  }
  embedded = 0;
  for (const tr of subTracks) {
    const codec = tr.codec?.toLowerCase() ?? '';
    if (IN_VIDEO_SUBS.includes(codec)) continue;
    const source = tr.external ? (tr.path ? { file: tr.path, index: 0 } : null) : { file: null, index: embedded++ };
    if (!source || !isTextSub(tr)) continue;
    plan.subs.push({ ...source, lang: tr.lang, title: tr.title, transcode: false, channels: 0 });
    plan.subIds.push(tr.id);
  }
  return plan;
}

/// Whether HLS buys this file anything over the copy.
export function hlsWorthIt(plan: TracksPlan): boolean {
  return plan.audio.length > 1 || plan.subs.length > 0;
}
