import { describe, expect, it } from 'vitest';

import { isDirectMediaLink, linkFailure } from './link-failure';

describe('isDirectMediaLink', () => {
  it('knows a stream or a file by its path, whatever the query carries', () => {
    expect(isDirectMediaLink('https://cdn.example/hls/master.m3u8?token=abc&exp=1')).toBe(true);
    expect(isDirectMediaLink('https://cdn.example/manifest.mpd')).toBe(true);
    expect(isDirectMediaLink('https://cdn.example/film.MKV')).toBe(true);
    expect(isDirectMediaLink('https://cdn.example/song.flac#t=10')).toBe(true);
  });

  it('takes a page for a page, even one whose query mentions a stream', () => {
    expect(isDirectMediaLink('https://www.youtube.com/watch?v=abc')).toBe(false);
    expect(isDirectMediaLink('https://site.example/player?src=a.m3u8')).toBe(false);
    expect(isDirectMediaLink('not a url.m3u8')).toBe(false);
  });
});

describe('linkFailure', () => {
  it("lets the server's answer speak first, page or not", () => {
    expect(linkFailure('https://cdn.example/a.m3u8', 403)).toEqual({ kind: 'denied', status: 403 });
    expect(linkFailure('https://www.youtube.com/watch?v=abc', 401)).toEqual({ kind: 'denied', status: 401 });
    expect(linkFailure('https://cdn.example/a.m3u8', 404)).toEqual({ kind: 'gone', status: 404 });
    expect(linkFailure('https://cdn.example/a.m3u8', 410)).toEqual({ kind: 'gone', status: 410 });
    expect(linkFailure('https://cdn.example/a.m3u8', 502)).toEqual({ kind: 'http', status: 502 });
  });

  it('blames yt-dlp only for a page', () => {
    expect(linkFailure('https://cdn.example/a.m3u8', null).kind).toBe('stream');
    expect(linkFailure('https://www.youtube.com/watch?v=abc', null).kind).toBe('site');
  });
});
