import { describe, expect, it } from 'vitest';

import { codeFromLink, magnetFromLink } from './link.svelte';

// This is the parser that decides what an arbitrary string from outside the
// application may do to the player: a custom scheme is a surface any page on
// the internet can aim at. So the test is mostly about what it *refuses* —
// exactly one shape is understood and everything else has to fall through to
// nothing, in silence, rather than being guessed at.

describe('codeFromLink', () => {
  it('takes the one shape it is for', () => {
    expect(codeFromLink('frameplayer://join/ABC123')).toBe('ABC123');
  });

  it('accepts what a link actually looks like in the wild', () => {
    // Copied out of a chat window with space around it, lower-cased by an
    // over-helpful client, or carrying the dash the relay's own page prints.
    expect(codeFromLink('  frameplayer://join/abc123  ')).toBe('ABC123');
    expect(codeFromLink('FRAMEPLAYER://JOIN/abc123')).toBe('ABC123');
    expect(codeFromLink('frameplayer://join/ABC-123')).toBe('ABC123');
    expect(codeFromLink('frameplayer://join/abc123/')).toBe('ABC123');
    expect(codeFromLink('frameplayer://join/abc123?from=chat')).toBe('ABC123');
    expect(codeFromLink('frameplayer://join/abc123#x')).toBe('ABC123');
    // A client that percent-encoded the dash.
    expect(codeFromLink('frameplayer://join/ABC%2D123')).toBe('ABC123');
    // Some senders double the slash.
    expect(codeFromLink('frameplayer://join//ABC123')).toBe('ABC123');
  });

  it('folds the glyphs the code alphabet exists to avoid', () => {
    expect(codeFromLink('frameplayer://join/ABCO23')).toBe('ABC023');
    expect(codeFromLink('frameplayer://join/ABCl23')).toBe('ABC123');
  });

  it('refuses another scheme', () => {
    expect(codeFromLink('https://relay.invalid/j/ABC123')).toBe('');
    expect(codeFromLink('file:///etc/passwd')).toBe('');
    expect(codeFromLink('javascript:alert(1)')).toBe('');
    // Not a prefix match on the scheme, either.
    expect(codeFromLink('notframeplayer://join/ABC123')).toBe('');
  });

  it('refuses an action it does not have', () => {
    // The only thing a link may do is offer a room. Anything else is a request
    // to grow a second meaning for this surface, and it is refused rather than
    // parsed — today by having no other branch, and tomorrow by this test.
    expect(codeFromLink('frameplayer://open/%2Fetc%2Fpasswd')).toBe('');
    expect(codeFromLink('frameplayer://play/https://x.invalid/v.mp4')).toBe('');
    expect(codeFromLink('frameplayer://')).toBe('');
    expect(codeFromLink('frameplayer://join')).toBe('');
    expect(codeFromLink('frameplayer://join/')).toBe('');
  });

  it('refuses anything that cannot be a code', () => {
    expect(codeFromLink('frameplayer://join/ABC12')).toBe('');
    expect(codeFromLink('frameplayer://join/ABC1234')).toBe('');
    expect(codeFromLink('frameplayer://join/../../etc')).toBe('');
    expect(codeFromLink('frameplayer://join/ABC!23')).toBe('');
    expect(codeFromLink('')).toBe('');
  });

  it('survives a percent-escape that does not decode', () => {
    // `decodeURIComponent` throws on a lone `%`, and an exception here would
    // take down whatever is handling the link rather than ignoring it.
    expect(() => codeFromLink('frameplayer://join/%E0%A4%A')).not.toThrow();
    expect(codeFromLink('frameplayer://join/%E0%A4%A')).toBe('');
  });
});

describe('magnetFromLink', () => {
  const hash = '0123456789abcdef0123456789abcdef01234567';

  it('passes a magnet through whole, trackers and all', () => {
    const link = `magnet:?xt=urn:btih:${hash}&dn=Test+Film&tr=udp%3A%2F%2Ftracker.invalid`;
    expect(magnetFromLink(link)).toBe(link);
    expect(magnetFromLink(`  ${link}\n`)).toBe(link);
    expect(magnetFromLink(`MAGNET:?xt=urn:btih:${hash}`)).toBe(`MAGNET:?xt=urn:btih:${hash}`);
  });

  // What is inside is the torrent engine's to judge, as for a pasted magnet:
  // a broken hash still reaches the box and fails there, with the same words.
  it('leaves the contents to the engine', () => {
    expect(magnetFromLink('magnet:?dn=Test')).toBe('magnet:?dn=Test');
  });

  it('ignores every other shape', () => {
    expect(magnetFromLink('https://example.com/')).toBe('');
    expect(magnetFromLink('frameplayer://join/ABC123')).toBe('');
    expect(magnetFromLink('magnet:')).toBe('');
    expect(magnetFromLink('file:///tmp/film.mkv')).toBe('');
  });
});
