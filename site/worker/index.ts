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
 *
 * Its second job is `/s/*`, the page counter's own address (src/analytics.ts),
 * which it forwards to Rybbit — see {@link counter}.
 */
import { ANALYTICS_PREFIX, RYBBIT_UPSTREAM } from '../src/analytics.ts'

interface Env {
  ASSETS: { fetch: (request: Request | string | URL) => Promise<Response> }
}

/** Workers add `cf` to `fetch`; the DOM types do not know it. */
type EdgeFetch = (
  input: string,
  init: RequestInit & { cf?: { cacheEverything: boolean; cacheTtl: number } },
) => Promise<Response>

/** Connection headers, which by definition are never forwarded. */
const HOP_BY_HOP = ['connection', 'keep-alive', 'proxy-authenticate', 'proxy-authorization', 'te', 'trailer', 'transfer-encoding', 'upgrade']

/**
 * What may be forwarded, and for how long the edge keeps the answer (Rybbit's
 * own guidance: the script an hour, the site's config five minutes, events
 * never). The list is closed on purpose: session replay, `identify` and the
 * feature flags are not on it, so a call to any of them from this site ends in
 * a 404 here rather than in a promise somebody has to keep.
 */
function routeOf(path: string): { method: 'GET' | 'POST'; ttl: number } | null {
  if (path === 'script.js') return { method: 'GET', ttl: 3600 }
  if (path === 'track') return { method: 'POST', ttl: 0 }
  if (/^site\/tracking-config\/[\w-]{1,64}$/.test(path)) return { method: 'GET', ttl: 300 }
  return null
}

/** The only query keys that reach Rybbit: the channel is read from these and the referrer's host. */
const MARKS = ['utm_source', 'utm_medium', 'utm_campaign', 'utm_term', 'utm_content']

/**
 * An event before it leaves: the query string cut down to `utm_*`, the
 * referrer to its origin.
 *
 * The script sends `location.search` whole, and with it the click ids —
 * `fbclid`, `gclid`, `trk` — each the number of one particular click by which
 * an ad platform knows its user; in the dashboard it would sit as a raw string
 * forever. A referrer such as `l.facebook.com/l.php?u=…&h=…` carries the same
 * kind of token in its path, and Rybbit reads the channel from the host alone.
 *
 * A body that does not parse as an object goes on untouched: Rybbit rejects it
 * itself, and a second validator here would be invention.
 */
function scrub(body: string): string {
  let event: unknown
  try {
    event = JSON.parse(body)
  } catch {
    return body
  }
  if (typeof event !== 'object' || event === null || Array.isArray(event)) return body
  const fields = event as Record<string, unknown>
  if (typeof fields.querystring === 'string') {
    const kept = new URLSearchParams()
    for (const [key, value] of new URLSearchParams(fields.querystring)) {
      if (MARKS.includes(key.toLowerCase())) kept.append(key, value)
    }
    const query = kept.toString()
    fields.querystring = query === '' ? '' : `?${query}`
  }
  if (typeof fields.referrer === 'string' && fields.referrer !== '') {
    try {
      fields.referrer = `${new URL(fields.referrer).origin}/`
    } catch {
      fields.referrer = ''
    }
  }
  return JSON.stringify(fields)
}

/**
 * `/s/*` → Rybbit.
 *
 * The visitor's address travels as `X-Forwarded-For` — without it every
 * visitor is one visitor in the worker's datacenter, since Rybbit counts
 * sessions by address and browser rather than by a cookie — and the site's
 * settings in Rybbit must have **First-Party Proxy** on, or it believes the
 * network's topology over the header. The rest of the browser's headers go as
 * they came: bot filtering reads them. Cookies do not: there are none of ours
 * on this domain, and the edge's own are nothing the counter needs.
 */
async function counter(request: Request, path: string): Promise<Response> {
  const route = routeOf(path)
  if (route === null) return new Response('not found', { status: 404 })
  if (request.method !== route.method) {
    return new Response('method not allowed', { status: 405, headers: { allow: route.method } })
  }

  const headers = new Headers(request.headers)
  // The length goes too: `scrub` rewrites the body, and the old one would lie.
  for (const name of [...HOP_BY_HOP, 'host', 'cookie', 'content-length']) headers.delete(name)
  const visitor = request.headers.get('cf-connecting-ip')
  if (visitor !== null) headers.set('x-forwarded-for', visitor)

  const url = new URL(request.url)
  const upstream = await (fetch as EdgeFetch)(`${RYBBIT_UPSTREAM}/${path}${url.search}`, {
    method: request.method,
    headers,
    body: route.method === 'POST' ? scrub(await request.text()) : null,
    ...(route.ttl > 0 ? { cf: { cacheEverything: true, cacheTtl: route.ttl } } : {}),
  })

  const response = new Response(upstream.body, upstream)
  /* Their edge's headers stay theirs: network reports would be sent to their
     collector under our name, `alt-svc` would advertise their protocols for our
     domain, and CORS has nothing to allow on a same-origin request. */
  for (const name of ['report-to', 'nel', 'alt-svc', 'access-control-allow-origin', 'set-cookie']) {
    response.headers.delete(name)
  }
  response.headers.set('cache-control', route.ttl > 0 ? `public, max-age=${route.ttl}` : 'no-store')
  return response
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
    if (url.pathname.startsWith(ANALYTICS_PREFIX)) return counter(request, url.pathname.slice(ANALYTICS_PREFIX.length))

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
