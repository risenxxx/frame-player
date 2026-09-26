/**
 * A guide as Markdown, for something that reads rather than renders.
 *
 * The source is already Markdown — an MDX file — so this is mostly subtraction:
 * the imports go, and the three components the pages use become the plain text
 * they stand for: a mock of an interface and a diagram become their captions,
 * because a caption is what the picture was saying, and a note becomes a
 * quote. Everything else in an MDX body is ordinary Markdown and travels as it
 * is — except a link, which is made absolute, since this text is read away
 * from the page it came from.
 *
 * What it is for: `/<page>.md` beside every page, listed in `/llms.txt` and
 * served to anything that asks for `text/markdown` (see `worker/index.ts`), so
 * an agent quoting this site quotes the text rather than a stripped rendering
 * of the page's markup.
 */
import type { CollectionEntry } from 'astro:content'

/** `<Tag …>body</Tag>` → whatever `render` makes of the body. */
function component(text: string, tag: string, render: (body: string, attrs: string) => string): string {
  const pattern = new RegExp(`<${tag}([^>]*)>([\\s\\S]*?)</${tag}>`, 'g')
  return text.replace(pattern, (_, attrs: string, body: string) => render(body.trim().replace(/\s*\n\s*/g, ' '), attrs))
}

export function pageMarkdown(entry: CollectionEntry<'pages'>, site: URL): string {
  const d = entry.data
  const url = new URL(`/${entry.id}`, site).href

  let body = entry.body ?? ''
  body = body.replace(/^import .*$/gm, '')
  body = component(body, 'Mock', (caption) => `*Figure: ${caption}*`)
  body = component(body, 'PieceMap', (caption) => `*Diagram: ${caption}*`)
  body = component(body, 'Note', (note) => `> **Note.** ${note}`)
  body = body.replace(/<p class="table-note">([\s\S]*?)<\/p>/g, '$1')
  // Whatever markup is left is the page's own HTML, which Markdown carries.
  body = body.replace(/\]\((\/[^)]*)\)/g, (_, path: string) => `](${new URL(path, site).href})`)
  body = body.replace(/\n{3,}/g, '\n\n').trim()

  const questions = d.faq.map(({ q, a }) => `### ${q}\n\n${a}`).join('\n\n')

  return [
    `# ${d.heading}`,
    '',
    `> ${d.lede}`,
    '',
    `Source: ${url}`,
    `Updated: ${d.updated.toISOString().slice(0, 10)}`,
    '',
    body,
    ...(questions ? ['', '## Questions', '', questions] : []),
    '',
  ].join('\n')
}
