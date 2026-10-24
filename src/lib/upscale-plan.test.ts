/**
 * When the upscaler runs, and what a file plays with. The ratio has to agree
 * with the shaders' own `//!WHEN` lines, which were measured against mpv
 * (upscale-plan.ts), or the panel says "active" over a picture nothing touched.
 */

import { describe, expect, it } from 'vitest';

import {
  fileRecord,
  nextUpscale,
  resolveUpscale,
  shaderFor,
  upscaleEngaged,
  upscaleRatio,
  type OsdBox,
} from './upscale-plan';

const box = (w: number, h: number, mx = 0, my = 0): OsdBox => ({ w, h, ml: mx, mr: mx, mt: my, mb: my });

describe('upscaleRatio', () => {
  it('measures the picture without its bars', () => {
    // 1080p film letterboxed into a 3456×2234 screen: 3456×1944, bars of 145.
    const r = upscaleRatio(box(3456, 2234, 0, 145), { w: 1920, h: 1080 }, 0);
    expect(r?.x).toBeCloseTo(1.8);
    expect(r?.y).toBeCloseTo(1.8);
  });

  it('clips a picture zoomed past the window to the window', () => {
    // Measured: 1280×720 at zoom 1 in 1600×900 does not engage — the window
    // part is 1600 wide, not 2560.
    const r = upscaleRatio(box(1600, 900, -480, -270), { w: 1280, h: 720 }, 0);
    expect(r).toEqual({ x: 1.25, y: 1.25 });
    expect(upscaleEngaged('video', r)).toBe(false);
  });

  it('measures a turned frame against the turned screen', () => {
    // Measured: 1280×720 turned by 90° in 1000×1800 engages.
    const r = upscaleRatio(box(1000, 1800, 0, 11), { w: 1280, h: 720 }, 90);
    expect(r?.x).toBeCloseTo(1000 / 720);
    expect(r?.y).toBeCloseTo(1778 / 1280);
    expect(upscaleEngaged('video', r)).toBe(true);
    expect(upscaleRatio(box(1000, 1800), { w: 1280, h: 720 }, 270)?.x).toBeCloseTo(1000 / 720);
  });

  it('needs both axes', () => {
    // Measured: a 1:1 override on 1280×720 in 1600×1600 does not engage.
    expect(upscaleEngaged('video', upscaleRatio(box(1600, 1600), { w: 1280, h: 720 }, 0))).toBe(false);
  });

  it('has nothing to say before a frame or a surface exists', () => {
    expect(upscaleRatio(box(0, 0), { w: 1920, h: 1080 }, 0)).toBeNull();
    expect(upscaleRatio(box(1920, 1080), { w: 0, h: 0 }, 0)).toBeNull();
    expect(upscaleEngaged('video', null)).toBe(false);
  });
});

describe('upscaleEngaged', () => {
  it('follows each shader’s own threshold', () => {
    // Measured: 1280×720 in 1600×900 (1.25) does not engage ArtCNN, 1760×990
    // (1.375) does; Anime4K asks for 1.2.
    expect(upscaleEngaged('video', { x: 1.25, y: 1.25 })).toBe(false);
    expect(upscaleEngaged('video', { x: 1.375, y: 1.375 })).toBe(true);
    expect(upscaleEngaged('anime', { x: 1.25, y: 1.25 })).toBe(true);
    expect(upscaleEngaged('strong', { x: 1.25, y: 1.25 })).toBe(true);
    expect(upscaleEngaged(null, { x: 3, y: 3 })).toBe(false);
  });
});

describe('scopes', () => {
  it('lets the file beat the playlist, and is off with neither', () => {
    expect(resolveUpscale('off', 'video')).toEqual({ mode: 'off', scope: 'file', stored: true });
    expect(resolveUpscale(null, 'anime')).toEqual({ mode: 'anime', scope: 'folder', stored: true });
    expect(resolveUpscale(null, null)).toEqual({ mode: 'off', scope: 'file', stored: false });
  });

  it('keeps a file’s "off" only where the playlist would turn it on', () => {
    expect(fileRecord('off', 'video')).toBe('off');
    expect(fileRecord('off', null)).toBeNull();
    expect(fileRecord('off', 'off')).toBeNull();
    expect(fileRecord('video', 'video')).toBe('video');
  });

  it('cycles through every mode back to off', () => {
    expect(['off', 'video', 'anime', 'strong'].map((m) => nextUpscale(m as never))).toEqual([
      'video',
      'anime',
      'strong',
      'off',
    ]);
  });
});

describe('shaderFor', () => {
  it('runs the generative shader only on sources up to 720p', () => {
    expect(shaderFor('strong', { w: 854, h: 480 })).toBe('strong');
    expect(shaderFor('strong', { w: 1280, h: 720 })).toBe('strong');
    // Phone video, upright: the long side is the height.
    expect(shaderFor('strong', { w: 720, h: 1280 })).toBe('strong');
    expect(shaderFor('strong', { w: 1920, h: 1080 })).toBe('video');
    expect(shaderFor('strong', { w: 1440, h: 720 })).toBe('video');
  });

  it('takes the mode as asked before the source is measured', () => {
    expect(shaderFor('strong', { w: 0, h: 0 })).toBe('strong');
  });

  it('leaves the other modes alone', () => {
    expect(shaderFor('off', { w: 854, h: 480 })).toBeNull();
    expect(shaderFor('video', { w: 3840, h: 2160 })).toBe('video');
    expect(shaderFor('anime', { w: 854, h: 480 })).toBe('anime');
  });
});
