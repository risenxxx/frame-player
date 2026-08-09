/**
 * How a source becomes a playing file: the link box, torrents, and the
 * indicator that says something slow is under way.
 *
 * **This is one module rather than the two it was planned as**, and trying to
 * cut it in half is what showed why. "The link box" and "torrents" depend on
 * each other in both directions and by design: a magnet is pasted into the box
 * that takes links, because that is the obvious place to put a link, so
 * `submitLink` dispatches into the torrent flow — and the torrent flow reports
 * its failures back through `linkBox.torrentError`, raising that same dialog,
 * because a dropped `.torrent` has nowhere else to say what went wrong.
 * Deleting a torrent then has to refresh the box's own recents list, since a
 * magnet in it names a season that no longer exists. A boundary drawn between
 * them would be a cycle with a hook bolted across it.
 *
 * What it deliberately does not own: `loadFiles` (player.svelte.ts), the queue
 * (playlist.svelte.ts) and the position store (history.svelte.ts). It composes
 * those; it does not reimplement any of them.
 *
 * The window dimming that goes with a system file picker is the shell's, and is
 * borrowed through `withFileDialog` rather than reimplemented here.
 */

import { invoke } from '@tauri-apps/api/core';
import { open as pickPath } from '@tauri-apps/plugin-dialog';
import { tick } from 'svelte';

import { withFileDialog } from './chrome.svelte';
import { fmtSize, fmtSpeed } from './units';
import {
  forgetLink,
  purgeTorrentHistory,
  recentLinks,
  rememberLink,
  rememberTitle,
  resolveTitle,
  titleFor,
  type RecentItem,
} from './history.svelte';
import { t } from './i18n.svelte';
import { showOsd } from './osd.svelte';
import {
  applyYtdlpPath,
  isNetworkSource,
  loadFiles,
  pickAndLoadFiles,
  player,
  refreshYtdlp,
  ytdlp,
} from './player.svelte';
import { queueTorrent } from './playlist.svelte';
import { isMagnet, isTorrentLink, magnetFor } from './source';
import {
  addTorrent,
  findSupersededTorrent,
  forgetTorrent,
  listTorrents,
  rememberTorrent,
  resolveTorrentFile,
  setSeeding,
  torrent,
  torrentIsPlaying,
  torrentPrefs,
  torrentResume,
  torrentVideos,
  updateTorrent,
  type RememberedTorrent,
  type TorrentFile,
  type TorrentInfo,
  type TorrentRow,
} from './torrent.svelte';

/**
 * An `end-file` error does NOT mean the attempt failed.
 *
 * mpv's ytdl hook routinely raises one on the way to succeeding — it opens the
 * original URL, gives up on it, and loads what yt-dlp resolved instead — and a
 * playlist transition can do the same. Reporting on the event itself therefore
 * threw an error dialog over videos that were about to play, which is worse
 * than saying nothing. So the verdict waits: if anything loads in the meantime,
 * `cancelLoadFailure` (from `onFileLoaded`) cancels it and nobody hears about it.
 */
const LOAD_FAIL_GRACE = 2500;

/// How long to wait before asking a site for a title mpv has not produced.
const TITLE_LOOKUP_DELAY = 1500;

class Opening {
  linkOpen = $state(false);
  /// The link box's own state, grouped so `LinkDialog` can take it whole and
  /// bind into it. It outlives the dialog closing: a failed link reopens it with
  /// the same text and an explanation of what went wrong.
  box = $state({
    value: '',
    failed: false,
    /// A link failed to open. Kept until the next attempt, because the reason is
    /// worth reading twice — especially the yt-dlp one.
    torrentError: null as string | null,
    /// Read when the dialog opens rather than kept live: the list only changes
    /// when a link is opened, which closes the dialog.
    recent: [] as string[],
    /// Titles for the rows, by link. A row shows its id until one arrives.
    titles: {} as Record<string, string>,
    inputEl: undefined as HTMLInputElement | undefined,
  });
  /// The link that failed, so the fix can retry it without retyping.
  lastLink = $state('');
  /// What the player is currently trying to open. NOT `lastLink`, which only
  /// ever remembers the last thing typed into the dialog and is never cleared:
  /// judging a failure by that meant a stray `end-file` error long afterwards —
  /// and they happen routinely, e.g. when a load is abandoned because another
  /// file was opened — reopened the link dialog over the start screen. Its
  /// backdrop then swallowed the next click, so opening a file "did nothing"
  /// until enough clicks had dismissed enough dialogs.
  attempting = $state('');
  /// An install or update of yt-dlp is running.
  ytdlpBusy = $state(false);
  /// Download progress, 0..100, or null when there is nothing to report — an
  /// update runs inside yt-dlp itself, which does not tell us how far along it
  /// is, so the button falls back to a label without a figure.
  ytdlpPct = $state<number | null>(null);

  /**
   * A network source is being opened and nothing is on screen yet.
   *
   * Between `loadfile` and the first frame a URL can take many seconds — yt-dlp
   * resolving the page, then the stream buffering — and all of it used to be a
   * black window with no sign that anything was happening. Local files never get
   * here: they open faster than the indicator would fade in.
   */
  busy = $state(false);

  /// A resolved torrent waiting for the viewer to choose a file from it.
  pick = $state<TorrentInfo | null>(null);

  /// The torrent being replaced, and the link for it. Null when the dialog is shut.
  updateFor = $state<RememberedTorrent | null>(null);
  updateValue = $state('');
  updateBusy = $state(false);
  updateError = $state<string | null>(null);
  /// Set when a link typed into the ordinary "Open link…" box looks like a newer
  /// release of something already in the list — see `submitLink`.
  updateSuggested = $state(false);

  /// The torrent list on the start screen.
  rows = $state<TorrentRow[]>([]);
  /// The folder being deleted, so its row can gray out — `remove_dir_all` over
  /// 9 GB is not instant and a row that keeps looking clickable invites a second
  /// press.
  rowBusy = $state<string | null>(null);
  /// The folder currently being resolved, so its row can say so. Resolving a
  /// magnet is a DHT lookup — a second at best — and the row used to sit there
  /// looking untouched for all of it.
  rowOpening = $state<string | null>(null);

  /// The name of the video being played, when mpv has none. mpv normally
  /// supplies it (yt-dlp resolves the page), so this is the fallback for the
  /// case the viewer actually sees: a link whose title never arrived, leaving a
  /// video id in the title bar.
  resolvedTitle = $state<string | null>(null);

  total = $derived(this.rows.reduce((sum, r) => sum + r.size, 0));

  /// What the indicator says. mpv reports the cache fill only while there is a
  /// cache, so an absent figure means "still resolving", not "0%".
  label = $derived.by(() => {
    if (player.stalled) return t('load.stalled');
    const pct = player.cacheBuffering;
    return pct !== null && pct < 100 ? t('load.buffering', { percent: pct }) : t('load.opening');
  });

  /**
   * The live torrent readout, or null when there is nothing to report.
   *
   * Only while a torrent is actually feeding playback: a finished download has
   * no rate worth watching, and a local file has no swarm. `busy` is excluded
   * because the loading overlay is already saying all of this, and two copies of
   * the same numbers on screen at once is worse than one.
   */
  chip = $derived.by(() => {
    const s = torrent.status;
    if (!s || !player.hasFile || this.busy) return null;
    if (s.state !== 'live' && s.state !== 'initializing') return null;
    // Nothing left to fetch for this file — the chip has served its purpose and
    // would otherwise sit at 100% for the rest of the film.
    if (s.file_size > 0 && s.file_done >= s.file_size) return null;
    return s;
  });

  /**
   * The second line, when a torrent is what is slow.
   *
   * mpv's own answer to "why is nothing happening" over a torrent is
   * `paused-for-cache`, which says *that* it is waiting and nothing about why.
   * Whether the swarm is empty or simply slow is the difference between "this is
   * broken" and "give it ten seconds", and it is the only thing that makes
   * waiting bearable — so the peer count and the rate go under the label
   * whenever they are what the viewer is waiting on.
   */
  torrentLabel = $derived.by(() => {
    const s = torrent.status;
    if (!s || s.state === 'gone') return null;
    if (s.state === 'error') return s.error ?? t('torrent.error');
    // Not a swarm problem at all: librqbit is hashing what is already on disk.
    // Measured at 3.1 s for a season with 46 MB fetched, and it is the whole of
    // the wait when reopening a half-watched one — while `peers` is legitimately
    // zero, so without this the popup announced "no peers" for something that
    // was not looking for any.
    if (s.state === 'initializing') return t('torrent.verifying');
    // No peers at all is a different problem from a slow one, and the advice
    // ("this torrent has no seeders") is different too.
    if (s.peers === 0) {
      return s.peers_seen > 0 ? t('torrent.connecting') : t('torrent.no_peers');
    }
    return t('torrent.rate', { peers: s.peers, speed: fmtSpeed(s.down_bps) });
  });
}

export const opening = new Opening();

let loadFailTimer: ReturnType<typeof setTimeout> | undefined;

// ---- The link box ---------------------------------------------------------

export async function openLinkDialog() {
  opening.box.failed = false;
  opening.box.recent = recentLinks();
  for (const url of opening.box.recent) {
    const known = titleFor(url);
    if (known) opening.box.titles = { ...opening.box.titles, [url]: known };
  }
  opening.linkOpen = true;
  void fillLinkTitles();
  await tick();
  // Focused so the paste shortcut works with nothing else to click.
  opening.box.inputEl?.focus();
  opening.box.inputEl?.select();
}

/// Fill in whatever names are missing, one request at a time — this is a
/// network call per unknown link, and a list of eight should not open eight
/// connections at once.
async function fillLinkTitles() {
  for (const url of opening.box.recent) {
    if (opening.box.titles[url]) continue;
    const title = await resolveTitle(url);
    if (title) opening.box.titles = { ...opening.box.titles, [url]: title };
  }
}

/// Drop one row of the recent-links list.
///
/// The list is the *only* thing touched: a magnet in it may name a torrent whose
/// episodes are on disk, and "I do not want this link offered again" is not
/// "delete the season" — that is what the torrent list's own cross is for.
export function dropLink(url: string) {
  forgetLink(url);
  opening.box.recent = recentLinks();
}

export function submitLink(url = opening.box.value.trim()) {
  if (!url) return;
  // Dispatched by shape, not by a separate box: a magnet and a YouTube link are
  // the same gesture — the viewer has a link — and which protocol reaches the
  // video is the player's problem, not theirs.
  if (isTorrentLink(url)) {
    // A re-upload usually arrives here rather than through the update button,
    // because pasting a link into the box that takes the recents list is the
    // obvious move. Opening it as a separate torrent would silently re-download
    // a season and reset its history, so the resemblance is worth a question —
    // asked before anything is added, while both outcomes are still cheap.
    const superseded = findSupersededTorrent(url);
    if (superseded) {
      opening.linkOpen = false;
      void openUpdateDialog(superseded, url);
      return;
    }
    void openTorrent(url);
    return;
  }
  opening.linkOpen = false;
  opening.box.failed = false;
  opening.lastLink = url;
  rememberLink(url);
  opening.box.recent = recentLinks();
  void loadFiles([url]);
}

/**
 * Install or update yt-dlp, then have another go at the link that failed.
 *
 * Only our own copy is ever updated — one from Homebrew or pip belongs to its
 * package manager, so there the dialog explains instead of offering a button it
 * has no right to press.
 */
export async function fixYtdlp() {
  if (opening.ytdlpBusy) return;
  opening.ytdlpBusy = true;
  opening.ytdlpPct = null;
  try {
    const version = await invoke<string>(ytdlp.present ? 'ytdlp_update' : 'ytdlp_install');
    await refreshYtdlp();
    // mpv learns the path without a restart: installing it mid-session is
    // exactly the case this whole flow exists for.
    applyYtdlpPath();
    showOsd(t('link.ytdlp_ready', { version }));
    opening.box.failed = false;
    if (opening.lastLink) {
      opening.linkOpen = false;
      void loadFiles([opening.lastLink]);
    }
  } catch (e) {
    showOsd(t('link.ytdlp_failed'));
    console.warn('yt-dlp install/update failed:', e);
  } finally {
    opening.ytdlpBusy = false;
    opening.ytdlpPct = null;
  }
}

export function reportLoadFailure() {
  clearTimeout(loadFailTimer);
  loadFailTimer = setTimeout(() => {
    opening.busy = false;
    // A link that fails deserves more than a popup that fades: the fix is
    // usually one button, and the dialog is where the user just was. Judged by
    // what was being opened, and only while nothing has opened since.
    if (isNetworkSource(opening.attempting) && !player.hasFile) {
      opening.lastLink = opening.attempting;
      opening.box.failed = true;
      opening.linkOpen = true;
      return;
    }
    showOsd(t('osd.open_failed'));
  }, LOAD_FAIL_GRACE);
}

/// Drop a pending verdict without declaring success — what `beforeLoad` needs,
/// since the *next* attempt is about to start and the previous one's failure is
/// no longer anyone's business.
export function cancelLoadFailure() {
  clearTimeout(loadFailTimer);
}

/// A file opened. Whatever failed on the way there was a step and not an
/// outcome, and nothing is being attempted any more.
export function noteOpened() {
  clearTimeout(loadFailTimer);
  opening.attempting = '';
  opening.busy = false;
}

/**
 * Ask the site for a name, but only when mpv came up empty-handed.
 *
 * mpv normally supplies `media-title` — yt-dlp resolved the page on the way in —
 * so this is the fallback for the case the viewer actually sees: a link whose
 * title never arrived, leaving a video id in the title bar. The delay is what
 * makes it rare: a title that is going to appear has appeared by then, and
 * asking the site for something already known is a request for nothing.
 *
 * Guarded on `filePath` at both ends, because a second and a half is long enough
 * for the viewer to have opened something else.
 */
export function resolveTitleIfMissing() {
  opening.resolvedTitle = null;
  const src = player.filePath;
  if (!src || !isNetworkSource(src)) return;
  setTimeout(() => {
    if (player.filePath !== src || player.mediaTitle) return;
    void resolveTitle(src).then((title) => {
      if (title && player.filePath === src) opening.resolvedTitle = title;
    });
  }, TITLE_LOOKUP_DELAY);
}

// ---- Torrents -------------------------------------------------------------
//
// Resolving a magnet is a DHT lookup that takes ten seconds when it works, so
// the dialog stays up saying so rather than closing on a black window — the same
// rule as everywhere else here: a slow operation announces itself before it
// starts, not after it finishes.
//
// What comes back is a *list*. A torrent with one film plays it; a torrent
// holding a season is a question only the viewer can answer, so it gets a
// picker. Either way every video in it becomes a queue entry, which costs
// nothing until one is played (see torrent.rs).

export async function openTorrent(source: string) {
  opening.box.torrentError = null;
  opening.pick = null;
  opening.lastLink = source;
  try {
    const info = await addTorrent(source);
    // **What is remembered is a magnet, whatever was handed over.** A dropped
    // `.torrent` file is a path that can be moved or deleted and a `.torrent`
    // URL is a page that can go down, while the info hash names the torrent for
    // good — and the metadata cache beside it (torrent.rs) makes reopening from
    // a bare magnet cost no lookup at all. So a file opened once keeps working
    // after it is thrown away, which is what people do with `.torrent` files.
    const key = isMagnet(source) ? source : magnetFor(info.info_hash, info.name);
    rememberLink(key);
    // The torrent's own name against the magnet, so the recents list reads as a
    // film rather than a hash. `displayName` would fall back to the `dn`
    // parameter, but that is the name whoever made the link chose to put in it;
    // this is the one in the torrent.
    if (info.name) rememberTitle(key, info.name);
    // How to reopen this torrent later, which is what makes the start-screen
    // list and the history cards work after a restart.
    rememberTorrent(info, key);
    opening.box.recent = recentLinks();
    const videos = torrentVideos(info);
    if (videos.length === 0) {
      opening.box.torrentError = t('torrent.no_video');
      return;
    }
    if (videos.length === 1) {
      await playTorrentFile(info, videos[0]);
      return;
    }
    opening.pick = info;
  } catch (e) {
    // The one failure worth naming: a swarm that never answered is a fact about
    // the torrent, and "try a different magnet" is the actual advice.
    opening.box.torrentError =
      String(e) === 'resolve_timeout'
        ? t('torrent.timeout')
        : t('torrent.failed', { reason: String(e) });
    // A torrent does not only arrive from the box that shows this message: a
    // dropped `.torrent` reaches here with no dialog open, and a corrupt file
    // would then fail in complete silence. Raising it is the same answer the
    // start-screen rows give, and it is a no-op when the box is already up.
    opening.linkOpen = true;
  }
}

/// The ordinary "open a file" picker. Here rather than in the page because
/// opening a file is what this module is for; the window dimming belongs to the
/// shell and is borrowed through `withFileDialog`.
export async function openFileDialog() {
  await withFileDialog(pickAndLoadFiles);
}

/// Pick a `.torrent` from disk. The same dimming flag as the other pickers: the
/// system dialog dims the window while it is up.
export async function pickTorrentFile() {
  const sel = await withFileDialog(() =>
    pickPath({
      multiple: false,
      filters: [{ name: t('dialog.torrent'), extensions: ['torrent'] }],
    }),
  );
  if (typeof sel === 'string') await openTorrent(sel);
}

export async function playTorrentFile(info: TorrentInfo, file: TorrentFile) {
  opening.pick = null;
  opening.linkOpen = false;
  opening.box.failed = false;
  opening.box.torrentError = null;
  await loadFiles([file.url]);
  // After the load, like the folder queue: the entry being played is already in
  // the playlist and the rest go around it.
  await queueTorrent(torrentVideos(info), file.url);
}

/**
 * Turning seeding off has to take effect now, not at the next magnet — a switch
 * that reads "off" while pieces are still going out would be a lie about the one
 * behavior here with legal weight attached. That costs the running torrent, so
 * the popup says so rather than leaving a stream that silently died.
 */
export async function toggleSeeding() {
  const stopped = await setSeeding(!torrentPrefs.seeding);
  if (stopped) {
    torrent.info = null;
    torrent.status = null;
    showOsd(t('torrent.seed_restarted'));
  }
}

export async function clearTorrentCache() {
  const freed = await invoke<number>('torrent_clear_cache').catch(() => 0);
  showOsd(t('torrent.cache_cleared', { size: fmtSize(freed) }));
}

/**
 * Open something from the watch history — and **give it back its queue**.
 *
 * A local file is its own address and opens directly, and gets the folder queue
 * for free (`loadFiles` builds it through the `filesOpened` hook). A **torrent
 * entry is neither**: what the record holds is a loopback URL whose port died
 * with the session that issued it, so after a restart every torrent card led to
 * a refused connection — it has to be re-derived from the remembered magnet,
 * which is what `resolveTorrentFile` does and the reason torrents are remembered
 * at all. And the resolver used to hand back one URL with the rest of the season
 * dropped, so an episode opened from the history had no next episode while the
 * same episode opened from the torrent list did: one file, two behaviors,
 * invisible from outside.
 */
export async function openRecent(item: RecentItem) {
  if (!item.id.startsWith('torrent:')) {
    await loadFiles([item.path]);
    return;
  }
  opening.busy = true;
  const resolved = await resolveTorrentFile(item.id);
  if (!resolved) {
    opening.busy = false;
    showOsd(t('start.torrent_lost'));
    return;
  }
  await loadFiles([resolved.url]);
  await queueTorrent(torrentVideos(resolved.info), resolved.url);
}

// ---- The torrent list on the start screen ---------------------------------

export async function refreshTorrents() {
  opening.rows = await listTorrents();
}

/**
 * Reopen a torrent from the start screen.
 *
 * **Goes straight to where the viewer left off**, which is what the row says it
 * will do — it prints "продолжить: S01E03 · 23:14" — and what it used to ignore,
 * dropping the same file picker a freshly pasted magnet gets. Landing on a list
 * of nine identical file names to hunt for the episode you were already watching
 * is the player forgetting something it plainly knows.
 *
 * The picker is still what happens when there is nothing to continue: a torrent
 * opened but never watched, or one whose positions have aged out.
 *
 * What it opens with is `row.magnet`, which is the remembered one where there is
 * one and a magnet built from the folder's own info hash where there is not —
 * see `listTorrents`. Opening it then records it, so a row that arrived on this
 * disk without our knowing it names itself from the second time onwards.
 */
export async function openRememberedTorrent(row: TorrentRow) {
  if (!row.magnet || opening.rowOpening) return;
  const magnet = row.magnet;
  const resume = torrentResume(row);
  opening.rowOpening = row.folder;
  opening.box.torrentError = null;
  try {
    const info = await addTorrent(magnet);
    rememberTorrent(info, magnet);
    const videos = torrentVideos(info);
    if (!videos.length) {
      opening.box.torrentError = t('torrent.no_video');
      opening.linkOpen = true;
      return;
    }
    const wanted =
      (resume && videos.find((f) => f.index === resume.index)) ??
      (videos.length === 1 ? videos[0] : null);
    if (wanted) {
      await playTorrentFile(info, wanted);
      return;
    }
    opening.pick = info;
  } catch (e) {
    opening.box.torrentError =
      String(e) === 'resolve_timeout'
        ? t('torrent.timeout')
        : t('torrent.failed', { reason: String(e) });
    opening.linkOpen = true;
  } finally {
    opening.rowOpening = null;
  }
}

/// Delete a torrent completely — data, history, magnet. One action, because a
/// half-deleted torrent is the confusing state: files gone but the season still
/// listed, or the reverse.
export async function deleteTorrent(row: TorrentRow) {
  if (torrentIsPlaying(row) || opening.rowBusy) return;
  opening.rowBusy = row.folder;
  try {
    const freed = await forgetTorrent(row);
    if (row.info_hash) {
      purgeTorrentHistory(row.info_hash);
      if (row.known) forgetLink(row.known.magnet);
    }
    await refreshTorrents();
    opening.box.recent = recentLinks();
    showOsd(t('torrent.cache_cleared', { size: fmtSize(freed) }));
  } finally {
    opening.rowBusy = null;
  }
}

// ---- Replacing a torrent with its re-upload -------------------------------
//
// BitTorrent cannot add a file to a torrent, so an uploader who adds an episode
// publishes a different one. The player cannot discover that — the new magnet
// lives on a tracker page we have no access to — but once the viewer brings it,
// everything else is ours to do: keep the episodes already on disk, and keep the
// watch history that is keyed by the old info hash.

/// The dialog focuses its own field once it is on screen — see the note in
/// TorrentUpdateDialog.svelte for why that is not done from here.
export function openUpdateDialog(known: RememberedTorrent, magnet = '') {
  opening.updateFor = known;
  opening.updateValue = magnet;
  opening.updateError = null;
  opening.updateSuggested = !!magnet;
}

export async function submitUpdate() {
  const magnet = opening.updateValue.trim();
  const known = opening.updateFor;
  if (!magnet || !known || opening.updateBusy) return;
  if (!isTorrentLink(magnet)) {
    opening.updateError = t('torrent.update_not_magnet');
    return;
  }
  opening.updateBusy = true;
  opening.updateError = null;
  try {
    const { info, matched, added } = await updateTorrent(known, magnet);
    opening.updateFor = null;
    opening.box.recent = recentLinks();
    await refreshTorrents();
    showOsd(t('torrent.updated', { matched, added }));
    // Straight into the picker: the viewer came here for the new episode, and
    // making them click the row again to reach it would be the second half of
    // the same errand.
    const videos = torrentVideos(info);
    if (videos.length) opening.pick = info;
  } catch (e) {
    const reason = String(e);
    opening.updateError = reason.includes('same_torrent')
      ? t('torrent.update_same')
      : reason.includes('no_hash')
        ? t('torrent.update_not_magnet')
        : t('torrent.failed', { reason });
  } finally {
    opening.updateBusy = false;
  }
}
