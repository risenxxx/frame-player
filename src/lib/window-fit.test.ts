import { describe, expect, it } from 'vitest';

import { fitWindow, floorForShape, pictureShape, pinchResizes, placeAround, shapeFits, shapeKey } from './window-fit';

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

describe('placeAround', () => {
  const screen = { x: 0, y: 0, w: 3456, h: 2000 };
  const center = (r: { x: number; y: number; w: number; h: number }) => ({
    x: r.x + r.w / 2,
    y: r.y + r.h / 2,
  });

  it('keeps the center, not the corner', () => {
    // A 2:1 film, then a square clip at the same area.
    const film = { x: 800, y: 500, w: 1414, h: 707 };
    const clip = placeAround(film, { w: 1000, h: 1000 }, screen, 48);
    expect(clip).toEqual({ x: 1007, y: 354, w: 1000, h: 1000 });
    // Whole pixels: half of one is as far off as an odd size can put it.
    expect(Math.abs(center(clip).x - center(film).x)).toBeLessThanOrEqual(0.5);
    expect(Math.abs(center(clip).y - center(film).y)).toBeLessThanOrEqual(0.5);
  });

  it('comes back the way it went', () => {
    const film = { x: 800, y: 500, w: 1414, h: 707 };
    const clip = placeAround(film, { w: 1000, h: 1000 }, screen, 48);
    const again = placeAround(clip, { w: 1414, h: 707 }, screen, 48);
    expect(Math.abs(again.x - film.x)).toBeLessThanOrEqual(1);
    expect(Math.abs(again.y - film.y)).toBeLessThanOrEqual(1);
  });

  it('stays inside the screen, one axis at a time', () => {
    // Near the top: growing upwards from the center would leave the screen.
    const film = { x: 800, y: 60, w: 1414, h: 707 };
    const clip = placeAround(film, { w: 1000, h: 1000 }, screen, 48);
    expect(clip.y).toBe(48);
    // …and across, nothing was in the way.
    expect(clip.x).toBe(1007);
  });

  it('is pulled in from the far edges too', () => {
    const corner = { x: 2400, y: 1500, w: 1000, h: 450 };
    const clip = placeAround(corner, { w: 670, h: 670 }, screen, 48);
    expect(clip.x + clip.w).toBeLessThanOrEqual(screen.w - 48);
    expect(clip.y + clip.h).toBe(screen.h - 48);
  });

  it('measures from where the screen starts', () => {
    // A second monitor to the left of the first, and above it.
    const left = { x: -2560, y: -300, w: 2560, h: 1400 };
    const film = { x: -2500, y: -280, w: 1414, h: 707 };
    const clip = placeAround(film, { w: 1000, h: 1000 }, left, 48);
    expect(clip.x).toBe(-2293);
    expect(clip.y).toBe(-252);
  });

  it('centers on the screen what the margins cannot hold', () => {
    const clip = placeAround({ x: 10, y: 10, w: 800, h: 600 }, { w: 960, h: 1960 }, { x: 0, y: 0, w: 1000, h: 2000 }, 48);
    expect(clip.x).toBe(20);
    expect(clip.y).toBe(20);
  });

  it('keeps the center where the screen is not known', () => {
    const clip = placeAround({ x: 100, y: 100, w: 1414, h: 707 }, { w: 1000, h: 1000 }, null, 48);
    expect(clip).toEqual({ x: 307, y: -46, w: 1000, h: 1000 });
  });
});

describe('pinchResizes', () => {
  const plain = { setting: 'resize' as const, fullscreen: false, resting: false, mini: false, covered: false };

  it('is the window over a picture in a window', () => {
    expect(pinchResizes(plain)).toBe(true);
  });

  it('is the zoom wherever the window cannot grow', () => {
    expect(pinchResizes({ ...plain, fullscreen: true })).toBe(false);
    expect(pinchResizes({ ...plain, mini: true })).toBe(false);
  });

  it('stands down where a resize would draw the screen twice', () => {
    expect(pinchResizes({ ...plain, resting: true })).toBe(false);
    expect(pinchResizes({ ...plain, covered: true })).toBe(false);
  });

  it('is the setting first', () => {
    expect(pinchResizes({ ...plain, setting: 'zoom' })).toBe(false);
  });
});
