/**
 * Keeping in step with the room, and the one decision the roadmap made in
 * advance: **correct drift by nudging speed, not by seeking.**
 *
 * The reasoning is measured elsewhere in this project and is worth restating,
 * because the obvious implementation is the wrong one. An exact seek costs
 * decoding from the preceding keyframe, and on a file with a long GOP that is
 * ~1.9 s (see "Seek flags are a performance contract" in CLAUDE.md) — so a
 * player that seeks to fix 300 ms of drift stops for two seconds to correct a
 * third of a second, arriving *further* out of step than it started and with a
 * visible stall. Ten seconds at 103 % of speed fixes the same 300 ms with
 * nothing to see and nothing to hear: mpv resamples with `scaletempo2` by
 * default, so pitch is preserved and a few per cent is inaudible.
 *
 * Seeking is still the right answer past a point — a viewer who arrives a minute
 * late is not going to be walked there at 110 % — so there are three bands, and
 * the thresholds are what this module is:
 *
 *   |drift| ≤ deadband  nothing — and the band is derived from how well this
 *                      machine knows the relay's clock rather than fixed, so a
 *                      close relay is held an order of magnitude tighter than a
 *                      distant one. See `deadbandFor`.
 *   ≤ 2 s              nudge the speed, aiming to erase it over ~10 s.
 *   > 2 s              one seek, and accept its cost.
 *
 * Pure arithmetic on purpose: a wrong answer here is silent (a room that feels
 * loose, or a film that will not settle) and this is the cheapest place to pin
 * it down.
 */

/**
 * The band inside which the difference is left alone — **derived from how well
 * the clock is actually known**, not a constant.
 *
 * A fixed 150 ms was the first version and it is what a viewer noticed: the room
 * held to within a tenth of a second and then stopped trying, because everything
 * below that was declared close enough. But a tenth of a second is not a
 * property of the network, it is a guess about it — over a nearby relay the
 * offset is known to a few milliseconds, and there is no reason to accept
 * fifteen times that.
 *
 * What the band must never go below is the *measurement*: correcting a
 * difference smaller than the error bars is chasing noise, and two clients each
 * uncertain by `u` can be `2u` apart while both being exactly right.
 *
 * The floor is separately useful — below ~40 ms the correction would be under
 * half a per cent of speed, which `speedChanged` declines to write to mpv
 * anyway, so a smaller band would only produce arithmetic nobody acts on.
 */
export const MAX_DEADBAND_S = 0.15;
export const MIN_DEADBAND_S = 0.04;

export function deadbandFor(uncertaintyMs: number): number {
  // Nothing measured yet — the conservative band, until the first round trips
  // land. Being briefly loose beats correcting against an offset of zero.
  if (!Number.isFinite(uncertaintyMs)) return MAX_DEADBAND_S;
  return Math.min(MAX_DEADBAND_S, Math.max(MIN_DEADBAND_S, (2 * uncertaintyMs) / 1000));
}

/** Past this, a seek is cheaper than the time spent catching up. */
export const HARD_SEEK_S = 2;

/**
 * Past this, a *paused* room seeks instead.
 *
 * Much tighter than the playing case, and it has to be: while paused there is no
 * speed to nudge, so drift can only be removed by seeking — and everybody
 * looking at a still frame is looking at whether it is the *same* still frame.
 */
export const PAUSED_TOLERANCE_S = 0.25;

/** How long a nudge aims to take to erase the drift it is correcting. */
export const CATCHUP_S = 10;

/** The most the speed may be bent, either way. */
export const MAX_ADJUST = 0.1;

/** What the player should do about the difference it has measured. */
export type Correction =
  | { do: 'nothing' }
  /** Write this speed to mpv. Already includes the room's own speed. */
  | { do: 'speed'; speed: number }
  /** Seek to the room's position. */
  | { do: 'seek' };

/**
 * Decide what to do about being `drift` seconds away from where the room is.
 *
 * `drift` is *local minus target*: positive means this player is ahead.
 */
export function correctionFor(
  drift: number,
  roomSpeed: number,
  paused: boolean,
  deadband: number,
): Correction {
  if (!Number.isFinite(drift)) return { do: 'nothing' };
  const size = Math.abs(drift);

  if (paused) {
    // No speed to bend, and a still frame is exactly where a difference shows.
    return size > PAUSED_TOLERANCE_S ? { do: 'seek' } : { do: 'nothing' };
  }
  if (size > HARD_SEEK_S) return { do: 'seek' };
  if (size <= deadband) return { do: 'nothing' };

  // Erase it over CATCHUP_S seconds: ahead by 1 s → run 10 % slow for ten
  // seconds. Proportional rather than a fixed step, so a small difference gets a
  // small correction and settles instead of hunting around the deadband.
  const adjust = clamp(-drift / CATCHUP_S, -MAX_ADJUST, MAX_ADJUST);
  return { do: 'speed', speed: round2(roomSpeed * (1 + adjust)) };
}

/**
 * Whether a speed write is worth making.
 *
 * mpv's `speed` is a property on its core thread, and rewriting it every second
 * with a value a hair from the last one is work for nothing — and, on a file
 * with an audio filter chain, an audible seam. Two decimals is also the
 * granularity `changeSpeed` already uses for the viewer's own presets.
 */
export function speedChanged(from: number, to: number): boolean {
  return Math.abs(from - to) >= 0.005;
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

function round2(v: number): number {
  return Math.round(v * 100) / 100;
}
