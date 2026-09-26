/**
 * Content negotiation in front of the static site: a request that asks for
 * `text/markdown` gets Markdown, everything else gets the page.
 *
 * This is Cloudflare's "Markdown for Agents" without Cloudflare's plan for it.
 * The Markdown is not converted at the edge — it is built with the site
 * (`src/pages/[...slug].md.ts`), so what an agent reads is the page's own
 * source rather than a rendering of its markup, and `/llms.txt` lists it.
 *
 * Everything else is served by the assets binding exactly as before. Only the
 * document routes run this worker at all (`run_worker_first` in
 * wrangler.jsonc); `/_astro/*`, `/gen/*` and the rest never leave the CDN.
 */
interface Env {
  ASSETS: { fetch: (request: Request | string | URL) => Promise<Response> }
}

/**
 * Does this request want Markdown more than it wants a page?
 *
 * The header is a list with quality values, so the answer is a comparison and
 * not a substring search: a browser sends `text/html,…;q=0.9,*\/*;q=0.8` and
 * must never be handed Markdown, while an agent sending `text/markdown` — or
 * `text/markdown, text/html;q=0.5` — must be.
 */
function wantsMarkdown(accept: string): boolean {
  let markdown = 0
  let html = 0
  for (const part of accept.split(',')) {
    const [type = '', ...params] = part.trim().split(';')
    const q = Number(params.find((p) => p.trim().startsWith('q='))?.split('=')[1] ?? '1')
    const quality = Number.isFinite(q) ? q : 0
    const name = type.trim().toLowerCase()
    if (name === 'text/markdown') markdown = Math.max(markdown, quality)
    if (name === 'text/html' || name === 'application/xhtml+xml') html = Math.max(html, quality)
  }
  return markdown > 0 && markdown >= html
}

/** The Markdown twin of a document address, or null for a path that has none. */
function twinOf(pathname: string): string | null {
  if (pathname === '/') return '/llms.txt'
  if (/\.[a-z0-9]+$/i.test(pathname)) return null
  return `${pathname.replace(/\/+$/, '')}.md`
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url)

    if ((request.method === 'GET' || request.method === 'HEAD') && wantsMarkdown(request.headers.get('accept') ?? '')) {
      const twin = twinOf(url.pathname)
      if (twin) {
        const markdown = await env.ASSETS.fetch(new URL(twin, url))
        // A page with no twin falls through to the page itself rather than 404.
        if (markdown.ok) {
          const headers = new Headers(markdown.headers)
          headers.set('content-type', 'text/markdown; charset=utf-8')
          headers.set('vary', 'Accept')
          // The page is the canonical address; this is one representation of it.
          headers.set('link', `<${url.origin}${url.pathname}>; rel="canonical"`)
          return new Response(markdown.body, { status: 200, headers })
        }
      }
    }

    const response = await env.ASSETS.fetch(request)
    /* The same address now answers with two kinds of thing, so every cache
       between here and the reader has to key on what was asked for. */
    const headers = new Headers(response.headers)
    const vary = headers.get('vary')
    headers.set('vary', vary && !/\baccept\b/i.test(vary) ? `${vary}, Accept` : 'Accept')
    return new Response(response.body, { status: response.status, statusText: response.statusText, headers })
  },
}
