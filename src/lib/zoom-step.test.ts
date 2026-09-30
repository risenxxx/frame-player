import { describe, expect, it } from 'vitest';

import { WHEEL_ZOOM_STEP, zoomStep } from './zoom-step';

describe('zoomStep', () => {
  it('is a fixed step for a wheel notch, whichever way it turns', () => {
    // Chromium's notch and its 120-pixel cousin.
    expect(zoomStep(-100, 0)).toBe(WHEEL_ZOOM_STEP);
    expect(zoomStep(100, 0)).toBe(-WHEEL_ZOOM_STEP);
    expect(zoomStep(-120, 0)).toBe(WHEEL_ZOOM_STEP);
  });

  it('is a step for a wheel that counts in lines or pages', () => {
    expect(zoomStep(-3, 1)).toBe(WHEEL_ZOOM_STEP);
    expect(zoomStep(1, 2)).toBe(-WHEEL_ZOOM_STEP);
  });

  it('zooms a pinch by what the fingers did', () => {
    // WebKit: deltaY = −100 × magnification. A spread of 5 % is ×1.05.
    expect(zoomStep(-5, 0)).toBeCloseTo(Math.log2(1.05), 10);
    expect(zoomStep(10, 0)).toBeCloseTo(Math.log2(0.9), 10);
  });

  it('adds up to the gesture rather than to the number of events', () => {
    // Forty small events of a real pinch: the zoom is their product, not
    // forty steps of 0.1 (which would be 4, past the 800 % ceiling).
    let z = 0;
    for (let i = 0; i < 40; i++) z += zoomStep(-2.5, 0);
    expect(Math.pow(2, z)).toBeCloseTo(Math.pow(1.025, 40), 8);
    expect(z).toBeLessThan(1.5);
  });

  it('does nothing for nothing', () => {
    expect(zoomStep(0, 0)).toBe(0);
    expect(zoomStep(Number.NaN, 0)).toBe(0);
  });
});
