# The landing page

The landing page and the guides behind it, built with Astro and served by
Cloudflare Workers static assets. No framework runtime, no external scripts: the
home page is 92 KB of HTML with the stylesheet inlined — 24.6 KB over the wire —
plus 2.3 KB of JavaScript for the theme switch, the header and the scene
animations.

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

## Download links that outlive a release

`/download/windows` and `/download/macos` (`/download/mac` too) redirect (302) to the
current installer. They are what goes into catalog listings, articles and
anything else that is not rebuilt with the site — and what the page's own
buttons link to, so a link copied off them lasts too (except under
`npm run dev`, which does not serve `_redirects`). The files carry the version in
their names and old versions are pruned from the bucket, so a direct link dies
at the next release. `astro.config.ts` writes them into `_redirects` from the
same `release()` the buttons use, and the release workflow rebuilds the site,
which is what moves them on.

## Guide pages

Everything beyond the home page — a page per feature, the comparisons, the
guides — is one MDX file in `src/content/pages/`, and the file's place there is
its address: `compare/iina.mdx` is `/compare/iina`. They are reference pages, not
posts: `updated` is the day a page was last checked against the player, and it
is what the sitemap reports.

A new page is the file and nothing else. The frontmatter is checked by
`src/content.config.ts` (title, description, heading, lede, eyebrow, the short
name used in links, `related`, `faq`); `[...slug].astro` renders it in
`layouts/Article.astro`, which adds the breadcrumbs, the questions, "Read next",
the download block and the structured data; the footer and the sitemap list it
by themselves. A `related` entry that names no page fails the build.

What a page can put in its text, from `src/components/prose/`:

| Component | What it is |
|---|---|
| `<Mock kind>` | One of the home page's own mocks, from `src/components/mocks/` — the same components the sections use. `torrent`, `cast`, `room` and `subs` are the sections' own; `playing`, `preview` and `audio` are the hero's player window (`PlayerMock`) showing one thing each: a torrent's readout, a hover preview, the audio menu. A guide shows the player as a mock, never as a screenshot, so every picture of it on the site is the same drawing |
| `<Note>` | An aside with an accent rule |
| `<PieceMap>` | The torrent piece diagram |

Markdown tables get readable column widths and scroll sideways when that is wider than the column, with a fade on the side that has more and a thin bar under it (`scripts/pan.ts`, the same arrangement as the architecture figure on risen.dev). Anything wider
than the 720px text column takes `wide`, up to 1040px. After adding a page, run
`npm run og` for its link card (`public/og/<id>.png`; without one the page uses
the site's), and `npm run audit` against it with `AUDIT_URL`.

## What an agent reads

Every guide is also built as Markdown, at its own address plus `.md`
(`src/pages/[...slug].md.ts` with `src/markdown.ts`): the MDX body with the
imports dropped and its three components turned into the text they stand for,
under the heading, the lede, the source address and the date the page was last
checked. `/llms.txt` is the index of those — the llms.txt convention, generated
like the sitemap, so a new page appears in it by existing.

The same address answers with either, by content negotiation:
`worker/index.ts` is a worker in front of the assets that hands back the
Markdown twin when the request asks for `text/markdown` at least as strongly as
for `text/html`, and the page otherwise, with `Vary: Accept` on both. That is
Cloudflare's "Markdown for Agents" without its plan: the Markdown is built with
the site rather than converted at the edge, so what an agent reads is the
page's own source. `run_worker_first` in `wrangler.jsonc` keeps everything with
an extension — the hashed bundles, the pictures — on the CDN, where the worker
never sees it.

```bash
curl -H 'Accept: text/markdown' https://frameplayer.app/torrent-streaming
curl https://frameplayer.app/torrent-streaming.md   # the same bytes
curl https://frameplayer.app/llms.txt
```

Titles are capped at 70 characters and descriptions at 170 by the collection
schema, because past that a search result, a link preview and a quote all cut
them somewhere nobody chose. Structured data is one `@graph` per page (`WebSite`,
`SoftwareApplication` with the release date from `latest.json`, and on a guide a
`TechArticle` with `datePublished`/`dateModified` plus its breadcrumbs and
questions), so what a page says about itself and what it says about the player
are one record rather than three.

Addresses have no trailing slash and no extension: the build writes
`torrent-streaming.html` (`build.format: 'file'`), Workers serve it at
`/torrent-streaming`, and the canonical link says the same.

## How it is put together

| Path | What is there |
|---|---|
| `src/styles/` | The stylesheet, split by subject and imported in cascade order by the layout: tokens → base → chrome → player → mocks → sections → prose → narrow → motion. Order is load-bearing; `narrow.css` overrides what comes before it |
| `src/content/pages/` | The guide pages, one MDX file each — see above |
| `src/pages/404.astro` | Every missing address: the path the reader asked for, drawn as a file the player failed to open, over a test card. `noindex`, no canonical |
| `src/components/` | One file per section; `mocks/` holds the interface mocks the sections and the guides share — the player window, the title bar every other window carries, and one per section, `prose/` what a guide can use in its text. Each mock is the application's own surface rebuilt in HTML — the control row, the cast panel and the start screen carry the measurements and the glyphs from `src/lib/components/` rather than a screenshot |
| `src/scripts/` | `theme.ts` (system/light/dark, two switches, one state), `nav.ts` (the header's backdrop, driven by an observer on a 1 px marker rather than a scroll handler), `motion.ts` (`--p` from 0 to 1 per scene, once, when the section is properly in view) |
| `assets/img/` | One source per frame, committed and **never published**. **Placeholders** — see below |
| `public/gen/` | What `npm run images` makes from them: three formats, a handful of widths, hashed names. Generated locally and **committed** — CI never encodes |
| `scripts/images.mjs` | The generator, and the width ladders it uses |
| `tools/og.mjs` | Renders the link card, and one per guide page from its frontmatter, with the local browser. Run by hand when the design changes or a page is added; the result is committed |
| `tools/audit.mjs` | `npm run audit` — axe-core against a running `npm run preview`, in both themes, in a real browser. The file says which two ways a headless check of this page lies |

## The frames

`assets/img/*.jpg` are the frames inside the mocks. Two are photographs from
NASA and are public domain; the other seven, and the four `poster-*.jpg`, were
generated for this page and belong to the project. Nothing here needs a credit
line, and nothing here is share-alike — no frame drags a licence onto the page
around it.

Nothing in that directory is published. [`scripts/images.mjs`](scripts/images.mjs)
(`npm run images`, and in front of `dev`) writes AVIF, WebP and JPEG into
`public/gen`, and a manifest that
[`Frame.astro`](src/components/Frame.astro) reads at build time. The widths come
from the page itself: the script finds every `<Frame>` that uses a
picture, reads the `sizes` of its slot, and tops the ladder at twice the widest
CSS width it is drawn at (three times on a phone, when that is wider). Each AVIF
is checked against its WebP and re-encoded lower until it is the smaller one,
since it is what the `<picture>` offers first, and the run ends with a table of
every file's size. Two consequences worth knowing. The output names carry a hash of the bytes that
produced them, so `/gen/*` is served with a year's cache and a changed picture
is a different file rather than a stale one; and the hash is also the cache, so
a rebuild re-encodes only what changed. Measured on the whole page at 1440px:
**56 KB** of AVIF against 406 KB of JPEG before.

Both the files and the manifest are **committed**, because a clean encode is
minutes of AVIF and the Site workflow gives up at ten. So a changed picture, a
new `<Frame>` or a changed slot means `npm run images` and a commit of
`public/gen` and `src/img-manifest.json` with it. `typecheck` and `build` run
the script with `--check`, which encodes nothing and fails — locally and in CI —
if a file is missing, a stale one is left, or the manifest is not the one the
sources produce.

`Frame` takes a `slot` rather than a `sizes` string — the seven slots and the
measurements behind them are in that file. A new picture is a file in
`assets/img`, its intermediate rungs in `scripts/images.mjs`, and a
`<Frame name=… slot=… />` — the top rung follows from the slot.

Replacing the set is one directory and no code — keep the file names, or change
them where each component references one. What the mocks depend on is only
this: each hover preview and the frame under it are two moments of the **same**
film, the two machines in "watch together" cut between three moments of another
one, and the start screen shows four **different** films.

[`docs/footage-prompts.md`](docs/footage-prompts.md) is how a new set gets made
— the prompts, which slot wants what, and the finding about `--sref random`
that decides whether the frames come back as photographs or as illustrations.
