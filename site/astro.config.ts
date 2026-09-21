import { writeFile } from 'node:fs/promises'
import type { AstroIntegration } from 'astro'
import { defineConfig } from 'astro/config'
import mdx from '@astrojs/mdx'
import { DOWNLOAD_PATHS, release } from './src/release'

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

/**
 * Addresses that stay put across releases, for everywhere a link outlives a
 * build: a catalog listing, an article, a forum post. The files themselves
 * carry the version in their names and old versions are pruned from the
 * bucket, so a direct link dies with the next release. These redirect to
 * whatever `release()` resolved — the same targets as the buttons, fallback
 * to the GitHub release page included — and the release workflow rebuilds the
 * site, which is what moves them on.
 */
function downloads(): AstroIntegration {
  return {
    name: 'site-downloads',
    hooks: {
      'astro:build:done': async ({ dir }) => {
        const { windows, macos } = await release()
        const lines = [
          `${DOWNLOAD_PATHS.windows} ${windows} 302`,
          `${DOWNLOAD_PATHS.macos} ${macos} 302`,
          `/download/mac ${macos} 302`,
        ]
        await writeFile(new URL('_redirects', dir), `${lines.join('\n')}\n`)
      },
    },
  }
}

export default defineConfig({
  output: 'static',
  site: site.origin,
  /*
    One address per page, without a trailing slash: `/torrent-streaming`, never
    also `/torrent-streaming/`. Built as `torrent-streaming.html`, which Workers
    static assets serve at the bare path and redirect the slashed one to — so
    the canonical link, the sitemap and what the server answers all agree.
  */
  trailingSlash: 'never',
  integrations: [mdx(), headers(), downloads()],
  /*
    The stylesheet is one page's worth and compresses to a few kilobytes; a
    separate request for it only delays the first paint.
  */
  build: { inlineStylesheets: 'always', format: 'file' },
  /*
    Scoped styles through :where() add no specificity, so a component overrides
    a shared primitive by source order rather than by an attribute selector
    silently outranking its neighbours.
  */
  scopedStyleStrategy: 'where',
})
