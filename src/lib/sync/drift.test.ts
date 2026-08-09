import { describe, expect, it } from 'vitest';

import {
  CATCHUP_S,
  DEADBAND_S,
  HARD_SEEK_S,
  MAX_ADJUST,
  PAUSED_TOLERANCE_S,
  correctionFor,
  speedChanged,
} from './drift';

// The roadmap decided this in advance and the reasoning is measured elsewhere in
// the project: an exact seek costs decoding from the preceding keyframe — 1.9 s
// on a long-GOP file — so a player that seeks to fix 300 ms of drift stops for
// two seconds to correct a third of a second and comes out further behind than
// it started. Hence three bands, and hence a test on where they fall.

describe('correctionFor, while playing', () => {
  it('leaves imperceptible differences alone', () => {
    expect(correctionFor(0, 1, false)).toEqual({ do: 'nothing' });
    expect(correctionFor(DEADBAND_S - 0.01, 1, false)).toEqual({ do: 'nothing' });
    expect(correctionFor(-(DEADBAND_S - 0.01), 1, false)).toEqual({ do: 'nothing' });
  });

  it('slows down when this player is ahead, and speeds up when it is behind', () => {
    const ahead = correctionFor(0.5, 1, false);
    const behind = correctionFor(-0.5, 1, false);
    expect(ahead.do).toBe('speed');
    expect(behind.do).toBe('speed');
    if (ahead.do !== 'speed' || behind.do !== 'speed') return;
    expect(ahead.speed).toBeLessThan(1);
    expect(behind.speed).toBeGreaterThan(1);
    // Symmetric: neither direction is favoured.
    expect(1 - ahead.speed).toBeCloseTo(behind.speed - 1, 5);
  });

  it('aims to erase the difference over CATCHUP_S', () => {
    // Half a second behind → run 0.5/10 = 5 % fast, which closes it in ten
    // seconds. Proportional rather than a fixed step, so a small difference gets
    // a small correction and settles instead of hunting around the deadband.
    const plan = correctionFor(-0.5, 1, false);
    expect(plan.do).toBe('speed');
    if (plan.do !== 'speed') return;
    expect(plan.speed).toBeCloseTo(1 + 0.5 / CATCHUP_S, 5);
  });

  it('never bends the speed further than MAX_ADJUST', () => {
    // Just inside the seek threshold, which is the largest drift this band ever
    // sees — and even there the correction has to stay inaudible.
    const plan = correctionFor(-(HARD_SEEK_S - 0.01), 1, false);
    expect(plan.do).toBe('speed');
    if (plan.do !== 'speed') return;
    expect(plan.speed).toBeLessThanOrEqual(1 + MAX_ADJUST + 1e-9);
  });

  it('corrects around the room’s speed rather than around 1', () => {
    // A room watching at 1.5x that is half a second behind must end up near
    // 1.5 × 1.05, not near 1.05 — otherwise correcting the drift would also
    // silently undo the speed everybody chose.
    const plan = correctionFor(-0.5, 1.5, false);
    expect(plan.do).toBe('speed');
    if (plan.do !== 'speed') return;
    expect(plan.speed).toBeCloseTo(1.5 * (1 + 0.5 / CATCHUP_S), 2);
  });

  it('gives up and seeks once catching up would take too long', () => {
    expect(correctionFor(HARD_SEEK_S + 0.01, 1, false)).toEqual({ do: 'seek' });
    expect(correctionFor(-60, 1, false)).toEqual({ do: 'seek' });
  });

  it('does nothing with a difference that is not a number', () => {
    // `targetPosition()` is arithmetic on a clock estimate and a duration; an
    // absent one produces NaN, and `NaN > x` is false, so without the guard this
    // would silently fall through to the speed band and write NaN to mpv.
    expect(correctionFor(NaN, 1, false)).toEqual({ do: 'nothing' });
    expect(correctionFor(Infinity, 1, false)).toEqual({ do: 'nothing' });
  });
});

describe('correctionFor, while paused', () => {
  it('never touches the speed — there is none to bend', () => {
    for (const drift of [0.3, 1, 5, -0.3, -30]) {
      expect(correctionFor(drift, 1, true).do).not.toBe('speed');
    }
  });

  it('holds a much tighter tolerance than a playing room', () => {
    // Everyone is looking at a still frame, which is exactly where a difference
    // shows — and there is no way to remove it but to seek.
    expect(correctionFor(PAUSED_TOLERANCE_S - 0.01, 1, true)).toEqual({ do: 'nothing' });
    expect(correctionFor(PAUSED_TOLERANCE_S + 0.01, 1, true)).toEqual({ do: 'seek' });
    expect(PAUSED_TOLERANCE_S).toBeLessThan(HARD_SEEK_S);
  });
});

describe('speedChanged', () => {
  it('ignores a difference too small to be worth an mpv write', () => {
    // The property lives on mpv's core thread and rewriting it every second with
    // a value a hair from the last is work for nothing — and, on a file with an
    // audio filter chain, an audible seam.
    expect(speedChanged(1, 1.001)).toBe(false);
    expect(speedChanged(1, 1.02)).toBe(true);
    expect(speedChanged(1.05, 1)).toBe(true);
  });
});
