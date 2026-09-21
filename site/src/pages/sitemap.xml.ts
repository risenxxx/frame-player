import type { APIRoute } from 'astro'
import { getCollection } from 'astro:content'

/*
  The home page and every guide, written out rather than pulled from an
  integration: a handful of URLs is not worth a dependency, and this way the
  addresses follow `site` in astro.config.ts wherever the build is pointed.
  A guide's `lastmod` is its `updated` date — the day it was last checked
  against the player, which is the date a crawler should care about.
*/
export const GET: APIRoute = async ({ site }) => {
  const pages = (await getCollection('pages')).sort((a, b) => a.data.order - b.data.order)
  const url = (path: string, extra = '') =>
    `  <url><loc>${new URL(path, site).href}</loc>${extra}</url>`
  const body = `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
${[
  url('/'),
  ...pages.map((p) => url(`/${p.id}`, `<lastmod>${p.data.updated.toISOString().slice(0, 10)}</lastmod>`)),
].join('\n')}
</urlset>
`
  return new Response(body, { headers: { 'Content-Type': 'application/xml; charset=utf-8' } })
}
