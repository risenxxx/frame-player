import type { APIRoute } from 'astro'

/*
  One page, so one entry — written out rather than pulled from an integration:
  a sitemap with a single URL in it is not worth a dependency, and this way the
  address follows `site` in astro.config.ts wherever the build is pointed.
*/
export const GET: APIRoute = ({ site }) => {
  const url = new URL('/', site).href
  const body = `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>${url}</loc><changefreq>monthly</changefreq><priority>1.0</priority></url>
</urlset>
`
  return new Response(body, { headers: { 'Content-Type': 'application/xml; charset=utf-8' } })
}
