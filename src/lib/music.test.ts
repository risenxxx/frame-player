import { describe, expect, it } from 'vitest';

import { albumOrder, lyricLine } from './music';

const name = (p: string) => p.split('/').pop() ?? p;

describe('albumOrder', () => {
  it('follows the track numbers when they settle the whole order', () => {
    const paths = ['/a/c-intro.flac', '/a/a-middle.flac', '/a/b-outro.flac'];
    expect(albumOrder(paths, [[1, 1], [1, 2], [1, 3]], name)).toEqual(paths);
  });

  it('puts disc two after disc one', () => {
    const paths = ['/a/x.mp3', '/a/y.mp3', '/a/z.mp3'];
    expect(albumOrder(paths, [[2, 1], [1, 2], [1, 1]], name)).toEqual(['/a/z.mp3', '/a/y.mp3', '/a/x.mp3']);
  });

  it('falls back to the names when one file has no number', () => {
    const paths = ['/a/02 b.mp3', '/a/10 c.mp3', '/a/01 a.mp3'];
    expect(albumOrder(paths, [[1, 3], null, [1, 1]], name)).toEqual(['/a/01 a.mp3', '/a/02 b.mp3', '/a/10 c.mp3']);
  });

  it('falls back to the names when two files share a number', () => {
    // A folder of singles, each tagged track 1 of its own release.
    const paths = ['/a/b.mp3', '/a/a.mp3'];
    expect(albumOrder(paths, [[1, 1], [1, 1]], name)).toEqual(['/a/a.mp3', '/a/b.mp3']);
  });

  it('falls back to the names when the numbers did not arrive', () => {
    const paths = ['/a/b.mp3', '/a/a.mp3'];
    expect(albumOrder(paths, [], name)).toEqual(['/a/a.mp3', '/a/b.mp3']);
  });
});

describe('lyricLine', () => {
  it('collapses a line reported twice', () => {
    expect(lyricLine('Second line here\nSecond line here')).toBe('Second line here');
  });
  it('keeps two different lines, and drops blanks', () => {
    expect(lyricLine(' One \n\nTwo ')).toBe('One\nTwo');
  });
  it('is empty for nothing', () => {
    expect(lyricLine('')).toBe('');
  });
});
