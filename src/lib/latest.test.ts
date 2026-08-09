/**
 * The attempt-ordering guard.
 *
 * Worth testing for the reason the module exists: getting this wrong is silent
 * and permanent. Nothing throws, nothing logs — a stale answer simply lands
 * last and stays, and the symptom shows up somewhere else entirely (a track
 * list missing its subtitles, a download readout standing over the start
 * screen). None of that is visible in a screenshot, and none of it reproduces
 * on demand, since it needs two attempts to finish out of order.
 */

import { describe, expect, it } from 'vitest';

import { latest } from './latest';

describe('latest', () => {
  it('leaves a lone attempt fresh', () => {
    const runs = latest();
    expect(runs.begin().stale).toBe(false);
  });

  it('stales the older attempt as soon as a newer one begins', () => {
    const runs = latest();
    const first = runs.begin();
    expect(first.stale).toBe(false);
    const second = runs.begin();
    expect(first.stale).toBe(true);
    expect(second.stale).toBe(false);
  });

  it('re-reads on every access, which is what makes it usable after an await', async () => {
    const runs = latest();
    const run = runs.begin();
    // A boolean captured at `begin` would answer the question as it stood
    // before any waiting — the exact mistake this shape exists to prevent.
    await Promise.resolve();
    expect(run.stale).toBe(false);
    runs.begin();
    await Promise.resolve();
    expect(run.stale).toBe(true);
  });

  it('keeps only the newest fresh, however many are in flight', () => {
    const runs = latest();
    const attempts = [runs.begin(), runs.begin(), runs.begin()];
    expect(attempts.map((a) => a.stale)).toEqual([true, true, false]);
  });

  it('orders attempts per family, so two jobs cannot stale each other', () => {
    const tracks = latest();
    const chapters = latest();
    const trackRun = tracks.begin();
    chapters.begin();
    expect(trackRun.stale).toBe(false);
  });

  /**
   * The failure the guard is for, played out: a slow first attempt resolving
   * after a fast second one. Without the check the first write lands last and
   * the newer answer is gone for good.
   */
  it('lets the newer answer win when the older one finishes last', async () => {
    const runs = latest();
    const published: string[] = [];

    const attempt = async (value: string, delay: number) => {
      const run = runs.begin();
      await new Promise((r) => setTimeout(r, delay));
      if (run.stale) return;
      published.push(value);
    };

    const slow = attempt('old', 20);
    const fast = attempt('new', 0);
    await Promise.all([slow, fast]);

    expect(published).toEqual(['new']);
  });
});
