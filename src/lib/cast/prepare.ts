/**
 * Getting a file ready for a Chromecast — the prepare rung and the HLS
 * rendition, both with their progress on the OSD — and what a DLNA renderer
 * is handed beside the file.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getProperty } from 'tauri-plugin-libmpv-api';

import { isTextSub } from '../airplay-choice';
import { t } from '../i18n.svelte';
import { showOsd } from '../osd.svelte';
import { player } from '../player.svelte';
import { castCacheCapGb } from './prefs';

const PREPARE_EVENT = 'frameplayer://cast-prepare';

/**
 * The subtitle a DLNA renderer gets beside the file: the external one chosen
 * in the player, if it is text. The television lists the file's own subtitles
 * itself and switches between them from its own menu, but it cannot know about
 * a file next to the video unless it is told — so only the viewer's choice of
 * one travels, and a choice of nothing, or of an embedded track, sends nothing
 * extra.
 */
export function dlnaSubtitle(): string | null {
  const sub = player.subTracks.find((tr) => tr.selected);
  return sub?.external && sub.path && isTextSub(sub) ? sub.path : null;
}

/**
 * Run the prepare rung with its progress on the sticky OSD.
 *
 * The announcement obeys the slow-operation rule: the popup is up before
 * ffmpeg starts, carries the real figure (ffmpeg's own out_time against the
 * known duration), and every exit path of the caller replaces it.
 */
export async function prepareForCast(
  path: string,
  transcodeAudio: boolean,
  audioIndex: number,
  channels: number,
): Promise<string> {
  // A variant already in the cache needs no popup at all: "preparing 0%" over
  // work that takes no time reads as a glitch, and anything that delays its
  // replacement makes it read as a hang.
  const cached = await invoke<string | null>('cast_prepare_cached', {
    path,
    audioIndex,
    transcodeAudio,
  }).catch(() => null);
  if (cached) return cached;

  const video = (await getProperty('video-format', 'string').catch(() => null)) ?? '';
  return withPrepareProgress(() =>
    invoke<string>('cast_prepare', {
      path,
      audioIndex,
      transcodeAudio,
      channels,
      hevcTag: video === 'hevc',
      duration: player.duration,
      capBytes: castCacheCapGb() * 1024 ** 3,
    }),
  );
}

/// ffmpeg's progress on the sticky OSD for as long as `run` takes. The popup is
/// up before the work starts and every caller replaces it on every exit path.
export async function withPrepareProgress<T>(run: () => Promise<T>): Promise<T> {
  showOsd(t('cast.preparing', { pct: 0 }), { sticky: true, progress: 0 });
  // `live` guards against a progress event already dispatched into the JS
  // queue landing AFTER the caller's final popup: a resurrected sticky has no
  // owner left to replace it, which is a popup that hangs for good.
  let live = true;
  const unlisten = await listen<number>(PREPARE_EVENT, (event) => {
    if (!live) return;
    const frac = Math.max(0, Math.min(1, event.payload));
    showOsd(t('cast.preparing', { pct: Math.round(frac * 100) }), {
      sticky: true,
      progress: frac,
    });
  });
  try {
    return await run();
  } finally {
    live = false;
    unlisten();
  }
}

/**
 * The HLS twin of `prepareForCast`: a full VOD rendition (playlist + segments)
 * in a transient session directory. No cache probe — HLS sessions are
 * regenerated each time by contract, which is the mode's storage story.
 */
export async function prepareHls(
  path: string,
  transcodeAudio: boolean,
  audioIndex: number,
  channels: number,
  fmp4: boolean,
): Promise<string> {
  return withPrepareProgress(() =>
    invoke<string>('cast_hls_prepare', {
      path,
      audioIndex,
      transcodeAudio,
      channels,
      fmp4,
      duration: player.duration,
    }),
  );
}

/// The selected audio track as the prepare rung needs it: its index among the
/// audio streams and its channel count (falling back to the live decoder's).
export async function selectedAudioParams(): Promise<{ audioIndex: number; channels: number }> {
  const selected = player.audioTracks.find((track) => track.selected) ?? null;
  const audioIndex = selected ? Math.max(0, player.audioTracks.indexOf(selected)) : 0;
  const channels =
    selected?.channels ??
    (await getProperty('audio-params/channel-count', 'int64').catch(() => null)) ??
    6;
  return { audioIndex, channels };
}

/// Which HLS rendition to build — or `null` for "this file cannot go over HLS
/// at all", which sends it down the progressive path instead.
///
/// **HLS is H.264 only on the receiver this was measured against.** Measured against a real television with a
/// rendition per cell: H.264 plays in TS and (once the LOAD stopped carrying
/// `hlsSegmentFormat`) in fMP4, while HEVC is refused in fMP4 — with the tag
/// corrected to `hvc1`, and refused a beat earlier when a master playlist
/// declares `CODECS="hvc1…"`, i.e. it is the codec being turned down and not
/// the packaging. The same TV plays 4K HEVC HDR perfectly over progressive
/// MP4, because that path reaches its own decoder rather than the receiver's
/// browser pipeline. Carrying HEVC over HLS would need a video transcode rung,
/// which is out of scope; so in HLS mode a HEVC file quietly gets the
/// progressive rung, which for a local file is the better answer anyway.
///
/// TS rather than fMP4 for the H.264 case: both play, and TS is the format the
/// Default Media Receiver has always taken.
export async function hlsVariant(): Promise<'ts' | 'fmp4' | null> {
  const video = (await getProperty('video-format', 'string').catch(() => null)) ?? '';
  return video === 'h264' ? 'ts' : null;
}
