---
paths:
  - "site/**"
---

# The landing page — the short form

Loaded with the files under `site/`; the whole of it is `site/README.md`.

## Modules

| Path | What lives there |
|---|---|
| `site/` | The landing page: Astro, static output, Cloudflare Workers static assets. Its own bun project and its own workflow (`site.yml`) — the root gates do not reach it, exactly as they do not reach `services/`. Every mock on the page is one of the application's own surfaces rebuilt in HTML, with the glyphs and the measurements taken from `src/lib/components/` rather than from a screenshot; the frames inside them are placeholders and `site/README.md` says what has to happen to them before the site is announced. `site/assets/img` holds one source per picture and `scripts/images.mjs` (`bun run images`) builds the AVIF/WebP/JPEG ladder into `public/gen` under hashed names, which is what lets that path be cached for a year. The output and `src/img-manifest.json` are **committed** — a clean encode outlasts the Site workflow's ten minutes — and `typecheck`/`build` only run it with `--check`, which fails when they are out of date. Beyond the home page are the guide pages — one MDX file each in `site/src/content/pages/`, rendered by one layout; how to add one is in `site/README.md`. Every guide is also built as Markdown (`<page>.md`, indexed by `/llms.txt`), and `site/worker/index.ts` serves that twin to a request asking for `text/markdown` — content negotiation of our own rather than Cloudflare's paid one. Its words are in `site/src/i18n/` — English at the root, every other language under its prefix (`/ru`, `/zh`) with the English slug, a template reading the page's language from its address (`useLocale`) — and a language goes out by one line, `PUBLISHED` in `locales.ts`, which the build refuses while its dictionary has a gap; `site/README.md` ("Languages") is the whole of it |
