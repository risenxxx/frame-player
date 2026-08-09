/**
 * The two lookups a start-screen torrent row is built from.
 *
 * `torrentResume` is what makes a row say "продолжить: S01E03 — осталось 23:14"
 * and what makes clicking it resume instead of dropping a nine-name file picker
 * on someone hunting for the episode they were already watching. It reads the
 * position store directly rather than `history.recent`, which holds only the
 * twelve newest entries across everything — a torrent watched last week has
 * fallen out of that long before its data has fallen off the disk, and *that*
 * is the property worth pinning here, because nothing about it is visible until
 * a viewer with a busy history finds their season has stopped resuming.
 */

import { beforeEach, describe, expect, it } from 'vitest';

import { torrentPositions, torrentResume, type TorrentOnDisk } from './torrent.svelte';

const HASH = '08ada5a7a6183aae1e09d831df6748d566095a10';
const OTHER = 'ffffffffffffffffffffffffffffffffffffffff';

type Rec = { pos: number; dur: number; ts?: number; title?: string; src?: string };

function positions(map: Record<string, Rec>) {
  localStorage.setItem('frameplayer.positions', JSON.stringify(map));
}

// Typed, not cast: a cast would keep compiling after the shape changed, which
// is the one thing these tests are here to notice.
const row = (hash: string | null): TorrentOnDisk => ({
  folder: hash ?? 'x',
  size: 0,
  info_hash: hash,
  // What the disk says it is called, which nothing here depends on: a resume
  // point is looked up by hash and index alone.
  name: null,
});

beforeEach(() => localStorage.clear());

describe('torrentResume', () => {
  it('returns the newest position in this torrent', () => {
    positions({
      [`torrent:${HASH}/1`]: { pos: 10, dur: 100, ts: 1000, title: 'Ep 1' },
      [`torrent:${HASH}/3`]: { pos: 20, dur: 200, ts: 3000, title: 'Ep 3' },
      [`torrent:${HASH}/2`]: { pos: 30, dur: 300, ts: 2000, title: 'Ep 2' },
    });
    expect(torrentResume(row(HASH))).toMatchObject({ index: 3, pos: 20, dur: 200, name: 'Ep 3' });
  });

  it('ignores other torrents and ordinary files', () => {
    positions({
      [`torrent:${OTHER}/9`]: { pos: 50, dur: 100, ts: 9000 },
      '/films/a.mkv': { pos: 50, dur: 100, ts: 9000 },
      [`torrent:${HASH}/1`]: { pos: 10, dur: 100, ts: 1 },
    });
    expect(torrentResume(row(HASH))?.index).toBe(1);
  });

  it('reads past the twelve newest entries', () => {
    // The reason it does not go through `history.recent`: a season watched last
    // week is long out of that list while its data is still on disk.
    const map: Record<string, Rec> = {};
    for (let i = 0; i < 50; i++) map[`/films/${i}.mkv`] = { pos: 5, dur: 100, ts: 10_000 + i };
    map[`torrent:${HASH}/7`] = { pos: 60, dur: 1200, ts: 1 };
    positions(map);
    expect(torrentResume(row(HASH))?.index).toBe(7);
  });

  it('names the episode from its own file when no title was recorded', () => {
    positions({
      [`torrent:${HASH}/4`]: {
        pos: 1,
        dur: 2,
        ts: 1,
        src: `http://127.0.0.1:51413/t/${HASH}/4/The.Show.S01E04.mkv`,
      },
    });
    // `displayName` off the URL: percent-decoded, extension gone, dots to spaces.
    expect(torrentResume(row(HASH))?.name).toBe('The Show S01E04');
  });

  it('matches the hash case-insensitively', () => {
    // A folder from the older layout may be upper-case hex, and `torrent_list`
    // reports it as it found it. This is the bug the test found: the prefix was
    // built from the row unchanged while the ids were lower-cased, so such a
    // torrent's row lost its resume line and dropped the file picker instead.
    positions({ [`torrent:${HASH}/2`]: { pos: 1, dur: 2, ts: 1 } });
    expect(torrentResume(row(HASH.toUpperCase()))?.index).toBe(2);
    expect(torrentResume(row(HASH))?.index).toBe(2);
  });

  it('has nothing to say about a folder with no hash', () => {
    positions({ [`torrent:${HASH}/1`]: { pos: 1, dur: 2, ts: 1 } });
    expect(torrentResume(row(null))).toBeNull();
  });

  it('returns null rather than throwing on an empty or corrupt store', () => {
    expect(torrentResume(row(HASH))).toBeNull();
    localStorage.setItem('frameplayer.positions', 'not json');
    expect(torrentResume(row(HASH))).toBeNull();
  });
});

describe('torrentPositions', () => {
  it('indexes what the picker marks as started', () => {
    positions({
      [`torrent:${HASH}/0`]: { pos: 10, dur: 100, ts: 1 },
      [`torrent:${HASH}/5`]: { pos: 20, dur: 200, ts: 2 },
      [`torrent:${OTHER}/0`]: { pos: 99, dur: 100, ts: 3 },
    });
    expect(torrentPositions(HASH)).toEqual({
      0: { pos: 10, dur: 100 },
      5: { pos: 20, dur: 200 },
    });
  });

  it('takes the hash in either case', () => {
    positions({ [`torrent:${HASH}/1`]: { pos: 1, dur: 2, ts: 1 } });
    expect(torrentPositions(HASH.toUpperCase())).toEqual({ 1: { pos: 1, dur: 2 } });
  });

  it('is empty rather than absent when nothing was watched', () => {
    expect(torrentPositions(HASH)).toEqual({});
  });
});
