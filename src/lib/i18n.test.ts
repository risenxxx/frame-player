/**
 * The three dictionaries against each other. The type system already proves
 * every key exists in each; what it cannot see is a placeholder lost in
 * translation, which no screen shows until the value it should have carried is
 * missing from a sentence — or a stray `{name}` is printed in its place.
 */

import { describe, expect, it } from 'vitest';

import { DICTS, joinList, setLocale } from './i18n.svelte';

const placeholders = (s: string) => (s.match(/\{\w+\}/g) ?? []).sort().join(',');

describe('dictionaries', () => {
  it('carry the same placeholders in every language', () => {
    const drift: string[] = [];
    for (const [key, source] of Object.entries(DICTS.ru)) {
      for (const lang of ['en', 'zh'] as const) {
        const text = DICTS[lang][key as keyof typeof DICTS.ru];
        if (placeholders(text) !== placeholders(source)) drift.push(`${lang} ${key}`);
      }
    }
    expect(drift).toEqual([]);
  });

  it('have no Cyrillic outside Russian', () => {
    // A string pasted into the wrong dictionary is invisible to the type check.
    const stray = (['en', 'zh'] as const).flatMap((lang) =>
      Object.entries(DICTS[lang])
        .filter(([, text]) => /[А-яЁё]/.test(text))
        .map(([key]) => `${lang} ${key}`),
    );
    expect(stray).toEqual([]);
  });
});

describe('joinList', () => {
  it('joins with the language’s own separator', () => {
    setLocale('zh');
    expect(joinList(['亮度 +10', '对比度 −5'])).toBe('亮度 +10、对比度 −5');
    setLocale('en');
    expect(joinList(['Drama', 'Crime'])).toBe('Drama, Crime');
  });
});
