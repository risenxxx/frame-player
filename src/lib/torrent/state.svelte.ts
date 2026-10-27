/**
 * What the player knows about the torrent feeding it: the shapes Rust hands
 * over, the one reactive object every surface reads (`torrent`), and the
 * question the buffer map answers for the previews (`positionBuffered`).
 *
 * A leaf on purpose — everything else under `torrent/` writes into this object,
 * so it imports none of them.
 */

import type { FeedItem } from '../feed';

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
  /// The trackers the torrent itself names, capped in Rust (`trackers_of`).
  /// Only ever used to build a magnet for somebody *else* to open — see
  /// `magnetFor`.
  trackers: string[];
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

export interface PortStatus {
  /// `off` | `no_session` | `mapped` | `unmapped` | `no_router` | `no_port`
  state: string;
  port: number;
  detail: string | null;
}

class TorrentState {
  /// Merged, sorted, non-overlapping.
  buffered = $state<[number, number][]>([]);
  /// The torrent feeding the player, if any. Kept after the file is chosen
  /// because the file picker, the queue and the info panel all read it.
  info = $state<TorrentInfo | null>(null);
  /// Live figures for the file being played. Null when nothing is polling.
  status = $state<TorrentStatus | null>(null);
  /// Since when the swarm has had nobody in it, or 0.
  ///
  /// A count of zero is an ordinary reading for the first few seconds of any
  /// torrent and a diagnosis after half a minute, and the player had no way to
  /// tell those apart: "Подключаемся к раздаче…" stood unchanged for as long as
  /// it took the viewer to give up. Kept here rather than derived because it is
  /// a fact about *time*, and the status object is what carries the moment.
  peerlessSince = $state(0);
  /// A magnet is being resolved: the DHT lookup that turns an info hash into a
  /// file list, which routinely takes ten seconds and can take ninety.
  resolving = $state(false);
  /// When that resolve started, so the wait can show a clock.
  ///
  /// It is the longest wait anywhere in the player — `RESOLVE_TIMEOUT` is 90 s,
  /// and a cached-metadata add that times out falls through to a second one —
  /// and it is the one wait with no percentage to report, because nothing can be
  /// counted until the metadata arrives whole. A number that moves is the whole
  /// difference between a player that is still looking and one that has hung.
  resolvingSince = $state(0);
  /// What the router last said about the forwarded port, and whether an answer
  /// is being waited for. Read only by the settings row — see `refreshPortStatus`.
  portStatus = $state<PortStatus | null>(null);
  portChecking = $state(false);
  /// A newer upload found in the feed a torrent came from, by the info hash
  /// of the torrent it would replace. Session-only: it is a reading of
  /// somebody else's feed, and a stored one would outlive being true.
  feedUpdates = $state<Record<string, FeedItem>>({});
}

export const torrent = new TorrentState();

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
