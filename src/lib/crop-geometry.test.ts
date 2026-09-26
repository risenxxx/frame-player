import { describe, expect, it } from 'vitest';

import { aspectRect, formatRect, objectPosition, parseRect } from './crop-geometry';

const HD = { w: 1920, h: 1080, par: 1 };

describe('aspectRect', () => {
  it('letterboxes a 16:9 frame to scope', () => {
    expect(aspectRect(HD, 2.39, -2, 0)).toEqual({ x: 0, y: 138, w: 1920, h: 804 });
  });

  it('pillarboxes a 16:9 frame to 4:3', () => {
    expect(aspectRect(HD, 4 / 3, -2, 0)).toEqual({ x: 240, y: 0, w: 1440, h: 1080 });
  });

  it('answers null for the shape the frame already has', () => {
    expect(aspectRect({ w: 1440, h: 1080, par: 1 }, 4 / 3, -2, 0)).toBeNull();
  });

  it('measures an anamorphic frame in display units', () => {
    // NTSC DVD widescreen: 720x480 stored, shown at 16:9.
    const dvd = { w: 720, h: 480, par: (16 / 9) * (480 / 720) };
    expect(aspectRect(dvd, 4 / 3, -2, 0)).toEqual({ x: 90, y: 0, w: 540, h: 480 });
    expect(aspectRect(dvd, 2.39, -2, 0)).toEqual({ x: 0, y: 62, w: 720, h: 358 });
  });

  it('follows an aspect override rather than the stored shape', () => {
    // A 4:3 frame the viewer has told mpv to show at 16:9.
    const r = aspectRect({ w: 1440, h: 1080, par: 1 }, 16 / 9, 16 / 9, 0);
    expect(r).toBeNull();
  });

  it('measures along the other side after a quarter turn', () => {
    // A 16:9 frame turned upright is 9:16; asking for 4:3 in that orientation
    // means a 3:4 rectangle of the stored frame, i.e. cutting its width.
    expect(aspectRect(HD, 4 / 3, -2, 90)).toEqual({ x: 556, y: 0, w: 810, h: 1080 });
  });
});

describe('rect strings', () => {
  it('round-trips', () => {
    const r = { x: 0, y: 138, w: 1920, h: 804 };
    expect(formatRect(r)).toBe('1920x804+0+138');
    expect(parseRect('1920x804+0+138')).toEqual(r);
  });

  it('rejects anything mpv would not have written', () => {
    expect(parseRect('')).toBeNull();
    expect(parseRect('1920x804')).toBeNull();
  });
});

describe('objectPosition', () => {
  it('centres equal bars', () => {
    expect(objectPosition({ x: 0, y: 138, w: 1920, h: 804 }, { w: 1920, h: 1080 })).toBe('50% 50%');
  });

  it('follows a crop that kept more of the bottom than the top', () => {
    expect(objectPosition({ x: 0, y: 200, w: 1920, h: 800 }, { w: 1920, h: 1080 })).toBe('50% 71.43%');
  });
});
