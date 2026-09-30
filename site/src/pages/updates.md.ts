/*
  `/updates` as Markdown — its twin, like every guide's (see [...slug].md.ts),
  and what `worker/index.ts` answers with to a request for `text/markdown`.
  The release files are Markdown already, so this is them, in order, under a
  heading each.
*/
import type { APIRoute } from 'astro'
import { publishedReleases } from '../releases.ts'

export const GET: APIRoute = async ({ site }) => {
  const releases = await publishedReleases()
  const body = [
    '# Frame Player updates',
    '',
    '> What every version of Frame Player changed, newest first.',
    '',
    `Source: ${new URL('/updates', site).href}`,
    '',
    ...releases.flatMap((r) => [
      `## ${r.data.version} (${r.data.date.toISOString().slice(0, 10)})`,
      '',
      r.data.summary,
      '',
      // One level down: under a release's own heading its sections are h3 already.
      ...(r.body?.trim() ? [r.body.trim(), ''] : []),
    ]),
  ].join('\n')
  return new Response(body, { headers: { 'Content-Type': 'text/markdown; charset=utf-8' } })
}
