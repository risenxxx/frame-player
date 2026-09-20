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
| `assets/img/` | One source per frame, committed and **never published**. **Placeholders** — see below |
| `public/gen/` | What the build makes from them: three formats, a handful of widths, hashed names. Generated, ignored by git |
| `scripts/images.mjs` | The generator, and the width ladders it uses |
| `tools/og.mjs` | Renders the link card with the local browser. Run by hand when the design changes; the result is committed |

## The frames

`assets/img/*.jpg` are the frames inside the mocks. Two are photographs from
NASA and are public domain; the other seven, and the four `poster-*.jpg`, were
generated for this page and belong to the project. Nothing here needs a credit
line, and nothing here is share-alike — no frame drags a licence onto the page
around it.

Nothing in that directory is published. `npm run build` runs
[`scripts/images.mjs`](scripts/images.mjs) first, which writes AVIF, WebP and
JPEG at the widths each slot actually asks for into `public/gen`, and a manifest
that [`Frame.astro`](src/components/Frame.astro) reads at build time. Two
consequences worth knowing. The output names carry a hash of the bytes that
produced them, so `/gen/*` is served with a year's cache and a changed picture
is a different file rather than a stale one; and the hash is also the cache, so
a rebuild re-encodes only what changed. Measured on the whole page at 1440px:
**56 KB** of AVIF against 406 KB of JPEG before.

`Frame` takes a `slot` rather than a `sizes` string — the six slots and the
measurements behind them are in that file. A new picture is a file in
`assets/img`, a ladder in `scripts/images.mjs`, and a `<Frame name=… slot=… />`.

Replacing the set is one directory and no code — keep the file names, or change
them where each component references one. What the mocks depend on is only
this: the hover preview and the frame under it are two moments of the **same**
film, the two machines in "watch together" cut between three moments of another
one, and the start screen shows four **different** films.

[`docs/footage-prompts.md`](docs/footage-prompts.md) is how a new set gets made
— the prompts, which slot wants what, and the finding about `--sref random`
that decides whether the frames come back as photographs or as illustrations.
