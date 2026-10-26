/*
  The guide pages, by language.

  One collection holds every language: an English page is `torrent-streaming`,
  its translations `ru/torrent-streaming` and `zh/torrent-streaming` — the same
  path under the language's folder, which is also its address. Everything that
  lists pages (the footer, "Read next", the sitemap, llms.txt, the structured
  data) asks here for the list in one language, so a page that is not
  translated yet is listed in English rather than left out.

  Not importable by the worker: this module reads the content collection.
*/
import { getCollection, type CollectionEntry } from 'astro:content'
import { DEFAULT_LOCALE, LOCALES, enabledLocales, type Locale } from './locales.ts'

export type PageEntry = CollectionEntry<'pages'>

/** The language of a page, from the folder it is in. */
export function entryLang(entry: PageEntry): Locale {
  const first = entry.id.split('/')[0]!
  return (LOCALES as readonly string[]).includes(first) && first !== DEFAULT_LOCALE ? (first as Locale) : DEFAULT_LOCALE
}

/** The English page's id of any page: `ru/compare/vlc` → `compare/vlc`. */
export function baseId(entry: PageEntry): string {
  const lang = entryLang(entry)
  return lang === DEFAULT_LOCALE ? entry.id : entry.id.slice(lang.length + 1)
}

let all: Promise<PageEntry[]> | null = null

/** Every page this build writes: English, and the translations of the languages it carries. */
export function allPages(): Promise<PageEntry[]> {
  all ??= getCollection('pages').then((pages) => {
    const enabled = enabledLocales()
    const kept = pages.filter((p) => enabled.includes(entryLang(p)))
    check(pages)
    return kept
  })
  return all
}

/** The English pages, in their order. */
export async function englishPages(): Promise<PageEntry[]> {
  return (await allPages()).filter((p) => entryLang(p) === DEFAULT_LOCALE).sort((a, b) => a.data.order - b.data.order)
}

/** A page in a language, or the English page where it is not translated. */
export async function pageIn(lang: Locale, id: string): Promise<PageEntry | undefined> {
  const pages = await allPages()
  return pages.find((p) => p.id === `${lang}/${id}`) ?? pages.find((p) => p.id === id)
}

/** Every guide as a reader of `lang` sees it: in their language where it is written, in English where not. */
export async function pagesIn(lang: Locale): Promise<PageEntry[]> {
  const english = await englishPages()
  if (lang === DEFAULT_LOCALE) return english
  return Promise.all(english.map(async (p) => (await pageIn(lang, p.id))!))
}

/*
  What a translation owes the English page, checked once per build:

  - it has one: a translation whose English page is gone fails the build;
  - it says which English text it is (`translated`, the English `updated` it
    was made from), and when the English page has been revised since, the
    build says so — the translation still goes out, a page a few sentences
    behind is better than none, but nobody is left to find out by reading;
  - its links stay in its language: a link to a page that is translated must
    be the translation's address, and a link under the prefix must lead to a
    translation that exists.
*/
function check(pages: PageEntry[]): void {
  const ids = new Set(pages.map((p) => p.id))
  const english = new Map(pages.filter((p) => entryLang(p) === DEFAULT_LOCALE).map((p) => [p.id, p]))
  const errors: string[] = []
  const stale: string[] = []

  for (const page of pages) {
    const lang = entryLang(page)
    if (lang === DEFAULT_LOCALE) continue
    const source = english.get(baseId(page))
    if (!source) {
      errors.push(`${page.id}: no English page "${baseId(page)}" to be a translation of`)
      continue
    }
    if (!page.data.translated) {
      errors.push(`${page.id}: \`translated\` is missing — the English page's \`updated\` date this translation was made from`)
    } else if (source.data.updated > page.data.translated) {
      stale.push(`${page.id}: English updated ${source.data.updated.toISOString().slice(0, 10)}, translated from ${page.data.translated.toISOString().slice(0, 10)}`)
    }
    for (const id of page.data.related) {
      if (!english.has(id)) errors.push(`${page.id}: "related" names ${id}, which is not a page`)
    }

    const links = [...(page.body ?? '').matchAll(/\]\((\/[^)#\s]*)|href="(\/[^"#]*)/g)].map((m) => m[1] ?? m[2]!)
    const prefix = `/${lang}`
    /* Whether an English address exists in this language: the home page and
       the updates do in every one, a guide when it is translated. */
    const here = (path: string): boolean => path === '/' || path === '/updates' || ids.has(`${lang}${path}`)
    for (const raw of links) {
      const link = raw === '' ? '/' : raw
      if (/\.[a-z0-9]+$/i.test(link)) continue
      if (link === prefix || link.startsWith(`${prefix}/`)) {
        const path = link === prefix ? '/' : link.slice(prefix.length)
        if (!here(path)) errors.push(`${page.id}: links to ${link}, which is not translated — link to ${path}`)
      } else if (here(link)) {
        errors.push(`${page.id}: links to ${link}, which is in this language too — link to ${link === '/' ? prefix : prefix + link}`)
      }
    }
  }

  if (stale.length > 0) console.warn(`[i18n] translations behind their English page:\n  ${stale.join('\n  ')}`)
  if (errors.length > 0) throw new Error(`Translations:\n  ${errors.join('\n  ')}`)
}
