/**
 * `sub-speed` is a ratio whose direction is easy to get backwards, and backwards
 * doubles the drift instead of removing it — a mistake that passes every glance
 * at the first minute of a film.
 */

import { describe, expect, it } from 'vitest';

import {
  NTSC_FILM,
  SUB_SPEED_PRESETS,
  isPreset,
  isUnitSpeed,
  presetFactor,
  snapFps,
  subSpeedFactor,
} from './sub-speed';

describe('subSpeedFactor', () => {
  it('stretches a PAL subtitle onto an NTSC film video', () => {
    // mpv shows subtitle time (video − delay) / factor: a line at 60 s in the
    // 25 fps subtitle belongs at 62.56 s of the 23.976 fps video. Measured in
    // mpv 0.41 with exactly that line.
    const factor = subSpeedFactor(25, 23.976);
    expect(factor).toBeCloseTo(25 / NTSC_FILM, 9);
    expect(60 * factor).toBeCloseTo(62.56, 2);
  });

  it('treats the rounded and the reported NTSC rate as one', () => {
    expect(subSpeedFactor(23.976, 23.976024)).toBe(1);
    expect(subSpeedFactor(23.98, NTSC_FILM)).toBe(1);
    expect(subSpeedFactor(29.97, 30000 / 1001)).toBe(1);
  });

  it('keeps 24 against 23.976 apart', () => {
    // A tenth of a percent: 3.6 s an hour, which is why it is a preset.
    expect(subSpeedFactor(24, 23.976)).toBeCloseTo(1.001, 6);
  });
});

describe('snapFps', () => {
  it('leaves integer rates alone', () => {
    expect(snapFps(25)).toBe(25);
    expect(snapFps(24)).toBe(24);
  });
});

describe('presets', () => {
  it('recognizes a factor that went through storage', () => {
    const stored = JSON.parse(JSON.stringify(presetFactor(SUB_SPEED_PRESETS[0])));
    expect(isPreset(stored, SUB_SPEED_PRESETS[0])).toBe(true);
    expect(isPreset(stored, SUB_SPEED_PRESETS[1])).toBe(false);
  });

  it('never mistakes 24→23.976 for "as is"', () => {
    expect(isUnitSpeed(presetFactor(SUB_SPEED_PRESETS[2]))).toBe(false);
    expect(isPreset(1, SUB_SPEED_PRESETS[2])).toBe(false);
  });
});
