/*
  Every guide, as Markdown, at the page's own address plus `.md`. Linked from
  `/llms.txt` and what `worker/index.ts` hands back to anything asking for
  `text/markdown`.
*/
import type { APIRoute } from 'astro'
import { getCollection } from 'astro:content'
import { pageMarkdown } from '../markdown.ts'

export async function getStaticPaths() {
  const pages = await getCollection('pages')
  return pages.map((entry) => ({ params: { slug: entry.id }, props: { entry } }))
}

export const GET: APIRoute = ({ props, site }) => {
  const body = pageMarkdown(props.entry, site!)
  return new Response(body, { headers: { 'Content-Type': 'text/markdown; charset=utf-8' } })
}
