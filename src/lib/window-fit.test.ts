import { describe, expect, it } from 'vitest';

import { fitWindow, floorForShape, pictureShape, shapeFits, shapeKey } from './window-fit';

const MIN = { w: 480, h: 320 };
const HD = { w: 1920, h: 1080 };
const UPRIGHT = { w: 1080, h: 1920 };

describe('pictureShape', () => {
  it('is the display size when nothing turns the picture', () => {
    expect(pictureShape(1920, 1080, 0)).toEqual(HD);
    expect(pictureShape(1920, 1080, 180)).toEqual(HD);
  });

  it('turns with the picture', () => {
    // What mpv reports for a clip filmed upright: the size before the turn.
    expect(pictureShape(1920, 1080, 90)).toEqual(UPRIGHT);
    expect(pictureShape(1920, 1080, 270)).toEqual(UPRIGHT);
  });

  it('has no shape without a picture', () => {
    expect(pictureShape(0, 0, 0)).toBeNull();
    expect(pictureShape(1920, 0, 0)).toBeNull();
    expect(pictureShape(Number.NaN, 1080, 0)).toBeNull();
  });
});

describe('shapeKey', () => {
  it('tells an upright clip from the same clip lying down', () => {
    expect(shapeKey(HD)).not.toBe(shapeKey(UPRIGHT));
  });
});

describe('floorForShape', () => {
  it('widens a wide picture until the height is covered', () => {
    expect(floorForShape(HD, MIN)).toEqual({ w: 569, h: 320 });
    expect(floorForShape({ w: 2390, h: 1000 }, MIN)).toEqual({ w: 765, h: 320 });
  });

  it('heightens an upright one until the width is covered', () => {
    expect(floorForShape(UPRIGHT, MIN)).toEqual({ w: 480, h: 853 });
  });

  it('never lands under either minimum', () => {
    for (const shape of [HD, UPRIGHT, { w: 4, h: 3 }, { w: 1, h: 1 }, { w: 2390, h: 1000 }, { w: 3, h: 4 }]) {
      for (const min of [MIN, { w: 240, h: 135 }, { w: 960, h: 640 }, { w: 481, h: 321 }]) {
        const floor = floorForShape(shape, min);
        expect(floor.w).toBeGreaterThanOrEqual(min.w);
        expect(floor.h).toBeGreaterThanOrEqual(min.h);
      }
    }
  });
});

describe('fitWindow', () => {
  const room = { w: 1680, h: 945 };

  it('keeps the area and changes the shape', () => {
    const fit = fitWindow({ shape: { w: 4, h: 3 }, area: 1100 * 660, min: MIN, room });
    expect(fit.held).toBe(true);
    expect(fit.shrunk).toBe(false);
    expect(fit.w / fit.h).toBeCloseTo(4 / 3, 2);
    expect(Math.abs(fit.w * fit.h - 1100 * 660)).toBeLessThan(2000);
  });

  it('is stable: fitting what it produced changes nothing', () => {
    const once = fitWindow({ shape: HD, area: 1100 * 660, min: MIN, room });
    const twice = fitWindow({ shape: HD, area: once.w * once.h, min: MIN, room });
    expect(twice).toEqual(once);
  });

  it('shrinks into the screen and says so', () => {
    const fit = fitWindow({ shape: UPRIGHT, area: 1100 * 660, min: MIN, room });
    expect(fit).toEqual({ w: 532, h: 945, shrunk: true, held: true });
  });

  it('stops at the floor of the shape, not at the minimum per axis', () => {
    // Half size of a small clip: 320x180 asked for.
    const fit = fitWindow({ shape: { w: 640, h: 360 }, area: 320 * 180, min: MIN, room });
    expect(fit).toEqual({ w: 569, h: 320, shrunk: false, held: true });
  });

  it('gives the shape up where the screen cannot hold its floor', () => {
    // A 13-inch laptop: 853 is needed for 9:16 and 757 is what there is.
    const small = { w: 1392, h: 757 };
    const fit = fitWindow({ shape: UPRIGHT, area: 1100 * 660, min: MIN, room: small });
    expect(fit.held).toBe(false);
    expect(fit.w).toBe(480);
    expect(fit.h).toBe(757);
  });

  it('does not refuse for want of a screen', () => {
    const fit = fitWindow({ shape: UPRIGHT, area: 1100 * 660, min: MIN, room: null });
    expect(fit.held).toBe(true);
    expect(fit.shrunk).toBe(false);
  });
});

describe('shapeFits', () => {
  it('compares the floor with the room on both axes', () => {
    expect(shapeFits({ w: 480, h: 853 }, { w: 1392, h: 757 })).toBe(false);
    expect(shapeFits({ w: 569, h: 320 }, { w: 1392, h: 757 })).toBe(true);
    expect(shapeFits({ w: 480, h: 853 }, null)).toBe(true);
  });
});
