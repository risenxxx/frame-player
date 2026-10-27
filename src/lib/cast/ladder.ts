/**
 * Which rung of the compatibility ladder a file takes, for Cast and for
 * AirPlay; what the transports can be pointed at; and the plan the picker's
 * rows are drawn from.
 */

import { invoke } from '@tauri-apps/api/core';
import { getProperty } from 'tauri-plugin-libmpv-api';

import { AIRPLAY_AUDIO_COPY, airplayMode, hlsWorthIt, tracksPlan } from '../airplay-choice';
import { t } from '../i18n.svelte';
import { isNetworkSource, player } from '../player.svelte';
import { IS_MAC } from '../platform';
import { parseTorrentUrl } from '../source';
import { extensionOf } from './devices';
import { cast, type CastVerdict } from './state.svelte';

/// Containers the receiver takes as-is over HTTP.
const DIRECT_CONTAINERS = ['mp4', 'm4v', 'webm'];
/// Containers the prepare step can losslessly repack into MP4 — anything whose
/// video/audio pass the codec gates below.
const REMUX_CONTAINERS = ['mkv', 'mov', 'avi', 'ts', 'm2ts', 'mts', 'mpg', 'mpeg', 'vob', 'flv', '3gp'];
/// HEVC/VP9/AV1 pass deliberately: whether the device decodes them depends on
/// its class, we cannot ask from the sender side, and the honest answer is to
/// try and report the TV's own refusal.
const CAST_VIDEO = ['h264', 'hevc', 'vp9', 'av1'];
/// AC-3/E-AC-3 ride along for the same reason (passthrough — measured working
/// on the real chain here). Anything else the platform never decodes — DTS,
/// TrueHD, PCM — becomes the transcode rung, not a refusal.
export const CAST_AUDIO_COPY = ['aac', 'mp3', 'opus', 'vorbis', 'flac', 'ac3', 'eac3'];


/**
 * Which rung of the compatibility ladder this source takes (casting.md):
 * direct play → remux → remux + audio transcode → refuse naming the offender.
 * Only incompatible *video* refuses now — audio is never a dead end, because
 * the E-AC-3 transcode rung swallows DTS/TrueHD/PCM. Codecs are read from mpv
 * (it is playing this same content); the container comes from the resolved
 * source path.
 */
export async function castVerdict(src: string): Promise<CastVerdict> {
  const ext = src.split('.').pop()?.toLowerCase() ?? '';
  const video = (await getProperty('video-format', 'string').catch(() => null)) ?? '';
  const audio = (await getProperty('audio-codec-name', 'string').catch(() => null)) ?? '';
  if (video && !CAST_VIDEO.includes(video)) {
    return { kind: 'refuse', reason: t('cast.err_format', { what: video }) };
  }
  const audioCopy = !audio || CAST_AUDIO_COPY.includes(audio);
  if (DIRECT_CONTAINERS.includes(ext) && audioCopy) {
    return { kind: 'direct', transcodeAudio: false };
  }
  if (DIRECT_CONTAINERS.includes(ext) || REMUX_CONTAINERS.includes(ext)) {
    return { kind: 'prepare', transcodeAudio: !audioCopy };
  }
  return { kind: 'refuse', reason: t('cast.err_format', { what: ext || '?' }) };
}

/// What an AirPlay receiver decodes: AVFoundation's set, which is not the Cast
/// receiver's. VP8/VP9 and Opus/Vorbis are not in it, so WebM stops being a
/// container that plays as it is; FLAC and ALAC ride along in MP4.
const AIRPLAY_VIDEO = ['h264', 'hevc', 'av1'];
const AIRPLAY_DIRECT = ['mp4', 'm4v', 'mov'];

/**
 * The AirPlay ladder: the Cast one with AVFoundation's edits. **HEVC is always
 * prepared**, even in an MP4: AVFoundation plays it only tagged `hvc1`, the tag
 * cannot be read from mpv, and a remux that re-tags it is seconds and cached —
 * against a black screen with no error if the tag was `hev1`. AV1 passes as an
 * attempt; it plays only where the receiver's hardware has it.
 */
export async function airplayVerdict(src: string): Promise<CastVerdict> {
  const ext = extensionOf(src);
  const video = (await getProperty('video-format', 'string').catch(() => null)) ?? '';
  const audio = (await getProperty('audio-codec-name', 'string').catch(() => null)) ?? '';
  if (video && !AIRPLAY_VIDEO.includes(video)) {
    return { kind: 'refuse', reason: t('cast.err_format', { what: video }) };
  }
  const audioCopy = !audio || AIRPLAY_AUDIO_COPY.includes(audio);
  const container = AIRPLAY_DIRECT.includes(ext) || REMUX_CONTAINERS.includes(ext) || ext === 'webm';
  if (!container) return { kind: 'refuse', reason: t('cast.err_format', { what: ext || '?' }) };
  if (AIRPLAY_DIRECT.includes(ext) && audioCopy && video !== 'hevc') {
    return { kind: 'direct', transcodeAudio: false };
  }
  return { kind: 'prepare', transcodeAudio: !audioCopy };
}

/// The AirPlay row's second line: what happens to this file, as every other
/// row says it.
export function airplaySummary(): string {
  const plan = cast.plan;
  if (!plan) return t('cast.airplay_sum');
  if (plan.streaming) return t('cast.sum_stream_wait');
  if (isNetworkSource(plan.src)) return t('cast.local_only');
  const verdict = plan.airplayVerdict;
  if (!verdict) return t('cast.airplay_sum');
  if (verdict.kind === 'refuse') return t('cast.sum_refuse');
  // Tracks and subtitles mean an HLS rendition is cut first — seconds, but a
  // preparation all the same.
  const tracks = hlsWorthIt(tracksPlan(player.audioTracks, player.subTracks));
  if (airplayMode() !== 'original' && tracks) return t('cast.airplay_sum_tracks');
  return verdict.kind === 'prepare' ? t('cast.sum_prepare') : t('cast.sum_direct');
}

/**
 * What actually gets fed to the TV / ffmpeg for the current mpv source.
 *
 * A local path answers itself. A torrent stream resolves to its file on disk —
 * complete files only: the pieces live in sparse files where a hole reads as
 * zeros, so an incomplete file must never reach a decoder directly. Streaming
 * an incomplete torrent is the planned HLS-over-the-swarm rung (casting.md);
 * until it exists the refusal says to wait for the download.
 */
/// What the transports can be pointed at: a file on disk, or a torrent still
/// downloading (which only the DLNA rung can take — see `castOverDlna`).
type CastSource =
  | { src: string }
  | { stream: { infoHash: string; index: number; name: string; ext: string } }
  | { refuse: string };

export async function resolveCastSource(path: string): Promise<CastSource> {
  const torrent = parseTorrentUrl(path);
  if (torrent) {
    const local = await invoke<{ path: string; complete: boolean } | null>(
      'torrent_local_path',
      { infoHash: torrent.infoHash, index: torrent.index },
    ).catch(() => null);
    // A complete file is an ordinary file and takes every rung, prepare
    // included; an incomplete one can only be streamed, because nothing can
    // repack bytes that have not arrived.
    if (local?.complete) return { src: local.path };
    const name = decodeURIComponent(path.split('/').pop() ?? 'video');
    return {
      stream: {
        infoHash: torrent.infoHash,
        index: torrent.index,
        name,
        ext: name.split('.').pop()?.toLowerCase() ?? '',
      },
    };
  }
  if (isNetworkSource(path)) return { refuse: t('cast.local_only') };
  return { src: path };
}

/// Read once when the picker opens: the rows need the verdict, and it is the
/// same for every row (it is about the file, not the device).
export async function refreshCastPlan() {
  const path = player.filePath;
  if (!path) {
    cast.plan = null;
    return;
  }
  const resolved = await resolveCastSource(path);
  const src = 'src' in resolved ? resolved.src : path;
  cast.plan = {
    src,
    container: extensionOf(src),
    verdict: await castVerdict(src),
    airplayVerdict: IS_MAC ? await airplayVerdict(src) : null,
    streaming: 'stream' in resolved,
  };
}
