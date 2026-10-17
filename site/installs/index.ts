/**
 * The installation counter: one Durable Object holding how many first launches
 * the player has reported, reached only through the site's worker (a service
 * binding — this worker has no address of its own).
 *
 * It lives apart from the site because a worker that implements a Durable
 * Object gets no preview addresses, and the site's pull requests depend on them.
 *
 * What it keeps: the count, the count per platform, and — for the current UTC
 * day only — a salted hash of each address that has reported, so that a script
 * looping on the endpoint adds one a day rather than one a request. The salt is
 * random, made fresh each day and thrown away with that day's hashes, so not
 * even this worker can tell afterwards which address reported. No address is
 * ever stored.
 */
import { INSTALLS_BASE } from '../src/installs.ts'

interface Storage {
  get<T>(key: string): Promise<T | undefined>
  put(key: string, value: unknown): Promise<void>
  delete(keys: string[]): Promise<number>
  list<T>(options: { prefix: string }): Promise<Map<string, T>>
}

interface State {
  storage: Storage
}

interface Env {
  COUNTER: {
    idFromName(name: string): unknown
    get(id: unknown): { fetch(request: Request): Promise<Response> }
  }
}

const PLATFORMS = ['windows', 'macos', 'other'] as const

async function hash(salt: string, address: string): Promise<string> {
  const bytes = new TextEncoder().encode(`${salt}:${address}`)
  const digest = await crypto.subtle.digest('SHA-256', bytes)
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('')
}

export class InstallCounter {
  constructor(
    private state: State,
    _env: unknown,
  ) {}

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url)
    if (url.pathname === '/count') return Response.json(await this.count())
    if (url.pathname === '/install' && request.method === 'POST') return this.install(request)
    return new Response('not found', { status: 404 })
  }

  private async count(): Promise<{ installs: number; reported: number; byPlatform: Record<string, number> }> {
    const reported = (await this.state.storage.get<number>('n')) ?? 0
    const byPlatform: Record<string, number> = {}
    for (const p of PLATFORMS) byPlatform[p] = (await this.state.storage.get<number>(`n:${p}`)) ?? 0
    return { installs: INSTALLS_BASE + reported, reported, byPlatform }
  }

  private async install(request: Request): Promise<Response> {
    const address = request.headers.get('x-client-ip')
    if (!address) return new Response('no address', { status: 400 })

    let platform: (typeof PLATFORMS)[number] = 'other'
    try {
      const body = (await request.json()) as { platform?: unknown }
      const named = PLATFORMS.find((p) => p === body.platform)
      if (named) platform = named
    } catch {
      return new Response('bad body', { status: 400 })
    }

    const storage = this.state.storage
    const today = new Date().toISOString().slice(0, 10)
    let salt = await storage.get<string>('salt')
    if ((await storage.get<string>('day')) !== today || !salt) {
      const stale = await storage.list({ prefix: 'seen:' })
      if (stale.size > 0) await storage.delete([...stale.keys()])
      salt = crypto.randomUUID()
      await storage.put('salt', salt)
      await storage.put('day', today)
    }

    const seen = `seen:${await hash(salt, address)}`
    if (await storage.get(seen)) return new Response('already counted today', { status: 429 })
    await storage.put(seen, 1)
    // A Durable Object runs one request at a time between awaits on its own
    // storage, so these read-then-write pairs cannot interleave.
    await storage.put('n', ((await storage.get<number>('n')) ?? 0) + 1)
    await storage.put(`n:${platform}`, ((await storage.get<number>(`n:${platform}`)) ?? 0) + 1)
    return new Response(null, { status: 204 })
  }
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const counter = env.COUNTER.get(env.COUNTER.idFromName('global'))
    return counter.fetch(request)
  },
}
