/**
 * `sourceId` is the highest-consequence pure function in the player: it is the
 * key every watch position, every remembered dub and every poster is filed
 * under. A change in its shape does not fail — it silently forgets everything
 * recorded before, which looks to the viewer like the history clearing itself.
 *
 * So the tests here are mostly about *stability*, not cleverness: the local-path
 * identity that makes old records readable, and the two canonicalisations that
 * exist because the same video legitimately arrives twice under different URLs.
 */

import { describe, expect, it } from 'vitest';

import { isMagnet, isTorrentLink, magnetFor, parseTorrentUrl, sourceId, torrentId } from './source';

const HASH = '08ada5a7a6183aae1e09d831df6748d566095a10';

describe('sourceId', () => {
  it('leaves a local path exactly as it is', () => {
    // The whole reason nothing had to be migrated when ids were introduced.
    expect(sourceId('/Users/x/Films/a.mkv')).toBe('/Users/x/Films/a.mkv');
    expect(sourceId('E:\\Films\\a.mkv')).toBe('E:\\Films\\a.mkv');
    // Including the awkward ones: a path is not parsed, so nothing can mangle it.
    expect(sourceId('/films/The Movie (2024) [1080p].mkv')).toBe(
      '/films/The Movie (2024) [1080p].mkv',
    );
  });

  it('gives a torrent stream an identity its port cannot change', () => {
    const a = sourceId(`http://127.0.0.1:51413/t/${HASH}/3/Ep03.mkv`);
    const b = sourceId(`http://127.0.0.1:9999/t/${HASH}/3/Ep03.mkv`);
    expect(a).toBe(b);
    expect(a).toBe(`torrent:${HASH}/3`);
  });

  it('files the two ways a YouTube link is shared under one key', () => {
    // The share button adds `si`; the address bar does not. Same video.
    const shared = sourceId('https://www.youtube.com/watch?v=abc123&si=trackingtoken');
    const typed = sourceId('https://www.youtube.com/watch?v=abc123');
    expect(shared).toBe(typed);
  });

  it('sorts parameters, so order cannot create a second entry', () => {
    expect(sourceId('https://x.test/w?b=2&a=1')).toBe(sourceId('https://x.test/w?a=1&b=2'));
  });

  it('drops the fragment but keeps the path case', () => {
    // A fragment addresses a place within a page, not another video. A path is
    // case-sensitive on the server, so lower-casing it would be a different URL.
    expect(sourceId('https://X.test/Path/To?v=1#t=30')).toBe('https://x.test/Path/To?v=1');
  });

  it('keeps the parameters that say which video, or where in it', () => {
    // `t`/`start`/`list` are about what the viewer meant and are left alone —
    // dropping them would be a guess rather than a fact about the video.
    for (const q of ['t=90', 'start=90', 'list=PL1']) {
      expect(sourceId(`https://x.test/w?v=1&${q}`)).toContain(q);
    }
  });

  it('leaves no trailing "?" when the last parameter was tracking', () => {
    expect(sourceId('https://x.test/w?si=abc')).toBe('https://x.test/w');
  });

  it('falls back to the trimmed input rather than throwing', () => {
    expect(sourceId('  http://[not a url  ')).toBe('http://[not a url');
  });
});

describe('parseTorrentUrl', () => {
  it('reads the hash and the index out of our own route', () => {
    expect(parseTorrentUrl(`http://127.0.0.1:51413/t/${HASH}/12/Ep12.mkv`)).toEqual({
      infoHash: HASH,
      index: 12,
    });
  });

  it('lower-cases the hash so the two spellings are one torrent', () => {
    expect(parseTorrentUrl(`http://127.0.0.1:1/t/${HASH.toUpperCase()}/0/a.mkv`)?.infoHash).toBe(
      HASH,
    );
  });

  it('accepts the route with no trailing name', () => {
    expect(parseTorrentUrl(`http://127.0.0.1:1/t/${HASH}/0`)?.index).toBe(0);
  });

  it('refuses anything that is not our loopback route', () => {
    // The host check is narrow on purpose: 127.0.0.1 is where every development
    // server in the world lives, and a stray `/t/…` misfiled as a torrent would
    // take another site's position records with it.
    expect(parseTorrentUrl(`http://example.com/t/${HASH}/0/a.mkv`)).toBeNull();
    expect(parseTorrentUrl(`http://localhost:1/t/${HASH}/0/a.mkv`)).toBeNull();
    expect(parseTorrentUrl('http://127.0.0.1:1/t/nothex/0/a.mkv')).toBeNull();
    // 39 hex characters, not 40.
    expect(parseTorrentUrl(`http://127.0.0.1:1/t/${HASH.slice(1)}/0/a.mkv`)).toBeNull();
  });
});

describe('torrent links', () => {
  it('recognizes a magnet whatever its case, and after a paste with spaces', () => {
    expect(isMagnet(`magnet:?xt=urn:btih:${HASH}`)).toBe(true);
    expect(isMagnet(`  MAGNET:?xt=urn:btih:${HASH}  `)).toBe(true);
    expect(isMagnet('https://x.test/a.torrent')).toBe(false);
  });

  it('recognizes a .torrent link with a query or a fragment after it', () => {
    expect(isTorrentLink('https://x.test/a.torrent')).toBe(true);
    expect(isTorrentLink('https://x.test/a.torrent?key=1')).toBe(true);
    expect(isTorrentLink('https://x.test/a.torrent#frag')).toBe(true);
    // Not a torrent: the extension has to end the path.
    expect(isTorrentLink('https://x.test/a.torrentfile')).toBe(false);
    expect(isTorrentLink('https://youtube.com/watch?v=1')).toBe(false);
  });

  it('round-trips a magnet through the id and back', () => {
    const magnet = magnetFor(HASH, 'The Show S01');
    expect(isMagnet(magnet)).toBe(true);
    expect(magnet).toContain(`urn:btih:${HASH}`);
    // The name is a display name and must survive characters a URL cares about.
    expect(magnetFor(HASH, 'A & B / C')).toContain('dn=A%20%26%20B%20%2F%20C');
    // No `dn` at all rather than an empty one.
    expect(magnetFor(HASH, null)).toBe(`magnet:?xt=urn:btih:${HASH}`);
  });

  it('agrees with sourceId about what a torrent id looks like', () => {
    // The two are written in different places and a drift between them would
    // file a torrent's episodes under keys nothing else can find.
    expect(torrentId(HASH.toUpperCase(), 3)).toBe(
      sourceId(`http://127.0.0.1:1/t/${HASH}/3/Ep03.mkv`),
    );
  });
});
