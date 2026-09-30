import { getCollection, type CollectionEntry } from 'astro:content'
import { release } from './release.ts'

export type ReleaseEntry = CollectionEntry<'releases'>

const parts = (v: string): number[] => v.split('.').map(Number)
const compare = (a: string, b: string): number => {
  const [x, y] = [parts(a), parts(b)]
  return x[0] - y[0] || x[1] - y[1] || x[2] - y[2]
}

/**
 * The releases that are out, newest first.
 *
 * "Out" is what `latest.json` says, not what the repository says: the commit
 * that bumps the version carries its `changelog/<version>.md`, and a push that
 * touches it rebuilds the site half an hour before the installers exist. Until
 * the release workflow has published and rebuilt the site again, the newest
 * notes wait. Without the manifest (a local build, a preview) `release()`
 * falls back on the repository's own version, which shows everything written.
 */
export async function publishedReleases(): Promise<ReleaseEntry[]> {
  const { version } = await release()
  const all = await getCollection('releases')
  return all.filter((r) => compare(r.data.version, version) <= 0).sort((a, b) => compare(b.data.version, a.data.version))
}

/** Where a release is on `/updates`. */
export const anchor = (version: string): string => `v${version}`

export const longDate = (d: Date): string =>
  d.toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric', timeZone: 'UTC' })

/* Assembled rather than `en-GB`'s short form, which spells September "Sept". */
export const shortDate = (d: Date): string =>
  `${d.getUTCDate()} ${d.toLocaleDateString('en-US', { month: 'short', timeZone: 'UTC' })} ${d.getUTCFullYear()}`

/**
 * The headlines of a release, what is new before what was fixed: the bold
 * lead of each entry, without its full stop.
 */
export function headlines(entry: ReleaseEntry): string[] {
  const body = entry.body ?? ''
  const pick = (section: string): string[] => {
    const at = body.indexOf(`### ${section}`)
    if (at < 0) return []
    const rest = body.slice(at).split('\n').slice(1)
    const end = rest.findIndex((l) => l.startsWith('#'))
    return (end < 0 ? rest : rest.slice(0, end))
      .map((l) => /^- \*\*(.+?)\*\*/.exec(l)?.[1])
      .filter((h): h is string => !!h)
      .map((h) => h.replace(/\.$/, ''))
  }
  return [...pick('New'), ...pick('Improvements and fixes')]
}
