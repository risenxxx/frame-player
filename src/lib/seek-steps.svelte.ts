/**
 * How far the seek keys jump. There are two lengths the viewer can set, and
 * one that stays fixed.
 *
 * `short` is the arrows and `long` is J/L. Both are the viewer's choice
 * because the right step depends on the material: 5 s suits a film, a lecture
 * wants 15 or 30, and someone checking a scene frame by frame wants 1 or 2.
 * The precise pair (Shift+arrows) is **not** configurable. It is the exact
 * one-second step, and it exists so the coarse keys can land on a keyframe.
 * If it followed `short`, setting the arrows to 1 s would leave two keys doing
 * the same thing.
 *
 * A leaf on purpose. `keys.svelte.ts` reads it to label the actions, and the
 * settings sheet and `input` read it too. Putting it in `seek.svelte.ts` would
 * give the key table a path to mpv, cast and the thumbnail service.
 */

const PREFS_KEY = 'frameplayer.seek';

/// The choices the settings sheet offers. Named values, not a range, so the
/// sheet shows them as pills (see "two control shapes" in rules/ui-surfaces.md).
export const SHORT_STEPS = [1, 2, 3, 5, 10] as const;
export const LONG_STEPS = [10, 15, 20, 30, 60] as const;

export type ShortStep = (typeof SHORT_STEPS)[number];
export type LongStep = (typeof LONG_STEPS)[number];

class SeekSteps {
  short = $state<ShortStep>(5);
  long = $state<LongStep>(10);
}

export const seekSteps = new SeekSteps();

export function loadSeekSteps() {
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    if (!raw) return;
    const saved = JSON.parse(raw) as { short?: number; long?: number };
    const short = SHORT_STEPS.find((s) => s === saved.short);
    const long = LONG_STEPS.find((s) => s === saved.long);
    if (short) seekSteps.short = short;
    if (long) seekSteps.long = long;
  } catch {
    // corrupt entry: the defaults stay
  }
}

export function setSeekStep(which: 'short', value: ShortStep): void;
export function setSeekStep(which: 'long', value: LongStep): void;
export function setSeekStep(which: 'short' | 'long', value: number) {
  if (which === 'short') seekSteps.short = value as ShortStep;
  else seekSteps.long = value as LongStep;
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify({ short: seekSteps.short, long: seekSteps.long }));
  } catch {
    // not critical: the choice simply will not survive a restart
  }
}
