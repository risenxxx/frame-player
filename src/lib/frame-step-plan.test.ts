import { describe, expect, it } from 'vitest';

import {
  frameDuration,
  landedRight,
  reversalBudget,
  reversalNeed,
  shouldReverse,
  swapsDecoder,
  type ReverseInputs,
} from './frame-step-plan';

const GiB = 1024 ** 3;

describe('reversalBudget', () => {
  it('is a quarter of the machine, capped at 4 GiB', () => {
    expect(reversalBudget(8 * GiB)).toBe(2 * GiB);
    expect(reversalBudget(36 * GiB)).toBe(4 * GiB);
  });

  it("falls back to mpv's own default when the machine will not say", () => {
    expect(reversalBudget(0)).toBe(GiB);
    expect(reversalBudget(Number.NaN)).toBe(GiB);
  });
});

describe('reversalNeed', () => {
  it('matches what mpv was measured to hold for a 4K group', () => {
    // 240 frames of 3840×2160 8-bit 4:2:0: mpv's resident size grew by 3.0 GB.
    const need = reversalNeed({ w: 3840, h: 2160, bpp: 12, fps: 24 }, 10);
    expect(need / 1e9).toBeGreaterThan(3.0);
    expect(need / 1e9).toBeLessThan(3.5);
  });

  it('puts a long 1080p60 group over the default buffer and inside a 36 GB machine', () => {
    const need = reversalNeed({ w: 1920, h: 1080, bpp: 12, fps: 60 }, 10);
    expect(need).toBeGreaterThan(GiB);
    expect(need).toBeLessThan(reversalBudget(36 * GiB));
  });

  it('doubles with 10-bit', () => {
    const eight = reversalNeed({ w: 1920, h: 1080, bpp: 12, fps: 24 }, 2);
    const ten = reversalNeed({ w: 1920, h: 1080, bpp: 24, fps: 24 }, 2);
    expect(ten).toBeCloseTo(eight * 2);
  });
});

describe('swapsDecoder', () => {
  it('leaves software decoding alone', () => {
    expect(swapsDecoder('no', 1920, 1080)).toBe(false);
    expect(swapsDecoder(null, 1920, 1080)).toBe(false);
  });

  it('takes hardware to software up to 1440p and not above', () => {
    expect(swapsDecoder('videotoolbox', 1920, 1080)).toBe(true);
    expect(swapsDecoder('d3d11va', 2560, 1440)).toBe(true);
    expect(swapsDecoder('videotoolbox', 3840, 2160)).toBe(false);
  });

  it('does nothing before the decoder has reported a size', () => {
    expect(swapsDecoder('videotoolbox', 0, 0)).toBe(false);
  });
});

describe('shouldReverse', () => {
  const ok: ReverseInputs = {
    enabled: true,
    slow: true,
    broken: false,
    local: true,
    abLoop: false,
    software: true,
    need: GiB,
    budget: 2 * GiB,
  };

  it('goes backward on a slow local file that fits', () => {
    expect(shouldReverse(ok)).toBe(true);
  });

  it.each([
    ['the switch is off', { enabled: false }],
    ['the first step was fast', { slow: false }],
    ['nothing was measured yet', { slow: null }],
    ['it already misbehaved here', { broken: true }],
    ['the source is a stream', { local: false }],
    ['an A–B loop is set', { abLoop: true }],
    ['decoding is in hardware', { software: false }],
    ['the group was not measured', { need: null }],
    ['the group does not fit', { need: 3 * GiB }],
  ])('stays native when %s', (_why, change) => {
    expect(shouldReverse({ ...ok, ...change })).toBe(false);
  });
});

describe('landedRight', () => {
  const frame = 1 / 60;

  it('accepts the frame asked for and its neighbours', () => {
    expect(landedRight(34.5, 34.5, frame)).toBe(true);
    expect(landedRight(34.5, 34.483, frame)).toBe(true);
  });

  it('catches an overflowed buffer, which lands seconds away', () => {
    expect(landedRight(39.9, 33.542, 1 / 24)).toBe(false);
    expect(landedRight(39.9, 35.383, frame)).toBe(false);
  });

  it('treats a missing reading as a bad landing', () => {
    expect(landedRight(10, null, frame)).toBe(false);
  });
});

describe('frameDuration', () => {
  it('uses the rate when there is one and 1/24 otherwise', () => {
    expect(frameDuration(60)).toBeCloseTo(1 / 60);
    expect(frameDuration(null)).toBeCloseTo(1 / 24);
    expect(frameDuration(0)).toBeCloseTo(1 / 24);
  });
});
