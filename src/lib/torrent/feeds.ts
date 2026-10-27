import { invoke } from '@tauri-apps/api/core';

import { magnetFor } from '../source';
import type { Feed, FeedItem } from '../feed';
import { torrentPrefs } from './prefs.svelte';

// ---- RSS feeds -------------------------------------------------------------

/// Read a feed through Rust — the torrent proxy applies, since a feed URL names
/// what is being watched as plainly as a magnet. Throws `not_feed` for a page
/// that is not RSS or Atom.
export async function readFeed(url: string): Promise<Feed> {
  return await invoke<Feed>('feed_read', { url, proxy: torrentPrefs.proxy });
}

/**
 * Turn a feed item into a magnet the ordinary torrent path can open.
 *
 * A `.torrent` URL is preferred even when the item also names a hash: fetching
 * it puts the metadata in the cache `torrent_add` reads first, so the magnet
 * built from its hash opens without a DHT lookup — where a bare hash would be
 * the ninety-second resolve. A magnet the feed supplies is used as it is, with
 * its trackers; a hash alone is the last resort.
 */
export async function resolveFeedItem(
  item: FeedItem,
): Promise<{ magnet: string; infoHash: string }> {
  if (item.torrent_url) {
    try {
      const got = await invoke<{ info_hash: string; name: string | null }>('feed_torrent', {
        url: item.torrent_url,
        proxy: torrentPrefs.proxy,
      });
      return { magnet: magnetFor(got.info_hash, got.name ?? item.title), infoHash: got.info_hash };
    } catch (e) {
      // With a hash or a magnet still in hand, a dead download link is not the
      // end of it — the swarm can supply the metadata.
      if (!item.magnet && !item.info_hash) throw e;
      console.warn('[feed] .torrent download failed, falling back:', e);
    }
  }
  // Only a hex magnet is used as it is: a base32 one is the same torrent, but
  // the update path reads the hash out of the magnet and knows hex alone.
  const hex = item.magnet && /xt=urn:btih:([0-9a-f]{40})/i.exec(item.magnet)?.[1]?.toLowerCase();
  if (item.magnet && hex) return { magnet: item.magnet, infoHash: hex };
  if (item.info_hash) {
    return { magnet: magnetFor(item.info_hash, item.title), infoHash: item.info_hash };
  }
  throw new Error('feed_item_empty');
}
