import { describe, expect, it } from 'vitest';

import { opacityOf, parseMpvColor, withOpacity } from './sub-color';

describe('subtitle colours', () => {
  it('reads the forms mpv takes, alpha first in hex', () => {
    expect(parseMpvColor('#AF000000')).toEqual({ a: 175, r: 0, g: 0, b: 0 });
    expect(parseMpvColor('#FFFF00')).toEqual({ a: 255, r: 255, g: 255, b: 0 });
    expect(parseMpvColor('1/1/0/0.5')).toEqual({ r: 255, g: 255, b: 0, a: 128 });
    expect(parseMpvColor('yellow')).toBeNull();
  });

  it('gives the opacity, falling back to mpv default for nothing or nonsense', () => {
    expect(opacityOf(null, '#AF000000')).toBe(69);
    expect(opacityOf('#80FFFFFF', '#FFFFFFFF')).toBe(50);
    expect(opacityOf('nonsense', '#FFFFFFFF')).toBe(100);
  });

  it('changes the alpha and keeps the colour the viewer chose', () => {
    expect(withOpacity(50, '#FFFF00', '#FFFFFFFF')).toBe('#80FFFF00');
    expect(withOpacity(100, null, '#FFFFFFFF')).toBe('#FFFFFFFF');
    expect(withOpacity(0, null, '#AF000000')).toBe('#00000000');
  });
});
