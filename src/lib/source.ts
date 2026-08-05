/**
 * What a source *is*, as opposed to how the player currently reaches it.
 *
 * Everything the player remembers about a video — the watch position, the
 * chosen tracks, the poster — is filed under a key, and until now that key was
 * the path. A path is a fine key for a file and a poor one for anything else:
 *
 * - the same YouTube video arrives as `youtu.be/ID?si=<tracking>` from a share
 *   button and as `youtube.com/watch?v=ID` from the address bar, and those
 *   would be two unrelated entries;
 * - a torrent stream (ROADMAP 21) is served from `http://127.0.0.1:<port>/…`,
 *   and **the port changes every run** — so a key taken from the transport
 *   would forget the file between sessions, every time.
 *
 * So the identity comes from what is being watched, and the record keeps the
 * openable source next to it. A local path is its own identity, which is what
 * keeps every entry ever written still readable: `sourceId('/a/b.mkv')` is
 * `/a/b.mkv`, so there is nothing to migrate.
 */

import { isNetworkSource } from './player.svelte';

/**
 * Query parameters that identify the *visit* rather than the video.
 *
 * Only ones that are unambiguously tracking: `t`/`start` (a timestamp) and
 * `list` (playlist context) are left alone, because dropping them would be a
 * guess about what the viewer meant rather than a fact about the video.
 */
const TRACKING_PARAMS = new Set([
  'si',
  'pp',
  'feature',
  'ab_channel',
  'utm_source',
  'utm_medium',
  'utm_campaign',
  'utm_term',
  'utm_content',
  'fbclid',
  'gclid',
  'igshid',
]);

/**
 * Something to hand the torrent client rather than mpv.
 *
 * A magnet, or a link to a `.torrent` file — the two forms a torrent arrives in.
 * Both go into the same "Open link…" box as a YouTube URL and are dispatched by
 * shape, which is the entry point the roadmap called for: the viewer has a link,
 * and which protocol it needs is the player's problem.
 */
export function isTorrentLink(src: string): boolean {
  const s = src.trim();
  return isMagnet(s) || /\.torrent(?:[?#]|$)/i.test(s);
}

export function isMagnet(src: string): boolean {
  return /^magnet:/i.test(src.trim());
}

/**
 * A torrent stream served by our own local server, taken apart.
 *
 * The route is `http://127.0.0.1:<port>/t/<infohash>/<index>/<name>` (torrent.rs
 * builds it, `stream_url`), and the shape exists so that the identity can be
 * read straight back out of the URL: the info hash says *which torrent* and the
 * index says *which file in it*, while the port — the one part that changes
 * every run — is not needed at all. Recovering it from the string rather than
 * keeping a table beside the player means there is nothing to fall out of sync,
 * and a position recorded today still reads tomorrow on a different port.
 *
 * The host check is deliberately narrow. Anything on loopback answering `/t/…`
 * that is not ours would be misfiled under a torrent id, and 127.0.0.1 is where
 * every local development server in the world lives.
 */
export function parseTorrentUrl(src: string): { infoHash: string; index: number } | null {
  const match = /^https?:\/\/127\.0\.0\.1:\d+\/t\/([0-9a-f]{40})\/(\d+)(?:\/|$)/i.exec(src.trim());
  if (!match) return null;
  return { infoHash: match[1].toLowerCase(), index: Number(match[2]) };
}

/** `torrent:<infohash>/<index>` — see `parseTorrentUrl`. */
export function torrentId(infoHash: string, index: number): string {
  return `torrent:${infoHash.toLowerCase()}/${index}`;
}

/**
 * A stable key for a source.
 *
 * Local paths pass through untouched. A URL is canonicalised: the scheme and
 * host are lower-cased (they are case-insensitive, the path is not), the
 * fragment goes (it addresses a place *within* a page, not a different video),
 * tracking parameters go, and what remains is sorted — otherwise the same link
 * with its parameters in another order files itself twice.
 *
 * A torrent stream is the case the whole indirection was built for: its URL is
 * `http://127.0.0.1:<port>/…` and **the port changes every run**, so a key taken
 * from the transport would forget the film every session.
 */
export function sourceId(src: string): string {
  const torrent = parseTorrentUrl(src);
  if (torrent) return torrentId(torrent.infoHash, torrent.index);
  if (!isNetworkSource(src)) return src;
  try {
    const url = new URL(src.trim());
    url.hash = '';
    url.protocol = url.protocol.toLowerCase();
    url.hostname = url.hostname.toLowerCase();
    for (const name of [...url.searchParams.keys()]) {
      if (TRACKING_PARAMS.has(name.toLowerCase())) url.searchParams.delete(name);
    }
    url.searchParams.sort();
    // A trailing "?" left by deleting the last parameter is noise in a key.
    return url.toString().replace(/\?$/, '');
  } catch {
    // Not parseable as a URL — better an imperfect key than none.
    return src.trim();
  }
}
