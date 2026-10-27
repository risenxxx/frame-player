import { invoke } from '@tauri-apps/api/core';

import { displayName } from '../format';
import { positionsLoad } from '../history.svelte';
import { player } from '../player.svelte';
import { magnetFor, parseTorrentUrl } from '../source';
import { type RememberedTorrent, forgetRememberedTorrent, rememberedTorrent } from './remembered';
import { torrent } from './state.svelte';

// ---- What is on disk -------------------------------------------------------

export interface TorrentOnDisk {
  folder: string;
  /// Absolute path to that folder. The root belongs to Rust — the frontend
  /// cannot compose this itself, and is about to have even less business
  /// trying, since the viewer will be able to choose where the files go.
  path: string;
  info_hash: string | null;
  size: number;
  /// The torrent's own name, read from the metadata cached beside its data.
  /// Present for anything opened by a build that cached it, whichever build
  /// that was — see `cached_name` in torrent/dirs.rs.
  name: string | null;
}

/// One row of the start screen's torrent list: what disk says, plus what we
/// remembered about it, plus where the viewer left off.
export interface TorrentRow extends TorrentOnDisk {
  known: RememberedTorrent | null;
  /// Best display name: what we remembered, else what the disk says about
  /// itself, else the folder — which for anything this build wrote is an info
  /// hash, hence the fallback label in the UI.
  name: string | null;
  /// The magnet to reopen this row with, or null when there is nothing to go
  /// on and it can only be measured and deleted.
  ///
  /// **A remembered magnet is the better answer but not the only one.** The
  /// info hash *is* the torrent's identity, and the metadata cached beside the
  /// data is what makes reopening from a bare one cost no lookup — so a row
  /// whose record is missing is still openable, and opening it records it
  /// again. That matters because the record lives in localStorage, which is
  /// per webview: two builds of the player share this cache directory and not
  /// their stores, so each of them used to show the other's torrents as
  /// nameless rows with a dead button.
  magnet: string | null;
}

export async function listTorrents(): Promise<TorrentRow[]> {
  const disk = await invoke<TorrentOnDisk[]>('torrent_list').catch(() => [] as TorrentOnDisk[]);
  return disk.map((d) => {
    const known = d.info_hash ? rememberedTorrent(d.info_hash) : null;
    const name = known?.name ?? d.name ?? (d.info_hash ? null : d.folder);
    return {
      ...d,
      known,
      name,
      magnet: known?.magnet ?? (d.info_hash ? magnetFor(d.info_hash, name) : null),
    };
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

/// How long a deletion may take before the player stops waiting on it.
///
/// **A backstop against "never", not a bound on the work.** Removing a season is
/// seconds, and the Rust side now puts its own deadline on the one step that
/// could sit for ever — but a command that never answers at all (a panic inside
/// it resolves nothing, which is a real failure mode of the crate underneath)
/// would leave `rowBusy` set for the rest of the run, and every button on that
/// row dead with nothing on screen to say why. That is the one failure a viewer
/// cannot get out of without restarting the player, which is why it is worth a
/// timer even though it should never fire.
const DELETE_DEADLINE_MS = 60_000;

/// What a deadline rejects with, so the caller can say *that* rather than print
/// the word "timeout" at somebody.
export const DELETE_STUCK = 'delete_stuck';

/// Stop waiting after `ms`. The work is **not** cancelled — nothing here can
/// recall a command already in flight — this only stops the UI from waiting on
/// an answer that may never come.
export function withDeadline<T>(work: Promise<T>, ms = DELETE_DEADLINE_MS): Promise<T> {
  let timer: ReturnType<typeof setTimeout>;
  return Promise.race([
    work.finally(() => clearTimeout(timer)),
    new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject(DELETE_STUCK), ms);
    }),
  ]);
}

/**
 * Delete a torrent completely: its data, and everything remembered about it.
 *
 * One coherent "forget this", because a half-deleted torrent is the confusing
 * state — files gone but the season still listed, or the reverse. The caller
 * supplies the history removal (positions and tracks live in history.svelte.ts
 * and are keyed by `torrent:<hash>/<index>`, which only that module knows how to
 * walk), so this returns the bytes freed and leaves the rest to it.
 *
 * **It throws rather than answering zero.** This used to end in
 * `.catch(() => 0)`, which turned every possible failure — a refused path, a
 * directory that would not go, a command that never returned — into the same
 * "Freed 0 B" the successful case shows, with nothing written anywhere. So a
 * torrent that could not be deleted reported the deletion and stayed in the
 * list, which is indistinguishable from a torrent that was deleted and came
 * back, and neither the viewer nor a log could tell which had happened.
 */
export async function forgetTorrent(row: TorrentRow): Promise<number> {
  // **By path, not by name.** With the folder now possibly living in a
  // directory the viewer chose, a bare name is ambiguous between roots — and
  // the Rust side validates the path against the roots it knows rather than
  // trusting that this row came from `list`.
  const freed = await withDeadline(invoke<number>('torrent_forget', { path: row.path }));
  if (row.info_hash) {
    forgetRememberedTorrent(row.info_hash);
    if (torrent.info?.info_hash === row.info_hash) {
      torrent.info = null;
      torrent.status = null;
    }
  }
  return freed;
}
