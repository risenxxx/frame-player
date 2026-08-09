/**
 * What the room is watching, and whether this machine can watch the same thing.
 *
 * A relay carries no media, so the only thing a room can share about a film is
 * enough to *find* it — and how well that works depends entirely on where the
 * film came from. The four cases are four honest answers rather than four
 * formats:
 *
 * - **A torrent is the good case, and it is why the roadmap paired this feature
 *   with torrents in the first place.** An info hash plus a file index names a
 *   file exactly, and everyone can fetch it themselves. A guest joining mid-film
 *   adds the magnet and opens the right episode with no lookup at all, because
 *   the metadata is already cached beside the data (torrent.rs). Switching
 *   episodes is the same info hash with another index, which costs milliseconds.
 * - **A URL** works if the site lets each machine resolve it, which yt-dlp does
 *   on each end independently.
 * - **A local file cannot be sent**, so what travels is enough to *recognise* a
 *   copy: the release hash (the OpenSubtitles scheme, already implemented in
 *   `opensubtitles.rs` and reused rather than rewritten), the size and the
 *   duration. The viewer opens their own copy and the player says whether it is
 *   the same release, a different rip, or a different film — which matters,
 *   because on a different rip a shared timeline is not merely approximate, it
 *   is meaningless.
 * - **A file under a privacy root** publishes `hidden`. The timeline still
 *   works; the name does not leave the machine.
 *
 * That last one is why this module is the only place a `ContentRef` is ever
 * built: a privacy check with two entry points is a privacy check with one of
 * them missing.
 */

import { invoke } from '@tauri-apps/api/core';

import { displayName } from '../format';
import { isPrivatePath } from '../history.svelte';
import { isNetworkSource } from '../player.svelte';
import { magnetFor, parseTorrentUrl, sourceId } from '../source';
import { rememberedTorrent, type TorrentInfo } from '../torrent.svelte';
import type { ContentRef } from './protocol';

/**
 * How far two durations may differ and still be the same film.
 *
 * Generous on purpose: containers disagree about the last frame, a re-mux can
 * round, and a duration read before the file is fully open can be short. Two
 * seconds is far below the difference an actual different cut produces (an
 * extended edition, a version with the credits trimmed, a broadcast cut) and far
 * above the noise.
 */
export const DURATION_TOLERANCE = 2;

/** What a local file has to offer when checking it against the room's. */
export interface LocalFile {
  src: string;
  duration: number;
  size?: number;
  hash?: string;
}

/**
 * Whether two references name the same thing to watch.
 *
 * Identity, not equality: a title that arrived late, a duration that firmed up
 * after the file opened, or a magnet rebuilt from the info hash must not read as
 * a different film — otherwise every one of those would restart playback for
 * everybody in the room.
 */
export function sameContent(a: ContentRef | null, b: ContentRef | null): boolean {
  if (!a || !b) return a === b;
  if (a.kind !== b.kind) return false;
  switch (a.kind) {
    case 'torrent':
      return (
        a.infoHash.toLowerCase() === (b as typeof a).infoHash.toLowerCase() &&
        a.index === (b as typeof a).index
      );
    case 'url':
      // Through `sourceId`, so the same video shared as `youtu.be/ID?si=…` and
      // as `youtube.com/watch?v=ID` is one film rather than two.
      return sourceId(a.url) === sourceId((b as typeof a).url);
    case 'file': {
      const other = b as typeof a;
      // The hash is the identity when both ends have one. Without it there is
      // nothing to compare but the name, and two people's copies of a film
      // rarely share a file name — so "unknown" is the honest answer and
      // `compareLocal` is where it is turned into a sentence.
      if (a.hash && other.hash) return a.hash === other.hash;
      return false;
    }
    case 'hidden':
      // Two hidden files are indistinguishable by construction, which is the
      // point. Treating them as the same one keeps the timeline from restarting
      // every time the publisher re-states it.
      return true;
  }
}

/** How well the file this viewer has open matches what the room is watching. */
export type MatchVerdict =
  /// The same release, by hash. A shared timeline means exactly what it says.
  | 'exact'
  /// The same film, near enough — the durations agree but the hashes do not.
  | 'rip'
  /// Durations disagree: a different cut, or a different film. The timeline
  /// still runs, but it points somewhere else in this copy.
  | 'mismatch'
  /// Nothing open, or nothing to compare with.
  | 'unknown';

/**
 * Judge the open file against the room's.
 *
 * Only ever advisory — nothing is refused on this. A viewer who deliberately
 * opens the director's cut is not making a mistake the player should correct,
 * they are making one the player should mention.
 */
export function compareLocal(ref: ContentRef | null, local: LocalFile | null): MatchVerdict {
  if (!ref || !local || ref.kind === 'hidden') return 'unknown';
  if (ref.kind === 'torrent' || ref.kind === 'url') {
    // These identify the file itself, so being on it is the whole check.
    const here = contentIdOf(ref);
    return here && here === sourceId(local.src) ? 'exact' : 'mismatch';
  }
  if (local.hash && ref.hash) return local.hash === ref.hash ? 'exact' : durationVerdict(ref, local);
  return durationVerdict(ref, local);
}

function durationVerdict(ref: ContentRef & { duration: number }, local: LocalFile): MatchVerdict {
  if (!ref.duration || !local.duration) return 'unknown';
  return Math.abs(ref.duration - local.duration) <= DURATION_TOLERANCE ? 'rip' : 'mismatch';
}

/**
 * The stable id of what a reference names, in the same vocabulary the watch
 * history uses (`sourceId`) — so "is the room on the file I have open" is one
 * string comparison rather than a second notion of identity.
 *
 * Empty for a local file and for a hidden one: neither can be addressed from
 * another machine, which is exactly what makes them the awkward cases.
 */
export function contentIdOf(ref: ContentRef): string {
  switch (ref.kind) {
    case 'torrent':
      return `torrent:${ref.infoHash.toLowerCase()}/${ref.index}`;
    case 'url':
      return sourceId(ref.url);
    default:
      return '';
  }
}

/** What to show for a reference. Empty for `hidden`, which the UI names itself. */
export function contentTitle(ref: ContentRef | null): string {
  if (!ref || ref.kind === 'hidden') return '';
  return ref.title;
}

// ---- building one -----------------------------------------------------------

/**
 * Describe what this player has open, for the room.
 *
 * **The only place a `ContentRef` is made**, which is what makes the privacy
 * rule enforceable: a second builder would be a second place to forget it. This
 * is the seventh point in the player where something about a file can leave the
 * machine (after the position store, the update snapshot, the thumbnail cache,
 * the subtitle search and the two casting paths), and the most direct of them —
 * the others speak to a service or a device on the same network, and this one
 * speaks to other people.
 *
 * Returns null when there is nothing to say, which is different from `hidden`:
 * null means no file, `hidden` means a file the room is not told about.
 */
export async function contentOf(
  src: string | null,
  info: { title: string | null; duration: number; torrent: TorrentInfo | null },
): Promise<ContentRef | null> {
  if (!src) return null;

  const torrent = parseTorrentUrl(src);
  if (torrent) {
    // A torrent's identity is the info hash, and the magnet is only how to get
    // back to it — the remembered one where there is one (it carries the
    // trackers, which is what finds peers quickly), and one built from the hash
    // where there is not. The second still works: the info hash is the torrent.
    const known = rememberedTorrent(torrent.infoHash);
    const file = info.torrent?.files.find((f) => f.index === torrent.index);
    const name = info.torrent?.name ?? known?.name ?? null;
    return {
      kind: 'torrent',
      magnet: known?.magnet ?? magnetFor(torrent.infoHash, name),
      infoHash: torrent.infoHash.toLowerCase(),
      index: torrent.index,
      file: file?.path ?? '',
      title: info.title || displayName(file?.path ?? src),
      duration: info.duration,
    };
  }

  if (isNetworkSource(src)) {
    return { kind: 'url', url: src, title: info.title || displayName(src), duration: info.duration };
  }

  // A local file. The privacy check comes before anything is read off the disk,
  // so an excluded folder is not even hashed.
  if (isPrivatePath(src)) return { kind: 'hidden' };

  const id = await releaseId(src);
  return {
    kind: 'file',
    title: info.title || displayName(src),
    duration: info.duration,
    size: id?.size ?? 0,
    hash: id?.hash ?? '',
  };
}

/**
 * The release hash of a local file, or null.
 *
 * Failure is ordinary rather than exceptional — a file under 128 KiB has no
 * hash, and a file on a disconnected drive has nothing to read — and it costs
 * only the precision of the answer the other viewers get, so it is swallowed.
 */
async function releaseId(path: string): Promise<{ hash: string; size: number } | null> {
  try {
    return await invoke<{ hash: string; size: number }>('release_hash', { path });
  } catch {
    return null;
  }
}
