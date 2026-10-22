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

import { airplayVerdict, castVerdict, deviceIsVideoCapable, deviceKind, type TvDevice } from './cast.svelte';

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

describe('deviceKind', () => {
  const base = { key: 'k', name: 'n', model: '', ip: '10.0.0.2' };
  const castHalf = (model: string, video: boolean | null) => ({
    id: 'c', name: 'n', model, ip: '10.0.0.2', port: 8009, video,
  });
  const dlnaHalf = (mimes: string[]) => ({
    id: 'd', name: 'n', model: '', ip: '10.0.0.2', control_url: '', rendering_url: null, mimes,
  });
  const tv = (d: Partial<TvDevice>): TvDevice => ({ ...base, ...d });

  it('calls a Cast receiver without video out a speaker', () => {
    const nest = tv({ cast: castHalf('Google Nest Mini', false) });
    expect(deviceIsVideoCapable(nest)).toBe(false);
    expect(deviceKind(nest)).toBe('speaker');
  });

  it('keeps a sweep-found receiver, which said nothing, a screen', () => {
    expect(deviceKind(tv({ cast: castHalf('', null) }))).toBe('tv');
  });

  it('knows a Chromecast or Google TV by its own name', () => {
    expect(deviceKind(tv({ cast: castHalf('Chromecast', true) }))).toBe('box');
    expect(deviceKind(tv({ cast: castHalf('Google TV Streamer', true) }))).toBe('box');
    expect(deviceKind(tv({ cast: castHalf('BRAVIA 4K VH2', true) }))).toBe('tv');
  });

  it('reads a renderer by what it accepts', () => {
    expect(deviceKind(tv({ dlna: dlnaHalf(['audio/mpeg', 'audio/flac']) }))).toBe('speaker');
    expect(deviceKind(tv({ dlna: dlnaHalf(['video/mp4']) }))).toBe('tv');
  });

  it('lets a renderer with video outvote a Cast half without it', () => {
    const both = tv({ cast: castHalf('', false), dlna: dlnaHalf(['video/mp4']) });
    expect(deviceKind(both)).toBe('tv');
  });
});
