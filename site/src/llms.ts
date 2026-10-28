/*
  The site in one file, for something that reads it rather than renders it —
  the llms.txt convention: a title, a summary, and one line per page saying what
  is there. Every link points at that page's Markdown twin (`[...slug].md.ts`),
  because a link an agent follows should give it text and not markup.

  One per language: `/llms.txt` in English, `/ru/llms.txt` and `/zh/llms.txt`
  listing the same pages in that language (in English where a page is not
  translated yet). Generated, like the sitemap, so a new page appears here by
  existing.
*/
import { release } from './release.ts'
import { DEFAULT_LOCALE, LOCALE_META, enabledLocales, fill, localePath, multilingual, strings, type Locale } from './i18n/index.ts'
import { baseId, pagesIn } from './i18n/pages.ts'

export async function llmsText(lang: Locale, site: URL): Promise<string> {
  const s = strings(lang).llms
  const at = (path: string): string => new URL(path, site).href
  const pages = await pagesIn(lang)
  const { version, date } = await release()

  const group = (label: string, prefix: (id: string) => boolean): string[] => {
    const rows = pages.filter((p) => prefix(baseId(p)))
    if (rows.length === 0) return []
    return ['', `## ${label}`, '', ...rows.map((p) => `- [${p.data.short}](${at(`/${p.id}.md`)}): ${p.data.description}`)]
  }

  const others = multilingual() ? enabledLocales().filter((l) => l !== lang) : []

  return [
    '# Frame Player',
    '',
    ...s.summary.map((line) => `> ${line}`),
    '',
    fill(s.version, { version, released: date ? fill(s.released, { date: date.slice(0, 10) }) : '' }),
    s.platforms,
    '',
    `## ${s.start}`,
    '',
    `- ${fill(s.home, { url: at(localePath(lang, '/')) })}`,
    `- ${fill(s.updates, { url: at('/updates.md') })}`,
    `- ${fill(s.downloads, { windows: at('/download/windows'), windowsArm: at('/download/windows-arm64'), macos: at('/download/macos') })}`,
    ...group(s.features, (id) => !id.includes('/')),
    ...group(s.comparisons, (id) => id.startsWith('compare/')),
    ...group(s.guides, (id) => id.startsWith('guides/')),
    '',
    `## ${s.project}`,
    '',
    `- ${fill(s.source, { url: 'https://github.com/risenxxx/frame-player' })}`,
    `- ${fill(s.docs, { url: 'https://github.com/risenxxx/frame-player/tree/main/docs' })}`,
    `- ${fill(s.releases, { url: 'https://github.com/risenxxx/frame-player/releases' })}`,
    '',
    `## ${s.notes}`,
    '',
    `- ${s.markdown}`,
    `- ${s.dated}`,
    ...(others.length > 0
      ? ['', `## ${s.languages}`, '', ...others.map((l) => `- ${fill(s.language, { name: LOCALE_META[l].name, url: at(l === DEFAULT_LOCALE ? '/llms.txt' : `/${l}/llms.txt`) })}`)]
      : []),
    '',
  ].join('\n')
}
