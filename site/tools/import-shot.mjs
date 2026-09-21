/*
  Turns a window screenshot into a source picture for the page.

  A macOS window capture is the window plus a margin of transparent shadow, and
  the page draws its own shadow and its own rounded corners around a figure —
  two shadows is a smudge. So this finds the window by its opaque pixels, crops
  to it, fills the rounded corners with the player's own background and writes
  a JPEG into `assets/img`, where `scripts/images.mjs` builds the ladder like
  any other frame. It also prints the corner radius it found, in source pixels,
  which is what the figure's `border-radius` has to cover once scaled.

  Usage: node tools/import-shot.mjs <capture.png> <name>
         → assets/img/shot-<name>.jpg
*/
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import sharp from 'sharp'

const [, , input, name] = process.argv
if (!input || !name) {
  console.error('usage: node tools/import-shot.mjs <capture.png> <name>')
  process.exit(1)
}

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const { data, info } = await sharp(input).ensureAlpha().raw().toBuffer({ resolveWithObject: true })
const { width, height, channels } = info
const alpha = (x, y) => data[(y * width + x) * channels + 3]

/* The window is where the pixels are solid; the shadow never is. */
let top = height, bottom = -1, left = width, right = -1
for (let y = 0; y < height; y++) {
  for (let x = 0; x < width; x++) {
    if (alpha(x, y) < 250) continue
    if (y < top) top = y
    if (y > bottom) bottom = y
    if (x < left) left = x
    if (x > right) right = x
  }
}
if (bottom < 0) throw new Error('no opaque pixels: is this a window capture with a transparent margin?')

/* The corner: how far along the top edge the first solid pixel is. */
let radius = 0
while (radius < 200 && alpha(left + radius, top) < 250) radius++

const w = right - left + 1
const h = bottom - top + 1
const out = join(root, 'assets/img', `shot-${name}.jpg`)
await sharp(input)
  .extract({ left, top, width: w, height: h })
  .flatten({ background: '#08080a' })
  .jpeg({ quality: 90, mozjpeg: true })
  .toFile(out)

console.log(`${out}: ${w}×${h}, corner ≈ ${radius}px`)
