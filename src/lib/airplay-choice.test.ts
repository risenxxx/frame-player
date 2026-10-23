/**
 * Which way a file goes over AirPlay, and when a receiver has refused the
 * better way. Every branch here is a session that either plays with its
 * tracks or sits on AirPlay's splash screen, so each is pinned.
 */

import { beforeEach, describe, expect, it } from 'vitest';

import {
  airplayMode,
  airplayRefusals,
  forgetRefusals,
  hlsRefused,
  hlsWorthIt,
  HLS_REFUSE_MS,
  pictureKind,
  rememberRefusal,
  setAirPlayMode,
  tracksPlan,
  videoRange,
  wantsHls,
} from './airplay-choice';
import type { Track } from './player.svelte';

const tr = (id: number, codec: string, extra: Partial<Track> = {}): Track => ({
  id,
  selected: false,
  label: `t${id}`,
  lang: null,
  title: null,
  codec,
  forced: false,
  external: false,
  path: null,
  channels: 2,
  ...extra,
});

beforeEach(() => localStorage.clear());

describe('pictureKind', () => {
  it('ranks HDR above resolution, and 1080p as ordinary', () => {
    expect(pictureKind(3840, 2160, 'pq')).toBe('hdr');
    expect(pictureKind(1920, 1080, 'hlg')).toBe('hdr');
    expect(pictureKind(3840, 2160, 'bt.1886')).toBe('uhd');
    expect(pictureKind(1920, 1080, 'srgb')).toBe('sd');
    expect(pictureKind(1920, 800, null)).toBe('sd');
  });

  it('declares a range only for an HDR transfer', () => {
    expect(videoRange('pq')).toBe('PQ');
    expect(videoRange('hlg')).toBe('HLG');
    expect(videoRange('bt.1886')).toBeNull();
  });
});

describe('hlsRefused', () => {
  it('is silence past the master, after the grace', () => {
    expect(hlsRefused(HLS_REFUSE_MS + 1, 1)).toBe(true);
    expect(hlsRefused(HLS_REFUSE_MS + 1, 0)).toBe(true);
    expect(hlsRefused(HLS_REFUSE_MS - 1, 1)).toBe(false);
    // A receiver that asked for a variant has taken the stream.
    expect(hlsRefused(60_000, 2)).toBe(false);
  });
});

describe('wantsHls', () => {
  const none = {};
  it('follows a forced mode whatever the file', () => {
    expect(wantsHls('original', true, none, null, 'sd')).toBe(false);
    expect(wantsHls('tracks', false, { LG: { hdr: true } }, 'LG', 'hdr')).toBe(true);
  });

  it('under auto, takes HLS only when it buys tracks or subtitles', () => {
    expect(wantsHls('auto', true, none, null, 'hdr')).toBe(true);
    expect(wantsHls('auto', false, none, null, 'sd')).toBe(false);
  });

  it('under auto, skips HLS for a kind this receiver refused, and only that kind', () => {
    const refusals = { LG: { hdr: true as const } };
    expect(wantsHls('auto', true, refusals, 'LG', 'hdr')).toBe(false);
    expect(wantsHls('auto', true, refusals, 'LG', 'sd')).toBe(true);
    expect(wantsHls('auto', true, refusals, 'Apple TV', 'hdr')).toBe(true);
    // Before a receiver is chosen nothing is known about it.
    expect(wantsHls('auto', true, refusals, null, 'hdr')).toBe(true);
  });
});

describe('the stored choices', () => {
  it('default to auto and survive a round trip', () => {
    expect(airplayMode()).toBe('auto');
    setAirPlayMode('original');
    expect(airplayMode()).toBe('original');
  });

  it('remember refusals per receiver and forget them on request', () => {
    rememberRefusal('LG', 'hdr');
    rememberRefusal('LG', 'uhd');
    rememberRefusal('Apple TV', 'sd');
    expect(airplayRefusals()).toEqual({ LG: { hdr: true, uhd: true }, 'Apple TV': { sd: true } });
    forgetRefusals('LG');
    expect(airplayRefusals()).toEqual({ 'Apple TV': { sd: true } });
  });

  it('read a damaged store as empty', () => {
    localStorage.setItem('frameplayer.airplay-refusals', '{not json');
    expect(airplayRefusals()).toEqual({});
  });
});

describe('tracksPlan', () => {
  it('sends every audio track, transcoding only what AVFoundation lacks', () => {
    const plan = tracksPlan(
      [tr(1, 'eac3', { channels: 6 }), tr(2, 'dts', { channels: 8 }), tr(3, 'aac', { external: true, path: '/r/dub.mka', lang: 'rus' })],
      [],
    );
    expect(plan.audio.map((a) => [a.file, a.index, a.transcode, a.channels])).toEqual([
      [null, 0, false, 6],
      [null, 1, true, 8],
      ['/r/dub.mka', 0, false, 2],
    ]);
    expect(plan.audioIds).toEqual([1, 2, 3]);
  });

  it("counts an image subtitle's stream but does not send it, and skips captions", () => {
    const plan = tracksPlan(
      [tr(1, 'aac')],
      [tr(1, 'eia_608'), tr(2, 'hdmv_pgs_subtitle'), tr(3, 'subrip'), tr(4, 'ass'), tr(5, 'subrip', { external: true, path: '/r/e.srt' })],
    );
    // Captions are no stream, PGS is s:0, so the SRT is s:1 and the ASS s:2.
    expect(plan.subs.map((x) => [x.file, x.index])).toEqual([
      [null, 1],
      [null, 2],
      ['/r/e.srt', 0],
    ]);
    expect(plan.subIds).toEqual([3, 4, 5]);
  });

  it('is worth HLS only with a choice to make', () => {
    expect(hlsWorthIt(tracksPlan([tr(1, 'aac')], []))).toBe(false);
    expect(hlsWorthIt(tracksPlan([tr(1, 'aac')], [tr(2, 'hdmv_pgs_subtitle')]))).toBe(false);
    expect(hlsWorthIt(tracksPlan([tr(1, 'aac')], [tr(2, 'subrip')]))).toBe(true);
    expect(hlsWorthIt(tracksPlan([tr(1, 'aac'), tr(2, 'aac')], []))).toBe(true);
  });
});
