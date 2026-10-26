/* `/ru/llms.txt` and `/zh/llms.txt` — see src/llms.ts. */
import type { APIRoute } from 'astro'
import { llmsText } from '../../llms.ts'
import { DEFAULT_LOCALE, enabledLocales, type Locale } from '../../i18n/index.ts'

export function getStaticPaths() {
  return enabledLocales()
    .filter((lang) => lang !== DEFAULT_LOCALE)
    .map((lang) => ({ params: { lang } }))
}

export const GET: APIRoute = async ({ params, site }) =>
  new Response(await llmsText(params.lang as Locale, site!), { headers: { 'Content-Type': 'text/plain; charset=utf-8' } })
