/**
 * The site's page counter: Rybbit, reached through our own address.
 *
 * The script takes its event endpoint from its own `src`, cutting at
 * `/script.js`, so one prefix on this domain is all both sides need to agree
 * on: the page loads `/s/script.js`, the script posts to `/s/track`, and the
 * worker (`worker/index.ts`) forwards the two to Rybbit. That keeps a content
 * blocker's host list from deciding what gets counted, and costs the browser
 * no second connection — the page just arrived over this one.
 *
 * The site id is public by nature (it is in every page's markup), so it lives
 * here rather than in the build environment.
 */
export const RYBBIT_SITE_ID = 'd7cf4cb48ba8'

/** Where Rybbit itself answers. The worker's only upstream. */
export const RYBBIT_UPSTREAM = 'https://app.rybbit.io/api'

/** Our own prefix for it. `run_worker_first` in wrangler.jsonc names it too. */
export const ANALYTICS_PREFIX = '/s/'

export const ANALYTICS_SRC = `${ANALYTICS_PREFIX}script.js?siteId=${RYBBIT_SITE_ID}`
