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
 * The mouse wheel is here too, because a notch is the same question asked of
 * another control: `wheel` says whether the vertical wheel is the volume or a
 * seek (Shift+wheel is then the other one, see `wheel.ts`), and `wheelStep`
 * how far one notch seeks. Volume stays the default because on macOS a
 * vertical two-finger trackpad swipe is the same event as a wheel, and in seek
 * mode it seeks too.
 *
 * A leaf on purpose. `keys.svelte.ts` reads it to label the actions, and the
 * settings sheet and `input` read it too. Putting it in `seek.svelte.ts` would
 * give the key table a path to mpv, cast and the thumbnail service.
 */

import type { WheelMode } from './wheel';

const PREFS_KEY = 'frameplayer.seek';

/// The choices the settings sheet offers. Named values, not a range, so the
/// sheet shows them as pills (see "two control shapes" in rules/ui-surfaces.md).
export const SHORT_STEPS = [1, 2, 3, 5, 10] as const;
export const LONG_STEPS = [10, 15, 20, 30, 60] as const;
export const WHEEL_STEPS = [1, 5, 10, 30] as const;
export const WHEEL_MODES = ['volume', 'seek'] as const satisfies readonly WheelMode[];

export type ShortStep = (typeof SHORT_STEPS)[number];
export type LongStep = (typeof LONG_STEPS)[number];
export type WheelStep = (typeof WHEEL_STEPS)[number];

class SeekSteps {
  short = $state<ShortStep>(5);
  long = $state<LongStep>(10);
  wheel = $state<WheelMode>('volume');
  // 5 s is what a notch moved before this was a setting: 20 px a second
  // against a 100 px notch.
  wheelStep = $state<WheelStep>(5);
}

export const seekSteps = new SeekSteps();

export function loadSeekSteps() {
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    if (!raw) return;
    const saved = JSON.parse(raw) as { short?: number; long?: number; wheel?: string; wheelStep?: number };
    const short = SHORT_STEPS.find((s) => s === saved.short);
    const long = LONG_STEPS.find((s) => s === saved.long);
    const wheel = WHEEL_MODES.find((m) => m === saved.wheel);
    const wheelStep = WHEEL_STEPS.find((s) => s === saved.wheelStep);
    if (short) seekSteps.short = short;
    if (long) seekSteps.long = long;
    if (wheel) seekSteps.wheel = wheel;
    if (wheelStep) seekSteps.wheelStep = wheelStep;
  } catch {
    // corrupt entry: the defaults stay
  }
}

export function setSeekStep(which: 'short', value: ShortStep): void;
export function setSeekStep(which: 'long', value: LongStep): void;
export function setSeekStep(which: 'short' | 'long', value: number) {
  if (which === 'short') seekSteps.short = value as ShortStep;
  else seekSteps.long = value as LongStep;
  save();
}

export function setWheelMode(mode: WheelMode) {
  seekSteps.wheel = mode;
  save();
}

export function setWheelStep(value: WheelStep) {
  seekSteps.wheelStep = value;
  save();
}

function save() {
  const { short, long, wheel, wheelStep } = seekSteps;
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify({ short, long, wheel, wheelStep }));
  } catch {
    // not critical: the choice simply will not survive a restart
  }
}
