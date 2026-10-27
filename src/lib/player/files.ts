/**
 * What the player can be handed: the extensions a drop is sorted by, a network
 * source told from a file on this machine, and where mpv looks for a release's
 * external tracks.
 */

import { invoke } from '@tauri-apps/api/core';
import { command } from 'tauri-plugin-libmpv-api';

import { IS_MAC } from '../platform';

export const VIDEO_EXTENSIONS = [
  'mp4', 'mkv', 'avi', 'mov', 'webm', 'wmv', 'flv', 'ts', 'm2ts', 'mts',
  'mpg', 'mpeg', 'vob', 'ogv', 'm4v', '3gp', 'rm', 'rmvb', 'y4m',
];

// Dropping one of these attaches a track to the current file instead of trying
// to play it: an .srt handed to `loadfile` used to be opened as if it were a movie.
export const SUBTITLE_EXTENSIONS = [
  'srt', 'ass', 'ssa', 'sub', 'idx', 'vtt', 'sup', 'smi', 'mks',
];
export const AUDIO_EXTENSIONS = [
  'mp3', 'aac', 'flac', 'wav', 'ogg', 'opus', 'm4a', 'ac3', 'eac3', 'dts',
  'mka', 'wma', 'alac', 'aiff',
];

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
