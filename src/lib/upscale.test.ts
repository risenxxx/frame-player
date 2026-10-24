/**
 * Which upscaler a file opens with, where a change is written, and what mpv is
 * sent. The failure worth guarding is the silent one: a season's choice that
 * one episode ignores, or a shader left running into the next film because the
 * option outlives `loadfile`.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';

const calls: string[][] = [];
vi.mock('tauri-plugin-libmpv-api', () => ({
  command: vi.fn(async (name: string, args: string[]) => {
    calls.push([name, ...args]);
  }),
  getProperty: vi.fn(async () => null),
  setProperty: vi.fn(async () => undefined),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => undefined) }));
vi.mock('@tauri-apps/api/path', () => ({ join: vi.fn(async (...p: string[]) => p.join('/')) }));

import { history, upscaleScopes } from './history.svelte';
import { player } from './player.svelte';
import { initUpscale, restoreUpscale, setUpscale, setUpscaleScope, upscale } from './upscale.svelte';

const A = '/Films/Show/e01.mkv';
const B = '/Films/Show/e02.mkv';
const OTHER = '/Films/Other/film.mkv';

/// Lets the sends, which await `join`, land.
const settle = () => new Promise((r) => setTimeout(r, 0));

async function open(path: string) {
  player.filePath = path;
  player.filename = path.split('/').pop() ?? null;
  restoreUpscale();
  await settle();
}

/// The shader list the last send left mpv with.
function shaders(): string[] {
  let list: string[] = [];
  for (const [, , op, value] of calls) list = op === 'clr' ? [] : [...list, value];
  return list;
}

beforeEach(async () => {
  localStorage.clear();
  history.prefs = { enabled: true, excluded: [] };
  player.filePath = null;
  player.filename = null;
  initUpscale('/res/shaders');
  calls.length = 0;
  await open(OTHER);
});

describe('the upscaler', () => {
  it('stays with the file by default', async () => {
    await open(A);
    expect(upscale.scope).toBe('file');
    setUpscale('video');
    await settle();
    expect(shaders()).toEqual(['/res/shaders/ArtCNN_C4F16.glsl']);
    await open(B);
    expect(upscale.mode).toBe('off');
    expect(shaders()).toEqual([]);
    await open(A);
    expect(upscale.mode).toBe('video');
  });

  it('covers a season once moved to the playlist', async () => {
    await open(A);
    setUpscale('anime');
    setUpscaleScope('folder');
    expect(upscaleScopes(A)).toMatchObject({ file: null, folder: 'anime' });
    await open(B);
    expect(upscale).toMatchObject({ mode: 'anime', scope: 'folder' });
    expect(shaders()).toEqual(['/res/shaders/Anime4K_Upscale_CNN_x2_M.glsl']);
    await open(OTHER);
    expect(upscale.mode).toBe('off');
  });

  it('lets one episode opt out of its playlist', async () => {
    await open(A);
    setUpscale('video');
    setUpscaleScope('folder');
    await open(B);
    setUpscaleScope('file');
    setUpscale('off');
    expect(upscaleScopes(B)).toMatchObject({ file: 'off', folder: 'video' });
    await open(A);
    expect(upscale.mode).toBe('video');
    await open(B);
    expect(upscale).toMatchObject({ mode: 'off', scope: 'file' });
  });

  it('replaces what the playlist held when moved there', async () => {
    await open(A);
    setUpscale('video');
    setUpscaleScope('folder');
    await open(B);
    setUpscaleScope('file');
    setUpscale('anime');
    setUpscaleScope('folder');
    expect(upscaleScopes(B)).toMatchObject({ file: null, folder: 'anime' });
    await open(A);
    expect(upscale.mode).toBe('anime');
  });

  it('sends nothing when the next file plays with the same mode', async () => {
    await open(A);
    setUpscale('video');
    setUpscaleScope('folder');
    await settle();
    calls.length = 0;
    await open(B);
    expect(calls).toEqual([]);
  });

  it('keeps nothing for a link’s missing playlist', async () => {
    const url = 'https://example.com/v.mp4';
    await open(url);
    expect(upscale.hasFolder).toBe(false);
    setUpscaleScope('folder');
    expect(upscale.scope).toBe('file');
  });
});
