import { baseName, displayName } from '../format';
import { TORRENTS_KEY, history } from '../history.svelte';
import { player } from '../player.svelte';
import { parseTorrentUrl } from '../source';
import type { FeedOrigin } from '../feed';
import { torrentVideos } from './files';
import { addTorrent } from './session';
import { type TorrentInfo, torrent } from './state.svelte';

// ---- Remembering torrents --------------------------------------------------
//
// Three separate problems turned out to be one. A torrent entry in the watch
// history could not be reopened after a restart, because what it stores is a
// loopback URL and **the port is dead by then** — the very thing `sourceId`
// exists to work around, showing up one layer higher. The episodes of a torrent
// that were never started had no representation anywhere at all, so getting back
// into a season meant finding the magnet again. And the disk filled up silently.
//
// All three are answered by remembering, per info hash, the magnet that produced
// it. Note what is *not* kept here: the size and the folder, which are read from
// disk instead (`torrent_list`). Disk is the source of truth for storage, so
// what is taking space stays visible and deletable even with history switched
// off and nothing remembered — this store only adds the ability to *reopen*.

const STORE_KEY = TORRENTS_KEY;
/// Torrents whose data is gone are dropped on read, so this only bounds a list
/// of strings that never grows on its own.
const STORE_LIMIT = 100;

/**
 * Enough about the catalog release a torrent came from to find its successor.
 *
 * Only ever set for a torrent opened through the catalog — a pasted magnet has
 * no search to re-run, and that is the honest difference between the two ways
 * in rather than a gap to fill later.
 *
 * `url` is the exact handle (a tracker page is one release), `quality` is what
 * stops a 4K remux being offered as an update to a 1080p season, and the rest
 * is the query that produced the list.
 */
export interface CatalogOrigin {
  url: string;
  title: string;
  original: string;
  year: number | null;
  season: number | null;
  quality: number;
  /// The release's own title, which is what the name comparison runs against —
  /// `name` on the torrent is the folder's and says far less.
  release: string;
}

export interface RememberedTorrent {
  infoHash: string;
  /// What to hand `torrent_add` to get playable URLs back.
  magnet: string;
  name: string | null;
  /// Video file count, for the row's "9 файлов" without resolving anything.
  videos: number;
  at: number;
  /// Where this came from, when it came from the catalog. Absent otherwise.
  origin?: CatalogOrigin;
  /// The RSS feed this was opened from, which is what lets the start screen
  /// notice a re-upload by itself. Absent for anything else.
  feed?: FeedOrigin;
  /**
   * Episodes watched to the end, by file name.
   *
   * **A finished episode leaves no trace anywhere else.** `frameplayer.positions`
   * *deletes* its entry past the finished threshold — which is right for a
   * "continue watching" list and leaves a watched file indistinguishable from
   * one never opened. That is the fact both halves of this feature need: it is
   * what "delete the ones I have seen" reads, and what marks an episode in the
   * file picker so a season of nine near-identical names can be navigated.
   *
   * By **file name**, never by index: an uploader who inserts an episode
   * renumbers everything after it, which is exactly why positions are re-keyed
   * by name when a torrent is superseded. The name is the basename, decoded —
   * which is also all the stream URL carries, so it can be recovered even when
   * the file list is not loaded.
   */
  watched?: string[];
}

type Store = Record<string, RememberedTorrent>;

export function readStore(): Store {
  try {
    const raw = localStorage.getItem(STORE_KEY);
    return raw ? (JSON.parse(raw) as Store) : {};
  } catch {
    return {};
  }
}

/**
 * Record how to reopen a torrent.
 *
 * Privacy-gated exactly like the watch positions, and for the same reason: a
 * magnet names what is being watched as surely as a stored position does. With
 * history off nothing is written, and the storage list falls back to what disk
 * alone can say.
 */
export function rememberTorrent(info: TorrentInfo, magnet: string, origin?: CatalogOrigin) {
  if (!history.prefs.enabled) return;
  try {
    const store = readStore();
    store[info.info_hash] = {
      infoHash: info.info_hash,
      magnet,
      name: info.name,
      videos: torrentVideos(info).length,
      at: Date.now(),
      // Carried forward when this open did not come from the catalog, for the
      // same reason `watched` is: this runs on *every* open, and reopening a
      // season from the start screen would otherwise erase the one thing that
      // makes it updatable.
      origin: origin ?? store[info.info_hash]?.origin,
      // Handed over by whoever resolved the feed item, since the magnet that
      // reaches here says nothing about a feed — and carried forward otherwise,
      // for the reason `origin` is.
      feed: takeExpectedFeed(info.info_hash) ?? store[info.info_hash]?.feed,
      // Reopening a torrent must not forget which episodes were finished — this
      // runs on every open, including the one that follows a magnet being
      // pasted again.
      watched: store[info.info_hash]?.watched,
    };
    const trimmed = Object.values(store)
      .sort((a, b) => b.at - a.at)
      .slice(0, STORE_LIMIT);
    localStorage.setItem(
      STORE_KEY,
      JSON.stringify(Object.fromEntries(trimmed.map((t) => [t.infoHash, t]))),
    );
  } catch {
    // not critical
  }
}

/// Feed origins waiting for the torrent they describe to be remembered, by
/// info hash. A side channel rather than a parameter, because the open goes
/// through `openTorrent` and the update dialog, neither of which should have to
/// know that a feed exists.
export const expectedFeeds = new Map<string, FeedOrigin>();

export function expectFeed(infoHash: string, feed: FeedOrigin) {
  expectedFeeds.set(infoHash.toLowerCase(), feed);
}

function takeExpectedFeed(infoHash: string): FeedOrigin | undefined {
  const hash = infoHash.toLowerCase();
  const feed = expectedFeeds.get(hash);
  expectedFeeds.delete(hash);
  return feed;
}

/// Attach a feed to a torrent already remembered — a feed pasted into the
/// update dialog for a season that arrived by magnet.
export function setTorrentFeed(infoHash: string, feed: FeedOrigin) {
  if (!history.prefs.enabled) return;
  try {
    const store = readStore();
    const entry = store[infoHash.toLowerCase()];
    if (!entry) return;
    entry.feed = feed;
    localStorage.setItem(STORE_KEY, JSON.stringify(store));
  } catch {
    // not critical
  }
}

/// Every remembered torrent — what the start screen's feed check walks.
export function rememberedTorrents(): RememberedTorrent[] {
  return Object.values(readStore());
}

export function rememberedTorrent(infoHash: string): RememberedTorrent | null {
  return readStore()[infoHash.toLowerCase()] ?? null;
}

/**
 * Mark an episode as watched to the end.
 *
 * Privacy-gated like everything else in this store: with history off the player
 * records nothing about what is being watched, and this is exactly that.
 *
 * It writes into an entry that may not exist — a torrent opened before this
 * store did, or one whose magnet was never remembered — and does nothing in
 * that case rather than inventing a half-entry with no way to reopen it.
 */
export function markWatched(infoHash: string, name: string) {
  if (!history.prefs.enabled || !name) return;
  try {
    const store = readStore();
    const entry = store[infoHash.toLowerCase()];
    if (!entry) return;
    const watched = entry.watched ?? [];
    if (watched.includes(name)) return;
    entry.watched = [...watched, name];
    localStorage.setItem(STORE_KEY, JSON.stringify(store));
  } catch {
    // not critical
  }
}

/// Episode names this torrent has been watched to the end of.
export function watchedFiles(infoHash: string): Set<string> {
  return new Set(rememberedTorrent(infoHash)?.watched ?? []);
}

/**
 * The name of one file of a torrent, from the resolved list when it is loaded
 * and from the playing URL when it is not.
 *
 * The fallback is what makes this work for a torrent opened straight out of the
 * watch history: `torrent.info` is only filled by a resolve, while the stream
 * URL always carries the file's name as its last component — percent-encoded,
 * hence `displayName`'s decoding rather than a raw `baseName`.
 */
export function torrentFileName(infoHash: string, index: number): string {
  const info = torrent.info;
  if (info?.info_hash?.toLowerCase() === infoHash.toLowerCase()) {
    const file = info.files.find((f) => f.index === index);
    if (file) return baseName(file.path);
  }
  const ref = player.filePath ? parseTorrentUrl(player.filePath) : null;
  if (ref && ref.infoHash.toLowerCase() === infoHash.toLowerCase() && ref.index === index) {
    return decodeURIComponent(baseName(player.filePath!.split('?')[0]));
  }
  return '';
}

export function forgetRememberedTorrent(infoHash: string) {
  try {
    const store = readStore();
    delete store[infoHash.toLowerCase()];
    localStorage.setItem(STORE_KEY, JSON.stringify(store));
  } catch {
    // not critical
  }
}

/**
 * Open a file of a torrent given only its source id, re-resolving the magnet.
 *
 * This is what makes a torrent entry in the watch history work at all: the URL
 * recorded next to it points at a port that no longer exists, so the entry has
 * to be re-derived rather than reused. Returns the fresh URL, or null when the
 * magnet was never recorded (history off) or the swarm is gone.
 */
export async function resolveTorrentFile(
  id: string,
): Promise<{ url: string; info: TorrentInfo } | null> {
  const match = /^torrent:([0-9a-f]{40})\/(\d+)$/i.exec(id);
  if (!match) return null;
  const [, infoHash, indexText] = match;
  const known = rememberedTorrent(infoHash);
  if (!known) return null;
  const info = await addTorrent(known.magnet).catch(() => null);
  const url = info?.files.find((f) => f.index === Number(indexText))?.url;
  // The whole torrent comes back with it, not just the one file: a season
  // opened from the history is still a season, and the caller needs the list to
  // put the other episodes in the queue.
  return url && info ? { url, info } : null;
}
