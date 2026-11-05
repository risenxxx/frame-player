/**
 * Subtitle opacity, as the alpha of an mpv colour.
 *
 * mpv has no opacity option for subtitles: the text is `sub-color` and the box
 * (or, in the outline style, the shadow) is `sub-back-color`, and the alpha
 * channel of each is the opacity. So the sliders read and write the alpha and
 * leave the colour alone — a viewer who set yellow subtitles in their own
 * mpv.conf keeps yellow ones, only more or less see-through.
 *
 * mpv takes `#RRGGBB`, `#AARRGGBB` (alpha first) and `r/g/b[/a]` in 0..1. What
 * we write is always `#AARRGGBB`.
 */

export interface Rgba {
  r: number;
  g: number;
  b: number;
  /// 0..255
  a: number;
}

export function parseMpvColor(s: string | null | undefined): Rgba | null {
  if (!s) return null;
  const v = s.trim();
  const hex = /^#([0-9a-f]{6}|[0-9a-f]{8})$/i.exec(v);
  if (hex) {
    const h = hex[1];
    const n = (i: number) => parseInt(h.slice(i, i + 2), 16);
    return h.length === 8 ? { a: n(0), r: n(2), g: n(4), b: n(6) } : { a: 255, r: n(0), g: n(2), b: n(4) };
  }
  const parts = v.split('/').map(Number);
  if ((parts.length === 3 || parts.length === 4) && parts.every((p) => Number.isFinite(p) && p >= 0 && p <= 1)) {
    const c = (x: number) => Math.round(x * 255);
    return { r: c(parts[0]), g: c(parts[1]), b: c(parts[2]), a: parts.length === 4 ? c(parts[3]) : 255 };
  }
  return null;
}

const hex2 = (n: number) => Math.max(0, Math.min(255, Math.round(n))).toString(16).toUpperCase().padStart(2, '0');

export function formatMpvColor(c: Rgba): string {
  return `#${hex2(c.a)}${hex2(c.r)}${hex2(c.g)}${hex2(c.b)}`;
}

/// Opacity in percent, from a colour; `fallback` for one that does not parse.
export function opacityOf(s: string | null | undefined, fallback: string): number {
  const c = parseMpvColor(s) ?? parseMpvColor(fallback);
  return c ? Math.round((c.a / 255) * 100) : 100;
}

/// The same colour as `current` (or `fallback`) with this opacity.
export function withOpacity(percent: number, current: string | null | undefined, fallback: string): string {
  const c = parseMpvColor(current) ?? parseMpvColor(fallback) ?? { r: 255, g: 255, b: 255, a: 255 };
  return formatMpvColor({ ...c, a: (percent / 100) * 255 });
}
