/**
 * The second subtitle's look and place: it copies mpv's subtitle options, so
 * the arithmetic from mpv's 720-high units to CSS pixels is what has to hold.
 */

import { describe, expect, it } from 'vitest';

import { EM_PER_LIBASS_SIZE, SUB_STYLE_DEFAULTS, lineStyle, lineTop } from './second-line';

describe('lineStyle', () => {
  it('scales mpv sizes with the window and with sub-scale', () => {
    const at720 = lineStyle(SUB_STYLE_DEFAULTS, 720);
    expect(at720.lineHeight).toBe(38);
    expect(at720.fontSize).toBeCloseTo(38 * EM_PER_LIBASS_SIZE, 2);
    const bigger = lineStyle({ ...SUB_STYLE_DEFAULTS, scale: 1.5 }, 1440);
    expect(bigger.lineHeight).toBe(114);
    expect(bigger.stroke).toBeCloseTo(1.65 * 3, 2);
  });

  it('draws the outline style with a stroke and no box', () => {
    const look = lineStyle(SUB_STYLE_DEFAULTS, 720);
    expect(look.box).toBeNull();
    expect(look.strokeColor).toBe('rgba(0, 0, 0, 1)');
    expect(look.shadow).toBe('none');
  });

  it('draws the box style with the back colour and no stroke', () => {
    const look = lineStyle({ ...SUB_STYLE_DEFAULTS, borderStyle: 'background-box' }, 720);
    expect(look.stroke).toBe(0);
    expect(look.box).toBe('rgba(0, 0, 0, 0.686)');
  });

  it('keeps the text opacity the settings slider wrote', () => {
    expect(lineStyle({ ...SUB_STYLE_DEFAULTS, color: '#80FFFF00' }, 720).color).toBe(
      'rgba(255, 255, 0, 0.502)',
    );
  });

  it('quotes a named font and passes a generic one through', () => {
    expect(lineStyle({ ...SUB_STYLE_DEFAULTS, font: 'Arial' }, 720).fontFamily).toBe('"Arial", sans-serif');
    expect(lineStyle(SUB_STYLE_DEFAULTS, 720).fontFamily).toBe('sans-serif');
  });
});

describe('lineTop', () => {
  it("sits at mpv's own subtitle margin with nothing over the picture", () => {
    expect(lineTop(22, 720, 48, false)).toBe(22);
    expect(lineTop(22, 1080, 48, false)).toBe(33);
  });

  it('goes under the title bar while it is up', () => {
    expect(lineTop(22, 720, 48, true)).toBe(52);
  });

  it('never goes above its own margin for a short bar', () => {
    expect(lineTop(22, 1440, 28, true)).toBe(44);
  });
});
