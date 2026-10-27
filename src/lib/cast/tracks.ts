/** A track picked in the player while a television plays: each transport answers it its own way. */

import { invoke } from '@tauri-apps/api/core';

import { isTextSub } from '../airplay-choice';
import { isPrivatePath } from '../history.svelte';
import { t } from '../i18n.svelte';
import { showOsd } from '../osd.svelte';
import { player, type Track } from '../player.svelte';
import { airplaySwitch } from './airplay';
import { endCast } from './handback';
import { CAST_AUDIO_COPY } from './ladder';
import { prepareForCast, prepareHls } from './prepare';
import { cast, session } from './state.svelte';

/**
 * Switch the TV to another audio track mid-session.
 *
 * The prepared file carries exactly one audio track, so this is a re-prepare
 * from the original source with the new track (instant when that variant is
 * already cached) and a LOAD at the TV's current position. `cast.state` is
 * deliberately left alone: the TV keeps playing the old track while ffmpeg
 * runs, and flipping to 'preparing' would hand the whole UI back to the paused
 * local player mid-cast.
 */
export async function castSwitchAudio(track: Track): Promise<void> {
  const src = session.castSrcPath;
  if (!cast.remote || !src) return;
  // Over DLNA the renderer got the original file with every track in it, so the
  // choice is the television's own and ours to stay out of — re-sending the
  // file to change it would restart playback for nothing.
  if (cast.transport === 'dlna') {
    showOsd(t('cast.track_on_tv'));
    return;
  }
  if (cast.transport === 'airplay') {
    await airplaySwitch({ audio: track });
    return;
  }
  const codec = track.codec?.toLowerCase() ?? null;
  const transcode = !(codec && CAST_AUDIO_COPY.includes(codec));
  const audioIndex = Math.max(0, player.audioTracks.indexOf(track));
  const channels = track.channels ?? 6;
  const hidden = isPrivatePath(src);
  const previous = session.preparedPath;
  try {
    // The switch follows the session's transport: an HLS session gets a fresh
    // rendition (the displaced session dir is cleaned by cast_load Rust-side),
    // a progressive one re-prepares the MP4 (cached per track).
    const prepared = session.sessionHls
      ? await prepareHls(src, transcode, audioIndex, channels, session.sessionHls === 'fmp4')
      : await prepareForCast(src, transcode, audioIndex, channels);
    if (!cast.active) return;
    await invoke('cast_load', {
      path: prepared,
      position: cast.time,
      title: hidden ? null : player.displayTitle,
      hidden,
      hls: session.sessionHls,
    });
    if (!session.sessionHls) {
      session.preparedPath = prepared;
      session.preparedHidden = hidden;
      // The variant just replaced must not linger for a private file; a public
      // one stays cached for switching back.
      if (hidden && previous && previous !== prepared) {
        void invoke('cast_forget_prepared', { path: previous }).catch(() => {});
      }
    }
    showOsd(track.label);
  } catch (e) {
    console.warn('castSwitchAudio failed:', e);
    // The session survives on the old track; the popup must still be replaced.
    if (cast.active) showOsd(t('cast.prepare_failed'));
  }
}

/// A subtitle picked while a television plays. Only AirPlay's HLS rendition
/// carries one there; an image subtitle cannot go, and is said so.
export async function castSwitchSub(track: Track | null): Promise<void> {
  if (cast.remote && cast.transport === 'dlna') {
    await dlnaSwitchSub(track);
    return;
  }
  if (!cast.remote || cast.transport !== 'airplay') return;
  if (track && !isTextSub(track)) {
    showOsd(t('cast.airplay_sub_image'));
    return;
  }
  await airplaySwitch({ sub: track });
}

/**
 * A subtitle picked in the player while a DLNA renderer plays. An external
 * one is news to the television, so the file goes again with it, from where
 * the television is — the one way to tell a renderer about a subtitle is the
 * metadata of a load. Anything else is the television's own to switch: the
 * file's subtitles are in its menu already, and so is turning them off.
 */
async function dlnaSwitchSub(track: Track | null): Promise<void> {
  const src = session.castSrcPath;
  const next = track?.external && track.path && isTextSub(track) ? track.path : null;
  if (!next || !src) {
    showOsd(t('cast.sub_on_tv'));
    return;
  }
  if (next === session.dlnaSubPath) return;
  const hidden = isPrivatePath(src);
  session.dlnaSubPath = next;
  // The load begins with a Stop, and the poll must not read that as the
  // viewer stopping the film — the same guard as moving to the next episode.
  session.advancing = true;
  try {
    await invoke('dlna_load', {
      path: src,
      position: cast.time,
      title: hidden ? null : player.displayTitle,
      hidden,
      subtitle: next,
    });
    showOsd(track!.label);
  } catch (e) {
    console.warn('dlna subtitle switch failed:', e);
    session.advancing = false;
    if (cast.active) await endCast({ osd: t('cast.err_load'), resumeLocal: true });
  } finally {
    session.advancing = false;
  }
}
