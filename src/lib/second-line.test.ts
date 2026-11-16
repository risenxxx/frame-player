/**
 * The second subtitle's place: from mpv's 720-high units and the bars' CSS
 * pixels to the fractions secondsub.lua lays the line out with.
 */

import { describe, expect, it } from 'vitest';

import { lineTop, placeArgs } from './second-line';

describe('lineTop', () => {
  it("sits at mpv's own subtitle margin with nothing over the picture", () => {
    expect(lineTop(22, 720, 48, false)).toBe(22);
    expect(lineTop(22, 1080, 48, false)).toBe(33);
  });

  it('goes under the title bar while it is up', () => {
    expect(lineTop(22, 720, 48, true)).toBe(52);
  });

  it('never goes above its own margin for a short bar', () => {
    expect(lineTop(22, 1440, 28, true)).toBe(44);
  });
});

describe('placeArgs', () => {
  it('sends the top edge as a fraction of the window', () => {
    expect(placeArgs(true, 36, 720)).toEqual(['secondsub', 'place', '1', '0.0500', '0.5000']);
  });

  it('says hidden without losing the place', () => {
    expect(placeArgs(false, 36, 720)).toEqual(['secondsub', 'place', '0', '0.0500', '0.5000']);
  });

  it('is the same message when a resize keeps the line where it was', () => {
    expect(placeArgs(true, lineTop(22, 720, 48, false), 720)).toEqual(
      placeArgs(true, lineTop(22, 1080, 48, false), 1080),
    );
  });

  it('survives a window not measured yet', () => {
    expect(placeArgs(true, 22, 0)[3]).toBe('0.0000');
  });
});
