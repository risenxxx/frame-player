import { invoke } from '@tauri-apps/api/core';

import { baseName, displayName } from '../format';
import { history, positionsLoad } from '../history.svelte';
import { torrentId } from '../source';
import { torrentVideos } from './files';
import {
  type RememberedTorrent,
  expectFeed,
  expectedFeeds,
  forgetRememberedTorrent,
  readStore,
  rememberTorrent,
} from './remembered';
import { addTorrent } from './session';
import { type TorrentInfo, torrent } from './state.svelte';

// ---- Replacing a torrent with its re-upload ---------------------------------

/**
 * Does this name look like the same release as that one?
 *
 * The case it exists for: a tracker publishes "…Серии: 1-9 из 9…" and a week
 * later "…Серии: 1-10 из 10…" — the same forty-word title with two numbers
 * changed. Token overlap handles that without knowing anything about the
 * tracker's naming, where a prefix comparison would break on the first release
 * that puts the episode count at the front.
 *
 * Measured against the alternative that suggests itself: comparing the whole
 * strings, or their first N characters, calls a re-upload "different" as soon as
 * the changed digits move. Overlap of the *shorter* side against the longer is
 * what makes "1-9 из 9" and "1-10 из 10" score ~0.9 while two unrelated films
 * score near zero.
 */
export function looksLikeSameRelease(a: string, b: string): boolean {
  const tokens = (s: string) =>
    new Set(
      s
        .toLowerCase()
        .split(/[^\p{L}\p{N}]+/u)
        .filter((w) => w.length > 1),
    );
  const x = tokens(a);
  const y = tokens(b);
  if (x.size < 3 || y.size < 3) return false;
  const [small, large] = x.size <= y.size ? [x, y] : [y, x];
  let shared = 0;
  for (const w of small) if (large.has(w)) shared++;
  return shared / small.size >= 0.6;
}

/** A remembered torrent the given magnet looks like a newer release of. */
export function findSupersededTorrent(magnet: string): RememberedTorrent | null {
  const name = displayName(magnet);
  const hash = /xt=urn:btih:([0-9a-f]{40})/i.exec(magnet)?.[1]?.toLowerCase();
  for (const known of Object.values(readStore())) {
    // The same torrent is not an update of itself.
    if (!known.name || known.infoHash === hash) continue;
    if (looksLikeSameRelease(name, known.name) || looksLikeSameRelease(name, known.magnet)) {
      return known;
    }
  }
  return null;
}

/**
 * Hand an old torrent's data and history over to the re-upload that replaces it.
 *
 * Two things happen that a plain "open this magnet" cannot do, and they are the
 * entire reason this exists rather than telling people to paste the link in the
 * usual box. The **data** is kept: the folder is renamed to the new info hash
 * before the torrent is added, so librqbit verifies the episodes already there
 * and fetches only what is new. And the **watch positions** are kept: they are
 * keyed by `torrent:<hash>/<index>`, so without this a season's history would
 * reset to zero for the sake of one added episode.
 *
 * Positions are matched by **file name**, taken from the URL each record already
 * stores — the index cannot be trusted, since one inserted episode renumbers
 * everything after it.
 */
export async function updateTorrent(
  old: RememberedTorrent,
  magnet: string,
): Promise<{ info: TorrentInfo; matched: number; added: number }> {
  const newHash = /xt=urn:btih:([0-9a-f]{40})/i.exec(magnet)?.[1]?.toLowerCase();
  if (!newHash) throw new Error('no_hash');
  if (newHash === old.infoHash) throw new Error('same_torrent');

  // Before the add, or the new torrent creates an empty folder of its own and
  // there is nothing left to hand over.
  //
  // A failure here is not fatal — the update still works, it just re-downloads
  // what was already on disk — but it is not nothing either: the old folder
  // stays while the line below forgets its magnet, which is exactly how a row
  // ends up nameless and undeletable-by-anything-but-its-size. So it is said out
  // loud rather than swallowed, since "target exists" and "nothing to move" are
  // the two answers that explain the leftover.
  await invoke('torrent_relocate', { oldHash: old.infoHash, newHash }).catch((e) =>
    console.warn('[torrent] handing the old data over failed:', e),
  );

  // The feed goes with the season: the new torrent is the next upload of the
  // same release, so its feed is the one to watch. Whoever resolved a feed
  // item has already said which item that was; otherwise the old record is
  // the best there is, and its stem is the same anyway.
  if (old.feed && !expectedFeeds.has(newHash)) expectFeed(newHash, old.feed);

  const info = await addTorrent(magnet);
  const moved = migratePositions(old.infoHash, info);
  delete torrent.feedUpdates[old.infoHash];
  forgetRememberedTorrent(old.infoHash);
  rememberTorrent(info, magnet);
  return { info, matched: moved, added: torrentVideos(info).length - moved };
}

/// Re-key every watch position from the old torrent onto the new one, by file
/// name. Returns how many were carried over — which is also what tells the
/// viewer the update found the right torrent.
function migratePositions(oldHash: string, info: TorrentInfo): number {
  if (!history.prefs.enabled) return 0;
  const prefix = `torrent:${oldHash.toLowerCase()}/`;
  const byName = new Map(info.files.map((f) => [baseName(f.path).toLowerCase(), f]));
  let moved = 0;
  try {
    const positions = positionsLoad();
    for (const [id, rec] of Object.entries(positions)) {
      if (!id.toLowerCase().startsWith(prefix)) continue;
      delete positions[id];
      // The name lives in the URL the record already carries — no extra store,
      // and it works for entries written before any of this existed.
      const name = rec.src ? decodeURIComponent(rec.src.split('/').pop() ?? '') : '';
      const file = byName.get(name.toLowerCase());
      if (!file) continue;
      positions[torrentId(info.info_hash, file.index)] = { ...rec, src: file.url };
      moved++;
    }
    localStorage.setItem('frameplayer.positions', JSON.stringify(positions));
  } catch {
    // A failed migration costs the history, not the data.
  }
  return moved;
}
