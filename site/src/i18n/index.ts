/*
  The site's strings, and the one way a template reaches them:

    const { lang, s, href } = useLocale(Astro)

  `s` is the dictionary of the page's language (from its address), `href` turns
  an English address into this language's one when the page exists here and
  leaves it English when it does not. Text goes out through `<T s={…} />`
  (T.astro), which renders the markup en.ts describes.
*/
import { en, type Strings } from './en.ts'
import { ru } from './ru.ts'
import { zh } from './zh.ts'
import {
  DEFAULT_LOCALE,
  LOCALES,
  LOCALE_META,
  PUBLISHED,
  basePath,
  enabledLocales,
  localeOf,
  localePath,
  type Locale,
} from './locales.ts'

export * from './locales.ts'
export type { Strings }

/** A translation: every key optional while it is being written. A list is
    whole or absent — its items are matched to icons and links by position. */
type DeepPartial<T> = T extends readonly unknown[] ? T : T extends object ? { [K in keyof T]?: DeepPartial<T[K]> } : T
export type Translation = DeepPartial<Strings>

const DICTIONARIES: Record<Locale, Translation> = { en, ru, zh }

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)

const params = (s: string): string => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(',')

/**
 * What a translation lacks against English, as dotted paths: a key that is
 * not there, a list of another length, or a string whose `{placeholders}`
 * differ — a dropped `{n}` prints nothing where the number should be.
 */
export function missing(lang: Locale): string[] {
  const out: string[] = []
  const walk = (ref: unknown, got: unknown, path: string): void => {
    if (got === undefined) return void out.push(path)
    if (Array.isArray(ref)) {
      if (!Array.isArray(got) || got.length !== ref.length) return void out.push(`${path} (a list of ${ref.length})`)
      ref.forEach((r, i) => walk(r, got[i], `${path}[${i}]`))
    } else if (isObject(ref)) {
      if (!isObject(got)) return void out.push(path)
      for (const k of Object.keys(ref)) walk(ref[k], got[k], path ? `${path}.${k}` : k)
    } else if (typeof ref === 'string' && (typeof got !== 'string' || params(ref) !== params(got))) {
      out.push(`${path} (placeholders: ${params(ref) || 'none'})`)
    }
  }
  walk(en, DICTIONARIES[lang], '')
  return out
}

/* English wherever the translation is one of the things `missing` lists: a
   list of another length would leave an icon without its words, a string
   without its `{n}` would print nothing where the number goes. */
function merge<T>(base: T, over: unknown): T {
  if (over === undefined) return base
  if (Array.isArray(base)) {
    return (Array.isArray(over) && over.length === base.length ? base.map((b, i) => merge(b, over[i])) : base) as T
  }
  if (isObject(base)) {
    if (!isObject(over)) return base
    const out: Record<string, unknown> = { ...base }
    for (const k of Object.keys(base)) out[k] = merge(base[k], over[k])
    return out as T
  }
  if (typeof base === 'string') return (typeof over === 'string' && params(base) === params(over) ? over : base) as T
  return over as T
}

const cache = new Map<Locale, Strings>()
const warned = new Set<Locale>()

/**
 * The dictionary of a language. A published language must be complete — the
 * build stops on the first page that asks for it otherwise. One that is only
 * being worked on (`SITE_LOCALES`) borrows English for what it lacks.
 */
export function strings(lang: Locale): Strings {
  const hit = cache.get(lang)
  if (hit) return hit
  if (lang !== DEFAULT_LOCALE) {
    const gaps = missing(lang)
    if (gaps.length > 0 && PUBLISHED.includes(lang)) {
      throw new Error(`${lang} is published but ${gaps.length} strings are missing or wrong:\n  ${gaps.join('\n  ')}`)
    }
    if (gaps.length > 0 && !warned.has(lang)) {
      warned.add(lang)
      console.warn(`[i18n] ${lang}: ${gaps.length} strings fall back to English`)
    }
  }
  const s = lang === DEFAULT_LOCALE ? en : merge(en, DICTIONARIES[lang])
  cache.set(lang, s)
  return s
}

/* ── text ───────────────────────────────────────────────────────────────── */

/** A value that is already markup, for the rare placeholder that is a link or a glyph. */
export class Html {
  constructor(readonly html: string) {}
}
export const html = (markup: string): Html => new Html(markup)

export type Params = Record<string, string | number | Html>

const escape = (s: string): string => s.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;')

const SEP = '<span class="sep"></span>'

/**
 * A string of en.ts as markup: escaped first, then its own few marks turned
 * into tags, then the placeholders filled — so a value can never be read as
 * markup, and a placeholder this call does not fill stays as it was written
 * (a script fills it later, see `data-say` in download.ts).
 */
export function rich(text: string, values: Params = {}): string {
  return escape(text)
    .replace(/\*\*(.+?)\*\*/g, '<b>$1</b>')
    .replace(/\*(.+?)\*/g, '<i>$1</i>')
    .replace(/`(.+?)`/g, '<span class="mono">$1</span>')
    .replace(/\{(\w+)\}/g, (whole, key: string) => {
      if (key === 'sep') return SEP
      const v = values[key]
      if (v === undefined) return whole
      return v instanceof Html ? v.html : escape(String(v))
    })
}

/** A string of en.ts as plain text — for an attribute, a title, structured data. */
export function fill(text: string, values: Record<string, string | number> = {}): string {
  return text.replace(/\{(\w+)\}/g, (whole, key: string) => (values[key] === undefined ? whole : String(values[key])))
}

/* ── addresses ──────────────────────────────────────────────────────────── */

/**
 * The pages every language carries, by their English address. The guides join
 * page by page as they are translated.
 */
const EVERY_LANGUAGE = new Set(['/', '/updates'])

/*
  Which guides are translated, known without loading one: a translation is the
  English page's path under its language's folder — `ru/torrent-streaming.mdx`
  beside `torrent-streaming.mdx` — and the glob's keys are those paths.
  Synchronous, so a link anywhere on any page can ask.
*/
function pageFiles(): string[] {
  try {
    return Object.keys(import.meta.glob('../content/pages/**/*.mdx'))
  } catch {
    // Outside Vite (a script run with bun) there is no glob, and no guides.
    return []
  }
}
const TRANSLATED = new Set(
  pageFiles()
    .map((file) => file.replace('../content/pages/', '').replace(/\.mdx$/, ''))
    .filter((id) => LOCALES.some((l) => l !== DEFAULT_LOCALE && id.startsWith(`${l}/`))),
)

/** The languages a page exists in, by its English address. */
export function pageLocales(path: string): Locale[] {
  const enabled = enabledLocales()
  if (EVERY_LANGUAGE.has(path)) return enabled
  return enabled.filter((l) => l === DEFAULT_LOCALE || TRANSLATED.has(`${l}${path}`))
}

/**
 * The same page in every language it exists in, by address — for the switch,
 * the hreflang links and the sitemap. A page in one language has no alternates
 * worth printing, and gets back a single entry.
 */
export function alternatesOf(pathname: string): Partial<Record<Locale, string>> {
  const base = basePath(pathname)
  const out: Partial<Record<Locale, string>> = {}
  for (const l of pageLocales(base)) out[l] = localePath(l, base)
  return out
}

/** The site's own address for a link written in English, in the language `lang`. */
export function linkFor(lang: Locale, path: string): string {
  if (lang === DEFAULT_LOCALE || !path.startsWith('/')) return path
  const at = path.indexOf('#')
  const base = at < 0 ? path : path.slice(0, at) || '/'
  return pageLocales(base).includes(lang) ? localePath(lang, path) : path
}

/** Everything a template needs to speak the language of the page it is on. */
export function useLocale(astro: { url: URL }) {
  const lang = localeOf(astro.url.pathname)
  return {
    lang,
    s: strings(lang),
    meta: LOCALE_META[lang],
    /** Whether this page is the home page, in whichever language. */
    home: basePath(astro.url.pathname) === '/',
    href: (path: string) => linkFor(lang, path),
  }
}

/* Russian dates come out of Intl as "25 октября 2026 г."; the "г." is a
   formality a list of release dates does not need. */
const bare = (lang: Locale, s: string): string => (lang === 'ru' ? s.replace(/\s*г\.$/, '') : s)

/** Dates, in the page's language. English keeps the forms it always printed. */
export function longDate(lang: Locale, d: Date): string {
  return bare(lang, d.toLocaleDateString(LOCALE_META[lang].intl, { day: 'numeric', month: 'long', year: 'numeric', timeZone: 'UTC' }))
}

/* Assembled for English rather than `en-GB`'s short form, which spells
   September "Sept". */
export function shortDate(lang: Locale, d: Date): string {
  if (lang === 'en') {
    return `${d.getUTCDate()} ${d.toLocaleDateString('en-US', { month: 'short', timeZone: 'UTC' })} ${d.getUTCFullYear()}`
  }
  return bare(lang, d.toLocaleDateString(LOCALE_META[lang].intl, { day: 'numeric', month: 'short', year: 'numeric', timeZone: 'UTC' }))
}
