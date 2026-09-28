/**
 * The dark between two pictures, off the Mac: the web view draws it, and the
 * video view is never asked.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('./platform', () => ({ IS_MAC: false }));
vi.mock('tauri-plugin-libmpv-api', () => ({
  command: vi.fn(async () => undefined),
  getProperty: vi.fn(async () => null),
  setProperty: vi.fn(async () => undefined),
}));

const asked: string[] = [];
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (name: string) => {
    asked.push(name);
    return undefined;
  }),
}));

import { RAISE_MS, curtain, curtainFileLoaded, curtainSettled, initCurtain, raiseCurtain } from './curtain.svelte';
import { player } from './player.svelte';

beforeEach(() => {
  vi.useFakeTimers();
  initCurtain({ loading: () => false });
  player.filename = 'film.mkv';
  player.filePath = '/film.mkv';
  player.videoW = 1920;
  player.videoH = 1080;
  player.sourceTransfer = 'bt.1886';
});

afterEach(() => {
  vi.useRealTimers();
});

describe('where there is no video view to fade', () => {
  it('is drawn by the web view, down and up, for every dark', async () => {
    for (let i = 0; i < 2; i++) {
      const dark = raiseCurtain();
      await vi.advanceTimersByTimeAsync(RAISE_MS + 40);
      await dark;
      expect(curtain.drawn).toBe(true);
      expect(curtain.on).toBe(true);
      curtainFileLoaded();
      curtainSettled(240);
      expect(curtain.on).toBe(false);
      await vi.advanceTimersByTimeAsync(400);
      expect(curtain.drawn).toBe(true);
    }
    expect(asked).toEqual([]);
  });
});
