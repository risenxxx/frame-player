import { writeFile } from 'node:fs/promises'
import type { AstroIntegration } from 'astro'
import { defineConfig } from 'astro/config'

/*
  One page, static output, no framework runtime. `SITE_ORIGIN` lets a preview
  build advertise its own address in the canonical link and the sitemap instead
  of the production one.
*/
const site = new URL(process.env.SITE_ORIGIN ?? 'https://frameplayer.app')

/** A preview build asks search engines to stay away: production is canonical. */
const preview = process.env.SITE_PREVIEW === '1'

/**
 * `_headers` is written after the build rather than kept in `public/`: the
 * noindex header belongs to preview builds only, and one file serving both
 * modes would have to be remembered by hand.
 */
function headers(): AstroIntegration {
  return {
    name: 'site-headers',
    hooks: {
      'astro:build:done': async ({ dir }) => {
        const lines = [
          '/*',
          '  X-Content-Type-Options: nosniff',
          '  Referrer-Policy: strict-origin-when-cross-origin',
          '  X-Frame-Options: DENY',
          ...(preview ? ['  X-Robots-Tag: noindex, nofollow'] : []),
          '/_astro/*',
          '  Cache-Control: public, max-age=31536000, immutable',
          /*
            Everything under /gen carries a hash of the bytes that made it, so a
            year is safe: a changed picture is a different name rather than a
            stale file somebody has to invalidate. /img is what is left of the
            hand-placed set, which does not.
          */
          '/gen/*',
          '  Cache-Control: public, max-age=31536000, immutable',
          '/img/*',
          '  Cache-Control: public, max-age=604800',
        ]
        await writeFile(new URL('_headers', dir), `${lines.join('\n')}\n`)
      },
    },
  }
}

export default defineConfig({
  output: 'static',
  site: site.origin,
  integrations: [headers()],
  /*
    The stylesheet is one page's worth and compresses to a few kilobytes; a
    separate request for it only delays the first paint.
  */
  build: { inlineStylesheets: 'always' },
  /*
    Scoped styles through :where() add no specificity, so a component overrides
    a shared primitive by source order rather than by an attribute selector
    silently outranking its neighbours.
  */
  scopedStyleStrategy: 'where',
})
