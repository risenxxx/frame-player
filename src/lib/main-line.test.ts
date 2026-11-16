/**
 * The main subtitle line: what the script reports, the ceiling that keeps it
 * clear of the control bar, and the `sub-pos` a drag lands on.
 */

import { describe, expect, it } from 'vitest';

import { dragSubPos, liftCeiling, parseMainLine } from './main-line';

describe('parseMainLine', () => {
  it('reads what the script reports, quoted as mpv formats a node', () => {
    expect(parseMainLine('"0.45234 0.90694 0.55859 0.96250 100.000 0.95278"')).toEqual({
      box: { x0: 0.45234, y0: 0.90694, x1: 0.55859, y1: 0.9625 },
      pos: 100,
      range: 0.95278,
    });
  });

  it('is nothing for no line, or for anything else', () => {
    expect(parseMainLine('""')).toBeNull();
    expect(parseMainLine('0.1 0.2 0.3 0.4')).toBeNull();
    expect(parseMainLine('0.1 0.2 0.3 0.4 100 0')).toBeNull();
  });
});

describe('liftCeiling', () => {
  it("puts the line box's end just above the bar", () => {
    // 720 high, margin 34: the line box ends at 6.86 px per unit.
    const c = liftCeiling(720, 90, 34, 8)!;
    expect(((720 - 34) * c) / 100).toBeCloseTo(720 - 98);
  });

  it('scales the margin with the window, as mpv does', () => {
    const c = liftCeiling(1440, 180, 34, 16)!;
    expect(c).toBeCloseTo(liftCeiling(720, 90, 34, 8)!);
  });

  it('never goes past the bottom, nor above the top', () => {
    expect(liftCeiling(720, 0, 34, 0)).toBe(100);
    expect(liftCeiling(720, 800, 34, 8)).toBe(0);
  });

  it('has nothing to say about a window not measured yet', () => {
    expect(liftCeiling(0, 90, 34, 8)).toBeNull();
  });
});

describe('dragSubPos', () => {
  it('moves the line with the pointer', () => {
    // 100 units over 0.95 of a 720-high window: 6.84 px each.
    expect(dragSubPos(100, -0.95 * 720 * 0.5, 720, 0.95)).toBe(50);
  });

  it('stops at the top and at the bottom edge', () => {
    expect(dragSubPos(20, -720, 720, 0.95)).toBe(0);
    expect(dragSubPos(90, 300, 720, 0.95)).toBe(100);
  });

  it('keeps a tenth', () => {
    expect(dragSubPos(50, 1, 720, 0.95)).toBe(50.1);
  });

  it('leaves the line alone with nothing to measure against', () => {
    expect(dragSubPos(80, 50, 0, 0.95)).toBe(80);
  });
});
