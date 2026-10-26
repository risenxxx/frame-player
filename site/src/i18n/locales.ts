/*
  The languages the site is written in, and which of them are out.

  English is the default and lives at the root (`/`, `/updates`,
  `/torrent-streaming`); every other language is the same address under its
  own prefix (`/ru`, `/ru/updates`). The slug stays English in every language,
  so one page is one path with a prefix in front of it, and the alternates of
  a page are arithmetic rather than a table somebody keeps.

  This module is pure — no Astro, no content collections — because the worker
  (worker/index.ts) imports it too, to know which language a request is for.
*/

export const LOCALES = ['en', 'ru', 'zh'] as const
export type Locale = (typeof LOCALES)[number]

export const DEFAULT_LOCALE: Locale = 'en'

/**
 * The languages a production build publishes. A language joins this list when
 * every string of it is written — the build refuses a published language with
 * a missing string (see `missing` in index.ts) rather than ship half a page in
 * English.
 */
export const PUBLISHED: readonly Locale[] = ['en', 'ru', 'zh']

export interface LocaleMeta {
  /** `<html lang>`, and what a block in another language is marked with. */
  htmlLang: string
  /** The `hreflang` of this language's addresses. Chinese is the simplified
      script, which is what `zh-Hans` says and `zh` alone does not. */
  hreflang: string
  /** `og:locale`. English keeps the bare `en` it has always carried. */
  ogLocale: string
  /** For dates and numbers. */
  intl: string
  /** The language's own name for itself, as the switch spells it. */
  name: string
  /** The switch's short label. */
  short: string
}

export const LOCALE_META: Record<Locale, LocaleMeta> = {
  en: { htmlLang: 'en', hreflang: 'en', ogLocale: 'en', intl: 'en-GB', name: 'English', short: 'EN' },
  ru: { htmlLang: 'ru', hreflang: 'ru', ogLocale: 'ru_RU', intl: 'ru-RU', name: 'Русский', short: 'RU' },
  zh: { htmlLang: 'zh-Hans', hreflang: 'zh-Hans', ogLocale: 'zh_CN', intl: 'zh-CN', name: '简体中文', short: '中文' },
}

const isLocale = (v: string): v is Locale => (LOCALES as readonly string[]).includes(v)

/**
 * The languages this build writes out. `SITE_LOCALES` (a comma list) overrides
 * the published set, which is how a language is worked on before it is out:
 * `SITE_LOCALES=en,ru,zh bun run dev`. A language without its strings falls
 * back to English there, and only there.
 */
export function enabledLocales(): Locale[] {
  const env = typeof process !== 'undefined' ? process.env.SITE_LOCALES : undefined
  const asked = env ? env.split(',').map((v) => v.trim()).filter(isLocale) : [...PUBLISHED]
  return LOCALES.filter((l) => l === DEFAULT_LOCALE || asked.includes(l))
}

/** True when the build carries more than one language — the switch, the
    hreflang links and the hint are drawn only then. */
export const multilingual = (): boolean => enabledLocales().length > 1

/**
 * The language of an address, from its first segment: `/ru`, `/ru.html`,
 * `/ru/updates`. Everything else is English, including a prefix this build
 * does not carry — the worker asks about addresses the build may not have.
 */
export function localeOf(pathname: string): Locale {
  const m = /^\/([a-z]{2})(?=$|[/.])/.exec(pathname)
  return m && isLocale(m[1]!) && m[1] !== DEFAULT_LOCALE ? m[1] : DEFAULT_LOCALE
}

/**
 * The English address a path stands for: the prefix, the `.html` the build
 * writes and a trailing `index` taken off. `/ru.html` and `/ru` are `/`;
 * `/ru/updates` is `/updates`.
 */
export function basePath(pathname: string): string {
  const lang = localeOf(pathname)
  let p = pathname.replace(/\.html$/, '').replace(/\/index$/, '')
  if (lang !== DEFAULT_LOCALE) p = p.slice(lang.length + 1)
  return p === '' ? '/' : p
}

/**
 * An English address in another language: `/` is `/ru`, `/updates` is
 * `/ru/updates`, and a fragment stays where it was (`/#torrents` is
 * `/ru#torrents`). English addresses come back unchanged.
 */
export function localePath(lang: Locale, path: string): string {
  if (lang === DEFAULT_LOCALE) return path
  const at = path.indexOf('#')
  const [p, hash] = at < 0 ? [path, ''] : [path.slice(0, at), path.slice(at)]
  return `/${lang}${p === '/' || p === '' ? '' : p}${hash}`
}
