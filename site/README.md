# The landing page

One static page, built with Astro and served by Cloudflare Workers static
assets. No framework runtime, no external scripts: the whole page is 92 KB of
HTML with the stylesheet inlined — 24.6 KB over the wire — plus 2.3 KB of
JavaScript for the theme switch, the header and the scene animations.

```bash
npm install
npm run dev        # http://localhost:4321
npm run build      # → dist/
npm run typecheck  # astro check
npm run og         # re-render public/og.png and the touch icon (needs Chrome)
```

Deployment is [`.github/workflows/site.yml`](../.github/workflows/site.yml):
every push to `main` that touches `site/` builds and deploys, a pull request
gets its own preview address. It needs the repository secrets
`CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID`.

## How it is put together

| Path | What is there |
|---|---|
| `src/styles/` | The stylesheet, split by subject and imported in cascade order by the layout: tokens → base → chrome → player → mocks → sections → narrow → motion. Order is load-bearing; `narrow.css` overrides what comes before it |
| `src/components/` | One file per section. Each mock is the application's own surface rebuilt in HTML — the control row, the cast panel and the start screen carry the measurements and the glyphs from `src/lib/components/` rather than a screenshot |
| `src/scripts/` | `theme.ts` (system/light/dark, two switches, one state), `nav.ts` (the header's backdrop, driven by an observer on a 1 px marker rather than a scroll handler), `motion.ts` (`--p` from 0 to 1 per scene, once, when the section is properly in view) |
| `public/img/` | The frames inside the mocks. **Placeholders** — see below |
| `tools/og.mjs` | Renders the link card with the local browser. Run by hand when the design changes; the result is committed |

## The frames are placeholders

`public/img/*.jpg` are stills taken from third-party demo reels while the design
was being worked out. They are fine for looking at; they are **not** licensed
for a public page, and they should be replaced before this site is announced —
with footage of the project's own, with stock under a licence that allows
commercial use (Pexels, Mixkit and Coverr all do, without attribution), or with
frames generated for the purpose.

Swapping them is one directory and no code: keep the file names, or change them
in the component that references each one. What the mocks depend on is only
this — the hover preview and the frame under it are two moments of the *same*
film, and the start screen shows four different ones.
