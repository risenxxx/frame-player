/**
 * The second subtitle's place: from mpv's 720-high units, the bars' CSS pixels
 * and a drag to the fractions sublines.lua lays the line out with.
 */

import { describe, expect, it } from 'vitest';

import {
  SNAP_PX,
  defaultPlace,
  dragPlace,
  parseBox,
  placeArgs,
  placeInForce,
  validPlace,
  type Box,
  type LinePlace,
} from './second-line';

const W = 1280;
const H = 720;

describe('defaultPlace', () => {
  it("is centred at mpv's own subtitle margin from the top", () => {
    expect(defaultPlace(22)).toEqual({ x: 0.5, y: 22 / 720, edge: 'top' });
  });
});

describe('placeInForce', () => {
  const top = defaultPlace(22);

  it('is the place itself while the bars are away', () => {
    expect(placeInForce(top, H, 48, 90, false, true)).toEqual(top);
  });

  it('goes under the title bar while it is up', () => {
    expect(placeInForce(top, H, 48, 90, true, true).y * H).toBeCloseTo(52);
  });

  it('stays where it is when that is already below the bar', () => {
    const low: LinePlace = { x: 0.3, y: 0.2, edge: 'top' };
    expect(placeInForce(low, H, 48, 90, true, true)).toEqual(low);
  });

  it('stands on the control bar when it hangs from the bottom', () => {
    const bottom: LinePlace = { x: 0.5, y: 0.98, edge: 'bottom' };
    expect(placeInForce(bottom, H, 48, 90, true, true).y * H).toBeCloseTo(H - 94);
  });

  it('lets the control bar cover it when the viewer turned the lift off', () => {
    const bottom: LinePlace = { x: 0.5, y: 0.98, edge: 'bottom' };
    expect(placeInForce(bottom, H, 48, 90, true, false)).toEqual(bottom);
  });

  it('survives a window not measured yet', () => {
    expect(placeInForce(top, 0, 48, 90, true, true)).toEqual(top);
  });
});

describe('parseBox', () => {
  it('reads what the script reports', () => {
    expect(parseBox('0.47422 0.05694 0.53750 0.16528')).toEqual({
      x0: 0.47422,
      y0: 0.05694,
      x1: 0.5375,
      y1: 0.16528,
    });
  });

  it('reads it as mpv formats a node, in quotes', () => {
    expect(parseBox('"0.42308 0.08828 0.58317 0.13906"')?.x0).toBe(0.42308);
    expect(parseBox('""')).toBeNull();
  });

  it('is nothing while nothing is drawn, or for anything else', () => {
    expect(parseBox('')).toBeNull();
    expect(parseBox('0.1 0.2 0.3')).toBeNull();
    expect(parseBox('0.5 0.2 0.4 0.3')).toBeNull();
    expect(parseBox('a b c d')).toBeNull();
  });
});

describe('dragPlace', () => {
  // A line at the default place, drawn a little under its anchor as libass does.
  const start: LinePlace = { x: 0.5, y: 0.05, edge: 'top' };
  const box: Box = { x0: 0.45, y0: 0.057, x1: 0.55, y1: 0.12 };

  it('does not move the line on a grab without a move', () => {
    expect(dragPlace(start, box, 0, 0, W, H)).toEqual({ place: start, snapped: true });
  });

  it('moves it with the pointer', () => {
    const { place, snapped } = dragPlace(start, box, 128, 72, W, H);
    expect(snapped).toBe(false);
    expect(place.edge).toBe('top');
    expect(place.x).toBeCloseTo(0.6);
    expect(place.y).toBeCloseTo(0.15);
  });

  it('snaps the centre to the middle within the threshold, and only within it', () => {
    expect(dragPlace({ ...start, x: 0.3 }, box, 0.2 * W + SNAP_PX - 1, 0, W, H).place.x).toBe(0.5);
    expect(dragPlace({ ...start, x: 0.3 }, box, 0.2 * W - SNAP_PX + 1, 0, W, H).snapped).toBe(true);
    const off = dragPlace({ ...start, x: 0.3 }, box, 0.2 * W + SNAP_PX + 1, 0, W, H);
    expect(off.snapped).toBe(false);
    expect(off.place.x).toBeGreaterThan(0.5);
  });

  it('keeps the line inside the window', () => {
    const left = dragPlace(start, box, -W, -H, W, H).place;
    expect(left.x).toBeCloseTo(0.05);
    expect(left.y).toBeCloseTo(-0.007);
    const right = dragPlace(start, box, W, 0, W, H).place;
    expect(right.x).toBeCloseTo(0.95);
  });

  it('hangs from the bottom once its centre is in the lower half', () => {
    const { place } = dragPlace(start, box, 0, 0.6 * H, W, H);
    expect(place.edge).toBe('bottom');
    // The box's bottom, less the offset carried over from the top anchor.
    expect(place.y).toBeCloseTo(0.12 + 0.6 - 0.007);
  });

  it('can be put on the very bottom', () => {
    const { place } = dragPlace(start, box, 0, H, W, H);
    expect(place.edge).toBe('bottom');
    expect(place.y).toBeCloseTo(1 - 0.007);
  });

  it('keeps a bottom anchor when grabbed and let go', () => {
    const low: LinePlace = { x: 0.5, y: 0.9, edge: 'bottom' };
    const lowBox: Box = { x0: 0.45, y0: 0.846, x1: 0.55, y1: 0.91 };
    expect(dragPlace(low, lowBox, 0, 0, W, H).place).toEqual(low);
  });
});

describe('validPlace', () => {
  it('takes a stored place back', () => {
    expect(validPlace({ x: 0.5, y: 0.9, edge: 'bottom' })).toEqual({ x: 0.5, y: 0.9, edge: 'bottom' });
  });

  it('refuses anything else', () => {
    expect(validPlace(null)).toBeNull();
    expect(validPlace({ x: 0.5, y: 2, edge: 'top' })).toBeNull();
    expect(validPlace({ x: 0.5, y: 0.5, edge: 'left' })).toBeNull();
    expect(validPlace({ x: '0.5', y: 0.5, edge: 'top' })).toBeNull();
  });
});

describe('placeArgs', () => {
  it('sends the place as fractions and an edge', () => {
    expect(placeArgs(true, { x: 0.5, y: 0.05, edge: 'top' })).toEqual([
      'sublines',
      'place',
      '1',
      '0.5000',
      '0.0500',
      'top',
    ]);
  });

  it('says hidden without losing the place', () => {
    expect(placeArgs(false, { x: 0.3, y: 0.9, edge: 'bottom' })[2]).toBe('0');
  });
});
