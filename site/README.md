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
| `<Shot name alt wide?>` | A window screenshot. `node tools/import-shot.mjs <capture.png> <name>` turns a CleanShot capture with a transparent margin into `assets/img/shot-<name>.jpg`, cropped to the window with its corners filled; the figure draws the radius and the shadow |
| `<Mock kind>` | One of the home page's own mocks — `torrent`, `cast`, `room`, `subs` — from `src/components/mocks/`, the same components the sections use |
| `<Note>` | An aside with an accent rule |
| `<PieceMap>` | The torrent piece diagram |

Markdown tables get readable column widths and scroll sideways when that is wider than the column, with a fade on the side that has more and a thin bar under it (`scripts/pan.ts`, the same arrangement as the architecture figure on risen.dev). Anything wider
than the 720px text column takes `wide`, up to 1040px. After adding a page, run
`npm run og` for its link card (`public/og/<id>.png`; without one the page uses
the site's), and `npm run audit` against it with `AUDIT_URL`.

Addresses have no trailing slash and no extension: the build writes
`torrent-streaming.html` (`build.format: 'file'`), Workers serve it at
`/torrent-streaming`, and the canonical link says the same.

## How it is put together

| Path | What is there |
|---|---|
| `src/styles/` | The stylesheet, split by subject and imported in cascade order by the layout: tokens → base → chrome → player → mocks → sections → prose → narrow → motion. Order is load-bearing; `narrow.css` overrides what comes before it |
| `src/content/pages/` | The guide pages, one MDX file each — see above |
| `src/pages/404.astro` | Every missing address: the path the reader asked for, drawn as a file the player failed to open, over a test card. `noindex`, no canonical |
| `src/components/` | One file per section; `mocks/` holds the four interface mocks the sections and the guides share, `prose/` what a guide can use in its text. Each mock is the application's own surface rebuilt in HTML — the control row, the cast panel and the start screen carry the measurements and the glyphs from `src/lib/components/` rather than a screenshot |
| `src/scripts/` | `theme.ts` (system/light/dark, two switches, one state), `nav.ts` (the header's backdrop, driven by an observer on a 1 px marker rather than a scroll handler), `motion.ts` (`--p` from 0 to 1 per scene, once, when the section is properly in view) |
| `assets/img/` | One source per frame, committed and **never published**. **Placeholders** — see below |
| `public/gen/` | What `npm run images` makes from them: three formats, a handful of widths, hashed names. Generated locally and **committed** — CI never encodes |
| `scripts/images.mjs` | The generator, and the width ladders it uses |
| `tools/og.mjs` | Renders the link card, and one per guide page from its frontmatter, with the local browser. Run by hand when the design changes or a page is added; the result is committed |
| `tools/import-shot.mjs` | Turns a window capture into a screenshot source for a guide |
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
from the page itself: the script finds every `<Frame>` and `<Shot>` that uses a
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

`Frame` takes a `slot` rather than a `sizes` string — the six slots and the
measurements behind them are in that file. A new picture is a file in
`assets/img`, its intermediate rungs in `scripts/images.mjs`, and a
`<Frame name=… slot=… />` — the top rung follows from the slot.

Replacing the set is one directory and no code — keep the file names, or change
them where each component references one. What the mocks depend on is only
this: the hover preview and the frame under it are two moments of the **same**
film, the two machines in "watch together" cut between three moments of another
one, and the start screen shows four **different** films.

[`docs/footage-prompts.md`](docs/footage-prompts.md) is how a new set gets made
— the prompts, which slot wants what, and the finding about `--sref random`
that decides whether the frames come back as photographs or as illustrations.
