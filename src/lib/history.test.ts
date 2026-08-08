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

import { beforeEach, describe, expect, it } from 'vitest';

import {
  history,
  isPrivatePath,
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
