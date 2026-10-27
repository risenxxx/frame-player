/**
 * UI localisation. Russian, English and Simplified Chinese, chosen once and
 * stored in localStorage.
 *
 * `t()` reads reactive state, so every call site re-renders when the language
 * changes — including plain markup, without any per-component subscription.
 * Keys are flat and namespaced by dot. The dictionaries are one module per
 * language under `i18n/` — plain TypeScript rather than JSON, because `ru.ts`
 * is the source of truth for the key set and the other two are held to it by
 * `satisfies`, so a missing English or Chinese string is a type error rather
 * than a silent gap. The Chinese is translated from the English, against the
 * glossary in docs/zh-glossary.md — read it before adding a key.
 *
 * Platform-dependent wording (Explorer vs Finder, Windows HDR vs macOS EDR)
 * gets separate keys instead of branching inside a string: the two sentences
 * differ by more than one word in at least one language.
 */

import { en } from './i18n/en';
import { ru, type MessageKey } from './i18n/ru';
import { zh } from './i18n/zh';

export type { MessageKey };

export type Locale = 'ru' | 'en' | 'zh';

/** Exported for the tests, which check the three against each other. */
export const DICTS: Record<Locale, Record<MessageKey, string>> = { ru, en, zh };

/// What `<html lang>` says. `zh` alone leaves the engine to guess the script,
/// and Han characters shared with Japanese are then drawn in Japanese forms.
const HTML_LANG: Record<Locale, string> = { ru: 'ru', en: 'en', zh: 'zh-CN' };

const STORAGE_KEY = 'frameplayer.locale';

function detectLocale(): Locale {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === 'ru' || saved === 'en' || saved === 'zh') return saved;
  } catch {
    // localStorage unavailable — fall through to the system language
  }
  const system = navigator.language ?? '';
  if (/^ru\b/i.test(system)) return 'ru';
  // Simplified only: a Traditional-script system (Taiwan, Hong Kong, Macau)
  // gets English, which it chose a language over, rather than the other script.
  if (/^zh\b/i.test(system) && !/-(tw|hk|mo|hant)\b/i.test(system)) return 'zh';
  return 'en';
}

class I18nState {
  current = $state<Locale>(detectLocale());
}

const state = new I18nState();

export function locale(): Locale {
  return state.current;
}

export function setLocale(next: Locale) {
  if (state.current === next) return;
  state.current = next;
  try {
    localStorage.setItem(STORAGE_KEY, next);
  } catch {
    // not critical: the choice simply will not survive a restart
  }
  document.documentElement.lang = HTML_LANG[next];
}

/** Translate a key, substituting `{name}` placeholders from `params`. */
export function t(key: MessageKey, params?: Record<string, string | number>): string {
  const text = DICTS[state.current][key];
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in params ? String(params[name]) : whole,
  );
}

/**
 * A list as this language writes one: `a, b, c`, or `a、b、c` in Chinese.
 * For lists built at runtime (genres, dubs, changed settings); a fixed list
 * belongs inside the string.
 */
export function joinList(items: readonly string[]): string {
  return items.join(t('list.sep'));
}

document.documentElement.lang = HTML_LANG[state.current];
