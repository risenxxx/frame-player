/**
 * Watch history, the "continue watching" list, and the privacy rules around them.
 *
 * By default the player remembers where you stopped. It can be turned off
 * entirely, and individual folders can be excluded — inside them NOTHING is
 * written: no position, no snapshot for restoring after an update, no thumbnail
 * storyboard on disk. That last one is enforced in Rust, hence `set_private_paths`;
 * privacy has to hold in all three places or it leaks.
 */

import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';

import { baseName, displayName, pathUnder } from './format';
import { t } from './i18n.svelte';
import { showOsd } from './osd.svelte';
import { type Attempt, latest } from './latest';
import { isNetworkSource, player, type TrackWish } from './player.svelte';
import { parseTorrentUrl, sourceId } from './source';

/// Seconds to rewind when resuming, so playback starts just before where the
/// viewer left off rather than exactly on it.
export const RESUME_OFFSET = 3;

/// How many cards the start screen shows. The rest of the history (up to 300
/// entries) does not go there: this is "continue watching", not a file manager.
/// Three times as many candidates are collected, to replace the ones whose
/// files turned out to be unreachable.
///
/// Twelve was tuned while most entries were silently dropped — every network
/// source failed the poster check — so the grid really held three or four. With
/// that fixed it filled three rows and pushed the torrents section 220px below
/// the fold at 1280x800, measured; a section nobody scrolls to may as well not
/// exist. The fix is the rail rather than a smaller number: one row whatever the
/// count, so this can stay generous without the page growing.
const RECENT_LIMIT = 12;

const HISTORY_KEY = 'frameplayer.history';
const POSITIONS_KEY = 'frameplayer.positions';
const RESUME_KEY = 'frameplayer.resume';
const TRACKS_KEY = 'frameplayer.tracks';

type HistoryPrefs = { enabled: boolean; excluded: string[] };

export type RecentItem = {
  /// The source identity (`sourceId`). Carried beside `path` because the two
  /// answer different questions and only agree for local files: a torrent's
  /// `path` is a loopback URL whose port dies with the session, so opening the
  /// card has to go through the id — see `openRecent` in +page.svelte.
  id: string;
  path: string;
  name: string;
  pos: number;
  dur: number;
  ts: number;
  poster: string | null;
};

class History {
  prefs = $state<HistoryPrefs>({ enabled: true, excluded: [] });
  recent = $state<RecentItem[]>([]);
}

export const history = new History();

// ---- Chosen tracks, per file ---------------------------------------------
// A separate store from the watch positions on purpose: those are deleted once
// a file is finished (past 97%), and "which dub I watch this show in" should
// outlive finishing an episode. Only *explicit* choices are recorded — mirroring
// back whatever mpv auto-selected would freeze it against later alang/slang
// changes for no benefit.

/// Legacy shape: plain mpv ids, written before the choice learned to describe
/// itself. Still read so a remembered dub is not silently lost on upgrade —
/// an id is meaningless in another file, but it is exact in the one it came
/// from, which is where it was stored.
type TrackChoice = { aid?: string; sid?: string; ts: number };

type TrackEntry = {
  audio?: TrackWish;
  sub?: TrackWish;
  /// Subtitle and audio delay, in seconds. Same store as the track choice
  /// because it answers the same question — "how does this file need to be
  /// played" — and has the same lifetime: a badly muxed rip needs its half
  /// second every time it is opened, long after the position is forgotten.
  subDelay?: number;
  audioDelay?: number;
  ts: number;
};

function tracksLoad(): Record<string, TrackChoice> {
  try {
    return JSON.parse(localStorage.getItem(TRACKS_KEY) ?? '{}');
  } catch {
    return {};
  }
}

export function rememberTrackChoice(path: string, kind: 'audio' | 'sub', value: string) {
  if (isPrivatePath(path)) return;
  try {
    const map = tracksLoad();
    const entry: TrackChoice = { ...map[path], ts: Date.now() };
    if (kind === 'audio') entry.aid = value;
    else entry.sid = value;
    map[path] = entry;
    const keys = Object.keys(map);
    if (keys.length > 300) {
      keys.sort((a, b) => map[a].ts - map[b].ts);
      for (const k of keys.slice(0, keys.length - 300)) delete map[k];
    }
    localStorage.setItem(TRACKS_KEY, JSON.stringify(map));
  } catch {
    // localStorage unavailable — not critical
  }
}

export function trackChoiceFor(path: string): TrackChoice | null {
  if (isPrivatePath(path)) return null;
  const entry = tracksLoad()[sourceId(path)];
  // Only the legacy shape has these; the new one stores descriptors.
  return entry && (entry.aid || entry.sid) ? entry : null;
}

// ---- Titles of network sources --------------------------------------------
//
// One cache, keyed by source id, filled from two places. mpv gives a title for
// free once a video plays (yt-dlp resolves it), and that covers everything ever
// watched. What it cannot cover is a link sitting in a list having never played
// — or one whose load failed — and there the site's own oEmbed endpoint is
// asked instead. Both write here, so the rest of the UI has one place to look.

const TITLES_KEY = 'frameplayer.titles';
const TITLES_LIMIT = 200;

function titlesLoad(): Record<string, string> {
  try {
    return JSON.parse(localStorage.getItem(TITLES_KEY) ?? '{}');
  } catch {
    return {};
  }
}

export function titleFor(src: string): string | null {
  return titlesLoad()[sourceId(src)] ?? null;
}

export function rememberTitle(src: string, title: string) {
  if (!title.trim() || isPrivatePath(src)) return;
  try {
    const map = titlesLoad();
    const id = sourceId(src);
    if (map[id] === title) return;
    map[id] = title;
    const keys = Object.keys(map);
    // No timestamps here — a title never goes stale, so the oldest inserted is
    // as good a thing to drop as any, and the order Object.keys returns is
    // insertion order.
    if (keys.length > TITLES_LIMIT) {
      for (const k of keys.slice(0, keys.length - TITLES_LIMIT)) delete map[k];
    }
    localStorage.setItem(TITLES_KEY, JSON.stringify(map));
  } catch {
    // localStorage unavailable — not critical
  }
}

/**
 * Ask the site what the video is called, and remember the answer.
 *
 * Only when nothing is known already, and never for a local file: this is a
 * request that tells someone what is being watched, so it goes only to the site
 * already serving the video and only when there is something to gain.
 */
export async function resolveTitle(src: string): Promise<string | null> {
  if (!isNetworkSource(src) || isPrivatePath(src)) return null;
  const known = titleFor(src);
  if (known) return known;
  const title = await invoke<string | null>('oembed_title', { url: src }).catch(() => null);
  if (title) rememberTitle(src, title);
  return title ?? null;
}

// ---- Subtitles we downloaded ----------------------------------------------
//
// A record of the files this player put on disk, and the only thing that
// distinguishes them from a subtitle the viewer made, corrected or dragged in.
// It exists for one action: removing a subtitle track. Detaching one we
// downloaded has to delete the file as well, because it was deliberately named
// so `sub-auto=fuzzy` picks it up — leave it and the removal quietly undoes
// itself the next time the episode is opened. A file we did not create is never
// deleted, so an unrecorded path simply detaches.
//
// Privacy-gated like the position store: the path is named after the video, so
// keeping it says what was watched.

const DOWNLOADED_SUBS_KEY = 'frameplayer.subs';
const DOWNLOADED_SUBS_LIMIT = 200;

function downloadedSubs(): string[] {
  try {
    const list = JSON.parse(localStorage.getItem(DOWNLOADED_SUBS_KEY) ?? '[]');
    return Array.isArray(list) ? list.filter((x) => typeof x === 'string') : [];
  } catch {
    return [];
  }
}

export function rememberDownloadedSub(path: string) {
  if (!history.prefs.enabled || isPrivatePath(path)) return;
  try {
    const next = [path, ...downloadedSubs().filter((x) => x !== path)].slice(
      0,
      DOWNLOADED_SUBS_LIMIT,
    );
    localStorage.setItem(DOWNLOADED_SUBS_KEY, JSON.stringify(next));
  } catch {
    // localStorage unavailable — the subtitle still works, it just cannot be
    // told apart from the viewer's own later on.
  }
}

export function isDownloadedSub(path: string): boolean {
  return downloadedSubs().includes(path);
}

export function forgetDownloadedSub(path: string) {
  try {
    localStorage.setItem(
      DOWNLOADED_SUBS_KEY,
      JSON.stringify(downloadedSubs().filter((x) => x !== path)),
    );
  } catch {
    // nothing to do
  }
}

// ---- Recent links ---------------------------------------------------------

const LINKS_KEY = 'frameplayer.links';
/// Declared here rather than in torrent.svelte.ts because `IDENTIFIED_STORES`
/// has to name it and the dependency runs torrent → history, never back.
export const TORRENTS_KEY = 'frameplayer.torrents';
const LINKS_LIMIT = 8;

/// Links, most recent first. Kept apart from the watch positions because they
/// answer a different question: not "where did I stop" but "what did I paste" —
/// a link is worth offering again even when it was closed after ten seconds,
/// which is exactly when nothing reaches the position store.
export function recentLinks(): string[] {
  if (!history.prefs.enabled) return [];
  try {
    const list = JSON.parse(localStorage.getItem(LINKS_KEY) ?? '[]');
    return Array.isArray(list) ? list.filter((x) => typeof x === 'string') : [];
  } catch {
    return [];
  }
}

export function rememberLink(url: string) {
  if (!history.prefs.enabled) return;
  try {
    // Compared by identity, so the same video pasted from a share button and
    // from the address bar does not fill the list twice.
    const id = sourceId(url);
    const next = [url, ...recentLinks().filter((x) => sourceId(x) !== id)].slice(0, LINKS_LIMIT);
    localStorage.setItem(LINKS_KEY, JSON.stringify(next));
  } catch {
    // localStorage unavailable — not critical
  }
}

export function forgetLinks() {
  try {
    localStorage.removeItem(LINKS_KEY);
  } catch {
    // nothing to do
  }
}

// ---- Chosen tracks, per file and per folder -------------------------------
//
// Two scopes, and the second is the point: watching episode 1 with the Russian
// dub should mean episode 2 opens with it too, and the file-keyed store can
// never say that — the next episode is a path it has never seen.
//
// The folder is the scope rather than "everywhere", because a folder is what a
// season is, and it is already what the play queue is built from. A global
// memory would carry an anime's Japanese track onto an unrelated film; a
// per-queue one would not survive a restart. A folder of assorted downloads
// degrades to roughly "the last thing I picked", which is a fair answer there.

const FOLDER_TRACKS_KEY = 'frameplayer.tracks.folder';

/// Case-insensitive, like the privacy roots and for the same reason: the same
/// folder comes back spelled differently depending on how the file was opened.
function folderKey(path: string): string {
  const cut = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return (cut > 0 ? path.slice(0, cut) : path).toLowerCase();
}

function entriesLoad(key: string): Record<string, TrackEntry> {
  try {
    return JSON.parse(localStorage.getItem(key) ?? '{}');
  } catch {
    return {};
  }
}

function entriesSave(key: string, map: Record<string, TrackEntry>, limit: number) {
  try {
    const keys = Object.keys(map);
    if (keys.length > limit) {
      keys.sort((a, b) => map[a].ts - map[b].ts);
      for (const k of keys.slice(0, keys.length - limit)) delete map[k];
    }
    localStorage.setItem(key, JSON.stringify(map));
  } catch {
    // localStorage unavailable — not critical
  }
}

/**
 * Record an explicit pick, for this file and for its folder.
 *
 * Both, because they answer different questions: the file remembers exactly
 * what was playing here, the folder remembers what to look for next door. Only
 * explicit picks get here — mirroring back whatever mpv auto-selected would
 * freeze it against later `alang`/`slang` changes for no benefit.
 */
export function rememberTrack(path: string, kind: 'audio' | 'sub', wish: TrackWish) {
  if (isPrivatePath(path)) return;
  const scopes: [string, string, number][] = [[TRACKS_KEY, sourceId(path), 300]];
  // A stream has no folder. Taking one from a URL would put every video on a
  // site into a single scope — one `youtube.com/watch` bucket deciding the dub
  // for everything watched there.
  if (!isNetworkSource(path)) scopes.push([FOLDER_TRACKS_KEY, folderKey(path), 200]);
  for (const [key, id, limit] of scopes) {
    const map = entriesLoad(key);
    map[id] = { ...map[id], [kind]: wish, ts: Date.now() };
    entriesSave(key, map, limit);
  }
}

/**
 * Record a delay the viewer dialled in. Per source only — never per folder:
 * a mux error belongs to one file, and applying one episode's correction to
 * the next would be inventing a fault the next one may not have.
 */
export function rememberDelay(path: string, kind: 'sub' | 'audio', seconds: number) {
  if (isPrivatePath(path)) return;
  const map = entriesLoad(TRACKS_KEY);
  const id = sourceId(path);
  const field = kind === 'sub' ? 'subDelay' : 'audioDelay';
  const entry: TrackEntry = { ...map[id], ts: Date.now() };
  // Zero is the default, so it is stored as "nothing to say" rather than as a
  // value — otherwise resetting a delay would leave a record behind for ever.
  if (seconds === 0) delete entry[field];
  else entry[field] = seconds;
  map[id] = entry;
  entriesSave(TRACKS_KEY, map, 300);
}

/// Delays recorded for this source, zero meaning none. Zero is also what must
/// be applied when there is no record: mpv keeps `sub-delay` across a file
/// change (measured), so an unset one is the previous file's still in force.
export function delaysFor(path: string): { sub: number; audio: number } {
  if (isPrivatePath(path)) return { sub: 0, audio: 0 };
  const entry = entriesLoad(TRACKS_KEY)[sourceId(path)];
  return { sub: entry?.subDelay ?? 0, audio: entry?.audioDelay ?? 0 };
}

/// What to look for in this file: what was picked here before, or failing that
/// what was picked in this folder.
export function trackWishFor(path: string, kind: 'audio' | 'sub'): TrackWish | null {
  if (isPrivatePath(path)) return null;
  const own = entriesLoad(TRACKS_KEY)[sourceId(path)]?.[kind];
  if (own || isNetworkSource(path)) return own ?? null;
  return entriesLoad(FOLDER_TRACKS_KEY)[folderKey(path)]?.[kind] ?? null;
}

/// `title` is what the player itself displayed — mpv's `media-title`, i.e. the
/// container's own metadata, or the real name yt-dlp resolved for a link. Kept
/// because the start screen has nothing else to go on: from a path alone the
/// best possible answer is a tidied file name, and for a URL it is a video id.
/// Keyed by `sourceId`, not by path — see source.ts. `src` is what to hand
/// `loadfile` to open it again, which for a local file is the key itself; that
/// identity is what lets every record written before this change be read
/// without a migration.
export function positionsLoad(): Record<
  string,
  { pos: number; dur: number; ts: number; title?: string; src?: string }
> {
  try {
    return JSON.parse(localStorage.getItem(POSITIONS_KEY) ?? '{}');
  } catch {
    return {};
  }
}

export function loadHistoryPrefs() {
  try {
    const raw = localStorage.getItem(HISTORY_KEY);
    if (raw) history.prefs = { enabled: true, excluded: [], ...JSON.parse(raw) };
  } catch {
    // corrupt entry — the defaults stay
  }
  pushPrivatePaths();
}

function saveHistoryPrefs() {
  try {
    localStorage.setItem(HISTORY_KEY, JSON.stringify(history.prefs));
  } catch {
    // not critical
  }
  pushPrivatePaths();
}

/// Hand the list to Rust, where it decides whether to create a thumbnail cache
/// file at all. History switched off is private wholesale, not by folder.
function pushPrivatePaths() {
  void invoke('set_private_paths', {
    paths: history.prefs.excluded,
    all: !history.prefs.enabled,
  }).catch(() => {});
}

/// Mirror of the Rust `is_private` check: matching on a path component boundary
/// (otherwise "/Movies" would swallow "/Movies2"), case-insensitive.
export function isPrivatePath(path: string): boolean {
  if (!history.prefs.enabled) return true;
  return history.prefs.excluded.some((root) => pathUnder(path, root));
}

/**
 * Past this, a video counts as finished: its position record is deleted, so it
 * leaves "continue watching" rather than sitting there at 99 %.
 *
 * Exported because that deletion is *why* a finished episode leaves no trace,
 * and the torrent store keeps its own record of watched episodes precisely to
 * fill the gap. Two thresholds that must agree would drift; one does not.
 */
export const FINISHED_FRACTION = 0.97;

export function persistPosition(path: string, pos: number, dur: number, title?: string) {
  if (isPrivatePath(path)) return;
  try {
    const map = positionsLoad();
    const id = sourceId(path);
    // The "started watching" threshold is 15 seconds, but no more than 5% of
    // the duration. A flat 15 s on its own made short clips unrecordable in
    // principle: a 17-second clip had a two-second window between "not started"
    // (15 s) and "already finished" (97% = 17 s), and anything under 15.5 s
    // never entered the history at all. Long files are unaffected: 5% of a
    // two-hour film is 6 minutes, so the threshold stays those same 15 s.
    const startedAt = dur > 0 ? Math.min(15, dur * 0.05) : 15;
    const watched = pos >= startedAt;
    const finished = dur > 0 && pos / dur > FINISHED_FRACTION;
    if (!watched || finished) delete map[id];
    // The title is only overwritten by a real one: mpv reports `media-title` a
    // beat after the file opens, and a null in the meantime must not erase what
    // an earlier viewing recorded.
    else map[id] = { pos, dur, ts: Date.now(), title: title || map[id]?.title, src: path };
    const keys = Object.keys(map);
    if (keys.length > 300) {
      keys.sort((a, b) => map[a].ts - map[b].ts);
      for (const k of keys.slice(0, keys.length - 300)) delete map[k];
    }
    localStorage.setItem(POSITIONS_KEY, JSON.stringify(map));
  } catch {
    // localStorage unavailable — not critical
  }
}

/// Last position seen, so it can still be committed once the file is already
/// gone from mpv (a playlist switch blanks the mirrors first).
let lastKnownPos: { path: string; pos: number; dur: number; title: string | null } | null = null;
let lastPosSaveTs = 0;

/** Updates lastKnownPos on every call, writes to the store at most every 5 s. */
export function maybeRecordPosition(force = false) {
  if (!player.filePath || player.duration <= 0) return;
  lastKnownPos = {
    path: player.filePath,
    pos: player.timePos,
    dur: player.duration,
    // What the title bar is showing, which for a link is the name yt-dlp
    // resolved and for a container its own metadata.
    title: player.mediaTitle,
  };
  const now = performance.now();
  if (!force && now - lastPosSaveTs < 5000) return;
  lastPosSaveTs = now;
  persistPosition(player.filePath, player.timePos, player.duration, player.mediaTitle ?? undefined);
}

/** Commit whatever was last seen — on file change and on window close. */
export function flushPosition() {
  if (!lastKnownPos) return;
  persistPosition(
    lastKnownPos.path,
    lastKnownPos.pos,
    lastKnownPos.dur,
    lastKnownPos.title ?? undefined,
  );
  lastKnownPos = null;
}

/**
 * Snapshot of "what to open after the restart" (same file, same position).
 * Written BEFORE an update install: on Windows the NSIS installer kills the
 * process inside downloadAndInstall, and code after it never runs.
 */
/// Note this one keeps a PATH, not a source id: it is not a lookup key but the
/// thing handed to `loadfile` after the restart. The id answers "which video is
/// this"; the path answers "how do I open it", and here only the second is
/// wanted.
export function saveResumeSnapshot() {
  // A private file must not survive the restart: the snapshot sits in
  // localStorage in plain text and restores itself, unasked.
  if (player.filePath && !isPrivatePath(player.filePath)) {
    localStorage.setItem(
      RESUME_KEY,
      JSON.stringify({ path: player.filePath, pos: player.timePos, paused: player.paused }),
    );
  } else {
    localStorage.removeItem(RESUME_KEY);
  }
}

export function dropResumeSnapshot() {
  localStorage.removeItem(RESUME_KEY);
}

/// The list reloads on every return to the start screen, and posters from the
/// previous pass must not append themselves to it.
const recentReads = latest();

/**
 * Builds the "continue watching" list and pulls in posters. Entries whose file
 * vanished drop out silently: poster_frame returns an error, and keeping
 * something unopenable in the list is pointless.
 */
/// Where to decode this entry's poster from, or null for "there is none".
///
/// A torrent episode stores a loopback URL, so the plain path check skipped it
/// and its card showed the link mark for ever. The file is on disk under the
/// info hash and the cached `.torrent` beside it names the file —
/// `torrent_offline_file` answers from those two **without adding the torrent**,
/// joining the DHT or connecting to a peer, which is what kept this off the
/// start screen before. It answers only for a complete file: the holes in an
/// incomplete one read back as zeros, so a poster decoded from one would be a
/// black rectangle presented as a frame of the film.
async function posterSource(item: RecentItem): Promise<string | null> {
  const torrent = parseTorrentUrl(item.path);
  if (torrent) {
    const local = await invoke<{ path: string; complete: boolean } | null>(
      'torrent_offline_file',
      { infoHash: torrent.infoHash, index: torrent.index },
    ).catch(() => null);
    return local?.path ?? null;
  }
  return isNetworkSource(item.path) ? null : item.path;
}

export async function loadRecent() {
  const run = recentReads.begin();
  // Blobs from the previous list live until explicitly revoked — otherwise
  // every return to the start screen would leak a handful of URLs.
  for (const r of history.recent) if (r.poster) URL.revokeObjectURL(r.poster);
  if (!history.prefs.enabled) {
    history.recent = [];
    return;
  }
  const map = positionsLoad();
  const pool: RecentItem[] = Object.entries(map)
    .filter(([id, v]) => !isPrivatePath(v.src ?? id))
    .map(([id, v]) => ({
      id,
      // What to open. Records from before source ids were keyed by the path
      // itself, so the key is the fallback and nothing needs rewriting.
      path: v.src ?? id,
      name: v.title || displayName(v.src ?? id),
      pos: v.pos,
      dur: v.dur,
      ts: v.ts ?? 0,
      poster: null,
    }))
    .sort((a, b) => b.ts - a.ts)
    .slice(0, RECENT_LIMIT * 3);
  // **Verified before it is published.** This list used to go on screen in full
  // and then shed entries one at a time as their posters failed — a grid of
  // black cards that collapsed a moment later, including files renamed months
  // ago. A `stat` is microseconds where decoding a poster is not, so the check
  // happens first and nothing unreachable is ever shown.
  //
  // Only local files can be checked this way, and only they need it: a URL does
  // not rot the way a renamed file does, and there is nothing cheap to ask about
  // one anyway.
  const locals = pool.filter((r) => !isNetworkSource(r.path)).map((r) => r.path);
  const alive = new Set<string>();
  if (locals.length) {
    const ok = await invoke<boolean[]>('paths_exist', { paths: locals }).catch(
      () => locals.map(() => true),
    );
    if (run.stale) return;
    locals.forEach((p, i) => {
      if (ok[i]) alive.add(p);
    });
  }
  const usable = pool.filter((r) => isNetworkSource(r.path) || alive.has(r.path));
  history.recent = usable.slice(0, RECENT_LIMIT);
  // **The returned promise ends here, with the list published and no picture in
  // it yet.** That is what lets the caller at launch await this before showing
  // the window: the existence check is one batched `stat`, so the rail is on
  // screen at its final size from the first frame instead of appearing a beat
  // later and pushing everything below it down. Awaiting the posters instead
  // would mean holding the window for a dozen keyframe decodes.
  void loadPosters(run);
}

/**
 * Fill in the pictures, one at a time rather than in a batch: each one decodes a
 * frame in the worst case, and starting a dozen decoders for the start screen is
 * exactly the greed already cured in the storyboard.
 *
 * **A missing poster does not remove the entry.** `poster_frame` opens a path,
 * so it fails for every network source — a YouTube link, a torrent episode — and
 * dropping those on that basis meant they could never stay in the list at all,
 * which quietly disabled resuming a torrent from a card. Whether the source is
 * gone is decided by the existence check in `loadRecent`; this loop only decides
 * whether there is a picture.
 *
 * Not awaited by `loadRecent`, and the attempt guard is what makes that safe: a
 * second call supersedes this one mid-flight. It is the caller's attempt that
 * is passed in, not a new one — this loop is the tail of that same pass, and
 * beginning another here would let two lists fill each other's posters in.
 */
async function loadPosters(run: Attempt) {
  for (const item of history.recent) {
    if (run.stale) return;
    // **A torrent episode has a poster too, and finding it costs no session.**
    // Its stored path is a loopback URL, so this loop used to skip it and the
    // card showed the link mark for ever. The file is on disk under the info
    // hash, and the cached `.torrent` beside it names it — `torrent_offline_file`
    // answers from those two without adding the torrent, joining the DHT or
    // connecting to a peer. It answers only for a **complete** file: the holes
    // in an incomplete one read back as zeros, and a poster decoded from one is
    // a black rectangle presented as a frame of the film.
    // A poster captured while the file played beats anything decodable now: it
    // exists for entries whose file is incomplete, gone or unreadable, and it
    // was chosen for looking like a picture rather than for sitting at a
    // particular second.
    const saved = await invoke<ArrayBuffer>('poster_saved', { id: item.id }).catch(() => null);
    const source = saved ? null : await posterSource(item);
    if (!saved && !source) continue;
    try {
      const buf =
        saved ?? (await invoke<ArrayBuffer>('poster_frame', { path: source!, pos: item.pos }));
      if (run.stale) return;
      if (buf.byteLength > 8) {
        const url = URL.createObjectURL(new Blob([new Uint8Array(buf, 8)], { type: 'image/jpeg' }));
        // The entry may have been removed by hand while we decoded.
        const live = history.recent.find((r) => r.path === item.path);
        if (live) live.poster = url;
        else URL.revokeObjectURL(url);
      }
    } catch {
      // A file that exists but will not decode keeps its place without a
      // picture, exactly like a link does.
    }
  }
}

/**
 * Remove one video from the history, along with its thumbnails on disk.
 *
 * **The argument is how to open it; what gets deleted is what it *is*.** This
 * used to delete `map[path]` from two stores by hand, and every store here is
 * keyed by `sourceId` — which equals the path for a local file and for nothing
 * else. So a local card went and the other two kinds came back at the next
 * `loadRecent`: a torrent episode always (its path is a loopback URL, its key is
 * `torrent:<hash>/<index>`) and a link whenever `sourceId` had canonicalised it,
 * which a share URL with `?si=` always has. It looked like a delete that worked
 * because the card also leaves `history.recent` below, so nothing re-read
 * localStorage until the next visit to the start screen — or, as it was
 * reported, until the next launch.
 *
 * Going through `purgeStores` rather than fixing the two `delete`s is what makes
 * that hold for a store added later, and it closes a leak the hand-written pair
 * had: the remembered titles were not touched, and that store is keyed by
 * identity, so for a local file it is a list of everything ever watched. The
 * other stores answer this predicate correctly by themselves — a torrent is
 * filed under `torrent:<hash>/`, so forgetting one episode cannot forget the
 * season, the folder track memory is keyed by the folder, and the downloaded
 * subtitles by their own paths.
 */
export function forgetRecent(path: string) {
  const id = sourceId(path);
  purgeStores((other) => other === id);
  // Thumbnails are addressed in Rust by a hash of the file path, so this half
  // is the one thing that legitimately still speaks in paths.
  void invoke('forget_thumbs', { path }).catch(() => {});
  const item = history.recent.find((r) => r.id === id);
  if (item?.poster) URL.revokeObjectURL(item.poster);
  history.recent = history.recent.filter((r) => r.id !== id);
}

/**
 * Forget everything, on request.
 *
 * Driven by the same list as the two selective purges, and that is a fix rather
 * than tidying: this used to name three keys by hand and cleared three of the
 * six. What survived was the remembered titles — keyed by source id, which for a
 * local file *is* the path, so that store on its own is a list of everything
 * ever watched — along with the per-folder track memory and the paths of every
 * subtitle the player had downloaded.
 */
export async function clearHistory() {
  forgetLinks();
  for (const store of IDENTIFIED_STORES) {
    try {
      localStorage.removeItem(store.key);
    } catch (e) {
      console.warn(`clearing ${store.key} failed:`, e);
    }
  }
  await invoke('clear_thumb_cache').catch(() => {});
  for (const r of history.recent) if (r.poster) URL.revokeObjectURL(r.poster);
  history.recent = [];
  showOsd(t('osd.history_cleared'));
}

// ---- The stores, as a list rather than as three lists ---------------------

/**
 * Every localStorage store that holds something about a *particular video*, and
 * where inside it that video's identity sits.
 *
 * Four separate operations have to walk this set — excluding a folder,
 * forgetting a torrent, forgetting one card, and clearing the history outright —
 * and each of them used to enumerate the stores by hand. Hand-written lists of
 * the same thing are one list too many even before they disagree, and they did
 * twice: **`clear history` cleared three of the six**, leaving the remembered
 * titles (which for a local file are keyed by the path, so that store *is* a
 * list of everything watched), the per-folder track memory and the
 * downloaded-subtitle paths sitting in localStorage after the viewer had asked
 * for all of it to go — and `forgetRecent` named two stores, deleted from both
 * by the wrong kind of string, and left the titles behind as well.
 *
 * **Every identity here is `sourceId`-shaped, never a path that happens to
 * work.** Positions carry `src` beside the key because the two answer different
 * questions — the id is "which video", `src` is "how do I open it" — and they
 * coincide only for a local file; a purge must ask the first question, or it
 * asks the transport where the film went.
 *
 * `frameplayer.links` is deliberately absent: a URL has no folder and no
 * position, so nothing in it can belong to one. It is cleared by name in
 * `clearHistory` alongside these. `frameplayer.history` is the preferences
 * themselves, which are not about a video at all.
 */
interface IdentifiedStore {
  key: string;
  /** Drop every entry whose identity matches. Never throws for one bad store. */
  purge(matches: (id: string) => boolean): void;
}

function jsonAt(key: string, fallback: string): unknown {
  try {
    return JSON.parse(localStorage.getItem(key) ?? fallback);
  } catch {
    return JSON.parse(fallback);
  }
}

/// A `Record<identity, entry>`. `identityOf` exists for the one store whose key
/// is not the whole answer.
function mapStore(
  key: string,
  identityOf: (id: string, entry: unknown) => string = (id) => id,
): IdentifiedStore {
  return {
    key,
    purge(matches) {
      const map = jsonAt(key, '{}') as Record<string, unknown>;
      let hit = false;
      for (const [id, entry] of Object.entries(map)) {
        if (!matches(identityOf(id, entry))) continue;
        delete map[id];
        hit = true;
      }
      if (hit) localStorage.setItem(key, JSON.stringify(map));
    },
  };
}

/// A bare list of paths.
function listStore(key: string): IdentifiedStore {
  return {
    key,
    purge(matches) {
      const list = jsonAt(key, '[]');
      if (!Array.isArray(list)) return;
      const kept = list.filter((p: unknown) => typeof p !== 'string' || !matches(p));
      if (kept.length !== list.length) localStorage.setItem(key, JSON.stringify(kept));
    },
  };
}

/// One object about one video, which is dropped whole or not at all.
function singleStore(key: string, field: string): IdentifiedStore {
  return {
    key,
    purge(matches) {
      const raw = localStorage.getItem(key);
      if (raw === null) return;
      let value: unknown;
      try {
        value = JSON.parse(raw);
      } catch {
        // Unreadable: it cannot be shown to be safe, so it goes.
        localStorage.removeItem(key);
        return;
      }
      const id = (value as Record<string, unknown> | null)?.[field];
      if (typeof id === 'string' && matches(id)) localStorage.removeItem(key);
    },
  };
}

const IDENTIFIED_STORES: IdentifiedStore[] = [
  // **The key, not the `src` beside it.** Reading the identity out of `src` was
  // wrong in the one case where the two differ at all: for a local file the key
  // *is* the path, so a folder purge is unaffected either way, while a torrent's
  // `src` is the loopback URL its session happened to serve it from — so
  // `purgeTorrentHistory`, which asks a `torrent:<hash>/` question, matched
  // nothing and left a position behind for every episode of a deleted torrent.
  // It passed its test because the fixture seeded entries with no `src` at all,
  // which `persistPosition` never writes.
  mapStore(POSITIONS_KEY),
  // Keyed by the bare info hash, so its identity is spelled the way a torrent's
  // is everywhere else. **It was missing entirely** — "forget everything" left
  // behind a magnet per torrent ever opened, which names what was watched as
  // plainly as a position does, and now also which episodes were finished. The
  // torrent purge reaches it through `forgetRememberedTorrent` as well; a
  // second, harmless, route.
  mapStore(TORRENTS_KEY, (id) => `torrent:${id}/`),
  mapStore(TRACKS_KEY),
  // Keyed by the folder rather than the file, which the predicates handle for
  // free: a folder is under itself, and a torrent id is not a folder.
  mapStore(FOLDER_TRACKS_KEY),
  mapStore(TITLES_KEY),
  listStore(DOWNLOADED_SUBS_KEY),
  // The snapshot restores itself after an update, unasked — a video the viewer
  // has excluded must not be what comes back.
  singleStore(RESUME_KEY, 'path'),
];

/**
 * Drop everything the stores hold about videos whose identity `matches`.
 *
 * The predicate is the only thing that differs between excluding a folder and
 * forgetting a torrent, which is the whole point: a store added to the list
 * above is covered by both without either being edited, and a predicate that
 * has nothing to say about a store simply matches none of its entries.
 */
function purgeStores(matches: (id: string) => boolean) {
  for (const store of IDENTIFIED_STORES) {
    try {
      store.purge(matches);
    } catch (e) {
      // One unreadable store must not stop the others: a purge that silently
      // did half its work is the failure this is guarding against.
      console.warn(`purging ${store.key} failed:`, e);
    }
  }
}

/**
 * Delete everything already recorded about the videos inside a folder.
 *
 * Excluding a folder used to mean only "stop recording, and hide what is
 * there": the entries stayed in localStorage and the frames stayed in the
 * thumbnail cache, so removing the folder from the list brought them all back.
 * That is the wrong half of a privacy control — the start screen was clean
 * while the disk was not.
 *
 * The thumbnails are Rust's to delete, because their cache is addressed by a
 * hash of the path.
 */
async function purgeFolder(dir: string) {
  purgeStores((id) => pathUnder(id, dir));
  await invoke('forget_thumbs_under', { folder: dir }).catch((e) => {
    console.warn('purging cached thumbnails failed:', e);
  });
}

/**
 * Forget everything recorded about one torrent.
 *
 * The torrent's own data is deleted by `forgetTorrent` in torrent.svelte.ts;
 * this is the history half, kept here because it is the only module that knows
 * which stores exist and how they are keyed. Every one of them files a torrent
 * under `torrent:<hash>/<index>` (see `sourceId`), so a prefix match over the
 * identity is the whole rule — where excluding a folder asks the same stores a
 * path-shaped question, this asks an id-shaped one and the stores do not care
 * which. The path-keyed ones (the folder track scope, the downloaded subtitles,
 * the resume snapshot) simply match nothing, which is the right answer: a
 * torrent has no folder, and its snapshot holds a loopback URL.
 */
export function purgeTorrentHistory(infoHash: string) {
  const prefix = `torrent:${infoHash.toLowerCase()}/`;
  const mine = (id: string) => id.toLowerCase().startsWith(prefix);
  purgeStores(mine);
  for (const item of history.recent) {
    if (mine(item.id) && item.poster) URL.revokeObjectURL(item.poster);
  }
  history.recent = history.recent.filter((item) => !mine(item.id));
}

/** Drop a magnet from the recent-links list — part of forgetting a torrent. */
export function forgetLink(url: string) {
  try {
    localStorage.setItem(LINKS_KEY, JSON.stringify(recentLinks().filter((u) => u !== url)));
  } catch {
    // not critical
  }
}

export async function addExcludedFolder() {
  const dir = await open({ directory: true, multiple: false }).catch(() => null);
  if (typeof dir !== 'string') return;
  if (!history.prefs.excluded.includes(dir)) {
    history.prefs.excluded = [...history.prefs.excluded, dir];
    saveHistoryPrefs();
    // Order matters: the folder is in the list before anything is deleted, so a
    // write racing this purge (the position timer fires every ~5 s) is already
    // refused by `isPrivatePath` rather than landing behind it.
    await purgeFolder(dir);
    void loadRecent();
  }
}

export function removeExcludedFolder(dir: string) {
  history.prefs.excluded = history.prefs.excluded.filter((d) => d !== dir);
  saveHistoryPrefs();
  void loadRecent();
}

export function toggleHistory() {
  history.prefs.enabled = !history.prefs.enabled;
  saveHistoryPrefs();
  void loadRecent();
}
