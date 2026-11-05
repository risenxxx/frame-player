import { describe, expect, it } from 'vitest';

import { clampSpeed, nextSpeed } from './speed';

describe('speed ladder', () => {
  it('steps through round values from 1×', () => {
    const up: number[] = [];
    let s = 1;
    for (let i = 0; i < 4; i++) up.push((s = nextSpeed(s, 1)));
    expect(up).toEqual([1.25, 1.5, 1.75, 2]);
    expect(nextSpeed(1, -1)).toBe(0.75);
  });

  it('stops at both ends', () => {
    expect(nextSpeed(4, 1)).toBe(4);
    expect(nextSpeed(0.25, -1)).toBe(0.25);
  });

  it('steps an off-ladder speed to the nearest rung in that direction', () => {
    expect(nextSpeed(1.56, 1)).toBe(1.75);
    expect(nextSpeed(1.56, -1)).toBe(1.5);
    // A room's drift correction leaves 1.04: up is 1.25, down is 1.
    expect(nextSpeed(1.04, -1)).toBe(1);
  });

  it('clamps and rounds', () => {
    expect(clampSpeed(9)).toBe(4);
    expect(clampSpeed(0.1)).toBe(0.25);
    expect(clampSpeed(1.234)).toBe(1.23);
  });
});
