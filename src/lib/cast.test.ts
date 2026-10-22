/**
 * AirPlay's ladder is the Cast one with AVFoundation's edits, and every edit is
 * a way for the wrong answer to be silent: WebM offered as playable is a black
 * screen, an HEVC MP4 sent as it is plays only if its tag happens to be `hvc1`.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';

let props: Record<string, string> = {};

vi.mock('tauri-plugin-libmpv-api', () => ({
  command: vi.fn(async () => undefined),
  getProperty: vi.fn(async (name: string) => props[name] ?? null),
  setProperty: vi.fn(async () => undefined),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => undefined) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));

import { airplayVerdict, castVerdict } from './cast.svelte';

function file(video: string, audio: string) {
  props = { 'video-format': video, 'audio-codec-name': audio };
}

beforeEach(() => {
  props = {};
});

describe('airplayVerdict', () => {
  it('plays an H.264/AAC MP4 as it is', async () => {
    file('h264', 'aac');
    expect(await airplayVerdict('/f/a.mp4')).toEqual({ kind: 'direct', transcodeAudio: false });
    expect(await airplayVerdict('/f/a.MOV')).toEqual({ kind: 'direct', transcodeAudio: false });
  });

  it('always prepares HEVC, even in an MP4, for the hvc1 tag', async () => {
    file('hevc', 'eac3');
    expect(await airplayVerdict('/f/a.mp4')).toEqual({ kind: 'prepare', transcodeAudio: false });
  });

  it('refuses VP9 where the Cast ladder takes it', async () => {
    file('vp9', 'opus');
    expect((await airplayVerdict('/f/a.webm')).kind).toBe('refuse');
    expect((await castVerdict('/f/a.webm')).kind).toBe('direct');
  });

  it('transcodes audio AVFoundation lacks, keeps FLAC', async () => {
    file('h264', 'dts');
    expect(await airplayVerdict('/f/a.mkv')).toEqual({ kind: 'prepare', transcodeAudio: true });
    file('h264', 'opus');
    expect(await airplayVerdict('/f/a.mp4')).toEqual({ kind: 'prepare', transcodeAudio: true });
    file('h264', 'flac');
    expect(await airplayVerdict('/f/a.mp4')).toEqual({ kind: 'direct', transcodeAudio: false });
  });

  it('repacks MKV and refuses what nothing can repack', async () => {
    file('h264', 'aac');
    expect(await airplayVerdict('/f/a.mkv')).toEqual({ kind: 'prepare', transcodeAudio: false });
    expect((await airplayVerdict('/f/a.rmvb')).kind).toBe('refuse');
  });

  it('takes the extension from the basename, not a dotted folder', async () => {
    file('h264', 'aac');
    expect((await airplayVerdict('/f/Season.1/episode')).kind).toBe('refuse');
  });
});

