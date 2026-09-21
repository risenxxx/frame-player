import { defineCollection, z } from 'astro:content'
import { glob } from 'astro/loaders'

/*
  The pages beyond the landing page: guides, comparisons, a page per feature.
  One collection, because they share one layout and one set of fields; the
  folder a file sits in is its address — `compare/iina.mdx` is `/compare/iina`.

  They are reference pages rather than posts. `updated` is the date the page was
  last checked against the player, not the day it was first published, and it
  is what the sitemap reports.
*/
const pages = defineCollection({
  loader: glob({ pattern: '**/*.mdx', base: './src/content/pages' }),
  schema: z.object({
    /** The document title — what a search result shows. */
    title: z.string(),
    /** The meta description, and the link card's text. */
    description: z.string(),
    /** The page's own headline, which can be longer than the title. */
    heading: z.string(),
    lede: z.string(),
    /** The kicker on the page's link card (`npm run og`). Not printed on the
        page itself, where the breadcrumbs already say the same thing. */
    eyebrow: z.string(),
    /** What the page is called wherever it is a link: the footer, "Read next". */
    short: z.string(),
    updated: z.coerce.date(),
    /** Order in the footer and in lists of pages. */
    order: z.number().default(100),
    /** Other pages worth reading after this one, by their id (`compare/iina`). */
    related: z.array(z.string()).default([]),
    faq: z.array(z.object({ q: z.string(), a: z.string() })).default([]),
  }),
})

export const collections = { pages }
