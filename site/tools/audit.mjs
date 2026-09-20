/*
  Accessibility, in a real browser, in both themes.

  It exists because this question came back twice and cost a day of guessing
  each time. Two traps make a headless check lie, and both were hit here:

  - `--virtual-time-budget` freezes the document timeline, so every CSS
    transition and animation holds its *starting* value for ever. A page read
    that way reports a hero button at 4.4:1 (mid-fade) and a theme switch that
    never finished, none of which is true a second later in a real browser.
  - axe does **not** skip an `aria-hidden` subtree for colour contrast. Marking
    the mocks decorative does not exempt what they draw; only their actual
    colours do.

  So: a real page, real time, `prefers-color-scheme` emulated rather than a
  runtime attribute flip, and the read taken five seconds in — roughly when a
  Lighthouse gatherer takes its own. Text that is still hidden (a section that
  has not been scrolled to) is skipped by axe exactly as it is by a reader.

  Run it against `npm run preview`, or point AUDIT_URL somewhere else.
*/
import { readFileSync } from 'node:fs'
import puppeteer from 'puppeteer-core'

const CHROME =
  process.env.CHROME_PATH ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
const URL_ = process.env.AUDIT_URL ?? 'http://127.0.0.1:4321'

const axe = readFileSync(new URL('../node_modules/axe-core/axe.min.js', import.meta.url), 'utf8')

const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: 'new',
  args: ['--hide-scrollbars'],
})

let failed = 0
for (const theme of ['light', 'dark']) {
  const page = await browser.newPage()
  await page.setViewport({ width: 1350, height: 940 })
  await page.emulateMediaFeatures([{ name: 'prefers-color-scheme', value: theme }])
  await page.goto(URL_, { waitUntil: 'networkidle0' })
  await new Promise((r) => setTimeout(r, 5000))
  await page.evaluate(axe)
  const result = await page.evaluate(async () => await axe.run(document))
  failed += result.violations.length
  const detail = result.violations
    .map(
      (v) =>
        `\n    ${v.id} (${v.nodes.length}): ${v.nodes.slice(0, 5).map((n) => n.target.join(' ')).join(', ')}`,
    )
    .join('')
  console.log(`${theme}: ${result.violations.length} violations${detail}`)
  await page.close()
}

await browser.close()
process.exit(failed === 0 ? 0 : 1)
