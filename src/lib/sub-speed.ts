/**
 * Fitting a subtitle made for one frame rate onto a video running at another —
 * the arithmetic behind mpv's `sub-speed`, kept pure because every one of its
 * mistakes is a subtitle that looks right for the first ten minutes.
 *
 * A subtitle timed against a 25 fps release (a PAL speed-up) puts a line spoken
 * at film time T at T·23.976/25. On a 23.976 fps video it therefore does not
 * start late, it drifts — about 4 % of the elapsed time, two and a half minutes
 * an hour — which no delay can fix. mpv maps video time to subtitle time as
 * `(video − sub-delay) / sub-speed` (dec_sub.c, `pts_to_subtitle`), so the
 * factor that undoes it is **subtitle fps / video fps**, and the delay still
 * applies on top, in video time.
 */

/// NTSC film, which is what "23.976" means: every tagged value is a rounding of
/// this, and mpv reports it as 23.976024.
export const NTSC_FILM = 24000 / 1001;

/// The rates a person writes rounded. Snapping them is what keeps a subtitle
/// tagged "23.976" and a video reporting 23.976024 from producing a factor of
/// 1.000001 — invisible for a minute, a quarter of a second by the end of a
/// long film.
const NTSC_RATES = [24000 / 1001, 30000 / 1001, 48000 / 1001, 60000 / 1001];

export function snapFps(fps: number): number {
  return NTSC_RATES.find((rate) => Math.abs(fps - rate) < 0.01) ?? fps;
}

/// Anything closer to 1 than this is 1. A millionth is 7 ms over two hours,
/// and a factor stored as 1.0000000002 would otherwise be a record for ever.
const SPEED_EPSILON = 1e-6;

export function isUnitSpeed(factor: number): boolean {
  return Math.abs(factor - 1) < SPEED_EPSILON;
}

/// The factor that fits a subtitle made for `subFps` onto a video at `videoFps`.
export function subSpeedFactor(subFps: number, videoFps: number): number {
  const factor = snapFps(subFps) / snapFps(videoFps);
  return isUnitSpeed(factor) ? 1 : factor;
}

export type SubSpeedPreset = { from: number; to: number };

/// The manual choices, for a subtitle whose rate nobody told us — an external
/// file, an embedded track. These three are the pairs that actually occur: PAL
/// and NTSC releases of the same film both ways, and a 24 fps master against
/// its 23.976 video.
export const SUB_SPEED_PRESETS: readonly SubSpeedPreset[] = [
  { from: 25, to: NTSC_FILM },
  { from: NTSC_FILM, to: 25 },
  { from: 24, to: NTSC_FILM },
];

export function presetFactor(preset: SubSpeedPreset): number {
  return subSpeedFactor(preset.from, preset.to);
}

/// Whether a factor mpv holds is this preset. Loose enough for a value that
/// went through a float and localStorage, tight enough that 24→23.976 (1.001)
/// is never mistaken for "as is".
export function isPreset(factor: number, preset: SubSpeedPreset): boolean {
  return Math.abs(factor - presetFactor(preset)) < 1e-4;
}
