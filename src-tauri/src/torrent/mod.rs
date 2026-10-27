//! Torrent streaming (ROADMAP 21): a magnet link becomes a URL mpv can open.
//!
//! The architecture is the settled one — peerflix, Stremio and webtorrent all
//! work this way. A torrent client inside the process runs a local HTTP server;
//! mpv opens `http://127.0.0.1:<port>/…` and its Range requests *are* the signal
//! for "the viewer is here", which the client turns into piece priority.
//! [librqbit](https://lib.rs/crates/librqbit) already does that half: it blocks
//! a read until the piece arrives and prioritizes what is being streamed.
//!
//! Four decisions in here are worth stating, because each one is a place the
//! obvious implementation is wrong.
//!
//! **The session is lazy.** Creating one opens sockets, joins the DHT and starts
//! talking to strangers. The overwhelming majority of launches never open a
//! magnet, and a video player that joins a peer-to-peer network on startup is
//! not one people would keep. So it is built on the first `torrent_add` and not
//! before.
//!
//! **The identity lives in the URL path, not in the port.** A stream is served
//! from `/t/<infohash>/<index>/<name.mkv>`, so `sourceId()` on the frontend can
//! read `torrent:<infohash>/<index>` straight out of the URL with no extra state
//! to keep in sync. The port is deliberately not part of it: it is ephemeral and
//! changes every run, which is exactly why a key taken from the transport
//! forgets the file every session (ROADMAP 20). The trailing file name is not
//! decoration either — it gives ffmpeg an extension to probe by and the player a
//! name to show.
//!
//! **What gets downloaded is decided by what is being read, not up front.** A
//! torrent holding a season is twelve episodes, and selecting them all to watch
//! one would pull ~30 GB for a 45-minute view. The torrent is therefore added
//! *paused* with nothing selected, and the HTTP handler itself puts a file into
//! `only_files` and unpauses when a request for it arrives. That is the same
//! principle as piece priority one level up — the reader steers the download —
//! and it is what lets the frontend hand mpv a playlist of every video in the
//! torrent without any of them costing anything until it is played.
//!
//! **The server is loopback-only, on an ephemeral port.** This is a private
//! channel between us and our own mpv, not a service. Binding `127.0.0.1:0`
//! means no fixed port to collide with a second instance and nothing reachable
//! from the network.
//!
//! Not done here, and deliberately: buffered ranges on the seekbar. librqbit
//! keeps the piece bitmap behind `pub(crate) with_chunk_tracker`, so the finest
//! public measure is `TorrentStats::file_progress` — bytes per file, which is a
//! percentage and not a map. Drawing *where* the holes are needs either an
//! upstream accessor or our own tracking of what the stream has served.

mod add;
mod announce;
mod commands;
mod dirs;
mod http;
mod session;
mod status;
mod storage;
#[cfg(test)]
mod tests;

pub use commands::*;
pub(crate) use dirs::cache_metadata;
pub(crate) use http::{parse_range, urlencode};
pub(crate) use storage::dir_size;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use librqbit::{ManagedTorrent, Session};
use serde::Serialize;
use tokio::sync::Mutex as AsyncMutex;

use session::SessionPrefs;

/// One file inside a torrent, as the frontend needs it.
///
/// `path` is the path *within* the torrent (so a season in a folder still reads
/// as one), `index` is its position in the torrent's own file list — the half of
/// the source identity that a hash cannot supply.
#[derive(Serialize, Clone)]
pub struct TorrentFile {
    pub index: usize,
    /// Path inside the torrent, `/`-separated on every platform: this is a
    /// torrent's own notion of a name, not something on this filesystem.
    pub path: String,
    pub size: u64,
    /// The URL mpv opens for this file. Filled in here rather than assembled on
    /// the frontend, so the shape of the route stays a fact of one file.
    pub url: String,
}

#[derive(Serialize, Clone)]
pub struct TorrentInfo {
    pub info_hash: String,
    /// The torrent's own name — the folder for a multi-file torrent, the file
    /// name for a single one. `None` for a magnet that carried no `dn`.
    pub name: Option<String>,
    pub files: Vec<TorrentFile>,
    /// The trackers this torrent names, so a magnet rebuilt from its info hash
    /// can carry them. See `trackers_of`: the hash alone is a DHT-only magnet,
    /// and on a network where the DHT is filtered that is the difference
    /// between resolving in a second and not resolving at all.
    pub trackers: Vec<String>,
}

/// A torrent's data as it exists **on disk**, which is deliberately a separate
/// question from what the session knows about.
///
/// Disk is the source of truth for storage: the folder and its size can always
/// be read, with no record on our side and regardless of whether history is
/// switched off. What the frontend's own store adds on top is the ability to
/// *reopen* one — so a viewer with history disabled still sees what is taking
/// space and can delete it, which is the half that must never depend on us
/// having remembered anything.
/// A torrent file as it exists on this disk, for the decoder to open directly.
#[derive(Serialize)]
pub struct LocalFile {
    pub path: String,
    /// Every byte is present. False means only `buffered()` positions may be
    /// decoded — see `local_path`.
    pub complete: bool,
}

#[derive(Serialize)]
pub struct TorrentOnDisk {
    /// Directory name inside the torrent cache. For anything this build wrote
    /// that is the 40-hex info hash (see `add`); anything else predates that
    /// layout and can only be measured and deleted, not resumed.
    pub folder: String,
    /// Where that folder actually is, so the frontend can hand it to
    /// `revealItemInDir`. The name alone is not enough — the root is Rust's to
    /// know, and about to stop being a constant (the viewer will be able to
    /// choose it).
    pub path: String,
    /// Set when `folder` is a well-formed info hash.
    pub info_hash: Option<String>,
    pub size: u64,
    /// The torrent's own name, read from the metadata cached beside the data.
    ///
    /// The frontend's store answers this too, and better — it also knows how
    /// many videos are inside — but it is *localStorage*, which is per webview
    /// and can legitimately not be the one that opened this torrent: two builds
    /// of the player share `app_cache_dir` and therefore this directory, while
    /// keeping separate stores. Then a season downloaded by one shows up in the
    /// other as a nameless row it cannot open, which reads as corruption rather
    /// than as two windows onto one cache.
    ///
    /// So the disk names itself. It is the same metadata that already makes
    /// reopening cost no DHT lookup, which is what makes the row openable as
    /// well as readable — everything needed is the info hash plus these bytes.
    pub name: Option<String>,
}

/// What the player shows while a torrent is feeding it.
///
/// Deliberately the numbers a stalled viewer needs in order to know *why*: peers
/// says whether there is a swarm at all, speed says whether it is moving, and
/// the per-file progress says how much of this episode is on disk. A stall with
/// zero peers and a stall at 2 MB/s are different problems and must not look the
/// same.
#[derive(Serialize, Default)]
pub struct TorrentStatus {
    /// `initializing` | `live` | `paused` | `error` | `gone`
    pub state: String,
    pub error: Option<String>,
    /// Connected peers, and how many have been seen at all. Zero live with a
    /// non-zero seen count is "the swarm is there but nobody is talking".
    pub peers: usize,
    pub peers_seen: usize,
    /// Bytes per second, down and up.
    pub down_bps: f64,
    pub up_bps: f64,
    /// Bytes of the streamed file already on disk, and its total size.
    pub file_done: u64,
    pub file_size: u64,
    /// Whole-torrent figures, for the media info panel.
    pub total_done: u64,
    pub total_size: u64,
}

struct Entry {
    handle: Arc<ManagedTorrent>,
    /// File indices handed to `only_files` so far. Kept beside the handle
    /// because librqbit takes the whole set on every update, so adding one
    /// means knowing the others.
    selected: HashSet<usize>,
}

#[derive(Default)]
pub struct TorrentService {
    /// Everything under one lock, and an *async* one: selecting a file awaits
    /// librqbit, and doing that under a `std::sync::Mutex` across an await point
    /// is the deadlock this avoids.
    inner: AsyncMutex<Inner>,
}

#[derive(Default)]
struct Inner {
    session: Option<Arc<Session>>,
    /// The local server's port, once it is listening. Zero means not started.
    port: u16,
    /// By info hash, lower-case hex.
    torrents: HashMap<String, Entry>,
    /// What the **live session** was built with. Not the preferences — what is
    /// actually running, which is the only thing that can be trusted to answer
    /// "are we uploading right now".
    prefs: SessionPrefs,
    /// The interface the live session is bound to — what `prefs.route`
    /// resolved to when it was built. Kept beside the preference because the
    /// same preference can mean a different interface later ("past the VPN" is
    /// the Wi-Fi today and the Ethernet tomorrow), and a session left bound to
    /// one that went away sends nothing at all.
    device: Option<String>,
}
