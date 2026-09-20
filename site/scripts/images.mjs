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

  The width ladders are measured rather than generic — see `LADDERS`.

  It runs in front of `dev`, `typecheck` and `build` alike, not only the last of
  them: the manifest is a TypeScript import, so a checkout that has never built
  fails `astro check` on a missing module rather than on anything real. The hash
  cache is what makes running it three times cost nothing.
*/
import { createHash } from 'node:crypto'
import { mkdir, readFile, readdir, writeFile } from 'node:fs/promises'
import { existsSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import sharp from 'sharp'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const SRC = join(root, 'assets/img')
const OUT = join(root, 'public/gen')
const MANIFEST = join(root, 'src/img-manifest.json')

/*
  What each picture is asked to be, in CSS pixels, measured across the page's
  breakpoints in a real browser rather than guessed from the markup:

  - the hero window runs 356 → 1118 and is the LCP, so it gets the longest ladder;
  - a section mock's frame runs 356 → 834, since below 900px the section is one
    column and the window takes the whole width back;
  - a start-screen card is 101 → 290 — widest on a phone, not on a desktop,
    because there the rail has fewer, larger cards;
  - the hero's hover preview is 120 or 180 and nothing else;
  - the two machines on one timeline run 136 → 451;
  - a catalogue poster runs 77 → 192 against a 293px source, which is the one
    picture already short of its own 2× and the reason nothing is upscaled here.

  Each ladder covers 1× and 2× of its range and is capped at the source width.
*/
const LADDERS = {
  'orion-moon': [400, 700, 1120, 1600, 1920],
  'comet-sea': [400, 620, 840, 1240, 1680],
  'ridge-hiker': [400, 620, 840, 1120],
  'earth-aurora': [240, 360, 600, 900],
  'lake-galaxy': [200, 360, 460, 600, 900],
  'lake-tent': [200, 360, 460, 600, 900],
  'lake-trails': [200, 360, 460, 600, 900],
  'highway-trails': [200, 320, 400, 600],
  'moon-eclipse': [200, 320, 400, 600],
  poster: [120, 200, 293],
}

/* Quality per format, chosen so the three are visually the same picture: AVIF
   and WebP carry a lower number than JPEG for the same result. */
const ENC = {
  avif: (p) => p.avif({ quality: 52, effort: 5 }),
  webp: (p) => p.webp({ quality: 74 }),
  jpg: (p) => p.jpeg({ quality: 80, mozjpeg: true }),
}

const ladderFor = (name) => LADDERS[name] ?? LADDERS[name.startsWith('poster-') ? 'poster' : name]

async function main() {
  await mkdir(OUT, { recursive: true })
  const files = (await readdir(SRC)).filter((f) => f.endsWith('.jpg')).sort()
  const manifest = {}
  let made = 0

  for (const file of files) {
    const name = file.replace(/\.jpg$/, '')
    const bytes = await readFile(join(SRC, file))
    const meta = await sharp(bytes).metadata()
    const ladder = ladderFor(name)
    if (!ladder) throw new Error(`no width ladder for ${name} — add one to LADDERS`)

    const widths = [...new Set(ladder.filter((w) => w < meta.width).concat(meta.width))].sort((a, b) => a - b)
    const entry = { w: meta.width, h: meta.height, widths, formats: { avif: {}, webp: {}, jpg: {} } }

    for (const format of Object.keys(ENC)) {
      for (const w of widths) {
        const key = createHash('sha256').update(bytes).update(`${w}/${format}/1`).digest('hex').slice(0, 8)
        const out = `${name}-${w}.${key}.${format}`
        entry.formats[format][w] = `/gen/${out}`
        if (existsSync(join(OUT, out))) continue
        await ENC[format](sharp(bytes).resize(w)).toFile(join(OUT, out))
        made++
      }
    }
    manifest[name] = entry
  }

  await writeFile(MANIFEST, `${JSON.stringify(manifest, null, 2)}\n`)
  console.log(`images: ${files.length} sources, ${made} encoded, manifest written`)
}

main().catch((e) => {
  console.error(e)
  process.exit(1)
})
