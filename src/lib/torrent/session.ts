/**
 * Adding a torrent to the session and letting go of it: the resolve, what its
 * failure says to the viewer, and the release that stops the peer traffic once
 * nothing is watching.
 */

import { invoke } from '@tauri-apps/api/core';

import { t } from '../i18n.svelte';
import { latest } from '../latest';
import { askAboutVpn, effectiveRoute, torrentPrefs } from './prefs.svelte';
import { type TorrentInfo, torrent } from './state.svelte';

/**
 * Resolve a magnet (or a `.torrent` URL) into its file list.
 *
 * Throws with a short reason the caller turns into a sentence — `resolve_timeout`
 * for a swarm that never answered, anything else for librqbit's own message.
 */
export async function addTorrent(source: string): Promise<TorrentInfo> {
  // Before the resolve clock starts: the question is the viewer's time, not
  // the swarm's.
  await askAboutVpn();
  const run = resolves.begin();
  torrent.resolving = true;
  torrent.resolvingSince = Date.now();
  try {
    // One object rather than a field per preference: Rust compares it whole to
    // decide whether the live session can be reused, and a value left out here
    // would be a session quietly running with the previous one.
    const info = await invoke<TorrentInfo>('torrent_add', {
      source,
      prefs: {
        seeding: torrentPrefs.seeding,
        portForward: torrentPrefs.portForward,
        proxy: torrentPrefs.proxy,
        encryption: torrentPrefs.encryption,
        route: effectiveRoute(),
      },
    });
    torrent.info = info;
    return info;
  } finally {
    // Only the newest resolve may say that resolving is over: one that was
    // let go of (`forgetResolve`) or overtaken would otherwise take the flag
    // down under the one still running.
    if (!run.stale) torrent.resolving = false;
  }
}

const resolves = latest();

/**
 * Stop waiting for a resolve the viewer walked away from.
 *
 * The lookup itself is not cancelled — it runs inside `torrent_add` for up to
 * `RESOLVE_TIMEOUT` — but nothing on screen may go on saying it is under way:
 * the link box keeps its field disabled and the loading plate prints "looking
 * for the torrent" for as long as this flag stands.
 */
export function forgetResolve() {
  resolves.begin();
  torrent.resolving = false;
}

/**
 * Turn what `addTorrent` threw into a sentence.
 *
 * One place rather than one per caller, because there are two now — the link
 * box and the room panel — and the distinction worth keeping is the same on
 * both: **a swarm that never answered is a fact about the torrent**, and "try a
 * different magnet" is different advice from librqbit's own message. The room
 * used to log its reason to the console and show a bare "could not open it",
 * which is the one thing a viewer whose window is empty cannot act on.
 */
export function torrentFailureText(e: unknown): string {
  const reason = String(e);
  if (reason === 'resolve_timeout') return t('torrent.timeout');
  // The route could not be honoured. Said as itself rather than as a failure
  // to open: falling back to the system route would send the traffic exactly
  // where the viewer asked it not to go, so the torrent is refused instead.
  if (reason === 'route_no_direct') return t('torrent.route_no_direct');
  if (reason.startsWith('route_missing:')) {
    return t('torrent.route_missing', { name: reason.slice('route_missing:'.length) });
  }
  return t('torrent.failed', { reason });
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
