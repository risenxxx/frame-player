import { describe, expect, it } from 'vitest';

// Vite's `?raw`, the same way `format.test.ts` reads `shared/path-under.txt`:
// it keeps the fixture a build-time dependency rather than a path resolved at
// run time, so a file that moved fails the build instead of the assertion.
import FIXTURE_RAW from '../../../shared/sync-protocol.txt?raw';

import {
  CODE_LENGTH,
  PROTOCOL_FIELDS,
  emptyTimeline,
  formatCode,
  isErrorCode,
  normalizeCode,
  positionAt,
  type ContentRef,
  type Timeline,
} from './protocol';

// ---- the shared contract ----------------------------------------------------

/**
 * The half of the field-name contract this side owns.
 *
 * The compiler already proves `PROTOCOL_FIELDS` names every key of every
 * interface (see the `Complete<>` assertions in protocol.ts); this proves the
 * same lists match the ones Go produces. Both halves are needed — one without
 * the other is a contract with a single signature.
 */
describe('the wire matches shared/sync-protocol.txt', () => {
  const fixture = new Map<string, string[]>();
  for (const line of FIXTURE_RAW.split('\n')) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith('#')) continue;
    const [name, fields] = trimmed.split('\t');
    fixture.set(name.trim(), fields.trim().split(','));
  }

  // A file that moved, or was reformatted so every line reads as a comment,
  // would otherwise pass in silence — the one way a shared contract quietly
  // stops being one.
  it('parsed the fixture at all', () => {
    expect(fixture.size).toBeGreaterThanOrEqual(12);
  });

  it('names the same messages', () => {
    expect([...fixture.keys()].sort()).toEqual(Object.keys(PROTOCOL_FIELDS).sort());
  });

  for (const [name, want] of fixture) {
    it(`${name} has the agreed fields`, () => {
      expect([...(PROTOCOL_FIELDS[name] ?? [])].sort()).toEqual([...want].sort());
    });
  }
});

// ---- room codes -------------------------------------------------------------
//
// The same cases as `server/internal/wire/code_test.go`, because this is one
// behaviour written twice: the player normalises what somebody types so the
// relay is asked about a code that can exist, and the relay normalises again so
// a disagreement costs a refusal rather than the wrong room.

describe('normalizeCode', () => {
  it.each([
    ['ABC123', 'ABC123', 'already canonical'],
    ['abc123', 'ABC123', 'typed in lower case'],
    ['  ABC123  ', 'ABC123', 'pasted with space around it'],
    ['ABC-123', 'ABC123', 'read aloud, written with a dash'],
    ['A B C 1 2 3', 'ABC123', 'spelled out one character at a time'],
    // The whole reason for Crockford's alphabet: what a person types when they
    // are looking at the other glyph.
    ['ABCO23', 'ABC023', 'O read as zero'],
    ['ABCI23', 'ABC123', 'I read as one'],
    ['ABCl23', 'ABC123', 'lower-case L read as one'],
    ['', '', 'nothing'],
    ['ABC12', '', 'too short'],
    ['ABC1234', '', 'too long'],
    ['ABC!23', '', 'punctuation that is not a separator'],
    ['ABCU23', '', 'U — not in the alphabet and not a look-alike, so a typo'],
  ])('%s → %s (%s)', (input, want) => {
    expect(normalizeCode(input)).toBe(want);
  });

  it('is idempotent, or a code it produced would be refused on arrival', () => {
    for (const input of ['abc-123', 'ABCO23', 'ABCl23', ' zzz999 ']) {
      const once = normalizeCode(input);
      expect(once).not.toBe('');
      expect(normalizeCode(once)).toBe(once);
    }
  });

  it('formats for reading aloud, and leaves anything else alone', () => {
    expect(formatCode('ABC123')).toBe('ABC-123');
    expect(formatCode('short')).toBe('short');
    expect(normalizeCode(formatCode('ABC123'))).toBe('ABC123');
  });

  it('agrees with the length the UI promises', () => {
    expect(CODE_LENGTH).toBe(6);
  });
});

// ---- the projection ---------------------------------------------------------

describe('positionAt', () => {
  const content: ContentRef = { kind: 'url', url: 'x', title: 'x', duration: 100 };
  const playing: Timeline = { content, paused: false, position: 100, speed: 1, at: 10_000, rev: 1, by: 'a' };

  it('runs forward at the timeline speed', () => {
    expect(positionAt(playing, 12_000)).toBe(102);
    expect(positionAt({ ...playing, speed: 2 }, 12_000)).toBe(104);
    expect(positionAt({ ...playing, speed: 0.5 }, 12_000)).toBe(101);
  });

  it('does not advance while paused, however long ago the stamp was', () => {
    expect(positionAt({ ...playing, paused: true }, 999_000)).toBe(100);
  });

  it('does not advance when nothing is playing', () => {
    // `at` is stamped regardless, so without the content check the position of a
    // film that does not exist would run away.
    expect(positionAt({ ...emptyTimeline(), paused: false, at: 10_000 }, 999_000)).toBe(0);
  });

  it('runs backwards for a clock reading before the stamp — an offset is an estimate', () => {
    expect(positionAt(playing, 8_000)).toBe(98);
  });

  it('never comes out negative, because the caller seeks to it', () => {
    expect(positionAt(playing, -999_000)).toBe(0);
  });
});

describe('isErrorCode', () => {
  it('recognises what the relay can send', () => {
    expect(isErrorCode('no_room')).toBe(true);
    expect(isErrorCode('not_allowed')).toBe(true);
  });

  it('rejects anything else, so an unknown code cannot become a blank sentence', () => {
    expect(isErrorCode('teapot')).toBe(false);
    expect(isErrorCode('')).toBe(false);
  });
});
