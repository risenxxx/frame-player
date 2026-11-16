/**
 * The second subtitle drawn by the web view, styled after mpv's own.
 *
 * mpv can draw a second subtitle (`secondary-sid`) only pinned to the top edge
 * of the frame: `secondary-sub-pos` becomes libass's line position, which lifts
 * a bottom-anchored line and stops it 7 px under the edge at 0, and any other
 * value hangs the line from a fixed *bottom*, so its distance from the top
 * changes with the number of lines and the size (measured on 0.41: 51, 14 and
 * 8 px for one, two and three lines at `secondary-sub-pos=12`). No option
 * reaches that margin — `sub-margin-y` included — so the player draws the line
 * itself, from `secondary-sub-text`, where the margin is ours.
 *
 * What it copies, so the pair reads as one: mpv's font, size (`sub-font-size`
 * is in pixels of a 720-pixel-high window and grows with the window, times
 * `sub-scale`), weight, text colour, and either the outline (+ shadow) or the
 * box, whichever `sub-border-style` says. The secondary track is plain text in
 * mpv as well (`secondary-sub-ass-override=strip`), so no styling of the
 * file's own is lost by drawing it here.
 *
 * Pure: the arithmetic is tested, and the component only applies it.
 */

import { parseMpvColor, type Rgba } from './settings/sub-color';

export interface SubStyle {
  font: string;
  fontSize: number;
  bold: boolean;
  scale: number;
  borderSize: number;
  borderStyle: string;
  shadowOffset: number;
  color: string;
  borderColor: string;
  backColor: string;
}

/// mpv 0.41's own defaults, read from a bare instance.
export const SUB_STYLE_DEFAULTS: SubStyle = {
  font: 'sans-serif',
  fontSize: 38,
  bold: false,
  scale: 1,
  borderSize: 1.65,
  borderStyle: 'outline-and-shadow',
  shadowOffset: 0,
  color: '#FFFFFFFF',
  borderColor: '#FF000000',
  backColor: '#AF000000',
};

/// The height mpv's sizes are expressed against.
const REFERENCE_H = 720;

/// libass sizes a font by its whole line (ascent + descent), CSS by its em, so
/// the same number draws larger in libass. Calibrated in the app with the same
/// cue on both slots, mpv's line under ours: at 0.85 the two measured 726 and
/// 727 device pixels wide and 119 high each (mpv's defaults, macOS, where
/// `sans-serif` is Helvetica to both). The line height is the size itself.
export const EM_PER_LIBASS_SIZE = 0.85;

function css(c: Rgba): string {
  return `rgba(${c.r}, ${c.g}, ${c.b}, ${Math.round((c.a / 255) * 1000) / 1000})`;
}

/// mpv's generic family names are fontconfig's; CSS spells two of them the same.
function fontFamily(font: string): string {
  const generic = ['sans-serif', 'serif', 'monospace'];
  const name = font.trim() || 'sans-serif';
  return generic.includes(name.toLowerCase()) ? name.toLowerCase() : `"${name.replace(/"/g, '')}", sans-serif`;
}

export interface LineStyle {
  fontFamily: string;
  fontSize: number;
  lineHeight: number;
  fontWeight: number;
  color: string;
  /// Outline width in CSS pixels (drawn as a stroke twice as wide, under the fill).
  stroke: number;
  strokeColor: string;
  /// `text-shadow` value, or 'none'.
  shadow: string;
  /// The box behind the text in the box style, or null.
  box: string | null;
}

/// The line's look for a window `windowH` CSS pixels high.
export function lineStyle(style: SubStyle, windowH: number): LineStyle {
  const unit = (windowH / REFERENCE_H) * style.scale;
  const size = style.fontSize * unit;
  const text = parseMpvColor(style.color) ?? { r: 255, g: 255, b: 255, a: 255 };
  const border = parseMpvColor(style.borderColor) ?? { r: 0, g: 0, b: 0, a: 255 };
  const back = parseMpvColor(style.backColor) ?? { r: 0, g: 0, b: 0, a: 175 };
  const boxed = style.borderStyle === 'background-box' || style.borderStyle === 'opaque-box';
  const shadow = style.shadowOffset * unit;
  return {
    fontFamily: fontFamily(style.font),
    fontSize: Math.round(size * EM_PER_LIBASS_SIZE * 100) / 100,
    lineHeight: Math.round(size * 100) / 100,
    fontWeight: style.bold ? 700 : 400,
    color: css(text),
    stroke: boxed ? 0 : Math.round(style.borderSize * unit * 100) / 100,
    strokeColor: css(border),
    shadow: !boxed && shadow > 0 ? `${shadow}px ${shadow}px 0 ${css(back)}` : 'none',
    box: boxed ? css(back) : null,
  };
}

/// Where the line's top edge goes, in CSS pixels from the top of the window:
/// mpv's own subtitle margin (`sub-margin-y`, in the same 720-high units) when
/// nothing is over the picture, and under the title bar while it is up.
export function lineTop(marginY: number, windowH: number, barPx: number, chromeUp: boolean): number {
  const margin = (marginY * windowH) / REFERENCE_H;
  return chromeUp ? Math.max(margin, barPx + BAR_GAP) : margin;
}

/// Between the title bar and the line while the bar is up.
const BAR_GAP = 4;
