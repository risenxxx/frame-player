import { readFile } from 'node:fs/promises'
import { resolve } from 'node:path'

/**
 * Where the download buttons point, resolved once at build time.
 *
 * The release workflow uploads three files per version to the update bucket,
 * flat, with the version in every name:
 *
 *     FramePlayer_<version>_x64-setup.exe        the Windows installer
 *     FramePlayer_<version>_aarch64.dmg          the macOS disk image
 *     FramePlayer_<version>_aarch64.app.tar.gz   what the updater installs
 *
 * …and then `latest.json`, last, once the binaries are up. So the manifest is
 * the authority on which version exists, and the file names follow from it.
 * The manifest's macOS URL points at the `.app.tar.gz` — that is the updater's
 * payload, not a thing a person should be handed, so the image is composed from
 * the version instead.
 *
 * Old versions are pruned from the bucket after `latest.json` names the new
 * one, which is why this is resolved at build time from the manifest rather
 * than written down: a hard-coded link would survive exactly one release. The
 * site is rebuilt at the end of a release for the same reason.
 *
 * Without `UPDATES_ORIGIN` — a local build, a fork, a preview — the buttons
 * fall back to the GitHub release page, which is always correct and never
 * stale. The version then comes from the application's own manifest, so the
 * page never invents a number.
 */
const GITHUB = 'https://github.com/risenxxx/frame-player'

export interface Release {
  version: string
  windows: string
  macos: string
  /** When that version was published, ISO 8601, from `latest.json`'s `pub_date`.
      Null when the manifest could not be read — a date is either known or not
      claimed. */
  date: string | null
  /** True when the buttons hand over a file rather than a release page. */
  direct: boolean
}

/*
  Resolved against the working directory, not against `import.meta.url`: this
  module is bundled into `dist/pages/` before it runs, so a path relative to the
  module would climb out of the wrong directory. Astro builds with the site
  directory as the cwd, in CI as well.
*/
async function versionFromRepo(): Promise<string> {
  const conf = resolve(process.cwd(), '..', 'src-tauri', 'tauri.conf.json')
  const { version } = JSON.parse(await readFile(conf, 'utf8'))
  return version
}

/**
 * The addresses that stay put across releases. `astro.config.ts` writes them
 * into `_redirects`, pointing at whatever `release()` resolved.
 */
export const DOWNLOAD_PATHS = { windows: '/download/windows', macos: '/download/macos' } as const

/**
 * What the buttons link to: the stable addresses rather than the files, so a
 * link somebody copies off the page still works after the next release.
 * `astro dev` does not serve `_redirects`, so there the buttons keep the
 * resolved targets instead of pointing at a 404.
 */
export async function downloads(): Promise<Release> {
  const resolved = await release()
  return import.meta.env.DEV ? resolved : { ...resolved, ...DOWNLOAD_PATHS }
}

export async function release(): Promise<Release> {
  const origin = process.env.UPDATES_ORIGIN?.replace(/\/+$/, '')
  const fallback = async (): Promise<Release> => ({
    version: await versionFromRepo(),
    windows: `${GITHUB}/releases/latest`,
    macos: `${GITHUB}/releases/latest`,
    date: null,
    direct: false,
  })

  if (!origin) return fallback()

  try {
    const res = await fetch(`${origin}/latest.json`, { signal: AbortSignal.timeout(8000) })
    if (!res.ok) throw new Error(`latest.json: ${res.status}`)
    const manifest = (await res.json()) as { version?: string; pub_date?: string }
    const version = manifest.version
    if (!version) throw new Error('latest.json carries no version')
    return {
      version,
      windows: `${origin}/FramePlayer_${version}_x64-setup.exe`,
      macos: `${origin}/FramePlayer_${version}_aarch64.dmg`,
      date: manifest.pub_date ?? null,
      direct: true,
    }
  } catch (error) {
    // A build must not fail because the update host is having a moment.
    console.warn(`[site] falling back to the GitHub release page: ${String(error)}`)
    return fallback()
  }
}
