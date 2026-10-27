/**
 * The files of a torrent: which are the videos, in what order a queue plays
 * them, and which subtitles inside it belong to the one that is playing.
 */

import { command } from 'tauri-plugin-libmpv-api';

import { baseName, extensionOf } from '../format';
import { SUBTITLE_EXTENSIONS, VIDEO_EXTENSIONS, player } from '../player.svelte';
import { parseTorrentUrl } from '../source';
import { type TorrentFile, type TorrentInfo, torrent } from './state.svelte';

/**
 * Subtitle files inside the torrent that belong to a given video.
 *
 * Worth having because torrents routinely ship them — Sintel carries ten — and
 * without this they sit in the file list unreachable, while the viewer goes
 * looking on OpenSubtitles for something already on their disk. `sub-add` takes
 * a URL, so each one is a request to our own server; they are a couple of
 * kilobytes, so fetching them costs nothing measurable.
 *
 * Matched by name rather than by "everything in the torrent", because a season
 * carries one set per episode and attaching all ninety would make the subtitle
 * menu useless. Three shapes cover what releases actually do: the same stem
 * (`Ep01.srt` beside `Ep01.mkv`), the stem plus a language tag (`Sintel.en.srt`
 * beside `Sintel.mp4`), and a subfolder named after the episode (`Subs/Ep01/…`).
 * A single-video torrent takes all of them — there is nothing else they could
 * belong to.
 */
export function torrentSubtitles(info: TorrentInfo, videoPath: string): TorrentFile[] {
  const subs = info.files.filter((f) => SUBTITLE_EXTENSIONS.includes(extensionOf(f.path)));
  if (!subs.length) return [];

  const videos = info.files.filter((f) => VIDEO_EXTENSIONS.includes(extensionOf(f.path)));
  if (videos.length <= 1) return subs;

  const stem = baseName(videoPath).replace(/\.[^.]+$/, '').toLowerCase();
  if (!stem) return [];
  return subs.filter((f) => {
    const p = f.path.toLowerCase();
    // `includes` rather than an exact match: the stem may be followed by a
    // language tag, or be a path component of its own.
    return p.includes(stem);
  });
}

/** Video files of a torrent, in the order a queue should play them. */
export function torrentVideos(info: TorrentInfo): TorrentFile[] {
  const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });
  return info.files
    .filter((f) => VIDEO_EXTENSIONS.includes(extensionOf(f.path)))
    .sort((a, b) => collator.compare(a.path, b.path));
}

/// Guard so one file's subtitles are attached once, not on every `file-loaded`
/// (which fires again for a track change).
let subsAttachedFor: string | null = null;

/**
 * Attach the torrent's own subtitles for whatever just loaded.
 *
 * `auto` rather than `select`: mpv's `slang` should still decide which one wins,
 * exactly as it would for subtitles found beside a local file. Selecting the
 * first would override a preference the viewer has already expressed.
 */
export async function attachTorrentSubtitles() {
  const url = player.filePath;
  const info = torrent.info;
  if (!url || !info || subsAttachedFor === url) return;
  const ref = parseTorrentUrl(url);
  if (!ref || ref.infoHash !== info.info_hash) return;

  const video = info.files.find((f) => f.index === ref.index);
  if (!video) return;
  subsAttachedFor = url;

  for (const sub of torrentSubtitles(info, video.path)) {
    await command('sub-add', [sub.url, 'auto']).catch((e) =>
      console.warn('[torrent] sub-add failed:', e),
    );
  }
}
