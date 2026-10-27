import { invoke } from '@tauri-apps/api/core';

import { positionBuffered, torrent } from './state.svelte';

// ---- Poster capture ---------------------------------------------------------

/// Ids captured this run, so the once-per-file rule survives a pause, a seek
/// and the one-second status poll.
const posterTried = new Set<string>();

/// Wait this long into a file before spending anything on a poster: the viewer
/// has committed to it, several seconds are buffered, and an episode abandoned
/// after ten seconds costs nothing.
export const POSTER_AFTER_S = 45;

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
