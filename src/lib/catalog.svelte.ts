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
import { showOsd } from './osd.svelte';
import { latest } from './latest';
import { openUpdateDialog, opening, openTorrent } from './open.svelte';
import type { RememberedTorrent } from './torrent.svelte';
import { player } from './player.svelte';

/// No indexer is compiled in. Where the catalog looks when the viewer has named
/// nowhere is asked for at runtime — see `suggested` and `catalog_config`.
export const DEFAULT_INDEXER = '';

/**
 * The configuration document, beside `latest.json` on the update host.
 *
 * A static file rather than an endpoint on the metadata proxy, deliberately: it
 * is then independent of whether that service is up, deployed or reachable, and
 * it sits on infrastructure the updater already depends on — so the one thing
 * that has to be changeable quickly does not inherit the availability of the
 * one thing that does the most work.
 *
 * Not a setting. The other three addresses are, because somebody self-hosting
 * has a reason to point them elsewhere; this one only ever fills a gap for
 * viewers who have set nothing, and a self-hoster sets their own indexer
 * instead.
 */
const CONFIG_URL = 'https://updates.frameplayer.app/catalog.json';

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
const SORT_KEY = 'frameplayer.releaseSort';
const FLOOR_KEY = 'frameplayer.releaseFloor';
const DEAD_KEY = 'frameplayer.releaseHideDead';

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

/// What the service suggests at runtime. Never persisted — see `suggested`.
export interface CatalogConfig {
  indexer: string;
  disabled: boolean;
  notice: string;
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
  url: string;
}

/**
 * Where the panel is. Three states rather than a boolean, because "searching"
 * and "found nothing" are different things to a viewer and the panel has to say
 * which — an empty grid with no explanation reads as the feature being broken.
 */
export type CatalogPhase = 'idle' | 'loading' | 'ready' | 'failed';

/**
 * How the release list is ordered.
 *
 * Three questions, not one ranking with three tie-breaks. `quality` answers
 * "the best copy", `seeders` answers "the copy that will actually download",
 * and `size` answers the one a season makes you ask: **which of these is the
 * whole thing rather than one episode.**
 *
 * That last one is why size is a first-class order rather than a tie-break. A
 * series comes back as a mixture of single-episode releases and packs, and the
 * only signal separating them that every source fills in is how big they are —
 * the episode-range markers a title may carry are written by a minority (31 of
 * 768 rows in one measured response). Combined with a quality floor it is a
 * precise instrument: "1080p and above, biggest first" is the whole-season
 * query, and it needs no field the indexer does not have.
 */
export type ReleaseSort = 'quality' | 'seeders' | 'size';

/// A floor on quality. `0` is no floor; the rest are the values the indexer
/// reports, and a release whose quality it could not read (`0`) is only ever
/// shown when there is no floor at all — filtering it in would let an unknown
/// masquerade as whatever the viewer asked for.
export type QualityFloor = 0 | 720 | 1080 | 2160;

function readSort(): ReleaseSort {
  try {
    const v = localStorage.getItem(SORT_KEY);
    return v === 'seeders' || v === 'size' ? v : 'quality';
  } catch {
    return 'quality';
  }
}

export function setReleaseSort(next: ReleaseSort) {
  catalog.sort = next;
  try {
    if (next === 'quality') localStorage.removeItem(SORT_KEY);
    else localStorage.setItem(SORT_KEY, next);
  } catch {
    // not critical: the choice simply will not survive a restart
  }
}

function readFloor(): QualityFloor {
  try {
    const v = Number(localStorage.getItem(FLOOR_KEY));
    return v === 720 || v === 1080 || v === 2160 ? v : 0;
  } catch {
    return 0;
  }
}

export function setQualityFloor(next: QualityFloor) {
  catalog.floor = next;
  try {
    if (next === 0) localStorage.removeItem(FLOOR_KEY);
    else localStorage.setItem(FLOOR_KEY, String(next));
  } catch {
    // not critical
  }
}

export function setHideDead(on: boolean) {
  catalog.hideDead = on;
  try {
    if (on) localStorage.setItem(DEAD_KEY, 'on');
    else localStorage.removeItem(DEAD_KEY);
  } catch {
    // not critical
  }
}

function readHideDead(): boolean {
  try {
    return localStorage.getItem(DEAD_KEY) === 'on';
  } catch {
    return false;
  }
}

class Catalog {
  /**
   * Whether the catalog is offered at all.
   *
   * **On by default, with the switch kept.** It started off by default, on the
   * argument that this is the only surface in the player that tells a third
   * party what somebody is *looking for* rather than acting on a file they
   * already hold. That argument is still true and it is why the switch exists —
   * but it is an argument for a way out, not for hiding the feature from
   * everybody who never opens the settings. A default is a guess about what
   * most people want; a switch is the answer for the rest.
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

  /**
   * Where the metadata service suggests looking, when the viewer has not said.
   *
   * **In memory only, and deliberately.** The whole reason this is remote
   * configuration rather than a constant is that the instance's answer is the
   * current one; caching it on disk would pin whatever it happened to be the
   * first time somebody opened the panel, which is the per-build constant this
   * replaced, reintroduced by the client.
   */
  suggested = $state('');

  /// The service reports the catalog as unavailable, with an optional sentence
  /// saying why. One level above the address: an instance may need the panel to
  /// stop for reasons unrelated to which indexer it points at.
  suppressed = $state(false);
  suppressedNotice = $state('');

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

  /// The torrent whose update is being looked for, so the row can say so and a
  /// second click cannot start a parallel lookup.
  checking = $state<string | null>(null);

  /**
   * How the release list is ordered.
   *
   * `quality` is what Rust already sorted by and what the list arrives in — 4K
   * HDR first, seeders inside each group. `seeders` is the other legitimate
   * question ("what will actually download"), and it is a different one rather
   * than a worse one, which is why it is a control instead of a compromise
   * between the two.
   */
  sort = $state<ReleaseSort>(readSort());

  /// The lowest quality worth listing, and whether unseeded releases are shown
  /// at all. Remembered for the same reason the order is: these are preferences
  /// about how somebody chooses, not properties of one search.
  floor = $state<QualityFloor>(readFloor());
  hideDead = $state(readHideDead());

  /**
   * What survives the filters, before ordering.
   *
   * A quality floor deliberately drops releases whose quality the indexer could
   * not read, because keeping them would let an unknown pass as whatever was
   * asked for — but only when a floor is set at all, so the default view still
   * shows everything.
   */
  visibleReleases = $derived(
    this.releases.filter(
      (r) => (!this.floor || r.quality >= this.floor) && (!this.hideDead || r.seeders > 0),
    ),
  );

  /// How many releases the filters removed, so the panel can say so. An empty
  /// list under an active filter must not read the same as an empty answer from
  /// the indexer — one of those is the viewer's own doing and undoable.
  filteredOut = $derived(this.releases.length - this.visibleReleases.length);

  /// What the grid actually draws: the search results while there is a query,
  /// the trending list while there is not.
  shown = $derived(this.query.trim() ? this.results : this.trending);

  /**
   * The release list in the chosen order.
   *
   * A copy, never a sort in place: `sort()` mutates, and mutating the `$state`
   * array from inside a `$derived` is a write during a read — which is the
   * shape that made the ScrollFade effect re-run itself forever.
   *
   * It can only ever re-order what Rust sent, which is the top `MAX_RELEASES`
   * by the quality order — and **that cap does bind on a popular title**:
   * measured against the live indexer, `Dune` answered 1444 rows, 326 of which
   * survived the name and year filter, against a cap of 120. So "by seeders"
   * re-orders the best 120 by quality rather than the best 120 by seeders, and
   * a live 480p rip outside that window is not recoverable here. Raising the
   * cap is not the fix — a list nobody reads to the end is not more useful —
   * but if this ever matters, the honest answer is to make the cap a property
   * of the requested order rather than of the response.
   */
  sortedReleases = $derived.by(() => {
    const rows = this.visibleReleases;
    // Already in quality order from Rust, so the common case copies nothing.
    if (this.sort === 'quality') return rows;
    // A copy, never a sort in place: `sort()` mutates, and mutating the `$state`
    // array from inside a `$derived` is a write during a read — the shape that
    // made the ScrollFade effect re-run itself forever.
    if (this.sort === 'seeders') {
      return [...rows].sort(
        (a, b) => b.seeders - a.seeders || b.quality - a.quality || b.size - a.size,
      );
    }
    // Size, and **a dead release still sinks**. Otherwise the largest thing in
    // the list — which is exactly what somebody hunting a whole season is
    // reaching for — is routinely one nobody is seeding, and the order would
    // put the single most disappointing row first.
    return [...rows].sort(
      (a, b) =>
        Number(b.seeders > 0) - Number(a.seeders > 0) ||
        b.size - a.size ||
        b.seeders - a.seeders,
    );
  });
}

export const catalog = new Catalog();

/// Only the newest search may write — a slow first query landing after a
/// narrower second one would leave the grid permanently showing the wrong list.
const searches = latest();
const releaseReads = latest();

// ---- The setting ----------------------------------------------------------

/// What the viewer has set, and nothing else. Empty when they have not.
export function indexerUrl(): string {
  try {
    return localStorage.getItem(INDEXER_KEY) ?? DEFAULT_INDEXER;
  } catch {
    return DEFAULT_INDEXER;
  }
}

/**
 * Where releases are actually looked for.
 *
 * The viewer's own setting always wins; the service's suggestion only fills a
 * gap. That order is what keeps a configuration change from affecting somebody
 * who chose their own indexer.
 */
export function effectiveIndexer(): string {
  return indexerUrl() || catalog.suggested;
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

/**
 * **The stored value is the refusal, not the consent**, which is what makes the
 * default flip cost no migration: an absent key means on, and `off` is the only
 * thing ever written. Anyone who had turned the catalog on under the previous
 * default holds the string `on`, which is not `off` and therefore still reads
 * as enabled — so nobody's choice changes underneath them, in either direction.
 */
function readEnabled(): boolean {
  try {
    return localStorage.getItem(ENABLED_KEY) !== 'off';
  } catch {
    return true;
  }
}

export function setCatalogEnabled(on: boolean) {
  catalog.enabled = on;
  try {
    if (on) localStorage.removeItem(ENABLED_KEY);
    else localStorage.setItem(ENABLED_KEY, 'off');
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

  // **Set before the first `await`, not after it.** The panel is on screen the
  // moment `open` flips, and `idle` renders the "start typing" placeholder —
  // so with the phase left alone until the requests below have finished, the
  // first thing a viewer saw was an empty panel telling them to type,
  // replaced a moment later by the skeletons. Not a flicker either: the two
  // calls that follow are a network round trip and a reachability check with a
  // four-second timeout, so on a slow link that wrong answer stood for
  // seconds.
  //
  // Only when there is nothing to show yet. A reopened panel still holds its
  // trending list or its results, and those render immediately — announcing a
  // load in front of content that is already there would be its own flash.
  if (!catalog.trending.length && !catalog.query.trim()) catalog.phase = 'loading';

  // Asked on **every** opening rather than once per session: a configuration
  // change has to reach a player that is already running, and somebody who
  // leaves the app open for a week would otherwise be on last week's answer.
  // The service caches its own reply, so this costs a round trip and nothing
  // else.
  const config = await invoke<CatalogConfig>('catalog_config', { url: CONFIG_URL }).catch(
    () => null,
  );
  catalog.suggested = config?.indexer ?? '';
  catalog.suppressed = config?.disabled ?? false;
  catalog.suppressedNotice = config?.notice ?? '';

  catalog.hasMeta = await invoke<boolean>('catalog_ready', { proxy: tmdbUrl() }).catch(() => false);
  if (!catalog.hasMeta || catalog.trending.length) {
    // Unconditionally, because the phase set above is now `loading` rather than
    // `idle` — the old guard tested for `idle` and would leave the skeletons up
    // for good on the path where there is nothing to fetch. Nothing is loading
    // on either branch here: the trending list is already in hand, or there is
    // no metadata service to ask.
    catalog.phase = 'ready';
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
      base: effectiveIndexer(),
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
    // **Raised before the magnet is handed over, not after.** Closing the panel
    // leaves the start screen with nothing happening on it, while resolving a
    // magnet is a DHT lookup — measured elsewhere in this project at 1.5 to 10
    // seconds on a cold torrent. Without this the player looked like it had
    // simply swallowed the click, which is the same silence `openRecent` fixes
    // for a history card and the torrent rows fix with their own spinner.
    //
    // The overlay is worth more here than a plain spinner would be: it prints
    // peers and rate from `torrentLabel`, so a swarm that is slow to answer
    // says so rather than looking like a hang.
    opening.busy = true;
    // What makes this torrent updatable later without anybody pasting a link.
    // Only the catalog can supply it, which is why it travels with the open
    // rather than being looked up afterwards.
    await openTorrent(release.magnet, {
      url: release.url,
      title: catalog.picked?.title ?? catalog.query,
      original: catalog.picked?.original_title ?? catalog.query,
      year: catalog.picked?.year ?? null,
      season: catalog.season,
      quality: release.quality,
      release: release.title,
    });
  } finally {
    catalog.starting = null;
    // `noteOpened` clears this when a file actually loads, and
    // `reportLoadFailure` clears it on the way to the error dialog. Neither
    // fires when the resolve itself throws — `openTorrent` catches that and
    // raises the link dialog — so without this the overlay would stand over a
    // start screen for the rest of the session.
    if (!player.hasFile) opening.busy = false;
  }
}

/**
 * Look for a newer release of a torrent already on disk, and hand it back.
 *
 * **A button rather than a background check, and the measurement is what
 * decided that.** The exact path — the same tracker page carrying a different
 * torrent — depends on the indexer having re-crawled that page, and it re-crawls
 * rarely: of 464 rows in one live response, 1 had been touched in the last 90
 * days and 6 in 180, with 456 sharing a single bulk-sweep date. A check that
 * usually finds nothing, run automatically, is a request per remembered torrent
 * to somebody else's service on every launch, and it teaches people to ignore
 * its answer. Asked for, it is one request when somebody actually wants to know.
 *
 * The fuzzy path — a *different* page, same release, published later — is the
 * one that pays, because new rows are indexed promptly. Both are tried in Rust;
 * this only carries the question and the answer.
 *
 * Returns null when there is nothing newer, and throws only when the lookup
 * itself failed — the caller has to tell those apart, since "no update" is an
 * answer and "could not ask" is not.
 */
export async function findTorrentUpdate(known: RememberedTorrent): Promise<Release | null> {
  const origin = known.origin;
  // A pasted magnet has no search to re-run. Not a failure: the manual dialog
  // is the honest answer for it, and the caller falls back to exactly that.
  if (!origin) return null;
  return await invoke<Release | null>('catalog_find_update', {
    base: effectiveIndexer(),
    title: origin.title,
    originalTitle: origin.original,
    year: origin.year,
    season: origin.season,
    knownUrl: origin.url,
    knownHash: known.infoHash,
    knownName: origin.release,
    knownQuality: origin.quality,
  });
}

/**
 * The update button on a torrent row: look first, ask second.
 *
 * The automatic lookup only works for a torrent that came from the catalog and
 * only when the indexer has something newer, so **every other outcome falls
 * through to the dialog that was there before** — which is not a consolation
 * prize but the correct answer for a pasted magnet, where there is no search to
 * re-run.
 *
 * What it never does is apply the update. `openUpdateDialog` is given the found
 * magnet pre-filled, so the viewer still confirms: replacing a season's torrent
 * re-checks every episode already on disk, and that is not a thing to do on a
 * guess about a name.
 */
export async function checkTorrentUpdate(known: RememberedTorrent) {
  if (catalog.checking) return;
  catalog.checking = known.infoHash;
  try {
    const found = await findTorrentUpdate(known);
    if (found) {
      showOsd(t('torrent.update_found'));
      openUpdateDialog(known, found.magnet);
      return;
    }
    // Told apart on purpose: a torrent with no catalog origin was never
    // searchable, while one that was searched and came back empty is a fact
    // about the indexer worth reporting.
    showOsd(known.origin ? t('torrent.update_none') : t('torrent.update_manual'));
  } catch {
    showOsd(t('torrent.update_check_failed'));
  } finally {
    catalog.checking = null;
  }
  openUpdateDialog(known);
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
