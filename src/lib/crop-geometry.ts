/**
 * The arithmetic behind `video-crop`, apart from mpv so it can be tested. Every
 * error in it is a plausible picture with the wrong strip missing, which is the
 * kind of bug nobody reports until a subtitle is cut in half.
 */

export type Rect = { x: number; y: number; w: number; h: number };
export type Frame = { w: number; h: number; par: number };

export function formatRect(r: Rect): string {
  return `${r.w}x${r.h}+${r.x}+${r.y}`;
}

export function parseRect(s: string): Rect | null {
  const m = /^(\d+)x(\d+)\+(\d+)\+(\d+)$/.exec(s);
  if (!m) return null;
  return { w: +m[1], h: +m[2], x: +m[3], y: +m[4] };
}

/**
 * The centred rectangle of `frame` whose display ratio is `ratio`, or null when
 * the frame already has it (within 1 %).
 *
 * Worked in display units, since that is where a ratio means anything: an
 * anamorphic frame's pixels are not square (`par`), and an aspect override
 * (`override` > 0) changes the shape the whole frame is shown at. A quarter
 * turn swaps the side the ratio is measured along. Edges land on even pixels,
 * because 4:2:0 chroma has one sample per two.
 */
export function aspectRect(frame: Frame, ratio: number, override: number, rotate: number): Rect | null {
  const whole = override > 0 ? override : (frame.w * frame.par) / frame.h;
  const par = (whole * frame.h) / frame.w;
  const target = rotate % 180 === 0 ? ratio : 1 / ratio;
  if (Math.abs(whole - target) / target < 0.01) return null;
  const even = (n: number) => Math.max(2, Math.round(n / 2) * 2);
  if (target > whole) {
    const h = Math.min(frame.h, even((frame.w * par) / target));
    return { x: 0, y: even((frame.h - h) / 2), w: frame.w, h };
  }
  const w = Math.min(frame.w, even((frame.h * target) / par));
  return { x: even((frame.w - w) / 2), y: 0, w, h: frame.h };
}

/**
 * Where `kept` sits inside a `full` frame, as an `object-position`: what puts
 * a full-frame thumbnail's kept part into a box of the cropped shape.
 * `object-fit: cover` alone centres it, which is right only while the bars
 * are equal.
 */
export function objectPosition(kept: Rect, full: { w: number; h: number }): string {
  const at = (offset: number, size: number, whole: number) =>
    whole > size ? `${+((offset / (whole - size)) * 100).toFixed(2)}%` : '50%';
  return `${at(kept.x, kept.w, full.w)} ${at(kept.y, kept.h, full.h)}`;
}
