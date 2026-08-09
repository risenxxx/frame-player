/**
 * The name-reading end of the player. Everything here has one property in
 * common: it is fed release names, which are written for a sorting algorithm
 * rather than for a person, and every one of them is somebody's edge case.
 *
 * `parseEpisode` is the one with teeth. Without the season and episode numbers
 * an OpenSubtitles search for a series reaches the wrong thing entirely —
 * measured, "the day of the jackal" returns the 1973 film — and the restraint in
 * its patterns is load-bearing: a release name is full of numbers sitting
 * exactly where a naive parser would read an episode.
 */

import { describe, expect, it } from 'vitest';

// Vite's `?raw`, not `node:fs`: the file becomes a module dependency, so the
// test re-runs when a vector is added, and it needs no `@types/node` for what
// is one string.
import VECTORS_RAW from '../../shared/path-under.txt?raw';

import {
  baseName,
  displayName,
  extensionOf,
  fileStem,
  formatTime,
  parseEpisode,
  pathUnder,
  readableLink,
  samePath,
  shotStamp,
} from './format';

describe('formatTime', () => {
  it('drops the hour until there is one', () => {
    expect(formatTime(0)).toBe('0:00');
    expect(formatTime(65)).toBe('1:05');
    expect(formatTime(3599)).toBe('59:59');
    expect(formatTime(3600)).toBe('1:00:00');
    expect(formatTime(3661)).toBe('1:01:01');
  });

  it('does not print a negative or a NaN at the viewer', () => {
    // `duration - timePos` goes briefly negative at the end of a file, and
    // `time-pos` is NaN before mpv reports one.
    expect(formatTime(-5)).toBe('0:00');
    expect(formatTime(Number.NaN)).toBe('0:00');
  });
});

describe('displayName', () => {
  it('turns a release name into something readable', () => {
    expect(displayName('/f/The.Movie.2024.1080p.BluRay.mkv')).toBe(
      'The Movie 2024 1080p BluRay',
    );
  });

  it('leaves a name that already reads properly alone', () => {
    // The dot rule is "between non-space characters", so this must not become
    // "The Movie (2024) 4K" with the spaces doubled or the parenthesis moved.
    expect(displayName('/f/The Movie (2024).mkv')).toBe('The Movie (2024)');
  });

  it('percent-decodes, because our own torrent URLs are encoded', () => {
    // Without this an ordinary film reads as `The%20Movie%202024`.
    expect(displayName('http://127.0.0.1:1/t/abc/0/The%20Movie%202024.mkv')).toBe(
      'The Movie 2024',
    );
  });

  it('drops a query and a fragment', () => {
    // A shared YouTube link otherwise shows as `TfUP5fQcbCM?si=…`.
    expect(displayName('https://youtu.be/TfUP5fQcbCM?si=ykE0V6YNxD9bQ3bz')).toBe('TfUP5fQcbCM');
  });

  it('reads a magnet through its display name', () => {
    expect(displayName('magnet:?xt=urn:btih:abc&dn=The+Show+S01')).toBe('The Show S01');
    // A real plus arrives as %2B and must survive the `+`→space replacement.
    expect(displayName('magnet:?xt=urn:btih:abc&dn=C%2B%2B')).toBe('C++');
    // Nothing to show: the magnet itself rather than an empty string.
    expect(displayName('magnet:?xt=urn:btih:abc')).toBe('magnet:?xt=urn:btih:abc');
  });

  it('replaces a dot only between two characters', () => {
    // The rule is narrower than "dots become spaces", and the two cases that
    // show it: a leading dot is not a separator, and a dot already surrounded by
    // spaces is punctuation somebody typed.
    expect(displayName('/f/.hidden.mkv')).toBe('.hidden');
    expect(displayName('/f/Movie . 2024.mkv')).toBe('Movie . 2024');
  });

  it('survives a lone percent, which is legal on disk', () => {
    // `decodeURIComponent` throws on this, and a file name is not worth an
    // exception on the start screen.
    expect(displayName('/f/100%.mkv')).toBe('100%');
  });
});

describe('readableLink', () => {
  it('keeps a magnet to its name and hash, dropping the tracker list', () => {
    const magnet =
      'magnet:?xt=urn:btih:ABCDEF0123456789abcdef0123456789ABCDEF01&dn=The+Show&tr=udp%3A%2F%2Ftracker.example%3A80&tr=udp%3A%2F%2Fother.example%3A80';
    const out = readableLink(magnet);
    expect(out).toContain('The Show');
    expect(out).toContain('btih:abcdef0123456789abcdef0123456789abcdef01');
    expect(out).not.toContain('tracker.example');
  });

  it('decodes a URL so a Cyrillic path is legible', () => {
    expect(readableLink('https://x.test/%D1%84%D0%B8%D0%BB%D1%8C%D0%BC')).toBe(
      'https://x.test/фильм',
    );
  });
});

describe('parseEpisode', () => {
  it('reads the forms releases actually use', () => {
    expect(parseEpisode('The.Show.S01E03.1080p.mkv')).toMatchObject({ season: 1, episode: 3 });
    expect(parseEpisode('The Show s1.e3.mkv')).toMatchObject({ season: 1, episode: 3 });
    expect(parseEpisode('The Show Season 1 Episode 3.mkv')).toMatchObject({ season: 1, episode: 3 });
    expect(parseEpisode('Сериал сезон 1 серия 3.mkv')).toMatchObject({ season: 1, episode: 3 });
    expect(parseEpisode('The Show 1x03.mkv')).toMatchObject({ season: 1, episode: 3 });
  });

  it('hands back the show title with the separators cleaned up', () => {
    expect(parseEpisode('The.Day.of.the.Jackal.S01E03.mkv')?.title).toBe('The Day of the Jackal');
    // Opening with the marker leaves nothing, and the caller keeps what it had.
    expect(parseEpisode('S01E03.mkv')?.title).toBe('');
  });

  it('**does not** read a resolution as a season and an episode', () => {
    // The false positive the `1x03` pattern is bounded for: 1920x1080 would
    // otherwise be "season 20, episode 10".
    expect(parseEpisode('The.Movie.1920x1080.mkv')).toBeNull();
  });

  it('does not read the other numbers a release name is full of', () => {
    expect(parseEpisode('The.Movie.2024.1080p.x264.mkv')).toBeNull();
    expect(parseEpisode('The.Movie.DTS.5.1.mkv')).toBeNull();
    expect(parseEpisode('The.Movie.720p.mkv')).toBeNull();
  });

  it('does not match inside a word', () => {
    // The leading boundary: "…s01e…" inside a word is not a marker.
    expect(parseEpisode('Glimpses01e03.mkv')).toBeNull();
  });

  it('accepts season 0 (specials) but not episode 0', () => {
    expect(parseEpisode('Show.S00E01.mkv')).toMatchObject({ season: 0, episode: 1 });
    expect(parseEpisode('Show.S01E00.mkv')).toBeNull();
  });

  it('ignores a query, so a stream URL parses like a path', () => {
    expect(parseEpisode('http://x.test/Show.S02E07.mkv?token=1')).toMatchObject({
      season: 2,
      episode: 7,
    });
  });
});

describe('paths', () => {
  it('takes the base name on either separator', () => {
    // Both appear: a stored record may hold one and a directory read the other.
    expect(baseName('/a/b/c.mkv')).toBe('c.mkv');
    expect(baseName('E:\\a\\b\\c.mkv')).toBe('c.mkv');
  });

  it('reads the extension in lower case, or nothing', () => {
    expect(extensionOf('/a/B.MKV')).toBe('mkv');
    expect(extensionOf('/a/README')).toBe('');
  });

  it('makes a file stem no filesystem will refuse', () => {
    expect(fileStem('a/b:c*d?.mkv')).not.toMatch(/[\\/:*?"<>|]/);
    expect(fileStem('.mkv')).toBe('frame');
    expect(fileStem('x'.repeat(200)).length).toBeLessThanOrEqual(80);
  });
});

describe('shotStamp', () => {
  it('sorts in playback order, being the position and not the clock', () => {
    expect(shotStamp(0)).toBe('00-00-00.000');
    expect(shotStamp(3661.5)).toBe('01-01-01.500');
    expect(shotStamp(-1)).toBe('00-00-00.000');
    // Lexicographic order is playback order — the whole point of the format.
    expect(shotStamp(59) < shotStamp(61)).toBe(true);
  });
});

/**
 * The two path comparisons, and the vectors the Rust twin also reads.
 *
 * `pathUnder` decides whether a privacy root applies, which makes it the
 * highest-cost predicate in the codebase to get wrong: matching too much hides
 * files the viewer never excluded, matching too little is a leak that shows
 * nothing on screen. Its Rust copy (`path_under` in thumb_service.rs) decides
 * the same thing for thumbnails on disk and for the hash sent to OpenSubtitles,
 * and the two have already disagreed once — the JS side compared separators
 * literally, so a root spelled `E:\Films` silently failed to match
 * `E:/Films/a.mkv`.
 *
 * So the cases live in `shared/path-under.txt` and both suites read that file.
 * Restating them here would be a third copy of the thing whose copies are the
 * problem.
 */
describe('path comparison', () => {
  interface Vector {
    path: string;
    root: string;
    want: boolean;
    n: number;
  }

  const VECTORS: Vector[] = VECTORS_RAW.split('\n').flatMap((raw, i): Vector[] => {
    const n = i + 1;
    const line = raw.trim();
    if (line === '' || line.startsWith('#')) return [];
    const parts = line.split('\t');
    if (parts.length !== 3) throw new Error(`line ${n}: expected <path> TAB <root> TAB yes|no`);
    const [path, root, want] = parts;
    if (want !== 'yes' && want !== 'no') throw new Error(`line ${n}: expected yes|no, got ${want}`);
    return [{ path, root, want: want === 'yes', n }];
  });

  // A vectors file that parsed to nothing — moved, renamed, or reformatted so
  // every line looks like a comment — would otherwise pass in silence, which is
  // the one way a shared contract can quietly stop being one. Mirrored in the
  // Rust test, and a floor rather than an exact count so adding a case does not
  // mean editing two test files to let it in.
  it('reads the shared vectors', () => {
    expect(VECTORS.length).toBeGreaterThanOrEqual(15);
  });

  for (const v of VECTORS) {
    it(`line ${v.n}: pathUnder(${v.path || '""'}, ${v.root || '""'}) === ${v.want}`, () => {
      expect(pathUnder(v.path, v.root)).toBe(v.want);
    });
  }

  describe('samePath', () => {
    it('ignores case and separator direction, on both sides', () => {
      expect(samePath('E:/Videos/ep1.mkv', 'e:\\videos\\EP1.MKV')).toBe(true);
    });

    it('is not fooled by a shared prefix', () => {
      expect(samePath('/a/ep1.mkv', '/a/ep10.mkv')).toBe(false);
    });

    it('does not treat a folder as the file inside it', () => {
      expect(samePath('/a', '/a/b.mkv')).toBe(false);
    });
  });
});
