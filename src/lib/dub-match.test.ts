import { describe, expect, it } from 'vitest';

import { looksLikeDub } from './dub-match';

const FILM = '/films/Movie.2019.1080p.mkv';
const TWO_HOURS = 2 * 3600;

describe('looksLikeDub', () => {
  it('takes a file as long as the film for a dub', () => {
    expect(looksLikeDub('/dubs/rus.flac', TWO_HOURS - 40, FILM, TWO_HOURS)).toBe(true);
    expect(looksLikeDub('/dubs/rus.ac3', TWO_HOURS + 300, FILM, TWO_HOURS)).toBe(true);
  });

  it('takes a song for a song', () => {
    expect(looksLikeDub('/music/01 - Track.flac', 245, FILM, TWO_HOURS)).toBe(false);
    // A soundtrack album rip in one file is still not the film's length.
    expect(looksLikeDub('/music/OST.flac', 3600, FILM, TWO_HOURS)).toBe(false);
  });

  it('keeps a floor for short videos', () => {
    expect(looksLikeDub('/a.mka', 68, '/clip.mp4', 60)).toBe(true);
    expect(looksLikeDub('/a.mka', 75, '/clip.mp4', 60)).toBe(false);
  });

  it('trusts a name taken from the video whatever the length', () => {
    expect(looksLikeDub('/films/Movie.2019.1080p.Rus.mka', 100, FILM, TWO_HOURS)).toBe(true);
    expect(looksLikeDub('C:\\films\\movie.2019.1080p.eng.ac3', 100, 'C:\\films\\Movie.2019.1080p.mkv', TWO_HOURS)).toBe(true);
  });

  it('attaches when a length is unknown, as a drop always did', () => {
    expect(looksLikeDub('/a.flac', null, FILM, TWO_HOURS)).toBe(true);
    expect(looksLikeDub('/a.flac', 245, 'https://example.com/live', 0)).toBe(true);
  });
});
