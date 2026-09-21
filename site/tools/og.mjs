/*
  Renders the link card, one card per guide page, and the touch icon with the
  browser that is already on this machine, and writes them into `public/`. They are committed, so this runs
  by hand when the design changes — never in CI, where there is no browser and
  no reason to re-render a file that did not change.

  Usage: npm run og
*/
import { execFile } from 'node:child_process'
import { mkdir, mkdtemp, readFile, readdir, writeFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { promisify } from 'node:util'

const run = promisify(execFile)
const CHROME =
  process.env.CHROME ??
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'

const MARK = `<svg viewBox="0 0 24 24" width="76" height="76" aria-hidden="true">
  <path fill="none" stroke="#818cf8" stroke-width="2.4" stroke-linecap="round"
    d="M3.2 7.6V5.9c0-1.5 1.2-2.7 2.7-2.7h1.7M16.4 3.2h1.7c1.5 0 2.7 1.2 2.7 2.7v1.7M20.8 16.4v1.7c0 1.5-1.2 2.7-2.7 2.7h-1.7M7.6 20.8H5.9c-1.5 0-2.7-1.2-2.7-2.7v-1.7"/>
  <path fill="#818cf8" stroke="#818cf8" stroke-width="1.6" stroke-linejoin="round" d="M10 9v6l5.4-3z"/>
</svg>`

const card = `<!doctype html><meta charset="utf-8">
<style>
  @import url('https://fonts.googleapis.com/css2?family=Onest:wght@400;500;600&display=swap');
  html, body { margin: 0; width: 1200px; height: 630px; }
  body {
    background: #08090c;
    color: #f2f4f8;
    font: 400 16px/1.5 Onest, system-ui, sans-serif;
    display: flex;
    flex-direction: column;
    justify-content: center;
    padding: 0 88px;
    position: relative;
    overflow: hidden;
  }
  .glow {
    position: absolute; left: 50%; top: -420px; width: 1500px; height: 1000px;
    transform: translateX(-50%);
    background: radial-gradient(closest-side, rgba(99,102,241,.26), rgba(99,102,241,.06) 50%, transparent);
  }
  .row { display: flex; align-items: center; gap: 22px; position: relative; }
  .name { font-size: 40px; font-weight: 600; letter-spacing: -0.02em; }
  h1 {
    position: relative; margin: 40px 0 0; font-size: 58px; line-height: 1.08;
    font-weight: 600; letter-spacing: -0.035em; max-width: 18em;
  }
  p { position: relative; margin: 28px 0 0; font-size: 25px; color: #a1a7b3; max-width: 30em; }
  .facts { position: relative; margin-top: 46px; display: flex; gap: 14px; font-size: 21px; color: #7c828e; }
  .facts b { color: #f2f4f8; font-weight: 500; }
  i { width: 5px; height: 5px; border-radius: 50%; background: currentColor; align-self: center; opacity: .6; }
</style>
<div class="glow"></div>
<div class="row">${MARK}<span class="name">Frame Player</span></div>
<h1>Files, links, magnets.<br>One player that gets them right.</h1>
<p>A video player for Windows and macOS, built on mpv.</p>
<div class="facts"><b>Free and open source</b><i></i>GPL-3.0<i></i>no account, no telemetry</div>`

const icon = `<!doctype html><meta charset="utf-8">
<style>
  html, body { margin: 0; width: 180px; height: 180px; }
  body { background: #0b0d11; display: grid; place-items: center; }
  svg { width: 132px; height: 132px; }
</style>${MARK.replace('width="76" height="76"', '')}`

const shot = async (html, out, size) => {
  const dir = await mkdtemp(join(tmpdir(), 'fp-og-'))
  const file = join(dir, 'page.html')
  await writeFile(file, html)
  await run(CHROME, [
    '--headless=new',
    '--disable-gpu',
    '--hide-scrollbars',
    '--virtual-time-budget=4000',
    `--window-size=${size}`,
    `--screenshot=${out}`,
    `file://${file}`,
  ])
  await rm(dir, { recursive: true, force: true })
  console.log('wrote', out)
}

await shot(card, new URL('../public/og.png', import.meta.url).pathname, '1200,630')

/*
  A guide's card is the same card with the page's own kicker and headline, so a
  link to /cast-to-tv says what is behind it rather than repeating the home
  page. Read straight out of the frontmatter; only two plain fields are needed,
  and a YAML parser is not worth a dependency for them. `public/og/<id>.png`
  is what Article.astro looks for, with `/` in the id turned into `-`.
*/
const escape = (t) => t.replace(/&/g, '&amp;').replace(/</g, '&lt;')
const field = (text, name) => text.match(new RegExp(`^${name}: (.+)$`, 'm'))?.[1].trim()
const pagesDir = new URL('../src/content/pages/', import.meta.url).pathname
const ogDir = new URL('../public/og/', import.meta.url).pathname
await mkdir(ogDir, { recursive: true })
for (const file of await readdir(pagesDir, { recursive: true })) {
  if (!file.endsWith('.mdx')) continue
  const text = await readFile(join(pagesDir, file), 'utf8')
  const heading = field(text, 'heading')
  const eyebrow = field(text, 'eyebrow')
  if (!heading || !eyebrow) throw new Error(`${file}: no heading or eyebrow in the frontmatter`)
  const page = card
    .replace(/<h1>[\s\S]*?<\/h1>/, `<p class="kicker">${escape(eyebrow)}</p><h1>${escape(heading)}</h1>`)
    .replace('</style>', '.kicker { position: relative; margin: 44px 0 0; font-size: 20px; font-weight: 500; letter-spacing: .08em; text-transform: uppercase; color: #818cf8; } .kicker + h1 { margin-top: 14px; }</style>')
  await shot(page, join(ogDir, `${file.replace(/\.mdx$/, '').replaceAll('/', '-')}.png`), '1200,630')
}
await shot(icon, new URL('../public/apple-touch-icon.png', import.meta.url).pathname, '180,180')
