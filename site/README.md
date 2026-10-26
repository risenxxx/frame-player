# The landing page

The landing page and the guides behind it, built with Astro and served by
Cloudflare Workers static assets. No framework runtime, no third-party hosts: the
home page is 92 KB of HTML with the stylesheet inlined — 24.6 KB over the wire —
plus 2.3 KB of JavaScript for the theme switch, the header and the scene
animations.

```bash
bun install
bun run dev        # http://localhost:4321
bun run build      # → dist/
bun run typecheck  # astro check
bun run og         # re-render public/og.png and the touch icon (needs Chrome)
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
`bun run dev`, which does not serve `_redirects`). The files carry the version in
their names and old versions are pruned from the bucket, so a direct link dies
at the next release. `astro.config.ts` writes them into `_redirects` from the
same `release()` the buttons use, and the release workflow rebuilds the site,
which is what moves them on.

## The installation count

The hero's "870+ installs" is live: the player reports its first launch once
(`src-tauri/src/install_count.rs`), `POST /api/install` hands that to the
counter worker in `installs/` (a Durable Object, reached only through the
`INSTALLS` service binding), and `worker/index.ts` writes the current number
into the home page's `data-installs` element, read at most every ten minutes
per isolate. The element's value is the label in the page's language with `{n}`
where the number goes, so the worker writes the number into `/ru` and `/zh` too
without knowing a word of either. The built page carries the starting number, `INSTALLS_BASE` in
`src/installs.ts`, so a counter that cannot be reached shows that rather than
nothing. `site.yml` deploys the counter before the site; locally both run in
one session with `wrangler dev -c wrangler.jsonc -c installs/wrangler.jsonc`.

## Languages

The site is written in English, Russian and Simplified Chinese. English is at
the root and every other language is the same address under its prefix —
`/ru`, `/ru/updates`, `/zh/torrent-streaming` — with the slug left in English,
so the alternates of a page are arithmetic rather than a table
(`src/i18n/locales.ts`).

```bash
SITE_LOCALES=en,ru,zh bun run dev     # every language, the unwritten ones in English
SITE_LOCALES=en,ru,zh bun run build
```

A production build writes the languages in `PUBLISHED` (`src/i18n/locales.ts`)
and nothing else; `SITE_LOCALES` is how a language is worked on before it is
out. A published language must be complete — the build stops and lists what a
dictionary lacks, a list of the wrong length or a string whose `{placeholders}`
differ from the English — and an unpublished one borrows English string by
string, with one line in the build log saying how much. A build with one
language prints nothing of the rest: no switch, no hint, no hreflang.

Every word the templates print is in `src/i18n/en.ts`, which defines the shape;
`ru.ts` and `zh.ts` are typed against it. A template reads its page's language
from the address — `const { lang, s, href } = useLocale(Astro)` — and prints a
string through `<T s={…} />`, which renders the little markup a string may
carry (`**bold**`, `*italic*`, `` `mono` ``, `{sep}` and `{placeholders}`, see
the head of en.ts). `href` is how a link stays in the reader's language: an
English address becomes this language's one when the page exists here, and
stays English when it does not. The strings inside the mocks are the
application's own interface, and say what the player says in that language
(`src/lib/i18n.svelte.ts` at the root of the repository).

What a page in another language carries:

- `<html lang>`, the canonical address of its own, an `hreflang` link for every
  language it exists in plus `x-default` (English), `og:locale` and its
  alternates; the sitemap names the same set per address (`xhtml:link`).
- The switch in the header and the footer (`Langs.astro`): the same page in
  each language, or that language's home page where the page is not written.
- The hint (`LangHint.astro`, `scripts/lang.ts`): one line at the foot of the
  screen, in the reader's language, when the browser asks for a language the
  page also exists in. Nothing redirects by `Accept-Language` — that would send
  a crawler, which asks in English, away from every other language, and leave a
  reader who wanted this page no way to keep it. Following the switch or
  closing the hint is a choice (`fp-lang`), and the hint is not shown again.
- Its own 404: Cloudflare answers a missing address with the nearest
  `404.html` up the tree, so `/ru/anything` is answered by `/ru/404`.

Each language downloads only its own faces. Every `@font-face` carries a
`unicode-range`, and a browser fetches a face only for characters it has to
draw — so Onest's and JetBrains Mono's Cyrillic, and the Cyrillic cut of the
mocks' Rubik (`rubik-player-cyrillic.woff2`, ~10 KB), reach the Russian pages
and nothing else. Chinese ships no font at all: its Han characters come from
the system (PingFang SC, Microsoft YaHei, Noto Sans CJK — the stack is
`:root:lang(zh)` in `base.css`), with Chinese quotes and the ellipsis taken
from the same face through `local()`, as the application does. The preload in
`Page.astro` follows the language too. A new string in a mock is checked
against the ranges in `player.css`; a character outside them falls back to
Onest rather than to a missing glyph.

Chinese headings break only after their punctuation (`word-break: keep-all`),
so a heading is written in phrases short enough to fit; they lose the Latin
headings' negative tracking and get a looser line height. The breakpoints that
depend on the length of words — the header's links, the facts lines — were
measured with all three languages in place, and a new language is measured
again with its own.

A guide in another language is a file of its own: the English page's path
under the language's folder — `src/content/pages/ru/compare/vlc.mdx` is
`/ru/compare/vlc` — with one import level more and one field more,
`translated`, the English page's `updated` date it was made from. Every list of
pages (the footer, "Read next", the sitemap, llms.txt, the structured data)
comes from `src/i18n/pages.ts`, which gives a reader the page in their language
where it is written and the English one where it is not, and the switch, the
hreflang links and `href()` learn of a translation by its file existing. The
build checks every translation (`check` in pages.ts):

- it has an English page, and says which text of it it is (`translated`);
- its links stay in its language — a link to a page that exists in this
  language must be that page's address (`/ru/cast-to-tv`, `/ru` for the home
  page), and a link under the prefix must lead to a page that exists;
- when the English page is revised after the date in `translated`, the build
  lists the translation as behind. It still goes out — a page a few sentences
  behind is better than none — but nobody has to find that out by reading.

So revising an English guide is: change it, bump its `updated`, and then
either bring each translation level and set its `translated` to the new date,
or leave it listed as behind until someone does.

The mocks are pictures of the player, so their labels are the player's: the
strings under `mock` in the dictionaries are held to the application's own
(`src/lib/i18n.svelte.ts`) in every language by `src/i18n/app.ts`, which fails
the build on a difference. A new label in a mock that the player also shows
gets a line in its `MOCK_STRINGS`; the Site workflow runs on a change to the
application's dictionary for that reason.

The link card is per language too: `bun run og` draws `og.png`, `og-ru.png`,
`og-zh.png` and a card per guide in each (`og/ru-cast-to-tv.png`), with the
card's few words in a table at the top of `tools/og.mjs`.

`/updates` exists in every language, but the release notes are written once,
in English, by the release: the page around them is translated and the notes
are marked `lang="en"`. Its Markdown twin and its feed stay English only.
`/llms.txt` has a twin per language (`/ru/llms.txt`), listing the guides in it.

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
| `<Mock kind>` | One of the home page's own mocks, from `src/components/mocks/` — the same components the sections use. `torrent`, `cast`, `room`, `subs` and `picture` are the sections' own; `playing`, `preview` and `audio` are the hero's player window (`PlayerMock`) showing one thing each: a torrent's readout, a hover preview, the audio menu. `upscale` is the picture panel's upscaling row stepped through its modes (`UpscaleMock`); `mini` is a whole desktop with the mini player dragged to its edges (`MiniMock`). A guide shows the player as a mock, never as a screenshot, so every picture of it on the site is the same drawing |
| `<Note>` | An aside with an accent rule |
| `<PieceMap>` | The torrent piece diagram |

Markdown tables get readable column widths and scroll sideways when that is wider than the column, with a fade on the side that has more and a thin bar under it (`scripts/pan.ts`, the same arrangement as the architecture figure on risen.dev). Anything wider
than the 720px text column takes `wide`, up to 1040px. After adding a page, run
`bun run og` for its link card (`public/og/<id>.png`; without one the page uses
the site's), and `bun run audit` against it with `AUDIT_URL`.

## Updates

`/updates` lists every release, newest first, and the home page shows the last
three as cards (`components/Updates.astro`). Both read the `releases` collection,
which is `changelog/*.md` at the root of the repository — the files
`bun run set-version` writes out of the changesets, and the same ones the GitHub
release takes its notes from (see `scripts/changelog.mjs`). The site writes none
of it. Only releases `latest.json` says are out are shown (`src/releases.ts`),
because the commit that bumps the version rebuilds the site well before its
installers exist. Beside the page are its Markdown twin (`/updates.md`) and an
Atom feed of the last twenty (`/updates.xml`). A push that edits `changelog/`
redeploys the site, so a note corrected after its release is corrected here too.

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

The same worker is the page counter's address. Visits are counted by
[Rybbit](https://rybbit.com) — no cookies, nothing written to the device — and
the script and its events go through `/s/*` on this domain rather than to
Rybbit's host: `src/analytics.ts` holds the site id, `Analytics.astro` puts the
tag in production builds only (not `dev`, not a pull request's preview), and
the worker forwards a closed list of three endpoints with the click ids cut out
of the query and the referrer cut to its origin. The site's settings in Rybbit
need **First-Party Proxy** switched on, or every visitor is counted from the
worker's address.

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
| `src/i18n/` | The languages: which there are and which are out (`locales.ts`, pure — the worker imports it), the dictionaries (`en.ts` defines the keys), and `useLocale`/`rich`/`T.astro` — see "Languages" above |
| `src/views/` | The home page, `/updates` and the 404, each drawn once for every language: `src/pages/index.astro` and `src/pages/[lang]/index.astro` are both one line that renders `Home.astro`, and so on |
| `src/views/NotFound.astro` | Every missing address: the path the reader asked for, drawn as a file the player failed to open, over a test card. `noindex`, no canonical |
| `src/components/` | One file per section; `mocks/` holds the interface mocks the sections and the guides share — the player window, the title bar every other window carries, and one per section, `prose/` what a guide can use in its text. Each mock is the application's own surface rebuilt in HTML — the control row, the cast panel and the start screen carry the measurements and the glyphs from `src/lib/components/` rather than a screenshot |
| `src/scripts/` | `theme.ts` (system/light/dark, two switches, one state), `lang.ts` (the language choice and the hint), `nav.ts` (the header's backdrop, driven by an observer on a 1 px marker rather than a scroll handler), `motion.ts` (`--p` from 0 to 1 per scene, once, when the section is properly in view) |
| `assets/img/` | One source per frame, committed and **never published**. They are the final frames, not stand-ins for screenshots — see below |
| `public/gen/` | What `bun run images` makes from them: three formats, a handful of widths, hashed names. Generated locally and **committed** — CI never encodes |
| `scripts/images.mjs` | The generator, and the width ladders it uses |
| `tools/og.mjs` | Renders the link card, and one per guide page from its frontmatter, with the local browser. Run by hand when the design changes or a page is added; the result is committed |
| `tools/audit.mjs` | `bun run audit` — axe-core against a running `bun run preview`, in both themes, in a real browser. The file says which two ways a headless check of this page lies |

## The frames

`assets/img/*.jpg` are the frames inside the mocks. Two are photographs from
NASA and are public domain; the other eight, and the four `poster-*.jpg`, were
generated for this page and belong to the project. Nothing here needs a credit
line, and nothing here is share-alike — no frame drags a licence onto the page
around it.

Nothing in that directory is published. [`scripts/images.mjs`](scripts/images.mjs)
(`bun run images`, and in front of `dev`) writes AVIF, WebP and JPEG into
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
new `<Frame>` or a changed slot means `bun run images` and a commit of
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
