import { describe, expect, it } from 'vitest';

import { DURATION_TOLERANCE, compareLocal, contentIdOf, sameContent } from './content';
import type { ContentRef } from './protocol';

// What a room is watching is the one thing here whose wrong answer is both
// silent and expensive: `sameContent` deciding two references are different
// restarts the film for everybody, and deciding they are the same leaves a
// viewer on the previous episode with a timeline that looks perfectly healthy.

const torrent = (index: number, over: Partial<Extract<ContentRef, { kind: 'torrent' }>> = {}) =>
  ({
    kind: 'torrent',
    magnet: 'magnet:?xt=urn:btih:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    infoHash: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    index,
    file: `S01E0${index}.mkv`,
    title: `Episode ${index}`,
    duration: 1400,
    ...over,
  }) satisfies ContentRef;

const url = (u: string, over: Partial<Extract<ContentRef, { kind: 'url' }>> = {}) =>
  ({ kind: 'url', url: u, title: 'A film', duration: 5400, ...over }) satisfies ContentRef;

const file = (over: Partial<Extract<ContentRef, { kind: 'file' }>> = {}) =>
  ({ kind: 'file', title: 'A film', duration: 5400, size: 123, hash: 'abc123', ...over }) satisfies ContentRef;

describe('sameContent', () => {
  it('is identity, not equality — a late title must not read as a different film', () => {
    expect(sameContent(torrent(3), torrent(3, { title: 'Эпизод 3' }))).toBe(true);
    // A duration that firmed up after the file opened, and a magnet rebuilt from
    // the info hash rather than remembered. Both are routine.
    expect(sameContent(torrent(3), torrent(3, { duration: 1401.2 }))).toBe(true);
    expect(sameContent(torrent(3), torrent(3, { magnet: 'magnet:?xt=urn:btih:AAAA' }))).toBe(true);
  });

  it('separates episodes of one torrent', () => {
    expect(sameContent(torrent(3), torrent(4))).toBe(false);
  });

  it('does not care about the case of an info hash', () => {
    // One end reads it out of a URL, the other out of a remembered magnet, and
    // trackers publish both cases.
    expect(sameContent(torrent(1), torrent(1, { infoHash: 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA' }))).toBe(true);
  });

  it('canonicalises URLs, so a shared link and an address-bar one are one film', () => {
    expect(
      sameContent(
        url('https://www.youtube.com/watch?v=abc123&si=fromshare'),
        url('https://www.youtube.com/watch?v=abc123'),
      ),
    ).toBe(true);
    expect(sameContent(url('https://x.invalid/a'), url('https://x.invalid/b'))).toBe(false);
  });

  it('compares local files by release hash and nothing else', () => {
    expect(sameContent(file(), file({ title: 'другое имя' }))).toBe(true);
    expect(sameContent(file(), file({ hash: 'deadbeef' }))).toBe(false);
    // Without a hash on both sides there is nothing to compare but a name, and
    // two people's copies of a film rarely share one — so the honest answer is
    // "not the same", and `compareLocal` is where that becomes a sentence.
    expect(sameContent(file({ hash: '' }), file({ hash: '' }))).toBe(false);
  });

  it('treats two hidden files as one, or a private film would restart on every re-statement', () => {
    expect(sameContent({ kind: 'hidden' }, { kind: 'hidden' })).toBe(true);
  });

  it('never confuses kinds, or null with content', () => {
    expect(sameContent(torrent(1), url('x'))).toBe(false);
    expect(sameContent(null, null)).toBe(true);
    expect(sameContent(null, file())).toBe(false);
  });
});

describe('contentIdOf', () => {
  it('speaks the same vocabulary as the watch history', () => {
    // `sourceId('http://127.0.0.1:PORT/t/<hash>/<i>/…')` produces exactly this,
    // which is what makes "is the room on the file I have open" one comparison
    // rather than a second notion of identity.
    expect(contentIdOf(torrent(3))).toBe(
      'torrent:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/3',
    );
    expect(contentIdOf(url('https://x.invalid/a?si=track'))).toBe('https://x.invalid/a');
  });

  it('is empty for what cannot be addressed from another machine', () => {
    expect(contentIdOf(file())).toBe('');
    expect(contentIdOf({ kind: 'hidden' })).toBe('');
  });
});

describe('compareLocal', () => {
  it('is exact when the open file is the very one the room named', () => {
    expect(
      compareLocal(torrent(3), {
        src: 'http://127.0.0.1:51234/t/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/3/S01E03.mkv',
        duration: 1400,
      }),
    ).toBe('exact');
  });

  it('spots the wrong episode of the right torrent', () => {
    expect(
      compareLocal(torrent(3), {
        src: 'http://127.0.0.1:51234/t/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/4/S01E04.mkv',
        duration: 1400,
      }),
    ).toBe('mismatch');
  });

  it('matches a local copy by release hash', () => {
    expect(compareLocal(file({ hash: 'abc123' }), { src: '/f.mkv', duration: 5400, hash: 'abc123' })).toBe('exact');
  });

  it('calls a different hash with the same length a different rip', () => {
    // The common and useful case: two people have the same film from different
    // releases. The timeline is approximately right, and saying so is worth more
    // than refusing.
    expect(compareLocal(file({ hash: 'abc123' }), { src: '/f.mkv', duration: 5400, hash: 'zzz' })).toBe('rip');
  });

  it('calls a different length a mismatch, because there the timeline is meaningless', () => {
    expect(compareLocal(file({ duration: 5400 }), { src: '/f.mkv', duration: 7000, hash: 'zzz' })).toBe('mismatch');
  });

  it('tolerates the small disagreements containers actually produce', () => {
    const near = 5400 + DURATION_TOLERANCE - 0.1;
    const far = 5400 + DURATION_TOLERANCE + 0.1;
    expect(compareLocal(file({ hash: '' }), { src: '/f.mkv', duration: near })).toBe('rip');
    expect(compareLocal(file({ hash: '' }), { src: '/f.mkv', duration: far })).toBe('mismatch');
  });

  it('says nothing about a hidden film, or about nothing at all', () => {
    expect(compareLocal({ kind: 'hidden' }, { src: '/f.mkv', duration: 10 })).toBe('unknown');
    expect(compareLocal(null, { src: '/f.mkv', duration: 10 })).toBe('unknown');
    expect(compareLocal(file(), null)).toBe('unknown');
    // Nothing to compare: a duration nobody knows yet.
    expect(compareLocal(file({ hash: '', duration: 0 }), { src: '/f.mkv', duration: 0 })).toBe('unknown');
  });
});
