/**
 * Following whatever the player is playing: the status and buffer-map polls,
 * the finished-episode mark, and the one-ahead prefetch they trigger.
 */

import { invoke } from '@tauri-apps/api/core';

import { FINISHED_FRACTION } from '../history.svelte';
import { latest } from '../latest';
import { player } from '../player.svelte';
import { parseTorrentUrl, torrentId } from '../source';
import { torrentVideos } from './files';
import { POSTER_AFTER_S, captureTorrentPoster } from './poster';
import { markWatched, torrentFileName } from './remembered';
import { type TorrentStatus, torrent } from './state.svelte';

/// How often the status is re-read while a torrent is playing.
///
/// One second, like the media info panel's bitrate refresh and for the same
/// reason: these are figures a viewer watches move while deciding whether to
/// wait, and a slower tick reads as a frozen readout.
const POLL_MS = 1000;

/// Stretches of the playing file that are on disk, as fractions [0..1].
///
/// Read a little slower than the status: it is a map, not a counter, and the
/// eye does not track a band creeping by half a percent. Cheap either way —
/// measured, a seven-gigabyte season is ~5.6 KB of bitmap.
const BUFFER_POLL_MS = 2000;

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

/// Which run of the effect below owns the readout.
///
/// **A poll already in flight outlives the file it was asking about**, and that
/// is not a rare race: a status read is a round trip into Rust and the timer
/// fires every second, so closing a film catches one in the air often enough to
/// be reported as ordinary behavior. Clearing the interval does nothing to a
/// promise that has already been sent — it resolves afterwards, writes
/// `torrent.status` back, and since the file is gone nothing will ever clear it
/// again: the readout stays on screen over the start screen, permanently. The
/// same guard `loadTracks()` carries, for the same reason.
const polls = latest();

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
    const run = polls.begin();
    clearInterval(timer);
    timer = undefined;

    if (!ref) {
      torrent.status = null;
      torrent.buffered = [];
      torrent.peerlessSince = 0;
      return;
    }

    const read = async () => {
      try {
        const status = await invoke<TorrentStatus>('torrent_status', {
          infoHash: ref.infoHash,
          index: ref.index,
        });
        if (run.stale) return;
        torrent.status = status;
        // Only a *live* torrent with nobody in its swarm is evidence of
        // anything: `initializing` is librqbit hashing what is already on disk,
        // where a peer count of zero is what it should be.
        if (status.state !== 'live' || status.peers > 0) torrent.peerlessSince = 0;
        else if (!torrent.peerlessSince) torrent.peerlessSince = Date.now();
        // **Recorded here rather than where positions are written**, and that
        // is the dependency direction rather than convenience: the position
        // store must not know what a torrent is, while this poll already knows
        // the info hash, the index and how far playback has got. It ticks once a
        // second for exactly as long as a torrent is playing, which is precisely
        // when an episode can become finished.
        if (player.duration > 0 && player.timePos / player.duration > FINISHED_FRACTION) {
          markWatched(ref.infoHash, torrentFileName(ref.infoHash, ref.index));
        }
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
        if (!run.stale) torrent.status = null;
      }
    };
    const readBuffered = async () => {
      try {
        const bands = await invoke<[number, number][]>('torrent_buffered', {
          infoHash: ref.infoHash,
          index: ref.index,
        });
        if (!run.stale) torrent.buffered = bands;
      } catch {
        if (!run.stale) torrent.buffered = [];
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
