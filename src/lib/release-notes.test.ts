/**
 * The update button's tooltip reads a manifest written by the release workflow
 * — or by nobody, for a release from before notes existed. Both have to come
 * out as something to show.
 */

import { describe, expect, it } from 'vitest';

import { MAX_ITEMS, notesTip, parseReleaseNotes } from './release-notes';

const BODY = `Long lists now fade at their edges.

### New

- **Lists fade at their edges.** The queue and chapters fade out.

### Improvements and fixes

- **Menus keep their width.** On macOS a menu no longer widens.
- **A crash on macOS**
`;

describe('parseReleaseNotes', () => {
  it('reads the summary and every headline, without the full stop', () => {
    expect(parseReleaseNotes(BODY)).toEqual({
      summary: 'Long lists now fade at their edges.',
      items: ['Lists fade at their edges', 'Menus keep their width', 'A crash on macOS'],
    });
  });

  it('has nothing to say about a manifest without notes', () => {
    expect(parseReleaseNotes(undefined)).toBeNull();
    expect(parseReleaseNotes('')).toBeNull();
    expect(parseReleaseNotes('  \n ')).toBeNull();
  });

  it('keeps a summary that has no list under it', () => {
    expect(parseReleaseNotes('Maintenance release.\r\n')).toEqual({ summary: 'Maintenance release.', items: [] });
  });

  it('does not take a heading for the summary', () => {
    expect(parseReleaseNotes('### New\n\n- **Thing.** Detail')).toEqual({ summary: '', items: ['Thing'] });
  });

  it('shows text in another format as it is rather than nothing', () => {
    expect(parseReleaseNotes('## Changes\n* fixed things')).toEqual({ summary: '## Changes * fixed things', items: [] });
  });
});

describe('notesTip', () => {
  const more = (n: number) => `and ${n} more`;

  it('is the heading, the summary and a line per headline', () => {
    const notes = parseReleaseNotes(BODY)!;
    expect(notesTip("What's new in 1.19.0", notes, more)).toBe(
      "What's new in 1.19.0\n\nLong lists now fade at their edges.\n\n• Lists fade at their edges\n• Menus keep their width\n• A crash on macOS",
    );
  });

  it('says how many did not fit', () => {
    const items = Array.from({ length: MAX_ITEMS + 2 }, (_, i) => `Item ${i}`);
    const tip = notesTip('H', { summary: '', items }, more);
    expect(tip.split('\n').filter((l) => l.startsWith('• '))).toHaveLength(MAX_ITEMS);
    expect(tip.endsWith('and 2 more')).toBe(true);
  });
});
