/*
  Renders the link card, one card per guide page, and the touch icon with the
  browser that is already on this machine, and writes them into `public/`. They are committed, so this runs
  by hand when the design changes — never in CI, where there is no browser and
  no reason to re-render a file that did not change.

  Usage: bun run og
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

/*
  The card's words, per language. A copy of three strings the site keeps in
  src/i18n/ — this tool is plain Node and cannot import TypeScript, and the
  cards are redrawn by hand anyway; the headline is `hero.title`, keep the two
  in step. Chinese draws its Han from the system face, as the site does, and
  loses the Latin headline's negative tracking.
*/
const escape = (t) => t.replace(/&/g, '&amp;').replace(/</g, '&lt;')

const WORDS = {
  en: {
    title: ['Files, links, magnets.', 'One player that gets them right.'],
    sub: 'A video player for Windows and macOS, built on mpv.',
    facts: ['Free and open source', 'GPL-3.0', 'no account, no telemetry'],
  },
  ru: {
    title: ['Файлы, ссылки, торренты.', 'Один плеер — и всё как надо.'],
    sub: 'Видеоплеер для Windows и macOS на движке mpv.',
    facts: ['Бесплатно и с открытым кодом', 'GPL-3.0', 'без аккаунта и телеметрии'],
  },
  zh: {
    title: ['文件、链接、磁力链接，', '一个播放器全部搞定。'],
    sub: '基于 mpv 的视频播放器，适用于 Windows 和 macOS。',
    facts: ['免费开源', 'GPL-3.0', '无需账号，没有遥测'],
  },
}
const LANGS = Object.keys(WORDS)

const card = (lang) => {
  const w = WORDS[lang]
  return `<!doctype html><meta charset="utf-8">
<style>
  @import url('https://fonts.googleapis.com/css2?family=Onest:wght@400;500;600&display=swap');
  html, body { margin: 0; width: 1200px; height: 630px; }
  body {
    background: #08090c;
    color: #f2f4f8;
    font: 400 16px/1.5 Onest, ${lang === 'zh' ? '"PingFang SC", "Microsoft YaHei", "Noto Sans CJK SC", ' : ''}system-ui, sans-serif;
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
<h1>${w.title.map(escape).join('<br>')}</h1>
<p>${escape(w.sub)}</p>
<div class="facts"><b>${escape(w.facts[0])}</b><i></i>${escape(w.facts[1])}<i></i>${escape(w.facts[2])}</div>`
    .replace('</style>', lang === 'zh' ? 'h1, .kicker { letter-spacing: 0 !important; } h1 { line-height: 1.25; }</style>' : '</style>')
}

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

/* The site's own card: `og.png` in English, `og-ru.png` and `og-zh.png` beside
   it (Page.astro picks the page's language's). */
for (const lang of LANGS) {
  await shot(card(lang), new URL(`../public/og${lang === 'en' ? '' : `-${lang}`}.png`, import.meta.url).pathname, '1200,630')
}

/*
  A guide's card is the same card with the page's own kicker and headline, so a
  link to /cast-to-tv says what is behind it rather than repeating the home
  page. Read straight out of the frontmatter; only two plain fields are needed,
  and a YAML parser is not worth a dependency for them. `public/og/<id>.png`
  is what Article.astro looks for, with `/` in the id turned into `-` — a
  translation's id starts with its language (`ru/cast-to-tv` →
  `ru-cast-to-tv.png`), and so does its card's words.
*/
/* A value may be quoted, as YAML needs it to be when it holds ": ". */
const field = (text, name) => {
  const raw = text.match(new RegExp(`^${name}: (.+)$`, 'm'))?.[1].trim()
  return raw && /^(["']).*\1$/.test(raw) ? raw.slice(1, -1).replace(/\\"/g, '"') : raw
}
const pagesDir = new URL('../src/content/pages/', import.meta.url).pathname
const ogDir = new URL('../public/og/', import.meta.url).pathname
await mkdir(ogDir, { recursive: true })
for (const file of await readdir(pagesDir, { recursive: true })) {
  if (!file.endsWith('.mdx')) continue
  const text = await readFile(join(pagesDir, file), 'utf8')
  const heading = field(text, 'heading')
  const eyebrow = field(text, 'eyebrow')
  if (!heading || !eyebrow) throw new Error(`${file}: no heading or eyebrow in the frontmatter`)
  const first = file.split('/')[0]
  const page = card(LANGS.includes(first) ? first : 'en')
    .replace(/<h1>[\s\S]*?<\/h1>/, `<p class="kicker">${escape(eyebrow)}</p><h1>${escape(heading)}</h1>`)
    .replace('</style>', '.kicker { position: relative; margin: 44px 0 0; font-size: 20px; font-weight: 500; letter-spacing: .08em; text-transform: uppercase; color: #818cf8; } .kicker + h1 { margin-top: 14px; }</style>')
  await shot(page, join(ogDir, `${file.replace(/\.mdx$/, '').replaceAll('/', '-')}.png`), '1200,630')
}
await shot(icon, new URL('../public/apple-touch-icon.png', import.meta.url).pathname, '180,180')
