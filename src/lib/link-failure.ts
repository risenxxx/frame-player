/**
 * What the link dialog says when a link would not open.
 *
 * It used to have one answer for every network failure — "this needs yt-dlp"
 * (or "yt-dlp is out of date") — and for a direct stream that answer was wrong
 * twice over: a `.m3u8` needs no yt-dlp, and the usual reason it fails is a
 * server refusing the request, which FFmpeg states plainly in mpv's log. So the
 * verdict starts from what the server said, and blames yt-dlp only for a link
 * that yt-dlp is the way into — a page rather than a file.
 */

import { extensionOf } from './format';
import { AUDIO_EXTENSIONS, VIDEO_EXTENSIONS } from './player/files';

export type LinkFailureKind =
  /// 401/403: the server refused this request.
  | 'denied'
  /// 404/410: nothing at that address any more.
  | 'gone'
  /// Any other HTTP error status.
  | 'http'
  /// A link to a file or a stream, with no status to go on.
  | 'stream'
  /// A page: the yt-dlp explanation.
  | 'site';

export type LinkFailure = { kind: LinkFailureKind; status: number | null };

/// A stream's playlist or manifest rather than a page: HLS, DASH, a plain list.
const STREAM_EXTENSIONS = ['m3u8', 'm3u', 'mpd'];

/// A link whose path names a file mpv plays by itself — query and fragment
/// aside, which is where a signed CDN link keeps its token.
export function isDirectMediaLink(url: string): boolean {
  let path: string;
  try {
    path = new URL(url.trim()).pathname;
  } catch {
    return false;
  }
  const ext = extensionOf(path);
  return STREAM_EXTENSIONS.includes(ext) || VIDEO_EXTENSIONS.includes(ext) || AUDIO_EXTENSIONS.includes(ext);
}

export function linkFailure(url: string, status: number | null): LinkFailure {
  if (status === 401 || status === 403) return { kind: 'denied', status };
  if (status === 404 || status === 410) return { kind: 'gone', status };
  if (status !== null) return { kind: 'http', status };
  return { kind: isDirectMediaLink(url) ? 'stream' : 'site', status: null };
}
