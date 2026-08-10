/**
 * The watch history, and the privacy rules around it.
 *
 * These are the tests with the highest cost of being wrong and the lowest chance
 * of being noticed. A privacy leak here is invisible by construction: the start
 * screen looks clean while the disk is not, and nobody finds out until they
 * remove a folder from the exclusion list and everything they thought was never
 * recorded comes back. There are five enforcement points in the app and this
 * module owns the predicate all of them ask.
 *
 * The path matching is mirrored in Rust (`path_under`), so the boundary cases
 * here are also the contract that copy has to keep.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';

// The purge paths talk to Rust (thumbnails on disk) and to the folder picker.
// Neither is what is under test here — what is, is that every store keyed by a
// video is emptied — so both are stubbed at the module boundary rather than
// worked around inside the test.
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => undefined) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(async () => '/Private') }));

import { invoke } from '@tauri-apps/api/core';

const invoked = invoke as unknown as ReturnType<typeof vi.fn>;

import {
  addExcludedFolder,
  clearHistory,
  history,
  isPrivatePath,
  purgeTorrentHistory,
  persistPosition,
  positionsLoad,
  rememberTitle,
  rememberTrack,
  titleFor,
  trackWishFor,
} from './history.svelte';

const POSITIONS = 'frameplayer.positions';

beforeEach(() => {
  localStorage.clear();
  history.prefs = { enabled: true, excluded: [] };
});

describe('isPrivatePath', () => {
  it('matches on a path component boundary, not on a prefix', () => {
    // The bug this exists to prevent: excluding /Movies must not silently
    // exclude /Movies2, which is a different folder with a similar name.
    history.prefs = { enabled: true, excluded: ['/Movies'] };
    expect(isPrivatePath('/Movies/a.mkv')).toBe(true);
    expect(isPrivatePath('/Movies')).toBe(true);
    expect(isPrivatePath('/Movies2/a.mkv')).toBe(false);
    expect(isPrivatePath('/MoviesArchive/a.mkv')).toBe(false);
  });

  it('is case-insensitive and accepts either separator', () => {
    // The two sides come from different places: a stored record, a directory
    // read in Rust, or mpv's `path` — and on Windows they disagree about both.
    history.prefs = { enabled: true, excluded: ['E:\\Films'] };
    expect(isPrivatePath('e:\\films\\a.mkv')).toBe(true);
    expect(isPrivatePath('E:/Films/a.mkv')).toBe(true);
    expect(isPrivatePath('E:\\Films2\\a.mkv')).toBe(false);
  });

  it('ignores a trailing separator on the root', () => {
    history.prefs = { enabled: true, excluded: ['/Movies/'] };
    expect(isPrivatePath('/Movies/a.mkv')).toBe(true);
  });

  it('treats an empty root as matching nothing', () => {
    // "History off" is a separate flag and **not** a synthetic `/` root: roots
    // are trimmed of trailing separators, so `/` collapses to `''`, and if that
    // matched everything the two controls would be impossible to tell apart.
    history.prefs = { enabled: true, excluded: ['/'] };
    expect(isPrivatePath('/Movies/a.mkv')).toBe(false);
  });

  it('makes everything private when history is off', () => {
    history.prefs = { enabled: false, excluded: [] };
    expect(isPrivatePath('/anywhere/a.mkv')).toBe(true);
    expect(isPrivatePath('https://x.test/v')).toBe(true);
  });
});

describe('persistPosition', () => {
  it('writes nothing at all for a private path', () => {
    history.prefs = { enabled: true, excluded: ['/Private'] };
    persistPosition('/Private/a.mkv', 600, 3600);
    expect(positionsLoad()).toEqual({});
  });

  it('waits until the file has actually been started', () => {
    persistPosition('/f/a.mkv', 5, 3600);
    expect(positionsLoad()['/f/a.mkv']).toBeUndefined();
    persistPosition('/f/a.mkv', 20, 3600);
    expect(positionsLoad()['/f/a.mkv']).toMatchObject({ pos: 20, dur: 3600 });
  });

  it('forgets a file that has been finished', () => {
    persistPosition('/f/a.mkv', 600, 1000);
    expect(positionsLoad()['/f/a.mkv']).toBeDefined();
    persistPosition('/f/a.mkv', 980, 1000); // 98%
    expect(positionsLoad()['/f/a.mkv']).toBeUndefined();
  });

  it('can record a clip shorter than the flat threshold', () => {
    // The relative half of the floor is not optional: with a flat 15 s a 17 s
    // clip had a two-second window between "not started" and "already
    // finished", and anything under ~15.5 s could never be recorded at all.
    persistPosition('/f/clip.mkv', 1, 17); // 5% of 17 s is 0.85 s
    expect(positionsLoad()['/f/clip.mkv']).toMatchObject({ pos: 1 });
  });

  it('keeps the title a real one, and never lets a null erase it', () => {
    // mpv reports `media-title` a beat after the file opens; the null in
    // between must not wipe what an earlier viewing recorded.
    persistPosition('/f/a.mkv', 20, 3600, 'The Real Title');
    expect(positionsLoad()['/f/a.mkv'].title).toBe('The Real Title');
    persistPosition('/f/a.mkv', 25, 3600, undefined);
    expect(positionsLoad()['/f/a.mkv'].title).toBe('The Real Title');
  });

  it('records the openable source beside the identity', () => {
    // The id answers "which video is this", `src` answers "how do I open it",
    // and for anything but a local file they are different strings.
    const url = 'https://youtu.be/abc?si=tracking';
    persistPosition(url, 30, 600);
    const entry = Object.entries(positionsLoad())[0];
    expect(entry[0]).not.toContain('si=');
    expect(entry[1].src).toBe(url);
  });
});

describe('remembered tracks', () => {
  it('gives a local file a folder scope, so the next episode inherits', () => {
    rememberTrack('/Show/S01E01.mkv', 'audio', {
      lang: 'ru',
      title: null,
      codec: null,
      forced: false,
      index: 1,
    });
    // The neighbour has no entry of its own and picks the folder's up.
    expect(trackWishFor('/Show/S01E02.mkv', 'audio')).toMatchObject({ lang: 'ru' });
    // A different folder does not.
    expect(trackWishFor('/Other/S01E02.mkv', 'audio')).toBeNull();
  });

  it('gives a network source no folder scope', () => {
    // A URL has no folder: taking one would put every video on a site into a
    // single `youtube.com/watch` bucket deciding the dub for all of them.
    //
    // Guarded on **both** sides — the write does not create a folder entry and
    // the read does not consult one — so each has to be asserted separately.
    // This test started by checking only the read, which meant the write guard
    // could have been deleted without anything noticing.
    rememberTrack('https://youtube.com/watch?v=one', 'audio', {
      lang: 'ru',
      title: null,
      codec: null,
      forced: false,
      index: 1,
    });
    expect(localStorage.getItem('frameplayer.tracks.folder')).toBeNull();
    expect(trackWishFor('https://youtube.com/watch?v=two', 'audio')).toBeNull();
    expect(trackWishFor('https://youtube.com/watch?v=one', 'audio')).toMatchObject({ lang: 'ru' });
  });

  it('records nothing for a private path, and answers nothing about one', () => {
    history.prefs = { enabled: true, excluded: ['/Private'] };
    rememberTrack('/Private/a.mkv', 'audio', {
      lang: 'ru',
      title: null,
      codec: null,
      forced: false,
      index: 0,
    });
    expect(localStorage.getItem('frameplayer.tracks')).toBeNull();
    expect(trackWishFor('/Private/a.mkv', 'audio')).toBeNull();
  });

  it('stores "no subtitles" as a choice rather than as an absence', () => {
    rememberTrack('/f/a.mkv', 'sub', 'no');
    expect(trackWishFor('/f/a.mkv', 'sub')).toBe('no');
  });
});

describe('titles', () => {
  it('keys a title by identity, so two spellings of a link agree', () => {
    rememberTitle('https://youtube.com/watch?v=abc', 'The Video');
    expect(titleFor('https://youtu.be/abc')).toBeNull(); // a different URL entirely
    expect(titleFor('https://youtube.com/watch?v=abc&si=x')).toBe('The Video');
  });

  it('refuses to record one for a private path', () => {
    history.prefs = { enabled: true, excluded: ['/Private'] };
    rememberTitle('/Private/a.mkv', 'Something');
    expect(titleFor('/Private/a.mkv')).toBeNull();
  });

  it('ignores an empty title', () => {
    rememberTitle('/f/a.mkv', '   ');
    expect(titleFor('/f/a.mkv')).toBeNull();
  });
});

describe('a corrupt store', () => {
  it('reads as empty rather than throwing', () => {
    localStorage.setItem(POSITIONS, 'not json');
    expect(positionsLoad()).toEqual({});
    // ...and a write over it still works.
    persistPosition('/f/a.mkv', 20, 3600);
    expect(positionsLoad()['/f/a.mkv']).toBeDefined();
  });
});

/**
 * Forgetting, and the three ways to ask for it.
 *
 * These are the tests with the same shape as the privacy ones and the same cost
 * of being wrong: what is left behind is invisible. The start screen shows
 * nothing about a purged folder whether or not its titles are still in
 * localStorage, so the only way to find out is to look — which is what this
 * does, by writing into every store and then reading all of them back.
 *
 * The store list is deliberately restated here rather than imported. A test
 * that walked the same registry the code walks would pass for a store the
 * registry has forgotten, which is the one failure worth catching.
 */
describe('purging', () => {
  const STORES = {
    positions: 'frameplayer.positions',
    tracks: 'frameplayer.tracks',
    folderTracks: 'frameplayer.tracks.folder',
    titles: 'frameplayer.titles',
    subs: 'frameplayer.subs',
    resume: 'frameplayer.resume',
    links: 'frameplayer.links',
    torrents: 'frameplayer.torrents',
  };

  /// Write one entry about a file into every store that holds one.
  function seedFile(path: string, folder: string) {
    localStorage.setItem(STORES.positions, JSON.stringify({ [path]: { pos: 60, dur: 600 } }));
    localStorage.setItem(STORES.tracks, JSON.stringify({ [path]: { ts: 1 } }));
    localStorage.setItem(STORES.folderTracks, JSON.stringify({ [folder]: { ts: 1 } }));
    localStorage.setItem(STORES.titles, JSON.stringify({ [path]: 'A Film' }));
    localStorage.setItem(STORES.subs, JSON.stringify([`${path}.ru.srt`]));
    localStorage.setItem(STORES.resume, JSON.stringify({ path, pos: 60 }));
    // Not about this file, and that is the point: a magnet names a season as
    // plainly as a position names a film, and it holds which episodes were
    // finished. "Forget everything" left every one of them behind until this
    // store joined the list.
    localStorage.setItem(
      STORES.torrents,
      JSON.stringify({ abc: { infoHash: 'abc', magnet: 'magnet:?xt=urn:btih:abc', watched: ['e1'] } }),
    );
  }

  /// `links` and `torrents` are keyed by something that is not a path, so a
  /// folder exclusion legitimately leaves them alone; `clearHistory` checks
  /// them by name instead.
  const leftovers = () =>
    Object.entries(STORES)
      .filter(([name]) => name !== 'links' && name !== 'torrents')
      .filter(([, key]) => {
        const raw = localStorage.getItem(key);
        if (raw === null) return false;
        const value = JSON.parse(raw);
        return Array.isArray(value) ? value.length > 0 : Object.keys(value ?? {}).length > 0;
      })
      .map(([name]) => name);

  it('leaves nothing behind when a folder is excluded', async () => {
    seedFile('/Private/a.mkv', '/Private');
    await addExcludedFolder();
    expect(leftovers()).toEqual([]);
  });

  it('adds the folder to the list before deleting, so a racing write is refused', async () => {
    // The position timer fires every ~5 s and the purge ends in a round trip to
    // Rust, so a write genuinely does land in the middle of this — which is the
    // only moment the ordering can be observed at all. The thumbnail call is the
    // seam: it happens after the stores are emptied, so a write arriving there
    // survives unless the folder is *already* excluded and `persistPosition`
    // refuses it. Ordered the other way this entry comes back for good.
    seedFile('/Private/a.mkv', '/Private');
    invoked.mockImplementationOnce(async () => {
      persistPosition('/Private/a.mkv', 600, 3600);
    });
    await addExcludedFolder();
    expect(history.prefs.excluded).toContain('/Private');
    expect(leftovers()).toEqual([]);
  });

  it('leaves a neighbouring folder with a similar name alone', async () => {
    localStorage.setItem(
      STORES.titles,
      JSON.stringify({ '/Private/a.mkv': 'gone', '/Private2/b.mkv': 'kept' }),
    );
    await addExcludedFolder();
    expect(JSON.parse(localStorage.getItem(STORES.titles)!)).toEqual({ '/Private2/b.mkv': 'kept' });
  });

  it('leaves nothing behind when the history is cleared', async () => {
    // The bug this pins: `clearHistory` named three keys by hand and cleared
    // three of six. The titles store survived — and keyed by source id, which
    // for a local file is the path, that store on its own is a list of
    // everything ever watched.
    seedFile('/Films/a.mkv', '/Films');
    localStorage.setItem(STORES.links, JSON.stringify(['https://x.test/v']));
    await clearHistory();
    expect(leftovers()).toEqual([]);
    expect(localStorage.getItem(STORES.links)).toBe(null);
    expect(localStorage.getItem(STORES.torrents)).toBe(null);
  });

  describe('purgeTorrentHistory', () => {
    const HASH = 'ABCDEF0123456789abcdef0123456789abcdef01';
    const id = (i: number) => `torrent:${HASH.toLowerCase()}/${i}`;

    beforeEach(() => {
      localStorage.setItem(
        STORES.positions,
        JSON.stringify({ [id(0)]: { pos: 1 }, [id(1)]: { pos: 2 }, '/Films/a.mkv': { pos: 3 } }),
      );
      localStorage.setItem(STORES.titles, JSON.stringify({ [id(0)]: 'S01E01', '/Films/a.mkv': 'A' }));
    });

    it('takes every episode of the torrent and nothing else', () => {
      purgeTorrentHistory(HASH);
      expect(positionsLoad()).toEqual({ '/Films/a.mkv': { pos: 3 } });
      expect(JSON.parse(localStorage.getItem(STORES.titles)!)).toEqual({ '/Films/a.mkv': 'A' });
    });

    it('matches the hash whatever case either side arrives in', () => {
      // Both sides, and they fail differently. The *argument* is upper-case
      // whenever it comes from a folder name in the older layout, which is the
      // mismatch that once cost a torrent its "continue watching" line. The
      // *stored id* is lower-case everywhere the current code writes one
      // (`torrentId` and `parseTorrentUrl` both normalize), so that half is
      // defence against a store this build did not write — an entry left by an
      // older version, or edited by hand. Cheap, and the alternative is an
      // orphan nothing can ever delete.
      localStorage.setItem(
        STORES.positions,
        JSON.stringify({ [`torrent:${HASH.toUpperCase()}/0`]: { pos: 1 }, '/Films/a.mkv': { pos: 3 } }),
      );
      purgeTorrentHistory(HASH.toUpperCase());
      expect(Object.keys(positionsLoad())).toEqual(['/Films/a.mkv']);
    });

    it('does not touch the path-keyed stores, which cannot hold a torrent', () => {
      localStorage.setItem(STORES.resume, JSON.stringify({ path: '/Films/a.mkv', pos: 60 }));
      purgeTorrentHistory(HASH);
      expect(localStorage.getItem(STORES.resume)).not.toBe(null);
    });
  });
});
