/**
 * Torrent RSS feeds, the pure half: which item of a feed is which release.
 *
 * The Rust half (feed.rs) reads a feed into items — a title, maybe an info
 * hash, maybe a magnet, maybe a `.torrent` URL. What this decides is the one
 * question the update check has to get right: **is this item a newer upload of
 * the release the viewer is watching, or a different release beside it?**
 *
 * A per-release feed routinely holds both. The one this was built against lists
 * `… | WEB-DL 1080p | AVC | 1-11` and `… | WEB-DL 1080p | HEVC | 1-11` side by
 * side, and next week `… | AVC | 1-12`. The token overlap the link box uses for
 * a pasted magnet (`looksLikeSameRelease`) scores AVC against HEVC at 6/7, well
 * over its threshold — fine for a question the viewer answers, wrong for an
 * offer the player makes by itself. So a feed compares **stems**: the title with
 * its episode *range* taken out, and everything else required to be equal.
 *
 * Only a range is taken out, never a lone number. A feed with one torrent per
 * episode (`Show - 11`, `Show - 12`) is a different publishing model: those are
 * separate torrents, not re-uploads, and offering to replace episode 11's
 * torrent with episode 12's would move one file's data into a folder for
 * another. With lone numbers kept, their stems differ and nothing is offered.
 */

export interface FeedItem {
  title: string;
  info_hash: string | null;
  magnet: string | null;
  torrent_url: string | null;
  size: number | null;
  published: number | null;
}

export interface Feed {
  title: string | null;
  items: FeedItem[];
}

/// Where a remembered torrent came from, when that was a feed.
export interface FeedOrigin {
  url: string;
  /// The title of the item this torrent was opened from — what the next
  /// check compares stems against.
  item: string;
}

/**
 * Does this look like a feed rather than a page or a stream?
 *
 * Only a routing hint: the link is read as a feed first, and Rust answers
 * `not_feed` for anything that is not RSS or Atom, in which case the link goes
 * on to mpv exactly as before. So a false positive costs one request, and the
 * pattern is written for what feed URLs actually look like — `/rss/…`,
 * `feed.xml`, `?format=atom` — rather than to be airtight.
 */
export function isFeedLink(src: string): boolean {
  const s = src.trim();
  if (!/^https?:\/\//i.test(s)) return false;
  let url: URL;
  try {
    url = new URL(s);
  } catch {
    return false;
  }
  const rest = `${url.pathname}${url.search}`.toLowerCase();
  return /(^|[/._?=&-])(rss|atom|feeds?)([/._?=&-]|$)/.test(rest) || /\.xml$/.test(url.pathname);
}

/**
 * The release a title names, with the part a re-upload changes removed.
 *
 * `1-11`, `01–12`, `1-9 из 12`, `[05 of 12]`, `Серии: 1-10 из 10` all collapse
 * to one marker; case, punctuation and spacing are folded so a tracker that
 * tidies its separators is not publishing a new release.
 */
export function releaseStem(title: string): string {
  return title
    .toLowerCase()
    .replace(/\d+\s*[-–—~]\s*\d+(\s*(из|of|\/)\s*\d+)?/g, ' # ')
    .replace(/\d+\s*(из|of)\s*\d+/g, ' # ')
    .split(/[^\p{L}\p{N}#]+/u)
    .filter(Boolean)
    .join(' ');
}

/// Can this item be opened at all? A feed that only announces a page changed
/// carries none of the three.
export function isOpenable(item: FeedItem): boolean {
  return !!(item.info_hash || item.magnet || item.torrent_url);
}

/**
 * The item that replaces the torrent the viewer has, or null.
 *
 * `knownHash` is what is on disk; `knownItem` is the title of the item it came
 * from. A candidate has the same stem, is openable, and is **not the torrent
 * already held** — judged by hash, which an item without one cannot be, so such
 * an item only counts when it is also not the item already opened by title.
 * The newest candidate wins; where dates are missing, document order decides,
 * last one first, since that is how a feed appends.
 */
export function findFeedUpdate(
  items: readonly FeedItem[],
  knownHash: string,
  knownItem: string,
): FeedItem | null {
  const stem = releaseStem(knownItem);
  const hash = knownHash.toLowerCase();
  const candidates = items
    .map((item, order) => ({ item, order }))
    .filter(({ item }) => {
      if (!isOpenable(item) || releaseStem(item.title) !== stem) return false;
      if (item.info_hash) return item.info_hash.toLowerCase() !== hash;
      return item.title !== knownItem;
    });
  candidates.sort(
    (a, b) => (b.item.published ?? 0) - (a.item.published ?? 0) || b.order - a.order,
  );
  return candidates[0]?.item ?? null;
}

/// The item a torrent on disk was opened from, found by its hash — how a feed
/// pasted into the update dialog is attached to a torrent that arrived by magnet.
export function itemForHash(items: readonly FeedItem[], hash: string): FeedItem | null {
  const h = hash.toLowerCase();
  return items.find((i) => i.info_hash?.toLowerCase() === h) ?? null;
}

/// Newest first, which is what someone choosing from a feed is looking for; a
/// feed's own order varies by site (the one this was built against is oldest
/// first).
export function sortFeedItems(items: readonly FeedItem[]): FeedItem[] {
  return items
    .map((item, order) => ({ item, order }))
    .sort((a, b) => (b.item.published ?? 0) - (a.item.published ?? 0) || a.order - b.order)
    .map(({ item }) => item);
}
