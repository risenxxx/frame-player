/**
 * What the player can be handed: the extensions a drop is sorted by, a network
 * source told from a file on this machine, and where mpv looks for a release's
 * external tracks.
 */

import { invoke } from '@tauri-apps/api/core';
import { command } from 'tauri-plugin-libmpv-api';

import tauriConf from '../../../src-tauri/tauri.conf.json';
import { IS_MAC } from '../platform';

/// **One list, the file associations'.** What the player registers itself for
/// (Windows' shell registration and the macOS bundle are both written from
/// `bundle.fileAssociations`) is also what a folder queue, a torrent's file
/// list, a drop and the open dialog count as video or audio — kept twice, the
/// two had drifted (`.rmvb` was queued from a folder but never associated,
/// issue #76). The set is what FFmpeg actually demuxes, not every extension mpv
/// will try.
function associated(name: 'Video' | 'Audio'): string[] {
  const assoc = tauriConf.bundle.fileAssociations.find((a) => a.name === name);
  if (!assoc) throw new Error(`tauri.conf.json has no "${name}" file association`);
  return assoc.ext;
}

export const VIDEO_EXTENSIONS = associated('Video');

// Dropping one of these attaches a track to the current file instead of trying
// to play it: an .srt handed to `loadfile` used to be opened as if it were a movie.
export const SUBTITLE_EXTENSIONS = [
  'srt', 'ass', 'ssa', 'sub', 'idx', 'vtt', 'sup', 'smi', 'mks',
];
export const AUDIO_EXTENSIONS = associated('Audio');

/// What the extension says: a sound file. The tracks decide in the end —
/// `audio-screen.svelte.ts` waits for them — but the extension is known before
/// the file opens, which is when the queue is built and the screen is kept up.
export function isAudioPath(path: string | null): boolean {
  if (!path || isNetworkSource(path)) return false;
  const dot = path.lastIndexOf('.');
  return dot > 0 && AUDIO_EXTENSIONS.includes(path.slice(dot + 1).toLowerCase());
}

/// Longer than this, an audio file is listened to the way a film is watched —
/// an audiobook chapter, a podcast, a concert — and resumes where it stopped.
/// Shorter, it is a song: it starts from the top and stays out of the history.
export const SONG_MAX_S = 20 * 60;

export function isSong(path: string | null, duration: number): boolean {
  return isAudioPath(path) && duration > 0 && duration < SONG_MAX_S;
}

/**
 * A source mpv can open that is not a file on this machine.
 *
 * The distinction earns its own helper because a surprising amount of the
 * player is quietly local-only: the thumbnail storyboard decodes the whole file
 * in the background, the queue is built by reading a directory, "show in
 * Finder" needs something to show. Run any of those against a URL and they
 * either fail confusingly or, worse, work — by pulling the entire stream down a
 * second time.
 *
 * Scheme rather than a regex: `://` shows up in file names often enough, and
 * mpv itself dispatches on the scheme.
 */
const NETWORK_SCHEME = /^(?:https?|rtmps?|rtsp|srt|udp|mmsh?|magnet):/i;

export function isNetworkSource(path: string | null): boolean {
  return !!path && NETWORK_SCHEME.test(path.trim());
}

/// The subtitle folders mpv always looks in, whatever is found on disk.
export const SUB_DIRS = ['Subs', 'subs', 'Subtitles', 'subtitles'];
export const PATH_LIST_SEP = IS_MAC ? ':' : ';';

/**
 * Point mpv's own search for external tracks at the folders this release keeps
 * them in, before the file is opened.
 *
 * mpv looks beside the video and in the folders `sub-file-paths` and
 * `audio-file-paths` name, without descending — and releases nest, one folder
 * per studio or group (`RUS Sound/<studio>/`). So the folders that exist are
 * read off the disk (external_dirs.rs) and handed over as those lists. Through
 * mpv's search rather than a `sub-add` afterwards, because a track mpv finds
 * itself is given a language from its file name and takes part in `alang`/
 * `slang` selection; one added after the load does neither.
 *
 * The answer belongs to the folder, not to the episode, so a queue moving on
 * inside one folder needs no new answer — which is why advancing the queue
 * does not wait for this. Names are relative to the video's folder, so a stale
 * list for another folder is harmless: mpv skips a directory that is not there.
 */
export async function pointAtExternalTracks(path: string) {
  if (isNetworkSource(path)) return;
  const found = await invoke<{ subs: string[]; audio: string[] }>('external_track_dirs', { path }).catch(
    () => null,
  );
  const subs = [...new Set([...(found?.subs ?? []), ...SUB_DIRS])];
  await command('set', ['sub-file-paths', subs.join(PATH_LIST_SEP)]).catch(() => {});
  await command('set', ['audio-file-paths', (found?.audio ?? []).join(PATH_LIST_SEP)]).catch(() => {});
}
