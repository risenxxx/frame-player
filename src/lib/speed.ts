/**
 * Playback speed: the values a step lands on.
 *
 * A fixed ladder, not a factor. The keys used to multiply by 1.25, which from
 * 1× gives 1.25, 1.56, 1.95, 2.44 — exact, and unreadable: nobody can say what
 * 1.56× means or get back to 1.5× with the keys. The ladder is the values
 * people ask for by name, closer together where the ear notices the difference.
 *
 * A speed that is not on the ladder (set by a room, or by the viewer's own
 * mpv.conf) steps to the nearest rung in the direction asked, so one press
 * always lands on a round value and never skips one.
 */

export const SPEED_LADDER = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4] as const;
export const SPEED_MIN = SPEED_LADDER[0];
export const SPEED_MAX = SPEED_LADDER[SPEED_LADDER.length - 1];

/// Rungs closer than this to the current speed count as "here".
const EPS = 0.005;

/// The next rung above (`dir` 1) or below (`dir` -1) `from`; the end of the
/// ladder stays where it is.
export function nextSpeed(from: number, dir: 1 | -1): number {
  if (dir > 0) return SPEED_LADDER.find((s) => s > from + EPS) ?? SPEED_MAX;
  return [...SPEED_LADDER].reverse().find((s) => s < from - EPS) ?? SPEED_MIN;
}

/// Clamped and rounded to hundredths, which is what mpv is given and what the
/// readouts print.
export function clampSpeed(v: number): number {
  return Math.round(Math.max(SPEED_MIN, Math.min(SPEED_MAX, v)) * 100) / 100;
}
