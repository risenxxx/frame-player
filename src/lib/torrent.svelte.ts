/**
 * Torrent streaming, player side (ROADMAP 21).
 *
 * The Rust half (torrent/) turns a magnet into a list of files, each with a
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

// The module itself lives under `torrent/`, one file per seam; this file is the
// name the rest of the player imports it by, and re-exports what it always did.
//
// The order of these lines is the order the parts evaluate in. Every part only
// constructs its state, so the order inside `torrent/` is free — what is not is
// the rest of `$lib`: the first part to reach it (`remembered`) imports
// `format` and then `history.svelte`, which is the order this file imported
// them in before it was split, so whoever loads it first still evaluates the
// rest of `$lib` in the same order.

export {
  type PortStatus,
  type TorrentFile,
  type TorrentInfo,
  type TorrentStatus,
  positionBuffered,
  torrent,
} from './torrent/state.svelte';
export {
  type CatalogOrigin,
  type RememberedTorrent,
  expectFeed,
  forgetRememberedTorrent,
  markWatched,
  rememberTorrent,
  rememberedTorrent,
  rememberedTorrents,
  resolveTorrentFile,
  setTorrentFeed,
  torrentFileName,
  watchedFiles,
} from './torrent/remembered';
export {
  type Encryption,
  type NetIface,
  type NetView,
  type Route,
  answerVpnAsk,
  loadTorrentPrefs,
  net,
  refreshNet,
  refreshPortStatus,
  sameRoute,
  setEncryption,
  setPortForward,
  setProxy,
  setRoute,
  setSeeding,
  setVpnAsk,
  torrentPrefs,
} from './torrent/prefs.svelte';
export { attachTorrentSubtitles, torrentSubtitles, torrentVideos } from './torrent/files';
export { addTorrent, forgetResolve, releaseTorrent, torrentFailureText } from './torrent/session';
export { trackTorrentPlayback } from './torrent/poll.svelte';
export {
  type TorrentOnDisk,
  type TorrentRow,
  DELETE_STUCK,
  forgetTorrent,
  listTorrents,
  torrentIsPlaying,
  torrentPositions,
  torrentResume,
  withDeadline,
} from './torrent/disk';
export { findSupersededTorrent, looksLikeSameRelease, updateTorrent } from './torrent/replace';
export { readFeed, resolveFeedItem } from './torrent/feeds';
export { captureTorrentPoster } from './torrent/poster';
