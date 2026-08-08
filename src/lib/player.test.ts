/**
 * The two pure decisions in `player.svelte.ts` that go wrong quietly.
 *
 * `matchTrack` picks the dub for a whole season from a description, and a wrong
 * answer is a season watched in the wrong language by someone who assumes the
 * player remembered. `skipKind` decides whether to offer a skip button, and a
 * false positive hides one over the film itself — the failure the anchoring
 * exists to prevent, and the one that is easiest to reintroduce by "just adding
 * another word" to a pattern.
 *
 * Both are also the kind of code a refactor moves without reading.
 */

import { describe, expect, it } from 'vitest';

import { delayIsZero, matchTrack, roundDelay, skipKind, type Track } from './player.svelte';

function track(p: Partial<Track> & { id: number }): Track {
  return {
    selected: false,
    label: `track ${p.id}`,
    lang: null,
    title: null,
    codec: null,
    forced: false,
    external: false,
    ...p,
  } as Track;
}

const chapter = (title: string | null) => ({ index: 0, time: 0, title });

describe('matchTrack', () => {
  it('finds the same language at another position', () => {
    // The case the whole feature exists for: the Russian dub is #2 in episode 1
    // and #3 in episode 2, so an id would have picked the wrong track.
    const list = [track({ id: 1, lang: 'eng' }), track({ id: 2, lang: 'jpn' }), track({ id: 3, lang: 'rus' })];
    const found = matchTrack(list, { lang: 'ru', title: null, codec: null, forced: false, index: 1 });
    expect(found?.track.id).toBe(3);
  });

  it('treats the 639-1 and 639-2 spellings as one language', () => {
    // `jpn` and `ja` share one letter, so a prefix comparison would be wrong.
    const list = [track({ id: 1, lang: 'ja' })];
    expect(matchTrack(list, { lang: 'jpn', title: null, codec: null, forced: false, index: 0 })?.track.id).toBe(1);
    const list2 = [track({ id: 1, lang: 'jpn' })];
    expect(matchTrack(list2, { lang: 'ja', title: null, codec: null, forced: false, index: 0 })?.track.id).toBe(1);
  });

  it('matches by title when the release carries no languages', () => {
    // Real releases exist with titles ("Rus"/"Jap") and no language tags at all.
    const list = [track({ id: 1, title: 'Jap' }), track({ id: 2, title: 'Rus' })];
    const found = matchTrack(list, { lang: null, title: 'Rus', codec: null, forced: false, index: 0 });
    expect(found?.track.id).toBe(2);
  });

  it('never substitutes a forced track for a full one', () => {
    // A forced track carries the alien dialogue, not the script. The −50 has to
    // be enough to drop a language-only match below the floor.
    const list = [track({ id: 1, lang: 'rus', forced: true })];
    expect(matchTrack(list, { lang: 'ru', title: null, codec: null, forced: false, index: 0 })).toBeNull();
  });

  it('applies nothing at all when the episode has no such language', () => {
    // The right answer for an episode with no Russian dub is mpv's own choice,
    // not the nearest thing we can find.
    const list = [track({ id: 1, lang: 'eng' }), track({ id: 2, lang: 'jpn' })];
    expect(matchTrack(list, { lang: 'ru', title: null, codec: null, forced: false, index: 0 })).toBeNull();
  });

  it('refuses a codec-only match', () => {
    // 12 points is below the floor on purpose: guessing a dub from "both are
    // AC-3" is worse than leaving mpv alone.
    const list = [track({ id: 1, codec: 'ac3' })];
    expect(matchTrack(list, { lang: 'ru', title: null, codec: 'ac3', forced: false, index: 0 })).toBeNull();
  });

  it('uses position only to break a tie', () => {
    // Two identical Russian tracks: the one at the remembered index wins, and
    // the +5 must not be able to outrank a language match on its own.
    const list = [track({ id: 1, lang: 'rus' }), track({ id: 2, lang: 'rus' })];
    expect(matchTrack(list, { lang: 'ru', title: null, codec: null, forced: false, index: 1 })?.track.id).toBe(2);
    const other = [track({ id: 1, lang: 'eng' }), track({ id: 2, lang: 'rus' })];
    expect(matchTrack(other, { lang: 'ru', title: null, codec: null, forced: false, index: 0 })?.track.id).toBe(2);
  });

  it('reports a score the caller can compare', () => {
    // The restore keeps the score it acted on so a better candidate — an
    // external .srt arriving late — can still displace it.
    const weak = matchTrack([track({ id: 1, lang: 'rus' })], { lang: 'ru', title: null, codec: null, forced: false, index: 9 });
    const strong = matchTrack([track({ id: 1, lang: 'rus', title: 'Дубляж' })], { lang: 'ru', title: 'Дубляж', codec: null, forced: false, index: 9 });
    expect(strong!.score).toBeGreaterThan(weak!.score);
  });
});

describe('skipKind', () => {
  it('recognises a chapter that names itself', () => {
    expect(skipKind(chapter('Intro'))).toBe('intro');
    expect(skipKind(chapter('OP'))).toBe('intro');
    expect(skipKind(chapter('Заставка'))).toBe('intro');
    expect(skipKind(chapter('Ending'))).toBe('credits');
    expect(skipKind(chapter('Титры'))).toBe('credits');
    expect(skipKind(chapter('Recap'))).toBe('recap');
    expect(skipKind(chapter('Превью'))).toBe(null); // not a spelling we claim
    expect(skipKind(chapter('Анонс'))).toBe('preview');
  });

  it('does NOT match an ordinary sentence containing the word', () => {
    // The bug the anchoring exists for: these are ordinary English words, and a
    // substring search offers to skip the film.
    expect(skipKind(chapter('Ending the war for good'))).toBeNull();
    expect(skipKind(chapter('Opening the vault'))).toBeNull();
    expect(skipKind(chapter('A recap of the situation follows'))).toBeNull();
  });

  it('reads "opening credits" as an intro, not as credits', () => {
    // Not a test of the pattern order — the anchored alternatives are disjoint,
    // so reversing them changes nothing (see the note at the table). This pins
    // the answer itself, which is what matters.
    expect(skipKind(chapter('Opening Credits'))).toBe('intro');
  });

  it('tolerates the decorations rips actually use', () => {
    expect(skipKind(chapter('01. Intro'))).toBe('intro');
    expect(skipKind(chapter('  Intro…  '))).toBe('intro');
    expect(skipKind(chapter('1) Ending'))).toBe('credits');
  });

  it('caps the open-ended tails at the word budget', () => {
    expect(skipKind(chapter('Previously on The Show'))).toBe('recap');
    // Six short words. The character budget inside the pattern (`.{0,40}`) is
    // not what refuses this — a long title trips that one and would pass here
    // even with the word cap removed, which is how the first version of this
    // test managed to pin nothing.
    expect(skipKind(chapter('Previously on a b c'))).toBe('recap');
    expect(skipKind(chapter('Previously on a b c d'))).toBeNull();
  });

  it('says nothing about a chapter with no title', () => {
    // mpv names an untitled chapter itself; there is nothing to base a claim on.
    expect(skipKind(chapter(null))).toBeNull();
    expect(skipKind(chapter(''))).toBeNull();
  });

  it('is not fooled by "субтитры" containing "титры"', () => {
    expect(skipKind(chapter('Субтитры'))).toBeNull();
  });
});

describe('delay zero', () => {
  it('treats mpv’s floating-point residue as zero', () => {
    // Ten presses forward and ten back through mpv's `add`. Shown as 0.00, and
    // `=== 0` is false — which left the reset button enabled and wrote a record
    // that could never be cleared.
    let v = 0;
    for (let i = 0; i < 10; i++) v += 0.1;
    for (let i = 0; i < 10; i++) v -= 0.1;
    expect(v).not.toBe(0);
    expect(delayIsZero(v)).toBe(true);
    expect(roundDelay(v)).toBe(0);
  });

  it('keeps a delay the viewer can hear', () => {
    // 10 ms is below the readout's precision but above nothing: the threshold is
    // half the last digit shown, so this is where the line sits.
    expect(delayIsZero(0.1)).toBe(false);
    expect(delayIsZero(-0.1)).toBe(false);
    expect(roundDelay(0.30000000000000004)).toBe(0.3);
    expect(roundDelay(-1.2345)).toBe(-1.23);
  });
});
