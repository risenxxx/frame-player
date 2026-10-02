/**
 * Names for external tracks found in a release's subfolders. A wrong answer is
 * a plausible label on the wrong dub, and — because the remembered choice is
 * matched by title — the wrong dub chosen again in every following episode.
 */

import { describe, expect, it } from 'vitest';

import { externalTrackName, genericFolder, mpvFallbackTitle } from './external-title';

const VIDEO = '/media/Show/Show - 01.mkv';

describe('genericFolder', () => {
  it('reads a kind folder, with or without a language', () => {
    expect(genericFolder('Subs')).toBe('');
    expect(genericFolder('RUS Sound')).toBe('ru');
    expect(genericFolder('[ENG] Subtitles')).toBe('en');
    expect(genericFolder('rus_dub')).toBe('ru');
    expect(genericFolder('RUS')).toBe('ru');
  });

  it('leaves a real name alone', () => {
    expect(genericFolder('StudioA')).toBeNull();
    expect(genericFolder('Signs')).toBeNull();
    // A one-word name that happens to be a language code is not a tag.
    expect(genericFolder('It')).toBeNull();
  });
});

describe('mpvFallbackTitle', () => {
  it('is what is left after the video name', () => {
    expect(mpvFallbackTitle('/media/Show/Subs/Show - 01.rus.ass', VIDEO)).toBe('rus.ass');
    expect(mpvFallbackTitle('/media/Show/RUS Sound/A/Show - 01.mka', VIDEO)).toBe('mka');
  });
});

describe('externalTrackName', () => {
  it('names a dub after its studio folder and takes the language from the kind folder', () => {
    expect(externalTrackName('/media/Show/RUS Sound/StudioA/Show - 01.mka', VIDEO, 'mka', null)).toEqual({
      title: 'StudioA',
      lang: 'ru',
    });
  });

  it('keeps the language mpv read from the file name', () => {
    expect(
      externalTrackName('/media/Show/RUS Sound/StudioA/Show - 01.jpn.mka', VIDEO, 'jpn.mka', 'jpn'),
    ).toEqual({ title: 'StudioA', lang: 'jpn' });
  });

  it('falls back to the kind folder when nothing else names the file', () => {
    expect(externalTrackName('/media/Show/RUS Subs/Show - 01.ass', VIDEO, 'ass', null)).toEqual({
      title: 'RUS Subs',
      lang: 'ru',
    });
  });

  it('uses what follows the video name when it is more than a language', () => {
    expect(
      externalTrackName('/media/Show/Subs/Show - 01.Full.ass', VIDEO, 'Full.ass', null),
    ).toEqual({ title: 'Full', lang: null });
  });

  it('works on Windows paths', () => {
    expect(
      externalTrackName('C:\\media\\Show\\Subs\\GroupB\\Show - 01.ass', 'C:\\media\\Show\\Show - 01.mkv', 'ass', null),
    ).toEqual({ title: 'GroupB', lang: null });
  });

  it('leaves a file beside the video, and a title from the container, to mpv', () => {
    expect(externalTrackName('/media/Show/Show - 01.rus.ass', VIDEO, 'rus.ass', 'rus')).toBeNull();
    expect(
      externalTrackName('/media/Show/RUS Sound/A/Show - 01.mka', VIDEO, 'Studio A (5.1)', null),
    ).toBeNull();
  });
});
