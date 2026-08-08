/**
 * The content-language table, which is not the interface language and must not
 * become it. Two properties matter and neither is visible from the UI:
 *
 * The `alang`/`slang` line is **derived from mpv.conf, never stored beside it**,
 * so the round trip has to be lossless for a value someone typed by hand — an
 * entry silently dropped changes which track mpv selects, and nothing says so.
 *
 * And the ordering is a priority list: a spelling out of place means an English
 * track beating a Japanese one on a file tagged `jpn`.
 */

import { describe, expect, it } from 'vitest';

import { languageName, mpvLangValue, parseLangList, searchLanguages } from './languages';

describe('mpvLangValue', () => {
  it('keeps a language’s spellings adjacent', () => {
    // `--alang` is ordered: grouping by spelling instead would let an `eng`
    // track beat a `rus` one on a file that tags both.
    expect(mpvLangValue(['ru', 'en'])).toBe('ru,rus,en,eng');
  });

  it('passes an unknown tag through rather than dropping it', () => {
    expect(mpvLangValue(['ru', 'zzz'])).toBe('ru,rus,zzz');
  });

  it('writes nothing for an empty list', () => {
    expect(mpvLangValue([])).toBe('');
  });
});

describe('parseLangList', () => {
  it('reads back exactly what this dialog writes', () => {
    // The two values the old two-choice setting could produce. Nothing needed
    // migrating because of this.
    expect(parseLangList('rus,ru')).toEqual(['ru']);
    expect(parseLangList('eng,en')).toEqual(['en']);
  });

  it('round-trips through mpvLangValue', () => {
    for (const codes of [['ru'], ['ja', 'en'], ['ru', 'en', 'ja']]) {
      expect(parseLangList(mpvLangValue(codes))).toEqual(codes);
    }
  });

  it('collapses the bibliographic and terminological spellings', () => {
    // `ger`/`deu` and `fre`/`fra` are the same languages under ISO 639-2/B and
    // /T, and a viewer's own mpv.conf may use either.
    expect(parseLangList('ger,deu,fre')).toEqual(['de', 'fr']);
  });

  it('keeps an unknown tag under its own name', () => {
    // The line belongs to the viewer. Dropping an entry would change what mpv
    // selects, silently.
    expect(parseLangList('ru,zzz,en')).toEqual(['ru', 'zzz', 'en']);
  });

  it('survives the shapes a hand-written line takes', () => {
    expect(parseLangList(' RU , , rus ,en ')).toEqual(['ru', 'en']);
    expect(parseLangList(null)).toEqual([]);
    expect(parseLangList('')).toEqual([]);
  });
});

describe('searchLanguages', () => {
  it('puts the exact code first', () => {
    // Measured: typing `ru` used to match Romanian first, through its own `rum`,
    // which sorts ahead of Russian alphabetically.
    expect(searchLanguages('ru')[0].code).toBe('ru');
    expect(searchLanguages('jpn')[0].code).toBe('ja');
  });

  it('matches the endonym, which is how the list is read', () => {
    // Someone hunting for Polish subtitles is looking for "Polski".
    expect(searchLanguages('polski')[0].code).toBe('pl');
    expect(searchLanguages('рус')[0].code).toBe('ru');
  });

  it('excludes what is already chosen', () => {
    expect(searchLanguages('ru', ['ru']).some((l) => l.code === 'ru')).toBe(false);
  });

  it('returns the whole table for an empty query', () => {
    expect(searchLanguages('').length).toBeGreaterThan(20);
  });
});

describe('languageName', () => {
  it('names a language in itself', () => {
    // Which is also what keeps sixty languages out of the t() dictionary.
    expect(languageName('ru')).toBe('Русский');
    expect(languageName('ja')).toBe('日本語');
  });

  it('falls back to the code it was given', () => {
    expect(languageName('zzz')).toBe('zzz');
  });
});
