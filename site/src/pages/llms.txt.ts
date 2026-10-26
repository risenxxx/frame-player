/* `/llms.txt`, in English — see src/llms.ts. */
import type { APIRoute } from 'astro'
import { llmsText } from '../llms.ts'

export const GET: APIRoute = async ({ site }) =>
  new Response(await llmsText('en', site!), { headers: { 'Content-Type': 'text/plain; charset=utf-8' } })
