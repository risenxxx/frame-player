/**
 * The play queue: mpv's playlist, the folder that fills it, and the posters the
 * end-of-file screen shows for the neighbouring entries.
 *
 * Two things here are not obvious and both are load-bearing.
 *
 * **The end of a file is a moment we own.** `keep-open=always` in the player's
 * initial options stops mpv at the end of *every* entry, not just the last one
 * (which is all `yes` does), so advancing is ours to do — with a countdown the
 * viewer can cancel, and with the wrap that `loop-playlist` would otherwise
 * perform invisibly.
 *
 * **A queue is only ever built from a single opened file.** Dropping five files
 * is an explicit selection and stays exactly that; opening one episode is the
 * case where the rest of the folder is wanted.
 */

import { invoke } from '@tauri-apps/api/core';
import { command, getProperty, setProperty } from 'tauri-plugin-libmpv-api';

import { baseName, displayName, extensionOf } from './format';
import { VIDEO_EXTENSIONS, isNetworkSource, player, readList } from './player.svelte';

export interface PlaylistEntry {
  index: number;
  /// The path as mpv holds it, which is what we handed to `loadfile`.
  path: string;
  /// What to show. A real name when there is one — mpv's, or the container's
  /// read ahead — otherwise the file name tidied by `displayName`.
  title: string;
  /// True when the title came from the file rather than from its name. Only
  /// the others are worth opening, and only once.
  named: boolean;
}

/// Upper bound on the queue built from a folder, counting the opened file.
/// A season is tens of files; a downloads folder can hold thousands, and every
/// entry is a `loadfile` command plus a row in the panel.
const QUEUE_LIMIT = 300;

/// Where in an unseen file to grab the poster for the end-of-file card. Far
/// enough in to be past a black frame or a distributor logo, and short enough
/// that any real episode reaches it.
const POSTER_AT = 60;

/// Posters are blobs; keeping every one ever decoded would leak a URL per file
/// visited. Only the neighbours of the current entry are ever shown.
const POSTER_CACHE = 6;

const PREFS_KEY = 'frameplayer.playlist';

class PlaylistState {
  entries = $state<PlaylistEntry[]>([]);
  /// Blob URLs by path, for the end-of-file cards.
  posters = $state<Record<string, string>>({});
  /// Queue the rest of the folder when a single file is opened.
  queueFolder = $state(true);
  /// Roll on to the next entry by itself when a file ends.
  autoAdvance = $state(true);

  get hasQueue(): boolean {
    return player.playlistCount > 1;
  }
}

export const playlist = new PlaylistState();

export function loadPlaylistPrefs() {
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    if (!raw) return;
    const saved = JSON.parse(raw) as { queueFolder?: boolean; autoAdvance?: boolean };
    if (typeof saved.queueFolder === 'boolean') playlist.queueFolder = saved.queueFolder;
    if (typeof saved.autoAdvance === 'boolean') playlist.autoAdvance = saved.autoAdvance;
  } catch {
    // corrupt entry — the defaults stay
  }
}

export function setPlaylistPref(key: 'queueFolder' | 'autoAdvance', value: boolean) {
  playlist[key] = value;
  try {
    localStorage.setItem(
      PREFS_KEY,
      JSON.stringify({ queueFolder: playlist.queueFolder, autoAdvance: playlist.autoAdvance }),
    );
  } catch {
    // not critical: the choice simply will not survive a restart
  }
}

// ---- Reading the queue ----------------------------------------------------

/// Same guard as the track and chapter lists: a read is dozens of sequential
/// round-trips, and several things ask for one.
let entriesSeq = 0;

export async function loadPlaylist() {
  const seq = ++entriesSeq;
  const list = await readList('playlist', async (base, index) => {
    const path = await getStr(`${base}/filename`);
    if (!path) return null;
    // mpv fills `playlist/N/title` from `media-title`, and only for entries it
    // has actually opened — so everything ahead of the playhead is unnamed
    // until it plays. `containerTitles` is the same answer fetched early.
    const title = await getStr(`${base}/title`);
    const known = title || containerTitles.get(path) || null;
    // `displayName`, not a bare basename: a path is written for sorting, not
    // for reading. Not `fileStem` either, which sanitises for use as a FILE
    // name and would turn "Show: Part 1" into "Show_ Part 1" on screen.
    return {
      index,
      path,
      title: known || displayName(path),
      named: !!known,
    } satisfies PlaylistEntry;
  }).catch(() => [] as PlaylistEntry[]);
  if (seq !== entriesSeq) return;
  playlist.entries = list;
  scheduleTitleRead();
}

// ---- Reading titles out of the files --------------------------------------

/// Container titles found so far, by path. Session-only: the read is cheap
/// enough not to be worth a store, and a store would be one more place the
/// privacy rules have to be enforced.
const containerTitles = new Map<string, string>();

/// Paths already looked at, so a file with no title tag is not reopened every
/// time the playlist changes.
const titlesTried = new Set<string>();

let titleTimer: ReturnType<typeof setTimeout> | undefined;

/// Delay before naming the queue. The read is fast (measured 0.10 ms per file
/// against a warm cache) but it is still I/O, and the seconds right after a
/// file opens belong to the file that is opening.
const TITLE_READ_DELAY = 1200;

function scheduleTitleRead() {
  clearTimeout(titleTimer);
  // A queue of one is the ordinary case and has nothing to list, so it does not
  // pay for this at all.
  if (playlist.entries.length < 2) return;
  titleTimer = setTimeout(() => void readQueueTitles(), TITLE_READ_DELAY);
}

/// Read now rather than on the timer — the panel is open and the names are
/// being looked at. Anything already tried is skipped, so this is free once
/// the delayed pass has run.
export function ensureQueueTitles() {
  clearTimeout(titleTimer);
  void readQueueTitles();
}

async function readQueueTitles() {
  const paths = playlist.entries
    .filter((e) => !e.named && !titlesTried.has(e.path) && !isNetworkSource(e.path))
    .map((e) => e.path);
  if (!paths.length) return;
  for (const path of paths) titlesTried.add(path);
  const titles = await invoke<(string | null)[]>('container_titles', { paths }).catch(
    () => [] as (string | null)[],
  );
  let found = false;
  titles.forEach((title, i) => {
    if (!title) return;
    containerTitles.set(paths[i], title);
    found = true;
  });
  if (!found) return;
  // Rewritten in place rather than re-read: mpv has nothing new to say, and a
  // full `loadPlaylist` is dozens of round-trips for a change we already have.
  playlist.entries = playlist.entries.map((e) => {
    const title = e.named ? null : containerTitles.get(e.path);
    return title ? { ...e, title, named: true } : e;
  });
}

async function getStr(prop: string): Promise<string | null> {
  const value = await getProperty(prop, 'string').catch(() => null);
  return value?.trim() || null;
}

// ---- Neighbours -----------------------------------------------------------

/**
 * The entry `offset` steps away, wrapping when the queue repeats.
 *
 * The wrap is computed here rather than left to mpv: with `keep-open=always`
 * mpv never advances on its own, so `loop-playlist` never fires and "repeat
 * all" would otherwise do nothing at all.
 */
/**
 * How long the end screen counts down before starting the next entry.
 *
 * Here rather than in either place that reads it, because both must agree: the
 * page arms a timer with it and the card draws a progress sweep of exactly that
 * duration. Two copies would drift and the bar would finish early or late.
 */
export const ADVANCE_MS = 5000;

export function neighbour(offset: number): PlaylistEntry | null {
  const count = playlist.entries.length;
  if (count === 0) return null;
  const target = player.playlistPos + offset;
  if (target >= 0 && target < count) return playlist.entries[target] ?? null;
  if (player.loopMode !== 'all') return null;
  return playlist.entries[((target % count) + count) % count] ?? null;
}

/** Switch to a queue entry and play it. */
export async function playEntry(entry: PlaylistEntry) {
  await command('playlist-play-index', [String(entry.index)]).catch(() => {});
  // `keep-open` paused us at the end of the previous file, and pause survives a
  // file change — without this the next episode opens on a still frame.
  await setProperty('pause', false).catch(() => {});
}

// ---- Posters --------------------------------------------------------------

/**
 * Decode one frame of a file that is not open, for the end-of-file card.
 *
 * One decode at a time and only for a card about to be shown: this is the same
 * budget the start screen keeps, for the same reason — a keyframe decode
 * competes with playback (see the storyboard notes in CLAUDE.md).
 */
let posterInFlight = false;

export async function ensurePoster(path: string) {
  // A poster is decoded out of the file; a stream has none to decode, and
  // asking would mean pulling the video down a second time.
  if (isNetworkSource(path)) return;
  if (playlist.posters[path] || posterInFlight) return;
  posterInFlight = true;
  try {
    const buf = await invoke<ArrayBuffer>('poster_frame', { path, pos: POSTER_AT });
    if (buf.byteLength <= 8) return;
    // The first 8 bytes are the PTS header the thumb service prefixes.
    const url = URL.createObjectURL(new Blob([new Uint8Array(buf, 8)], { type: 'image/jpeg' }));
    playlist.posters = { ...playlist.posters, [path]: url };
    prunePosters();
  } catch {
    // no poster — the card shows its name and nothing else
  } finally {
    posterInFlight = false;
  }
}

function prunePosters() {
  const paths = Object.keys(playlist.posters);
  if (paths.length <= POSTER_CACHE) return;
  const next = { ...playlist.posters };
  for (const path of paths.slice(0, paths.length - POSTER_CACHE)) {
    URL.revokeObjectURL(next[path]);
    delete next[path];
  }
  playlist.posters = next;
}

export function dropPosters() {
  for (const url of Object.values(playlist.posters)) URL.revokeObjectURL(url);
  playlist.posters = {};
}

// ---- Building a queue from the folder -------------------------------------

/// Path comparison for "is this the file we just opened".
///
/// Case-insensitive, like the privacy roots in history.svelte.ts and for the
/// same reason: Windows and macOS both hand back paths whose case does not
/// match what was asked for. **And separator-insensitive**, because the two
/// sides of this comparison come from different places — one from a stored
/// record or mpv's `path`, the other from a directory read in Rust — and on
/// Windows a mixed `E:/Videos\ep1.mkv` costs the whole queue: `findIndex`
/// answers −1, `queueAround` returns without a word, and the player looks like
/// it decided this folder has no other episodes.
function samePath(a: string, b: string): boolean {
  return a.toLowerCase().replace(/\\/g, '/') === b.toLowerCase().replace(/\\/g, '/');
}

/**
 * Put `entries` around the one at `at`, which is already loading.
 *
 * The opened entry keeps its natural position rather than becoming entry zero:
 * the ones that sort before it are inserted ahead of it, so "previous episode"
 * means what it says. `insert-at` is mpv 0.38+; on anything older the insert
 * fails and the queue is simply forward-only, which is worth having anyway.
 *
 * The list is truncated *around* `at` rather than from the end: in a folder of
 * thousands — or a torrent holding an entire series — the episodes that matter
 * are its neighbours.
 */
async function queueAround(entries: string[], at: number, what: string) {
  if (at < 0 || entries.length < 2) return;

  const half = Math.floor(QUEUE_LIMIT / 2);
  const from = Math.max(0, Math.min(at - half, entries.length - QUEUE_LIMIT));
  const chosen = entries.slice(from, from + QUEUE_LIMIT);
  if (chosen.length < entries.length) {
    console.info(`[playlist] ${what} has ${entries.length} videos, queued ${chosen.length}`);
  }

  const here = at - from;
  for (const path of chosen.slice(here + 1)) {
    await command('loadfile', [path, 'append']).catch(() => {});
  }
  for (const [i, path] of chosen.slice(0, here).entries()) {
    try {
      await command('loadfile', [path, 'insert-at', String(i)]);
    } catch {
      // mpv older than 0.38: keep what was appended and stop trying.
      break;
    }
  }
  void loadPlaylist();
}

/** Queue the rest of the folder behind a file that is already loading. */
export async function queueFolder(openedPath: string) {
  if (!playlist.queueFolder) return;
  // A stream has no folder to look in. (`folder_entries` would take the URL for
  // a path, read a directory that does not exist and fail — harmlessly, but
  // there is no reason to ask.)
  if (isNetworkSource(openedPath)) return;
  const siblings = await invoke<string[]>('folder_entries', { path: openedPath }).catch(
    () => [] as string[],
  );
  const videos = siblings.filter((p) => VIDEO_EXTENSIONS.includes(extensionOf(p)));
  if (videos.length < 2) return;

  videos.sort((a, b) => naturalOrder.compare(baseName(a), baseName(b)));
  const at = videos.findIndex((p) => samePath(p, openedPath));
  if (at < 0) {
    // The folder was read and the file we just opened is not in it — a path
    // shape neither side expected, and previously the queue simply never
    // appeared. Say so: this is the only way the next report arrives with the
    // two strings that disagree.
    console.warn('[playlist] opened file not found in its own folder', {
      openedPath,
      example: videos[0],
    });
    return;
  }
  await queueAround(videos, at, 'folder');
}

/**
 * Queue the rest of a torrent behind the file being opened.
 *
 * A torrent holding a season *is* a playlist — the roadmap's observation, and it
 * costs nothing here because a URL is only a string: nothing is downloaded until
 * mpv actually reads one, at which point the server selects that file (see the
 * module header in torrent.rs). So a twelve-episode torrent produces a
 * twelve-entry queue that still only fetches the episode being watched.
 *
 * Ordered by the *path inside the torrent*, not the torrent's own file order,
 * which is whatever the person who packed it happened to use.
 */
export async function queueTorrent(files: { path: string; url: string }[], openedUrl: string) {
  if (!playlist.queueFolder || files.length < 2) return;
  const sorted = [...files].sort((a, b) => naturalOrder.compare(a.path, b.path));
  await queueAround(
    sorted.map((f) => f.url),
    sorted.findIndex((f) => f.url === openedUrl),
    'torrent',
  );
}

/// Natural order, the way a file manager sorts: "ep2" before "ep10".
const naturalOrder = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });
