/**
 * The subtitle button's tag with two subtitles on screen: the pair is the
 * state worth seeing from the bar, and the second one alone is still "on".
 */

import { describe, expect, it } from 'vitest';

import type { Track } from './player.svelte';
import { subBadge } from './track-badge';

function track(p: Partial<Track> & { id: number }): Track {
  return {
    selected: false,
    secondary: false,
    label: `track ${p.id}`,
    lang: null,
    title: null,
    codec: null,
    forced: false,
    external: false,
    path: null,
    channels: null,
    ...p,
  };
}

describe('subBadge', () => {
  it('counts the tracks while nothing is on', () => {
    expect(subBadge([track({ id: 1, lang: 'rus' }), track({ id: 2, lang: 'eng' })])).toEqual({
      text: '2',
      on: false,
    });
  });

  it('names the one track that is on', () => {
    const list = [track({ id: 1, lang: 'rus', selected: true }), track({ id: 2, lang: 'eng' })];
    expect(subBadge(list)).toEqual({ text: 'RU', on: true });
  });

  it('joins the main and the second subtitle, main first', () => {
    const list = [
      track({ id: 1, lang: 'eng', secondary: true }),
      track({ id: 2, lang: 'rus', selected: true }),
    ];
    expect(subBadge(list)).toEqual({ text: 'RU+EN', on: true });
  });

  it('shows the second subtitle alone while the main one is off', () => {
    const list = [track({ id: 1, lang: 'rus' }), track({ id: 2, lang: 'jpn', secondary: true })];
    expect(subBadge(list)).toEqual({ text: 'JA', on: true });
  });

  it('falls back to the place in the list for a track with no language', () => {
    const list = [track({ id: 1, lang: 'rus', selected: true }), track({ id: 2, secondary: true })];
    expect(subBadge(list)).toEqual({ text: 'RU+2', on: true });
  });
});
