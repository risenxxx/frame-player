import { describe, expect, it } from 'vitest';

import { AUDIO_EXTENSIONS, isAudioPath, SUBTITLE_EXTENSIONS, VIDEO_EXTENSIONS } from './files';

describe('the extension lists', () => {
  it('are the file associations, so a format is never queued without being associated', () => {
    // The second half of issue #76: `.rmvb` was in the queue's list and not in
    // the association's.
    expect(VIDEO_EXTENSIONS).toContain('rmvb');
    expect(VIDEO_EXTENSIONS).toContain('mkv');
    expect(AUDIO_EXTENSIONS).toContain('m4b');
  });

  it('spell every extension the way a ProgId can carry it', () => {
    // shell_handler.rs names each Windows file type `FramePlayer.<ext>`, and a
    // ProgId takes letters, digits and periods: `dvr-ms` would not be one.
    for (const ext of [...VIDEO_EXTENSIONS, ...AUDIO_EXTENSIONS]) {
      expect(ext).toMatch(/^[a-z0-9]+$/);
    }
  });

  it('never name one extension twice, or as two kinds', () => {
    const all = [...VIDEO_EXTENSIONS, ...AUDIO_EXTENSIONS, ...SUBTITLE_EXTENSIONS];
    expect(new Set(all).size).toBe(all.length);
  });

  it('decide which files are audio', () => {
    expect(isAudioPath('/books/Chapter 1.m4b')).toBe(true);
    expect(isAudioPath('/films/Film.RMVB')).toBe(false);
  });
});
