/**
 * The catalog: finding something to watch, and the release that carries it.
 *
 * Two questions with two answers, and keeping them apart is the design. TMDB
 * says *what* — posters, localised titles, how many seasons — and an indexer
 * says *where from*: trackers, quality, dubs, seeders, a magnet. Both calls are
 * made in Rust (`catalog.rs`), for the `User-Agent` and to keep an arbitrary
 * user-supplied host out of the webview's fetch surface.
 *
 * **Metadata goes through our own proxy** (`services/tmdb`), which holds the
 * TMDB key — the player carries none. Posters are the exception and go straight
 * to TMDB's CDN wherever that works, which is most places and is where the
 * bandwidth actually is; see `posterUrl`.
 *
 * **This module composes rather than reimplements.** A chosen release is a
 * magnet, and a magnet already has one road into the player — `openTorrent`,
 * with its resolve, its file picker, its history and its queue. Nothing here
 * touches the torrent client.
 *
 * Both addresses are settings rather than constants for the same reason the
 * relay's is: running your own is a setting, not a fork. Empty means the
 * default, so clearing a field restores it instead of turning the feature off —
 * there is a separate switch for that.
 */

import { invoke } from '@tauri-apps/api/core';

import { locale, t } from './i18n.svelte';
import { latest } from './latest';
import { openTorrent } from './open.svelte';

/**
 * The public JacRed instance, measured working without a key: 3.2 million
 * releases across fourteen trackers, updated daily, answering `/api/v1.0/torrents`
 * with the release name already parsed into title, year, quality and dubs.
 *
 * It is somebody else's server and it sees what is searched for, which is what
 * the setting's hint says and why the field is there at all.
 */
export const DEFAULT_INDEXER = 'https://api.jacred.su';

/**
 * Our own TMDB proxy (`services/tmdb`), which is where the API key lives.
 *
 * The player carries no TMDB key at all — see the note at the top of
 * `catalog.rs` for why that is a reading of their terms rather than caution.
 * Self-hosting is a setting, so this is a default and not a constant.
 */
export const DEFAULT_TMDB = 'https://tmdb.frameplayer.app';

/**
 * TMDB's own image CDN, tried first for every poster.
 *
 * **The cheapest byte is the one that does not go through us.** Their CDN is
 * closer to the viewer than any server of ours, and a grid of twenty posters is
 * ~800 KB against ~25 KB of JSON — so proxying images by default would be
 * paying for the expensive nine tenths of the traffic in order to serve the
 * minority who need it.
 */
const TMDB_CDN = 'https://image.tmdb.org/t/p';

/// w342 covers the ~150px cards on a 2× display; w500 doubles the bytes for
/// nothing on screen.
const POSTER_SIZE = 'w342';

const INDEXER_KEY = 'frameplayer.indexer';
const TMDB_KEY = 'frameplayer.tmdb';
const ENABLED_KEY = 'frameplayer.catalog';
/// Whether TMDB's CDN was reachable last time. Remembered so a viewer behind a
/// block pays the discovery once rather than on every launch.
const CDN_KEY = 'frameplayer.tmdbCdn';

/// One title as the catalog knows it.
export interface CatalogItem {
  kind: 'movie' | 'tv';
  id: number;
  title: string;
  original_title: string;
  year: number | null;
  poster: string | null;
  overview: string;
  rating: number;
}

export interface CatalogDetails extends CatalogItem {
  seasons: number[];
  runtime: number | null;
  genres: string[];
}

export interface Release {
  title: string;
  tracker: string;
  size: number;
  seeders: number;
  peers: number;
  quality: number;
  video_type: string;
  voices: string[];
  seasons: number[];
  magnet: string;
  created: string;
}

/**
 * Where the panel is. Three states rather than a boolean, because "searching"
 * and "found nothing" are different things to a viewer and the panel has to say
 * which — an empty grid with no explanation reads as the feature being broken.
 */
export type CatalogPhase = 'idle' | 'loading' | 'ready' | 'failed';

class Catalog {
  /**
   * Whether the catalog is offered at all.
   *
   * **Off by default, and that is a decision rather than caution.** Everything
   * else in this player acts on something the viewer already has; the catalog
   * is the first surface that sends a question about what they *want* to watch
   * to a third party. Turning it on is a sentence in the settings and one
   * click, and the choice belongs to the viewer rather than to the installer.
   *
   * Reactive rather than a function reading localStorage at each call site: the
   * start screen's button appears and disappears with it, and a getter would
   * leave that button standing until something else happened to redraw.
   */
  enabled = $state(readEnabled());

  /// The panel is up. Here rather than in `overlays` for the same reason the
  /// link box is: this belongs to the flow that opens things, and `closeTopmost`
  /// reaches for it rather than owning it.
  open = $state(false);

  /// A metadata proxy answered, so there are posters and descriptions. Without
  /// one the panel falls back to searching the indexer by the typed text —
  /// degraded, not absent, exactly as subtitle search degrades without a key.
  hasMeta = $state(true);

  /// Posters load straight from TMDB's CDN. Flipped by the first image that
  /// fails, which is what a blocked CDN looks like from inside a webview.
  cdnDirect = $state(readCdn());

  query = $state('');
  results = $state<CatalogItem[]>([]);
  /// What the grid is showing when the query is empty: the trending list, which
  /// is fetched once per session and then reused.
  trending = $state<CatalogItem[]>([]);
  phase = $state<CatalogPhase>('idle');
  error = $state<string | null>(null);

  /// The title whose page is open, or null while the grid is.
  picked = $state<CatalogDetails | null>(null);
  /// Which season the release list is filtered to. Null for a film, and for a
  /// series it starts at the first season rather than at "all": a season pack
  /// list mixed with nine per-episode lists is unreadable.
  season = $state<number | null>(null);

  releases = $state<Release[]>([]);
  releasePhase = $state<CatalogPhase>('idle');
  releaseError = $state<string | null>(null);

  /// A release is being handed to the torrent client. The panel stays up and
  /// says so — resolving a magnet is a DHT lookup and the row would otherwise
  /// look untouched for a second or more.
  starting = $state<string | null>(null);

  /// What the grid actually draws: the search results while there is a query,
  /// the trending list while there is not.
  shown = $derived(this.query.trim() ? this.results : this.trending);
}

export const catalog = new Catalog();

/// Only the newest search may write — a slow first query landing after a
/// narrower second one would leave the grid permanently showing the wrong list.
const searches = latest();
const releaseReads = latest();

// ---- The setting ----------------------------------------------------------

export function indexerUrl(): string {
  try {
    return localStorage.getItem(INDEXER_KEY) ?? DEFAULT_INDEXER;
  } catch {
    return DEFAULT_INDEXER;
  }
}

export function setIndexerUrl(url: string) {
  try {
    const clean = url.trim().replace(/\/+$/, '');
    if (clean) localStorage.setItem(INDEXER_KEY, clean);
    else localStorage.removeItem(INDEXER_KEY);
  } catch {
    // not critical: the address simply will not survive a restart
  }
}

export function tmdbUrl(): string {
  try {
    return localStorage.getItem(TMDB_KEY) ?? DEFAULT_TMDB;
  } catch {
    return DEFAULT_TMDB;
  }
}

export function setTmdbUrl(url: string) {
  try {
    const clean = url.trim().replace(/\/+$/, '');
    if (clean) localStorage.setItem(TMDB_KEY, clean);
    else localStorage.removeItem(TMDB_KEY);
  } catch {
    // not critical
  }
}

// ---- Posters --------------------------------------------------------------

/**
 * Where a poster comes from, and how that gets decided.
 *
 * Direct from TMDB's CDN by default, because that is free for us and fastest
 * for the viewer. When an image fails to load — which is what a blocked CDN
 * looks like from inside a webview — the verdict flips to the proxy and the
 * grid re-renders against it.
 *
 * **Trying is the measurement; an IP address is only a guess about it.**
 * Geolocating the client would need a database, would be wrong for anyone on a
 * VPN or a corporate network, and would make the service derive somebody's
 * location from their address in order to infer a fact the browser can simply
 * report. What matters is not where you are, it is whether this webview loads
 * that URL.
 *
 * The verdict is remembered, so the discovery is paid once rather than on every
 * launch. It is re-examined on a `direct` verdict only: a block that has been
 * lifted costs one failed image to find out about, while re-testing a working
 * CDN on every launch costs nothing to nobody.
 */
function readCdn(): boolean {
  try {
    return localStorage.getItem(CDN_KEY) !== 'blocked';
  } catch {
    return true;
  }
}

export function posterUrl(path: string | null): string | null {
  if (!path) return null;
  if (catalog.cdnDirect) return `${TMDB_CDN}/${POSTER_SIZE}${path}`;
  // `path` is TMDB's own `/abc.jpg`, and the proxy validates both halves again
  // against a strict pattern before anything reaches its filesystem.
  return `${tmdbUrl()}/img/${POSTER_SIZE}${path}`;
}

/**
 * How many posters must fail before the CDN is declared unreachable.
 *
 * **One is not enough, and finding that out was worth the whole exercise.** An
 * `<img>` cannot tell a refused connection from a 404, and TMDB's CDN does
 * answer 404 for a path that has gone away — measured. So a single missing
 * poster would otherwise convict a perfectly reachable CDN and push every image
 * in the player through the proxy, permanently and silently. A blocked CDN
 * fails *every* image in a grid of twenty; a stale path fails one.
 */
const CDN_STRIKES = 3;
let cdnFailures = 0;

/**
 * A poster loaded. Resets the strike count, which is what keeps stray 404s from
 * accumulating into a false verdict over a long session — a blocked CDN never
 * gets here, so only a working one can clear the count.
 */
export function notePosterOk() {
  cdnFailures = 0;
}

/**
 * A poster failed to load.
 *
 * Once the verdict is `blocked` the `<img>` tags are rebuilt against the proxy,
 * so a failure after that is the proxy's problem and not the CDN's — flipping
 * back would put the two in a loop.
 */
export function notePosterFailed() {
  if (!catalog.cdnDirect) return;
  if (++cdnFailures < CDN_STRIKES) return;
  catalog.cdnDirect = false;
  try {
    localStorage.setItem(CDN_KEY, 'blocked');
  } catch {
    // not critical: the discovery is simply repeated next launch
  }
}

function readEnabled(): boolean {
  try {
    return localStorage.getItem(ENABLED_KEY) === 'on';
  } catch {
    return false;
  }
}

export function setCatalogEnabled(on: boolean) {
  catalog.enabled = on;
  try {
    if (on) localStorage.setItem(ENABLED_KEY, 'on');
    else localStorage.removeItem(ENABLED_KEY);
  } catch {
    // not critical: the choice simply will not survive a restart
  }
  // A switch that leaves the panel it just disabled standing on screen has not
  // finished doing what it says.
  if (!on) catalog.open = false;
}

// ---- Browsing -------------------------------------------------------------

/**
 * Raise the panel.
 *
 * The trending list is fetched here rather than at startup, and that is the
 * start screen's own rule applied one level down: nothing may leave this
 * machine because the player was launched. It is also cached for the session,
 * so re-opening the panel is instant.
 */
export async function openCatalog() {
  catalog.open = true;
  catalog.picked = null;
  catalog.releases = [];
  catalog.releasePhase = 'idle';
  catalog.hasMeta = await invoke<boolean>('catalog_ready', { proxy: tmdbUrl() }).catch(() => false);
  if (!catalog.hasMeta || catalog.trending.length) {
    if (catalog.phase === 'idle') catalog.phase = 'ready';
    return;
  }
  const run = searches.begin();
  catalog.phase = 'loading';
  try {
    const list = await invoke<CatalogItem[]>('catalog_trending', { proxy: tmdbUrl(), locale: locale() });
    if (run.stale) return;
    catalog.trending = list;
    catalog.phase = 'ready';
  } catch (e) {
    if (run.stale) return;
    catalog.phase = 'failed';
    catalog.error = describe(e);
  }
}

/**
 * Run the typed query.
 *
 * Without TMDB there is nothing to search *for* — the panel goes straight to the
 * indexer with the raw text and shows releases, which is the whole of the
 * keyless fallback.
 */
export async function runSearch() {
  const query = catalog.query.trim();
  if (!query) {
    catalog.results = [];
    catalog.phase = catalog.trending.length ? 'ready' : 'idle';
    return;
  }
  if (!catalog.hasMeta) {
    await loadReleases({ title: query, original: query, year: null, season: null });
    return;
  }
  const run = searches.begin();
  catalog.phase = 'loading';
  catalog.error = null;
  try {
    const list = await invoke<CatalogItem[]>('catalog_search', {
      proxy: tmdbUrl(),
      query,
      locale: locale(),
    });
    if (run.stale) return;
    catalog.results = list;
    catalog.phase = 'ready';
  } catch (e) {
    if (run.stale) return;
    catalog.phase = 'failed';
    catalog.error = describe(e);
  }
}

/**
 * Open one title's page: its details, and the releases for it.
 *
 * The two are fetched together rather than the second on demand, because a page
 * whose only content is a button that fetches the content is a page with a step
 * in it — what the viewer came for is the release list.
 */
export async function pickTitle(item: CatalogItem) {
  const run = searches.begin();
  catalog.picked = { ...item, seasons: [], runtime: null, genres: [] };
  catalog.releases = [];
  catalog.releasePhase = 'loading';
  try {
    const details = await invoke<CatalogDetails>('catalog_details', {
      proxy: tmdbUrl(),
      kind: item.kind,
      id: item.id,
      locale: locale(),
    });
    if (run.stale) return;
    catalog.picked = details;
    catalog.season = details.seasons.length ? details.seasons[0] : null;
  } catch {
    if (run.stale) return;
    // Details failing is survivable: the grid row already carries a title, a
    // year and a poster, which is everything the release lookup needs. Losing
    // the season list costs a series its picker and nothing else.
    catalog.season = null;
  }
  await loadReleases({
    title: catalog.picked?.title ?? item.title,
    original: catalog.picked?.original_title ?? item.original_title,
    year: catalog.picked?.year ?? item.year,
    season: catalog.season,
  });
}

/// Re-read the release list for another season of the same series.
export async function chooseSeason(season: number | null) {
  const picked = catalog.picked;
  if (!picked) return;
  catalog.season = season;
  await loadReleases({
    title: picked.title,
    original: picked.original_title,
    year: picked.year,
    season,
  });
}

async function loadReleases(args: {
  title: string;
  original: string;
  year: number | null;
  season: number | null;
}) {
  const run = releaseReads.begin();
  catalog.releasePhase = 'loading';
  catalog.releaseError = null;
  try {
    const list = await invoke<Release[]>('catalog_releases', {
      base: indexerUrl(),
      title: args.title,
      originalTitle: args.original,
      year: args.year,
      season: args.season,
    });
    if (run.stale) return;
    catalog.releases = list;
    catalog.releasePhase = 'ready';
  } catch (e) {
    if (run.stale) return;
    catalog.releases = [];
    catalog.releasePhase = 'failed';
    catalog.releaseError = describe(e);
  }
}

/**
 * Shut the panel.
 *
 * The query and the results are kept: a viewer who opened a title, found the
 * release list unconvincing and closed the sheet is one keystroke from being
 * back where they were, and re-running the search would be a second request for
 * an answer already in hand. The trending list is cached for the same reason.
 */
export function closeCatalog() {
  catalog.open = false;
}

/// Back to the grid from a title's page.
export function closeTitle() {
  catalog.picked = null;
  catalog.releases = [];
  catalog.releasePhase = 'idle';
  catalog.season = null;
}

/**
 * Play a chosen release.
 *
 * The magnet goes to the one road torrents already have. Everything downstream
 * — resolving, the file picker for a season, the watch history, the queue — is
 * `openTorrent`'s and is not reimplemented here.
 */
export async function playRelease(release: Release, close: () => void) {
  if (catalog.starting) return;
  catalog.starting = release.magnet;
  try {
    close();
    await openTorrent(release.magnet);
  } finally {
    catalog.starting = null;
  }
}

/**
 * Turn a Rust error into a sentence.
 *
 * The three that are worth telling apart: no indexer configured, the indexer
 * unreachable, and this build having no TMDB key. Everything else is the
 * network, and repeating a status code at somebody helps nobody.
 */
function describe(e: unknown): string {
  const raw = String(e);
  if (raw.includes('no_indexer')) return t('catalog.no_indexer');
  if (raw.includes('no_proxy')) return t('catalog.no_proxy');
  // Two different mistakes in the same field, and telling them apart is the
  // difference between "fix the address" and "this address cannot be used".
  if (raw.includes('insecure_proxy')) return t('catalog.insecure_proxy');
  if (raw.includes('bad_proxy')) return t('catalog.bad_proxy');
  if (raw.includes('http_')) return t('catalog.unreachable');
  return t('catalog.failed');
}

/// A release's one-line summary: quality, dynamic range, size, dubs.
export function releaseTags(r: Release): string[] {
  const tags: string[] = [];
  if (r.quality > 0) tags.push(r.quality >= 2160 ? '4K' : `${r.quality}p`);
  const dynamic = r.video_type.toLowerCase();
  if (dynamic === 'hdr' || dynamic === 'dv') tags.push(dynamic.toUpperCase());
  return tags;
}
