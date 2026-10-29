/*
  The mocks say what the application says.

  Every mock on this site is a picture of the player's own interface, so a
  label in it is the player's label in that language — "Найти субтитры…", not
  a translator's synonym for it. The application's dictionaries are
  src/lib/i18n/{ru,en,zh}.ts at the root of the repository; this module reads
  them as text and holds each mapped string of ours to them, in every
  language. A label renamed in the player fails the site's build until the
  mock follows. Read rather than imported: an import makes this build compile
  them under the application's tsconfig, which extends a file only
  `svelte-kit sync` writes — present after the player's own build, absent in
  the site's workflow.

  `MOCK_STRINGS` is the map: our dotted path → the application's key, with the
  values to fill into the application's placeholders where the mock shows a
  particular one (the mock's "9 videos" is the player's `{count} videos`), and
  the renames where the two name a placeholder differently.
*/
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { LOCALES, type Locale } from './locales.ts'
import { strings } from './index.ts'

interface Mapping {
  key: string
  /** Values put into the application's placeholders before comparing. */
  fill?: Record<string, string | number>
  /** Our placeholder name → the application's, for one left open on both sides. */
  rename?: Record<string, string>
}

const MOCK_STRINGS: Record<string, Mapping> = {
  'mock.common.thisFile': { key: 'adj.scope_file' },
  'mock.common.playlist': { key: 'adj.scope_folder' },
  'mock.common.addFile': { key: 'osc.add_file' },
  'mock.common.left': { key: 'start.remaining', rename: { t: 'time' } },
  'mock.common.download': { key: 'subs.download' },
  'mock.common.withKey': { key: 'osc.with_key' },
  'mock.opener.title': { key: 'start.link' },
  'mock.opener.go': { key: 'link.open' },
  'mock.cast.title': { key: 'cast.title' },
  'mock.cast.asIs': { key: 'cast.sum_direct' },
  'mock.cast.prepared': { key: 'cast.sum_prepare' },
  'mock.cast.checking': { key: 'cast.sum_checking' },
  'mock.room.host': { key: 'sync.host_badge' },
  'mock.subs.title': { key: 'osc.subs' },
  'mock.subs.off': { key: 'osc.subs_off' },
  'mock.subs.find': { key: 'subs.find' },
  'mock.subs.dialog': { key: 'subs.title' },
  'mock.subs.search': { key: 'subs.search' },
  'mock.subs.exactHint': { key: 'subs.by_hash' },
  'mock.subs.downloads': { key: 'subs.downloads', rename: { n: 'count' } },
  'mock.subs.exact': { key: 'subs.badge_hash' },
  'mock.player.skipIntro': { key: 'skip.intro' },
  'mock.player.audioTrack': { key: 'osc.audio' },
  'mock.player.delay': { key: 'osc.delay' },
  'mock.player.reset': { key: 'osc.delay_reset' },
  'mock.player.chapter': { key: 'chapter.generic', fill: { n: 2 } },
  'mock.picture.looking': { key: 'osd.crop_detecting' },
  'mock.picture.crop': { key: 'ctx.crop' },
  'mock.picture.auto': { key: 'crop.auto' },
  'mock.picture.none': { key: 'crop.none' },
  'mock.picture.color': { key: 'ctx.color' },
  'mock.picture.brightness': { key: 'adj.brightness' },
  'mock.picture.contrast': { key: 'adj.contrast' },
  'mock.picture.saturation': { key: 'adj.saturation' },
  'mock.picture.gamma': { key: 'adj.gamma' },
  'mock.picture.applyTo': { key: 'adj.apply_to' },
  'mock.picture.allFiles': { key: 'adj.scope_all' },
  'mock.upscale.title': { key: 'upscale.title' },
  'mock.upscale.off': { key: 'upscale.off' },
  'mock.upscale.video': { key: 'upscale.video' },
  'mock.upscale.anime': { key: 'upscale.anime' },
  'mock.upscale.strong': { key: 'upscale.strong' },
  'mock.catalog.title': { key: 'catalog.title' },
  'mock.start.hint': { key: 'start.hint' },
  'mock.start.openFile': { key: 'start.open' },
  'mock.start.openLink': { key: 'start.link' },
  'mock.start.catalog': { key: 'start.catalog' },
  'mock.start.continue': { key: 'start.recent' },
  'mock.start.torrents': { key: 'start.torrents' },
  'mock.start.videos': { key: 'start.torrent_files', fill: { count: 9 } },
}

/* One file per language, the object named after it: `export const ru = {`. */
function appDictionaries(): Record<Locale, Record<string, string>> {
  const out = {} as Record<Locale, Record<string, string>>
  for (const lang of LOCALES) {
    const file = resolve(process.cwd(), '..', `src/lib/i18n/${lang}.ts`)
    const text = readFileSync(file, 'utf8')
    const start = text.search(new RegExp(`^export const ${lang}\\b[^=]*= \\{`, 'm'))
    if (start < 0) throw new Error(`${file}: no \`export const ${lang} = {\``)
    const end = text.indexOf('\n}', start)
    const body = text.slice(start, end)
    const dict: Record<string, string> = {}
    for (const m of body.matchAll(/^\s+'([\w.]+)':\s*(['"`])((?:\\.|(?!\2)[^\\])*)\2/gms)) {
      dict[m[1]!] = m[3]!.replace(/\\(['"`\\])/g, '$1')
    }
    out[lang] = dict
  }
  return out
}

const at = (obj: unknown, path: string): unknown =>
  path.split('.').reduce<unknown>((o, k) => (o as Record<string, unknown> | undefined)?.[k], obj)

let checked = false

/** Holds every mapped mock string to the application's, in every language; throws on the first build that disagrees. */
export function checkMockStrings(): void {
  if (checked) return
  checked = true
  const app = appDictionaries()
  const errors: string[] = []
  for (const lang of LOCALES) {
    const s = strings(lang)
    for (const [path, { key, fill = {}, rename = {} }] of Object.entries(MOCK_STRINGS)) {
      const theirs = app[lang][key]
      if (theirs === undefined) {
        errors.push(`${path}: the application has no "${key}" in ${lang}`)
        continue
      }
      const expected = theirs.replace(/\{(\w+)\}/g, (whole, k: string) => (fill[k] === undefined ? whole : String(fill[k])))
      let ours = String(at(s, path))
      for (const [from, to] of Object.entries(rename)) ours = ours.replaceAll(`{${from}}`, `{${to}}`)
      if (ours !== expected) errors.push(`${lang} ${path}: "${ours}" — the application's ${key} says "${expected}"`)
    }
  }
  if (errors.length > 0) throw new Error(`Mock strings differ from the application:\n  ${errors.join('\n  ')}`)
}
