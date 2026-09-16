import { describe, expect, it } from 'vitest';

import {
  findFeedUpdate,
  isFeedLink,
  itemForHash,
  releaseStem,
  sortFeedItems,
  type FeedItem,
} from './feed';

const item = (title: string, hash: string | null, published: number | null = null): FeedItem => ({
  title,
  info_hash: hash,
  magnet: null,
  torrent_url: hash ? null : 'https://t.example/1.torrent',
  size: null,
  published,
});

const AVC = 'a'.repeat(40);
const HEVC = 'b'.repeat(40);
const AVC12 = 'c'.repeat(40);

describe('isFeedLink', () => {
  it('recognises the shapes feed URLs take', () => {
    expect(isFeedLink('https://www.example.org/api/v1/anime/torrents/rss/release/10241')).toBe(true);
    expect(isFeedLink('https://nyaa.example/?page=rss&q=show')).toBe(true);
    expect(isFeedLink('https://site.example/feed.xml')).toBe(true);
    expect(isFeedLink('https://site.example/feeds/videos.xml?channel_id=1')).toBe(true);
    expect(isFeedLink('https://tracker.example/atom/topic/1')).toBe(true);
  });

  it('leaves ordinary links, magnets and words that merely contain the letters alone', () => {
    expect(isFeedLink('https://www.youtube.com/watch?v=abc')).toBe(false);
    expect(isFeedLink('magnet:?xt=urn:btih:' + AVC)).toBe(false);
    expect(isFeedLink('https://site.example/rsstudio/video.mp4')).toBe(false);
    expect(isFeedLink('https://site.example/feedback')).toBe(false);
    expect(isFeedLink('/Users/me/rss/film.mkv')).toBe(false);
  });
});

describe('releaseStem', () => {
  it('takes the episode range out and nothing else', () => {
    const a = releaseStem('Необъятный океан 3 | WEB-DL 1080p | AVC | 1-11');
    expect(a).toBe(releaseStem('Необъятный океан 3 | WEB-DL 1080p | AVC | 1-12'));
    expect(a).not.toBe(releaseStem('Необъятный океан 3 | WEB-DL 1080p | HEVC | 1-11'));
    // The season number is part of the release.
    expect(a).not.toBe(releaseStem('Необъятный океан 2 | WEB-DL 1080p | AVC | 1-11'));
  });

  it('folds the range spellings trackers use', () => {
    const base = releaseStem('Show [Серии: 1-9 из 12] 1080p');
    expect(releaseStem('Show [Серии: 1-10 из 12] 1080p')).toBe(base);
    expect(releaseStem('Show  [Серии: 01–10 из 12]  1080p')).toBe(base);
    expect(releaseStem('Show (05 of 12) 1080p')).toBe(releaseStem('Show (06 of 12) 1080p'));
  });

  it('keeps a lone episode number, so one torrent per episode is never a re-upload', () => {
    expect(releaseStem('[Group] Show - 11 (1080p)')).not.toBe(releaseStem('[Group] Show - 12 (1080p)'));
  });
});

describe('findFeedUpdate', () => {
  const feed = [
    item('Show 3 | WEB-DL 1080p | AVC | 1-11', AVC, 100),
    item('Show 3 | WEB-DL 1080p | HEVC | 1-11', HEVC, 200),
  ];

  it('finds nothing while the feed still holds what is on disk', () => {
    expect(findFeedUpdate(feed, AVC, 'Show 3 | WEB-DL 1080p | AVC | 1-11')).toBeNull();
  });

  it('does not offer the other codec as an update, however alike the names are', () => {
    // HEVC is newer and one token apart — exactly what a similarity score accepts.
    expect(findFeedUpdate(feed, AVC, 'Show 3 | WEB-DL 1080p | AVC | 1-11')).toBeNull();
  });

  it('offers the re-upload of the same release', () => {
    const next = item('Show 3 | WEB-DL 1080p | AVC | 1-12', AVC12, 300);
    expect(findFeedUpdate([...feed, next], AVC, 'Show 3 | WEB-DL 1080p | AVC | 1-11')).toBe(next);
  });

  it('matches hashes regardless of case, and prefers the newest candidate', () => {
    const older = item('Show 3 | WEB-DL 1080p | AVC | 1-12', AVC12, 300);
    const newer = item('Show 3 | WEB-DL 1080p | AVC | 1-13', 'd'.repeat(40), 400);
    expect(findFeedUpdate([newer, older], AVC.toUpperCase(), 'Show 3 | WEB-DL 1080p | AVC | 1-11')).toBe(newer);
    expect(findFeedUpdate([...feed], AVC.toUpperCase(), 'Show 3 | WEB-DL 1080p | AVC | 1-11')).toBeNull();
  });

  it('without dates, takes the one a feed appended last', () => {
    const a = item('Show | 1-2', 'e'.repeat(40));
    const b = item('Show | 1-3', 'f'.repeat(40));
    expect(findFeedUpdate([a, b], AVC, 'Show | 1-1')).toBe(b);
  });

  it('judges an item with no hash by its title', () => {
    const same = item('Show | 1-11', null);
    const next = item('Show | 1-12', null);
    expect(findFeedUpdate([same], AVC, 'Show | 1-11')).toBeNull();
    expect(findFeedUpdate([same, next], AVC, 'Show | 1-11')).toBe(next);
  });

  it('ignores items with nothing to open', () => {
    const dead = { ...item('Show | 1-12', null), torrent_url: null };
    expect(findFeedUpdate([dead], AVC, 'Show | 1-11')).toBeNull();
  });
});

describe('helpers', () => {
  it('finds an item by hash and sorts newest first, stable without dates', () => {
    expect(itemForHash(feedOf(), HEVC.toUpperCase())?.title).toBe('b');
    expect(sortFeedItems(feedOf()).map((i) => i.title)).toEqual(['b', 'a', 'x', 'y']);
  });
});

function feedOf(): FeedItem[] {
  return [item('x', null), item('a', AVC, 100), item('b', HEVC, 200), item('y', null)];
}
