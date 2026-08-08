/**
 * Torrent streaming, player side (ROADMAP 21).
 *
 * The Rust half (torrent.rs) turns a magnet into a list of files, each with a
 * URL on our own loopback server; from mpv's point of view what follows is an
 * ordinary network stream, which is why so little of the player had to change.
 * What this module owns is the part that is *not* ordinary:
 *
 * **A stall over a torrent has a reason, and the player is the only thing that
 * can say what it is.** mpv reports `paused-for-cache` — "waiting" — and stops
 * there. Whether that means no peers, a slow swarm, or a jump into a cold region
 * that is filling at 3 MB/s is the difference between "this is broken" and "this
 * needs ten more seconds", and it is the whole difference between a torrent
 * player people tolerate and one they do not. So the status is polled while a
 * torrent is playing and the numbers are put where the stall is shown.
 *
 * **Which file of the torrent is playing is part of the identity.** A torrent is
 * a list of files and mpv's playlist holds all of them, so the poll target moves
 * with `path` rather than being fixed when the magnet was opened.
 *
 * The polling stops with the torrent — a timer that outlives the viewing is a
 * background BitTorrent client, which this app is deliberately not.
 */

import { invoke } from '@tauri-apps/api/core';
import { command } from 'tauri-plugin-libmpv-api';

import { baseName, displayName, extensionOf } from './format';
import { history, positionsLoad } from './history.svelte';
import { SUBTITLE_EXTENSIONS, VIDEO_EXTENSIONS, player } from './player.svelte';
import { parseTorrentUrl, torrentId } from './source';

export interface TorrentFile {
  index: number;
  /// Path inside the torrent, `/`-separated.
  path: string;
  size: number;
  /// What to hand `loadfile`.
  url: string;
}

export interface TorrentInfo {
  info_hash: string;
  name: string | null;
  files: TorrentFile[];
}

export interface TorrentStatus {
  state: 'initializing' | 'live' | 'paused' | 'error' | 'gone';
  error: string | null;
  peers: number;
  peers_seen: number;
  down_bps: number;
  up_bps: number;
  file_done: number;
  file_size: number;
  total_done: number;
  total_size: number;
}

/// How often the status is re-read while a torrent is playing.
///
/// One second, like the media info panel's bitrate refresh and for the same
/// reason: these are figures a viewer watches move while deciding whether to
/// wait, and a slower tick reads as a frozen readout.
const POLL_MS = 1000;

const PREFS_KEY = 'frameplayer.torrent';

/**
 * Whether to upload to other peers while streaming.
 *
 * **Off by default, and that default is a safety decision rather than a
 * technical one.** In Germany and several other jurisdictions the exposure from
 * *uploading* copyrighted material is categorically worse than from downloading
 * it — enforcement there works by connecting to peers who are advertising
 * pieces and recording that they served data. A video player must not opt its
 * users into that silently, and "the client defaults to it" is not a reason: the
 * viewer asked to watch something, not to redistribute it.
 *
 * What it buys when off is real but bounded, and worth stating honestly rather
 * than overselling: librqbit stops advertising which pieces it holds (no
 * bitfield, no `have` messages), refuses piece requests outright, and drops
 * peers once the file is complete. What it does **not** do is make the client
 * invisible — finding peers at all means announcing to trackers and the DHT, so
 * membership of the swarm is still observable. It removes the serving, not the
 * presence.
 */
class TorrentPrefs {
  seeding = $state(false);
}

export const torrentPrefs = new TorrentPrefs();

export function loadTorrentPrefs() {
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    if (!raw) return;
    const saved = JSON.parse(raw) as { seeding?: boolean };
    if (typeof saved.seeding === 'boolean') torrentPrefs.seeding = saved.seeding;
  } catch {
    // A corrupted preference must not cost the safe default.
  }
}

/**
 * Returns true when a running session had to be torn down to apply this, which
 * also ended whatever was streaming. Only ever happens on a real change.
 */
export async function setSeeding(on: boolean): Promise<boolean> {
  torrentPrefs.seeding = on;
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify({ seeding: on }));
  } catch {
    // not critical
  }
  return await invoke<boolean>('torrent_set_seeding', { seeding: on }).catch(() => false);
}

/// Stretches of the playing file that are on disk, as fractions [0..1].
///
/// Read a little slower than the status: it is a map, not a counter, and the
/// eye does not track a band creeping by half a percent. Cheap either way —
/// measured, a seven-gigabyte season is ~5.6 KB of bitmap.
const BUFFER_POLL_MS = 2000;

class TorrentState {
  /// Merged, sorted, non-overlapping.
  buffered = $state<[number, number][]>([]);
  /// The torrent feeding the player, if any. Kept after the file is chosen
  /// because the file picker, the queue and the info panel all read it.
  info = $state<TorrentInfo | null>(null);
  /// Live figures for the file being played. Null when nothing is polling.
  status = $state<TorrentStatus | null>(null);
  /// A magnet is being resolved: the DHT lookup that turns an info hash into a
  /// file list, which routinely takes ten seconds and can take a minute.
  resolving = $state(false);
}

export const torrent = new TorrentState();

/**
 * Subtitle files inside the torrent that belong to a given video.
 *
 * Worth having because torrents routinely ship them — Sintel carries ten — and
 * without this they sit in the file list unreachable, while the viewer goes
 * looking on OpenSubtitles for something already on their disk. `sub-add` takes
 * a URL, so each one is a request to our own server; they are a couple of
 * kilobytes, so fetching them costs nothing measurable.
 *
 * Matched by name rather than by "everything in the torrent", because a season
 * carries one set per episode and attaching all ninety would make the subtitle
 * menu useless. Three shapes cover what releases actually do: the same stem
 * (`Ep01.srt` beside `Ep01.mkv`), the stem plus a language tag (`Sintel.en.srt`
 * beside `Sintel.mp4`), and a subfolder named after the episode (`Subs/Ep01/…`).
 * A single-video torrent takes all of them — there is nothing else they could
 * belong to.
 */
export function torrentSubtitles(info: TorrentInfo, videoPath: string): TorrentFile[] {
  const subs = info.files.filter((f) => SUBTITLE_EXTENSIONS.includes(extensionOf(f.path)));
  if (!subs.length) return [];

  const videos = info.files.filter((f) => VIDEO_EXTENSIONS.includes(extensionOf(f.path)));
  if (videos.length <= 1) return subs;

  const stem = baseName(videoPath).replace(/\.[^.]+$/, '').toLowerCase();
  if (!stem) return [];
  return subs.filter((f) => {
    const p = f.path.toLowerCase();
    // `includes` rather than an exact match: the stem may be followed by a
    // language tag, or be a path component of its own.
    return p.includes(stem);
  });
}

/** Video files of a torrent, in the order a queue should play them. */
export function torrentVideos(info: TorrentInfo): TorrentFile[] {
  const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });
  return info.files
    .filter((f) => VIDEO_EXTENSIONS.includes(extensionOf(f.path)))
    .sort((a, b) => collator.compare(a.path, b.path));
}

/**
 * Resolve a magnet (or a `.torrent` URL) into its file list.
 *
 * Throws with a short reason the caller turns into a sentence — `resolve_timeout`
 * for a swarm that never answered, anything else for librqbit's own message.
 */
export async function addTorrent(source: string): Promise<TorrentInfo> {
  torrent.resolving = true;
  try {
    const info = await invoke<TorrentInfo>('torrent_add', {
      source,
      seeding: torrentPrefs.seeding,
    });
    torrent.info = info;
    return info;
  } finally {
    torrent.resolving = false;
  }
}

/// Guard so one file's subtitles are attached once, not on every `file-loaded`
/// (which fires again for a track change).
let subsAttachedFor: string | null = null;

/**
 * Attach the torrent's own subtitles for whatever just loaded.
 *
 * `auto` rather than `select`: mpv's `slang` should still decide which one wins,
 * exactly as it would for subtitles found beside a local file. Selecting the
 * first would override a preference the viewer has already expressed.
 */
export async function attachTorrentSubtitles() {
  const url = player.filePath;
  const info = torrent.info;
  if (!url || !info || subsAttachedFor === url) return;
  const ref = parseTorrentUrl(url);
  if (!ref || ref.infoHash !== info.info_hash) return;

  const video = info.files.find((f) => f.index === ref.index);
  if (!video) return;
  subsAttachedFor = url;

  for (const sub of torrentSubtitles(info, video.path)) {
    await command('sub-add', [sub.url, 'auto']).catch((e) =>
      console.warn('[torrent] sub-add failed:', e),
    );
  }
}

// ---- Remembering torrents --------------------------------------------------
//
// Three separate problems turned out to be one. A torrent entry in the watch
// history could not be reopened after a restart, because what it stores is a
// loopback URL and **the port is dead by then** — the very thing `sourceId`
// exists to work around, showing up one layer higher. The episodes of a torrent
// that were never started had no representation anywhere at all, so getting back
// into a season meant finding the magnet again. And the disk filled up silently.
//
// All three are answered by remembering, per info hash, the magnet that produced
// it. Note what is *not* kept here: the size and the folder, which are read from
// disk instead (`torrent_list`). Disk is the source of truth for storage, so
// what is taking space stays visible and deletable even with history switched
// off and nothing remembered — this store only adds the ability to *reopen*.

const STORE_KEY = 'frameplayer.torrents';
/// Torrents whose data is gone are dropped on read, so this only bounds a list
/// of strings that never grows on its own.
const STORE_LIMIT = 100;

export interface RememberedTorrent {
  infoHash: string;
  /// What to hand `torrent_add` to get playable URLs back.
  magnet: string;
  name: string | null;
  /// Video file count, for the row's "9 файлов" without resolving anything.
  videos: number;
  at: number;
}

type Store = Record<string, RememberedTorrent>;

function readStore(): Store {
  try {
    const raw = localStorage.getItem(STORE_KEY);
    return raw ? (JSON.parse(raw) as Store) : {};
  } catch {
    return {};
  }
}

/**
 * Record how to reopen a torrent.
 *
 * Privacy-gated exactly like the watch positions, and for the same reason: a
 * magnet names what is being watched as surely as a stored position does. With
 * history off nothing is written, and the storage list falls back to what disk
 * alone can say.
 */
export function rememberTorrent(info: TorrentInfo, magnet: string) {
  if (!history.prefs.enabled) return;
  try {
    const store = readStore();
    store[info.info_hash] = {
      infoHash: info.info_hash,
      magnet,
      name: info.name,
      videos: torrentVideos(info).length,
      at: Date.now(),
    };
    const trimmed = Object.values(store)
      .sort((a, b) => b.at - a.at)
      .slice(0, STORE_LIMIT);
    localStorage.setItem(
      STORE_KEY,
      JSON.stringify(Object.fromEntries(trimmed.map((t) => [t.infoHash, t]))),
    );
  } catch {
    // not critical
  }
}

export function rememberedTorrent(infoHash: string): RememberedTorrent | null {
  return readStore()[infoHash.toLowerCase()] ?? null;
}

export function forgetRememberedTorrent(infoHash: string) {
  try {
    const store = readStore();
    delete store[infoHash.toLowerCase()];
    localStorage.setItem(STORE_KEY, JSON.stringify(store));
  } catch {
    // not critical
  }
}

/**
 * Open a file of a torrent given only its source id, re-resolving the magnet.
 *
 * This is what makes a torrent entry in the watch history work at all: the URL
 * recorded next to it points at a port that no longer exists, so the entry has
 * to be re-derived rather than reused. Returns the fresh URL, or null when the
 * magnet was never recorded (history off) or the swarm is gone.
 */
export async function resolveTorrentFile(
  id: string,
): Promise<{ url: string; info: TorrentInfo } | null> {
  const match = /^torrent:([0-9a-f]{40})\/(\d+)$/i.exec(id);
  if (!match) return null;
  const [, infoHash, indexText] = match;
  const known = rememberedTorrent(infoHash);
  if (!known) return null;
  const info = await addTorrent(known.magnet).catch(() => null);
  const url = info?.files.find((f) => f.index === Number(indexText))?.url;
  // The whole torrent comes back with it, not just the one file: a season
  // opened from the history is still a season, and the caller needs the list to
  // put the other episodes in the queue.
  return url && info ? { url, info } : null;
}

/// What has already been asked for, so the one-second poll does not re-send it
/// every tick for the rest of the episode.
const prefetched = new Set<string>();

/**
 * Get the next episode ready while the current one plays.
 *
 * **One ahead, and only once the current file is complete** — those two bounds
 * are what keep this from being the background download manager the rest of the
 * app refuses to be. Fetching further would fill the disk with episodes that may
 * never be watched; fetching earlier would take bandwidth from the thing on
 * screen, which is the one thing a player must never do.
 *
 * "Next" is the torrent's own order, not mpv's playlist: the playlist may hold a
 * folder, a drop of loose files or nothing at all, while the sequence a season
 * is watched in is the one `torrentVideos` already sorts.
 */
async function prefetchNext(infoHash: string, index: number) {
  const info = torrent.info;
  if (!info || info.info_hash !== infoHash) return;
  const videos = torrentVideos(info);
  const at = videos.findIndex((f) => f.index === index);
  const next = at >= 0 ? videos[at + 1] : undefined;
  if (!next) return;

  const key = `${infoHash}/${next.index}`;
  if (prefetched.has(key)) return;
  prefetched.add(key);
  await invoke('torrent_prefetch', { infoHash, index: next.index }).catch(() => {
    // Let a failure be retried on a later tick rather than sticking for good.
    prefetched.delete(key);
  });
}

let timer: ReturnType<typeof setInterval> | undefined;
let bufferTimer: ReturnType<typeof setInterval> | undefined;

/**
 * Follow whatever the player is playing.
 *
 * Driven by `path` rather than started when a magnet is opened, because moving
 * to the next episode of a torrent is a playlist transition that this module
 * never hears about otherwise — and because the answer for a local file is
 * simply "stop polling".
 *
 * `onFileComplete` fires while the file being watched is fully on disk. It is a
 * callback rather than a call into the thumbnail service, and that is the one
 * import this module must not have: the two were the only cycle in `src/`
 * (`thumbs` asks *this* module where the data is, through `positionBuffered`,
 * which is a fair question about torrent state; this module telling the
 * thumbnail service to start is not). Expect it on every poll while the file is
 * complete, so whatever it starts has to be idempotent.
 */
export function trackTorrentPlayback(onFileComplete: () => void = () => {}) {
  $effect(() => {
    const ref = player.filePath ? parseTorrentUrl(player.filePath) : null;
    clearInterval(timer);
    timer = undefined;

    if (!ref) {
      torrent.status = null;
      torrent.buffered = [];
      return;
    }

    const read = async () => {
      try {
        const status = await invoke<TorrentStatus>('torrent_status', {
          infoHash: ref.infoHash,
          index: ref.index,
        });
        torrent.status = status;
        // The episode being watched is fully here, so the swarm is idle as far
        // as this file goes — the moment to get the next one ready, and the only
        // moment where doing so costs the current one nothing.
        if (status.file_size > 0 && status.file_done >= status.file_size) {
          void prefetchNext(ref.infoHash, ref.index);
          // The file has become an ordinary one on disk. Announced from here
          // because completion happens *during* playback — long after the
          // `path` and `duration` events anything else would hang off.
          onFileComplete();
        } else if (player.timePos > POSTER_AFTER_S && player.duration > 0) {
          // Still downloading: this is the only window in which a poster of it
          // can be taken at all, because after the session ends the holes in the
          // sparse file read back as zeros. Once per file, from what is on disk.
          void invoke<{ path: string; complete: boolean } | null>('torrent_local_path', {
            infoHash: ref.infoHash,
            index: ref.index,
          })
            .then((local) => {
              if (!local) return;
              return captureTorrentPoster(
                torrentId(ref.infoHash, ref.index),
                local.path,
                player.duration,
              );
            })
            .catch(() => {});
        }
      } catch {
        torrent.status = null;
      }
    };
    const readBuffered = async () => {
      try {
        torrent.buffered = await invoke<[number, number][]>('torrent_buffered', {
          infoHash: ref.infoHash,
          index: ref.index,
        });
      } catch {
        torrent.buffered = [];
      }
    };

    void read();
    void readBuffered();
    timer = setInterval(read, POLL_MS);
    bufferTimer = setInterval(readBuffered, BUFFER_POLL_MS);

    return () => {
      clearInterval(timer);
      clearInterval(bufferTimer);
      timer = undefined;
      bufferTimer = undefined;
    };
  });
}

// ---- What is on disk -------------------------------------------------------

export interface TorrentOnDisk {
  folder: string;
  info_hash: string | null;
  size: number;
}

/// One row of the start screen's torrent list: what disk says, plus what we
/// remembered about it, plus where the viewer left off.
export interface TorrentRow extends TorrentOnDisk {
  known: RememberedTorrent | null;
  /// Best display name: the torrent's own, else the folder — which for anything
  /// this build wrote is an info hash, hence the fallback label in the UI.
  name: string | null;
}

export async function listTorrents(): Promise<TorrentRow[]> {
  const disk = await invoke<TorrentOnDisk[]>('torrent_list').catch(() => [] as TorrentOnDisk[]);
  return disk.map((d) => {
    const known = d.info_hash ? rememberedTorrent(d.info_hash) : null;
    return { ...d, known, name: known?.name ?? (d.info_hash ? null : d.folder) };
  });
}

/**
 * Is mpv streaming from this torrent right now?
 *
 * The one reason a row must refuse to be deleted, and it used to be answered by
 * `torrent_list` as *session membership* — which is a different question: a
 * torrent stays in the session, paused, after the film is closed, so the flag
 * stayed true for the rest of the run and the delete button was dead from the
 * moment anything had been watched. Deleting a merely-loaded torrent is safe
 * anyway, since `forget` takes it out of the session before removing the
 * directory.
 *
 * Two things about how it is answered. The source is **what mpv has open**, not
 * `torrent.info` — that one is set when a magnet *resolves*, so a season whose
 * file picker was canceled would go on claiming to be playing with nothing
 * loaded at all. And it is a function rather than a field of the row, so it is
 * re-read whenever `filePath` changes: a value baked in when the list was
 * fetched would be a snapshot taken ~300 ms after the file was unloaded, and a
 * button stuck disabled is exactly the bug being fixed.
 */
export function torrentIsPlaying(row: TorrentOnDisk): boolean {
  const ref = player.filePath ? parseTorrentUrl(player.filePath) : null;
  if (!ref || !row.info_hash) return false;
  // Lower-cased on both sides: a folder from the older layout may be upper-case
  // hex, and `torrent_list` reports it as it found it.
  return ref.infoHash.toLowerCase() === row.info_hash.toLowerCase();
}

/**
 * Where the viewer left off inside this torrent — the newest position recorded
 * against any of its files.
 *
 * The single most useful line on a start-screen row, and the reason it is worth
 * a lookup at all: "продолжить: S01E03 — осталось 23:14" answers what the
 * torrent is *for*, where a size does not. It is also what lets a row resume
 * instead of dropping the file picker on someone hunting for the episode they
 * were already watching.
 *
 * Read straight from the position store rather than from `history.recent`,
 * which holds only the twelve newest entries across everything — a torrent
 * watched last week has fallen out of that long before its data has fallen off
 * the disk.
 */
export function torrentResume(
  row: TorrentOnDisk,
): { name: string; pos: number; dur: number; index: number } | null {
  if (!row.info_hash) return null;
  // Lower-cased, like the two other walks over this store and like
  // `torrentIsPlaying`: a folder from the older layout may be upper-case hex and
  // `torrent_list` reports it as it found it. This one was the odd one out —
  // comparing an upper-case prefix against a lower-cased id matches nothing, so
  // such a torrent's row silently lost its "continue watching" line and opened
  // the file picker instead of resuming.
  const prefix = `torrent:${row.info_hash.toLowerCase()}/`;
  let best: { name: string; pos: number; dur: number; index: number; ts: number } | null = null;
  for (const [id, rec] of Object.entries(positionsLoad())) {
    if (!id.toLowerCase().startsWith(prefix)) continue;
    const ts = rec.ts ?? 0;
    if (best && ts <= best.ts) continue;
    best = {
      name: rec.title || displayName(rec.src ?? id),
      pos: rec.pos,
      dur: rec.dur,
      index: Number(id.slice(prefix.length)),
      ts,
    };
  }
  return best;
}

/// Which files of a torrent have a saved position, by index — what the picker
/// marks, since a season is nine names differing by two characters.
export function torrentPositions(
  infoHash: string,
): Record<number, { pos: number; dur: number }> {
  const prefix = `torrent:${infoHash.toLowerCase()}/`;
  const out: Record<number, { pos: number; dur: number }> = {};
  for (const [id, rec] of Object.entries(positionsLoad())) {
    if (!id.toLowerCase().startsWith(prefix)) continue;
    out[Number(id.slice(prefix.length))] = { pos: rec.pos, dur: rec.dur };
  }
  return out;
}

/**
 * Delete a torrent completely: its data, and everything remembered about it.
 *
 * One coherent "forget this", because a half-deleted torrent is the confusing
 * state — files gone but the season still listed, or the reverse. The caller
 * supplies the history removal (positions and tracks live in history.svelte.ts
 * and are keyed by `torrent:<hash>/<index>`, which only that module knows how to
 * walk), so this returns the bytes freed and leaves the rest to it.
 */
export async function forgetTorrent(row: TorrentRow): Promise<number> {
  const freed = await invoke<number>('torrent_forget', { folder: row.folder }).catch(() => 0);
  if (row.info_hash) {
    forgetRememberedTorrent(row.info_hash);
    if (torrent.info?.info_hash === row.info_hash) {
      torrent.info = null;
      torrent.status = null;
    }
  }
  return freed;
}

// ---- Replacing a torrent with its re-upload ---------------------------------

/**
 * Does this name look like the same release as that one?
 *
 * The case it exists for: a tracker publishes "…Серии: 1-9 из 9…" and a week
 * later "…Серии: 1-10 из 10…" — the same forty-word title with two numbers
 * changed. Token overlap handles that without knowing anything about the
 * tracker's naming, where a prefix comparison would break on the first release
 * that puts the episode count at the front.
 *
 * Measured against the alternative that suggests itself: comparing the whole
 * strings, or their first N characters, calls a re-upload "different" as soon as
 * the changed digits move. Overlap of the *shorter* side against the longer is
 * what makes "1-9 из 9" and "1-10 из 10" score ~0.9 while two unrelated films
 * score near zero.
 */
export function looksLikeSameRelease(a: string, b: string): boolean {
  const tokens = (s: string) =>
    new Set(
      s
        .toLowerCase()
        .split(/[^\p{L}\p{N}]+/u)
        .filter((w) => w.length > 1),
    );
  const x = tokens(a);
  const y = tokens(b);
  if (x.size < 3 || y.size < 3) return false;
  const [small, large] = x.size <= y.size ? [x, y] : [y, x];
  let shared = 0;
  for (const w of small) if (large.has(w)) shared++;
  return shared / small.size >= 0.6;
}

/** A remembered torrent the given magnet looks like a newer release of. */
export function findSupersededTorrent(magnet: string): RememberedTorrent | null {
  const name = displayName(magnet);
  const hash = /xt=urn:btih:([0-9a-f]{40})/i.exec(magnet)?.[1]?.toLowerCase();
  for (const known of Object.values(readStore())) {
    // The same torrent is not an update of itself.
    if (!known.name || known.infoHash === hash) continue;
    if (looksLikeSameRelease(name, known.name) || looksLikeSameRelease(name, known.magnet)) {
      return known;
    }
  }
  return null;
}

/**
 * Hand an old torrent's data and history over to the re-upload that replaces it.
 *
 * Two things happen that a plain "open this magnet" cannot do, and they are the
 * entire reason this exists rather than telling people to paste the link in the
 * usual box. The **data** is kept: the folder is renamed to the new info hash
 * before the torrent is added, so librqbit verifies the episodes already there
 * and fetches only what is new. And the **watch positions** are kept: they are
 * keyed by `torrent:<hash>/<index>`, so without this a season's history would
 * reset to zero for the sake of one added episode.
 *
 * Positions are matched by **file name**, taken from the URL each record already
 * stores — the index cannot be trusted, since one inserted episode renumbers
 * everything after it.
 */
export async function updateTorrent(
  old: RememberedTorrent,
  magnet: string,
): Promise<{ info: TorrentInfo; matched: number; added: number }> {
  const newHash = /xt=urn:btih:([0-9a-f]{40})/i.exec(magnet)?.[1]?.toLowerCase();
  if (!newHash) throw new Error('no_hash');
  if (newHash === old.infoHash) throw new Error('same_torrent');

  // Before the add, or the new torrent creates an empty folder of its own and
  // there is nothing left to hand over.
  await invoke('torrent_relocate', { oldHash: old.infoHash, newHash }).catch(() => {});

  const info = await addTorrent(magnet);
  const moved = migratePositions(old.infoHash, info);
  forgetRememberedTorrent(old.infoHash);
  rememberTorrent(info, magnet);
  return { info, matched: moved, added: torrentVideos(info).length - moved };
}

/// Re-key every watch position from the old torrent onto the new one, by file
/// name. Returns how many were carried over — which is also what tells the
/// viewer the update found the right torrent.
function migratePositions(oldHash: string, info: TorrentInfo): number {
  if (!history.prefs.enabled) return 0;
  const prefix = `torrent:${oldHash.toLowerCase()}/`;
  const byName = new Map(info.files.map((f) => [baseName(f.path).toLowerCase(), f]));
  let moved = 0;
  try {
    const positions = positionsLoad();
    for (const [id, rec] of Object.entries(positions)) {
      if (!id.toLowerCase().startsWith(prefix)) continue;
      delete positions[id];
      // The name lives in the URL the record already carries — no extra store,
      // and it works for entries written before any of this existed.
      const name = rec.src ? decodeURIComponent(rec.src.split('/').pop() ?? '') : '';
      const file = byName.get(name.toLowerCase());
      if (!file) continue;
      positions[torrentId(info.info_hash, file.index)] = { ...rec, src: file.url };
      moved++;
    }
    localStorage.setItem('frameplayer.positions', JSON.stringify(positions));
  } catch {
    // A failed migration costs the history, not the data.
  }
  return moved;
}

/**
 * Is this position of the playing file on disk?
 *
 * The gate for previews before a file is complete: a position inside a buffered
 * range can be decoded, one outside would read zeros out of the sparse file and
 * produce a black frame with nothing to say it went wrong. Fractional rather
 * than in seconds because the map is of *bytes* — for a variable-bitrate file
 * the two are not proportional, so this is an approximation, and a deliberately
 * conservative one: `MARGIN` keeps requests away from the edge of a band, where
 * the byte-to-time estimate is least trustworthy and a GOP may start before the
 * range does.
 */
const BUFFER_MARGIN = 0.004;

export function positionBuffered(fraction: number): boolean {
  return torrent.buffered.some(
    ([a, b]) => fraction >= a + BUFFER_MARGIN && fraction <= b - BUFFER_MARGIN,
  );
}

/**
 * Stop feeding a torrent that is no longer being watched.
 *
 * Pausing rather than deleting: the pieces stay in the cache, so re-opening the
 * same magnet continues from where it stopped. What must not survive is the
 * peer traffic — uploading to strangers after the film is closed is not
 * something a video player may do quietly.
 */
export async function releaseTorrent() {
  const hash = torrent.info?.info_hash;
  torrent.info = null;
  torrent.status = null;
  if (hash) await invoke('torrent_release', { infoHash: hash }).catch(() => {});
}

// ---- Poster capture ---------------------------------------------------------

/// Ids captured this run, so the once-per-file rule survives a pause, a seek
/// and the one-second status poll.
const posterTried = new Set<string>();

/// Wait this long into a file before spending anything on a poster: the viewer
/// has committed to it, several seconds are buffered, and an episode abandoned
/// after ten seconds costs nothing.
const POSTER_AFTER_S = 45;

/**
 * Take the best frame of what has arrived and keep it for the start screen.
 *
 * The normal poster is decoded later, from the file, at the watch position —
 * which for a torrent still downloading is a black rectangle, because the holes
 * in a sparse file read back as zeros. Capturing while it plays inverts the
 * problem: the data is on disk exactly now, and the picture outlives both the
 * download and the file.
 *
 * Candidates are taken from the **buffered** map and spread across it rather
 * than clustered at the playhead: a poster should be a scene, and three
 * neighbouring frames are one scene. The scoring is Rust's — it has the pixels.
 */
export async function captureTorrentPoster(id: string, localPath: string, duration: number) {
  if (posterTried.has(id) || duration <= 0) return;
  const bands = torrent.buffered.filter(([a, b]) => b - a > 0.01);
  if (!bands.length) return;
  posterTried.add(id);

  // Up to four positions, biased to the middle of each band and away from the
  // very start of the film, where titles and black frames live.
  const candidates: number[] = [];
  for (const [a, b] of bands.slice(0, 3)) {
    // Spread across the band rather than clustered: three neighbouring frames
    // are one scene. No "skip the first N%" rule — after two minutes of a long
    // film the only band there *is* sits at the very start, and refusing it
    // means refusing the one window in which a poster can be taken at all. The
    // black frames a film opens with are handled where they should be, by the
    // scorer, which can see them.
    for (const f of [0.3, 0.55, 0.8]) {
      const fraction = a + (b - a) * f;
      if (!positionBuffered(fraction)) continue;
      candidates.push(fraction * duration);
      if (candidates.length >= 4) break;
    }
    if (candidates.length >= 4) break;
  }
  if (!candidates.length) {
    console.info('[poster] nothing buffered enough to sample', { id, bands });
    return;
  }
  console.info('[poster] capturing', { id, candidates });
  await invoke('poster_capture', { id, path: localPath, positions: candidates }).catch((e) =>
    console.warn('poster_capture failed:', e),
  );
}
