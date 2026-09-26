/*
  The site in one file, for something that reads it rather than renders it —
  the llms.txt convention: a title, a summary, and one line per page saying what
  is there. Every link points at that page's Markdown twin (`[...slug].md.ts`),
  because a link an agent follows should give it text and not markup.

  Generated, like the sitemap, so a new page appears here by existing.
*/
import type { APIRoute } from 'astro'
import { getCollection } from 'astro:content'
import { release } from '../release.ts'

export const GET: APIRoute = async ({ site }) => {
  const at = (path: string): string => new URL(path, site).href
  const pages = (await getCollection('pages')).sort((a, b) => a.data.order - b.data.order)
  const { version, date } = await release()

  const group = (label: string, prefix: (id: string) => boolean): string[] => {
    const rows = pages.filter((p) => prefix(p.id))
    if (rows.length === 0) return []
    return ['', `## ${label}`, '', ...rows.map((p) => `- [${p.data.short}](${at(`/${p.id}.md`)}): ${p.data.description}`)]
  }

  const body = [
    '# Frame Player',
    '',
    '> A free, open-source video player for Windows and macOS, built on libmpv. It plays local files,',
    '> links and magnet torrents, streams a torrent while it downloads, sends a file to a Chromecast or',
    '> a DLNA television without re-encoding it, and keeps several people on one timeline in a room.',
    '',
    `Version ${version}${date ? `, released ${date.slice(0, 10)}` : ''}. Licence: GPL-3.0-or-later.`,
    'Windows 10 and 11 (x64), macOS on Apple Silicon. No account, no telemetry.',
    '',
    '## Start here',
    '',
    `- [Home](${at('/')}): what the player is, with every feature in short.`,
    `- [Download for Windows](${at('/download/windows')}) · [Download for macOS](${at('/download/macos')}): always the current release.`,
    ...group('Features', (id) => !id.includes('/')),
    ...group('Comparisons', (id) => id.startsWith('compare/')),
    ...group('Guides', (id) => id.startsWith('guides/')),
    '',
    '## Project',
    '',
    '- [Source code](https://github.com/risenxxx/frame-player): Rust, Svelte and Go, GPL-3.0-or-later.',
    '- [Documentation](https://github.com/risenxxx/frame-player/tree/main/docs): how the player works inside, and why.',
    '- [Releases](https://github.com/risenxxx/frame-player/releases): notes and installers for every version.',
    '',
    '## Notes',
    '',
    '- Every page here is also served as Markdown: add `.md` to its address, or ask for `text/markdown`.',
    '- Pages carry the date they were last checked against the player; quote that date with the page.',
    '',
  ].join('\n')

  return new Response(body, { headers: { 'Content-Type': 'text/plain; charset=utf-8' } })
}
