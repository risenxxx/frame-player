/**
 * AirPlay's half of a session: the MP4 copy or the HLS rendition the receiver
 * is given, the fallback from one to the other, and a track picked while it
 * plays. The pure decisions behind it are in airplay-choice.ts.
 */

import { invoke } from '@tauri-apps/api/core';
import { getProperty } from 'tauri-plugin-libmpv-api';

import {
  AIRPLAY_AUDIO_COPY,
  airplayMode,
  rememberRefusal,
  tracksPlan,
  videoRange,
  type TrackSource,
} from '../airplay-choice';
import { isPrivatePath } from '../history.svelte';
import { t } from '../i18n.svelte';
import { showOsd } from '../osd.svelte';
import { player, type Track } from '../player.svelte';
import { endCast } from './handback';
import { airplayVerdict } from './ladder';
import { castCacheCapGb } from './prefs';
import { withPrepareProgress } from './prepare';
import { cast, session } from './state.svelte';

/**
 * Where an audio track comes from, as `cast_prepare_tracks` addresses it: an
 * embedded track by its position among the container's audio streams, an
 * external one (a dub from the release's own folders) as its file's first
 * stream. Null for an external track mpv names no file for. Pure, so tested.
 */
export function airplayAudioSource(track: Track, audioTracks: Track[]): TrackSource | null {
  let source: { file: string | null; index: number } | null;
  if (track.external) {
    source = track.path ? { file: track.path, index: 0 } : null;
  } else {
    source = { file: null, index: audioTracks.filter((tr) => !tr.external).indexOf(track) };
  }
  if (!source || source.index < 0) return null;
  const codec = track.codec?.toLowerCase() ?? '';
  return {
    ...source,
    lang: track.lang,
    title: track.title,
    transcode: !!codec && !AIRPLAY_AUDIO_COPY.includes(codec),
    channels: track.channels ?? 6,
  };
}

/**
 * The copy AirPlay plays: the video copied and **the one audio track the
 * viewer chose**, flagged default. A copy carrying every track and switching
 * them on the receiver was built and measured — an LG television played its
 * first track whatever the Mac selected, and showed no `mov_text` subtitle at
 * all, not even as the copy's only track flagged default — so the choice is
 * made in the copy, where every receiver has to honour it. Cached per track,
 * as the Cast rung's copies are.
 */
export async function prepareAirPlayCopy(src: string, track?: Track): Promise<string> {
  const audio = track ?? player.audioTracks.find((tr) => tr.selected) ?? player.audioTracks[0];
  const source = audio ? airplayAudioSource(audio, player.audioTracks) : null;
  const video = (await getProperty('video-format', 'string').catch(() => null)) ?? '';
  const args = {
    path: src,
    audio: source ? [source] : [],
    subs: [],
    hevcTag: video === 'hevc',
    duration: player.duration,
    capBytes: castCacheCapGb() * 1024 ** 3,
  };
  // A copy already in the cache needs no popup: "preparing 0%" over work that
  // takes no time reads as a glitch.
  const cached = await invoke<string | null>('cast_prepare_tracks', { ...args, probe: true }).catch(() => null);
  const path =
    cached ??
    (await withPrepareProgress(() => invoke<string | null>('cast_prepare_tracks', { ...args, probe: false })));
  if (!path) throw new Error('no copy');
  return path;
}

/**
 * Cut the HLS rendition with every audio track and every text subtitle. The
 * master declares the picture's size and range, which is what a receiver
 * decides by (docs/airplay.md: one refuses 4K and HDR from the declaration
 * alone), and AVFoundation itself refuses an HDR rendition without a range.
 */
export async function prepareAirPlayHls(src: string): Promise<string> {
  const plan = tracksPlan(player.audioTracks, player.subTracks);
  const video = (await getProperty('video-format', 'string').catch(() => null)) ?? '';
  // The container's rate, else the decoder's estimate; the master needs one
  // for an HDR picture (airplay_hls.rs, `frame_rate`).
  const fps =
    (await getProperty('container-fps', 'double').catch(() => null)) ??
    (await getProperty('estimated-vf-fps', 'double').catch(() => null)) ??
    0;
  const dir = await withPrepareProgress(() =>
    invoke<string>('airplay_hls_prepare', {
      path: src,
      plan: {
        audio: plan.audio,
        subs: plan.subs,
        hevcTag: video === 'hevc',
        width: player.videoW,
        height: player.videoH,
        frameRate: Number.isFinite(fps) ? fps : 0,
        range: videoRange(player.sourceTransfer),
        hidden: isPrivatePath(src),
      },
      duration: player.duration,
    }),
  );
  session.airplayHls = { audioIds: plan.audioIds, subIds: plan.subIds };
  cast.airplayTracks = true;
  return dir;
}

/// The rendition's indices for a choice: `audio`/`sub` override the current
/// selection (the mirror lags a pick by a round trip), a `null` sub is none.
export function airplaySelection(over: { audio?: Track; sub?: Track | null } = {}): { audio: number; sub: number } {
  const ids = session.airplayHls ?? { audioIds: [], subIds: [] };
  const audio = over.audio ?? player.audioTracks.find((tr) => tr.selected);
  const sub = 'sub' in over ? over.sub : player.subTracks.find((tr) => tr.selected);
  return {
    audio: Math.max(0, audio ? ids.audioIds.indexOf(audio.id) : 0),
    sub: sub ? ids.subIds.indexOf(sub.id) : -1,
  };
}

/**
 * Whether a file AirPlay could take as it is goes as a copy anyway: the viewer
 * is listening to an audio track the receiver would not pick by itself — any
 * but the file's first, or one from an external file. A film watched as it
 * was made goes untouched and costs no disk.
 */
export function airplayNeedsCopy(): boolean {
  const audio = player.audioTracks.find((tr) => tr.selected);
  if (!audio) return false;
  const firstEmbedded = player.audioTracks.find((tr) => !tr.external);
  return audio.external || audio.id !== firstEmbedded?.id;
}

/**
 * The receiver turned the HLS rendition down. Under "auto" the session goes on
 * as the MP4 copy at the set's position — the original picture, the chosen
 * audio track, no subtitles — and the refusal is filed under this receiver and
 * this kind of picture, so the next such file goes straight to the copy.
 * Under "tracks" the viewer asked for HLS and gets the reason instead.
 */
export async function airplayFallback(known: boolean): Promise<void> {
  const src = session.castSrcPath;
  if (!src) return;
  session.airplayFellBack = true;
  if (airplayMode() === 'tracks') {
    await endCast({ osd: t('cast.airplay_hls_refused'), resumeLocal: true });
    return;
  }
  if (session.airplayReceiver && !known) {
    rememberRefusal(session.airplayReceiver, session.airplayKind);
    cast.airplayRevision++;
  }
  console.warn(`airplay: ${session.airplayReceiver ?? 'the receiver'} refused HLS for ${session.airplayKind}, falling back to the copy`);
  session.airplayHls = null;
  cast.airplayTracks = false;
  cast.notice = t('cast.airplay_original_note');
  showOsd(t('cast.airplay_hls_fallback'), { sticky: true });
  try {
    const verdict = await airplayVerdict(src);
    const hidden = isPrivatePath(src);
    const path = verdict.kind === 'prepare' || airplayNeedsCopy() ? await prepareAirPlayCopy(src) : src;
    if (!cast.active) return;
    await invoke('airplay_replace', { path, position: cast.time, hidden, fresh: true });
    if (path !== src) {
      session.preparedPath = path;
      session.preparedHidden = hidden;
    }
    session.loadStartedAt = Date.now();
    showOsd(t('cast.airplay_hls_fallback'));
  } catch (e) {
    console.warn('airplay fallback failed:', e);
    if (cast.active) await endCast({ osd: t('cast.prepare_failed'), resumeLocal: true });
  }
}

/**
 * A track picked while AirPlay plays. On an HLS rendition that carries it, a
 * selection on the receiver — instant, no reload. A track the rendition does
 * not carry (a file loaded after it was cut) means a new rendition swapped in
 * at the set's position. On the MP4 copy, an audio track is that track's copy
 * swapped in (the receiver cannot be told to switch), and a subtitle cannot go.
 */
export async function airplaySwitch(over: { audio?: Track; sub?: Track | null }): Promise<void> {
  const src = session.castSrcPath;
  if (!cast.remote || !src) return;
  const hidden = isPrivatePath(src);
  const label = over.audio?.label ?? (over.sub ? over.sub.label : t('osd.subs_off'));
  try {
    if (session.airplayHls) {
      const carried = over.audio
        ? session.airplayHls.audioIds.includes(over.audio.id)
        : !over.sub || session.airplayHls.subIds.includes(over.sub.id);
      if (!carried) {
        const dir = await prepareAirPlayHls(src);
        if (!cast.active) return;
        await invoke('airplay_replace', { path: dir, position: cast.time, hidden });
      }
      await invoke('airplay_select', airplaySelection(over));
      showOsd(label);
      return;
    }
    if (!over.audio) {
      showOsd(t('cast.airplay_no_subs'));
      return;
    }
    const prepared = await prepareAirPlayCopy(src, over.audio);
    if (!cast.active) return;
    await invoke('airplay_replace', { path: prepared, position: cast.time, hidden });
    const previous = session.preparedPath;
    session.preparedPath = prepared;
    session.preparedHidden = hidden;
    if (hidden && previous && previous !== prepared) {
      void invoke('cast_forget_prepared', { path: previous }).catch(() => {});
    }
    showOsd(label);
  } catch (e) {
    console.warn('airplay track switch failed:', e);
    if (cast.active) showOsd(t('cast.prepare_failed'));
  }
}
