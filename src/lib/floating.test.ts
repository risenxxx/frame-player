/**
 * The placement arithmetic three surfaces share — tooltips, the context menu and
 * its submenu. Each of them used to place itself and each was wrong in a
 * different way, which is the argument for the module and also the argument for
 * these tests: the failures are geometric and only show at a window edge, which
 * is exactly where nobody clicks while testing by hand.
 */

import { describe, expect, it } from 'vitest';

import { flipAxis, shiftAxis } from './floating';

const PAD = 8;

describe('flipAxis', () => {
  it('takes the preferred side when it fits', () => {
    // A tooltip on a bar in the middle of the window: above, as asked.
    const r = flipAxis({ near: 400, far: 440, size: 30, limit: 800, preferBefore: true });
    expect(r.before).toBe(true);
    expect(r.pos).toBe(370);
  });

  it('flips when the preferred side has no room', () => {
    // The same tooltip on an element at the top of the window.
    const r = flipAxis({ near: 10, far: 50, size: 30, limit: 800, preferBefore: true });
    expect(r.before).toBe(false);
    expect(r.pos).toBe(50);
  });

  it('flips the other way too', () => {
    const r = flipAxis({ near: 700, far: 780, size: 60, limit: 800, preferBefore: false });
    expect(r.before).toBe(true);
    expect(r.pos).toBe(640);
  });

  it('takes the roomier side when neither fits, overriding the preference', () => {
    // The mini player: the menu is taller than the window, so no position shows
    // all of it. `preferBefore` is false and the roomier side is nonetheless
    // `before`, so this also pins that the preference loses — and `room` is what
    // the caller turns into a `max-height`.
    const r = flipAxis({ near: 250, far: 280, size: 500, limit: 300, preferBefore: false });
    expect(r.before).toBe(true);
    expect(r.room).toBe(250 - PAD);
    expect(r.pos).toBe(PAD);
  });

  it('picks the roomier side the other way round too', () => {
    const r = flipAxis({ near: 100, far: 140, size: 500, limit: 300, preferBefore: true });
    expect(r.before).toBe(false);
    expect(r.room).toBe(300 - PAD - 140);
  });

  it('keeps the gap out of the room it reports', () => {
    // `room` is the cap the caller may use, so a gap that is not available must
    // not be counted as space.
    const r = flipAxis({ near: 100, far: 140, size: 10, limit: 800, gap: 6, preferBefore: true });
    expect(r.room).toBe(100 - 6 - PAD);
    expect(r.pos).toBe(100 - 6 - 10);
  });

  it('never places the box past the near edge', () => {
    // Even when it does not fit, the box starts inside the window.
    const r = flipAxis({ near: 20, far: 30, size: 100, limit: 800, preferBefore: true });
    expect(r.pos).toBeGreaterThanOrEqual(PAD);
  });
});

describe('shiftAxis', () => {
  it('leaves a box that is already inside where it wanted to be', () => {
    expect(shiftAxis(100, 200, 800)).toBe(100);
  });

  it('slides a box back in from either edge', () => {
    expect(shiftAxis(-50, 200, 800)).toBe(PAD);
    expect(shiftAxis(700, 200, 800)).toBe(800 - PAD - 200);
  });

  it('parks a box wider than the window at the near edge', () => {
    // Something has to be cut off, and the start of a menu item is worth more
    // than its end.
    expect(shiftAxis(50, 1000, 400)).toBe(PAD);
    expect(shiftAxis(-50, 1000, 400)).toBe(PAD);
  });

  it('takes a caller-supplied padding', () => {
    expect(shiftAxis(-50, 100, 800, 20)).toBe(20);
  });
});
