/**
 * The crop mode is the playlist's and the rectangle is each file's own. The
 * failure to guard against is one file's bars cutting another file's picture.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const setCalls: string[][] = [];
const detected: string[] = [];
let detectAnswer: { x: number; y: number; w: number; h: number } | null = null;

vi.mock('tauri-plugin-libmpv-api', () => ({
  command: vi.fn(async (name: string, args: string[]) => {
    if (name === 'set') setCalls.push(args);
  }),
  getProperty: vi.fn(async (name: string) =>
    name === 'video-params/w' ? 1920 : name === 'video-params/h' ? 1080 : name === 'video-params/par' ? 1 : null,
  ),
  setProperty: vi.fn(async () => undefined),
}));
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (cmd: string, args: { path: string }) => {
    if (cmd === 'crop_detect') {
      detected.push(args.path);
      return detectAnswer;
    }
    return undefined;
  }),
}));

import { crop, restoreCrop, setCrop } from './crop.svelte';
import { cropFor, history } from './history.svelte';
import { player } from './player.svelte';

const A = '/Films/Show/e01.mkv';
const B = '/Films/Show/e02.mkv';
const OTHER = '/Films/Other/film.mkv';

function open(path: string) {
  player.filePath = path;
  player.filename = path.split('/').pop() ?? null;
  restoreCrop();
}

function lastCrop(): string | undefined {
  return setCalls.filter((c) => c[0] === 'video-crop').at(-1)?.[1];
}

beforeEach(() => {
  vi.useFakeTimers();
  localStorage.clear();
  history.prefs = { enabled: true, excluded: [] };
  setCalls.length = 0;
  detected.length = 0;
  detectAnswer = { x: 0, y: 138, w: 1920, h: 804 };
});

afterEach(() => {
  vi.useRealTimers();
});

describe('crop across a playlist', () => {
  it('auto on one file measures every other file for itself', async () => {
    open(A);
    await setCrop('auto');
    expect(lastCrop()).toBe('1920x804+0+138');

    detectAnswer = null; // the next episode has no bars
    open(B);
    expect(crop.mode).toBe('auto');
    await vi.advanceTimersByTimeAsync(2000);
    expect(detected.at(-1)).toBe(B);
    expect(lastCrop()).toBe('');
    // …and that answer is cached, so reopening does not decode again.
    expect(cropFor(B)).toMatchObject({ mode: 'auto', rect: '' });
    detected.length = 0;
    open(B);
    await vi.advanceTimersByTimeAsync(2000);
    expect(detected).toEqual([]);
  });

  it('a named shape is computed for each file, not copied', async () => {
    open(A);
    await setCrop('2.39');
    open(B);
    await vi.advanceTimersByTimeAsync(10);
    expect(lastCrop()).toBe('1920x804+0+138');
    expect(detected).toEqual([]);
  });

  it('none on one episode turns it off for the ones already cached', async () => {
    open(A);
    await setCrop('auto');
    open(B);
    await setCrop('none');
    setCalls.length = 0;
    open(A);
    await vi.advanceTimersByTimeAsync(2000);
    expect(crop.mode).toBe('none');
    expect(lastCrop()).toBeUndefined();
  });

  it('stays inside its playlist', async () => {
    open(A);
    await setCrop('auto');
    open(OTHER);
    await vi.advanceTimersByTimeAsync(2000);
    expect(crop.mode).toBe('none');
    expect(detected).toEqual([A]);
  });
});
