/*
  The releases as an Atom feed, for a reader that would rather be told than
  come and look. Each entry links to its anchor on `/updates` and carries the
  notes as HTML, rendered by the same Markdown pipeline as the page.
*/
import type { APIRoute } from 'astro'
import { publishedReleases, anchor } from '../releases.ts'

const esc = (s: string): string => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')

export const GET: APIRoute = async ({ site }) => {
  const releases = (await publishedReleases()).slice(0, 20)
  const page = new URL('/updates', site).href
  const entries = releases.map((r) => {
      // The glob loader renders a Markdown entry as it loads it.
      const notes = r.rendered?.html ?? ''
      const html = `<p>${esc(r.data.summary)}</p>${notes}`
      const when = r.data.date.toISOString()
      return [
        '  <entry>',
        `    <title>Frame Player ${r.data.version}</title>`,
        `    <id>${page}#${anchor(r.data.version)}</id>`,
        `    <link rel="alternate" type="text/html" href="${page}#${anchor(r.data.version)}"/>`,
        `    <updated>${when}</updated>`,
        `    <published>${when}</published>`,
        `    <summary>${esc(r.data.summary)}</summary>`,
        `    <content type="html">${esc(html)}</content>`,
        '  </entry>',
      ].join('\n')
    })

  const body = `<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Frame Player updates</title>
  <subtitle>What every version of Frame Player changed.</subtitle>
  <id>${page}</id>
  <link rel="alternate" type="text/html" href="${page}"/>
  <link rel="self" type="application/atom+xml" href="${new URL('/updates.xml', site).href}"/>
  <updated>${releases[0]?.data.date.toISOString() ?? new Date(0).toISOString()}</updated>
  <author><name>Frame Player</name></author>
${entries.join('\n')}
</feed>
`
  return new Response(body, { headers: { 'Content-Type': 'application/atom+xml; charset=utf-8' } })
}
