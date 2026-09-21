/*
  Every frame on the page, in three formats and the handful of widths the layout
  actually asks for.

  `assets/img` holds one source per picture and is never published; this writes
  the derivatives into `public/gen` and a manifest that `Frame.astro` reads at
  build time. Two things follow from that split. The output names carry a hash
  of what produced them — the source bytes, the width, the format and the
  encoder settings — so `/gen/*` can be cached for a year and a changed picture
  is simply a different file rather than a stale one somebody has to notice. And
  the hash is also the cache: a file that already exists is not encoded again,
  which is what keeps an ordinary rebuild from spending a minute on AVIF.

  The width ladders follow how large each picture is drawn — see `RUNGS`.

  It runs in front of `dev`, `typecheck` and `build` alike, not only the last of
  them: the manifest is a TypeScript import, so a checkout that has never built
  fails `astro check` on a missing module rather than on anything real. The hash
  cache is what makes running it three times cost nothing.
*/
import { createHash } from 'node:crypto'
import { mkdir, readFile, readdir, rm, stat, writeFile } from 'node:fs/promises'
import { existsSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import sharp from 'sharp'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const SRC = join(root, 'assets/img')
const OUT = join(root, 'public/gen')
const MANIFEST = join(root, 'src/img-manifest.json')

/*
  How large each picture is drawn decides how large it is encoded, and that is
  read off the page rather than written down twice. Every `<Frame name slot>`
  under `src/` (and every `<Shot name [wide]>`, which is a Frame with a slot
  chosen for it) says where a picture is used; `SIZES` in Frame.astro says how
  wide that slot is at every viewport. The widest CSS width a picture is ever
  given, times two, is the top of its ladder — rounded up to a multiple of 20 and
  capped at the source, since nothing is upscaled. A 3× screen is the one reason
  to go past that, and it is only real on a phone: a start-screen card is 300px
  there and 190px on a desktop, so its 3× rung (900) outgrows its 2× one (600).
  `PHONE` is the widest viewport that is assumed to be 3×.

  Measured in a browser against the `sizes` this derives from (Sep 2026): the
  hero draws orion-moon at up to 1118px, which is why its ladder still runs to
  the 1920 source and keeps 1600 (an 820px window at 2× asks for ~1510).

  `RUNGS` are the steps *below* the top, chosen by hand for the breakpoints in
  between; any rung at or above the computed top is dropped.
*/
const RUNGS = {
  /* 1320 is risen.dev's: it copies these files and draws orion-moon at 660px. */
  'orion-moon': [400, 700, 1120, 1320, 1600],
  'comet-sea': [400, 620, 840, 1240],
  'ridge-hiker': [400, 620, 840],
  'earth-aurora': [240, 360, 600],
  'lake-galaxy': [200, 360, 460, 600],
  'lake-tent': [200, 360, 460, 600],
  'lake-trails': [200, 360, 460, 600],
  'highway-trails': [200, 320, 400, 600],
  /* No 200: the frame is nearly black, its 200px WebP is 398 bytes, and an AVIF
     container alone costs about that — no quality gets under it. */
  'moon-eclipse': [320, 400, 600],
  poster: [120, 200],
  shot: [480, 720, 1080],
}
const PHONE = 480
const STEP = 20

const rungsFor = (name) => RUNGS[name] ?? RUNGS[name.replace(/-.*$/, '')]

/* Split on commas that are not inside parentheses — `clamp()` has its own. */
function splitTop(str) {
  const out = []
  let depth = 0
  let cur = ''
  for (const ch of str) {
    if (ch === '(') depth++
    if (ch === ')') depth--
    if (ch === ',' && depth === 0) {
      out.push(cur.trim())
      cur = ''
    } else cur += ch
  }
  return out.concat(cur.trim())
}

/* A CSS length at a given viewport width: px, vw, calc/clamp/min/max. */
function lengthAt(expr, vw) {
  const js = expr
    .replace(/calc\(/g, '(')
    .replace(/clamp\(/g, 'C(')
    .replace(/\b(min|max)\(/g, 'Math.$1(')
    .replace(/(\d+(?:\.\d+)?)vw/g, `($1*${vw}/100)`)
    .replace(/(\d+(?:\.\d+)?)px/g, '$1')
  if (/[^\d\s.+\-*/(),CMathinx]/.test(js)) throw new Error(`sizes: cannot evaluate "${expr}"`)
  return Function('C', `return ${js}`)((lo, v, hi) => Math.min(Math.max(v, lo), hi))
}

/* One `sizes` string as ordered viewport ranges, each with its length. Only
   `(min-width: Npx)` conditions, in descending order, which is all Frame uses. */
function parseSizes(sizes) {
  let upper = Infinity
  return splitTop(sizes).map((entry) => {
    const m = entry.match(/^\(min-width:\s*(\d+)px\)\s+(.+)$/)
    const lo = m ? Number(m[1]) : 0
    const range = { lo, hi: upper, length: m ? m[2] : entry }
    upper = lo - 1
    return range
  })
}

/* The widest CSS width a slot is drawn at, and what it asks for on a 3× phone.
   Every length here grows with the viewport, so a range's widest point is its
   top edge. */
function slotDemand(sizes) {
  const ranges = parseSizes(sizes)
  let widest = 0
  for (const r of ranges) {
    if (r.hi === Infinity && /vw/.test(r.length)) throw new Error(`sizes: "${r.length}" has no upper bound`)
    widest = Math.max(widest, lengthAt(r.length, r.hi === Infinity ? 0 : r.hi))
  }
  const phone = ranges.find((r) => PHONE >= r.lo && PHONE <= r.hi)
  /* Whole CSS pixels: at a range's last viewport a `clamp()` can leave 720.36,
     which is 720 on screen and must not push a rung up by a whole step. */
  return { widest: Math.round(widest), phone3x: 3 * Math.round(lengthAt(phone.length, PHONE)) }
}

async function walk(dir) {
  const out = []
  for (const e of await readdir(dir, { withFileTypes: true })) {
    const p = join(dir, e.name)
    if (e.isDirectory()) out.push(...(await walk(p)))
    else if (/\.(astro|mdx)$/.test(e.name)) out.push(p)
  }
  return out
}

/* name → the slots it is used in, found in the markup. */
async function usage() {
  const frame = await readFile(join(root, 'src/components/Frame.astro'), 'utf8')
  const block = frame.match(/const SIZES = \{([\s\S]*?)\n\}/)[1].replace(/\/\*[\s\S]*?\*\//g, '')
  const SIZES = Object.fromEntries([...block.matchAll(/(\w+):\s*'([^']+)'/g)].map((m) => [m[1], m[2]]))
  const used = {}
  const add = (name, slot) => {
    if (!SIZES[slot]) throw new Error(`${name} uses slot "${slot}", which Frame.astro does not define`)
    ;(used[name] ??= new Set()).add(slot)
  }
  for (const file of await walk(join(root, 'src'))) {
    const text = await readFile(file, 'utf8')
    for (const [tag] of text.matchAll(/<Frame\b[^>]*>/g)) {
      const name = tag.match(/\bname="([^"]+)"/)?.[1]
      const slot = tag.match(/\bslot="([^"]+)"/)?.[1]
      if (name && slot) add(name, slot)
    }
    for (const [tag] of text.matchAll(/<Shot\b[^>]*>/g)) {
      const name = tag.match(/\bname="([^"]+)"/)?.[1]
      if (name) add(`shot-${name}`, /\swide(?=[\s>/])/.test(tag) ? 'wide' : 'text')
    }
  }
  return { used, SIZES }
}

function ladderFor(name, srcWidth, slots, SIZES) {
  const rungs = rungsFor(name)
  if (!rungs) throw new Error(`no rungs for ${name} — add them to RUNGS`)
  if (!slots) throw new Error(`${name} is in assets/img but no <Frame> or <Shot> uses it`)
  let need = 0
  for (const slot of slots) {
    const { widest, phone3x } = slotDemand(SIZES[slot])
    need = Math.max(need, 2 * widest, phone3x)
  }
  const top = Math.min(Math.ceil(need / STEP) * STEP, srcWidth)
  return [...new Set(rungs.filter((w) => w < top).concat(top))].sort((a, b) => a - b)
}

/* Quality per format, chosen so the three are visually the same picture: AVIF
   and WebP carry a lower number than JPEG for the same result. AVIF comes first
   in the `<picture>`, so it is the file nearly every browser downloads — and it
   must therefore never be the heavier one. Some pictures (the dark, grainy lake
   frames) encode worse in AVIF at the shared quality than in WebP, so each AVIF
   is checked against its WebP and re-encoded a step lower until it is smaller;
   `AVIF_START` saves those the walk down. Every attempt starts from the source.
   Chroma is 4:2:0 like the WebP beside it: sharp's AVIF default is 4:4:4, which
   spends ~5% on colour detail the other two formats have already thrown away. */
const AVIF = { quality: 52, effort: 9, floor: 40, step: 3 }
const AVIF_START = { 'lake-galaxy': 45 }
const ENC = {
  avif: (p, quality) => p.avif({ quality, effort: AVIF.effort, chromaSubsampling: '4:2:0' }),
  webp: (p) => p.webp({ quality: 74 }),
  jpg: (p) => p.jpeg({ quality: 80, mozjpeg: true }),
}
/* Bumped whenever ENC or the AVIF walk changes, so the names change with it. */
const ENC_VERSION = 2

const kb = (n) => (n / 1000).toFixed(1)

async function main() {
  await mkdir(OUT, { recursive: true })
  const files = (await readdir(SRC)).filter((f) => f.endsWith('.jpg')).sort()
  const { used, SIZES } = await usage()
  const manifest = {}
  const table = []
  let made = 0

  for (const file of files) {
    const name = file.replace(/\.jpg$/, '')
    const bytes = await readFile(join(SRC, file))
    const meta = await sharp(bytes).metadata()
    const widths = ladderFor(name, meta.width, used[name], SIZES)
    const entry = { w: meta.width, h: meta.height, widths, formats: { avif: {}, webp: {}, jpg: {} } }
    const avifStart = AVIF_START[name] ?? AVIF.quality

    for (const w of widths) {
      const size = {}
      for (const format of ['webp', 'jpg', 'avif']) {
        const settings = format === 'avif' ? `q${avifStart}e${AVIF.effort}f${AVIF.floor}s420` : ''
        const key = createHash('sha256').update(bytes).update(`${w}/${format}/${settings}/${ENC_VERSION}`).digest('hex').slice(0, 8)
        const out = `${name}-${w}.${key}.${format}`
        const path = join(OUT, out)
        entry.formats[format][w] = `/gen/${out}`
        if (!existsSync(path)) {
          if (format === 'avif') {
            let q = avifStart
            let buf = await ENC.avif(sharp(bytes).resize(w), q).toBuffer()
            while (buf.length >= size.webp) {
              q -= AVIF.step
              if (q < AVIF.floor) {
                throw new Error(
                  `${name}-${w}: AVIF is still ${kb(buf.length)} KB against WebP's ${kb(size.webp)} KB at quality ${q + AVIF.step} — ` +
                    `lower AVIF.floor or give ${name} its own settings`,
                )
              }
              buf = await ENC.avif(sharp(bytes).resize(w), q).toBuffer()
            }
            await writeFile(path, buf)
          } else {
            await ENC[format](sharp(bytes).resize(w)).toFile(path)
          }
          made++
        }
        size[format] = (await stat(path)).size
      }
      /* A cached file was checked when it was made, but a hand-edited setting
         could still leave one behind that was not. */
      if (size.avif >= size.webp) throw new Error(`${name}-${w}: AVIF ${kb(size.avif)} KB is not smaller than WebP ${kb(size.webp)} KB`)
      table.push([name, w, size.avif, size.webp, size.jpg])
    }
    manifest[name] = entry
  }

  /* Anything in public/gen the manifest no longer names is a width or a setting
     that has gone — left there, it would still be deployed. */
  const keep = new Set(Object.values(manifest).flatMap((e) => Object.values(e.formats).flatMap((f) => Object.values(f).map((u) => u.slice(5)))))
  let pruned = 0
  for (const f of await readdir(OUT)) {
    if (!keep.has(f)) {
      await rm(join(OUT, f))
      pruned++
    }
  }

  await writeFile(MANIFEST, `${JSON.stringify(manifest, null, 2)}\n`)
  const pad = (v, n) => String(v).padStart(n)
  console.log(`${'image'.padEnd(20)}${pad('width', 6)}${pad('avif', 9)}${pad('webp', 9)}${pad('jpg', 9)}   (KB)`)
  for (const [name, w, a, wb, j] of table) console.log(`${name.padEnd(20)}${pad(w, 6)}${pad(kb(a), 9)}${pad(kb(wb), 9)}${pad(kb(j), 9)}`)
  console.log(`images: ${files.length} sources, ${made} encoded, ${pruned} stale removed, manifest written`)
}

main().catch((e) => {
  console.error(e)
  process.exit(1)
})
