/**
 * Which picture adjustment a file opens with, and where a change is written.
 *
 * Both ways of being wrong here are silent: a correction for one dark film
 * following the viewer into every other one, or a choice of "all files" that
 * every file obeys except the one it was made on.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';

const sent: Record<string, number> = {};
vi.mock('tauri-plugin-libmpv-api', () => ({
  command: vi.fn(async () => undefined),
  getProperty: vi.fn(async () => null),
  setProperty: vi.fn(async (name: string, value: number) => {
    sent[name] = value;
  }),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => undefined) }));

import { adjustScopes, history } from './history.svelte';
import {
  adjust,
  resolveAdjustConflict,
  restoreAdjust,
  setAdjust,
  setAdjustScope,
  setGlobalAdjust,
} from './picture-adjust.svelte';
import { player } from './player.svelte';

const A = '/Films/Show/e01.mkv';
const B = '/Films/Show/e02.mkv';
const OTHER = '/Films/Other/film.mkv';

function open(path: string) {
  player.filePath = path;
  player.filename = path.split('/').pop() ?? null;
  restoreAdjust();
}

beforeEach(() => {
  // Writes are debounced, and one left over from the previous test would land
  // after the clear below. With no file, `restoreAdjust` only flushes.
  player.filePath = null;
  restoreAdjust();
  localStorage.clear();
  history.prefs = { enabled: true, excluded: [] };
  for (const k of Object.keys(sent)) delete sent[k];
  open(OTHER);
});

describe('picture adjustments', () => {
  it('stay with the file by default', () => {
    open(A);
    expect(adjust.scope).toBe('file');
    setAdjust('brightness', 12);
    open(B); // flushes the pending write for A
    expect(adjust.values.brightness).toBe(0);
    expect(sent.brightness).toBe(0);
    open(A);
    expect(adjust.values.brightness).toBe(12);
    expect(sent.brightness).toBe(12);
  });

  it('cover the playlist, and the file beats it', () => {
    open(A);
    setAdjust('gamma', 20);
    setAdjustScope('folder');
    open(B);
    expect(adjust.scope).toBe('folder');
    expect(adjust.values.gamma).toBe(20);
    open(OTHER);
    expect(adjust.values.gamma).toBe(0);
    open(B);
    setAdjustScope('file');
    setAdjust('gamma', 5);
    open(A);
    expect(adjust.values.gamma).toBe(20);
    open(B);
    expect(adjust.values.gamma).toBe(5);
  });

  it('widening to every file clears what would have overridden it here', () => {
    open(A);
    setAdjust('contrast', 10);
    setAdjustScope('folder');
    setAdjustScope('all');
    expect(adjustScopes(A)).toMatchObject({ file: null, folder: null });
    open(A);
    expect(adjust.scope).toBe('all');
    expect(adjust.values.contrast).toBe(10);
    open(OTHER);
    expect(adjust.values.contrast).toBe(10);
  });

  it('store zeros only where a broader scope would show through', () => {
    open(A);
    setAdjust('saturation', 8);
    setAdjust('saturation', 0);
    open(B);
    expect(adjustScopes(A).file).toBeNull();

    open(A);
    setAdjust('saturation', 8);
    setAdjustScope('all');
    open(B);
    setAdjustScope('file');
    setAdjust('saturation', 0);
    open(OTHER);
    expect(adjustScopes(B).file).toEqual({ brightness: 0, contrast: 0, saturation: 0, gamma: 0 });
    open(B);
    expect(adjust.values.saturation).toBe(0);
    open(A);
    expect(adjust.values.saturation).toBe(8);
  });

  it('a global change from the settings repaints only a file that follows it', () => {
    open(A);
    setAdjust('brightness', 12);
    setGlobalAdjust('brightness', 30);
    expect(adjust.values.brightness).toBe(12);
    expect(sent.brightness).toBe(12);
    open(B);
    expect(adjust.scope).toBe('all');
    expect(adjust.values.brightness).toBe(30);
    setGlobalAdjust('brightness', 20);
    expect(sent.brightness).toBe(20);
  });

  it('a file showing nothing starts following the global values', () => {
    open(A);
    expect(adjust.followsGlobal).toBe(true);
    setGlobalAdjust('gamma', 10);
    expect(adjust.scope).toBe('all');
    expect(sent.gamma).toBe(10);
  });

  describe('moving to a scope that already holds other values', () => {
    // The case as reported: tuned on its own, then a different global
    // correction set in the settings, then "all files" picked in the panel.
    function setUp() {
      open(A);
      setAdjust('brightness', 12);
      setGlobalAdjust('contrast', 25);
      setAdjustScope('all');
    }

    it('asks instead of overwriting', () => {
      setUp();
      expect(adjust.conflict?.scope).toBe('all');
      expect(adjust.conflict?.theirs.contrast).toBe(25);
      expect(adjust.scope).toBe('file');
      expect(adjust.global?.contrast).toBe(25);
    });

    it('"mine" makes this file\'s values everybody\'s', () => {
      setUp();
      resolveAdjustConflict('mine');
      expect(adjust.scope).toBe('all');
      expect(adjust.global).toEqual({ brightness: 12, contrast: 0, saturation: 0, gamma: 0 });
      open(OTHER);
      expect(adjust.values.brightness).toBe(12);
      expect(adjust.values.contrast).toBe(0);
    });

    it('"theirs" gives this file\'s own values up', () => {
      setUp();
      resolveAdjustConflict('theirs');
      expect(adjust.scope).toBe('all');
      expect(adjust.values).toEqual({ brightness: 0, contrast: 25, saturation: 0, gamma: 0 });
      expect(sent.contrast).toBe(25);
      expect(sent.brightness).toBe(0);
      expect(adjustScopes(A).file).toBeNull();
    });

    it('is not asked when the values already agree', () => {
      open(A);
      setAdjust('contrast', 25);
      setGlobalAdjust('contrast', 25);
      setAdjustScope('all');
      expect(adjust.conflict).toBeNull();
      expect(adjust.scope).toBe('all');
    });
  });

  it('a link has no playlist to apply to', () => {
    open('https://example.com/watch?v=1');
    expect(adjust.hasFolder).toBe(false);
    setAdjustScope('folder');
    expect(adjust.scope).toBe('file');
  });
});
