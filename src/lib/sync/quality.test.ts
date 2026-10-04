import { describe, expect, it } from 'vitest';

import {
  DISRUPTION_WINDOW_MS,
  NET,
  UNSETTLED,
  UPGRADE_HOLD_MS,
  causeOf,
  combine,
  levelOf,
  netFigures,
  netLevel,
  parseLink,
  playLevel,
  pruneDisruptions,
  settle,
  type Settled,
} from './quality';

// Every mistake in this module is a plausible colour: a dot that is wrong looks
// exactly like a dot that works. So the thresholds are pinned where their
// consequences begin, and the hysteresis is pinned in both directions.

describe('netLevel', () => {
  it('says nothing until there are enough round trips to mean something', () => {
    expect(netLevel({ rtts: [], lost: 0 })).toBeNull();
    expect(netLevel({ rtts: [20, 20], lost: 0 })).toBeNull();
    expect(netLevel({ rtts: [20, 20, 20], lost: 0 })).toBe('good');
  });

  it('judges by the fastest round trip, not the average', () => {
    // One retransmission in the window is not a slow network.
    expect(netLevel({ rtts: [40, 42, 45, 41, 900], lost: 0 })).toBe('good');
  });

  it('turns yellow where the drift band reaches its ceiling, red where a pause is visibly late', () => {
    expect(netLevel({ rtts: [NET.fairRtt, NET.fairRtt, NET.fairRtt], lost: 0 })).toBe('good');
    expect(netLevel({ rtts: [NET.fairRtt + 1, NET.fairRtt + 5, NET.fairRtt + 9], lost: 0 })).toBe('fair');
    expect(netLevel({ rtts: [NET.poorRtt + 1, NET.poorRtt + 5, NET.poorRtt + 9], lost: 0 })).toBe('poor');
  });

  it('treats an uneven connection as a worse one, even when its best is fast', () => {
    expect(netLevel({ rtts: [30, 160, 170], lost: 0 })).toBe('fair');
    expect(netLevel({ rtts: [30, 400, 420], lost: 0 })).toBe('poor');
  });

  it('counts an unanswered ping as worse news than any round trip', () => {
    expect(netLevel({ rtts: [20, 20, 20], lost: 1 })).toBe('fair');
    expect(netLevel({ rtts: [20, 20, 20], lost: 2 })).toBe('poor');
  });
});

describe('netFigures', () => {
  it('reports the fastest round trip and the spread above it, rounded', () => {
    expect(netFigures([])).toBeNull();
    expect(netFigures([50.4, 80.2, 60.1])).toEqual({ rtt: 50, spread: 10 });
  });
});

describe('playLevel', () => {
  const now = 1_000_000;

  it('is green with nothing in the window, and forgets what has aged out', () => {
    expect(playLevel([], now)).toBe('good');
    expect(playLevel([now - DISRUPTION_WINDOW_MS - 1], now)).toBe('good');
  });

  it('is yellow for one disruption and red for a pattern of them', () => {
    expect(playLevel([now - 1000], now)).toBe('fair');
    expect(playLevel([now - 60_000, now - 1000], now)).toBe('poor');
  });

  it('prunes so the list cannot grow for the length of a film', () => {
    expect(pruneDisruptions([now - DISRUPTION_WINDOW_MS - 1, now - 5], now)).toEqual([now - 5]);
  });
});

describe('combine', () => {
  it('has no word while the network has none', () => {
    expect(combine(null, 'poor')).toBeNull();
  });

  it('takes the worse half, and names it', () => {
    expect(combine('good', 'good')).toBe('good');
    expect(combine('fair', 'good')).toBe('fair-net');
    expect(combine('good', 'poor')).toBe('poor-play');
    expect(combine('fair', 'poor')).toBe('poor-play');
  });

  it('blames the network on a tie, because it is the likelier cause of the other', () => {
    expect(combine('fair', 'fair')).toBe('fair-net');
    expect(combine('poor', 'poor')).toBe('poor-net');
  });
});

describe('the word on the wire', () => {
  it('round-trips every word it can produce', () => {
    for (const w of ['good', 'fair-net', 'fair-play', 'poor-net', 'poor-play']) {
      expect(parseLink(w)).toBe(w);
    }
  });

  it('shows nothing for a word from a newer build, or from none', () => {
    // A room whose members run different builds is the ordinary case: an
    // unknown word must be no dot, never a wrong one.
    expect(parseLink('excellent')).toBeNull();
    expect(parseLink('')).toBeNull();
    expect(parseLink(undefined)).toBeNull();
  });

  it('splits into a level and a cause', () => {
    expect(levelOf('good')).toBe('good');
    expect(causeOf('good')).toBeNull();
    expect(levelOf('fair-play')).toBe('fair');
    expect(causeOf('fair-play')).toBe('play');
    expect(levelOf('poor-net')).toBe('poor');
    expect(causeOf('poor-net')).toBe('net');
  });
});

describe('settle', () => {
  const at = (word: Settled['word']): Settled => ({ word, pending: null, since: 0 });

  it('reports the first verdict at once', () => {
    expect(settle(UNSETTLED, 'fair-net', 10).word).toBe('fair-net');
  });

  it('reports a worse verdict at once', () => {
    expect(settle(at('good'), 'poor-play', 10).word).toBe('poor-play');
  });

  it('reports a change of cause at the same level at once', () => {
    // It changes what the tooltip tells somebody to do.
    expect(settle(at('fair-net'), 'fair-play', 10).word).toBe('fair-play');
  });

  it('holds a better verdict back until it has lasted', () => {
    let s = settle(at('poor-net'), 'good', 1000);
    expect(s.word).toBe('poor-net');
    s = settle(s, 'good', 1000 + UPGRADE_HOLD_MS - 1);
    expect(s.word).toBe('poor-net');
    s = settle(s, 'good', 1000 + UPGRADE_HOLD_MS);
    expect(s.word).toBe('good');
  });

  it('starts the wait again when the better verdict wavers', () => {
    // A value sitting on a threshold is exactly what the hold is for: without
    // the restart, two good readings either side of a bad one would count as
    // twenty seconds of good.
    let s = settle(at('poor-net'), 'good', 0);
    s = settle(s, 'poor-net', UPGRADE_HOLD_MS / 2);
    s = settle(s, 'good', UPGRADE_HOLD_MS);
    expect(s.word).toBe('poor-net');
    s = settle(s, 'good', UPGRADE_HOLD_MS * 2);
    expect(s.word).toBe('good');
  });

  it('forgets everything when there is nothing to say', () => {
    expect(settle(at('poor-net'), null, 10)).toEqual({ word: null, pending: null, since: 10 });
  });
});
