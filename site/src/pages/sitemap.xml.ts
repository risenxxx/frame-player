import type { APIRoute } from 'astro'
import { publishedReleases } from '../releases.ts'
import { LOCALE_META, alternatesOf, multilingual, type Locale } from '../i18n/index.ts'
import { englishPages, pageIn } from '../i18n/pages.ts'

/*
  The home page and every guide, written out rather than pulled from an
  integration: a handful of URLs is not worth a dependency, and this way the
  addresses follow `site` in astro.config.ts wherever the build is pointed.
  A guide's `lastmod` is its `updated` date — the day it was last checked
  against the player, which is the date a crawler should care about.

  A page written in more than one language is listed once per language, each
  entry naming all of them (`xhtml:link`) — the sitemap's form of the hreflang
  links in the page's head, which a crawler reads without fetching the page.
*/
export const GET: APIRoute = async ({ site }) => {
  /* By the English page: `url` writes each of its languages. */
  const pages = await englishPages()
  // `/updates` changes when a release goes out, so that is its date.
  const [latest] = await publishedReleases()
  const at = (path: string): string => new URL(path, site).href
  /* Every language of a page, the English one as the default as well. */
  const links = (path: string): string => {
    const alts = Object.entries(alternatesOf(path)) as [Locale, string][]
    if (alts.length < 2) return ''
    const en = alts.find(([l]) => l === 'en')?.[1] ?? path
    return [
      ...alts.map(([l, p]) => `<xhtml:link rel="alternate" hreflang="${LOCALE_META[l].hreflang}" href="${at(p)}"/>`),
      `<xhtml:link rel="alternate" hreflang="x-default" href="${at(en)}"/>`,
    ].join('')
  }
  /* `extra` is per language: a translation has an `updated` date of its own. */
  const url = (path: string, extra: (lang: Locale) => string = () => ''): string =>
    (Object.entries(alternatesOf(path)) as [Locale, string][])
      .map(([l, p]) => `  <url><loc>${at(p)}</loc>${extra(l)}${links(path)}</url>`)
      .join('\n')
  const lastmod = (d: Date): string => `<lastmod>${d.toISOString().slice(0, 10)}</lastmod>`
  const xhtml = multilingual() ? ' xmlns:xhtml="http://www.w3.org/1999/xhtml"' : ''
  const body = `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"${xhtml}>
${[
  url('/'),
  url('/updates', () => (latest ? lastmod(latest.data.date) : '')),
  ...(await Promise.all(
    pages.map(async (p) => {
      const dates = new Map<Locale, Date>()
      for (const l of Object.keys(alternatesOf(`/${p.id}`)) as Locale[]) dates.set(l, (await pageIn(l, p.id))!.data.updated)
      return url(`/${p.id}`, (l) => lastmod(dates.get(l) ?? p.data.updated))
    }),
  )),
].join('\n')}
</urlset>
`
  return new Response(body, { headers: { 'Content-Type': 'application/xml; charset=utf-8' } })
}
