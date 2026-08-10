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

use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt as _;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, StreamBody};
use hyper::body::Frame;
use hyper::service::service_fn;
use hyper::{header, Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, Session, SessionOptions,
};
use serde::Serialize;
use tauri::Manager;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::io::ReaderStream;

/// How long to wait for a magnet's metadata before giving up.
///
/// A magnet is only an info hash: the file list has to be fetched from peers
/// found through the DHT and the trackers, and a torrent with no live seeders
/// never produces one. Without a bound the "opening" indicator would sit there
/// for ever, which reads as the player having hung rather than the swarm being
/// dead.
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(90);

/// How long to wait for a torrent to become streamable after it is started.
///
/// This covers librqbit's `Initializing` state — checking and allocating the
/// files on disk — which scales with the size of the *torrent*, not of the file
/// being watched: a nine-episode season is several seconds of it even when only
/// one episode is selected.
const INIT_TIMEOUT: Duration = Duration::from_secs(60);

/// How long one seeding announce may take before it is written off, and how
/// many of a magnet's trackers are asked. Both bounds exist so a dead tracker
/// cannot add to a wait that is already up to `RESOLVE_TIMEOUT` — this is a head
/// start, never a prerequisite. They run concurrently, so the cost is one
/// timeout, not four.
const SEED_ANNOUNCE_TIMEOUT: Duration = Duration::from_secs(6);
const SEED_ANNOUNCE_TRACKERS: usize = 4;

/// Read size for one body chunk. Matches librqbit's own streaming handler; a
/// piece is typically 1–16 MB, so this is well inside one and the reader blocks
/// on piece arrival rather than on the buffer.
const STREAM_CHUNK: usize = 65536;

/// librqbit's own session store, inside our cache so `torrent_clear_cache`
/// takes it with everything else. A dot name so `torrent_list` skips it — it is
/// ours, not a torrent occupying space.
const SESSION_DIR: &str = ".session";

/// How long `pause_restored` waits for one restored torrent to leave
/// `Initializing`. Generous in tries and tiny in step: with `fastresume` there
/// is nothing to hash, so this normally settles on the first or second look,
/// and the bound only matters for a store that has gone stale.
const RESTORE_SETTLE_TRIES: usize = 100;
const RESTORE_SETTLE_STEP: Duration = Duration::from_millis(20);

type Body = BoxBody<Bytes, std::io::Error>;

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
    /// Whether the **live session** was built to seed. Not the preference —
    /// what is actually running, which is the only thing that can be trusted to
    /// answer "are we uploading right now".
    seeding: bool,
    /// Likewise for the port mapping: baked into the session when it is built.
    port_forward: bool,
}

/// Which directories this feature may read, write and delete in.
///
/// **Once the viewer can choose where the files go, "the torrent directory" is
/// three different questions** and answering them with one path is how a media
/// player deletes somebody's films. They are kept apart here so that every rule
/// below can be stated against the right one.
///
/// `state` is ours unconditionally: the metadata cache, librqbit's session store
/// and the record of the roots themselves. It stays in the app cache directory
/// whatever the viewer picks, which keeps the chosen folder pure video, leaves
/// `prune_orphaned_store` and the clear-cache button pointed where they always
/// were, and means a removable drive going missing costs data and not the
/// player's own bookkeeping.
///
/// `root` is where **new** torrents go. `roots` is everywhere data may already
/// be — the current root, every root used before it, and `state` — because
/// changing the setting must not orphan a season downloaded last week. Lookups
/// scan all of them; nothing is ever moved by a change of setting.
///
/// **The dangerous half is what may be deleted**, and it is `is_ours` that
/// decides. In `state` the player owns the whole directory, so a folder from the
/// older name-based layout is still measurable and deletable there. In a root
/// the viewer chose, the only folders that exist are `<Name> [<infohash>]` ones
/// we created, and anything else is theirs: it is not listed, not measured, not
/// counted and above all not deleted. Rather than trusting the UI to send only
/// what it was shown, the check lives here, in front of every destructive path.
#[derive(Clone, Debug)]
pub struct Dirs {
    pub state: PathBuf,
    pub root: PathBuf,
    pub roots: Vec<PathBuf>,
}

/// The chosen root, recorded beside the state rather than in localStorage.
///
/// A dot-name so `list` skips it, and read from disk on every use rather than
/// pushed in at startup: `download_dir` is reached by commands that can arrive
/// before the frontend has had a chance to say anything, and a preference that
/// is only sometimes in effect would put files in two places.
const ROOTS_FILE: &str = ".roots.json";

#[derive(Serialize, serde::Deserialize, Default)]
struct RootsRecord {
    /// Absent means the default: `state` itself.
    root: Option<String>,
    /// Roots used before, so their data stays findable.
    #[serde(default)]
    seen: Vec<String>,
}

impl Dirs {
    /// Everything in one directory, which is what a test wants and what the
    /// player did before the root became a choice.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn single(dir: PathBuf) -> Self {
        Self {
            state: dir.clone(),
            root: dir.clone(),
            roots: vec![dir],
        }
    }

    /// The cache directory rather than Downloads: watching a torrent is a side
    /// effect, not a download the viewer asked to keep, and a cache directory is
    /// the one place an OS and a user both understand as disposable.
    fn state_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
        let dir = app
            .path()
            .app_cache_dir()
            .map_err(|e| format!("no cache dir: {e}"))?
            .join("torrents");
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {dir:?}: {e}"))?;
        Ok(dir)
    }

    fn load(app: &tauri::AppHandle) -> Result<Self, String> {
        let state = Self::state_dir(app)?;
        let record: RootsRecord = std::fs::read(state.join(ROOTS_FILE))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();

        let root = record
            .root
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| state.clone());

        // `state` last and always: it is the one root that cannot go missing,
        // and the one where a legacy folder can still be found.
        let mut roots = vec![root.clone()];
        roots.extend(record.seen.iter().map(PathBuf::from));
        roots.push(state.clone());
        roots.dedup_by(|a, b| a == b);
        let mut seen = std::collections::HashSet::new();
        roots.retain(|r| seen.insert(r.clone()) && r.is_dir());

        Ok(Self { state, root, roots })
    }

    /// Record a new root, keeping the previous one in `seen` so its data stays
    /// findable. Creating it is part of choosing it: a root that cannot be
    /// written to is a setting that fails at the next torrent instead of now.
    fn set_root(app: &tauri::AppHandle, root: Option<&str>) -> Result<(), String> {
        let state = Self::state_dir(app)?;
        let path = state.join(ROOTS_FILE);
        let mut record: RootsRecord = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();

        if let Some(root) = root {
            let root = PathBuf::from(root);
            std::fs::create_dir_all(&root)
                .map_err(|e| format!("cannot use {}: {e}", root.display()))?;
            // Refuse the state directory as a "chosen" root: it is already a
            // root, and recording it as one would make `is_ours` treat the whole
            // of it as a viewer's folder.
            let root = (root != state).then(|| root.to_string_lossy().into_owned());
            if let Some(previous) = record.root.replace(root.clone().unwrap_or_default()) {
                if !previous.is_empty() && Some(&previous) != root.as_ref() {
                    record.seen.push(previous);
                }
            }
            if root.is_none() {
                record.root = None;
            }
        } else if let Some(previous) = record.root.take() {
            record.seen.push(previous);
        }

        record.seen.sort();
        record.seen.dedup();
        record.seen.retain(|s| Some(s) != record.root.as_ref());

        let bytes = serde_json::to_vec(&record).map_err(|e| format!("{e}"))?;
        std::fs::write(&path, bytes).map_err(|e| format!("{e}"))
    }

    /// Is this directory one the player created, and may therefore delete?
    ///
    /// **The whole safety of a viewer-chosen root rests here.** Inside `state`
    /// the answer is yes for anything: the player owns that directory, and a
    /// folder from the older name-based layout has to stay deletable. Anywhere
    /// else the name must parse back to an info hash, which is a folder this
    /// build wrote and nothing a person would have made.
    fn is_ours(&self, path: &std::path::Path) -> bool {
        let Some(parent) = path.parent() else {
            return false;
        };
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return false;
        };
        if !self.roots.iter().any(|r| same_dir(r, parent)) {
            return false;
        }
        same_dir(&self.state, parent) || folder_hash(name).is_some()
    }
}

/// Two paths naming the same directory.
///
/// Canonicalised, so a root recorded as `/Users/x/Films` and a parent arriving
/// as `/Users/x/./Films` — or through a symlinked home, which macOS hands out
/// routinely — are one directory rather than two. A path that cannot be
/// canonicalised does not exist, and nothing may be deleted under it.
fn same_dir(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

impl TorrentService {
    fn download_dir(app: &tauri::AppHandle) -> Result<Dirs, String> {
        Dirs::load(app)
    }

    /// The session and the HTTP server, created on first use.
    ///
    /// Takes the directory rather than the `AppHandle` so the whole path —
    /// session, server, streaming — can be exercised by a test against a real
    /// swarm without a Tauri app around it (see `sintel_smoke`).
    /// The loopback HTTP server, started once and kept for the life of the app.
    ///
    /// Deliberately independent of the session: the session is torn down and
    /// rebuilt whenever the seeding preference changes, and rebinding a listener
    /// each time would leak an accept loop and hand out URLs on a port the old
    /// one still owns.
    async fn ensure_server(self: &Arc<Self>) -> Result<u16, String> {
        {
            let inner = self.inner.lock().await;
            if inner.port != 0 {
                return Ok(inner.port);
            }
        }

        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|e| format!("cannot open local stream port: {e}"))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("cannot read local stream port: {e}"))?
            .port();

        let service = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    continue;
                };
                let service = service.clone();
                tauri::async_runtime::spawn(async move {
                    let io = TokioIo::new(stream);
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(
                            io,
                            service_fn(move |req| {
                                let service = service.clone();
                                async move { Ok::<_, std::convert::Infallible>(service.serve(req).await) }
                            }),
                        )
                        .await;
                });
            }
        });

        self.inner.lock().await.port = port;
        Ok(port)
    }

    async fn ensure_started(
        self: &Arc<Self>,
        dirs: &Dirs,
        seeding: bool,
        port_forward: bool,
    ) -> Result<(Arc<Session>, u16), String> {
        // **The session's own directory is `state`, never the chosen root.**
        // librqbit's `output_folder` here is only a default for torrents added
        // without one, and ours always carry an explicit folder — so what this
        // actually decides is where the store and its resume data live, which
        // belong with the player's bookkeeping and not in somebody's film
        // library. It also means changing the root never moves the session.
        let dir = dirs.state.clone();
        let port = self.ensure_server().await?;
        {
            let inner = self.inner.lock().await;
            // Only reuse a session that matches the current preferences. Both
            // `disable_upload` and the port forwarder are fixed when the session
            // is built, so a session that seeds cannot be talked out of it and
            // one built without a mapping cannot grow one — see `set_seeding`.
            if let Some(session) = inner.session.clone() {
                if inner.seeding == seeding && inner.port_forward == port_forward {
                    return Ok((session, port));
                }
            }
        }
        self.shutdown_session().await;

        // librqbit does not create this itself: without it every torrent fails
        // its initial check with "error storing initial check bitfield", which
        // is the resume data it was just asked to keep.
        let session_dir = dir.join(SESSION_DIR);
        std::fs::create_dir_all(&session_dir)
            .map_err(|e| format!("cannot create the session store: {e}"))?;
        // Before the session, never after: what it removes are torrents the
        // session is about to restore.
        prune_orphaned_store(&session_dir);
        let session = Session::new_with_opts(
            dir,
            SessionOptions {
                // **Persistence is on for one reason: `fastresume`**, the only
                // way to skip re-hashing on every open. Without it librqbit
                // verifies the *whole torrent* — holes included, since the files
                // are created at full length — and that is not a small cost:
                // measured at 3127 ms for a 7.5 GB season with 46 MB fetched,
                // against ~2 ms with the resume data. Hashing runs at ~2.3 GB/s
                // here, so the wait grows with the torrent rather than with what
                // was actually watched.
                //
                // What it costs is a list of torrents restored when the session
                // is built (measured: 21 ms for one) — which is the
                // background-client behavior this app refuses, hence
                // `pause_restored` immediately after. Two things measured and
                // *not* a problem: the stored `paused` flag is honoured, and
                // data deleted behind our back is caught rather than trusted —
                // librqbit then reports 0 bytes held and a read blocks for
                // pieces, instead of quietly serving back the zeros a sparse
                // file would give.
                //
                // The folder is ours rather than librqbit's default (an OS
                // config directory): everything this feature writes belongs
                // under the cache it can be cleared from.
                persistence: Some(librqbit::SessionPersistenceConfig::Json {
                    folder: Some(session_dir),
                }),
                fastresume: true,
                // **The DHT keeps no state of its own, and that settles three
                // things at once.** Its persistence file records the port it
                // bound and rebinds it next time — so tearing the session down
                // and building another, which the seeding switch does, hits
                // "address already in use" and leaves the player unable to open
                // anything until a restart. It also wrote that file to
                // `com.rqbit.dht` in the OS cache root, outside the directory
                // this feature can be cleared from. What it buys is a warm
                // routing table, and that mattered far more before magnets were
                // resolved from our own metadata cache: the DHT is now reached
                // only for a torrent genuinely being opened for the first time,
                // where a bootstrap of a few hundred milliseconds disappears
                // into a ten-second lookup.
                disable_dht_persistence: true,
                // Off by default, and that default is a safety decision rather
                // than a technical one: in Germany and several other
                // jurisdictions the exposure from *uploading* copyrighted
                // material is categorically worse than from downloading it, and
                // a video player must not opt its users into that silently.
                // What the flag does is thorough — librqbit stops advertising
                // which pieces it has (no bitfield, no `have`), refuses piece
                // requests outright, and drops peers once the file is complete.
                disable_upload: !seeding,
                // Without a listener the session announces `port=0`, and
                // trackers refuse that outright (opentrackr answers "Port
                // can't be 0", the tracker 403s) — so every tracker announce was
                // silently worthless and the DHT was the *only* peer source,
                // which is exactly the redundancy failure that made its
                // Windows death (see vendor/README.md) a total outage. A real
                // port also lets NAT-ed seeds connect to us, which is how a
                // home-seeded swarm often reaches a leecher at all. A range,
                // not one port: the seeding switch rebuilds the session and
                // librqbit takes the first port that binds. No UPnP — a video
                // player does not open router ports behind the user's back.
                listen_port_range: Some(42800..42900),
                // **Off by default and opt-in, because it changes the machine
                // rather than the app**: a mapping makes this port reachable
                // from the internet for as long as the session lives. What it
                // buys is measured and large — of ~30 addresses one tracker
                // announce returned, 20–22 never answered a SYN, i.e. they are
                // behind NAT and can only ever be reached if they dial us. A
                // reachable client turns those from unreachable into possible.
                // See upnp.rs, which is what lets the setting say whether the
                // router actually did it: librqbit's forwarder reports to
                // nobody, and a switch that cannot tell is worse than none.
                enable_upnp_port_forwarding: port_forward,
                ..Default::default()
            },
        )
        .await
        .map_err(|e| format!("torrent session failed: {e:#}"))?;

        // **Before anything else can reach it.** A torrent that was live when
        // the app last closed is restored live, and would start talking to peers
        // for something nobody asked to watch — measured, and the one real cost
        // of persistence. `select` unpauses on demand, so nothing is lost by
        // insisting every restored torrent starts still.
        pause_restored(&session).await;

        let mut inner = self.inner.lock().await;
        inner.session = Some(session.clone());
        inner.seeding = seeding;
        inner.port_forward = port_forward;
        Ok((session, port))
    }

    /// The port librqbit is listening on, or 0 when no session has been built.
    ///
    /// What `upnp.rs` asks the router about — and the reason "no session yet" is
    /// a state of its own rather than a failure: nothing is mapped before the
    /// first torrent, because the session that would ask for it does not exist.
    pub async fn listen_port(&self) -> u16 {
        let inner = self.inner.lock().await;
        inner
            .session
            .as_ref()
            .and_then(|s| s.tcp_listen_port())
            .unwrap_or(0)
    }

    /// Stop the session and forget every torrent in it.
    ///
    /// The only way to stop seeding, because `disable_upload` is baked into the
    /// session when it is built and librqbit offers no way to change it after.
    /// Everything already fetched stays on disk, so re-opening the magnet
    /// continues rather than starting over — what ends is the peer traffic,
    /// which is the entire point of doing this.
    async fn shutdown_session(&self) {
        let session = {
            let mut inner = self.inner.lock().await;
            inner.torrents.clear();
            inner.session.take()
        };
        if let Some(session) = session {
            session.stop().await;
        }
    }

    /// Apply the seeding preference to whatever is running.
    ///
    /// Turning it **off has to take effect now**, not at the next magnet: the
    /// switch is a statement about what the machine is doing this second, and a
    /// setting that reads "off" while pieces are still going out would be a lie
    /// about the one behavior with legal weight attached to it. That costs the
    /// current stream — the session goes with it — which is the honest trade and
    /// is what the settings hint warns about.
    ///
    /// Turning it **on** needs no such urgency, but goes down the same path so
    /// there is only one rule to reason about.
    pub async fn set_seeding(&self, seeding: bool) -> bool {
        self.rebuild_if(|inner| inner.seeding != seeding).await
    }

    /// Apply the port-mapping preference, on the same terms as seeding.
    ///
    /// Turning it **off** has the same urgency for the same shape of reason: the
    /// switch is a statement about what this machine is exposing right now, and
    /// one reading "off" over a router that is still forwarding the port would
    /// be a lie about the only setting here that changes the machine rather than
    /// the app. `enable_upnp_port_forwarding` is baked into the session, so both
    /// directions cost the running torrent — which is what the hint warns about.
    /// The mapping itself lapses by itself once the forwarder stops renewing it
    /// (librqbit takes a 60 s lease).
    pub async fn set_port_forward(&self, on: bool) -> bool {
        self.rebuild_if(|inner| inner.port_forward != on).await
    }

    /// Tear the session down when a preference baked into it has changed.
    /// Returns whether it did, which the frontend turns into "the current
    /// torrent stopped", since it did.
    async fn rebuild_if(&self, changed: impl FnOnce(&Inner) -> bool) -> bool {
        let needed = {
            let inner = self.inner.lock().await;
            inner.session.is_some() && changed(&inner)
        };
        if needed {
            self.shutdown_session().await;
        }
        needed
    }

    /// Resolve a magnet (or a `.torrent` URL/path) into its file list.
    ///
    /// Added **paused with nothing selected**: this call answers "what is in
    /// here", and answering it must not start a download. What turns a file into
    /// traffic is a request for it arriving at the server.
    pub async fn add(
        self: &Arc<Self>,
        dirs: &Dirs,
        source: String,
        seeding: bool,
        port_forward: bool,
    ) -> Result<TorrentInfo, String> {
        let (session, port) = self.ensure_started(dirs, seeding, port_forward).await?;
        let dir = &dirs.state;

        let source = source.trim().to_string();

        // **A `.torrent` file on disk is the metadata itself**, so it is read
        // here rather than handed over as a source string: `AddTorrent::Url`
        // understands `http:`, `https:` and `magnet:` and nothing else, and a
        // local path reaches it as "provided path is not a valid magnet URL".
        // Reading it also means the hash below is *known* rather than hinted,
        // which is what makes a dropped file as first-class as a magnet — same
        // hash-named folder, same metadata cache, same history.
        let local_file = !source.starts_with("magnet:") && !source.contains("://");
        let file_bytes = if local_file {
            Some(std::fs::read(&source).map_err(|e| format!("{e}"))?)
        } else {
            None
        };

        // The folder has to be decided *before* the torrent is added, and it is
        // named after the info hash — which a magnet already carries in its
        // `xt=urn:btih:`. Parsing it costs nothing; asking librqbit would mean
        // resolving the magnet twice, and a resolve is a ten-second DHT lookup.
        //
        // A `.torrent` **URL** is the one source left with no hash to read, so
        // there the folder falls back to librqbit's own default (the torrent's
        // name). Such an entry simply shows up in `torrent_list` as one that
        // cannot be resumed, which is the same treatment folders from the older
        // layout get.
        let hinted_hash = match file_bytes.as_deref() {
            Some(bytes) => Some(
                librqbit::torrent_from_bytes::<librqbit::ByteBuf>(bytes)
                    .map_err(|e| format!("not a torrent file: {e:#}"))?
                    .info_hash
                    .as_string(),
            ),
            None => librqbit::Magnet::parse(&source)
                .ok()
                .and_then(|m| m.as_id20())
                .map(|id| id.as_string()),
        };

        // **The metadata has to be in hand before the folder can be chosen**,
        // because the folder is named after the torrent — so the order here is
        // now: get the bytes, then name the directory, then add. Three sources,
        // in the order they cost nothing, something, and a swarm.
        //
        // A `.torrent` file *is* the metadata. A magnet opened before has it
        // cached, which is what makes reopening a season instant (measured
        // 1.52 s → 4.45 ms) — librqbit runs with `persistence` for `fastresume`
        // but forgets nothing else, so without our own copy every open would go
        // back to the DHT.
        let mut meta_bytes = match file_bytes {
            Some(bytes) => Some(bytes),
            None => hinted_hash
                .as_ref()
                .and_then(|h| std::fs::read(meta_path(&dir, h)).ok()),
        };
        let from_cache = meta_bytes.is_some() && !local_file;

        // A magnet nobody here has opened: the swarm is the only place the name
        // exists. `list_only` is exactly this question — it resolves the
        // metadata and returns **before any storage is created**, so it costs
        // one lookup and touches neither the disk nor the session, and the
        // bytes it hands back are what the real add below replays in
        // milliseconds. Which is why resolving twice is not what this does.
        let mut seed = None;
        if meta_bytes.is_none() {
            if let Some(hash) = hinted_hash.as_deref() {
                let port = session.tcp_listen_port().unwrap_or(0);
                // See `announce_peers`: a torrent added paused announces
                // `port=0`, which trackers answer with almost nothing, so the
                // peers for the metadata fetch are asked for directly.
                seed = Some(announce_peers(&source, hash, port).await);
                let probe = AddTorrentOptions {
                    list_only: true,
                    initial_peers: seed.clone(),
                    ..Default::default()
                };
                match tokio::time::timeout(
                    RESOLVE_TIMEOUT,
                    session.add_torrent(AddTorrent::from_url(source.as_str()), Some(probe)),
                )
                .await
                {
                    Ok(Ok(AddTorrentResponse::ListOnly(r))) => {
                        meta_bytes = Some(r.torrent_bytes.to_vec())
                    }
                    Ok(Ok(_)) => eprintln!("[torrent] list_only did not answer with a listing"),
                    Ok(Err(e)) => return Err(format!("{e:#}")),
                    Err(_) => return Err("resolve_timeout".into()),
                }
            }
        }

        // Cache it before anything can fail: the next open is instant even if
        // this one goes wrong from here.
        if let (Some(hash), Some(bytes)) = (hinted_hash.as_deref(), meta_bytes.as_deref()) {
            if !from_cache {
                let path = meta_path(&dir, hash);
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(path, bytes);
            }
        }

        let meta_name = meta_bytes.as_deref().and_then(|b| {
            let meta = librqbit::torrent_from_bytes::<librqbit::ByteBuf>(b).ok()?;
            let name = String::from_utf8_lossy(&meta.info.name?).trim().to_string();
            (!name.is_empty()).then_some(name)
        });

        // **A hash-named folder is renamed the first time we can read the
        // torrent's name**, which is what carries the old layout across without
        // an upgrade step that walks the whole cache. It happens here, before
        // the torrent is added, because a rename under a live torrent is a
        // silent failure on Windows (the files are open) and a set of dangling
        // handles on macOS.
        //
        // The cost is one full re-check on this open: the folder moves out from
        // under librqbit's own store entry, so `fastresume` is discarded and the
        // pieces are hashed again — measured at ~3.2 s for a 7.5 GB season, once
        // per torrent, ever. A failed rename costs nothing at all: the old
        // folder is still found by hash on the next line.
        if let (Some(hash), Some(name)) = (hinted_hash.as_deref(), meta_name.as_deref()) {
            self.rename_legacy_folder(dirs, hash, name).await;
        }

        let folder = hinted_hash
            .as_ref()
            .map(|h| folder_for(dirs, h, meta_name.as_deref()));

        let opts = |initial_peers: Option<Vec<SocketAddr>>| AddTorrentOptions {
            paused: true,
            // Only ever set on the path that has to fetch metadata from the
            // swarm — see `announce_peers` for why librqbit's own announce
            // cannot find them there.
            initial_peers,
            // Reuse whatever of this torrent is already in the cache directory.
            // Without it, re-opening a magnet watched yesterday errors on the
            // existing files instead of continuing from them.
            overwrite: true,
            // **Nothing is wanted yet, and saying so is what keeps a season
            // from costing its full weight the moment it is opened.** Left
            // unset, this is `None`, and librqbit reads that as "everything":
            // `initializing.rs` stretches every file of the torrent to its full
            // length (`set_len`) before handing it over. On APFS and ext4 that
            // is free — a hole allocates nothing — but **NTFS has no sparse
            // file unless somebody asks for one with `FSCTL_SET_SPARSE`, and
            // librqbit never does**, so on Windows a nine-episode season
            // occupied all of itself from the first open while two episodes had
            // actually been fetched. Reported as "the empty files take up
            // space", which is exactly what they do.
            //
            // An empty selection is legal — `compute_only_files` only checks
            // that each index is in range — and it costs nothing later, because
            // `update_only_files`, which `select` already calls, does not
            // preallocate at all: it moves the chunk tracker and the
            // persistence record, and the file grows as pieces are written.
            //
            // It also gives `torrent_offline_file` back a signal it had lost:
            // that gate compares the file's length against the torrent's, and
            // with everything preallocated the comparison was always equal — so
            // on Windows, where the allocated size *is* the length, an untouched
            // file was indistinguishable from a finished one and a poster could
            // be decoded out of zeros.
            only_files: Some(Vec::new()),
            // **The folder carries the info hash**, rather than being librqbit's
            // default of the torrent's own name. `ManagedTorrentOptions
            // ::output_folder` is `pub(crate)`, so a name-derived folder could
            // never be *read back* and the mapping from "this torrent" to "this
            // directory" would have to be reconstructed by guessing — see
            // `folder_name` for why the answer is a suffix rather than an index
            // file, and `sanitize_name` for what a torrent name has to survive
            // before it can be a path at all.
            output_folder: folder
                .as_ref()
                .map(|f| f.to_string_lossy().into_owned()),
            ..Default::default()
        };

        // The bytes above are the whole torrent, so this add reaches no network
        // at all — measured 4.45 ms against a 1.52 s resolve. The `.torrent`
        // **URL** is the one source that gets here with nothing in hand: it has
        // no hash to name a folder or a cache entry with, so it goes to librqbit
        // as a URL and lands in librqbit's own default directory.
        let mut added = None;
        if let Some(bytes) = meta_bytes {
            match tokio::time::timeout(
                RESOLVE_TIMEOUT,
                session.add_torrent(AddTorrent::from_bytes(bytes), Some(opts(seed.clone()))),
            )
            .await
            {
                Ok(Ok(r)) => added = Some(r),
                // A truncated or stale cache file must not make the torrent
                // unopenable — the magnet is still the source of truth.
                Ok(Err(e)) => eprintln!("[torrent] metadata unusable, resolving: {e:#}"),
                Err(_) => eprintln!("[torrent] metadata timed out, resolving"),
            }
        }

        let added = match added {
            Some(r) => r,
            None => tokio::time::timeout(
                RESOLVE_TIMEOUT,
                session.add_torrent(AddTorrent::from_url(source.as_str()), Some(opts(seed))),
            )
            .await
            .map_err(|_| "resolve_timeout".to_string())?
            .map_err(|e| format!("{e:#}"))?,
        };

        let handle = match added {
            AddTorrentResponse::Added(_, h) | AddTorrentResponse::AlreadyManaged(_, h) => h,
            AddTorrentResponse::ListOnly(_) => return Err("torrent not started".into()),
        };

        // The fallback path above resolved from the swarm and nothing has cached
        // it — and a `.torrent` URL only learns its own hash here. Best-effort:
        // failing to write it costs a lookup on the next open and nothing else.
        if let Ok(bytes) = handle.with_metadata(|m| m.torrent_bytes.clone()) {
            let path = meta_path(&dir, &handle.info_hash().as_string());
            if !path.is_file() {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(path, &bytes);
            }
        }

        let info_hash = handle.info_hash().as_string();
        let files = handle
            .with_metadata(|meta| {
                meta.file_infos
                    .iter()
                    .enumerate()
                    .map(|(index, fi)| {
                        let path = fi
                            .relative_filename
                            .components()
                            .map(|c| c.as_os_str().to_string_lossy())
                            .collect::<Vec<_>>()
                            .join("/");
                        TorrentFile {
                            index,
                            url: stream_url(port, &info_hash, index, &path),
                            path,
                            size: fi.len,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .map_err(|e| format!("{e:#}"))?;

        let name = handle.name();

        let mut inner = self.inner.lock().await;
        inner.torrents.entry(info_hash.clone()).or_insert(Entry {
            handle,
            selected: HashSet::new(),
        });

        Ok(TorrentInfo {
            info_hash,
            name,
            files,
        })
    }

    /// Mark a file wanted, start the torrent if it is not running, and wait
    /// until it can actually be streamed.
    ///
    /// Idempotent and cheap on the common path (the file is already selected and
    /// the torrent is live), because this runs on every HTTP request — including
    /// the several that one seek produces.
    ///
    /// **A freshly added torrent is not ready to be touched**, and the order of
    /// the three steps here is the whole lesson. `add_torrent` returns while the
    /// torrent is still `Initializing` — checking and allocating the files on
    /// disk, which scales with the size of the *torrent*, not of the file being
    /// watched. In that state librqbit **refuses `update_only_files`** ("can't
    /// update initializing torrent") and `ManagedTorrent::stream` accepts only
    /// `Paused` or `Live`. So the wait comes first, then the selection, then the
    /// start. On Sintel (129 MB, one file) the window is too short to see; on a
    /// nine-episode 7.5 GB season it is seconds and swallowed the whole of
    /// playback, which surfaced as a bare 404 — indistinguishable from "no such
    /// file" until the failure started reporting its reason.
    async fn select(&self, info_hash: &str, index: usize) -> Result<Arc<ManagedTorrent>, String> {
        // The lock is dropped before each wait: initialization is seconds on a
        // large torrent, and holding it would serialize every other request —
        // including the parallel connections ffmpeg opens — behind it.
        let (session, handle) = {
            let inner = self.inner.lock().await;
            let session = inner.session.clone().ok_or("no torrent session")?;
            let entry = inner.torrents.get(info_hash).ok_or("unknown torrent")?;
            (session, entry.handle.clone())
        };

        self.wait_ready(&handle).await?;

        // **An episode already on disk needs no swarm at all.** Initialization
        // has just verified what is there, and `stream()` reads from a *paused*
        // torrent perfectly well — it only blocks when a piece is missing. So a
        // rewatch, or the next episode of a season fetched last night, opens
        // with zero peer connections and zero waiting. Skipping this was the
        // difference between "instant" and "sits there connecting to strangers
        // to be told what it already has".
        if file_complete(&handle, index) {
            return Ok(handle);
        }

        let wanted = {
            let mut inner = self.inner.lock().await;
            let entry = inner.torrents.get_mut(info_hash).ok_or("unknown torrent")?;
            entry.selected.insert(index).then(|| entry.selected.clone())
        };
        if let Some(wanted) = wanted {
            session
                .update_only_files(&handle, &wanted)
                .await
                .map_err(|e| format!("only_files: {e:#}"))?;
        }
        if handle.is_paused() {
            session
                .unpause(&handle)
                .await
                .map_err(|e| format!("unpause: {e:#}"))?;
        }
        // Starting can pass back through `Initializing`, and `stream()` would
        // refuse it there. Cheap when it does not: this returns at once unless
        // the torrent is actually initializing.
        self.wait_ready(&handle).await?;
        Ok(handle)
    }

    /// Give a torrent whose folder is a bare info hash the readable name we can
    /// now read out of its metadata.
    ///
    /// The migration for the old layout, done one torrent at a time at the
    /// moment it is opened — rather than as a sweep over the cache at startup,
    /// which would be a pass over somebody's whole disk for a cosmetic gain and
    /// would run while the session may already hold half of it.
    ///
    /// **A rename must not happen under a live torrent.** On Windows the files
    /// are open and the rename simply fails; on macOS it succeeds and leaves
    /// librqbit writing through handles to a path that no longer exists. So the
    /// torrent is taken out of the session first — which is also what drops the
    /// store entry pointing at the old directory, the thing
    /// `prune_orphaned_store` would otherwise collect on the next launch.
    ///
    /// The price is `fastresume`: the entry that carried the verified piece map
    /// goes with it, so this open re-hashes what is on disk (~3.2 s for a 7.5 GB
    /// season, measured). Once per torrent, ever. Anything that fails here
    /// leaves the old folder exactly as it was, and `folder_for` finds it by
    /// hash regardless — the readable name is a convenience, never the way back
    /// to the data.
    async fn rename_legacy_folder(&self, dirs: &Dirs, hash: &str, name: &str) {
        let hash = hash.to_ascii_lowercase();
        // In whichever root it turns up in, and renamed **in place** — moving it
        // to the current root would be a copy across volumes for a folder that
        // is working perfectly well where it is.
        let Some(from) = dirs.roots.iter().map(|r| r.join(&hash)).find(|p| p.is_dir()) else {
            return;
        };
        let to = from.with_file_name(folder_name(&hash, Some(name)));
        if to == from || to.exists() {
            return;
        }

        let session = {
            let mut inner = self.inner.lock().await;
            inner.torrents.remove(&hash);
            inner.session.clone()
        };
        if let Some(session) = session {
            if let Ok(id) = librqbit::api::TorrentIdOrHash::parse(&hash) {
                let _ = session.delete(id, false).await;
            }
        }
        match std::fs::rename(&from, &to) {
            Ok(()) => eprintln!("[torrent] {hash} renamed to {:?}", to.file_name()),
            Err(e) => eprintln!("[torrent] could not rename {hash}: {e}"),
        }
    }

    async fn wait_ready(&self, handle: &Arc<ManagedTorrent>) -> Result<(), String> {
        let t0 = std::time::Instant::now();
        let r = self.wait_ready_inner(handle).await;
        let ms = t0.elapsed().as_millis();
        // Verifying existing pieces scales with what is already on disk, and it
        // is the wait a viewer sees when reopening a half-watched season — worth
        // being able to attribute rather than guess at.
        if ms > 250 {
            eprintln!("[torrent] initialize took {ms} ms");
        }
        r
    }

    async fn wait_ready_inner(&self, handle: &Arc<ManagedTorrent>) -> Result<(), String> {
        tokio::time::timeout(INIT_TIMEOUT, handle.wait_until_initialized())
            .await
            .map_err(|_| "torrent did not initialize in time".to_string())?
            .map_err(|e| format!("initializing: {e:#}"))
    }

    /// Which stretches of a file are already on disk, as fractions of it.
    ///
    /// This is what turns a torrent's seekbar from a promise into a map: the
    /// viewer can see where a jump lands instantly and where it means waiting,
    /// which is the question the bar is actually being read for. It is also what
    /// makes previews possible before the file is complete — a position inside
    /// one of these ranges can be decoded, one outside cannot.
    ///
    /// **No fork of librqbit was needed for it, contrary to first appearances.**
    /// `with_chunk_tracker` is `pub(crate)`, but `Api::api_dump_haves` is public
    /// and hands the same bitmap out — as a `format!("{:?}")` of a `BitSlice`,
    /// evidently meant for debugging. That is undignified rather than fragile:
    /// the format is pinned by Cargo.lock, `parse_haves` is unit-tested against
    /// the real string, and a parse failure costs the map and nothing else.
    pub async fn buffered(
        &self,
        info_hash: &str,
        index: usize,
    ) -> Vec<(f64, f64)> {
        let (session, handle) = {
            let inner = self.inner.lock().await;
            let Some(session) = inner.session.clone() else {
                return Vec::new();
            };
            let Some(entry) = inner.torrents.get(&info_hash.to_ascii_lowercase()) else {
                return Vec::new();
            };
            (session, entry.handle.clone())
        };

        let dump = librqbit::Api::new(session, None)
            .api_dump_haves(librqbit::api::TorrentIdOrHash::Id(handle.id()));
        let Ok(have) = dump.map(|s| parse_haves(&s)) else {
            return Vec::new();
        };

        handle
            .with_metadata(|m| {
                let Some(fi) = m.file_infos.get(index) else {
                    return Vec::new();
                };
                file_ranges(
                    &have,
                    m.lengths.default_piece_length() as u64,
                    fi.offset_in_torrent,
                    fi.len,
                )
            })
            .unwrap_or_default()
    }

    /// The local path of a torrent file, and whether all of it is there.
    ///
    /// This is what lets the seekbar storyboard and the start-screen posters
    /// work for a torrent at all. They were gated off for every network source
    /// on the grounds that decoding the whole file in the background would pull
    /// it down a second time in an order unrelated to playback — true for a
    /// stream, and no longer true here: the data is an ordinary file on this
    /// disk, so decoding it costs nothing.
    ///
    /// `complete` is not a nicety, and the caller must respect it. librqbit
    /// creates every file at full length and the filesystem makes it **sparse**,
    /// so reading a stretch that has not arrived returns *zeros* rather than
    /// failing — decoding one would not hang, it would quietly produce black
    /// frames and green mush. So the background storyboard, which walks a file
    /// start to end, may only run on a complete file; single frames may be
    /// decoded from an incomplete one, but only at a position `buffered()` says
    /// is there.
    pub async fn local_path(&self, dirs: &Dirs, info_hash: &str, index: usize) -> Option<LocalFile> {
        if !is_info_hash(info_hash) {
            return None;
        }
        let handle = {
            let inner = self.inner.lock().await;
            inner.torrents.get(&info_hash.to_ascii_lowercase())?.handle.clone()
        };
        let complete = file_complete(&handle, index);
        let rel = handle
            .with_metadata(|m| m.file_infos.get(index).map(|f| f.relative_filename.clone()))
            .ok()
            .flatten()?;
        let path = folder_for(dirs, info_hash, None).join(rel);
        path.is_file().then(|| LocalFile {
            path: path.to_string_lossy().into_owned(),
            complete,
        })
    }

    /// Start fetching a file nobody is reading yet.
    ///
    /// The one place the reader does *not* steer the download, and deliberately
    /// bounded to it: while an episode plays, the next one is worth having ready
    /// so a season does not stop between entries. Anything more than one ahead
    /// would be a background client filling the disk with what may never be
    /// watched — which is the line this app draws everywhere else.
    ///
    /// Goes through `select`, so it inherits the whole contract: the wait for
    /// initialization, and the early return that makes it free when the file is
    /// already on disk.
    pub async fn prefetch(&self, info_hash: &str, index: usize) -> Result<(), String> {
        self.select(info_hash, index).await.map(|_| ())
    }

    pub async fn status(&self, info_hash: &str, index: usize) -> TorrentStatus {
        let handle = {
            let inner = self.inner.lock().await;
            match inner.torrents.get(info_hash) {
                Some(e) => e.handle.clone(),
                None => {
                    return TorrentStatus {
                        state: "gone".into(),
                        ..Default::default()
                    }
                }
            }
        };

        let stats = handle.stats();
        let (peers, peers_seen, down_bps, up_bps) = match &stats.live {
            Some(live) => (
                live.snapshot.peer_stats.live,
                live.snapshot.peer_stats.seen,
                // librqbit reports megabits per second; the panel wants bytes.
                live.download_speed.mbps * 125_000.0,
                live.upload_speed.mbps * 125_000.0,
            ),
            None => (0, 0, 0.0, 0.0),
        };

        let file_size = handle
            .with_metadata(|m| m.file_infos.get(index).map(|f| f.len).unwrap_or(0))
            .unwrap_or(0);

        TorrentStatus {
            state: stats.state.to_string(),
            error: stats.error.clone(),
            peers,
            peers_seen,
            down_bps,
            up_bps,
            file_done: stats.file_progress.get(index).copied().unwrap_or(0),
            file_size,
            total_done: stats.progress_bytes,
            total_size: stats.total_bytes,
        }
    }

    /// Stop feeding a torrent — it is no longer what the player is watching.
    ///
    /// Pause rather than delete: the pieces already fetched stay in the cache,
    /// so re-opening the same magnet continues instead of starting over. What it
    /// does end is the uploading and the peer connections, which must not
    /// outlive the viewing.
    pub async fn release(&self, info_hash: &str) {
        let inner = self.inner.lock().await;
        let (Some(session), Some(entry)) = (inner.session.clone(), inner.torrents.get(info_hash))
        else {
            return;
        };
        if !entry.handle.is_paused() {
            let _ = session.pause(&entry.handle).await;
        }
    }

    /// Everything the torrent cache is holding, measured from disk.
    ///
    /// Deliberately does not consult the session or any record of ours: a folder
    /// left by a previous run, or by the older name-based layout, still has to
    /// be visible and deletable. That is what keeps storage management working
    /// when history is switched off and there is nothing remembered at all.
    ///
    /// It does not report session membership either, and that is the correction
    /// rather than an omission: a torrent stays in the session, paused, after
    /// the viewer closes the film, so "the session holds it" answered "playing
    /// right now" with yes for the rest of the run and the delete button was
    /// dead from the moment anything had been watched. Deleting one that is
    /// merely loaded is perfectly safe — `forget` takes it out of the session
    /// before removing the directory — so the only row that must refuse is the
    /// one mpv is streaming from, and that is the frontend's own fact.
    /// **Every root, and in a chosen one only what we created.** A viewer who
    /// points this at their film library must not find their own folders listed
    /// here with a delete button beside them — nor even measured, since walking
    /// somebody's whole media drive to print a number is its own kind of rude.
    /// `is_ours` is the rule, and it is the same one `forget` and
    /// `torrent_clear_cache` are written against.
    pub fn list(&self, dirs: &Dirs) -> Vec<TorrentOnDisk> {
        let mut out = Vec::new();
        for dir in &dirs.roots {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let Some(folder) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                if folder.starts_with('.') || !entry.path().is_dir() {
                    continue;
                }
                if !dirs.is_ours(&entry.path()) {
                    continue;
                }
                let info_hash = folder_hash(&folder);
                // The metadata's own name first — the folder's is sanitized and
                // truncated, so it is what a person reads in a file manager
                // rather than what the torrent is called. But it is a far better
                // fallback than nothing when the cache entry is missing: that
                // row used to read "Раздача без названия" beside a directory
                // that says on its face what it holds.
                let name = info_hash
                    .as_deref()
                    .and_then(|h| cached_name(&dirs.state, h))
                    .or_else(|| folder_label(&folder));
                out.push(TorrentOnDisk {
                    size: dir_size(&entry.path()),
                    path: entry.path().to_string_lossy().into_owned(),
                    info_hash,
                    name,
                    folder,
                });
            }
        }
        out.sort_by(|a, b| b.size.cmp(&a.size));
        out
    }

    /// Hand one torrent's data over to its replacement.
    ///
    /// **BitTorrent has no way to add a file to a torrent** — the info hash
    /// covers the file list and every piece hash, so an uploader who adds an
    /// episode publishes a *different* torrent. Left alone that means
    /// re-downloading a season to gain one episode, because our folders are
    /// named after the info hash and the new one gets an empty one.
    ///
    /// The whole fix is a rename, done **before** the new torrent is added: the
    /// data ends up in the folder the new hash will ask for, and `overwrite`
    /// makes librqbit verify it and mark those pieces as held. Nothing else in
    /// `add` changes, and the "the folder carries the info hash" invariant that
    /// `torrent_list` and `torrent_forget` rely on survives intact — which it
    /// would not if the two were merged into one folder instead. That is also
    /// what keeps deletion unambiguous, the thing qBittorrent's shared-save-path
    /// approach gives up.
    ///
    /// The destination is deliberately the **bare hash** rather than a readable
    /// name: the replacement's own name is not known until its metadata is
    /// resolved, which is the very next thing `add` does — and `add` renames a
    /// hash-named folder the moment it can read a name for it. So the readable
    /// name arrives one step later, through the path that already does it,
    /// instead of this function guessing with the superseded torrent's name.
    pub async fn relocate(
        &self,
        dirs: &Dirs,
        old_hash: &str,
        new_hash: &str,
    ) -> Result<(), String> {
        if !is_info_hash(old_hash) || !is_info_hash(new_hash) {
            return Err("bad hash".into());
        }
        let (old_hash, new_hash) = (old_hash.to_ascii_lowercase(), new_hash.to_ascii_lowercase());
        if old_hash == new_hash {
            return Ok(());
        }
        let Some(from) = find_folder(&dirs.roots, &old_hash) else {
            return Err("nothing to move".into());
        };
        // **Beside the folder it replaces, not in the current root.** The two
        // may be different roots now, and `fs::rename` does not cross a volume —
        // so aiming at the chosen root would turn a rename of a 7.5 GB season
        // into a failure, or into a copy if somebody later "fixed" it that way.
        // The data stays where it already is, and `find_folder` looks in every
        // root regardless.
        let to = from.with_file_name(&new_hash);
        if !from.is_dir() {
            return Err("nothing to move".into());
        }
        // Asked by hash, not by path: the replacement may already have a folder
        // under its readable name, which `to` would not collide with and we
        // would then hand it a second one.
        if find_folder(&dirs.roots, &new_hash).is_some() {
            // The replacement already has data of its own. Merging two folders
            // is a different, riskier operation than a rename, and the caller
            // loses nothing by simply opening the new torrent normally.
            return Err("target exists".into());
        }

        // The old torrent must not be holding the files open across the rename.
        let (session, entry) = {
            let mut inner = self.inner.lock().await;
            (inner.session.clone(), inner.torrents.remove(&old_hash))
        };
        if let (Some(session), Some(entry)) = (session, entry) {
            let _ = session
                .delete(librqbit::api::TorrentIdOrHash::Id(entry.handle.id()), false)
                .await;
        }

        std::fs::rename(&from, &to).map_err(|e| format!("{e}"))?;
        // The cached metadata describes the torrent that no longer owns this
        // folder. Leaving it would hand the *old* file list to the next open.
        let _ = std::fs::remove_file(meta_path(&dirs.state, &old_hash));
        Ok(())
    }

    /// Drop a torrent from the session and delete its data.
    ///
    /// Both halves, in that order: removing the directory while librqbit still
    /// holds the files open would leave it writing pieces into a folder that no
    /// longer exists, which on some filesystems recreates it.
    /// **This is the one command in the player that removes a directory tree,
    /// and it now points wherever the viewer told the player to put files.** So
    /// the guard is not "the name looks alright" but three separate facts, none
    /// of which the caller supplies: the path's parent is a root the player
    /// knows, the folder is one the player created (`is_ours`), and the whole
    /// thing resolves — a path that cannot be canonicalised is refused rather
    /// than guessed at. The UI only ever sends back a row it was given, but a
    /// destructive command must not be safe *because* of what the UI does.
    pub async fn forget(&self, dirs: &Dirs, path: &str) -> Result<u64, String> {
        let path = PathBuf::from(path);
        // Cheap and first: nothing below can turn a traversal into something
        // safe, and refusing it here keeps the rest reading as one rule.
        if path.components().any(|c| c == std::path::Component::ParentDir) {
            return Err("bad folder".into());
        }
        let Some(folder) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
            return Err("bad folder".into());
        };
        if !path.is_dir() || !dirs.is_ours(&path) {
            return Err("not a torrent folder".into());
        }

        // Parsed out of the folder rather than tested against the whole of it:
        // the name now carries the hash in brackets, and a folder with no
        // readable hash at all is one from the older name-based layout, which
        // can be measured and deleted but was never in any session.
        if let Some(hash) = folder_hash(&folder) {
            let session = {
                let mut inner = self.inner.lock().await;
                inner.torrents.remove(&hash);
                inner.session.clone()
            };
            if let Some(session) = session {
                // **By hash, never by the handle we happen to be holding.** This
                // used to look the torrent up in `inner.torrents`, which is
                // filled by `add` alone — so it covered a torrent opened in
                // *this* run and nothing else. A torrent the session restored
                // from its own store was invisible to it, and deleting one
                // therefore removed the directory and our record of it while
                // leaving librqbit's: the next session restored it, and
                // restoring **recreates the folder** (see `prune_orphaned_store`
                // for why), so it came back as a nameless zero-byte row on the
                // start screen. `Session::delete` resolves a hash against its
                // own db, which holds restored torrents too.
                //
                // `delete_files: false` — the directory is ours to remove below,
                // and doing it here would depend on librqbit agreeing with us
                // about which files belong to this torrent.
                if let Ok(id) = librqbit::api::TorrentIdOrHash::parse(&hash) {
                    let _ = session.delete(id, false).await;
                }
            }
            // The cached metadata describes a torrent that is being thrown away.
            // Left behind it is a couple of hundred kilobytes per deleted
            // torrent, in a directory whose whole point is that its size is
            // accounted for.
            //
            // When there is no session at all — the common case, since the
            // session is lazy and deleting from the start screen rarely follows
            // opening something — the store entry is left for
            // `prune_orphaned_store` to collect the next time one is built. It
            // has to run there anyway, so doing it twice would be two places
            // that must agree about librqbit's file format instead of one.
            let _ = std::fs::remove_file(meta_path(&dirs.state, &hash));
        }

        let size = dir_size(&path);
        std::fs::remove_dir_all(&path).map_err(|e| format!("{e}"))?;
        Ok(size)
    }

    /// Delete some files of a torrent and keep the rest.
    ///
    /// **What makes this safe is `validate_fastresume`, not care on our part.**
    /// librqbit verifies at least one piece of every file its resume data claims
    /// to hold, plus a random sample of the others, and a single failure throws
    /// the whole bitfield away (`initializing.rs`). So a file removed behind its
    /// back cannot be served as zeros: the next open re-checks and finds it
    /// missing. The price is that re-check — measured at ~3.2 s for a 7.5 GB
    /// season — paid once, on the next open of this torrent.
    ///
    /// **The order is the part that has to be right.** The torrent leaves the
    /// session before anything is unlinked: on Windows an open file cannot be
    /// deleted at all, and on macOS it can, which is worse — librqbit keeps
    /// writing pieces through a handle to a file that no longer has a name.
    ///
    /// `.meta` is deliberately kept. The torrent is not being forgotten, only
    /// pruned, and that cached metadata is what makes reopening it cost 4.5 ms
    /// instead of a DHT lookup.
    ///
    /// Matching is by **file name against what is actually inside the folder**,
    /// which is both the simplest thing the caller can supply — it has names,
    /// not indices, and the two disagree the moment an uploader inserts an
    /// episode — and the reason no name can escape: nothing is joined onto the
    /// folder path, only entries found within it are considered.
    pub async fn forget_files(
        &self,
        dirs: &Dirs,
        path: &str,
        names: &[String],
    ) -> Result<u64, String> {
        let path = PathBuf::from(path);
        if path.components().any(|c| c == std::path::Component::ParentDir) {
            return Err("bad folder".into());
        }
        if !path.is_dir() || !dirs.is_ours(&path) {
            return Err("not a torrent folder".into());
        }
        if names.is_empty() {
            return Ok(0);
        }
        let wanted: std::collections::HashSet<&str> = names.iter().map(String::as_str).collect();

        if let Some(hash) = path.file_name().and_then(|n| n.to_str()).and_then(folder_hash) {
            let session = {
                let mut inner = self.inner.lock().await;
                inner.torrents.remove(&hash);
                inner.session.clone()
            };
            if let Some(session) = session {
                if let Ok(id) = librqbit::api::TorrentIdOrHash::parse(&hash) {
                    let _ = session.delete(id, false).await;
                }
            }
        }

        let mut freed = 0u64;
        remove_named_files(&path, &wanted, &mut freed);
        Ok(freed)
    }

    /// The one route: `/t/<infohash>/<index>/<anything>`.
    async fn serve(self: Arc<Self>, req: Request<hyper::body::Incoming>) -> Response<Body> {
        if req.method() != Method::GET && req.method() != Method::HEAD {
            return simple(StatusCode::METHOD_NOT_ALLOWED);
        }

        let path = req.uri().path().to_string();
        let mut parts = path.trim_start_matches('/').splitn(4, '/');
        let (Some("t"), Some(hash), Some(index)) = (parts.next(), parts.next(), parts.next()) else {
            return simple(StatusCode::NOT_FOUND);
        };
        let Ok(index) = index.parse::<usize>() else {
            return simple(StatusCode::NOT_FOUND);
        };
        let name = parts.next().unwrap_or("").to_string();
        let range = req
            .headers()
            .get(header::RANGE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string());
        self.serve_file(hash, index, &name, range, req.method() == Method::HEAD)
            .await
    }

    /// Serve one file of one torrent, Range and all. Split out of the route so
    /// the **cast** server can call it: a television cannot fetch from the
    /// loopback address this listener binds, and exposing this one to the LAN
    /// would put every torrent in the session on the network under a guessable
    /// path. So the cast side keeps its single-registered-source-behind-a-token
    /// rule and streams through here.
    pub(crate) async fn serve_file(
        self: &Arc<Self>,
        hash: &str,
        index: usize,
        name: &str,
        range: Option<String>,
        head_only: bool,
    ) -> Response<Body> {
        // The request itself is what selects the file: see the module header.
        //
        // The reason is logged rather than swallowed. This used to answer a bare
        // 404 for every failure, and "the torrent is still initializing" then
        // looked exactly like "no such file" — which cost a diagnosis, because
        // the only visible symptom was a player that would not start on some
        // torrents and would on others.
        let handle = match self.select(hash, index).await {
            Ok(h) => h,
            Err(e) => {
                eprintln!("[torrent] select {hash}/{index} failed: {e}");
                // Not 404: the file exists, it is not ready. A distinct code
                // keeps the two apart in a log and tells ffmpeg to give up on
                // this attempt rather than on the URL.
                return simple(StatusCode::SERVICE_UNAVAILABLE);
            }
        };

        let mut stream = match handle.stream(index) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[torrent] stream {hash}/{index} failed: {e:#}");
                return simple(StatusCode::SERVICE_UNAVAILABLE);
            }
        };
        let total = stream.len();

        let mut res = Response::builder()
            // Without this ffmpeg treats the source as unseekable and the
            // seekbar becomes a progress bar.
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_TYPE, mime_for(name))
            // A DLNA renderer reads these before it will seek; harmless to
            // everyone else (see cast.rs, where the same pair is set).
            .header("transferMode.dlna.org", "Streaming")
            .header(
                "contentFeatures.dlna.org",
                "DLNA.ORG_OP=01;DLNA.ORG_CI=0;\
                 DLNA.ORG_FLAGS=01700000000000000000000000000000",
            );

        let range = range.as_deref().and_then(|v| parse_range(v, total));

        let (status, start, len) = match range {
            Some(Some((start, end_inclusive))) => {
                res = res.header(
                    header::CONTENT_RANGE,
                    format!("bytes {start}-{end_inclusive}/{total}"),
                );
                (
                    StatusCode::PARTIAL_CONTENT,
                    start,
                    end_inclusive - start + 1,
                )
            }
            // A Range header that was present and unsatisfiable. Saying so is
            // what makes ffmpeg fall back sanely instead of reading garbage.
            Some(None) => {
                return Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(header::CONTENT_RANGE, format!("bytes */{total}"))
                    .body(empty())
                    .unwrap()
            }
            None => (StatusCode::OK, 0, total),
        };

        let res = res.status(status).header(header::CONTENT_LENGTH, len);

        if head_only {
            return res.body(empty()).unwrap();
        }

        if start > 0 && stream.seek(std::io::SeekFrom::Start(start)).await.is_err() {
            return simple(StatusCode::INTERNAL_SERVER_ERROR);
        }

        let body = StreamBody::new(
            ReaderStream::with_capacity(stream.take(len), STREAM_CHUNK).map(|r| r.map(Frame::data)),
        );
        res.body(BoxBody::new(body)).unwrap()
    }
}

/// Ask a magnet's own trackers who is in the swarm, so resolving it has
/// somewhere to start.
///
/// **A torrent is added paused — and upstream ties the *announced port* to
/// whether it is started.** `make_peer_rx` passes `None` for the port unless the
/// torrent is running, and a tracker asked from port 0 treats the caller as
/// unable to accept connections and answers with almost nothing: measured
/// against one such tracker on one info hash, **1 peer from port 0 against 26 from a
/// real one**. Adding paused is not negotiable here (it is the whole reason
/// "what is in this torrent" costs no download), so metadata is left to the DHT
/// alone — and a torrent whose DHT records have gone stale then never resolves
/// at all. Measured on a release with 24 live seeders: three fresh opens, three
/// 90-second timeouts, while the same torrent streams at 800 KB/s the moment it
/// has metadata.
///
/// So the swarm is asked directly, once, and the addresses are handed over as
/// `initial_peers`. Best effort in every direction — a tracker that fails,
/// answers something unparseable or is not HTTP costs nothing, because the DHT
/// is running regardless and this only ever *adds* somewhere to look.
///
/// The peer id is a throwaway rather than the session's, which is private. The
/// cost is that a tracker may briefly list two ids from this address; they
/// expire, and a client that restarts does the same thing.
async fn announce_peers(magnet: &str, info_hash: &str, port: u16) -> Vec<SocketAddr> {
    let (Some(hash), Ok(parsed)) = (hex_bytes(info_hash), librqbit::Magnet::parse(magnet)) else {
        return Vec::new();
    };
    let urls: Vec<String> = parsed
        .trackers
        .iter()
        .filter(|t| t.starts_with("http://") || t.starts_with("https://"))
        .take(SEED_ANNOUNCE_TRACKERS)
        .cloned()
        .collect();
    if urls.is_empty() {
        return Vec::new();
    }

    let peer_id = format!("-rQ0000-{:012x}", rand::random::<u64>() & 0xffff_ffff_ffff);
    let client = reqwest::Client::new();
    let asked = urls.len();
    let results = futures_util::future::join_all(
        urls.iter()
            .map(|url| announce_one(&client, url, &hash, peer_id.as_bytes(), port)),
    )
    .await;

    let mut seen = HashSet::new();
    let peers: Vec<SocketAddr> = results
        .into_iter()
        .flatten()
        .filter(|a| seen.insert(*a))
        .collect();
    eprintln!(
        "[torrent] seeding the resolve with {} peer(s) from {asked} tracker(s)",
        peers.len()
    );
    peers
}

async fn announce_one(
    client: &reqwest::Client,
    url: &str,
    info_hash: &[u8],
    peer_id: &[u8],
    port: u16,
) -> Vec<SocketAddr> {
    // The announce URL's own query is kept and re-appended, exactly as the
    // vendored tracker client does it: the `?magnet` marker some URLs carry, and a
    // private tracker's passkey live there, and replacing the query is a 403.
    let (base, base_query) = match url.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (url, None),
    };
    let mut target = format!(
        "{base}?info_hash={}&peer_id={}&event=started&port={port}\
         &uploaded=0&downloaded=0&left=1&compact=1&no_peer_id=1&numwant=200",
        urlencode_bytes(info_hash),
        urlencode_bytes(peer_id),
    );
    if let Some(q) = base_query {
        target.push('&');
        target.push_str(q);
    }

    let req = client
        .get(&target)
        // Without it the WAF in front of several trackers answers 403 — the
        // same header the vendored announce had to grow, for the same reason.
        .header(reqwest::header::USER_AGENT, "rqbit")
        .send();
    let body = match tokio::time::timeout(SEED_ANNOUNCE_TIMEOUT, req).await {
        Ok(Ok(res)) if res.status().is_success() => res.bytes().await.ok(),
        Ok(Ok(res)) => {
            eprintln!("[torrent] {base} answered {}", res.status());
            None
        }
        Ok(Err(e)) => {
            eprintln!("[torrent] {base} failed: {e}");
            None
        }
        Err(_) => {
            eprintln!("[torrent] {base} timed out");
            None
        }
    };
    body.map(|b| compact_peers(&b)).unwrap_or_default()
}

/// The `peers` field of an announce response, compact form only.
///
/// Scanned rather than deserialized, because this wants one field out of a
/// dictionary whose other keys vary by tracker and are of no interest — and
/// because a strict parser is exactly what made librqbit discard whole
/// responses over two absent statistics (see vendor/README.md). The length being
/// a non-zero multiple of six is the sanity check: the dictionary form of
/// `peers` fails it, and so does a stray match inside binary data.
fn compact_peers(body: &[u8]) -> Vec<SocketAddr> {
    const KEY: &[u8] = b"5:peers";
    let Some(at) = body.windows(KEY.len()).position(|w| w == KEY) else {
        return Vec::new();
    };
    let rest = &body[at + KEY.len()..];
    let Some(colon) = rest.iter().position(|b| *b == b':') else {
        return Vec::new();
    };
    let Ok(len) = std::str::from_utf8(&rest[..colon]).unwrap_or("").parse::<usize>() else {
        return Vec::new();
    };
    if len == 0 || len % 6 != 0 || rest.len() < colon + 1 + len {
        return Vec::new();
    }
    rest[colon + 1..colon + 1 + len]
        .chunks_exact(6)
        .map(|c| {
            SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::new(c[0], c[1], c[2], c[3]),
                u16::from_be_bytes([c[4], c[5]]),
            ))
        })
        // Port zero is not an address anything can be reached at.
        .filter(|a| a.port() != 0)
        .collect()
}

fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    if !is_info_hash(s) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// The piece bitmap out of `Api::api_dump_haves`.
///
/// The string is a `BitSlice` debug print — a header naming the type, its
/// address and length, then the bits: `… { addr: 0x…, head: 000, bits: 10 }
/// [1, 0, 1, 1, 0, 0, 0, 1, 1, 0]`. Only the bracketed tail is wanted, hence
/// `rfind`: the header contains brackets of its own on some builds, and taking
/// the *last* one cannot pick up the wrong list.
fn parse_haves(dump: &str) -> Vec<bool> {
    let Some(start) = dump.rfind('[') else {
        return Vec::new();
    };
    let body = &dump[start + 1..];
    let body = body.split(']').next().unwrap_or("");
    body.split(',')
        .filter_map(|t| match t.trim() {
            "1" | "true" => Some(true),
            "0" | "false" => Some(false),
            _ => None,
        })
        .collect()
}

/// Turn a piece bitmap into the fractions of ONE file that are present.
///
/// Pieces are laid over the whole torrent, so the first and last piece of a file
/// are usually shared with its neighbours — the intersection below is what stops
/// a shared piece from claiming bytes that belong to the file next door.
/// Adjacent runs are merged, because a seekbar wants a handful of bands rather
/// than two thousand hairlines.
fn file_ranges(have: &[bool], piece_len: u64, offset: u64, len: u64) -> Vec<(f64, f64)> {
    if piece_len == 0 || len == 0 {
        return Vec::new();
    }
    let first = (offset / piece_len) as usize;
    let last = ((offset + len - 1) / piece_len) as usize;
    let mut out: Vec<(f64, f64)> = Vec::new();
    for p in first..=last {
        if !have.get(p).copied().unwrap_or(false) {
            continue;
        }
        let ps = p as u64 * piece_len;
        let pe = ps + piece_len;
        // Clamp into the file, then make it file-relative.
        let s = ps.max(offset) - offset;
        let e = pe.min(offset + len) - offset;
        if e <= s {
            continue;
        }
        let (sf, ef) = (s as f64 / len as f64, e as f64 / len as f64);
        match out.last_mut() {
            Some(prev) if sf <= prev.1 + 1e-9 => prev.1 = ef,
            _ => out.push((sf, ef)),
        }
    }
    out
}

/// Pause everything the session restored from its own store.
///
/// Reached through the public `Api`, which is the only way to enumerate a
/// session's torrents from outside — `Session::db` is private. A torrent that
/// is already paused is left alone; the point is only that none of them are
/// *running* before the viewer has asked for anything.
async fn pause_restored(session: &Arc<Session>) {
    let api = librqbit::Api::new(session.clone(), None);
    let ids: Vec<_> = api
        .api_torrent_list()
        .torrents
        .into_iter()
        .filter_map(|t| t.id)
        .collect();

    for id in ids {
        let id = librqbit::api::TorrentIdOrHash::Id(id);
        // **Wait for it to initialize first.** A restored torrent starts in
        // `Initializing`, where librqbit refuses to pause it — and the moment
        // that finishes it goes live if the store said it was running, which is
        // exactly what this is here to prevent. Measured, the wait is a couple
        // of milliseconds with `fastresume`, because there is nothing to hash.
        for _ in 0..RESTORE_SETTLE_TRIES {
            match api.api_stats_v1(id) {
                Ok(st) if !matches!(st.state.to_string().as_str(), "initializing") => break,
                Ok(_) => tokio::time::sleep(RESTORE_SETTLE_STEP).await,
                Err(_) => break,
            }
        }
        if let Err(e) = api.api_torrent_action_pause(id).await {
            // **Already paused is the success case, not a failure**, and
            // librqbit spells it two ways depending on where the refusal comes
            // from: "not live" from the state machine, and "torrent is already
            // paused" from the action itself — the second is what an ordinary
            // startup prints, since most restored torrents were stored paused.
            // Reporting either one puts an error in the log for the outcome
            // this function exists to reach.
            let msg = format!("{e:#}");
            if !msg.contains("not live") && !msg.contains("already paused") {
                eprintln!("[torrent] pausing a restored torrent failed: {msg}");
            }
        }
    }
}

/// The torrent's own name, out of the metadata cached beside its data.
///
/// Cheap enough to do for every row of the storage list: the parse borrows from
/// the buffer rather than copying it, and the bulk of a `.torrent` is the piece
/// hashes, which are one byte string it never looks inside. Measured warm on a
/// 227 KB file (a ten-episode 4K season): **0.16 ms**, and a torrent cache holds
/// a handful of entries, not hundreds.
///
/// A file that will not parse gives no name and no error — it is a cache, the
/// row still measures and deletes, and `add` already falls back to the magnet
/// when these bytes turn out to be unusable.
fn cached_name(dir: &std::path::Path, info_hash: &str) -> Option<String> {
    let bytes = std::fs::read(meta_path(dir, info_hash)).ok()?;
    let meta = librqbit::torrent_from_bytes::<librqbit::ByteBuf>(&bytes).ok()?;
    let name = meta.info.name?;
    let name = String::from_utf8_lossy(&name).trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Forget the torrents in librqbit's store whose data folder is gone.
///
/// **A restore recreates the folder**, which is what makes a stale entry visible
/// rather than merely untidy: every restored torrent goes through
/// `Initializing`, and that calls `FileStorage::init`, which is
/// `create_dir_all` plus `create(true)` on every file of the torrent, with the
/// selected ones getting their full length back through `ensure_file_length`.
/// So a torrent whose directory was deleted comes back as an empty copy of
/// itself — a folder named by its info hash, nothing allocated inside it, and
/// nothing left on our side to name it with, since the magnet was forgotten
/// along with the data. That is the "Unnamed torrent · No link saved · 0 MB"
/// row, and it came back after every restart because the store still listed it
/// and nothing ever took it out.
///
/// A missing folder is unambiguous evidence and not a guess: every folder here
/// is ours, inside the cache directory, and a torrent still in the store has its
/// own recreated the moment it is restored. So this covers every way one can go
/// — `forget` with no session to delete from, `torrent_clear_cache`, and a
/// viewer emptying the cache in Finder — where a fix in `forget` alone covers
/// only the first.
///
/// The store's shape is read as plain JSON rather than through librqbit's own
/// types, which are private: `{"torrents": {<id>: {info_hash, output_folder,
/// …}}}` beside one `<hash>.torrent` and one `<hash>.bitv` per entry, all three
/// of which its `delete` removes together. Anything unparseable is left exactly
/// as it is — the cost of skipping is a ghost row, the cost of guessing wrong is
/// somebody's downloaded season.
fn prune_orphaned_store(session_dir: &std::path::Path) {
    let db_path = session_dir.join("session.json");
    let Ok(text) = std::fs::read_to_string(&db_path) else {
        return;
    };
    let Ok(mut db) = serde_json::from_str::<serde_json::Value>(&text) else {
        eprintln!("[torrent] the session store did not parse; leaving it alone");
        return;
    };
    let Some(torrents) = db.get_mut("torrents").and_then(|t| t.as_object_mut()) else {
        return;
    };

    let mut dropped = Vec::new();
    torrents.retain(|_, t| {
        // An entry that does not say where its data is cannot be judged, so it
        // stays. Ours all carry it — `add` sets `output_folder` explicitly.
        let Some(folder) = t.get("output_folder").and_then(|f| f.as_str()) else {
            return true;
        };
        if std::path::Path::new(folder).is_dir() {
            return true;
        }
        if let Some(hash) = t.get("info_hash").and_then(|h| h.as_str()) {
            dropped.push(hash.to_ascii_lowercase());
        }
        false
    });
    if dropped.is_empty() {
        return;
    }

    // Written the way librqbit writes it — temp file, then rename — because a
    // half-written store is one every torrent on this machine is restored from.
    let tmp = db_path.with_extension("json.pruning");
    let Ok(bytes) = serde_json::to_vec(&db) else {
        return;
    };
    if std::fs::write(&tmp, bytes).is_err() || std::fs::rename(&tmp, &db_path).is_err() {
        eprintln!("[torrent] could not rewrite the session store");
        let _ = std::fs::remove_file(&tmp);
        return;
    }
    for hash in &dropped {
        // The resume data and the metadata librqbit keeps beside the store; the
        // copy in `.meta` is ours and `forget` removes it there.
        let _ = std::fs::remove_file(session_dir.join(format!("{hash}.torrent")));
        let _ = std::fs::remove_file(session_dir.join(format!("{hash}.bitv")));
    }
    eprintln!(
        "[torrent] dropped {} torrent(s) from the session store: the data is gone",
        dropped.len()
    );
}

/// Where the cached torrent metadata for an info hash lives.
///
/// Beside the data folders rather than inside one, under a dot-name so
/// `torrent_list` skips it — it is ours, not a torrent, and it must not show up
/// as something occupying space that can be deleted.
fn meta_path(dir: &std::path::Path, info_hash: &str) -> PathBuf {
    dir.join(".meta").join(format!("{info_hash}.torrent"))
}

/// Is this file of the torrent completely on disk?
///
/// Read from the verified piece state that initialization has just produced —
/// `file_progress` is populated in the paused state too, so this is answerable
/// before the torrent has spoken to anyone.
fn file_complete(handle: &Arc<ManagedTorrent>, index: usize) -> bool {
    let Ok(Some(len)) = handle.with_metadata(|m| m.file_infos.get(index).map(|f| f.len)) else {
        return false;
    };
    // A zero-length file is not "complete", it is nothing worth streaming.
    len > 0 && handle.stats().file_progress.get(index).copied().unwrap_or(0) >= len
}

/// A directory name this build wrote, as opposed to one from the older
/// name-based layout. Exactly 40 hex characters, which no torrent name is by
/// accident.
fn is_info_hash(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The torrent name, made safe to be one path component on both platforms.
///
/// Everything here is a Windows rule; none of it costs anything on macOS, and
/// two of them are the kind of failure that only appears on somebody else's
/// machine. **Reserved device names** (`CON`, `NUL`, `COM1`…) cannot be a file
/// or a folder in any directory, with or without an extension, so a release
/// literally called `AUX` would be undeletable rather than merely odd — hence
/// the suffix that keeps the stem from being one. **A trailing dot or space is
/// silently dropped** by the Win32 layer, so a folder created as `Show.` is
/// afterwards addressed as `Show`, and every path we build from the name we
/// asked for misses it.
///
/// The length cap is the other half. A release name of ninety characters is
/// ordinary, the info hash adds 43 more, and inside the torrent there is often
/// another directory and a long file name on top — all of it against a path
/// limit that is still 260 by default on Windows. The readable part is a
/// convenience; the hash carries the identity, so it is the readable part that
/// gets cut. Cut on a **character** boundary, never a byte one, or a Cyrillic
/// name loses half a code point and stops being valid UTF-8.
fn sanitize_name(name: &str) -> String {
    const MAX_CHARS: usize = 80;
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];

    let mut out = String::with_capacity(name.len().min(MAX_CHARS * 4));
    let mut chars = 0;
    for c in name.chars() {
        if chars >= MAX_CHARS {
            break;
        }
        // The Windows set, plus the control range, plus the separators — a name
        // is one path component and may not grow another.
        let safe = match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => ' ',
            // Ours, not the platform's: the hash is written in brackets and a
            // name carrying its own would make the suffix unparseable.
            '[' | ']' => ' ',
            c if (c as u32) < 0x20 => ' ',
            c => c,
        };
        out.push(safe);
        chars += 1;
    }

    // Runs of spaces collapse, because the substitutions above make them.
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut trimmed = collapsed.trim_end_matches('.').trim().to_string();
    // **The stem is what makes a name reserved, not the whole of it**:
    // `com1.2026` addresses the serial port exactly as `COM1` does, extension
    // or no extension. So the escape goes at the end of the stem rather than at
    // the end of the name, which is where it was first written and where it
    // achieves nothing.
    let stem_len = trimmed.find('.').unwrap_or(trimmed.len());
    if RESERVED
        .iter()
        .any(|r| trimmed[..stem_len].eq_ignore_ascii_case(r))
    {
        trimmed.insert(stem_len, '_');
    }
    trimmed
}

/// `<Name> [<infohash>]`, or the bare hash when there is no name to use.
///
/// **The folder keeps naming itself**, which is the whole reason this is a
/// suffix rather than an index file mapping hash to directory.
/// `ManagedTorrentOptions.output_folder` is `pub(crate)`, so the mapping from a
/// torrent to its directory can never be read back out of librqbit and has to
/// be recoverable from what is on disk — the same constraint that made the
/// folder a bare info hash in the first place, and the same reason `cached_name`
/// reads the torrent's own metadata instead of trusting our localStorage.
///
/// A side file would answer it too, and would be a second source of truth to
/// keep in step with the directory: a rename, a half-written index or a folder
/// restored from a backup all put the two out of agreement, and the failure is
/// a torrent that cannot be found or, worse, a delete aimed at the wrong path.
fn folder_name(hash: &str, name: Option<&str>) -> String {
    let hash = hash.to_ascii_lowercase();
    match name.map(sanitize_name).filter(|n| !n.is_empty()) {
        Some(name) => format!("{name} [{hash}]"),
        None => hash,
    }
}

/// The info hash a folder belongs to, or `None` for one this build never wrote.
///
/// Both layouts: the bare hash, and the readable `<Name> [<hash>]`. Anything
/// else is a folder from the older name-based layout, which can be measured and
/// deleted but never resumed.
fn folder_hash(folder: &str) -> Option<String> {
    if is_info_hash(folder) {
        return Some(folder.to_ascii_lowercase());
    }
    let inner = folder.strip_suffix(']')?.rsplit_once(" [")?.1;
    is_info_hash(inner).then(|| inner.to_ascii_lowercase())
}

/// Delete every file under `dir` whose name is wanted, and any directory left
/// empty by doing so. `dir` itself stays: the torrent is being pruned, not
/// forgotten, and an absent folder would read as a torrent that vanished.
fn remove_named_files(
    dir: &std::path::Path,
    wanted: &std::collections::HashSet<&str>,
    freed: &mut u64,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Never followed: a link inside the folder points somewhere we were not
        // given permission to delete from.
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_symlink() {
            continue;
        }
        if meta.is_dir() {
            remove_named_files(&path, wanted, freed);
            // Best-effort and non-recursive: `remove_dir` refuses a directory
            // that still holds anything, which is exactly the check wanted here.
            let _ = std::fs::remove_dir(&path);
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !wanted.contains(name) {
            continue;
        }
        let size = file_disk_size(&meta);
        if std::fs::remove_file(&path).is_ok() {
            *freed += size;
        }
    }
}

/// The readable half of a folder name, when it has one.
fn folder_label(folder: &str) -> Option<String> {
    let (label, hash) = folder.strip_suffix(']')?.rsplit_once(" [")?;
    (is_info_hash(hash) && !label.is_empty()).then(|| label.to_string())
}

/// Where this torrent's data is, or is about to go.
///
/// **A folder that already exists always wins**, whatever it is called. That is
/// what lets the readable name arrive without a migration: a season downloaded
/// under the old layout keeps its hash-named directory and keeps working, and
/// nothing on disk is moved by an upgrade.
/// Existing data is found in **any** root the player has ever used; new data
/// goes to the current one. That is what keeps changing the setting from
/// orphaning a season downloaded last week — nothing is moved, and the old
/// folder answers to the same info hash it always did.
fn folder_for(dirs: &Dirs, hash: &str, name: Option<&str>) -> PathBuf {
    find_folder(&dirs.roots, hash).unwrap_or_else(|| dirs.root.join(folder_name(hash, name)))
}

/// The existing folder for an info hash, found by reading the directory.
///
/// A scan rather than a lookup, and cheap enough: a torrent cache holds a
/// handful of entries, and the alternative is the index file `folder_name`
/// exists to avoid.
fn find_folder(roots: &[PathBuf], hash: &str) -> Option<PathBuf> {
    let hash = hash.to_ascii_lowercase();
    roots.iter().find_map(|dir| {
        std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
            let name = e.file_name().to_str()?.to_owned();
            (folder_hash(&name)? == hash && e.path().is_dir()).then(|| e.path())
        })
    })
}

fn stream_url(port: u16, info_hash: &str, index: usize, path: &str) -> String {
    // Only the last component goes into the URL, and only to give ffmpeg an
    // extension: the path inside a torrent may contain anything, and the
    // identity is already in the hash and the index.
    let name = path.rsplit('/').next().unwrap_or("video");
    format!(
        "http://127.0.0.1:{port}/t/{info_hash}/{index}/{}",
        urlencode(name)
    )
}

/// Percent-encode everything that is not unreserved. Small and local rather than
/// a dependency: this escapes one file name for one loopback URL. `pub(crate)`
/// because cast.rs builds its URLs the same way.
pub(crate) fn urlencode(s: &str) -> String {
    urlencode_bytes(s.as_bytes())
}

fn urlencode_bytes(s: &[u8]) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `Some(Some((start, end_inclusive)))` for a usable range, `Some(None)` for one
/// that is present but unsatisfiable, `None` for a header we do not understand
/// (which is served as the whole file, per RFC 9110).
///
/// Only the single-range `bytes=` form: that is what ffmpeg sends (and what a
/// Cast receiver sends — cast.rs shares this parser), and a multipart response
/// to a media player would be answering a question nobody asked.
pub(crate) fn parse_range(value: &str, total: u64) -> Option<Option<(u64, u64)>> {
    let spec = value.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (start, end) = spec.split_once('-')?;
    let (start, end) = (start.trim(), end.trim());

    // A suffix range ("last N bytes") — ffmpeg uses it to read a trailing index.
    if start.is_empty() {
        let n: u64 = end.parse().ok()?;
        if n == 0 || total == 0 {
            return Some(None);
        }
        let n = n.min(total);
        return Some(Some((total - n, total - 1)));
    }

    let start: u64 = start.parse().ok()?;
    if start >= total {
        return Some(None);
    }
    let end = if end.is_empty() {
        total - 1
    } else {
        end.parse::<u64>().ok()?.min(total - 1)
    };
    if end < start {
        return Some(None);
    }
    Some(Some((start, end)))
}

fn mime_for(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "mov" => "video/quicktime",
        "ts" | "m2ts" | "mts" => "video/mp2t",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "m4a" | "aac" => "audio/mp4",
        _ => "application/octet-stream",
    }
}

fn empty() -> Body {
    BoxBody::new(Empty::<Bytes>::new().map_err(|e| match e {}))
}

fn simple(status: StatusCode) -> Response<Body> {
    Response::builder().status(status).body(empty()).unwrap()
}

// ---- Commands --------------------------------------------------------------

#[tauri::command]
pub async fn torrent_add(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<TorrentService>>,
    source: String,
    seeding: bool,
    port_forward: bool,
) -> Result<TorrentInfo, String> {
    let dirs = TorrentService::download_dir(&app)?;
    service
        .inner()
        .clone()
        .add(&dirs, source, seeding, port_forward)
        .await
}

/// Returns true if a running session had to be torn down to apply this — the
/// frontend turns that into "the current torrent stopped", since it did.
#[tauri::command]
pub async fn torrent_set_seeding(
    service: tauri::State<'_, Arc<TorrentService>>,
    seeding: bool,
) -> Result<bool, String> {
    Ok(service.set_seeding(seeding).await)
}

/// Returns true if a running session had to be torn down — see `set_port_forward`.
#[tauri::command]
pub async fn torrent_set_port_forward(
    service: tauri::State<'_, Arc<TorrentService>>,
    on: bool,
) -> Result<bool, String> {
    Ok(service.set_port_forward(on).await)
}

/// What the router says about the BitTorrent port.
///
/// Asked rather than assumed: librqbit's forwarder is fire-and-forget, so
/// "the switch is on" says nothing about whether a packet ever reached the
/// router — and on a router with UPnP disabled, which is common, it never does.
#[tauri::command]
pub async fn torrent_port_status(
    service: tauri::State<'_, Arc<TorrentService>>,
    on: bool,
) -> Result<crate::upnp::PortStatus, String> {
    if !on {
        return Ok(crate::upnp::PortStatus {
            state: "off".into(),
            ..Default::default()
        });
    }
    let port = service.listen_port().await;
    if port == 0 {
        // Nothing is mapped and nothing failed: the session that would ask for
        // a mapping is built on the first torrent, and there has not been one.
        return Ok(crate::upnp::PortStatus {
            state: "no_session".into(),
            ..Default::default()
        });
    }
    Ok(crate::upnp::check(port).await)
}

#[tauri::command]
pub async fn torrent_status(
    service: tauri::State<'_, Arc<TorrentService>>,
    info_hash: String,
    index: usize,
) -> Result<TorrentStatus, String> {
    Ok(service.status(&info_hash, index).await)
}

#[tauri::command]
pub async fn torrent_buffered(
    service: tauri::State<'_, Arc<TorrentService>>,
    info_hash: String,
    index: usize,
) -> Result<Vec<(f64, f64)>, String> {
    Ok(service.buffered(&info_hash, index).await)
}

#[tauri::command]
pub async fn torrent_local_path(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<TorrentService>>,
    info_hash: String,
    index: usize,
) -> Result<Option<LocalFile>, String> {
    let dirs = TorrentService::download_dir(&app)?;
    Ok(service.local_path(&dirs, &info_hash, index).await)
}

#[tauri::command]
pub async fn torrent_prefetch(
    service: tauri::State<'_, Arc<TorrentService>>,
    info_hash: String,
    index: usize,
) -> Result<(), String> {
    service.prefetch(&info_hash, index).await
}

#[tauri::command]
pub async fn torrent_release(
    service: tauri::State<'_, Arc<TorrentService>>,
    info_hash: String,
) -> Result<(), String> {
    service.release(&info_hash).await;
    Ok(())
}

#[tauri::command]
pub async fn torrent_list(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<TorrentService>>,
) -> Result<Vec<TorrentOnDisk>, String> {
    let dirs = TorrentService::download_dir(&app)?;
    Ok(service.list(&dirs))
}

#[tauri::command]
pub async fn torrent_relocate(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<TorrentService>>,
    old_hash: String,
    new_hash: String,
) -> Result<(), String> {
    let dirs = TorrentService::download_dir(&app)?;
    service.relocate(&dirs, &old_hash, &new_hash).await
}

#[tauri::command]
pub async fn torrent_forget(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<TorrentService>>,
    path: String,
) -> Result<u64, String> {
    let dirs = TorrentService::download_dir(&app)?;
    service.forget(&dirs, &path).await
}

/// Delete everything the torrent cache holds.
///
/// Streaming writes the pieces to disk, so a few films fill a cache directory
/// the viewer never chose to fill. Offered next to the thumbnail cache in
/// settings, for the same reason.
///
/// **This used to delete every entry in the directory, files included**, which
/// was exactly right while that directory was ours alone and is a way to erase
/// somebody's film library the moment they can point it at one. It now removes
/// only what `is_ours` vouches for: a torrent folder we created, in a root we
/// know. Loose files are swept in `state` and nowhere else — in the cache
/// directory a stray file is our own leftover, and in a chosen root it is
/// theirs.
#[tauri::command]
pub async fn torrent_clear_cache(app: tauri::AppHandle) -> Result<u64, String> {
    Ok(clear_all(&TorrentService::download_dir(&app)?))
}

/// The sweep itself, split from the command so a test can point it at a
/// directory holding somebody's films and prove they survive it.
fn clear_all(dirs: &Dirs) -> u64 {
    let mut freed = 0u64;
    for dir in &dirs.roots {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let in_state = same_dir(&dirs.state, dir);
        for entry in entries.flatten() {
            let path = entry.path();
            let removed = if path.is_dir() {
                // `.meta` and `.session` are dot-named and `is_ours` refuses
                // them for the same reason `list` skips them: they are the
                // player's bookkeeping, not a torrent occupying space.
                let named = entry.file_name();
                let named = named.to_string_lossy();
                if named.starts_with('.') || !dirs.is_ours(&path) {
                    continue;
                }
                let size = dir_size(&path);
                std::fs::remove_dir_all(&path).is_ok().then_some(size)
            } else if in_state {
                let size = dir_size(&path);
                std::fs::remove_file(&path).is_ok().then_some(size)
            } else {
                None
            };
            freed += removed.unwrap_or(0);
        }
    }
    freed
}

/// Delete some episodes of a torrent and keep the rest.
#[tauri::command]
pub async fn torrent_forget_files(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<TorrentService>>,
    path: String,
    names: Vec<String>,
) -> Result<u64, String> {
    let dirs = TorrentService::download_dir(&app)?;
    service.forget_files(&dirs, &path, &names).await
}

/// Where new torrents go. `null` puts them back in the cache directory.
///
/// The previous root is remembered rather than forgotten, so a season
/// downloaded before the change stays findable and deletable — nothing is
/// moved by choosing a new one.
#[tauri::command]
pub fn torrent_set_dir(app: tauri::AppHandle, path: Option<String>) -> Result<(), String> {
    Dirs::set_root(&app, path.as_deref())
}

/// The current root and whether it is the default, for the settings row.
#[tauri::command]
pub fn torrent_dir(app: tauri::AppHandle) -> Result<(String, bool), String> {
    let dirs = Dirs::load(&app)?;
    let default = dirs.root == dirs.state;
    Ok((dirs.root.to_string_lossy().into_owned(), default))
}

/// Bytes a file actually occupies, which is **not** its length.
///
/// librqbit creates every file of a torrent at full size up front, and on a
/// filesystem with sparse files (APFS, and ext4/btrfs) only the blocks written
/// are allocated. Measured on a nine-episode season with one episode partly
/// fetched: `len()` reports **7.45 GB**, the blocks hold **142 MB**. Reporting
/// the former in a storage list would be worse than not having the list — it
/// tells the viewer to delete something to reclaim space that was never taken.
///
/// Windows keeps `len()`: NTFS needs an explicit `FSCTL_SET_SPARSE` to make a
/// file sparse and librqbit does not ask for one, so there the length *is* what
/// is allocated. Erring toward the honest figure on each platform rather than
/// one formula that is wrong on both.
#[cfg(unix)]
fn file_disk_size(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    // `blocks()` is in 512-byte units by POSIX definition, regardless of the
    // filesystem's own block size.
    meta.blocks().saturating_mul(512)
}

#[cfg(not(unix))]
fn file_disk_size(meta: &std::fs::Metadata) -> u64 {
    meta.len()
}

pub(crate) fn dir_size(path: &std::path::Path) -> u64 {
    // `symlink_metadata`: a symlink is measured as the link, never followed —
    // otherwise a link pointing out of the cache would be counted as ours, and
    // a cycle would not terminate.
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_symlink() {
        return 0;
    }
    if meta.is_file() {
        return file_disk_size(&meta);
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries.flatten().map(|e| dir_size(&e.path())).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Range header is the whole seek contract with ffmpeg: get it wrong and
    /// the picture is either garbage or the seekbar stops working, both of which
    /// look like a torrent problem rather than a parsing one.
    #[test]
    fn ranges() {
        // Plain, open-ended, and a single byte.
        assert_eq!(parse_range("bytes=0-99", 1000), Some(Some((0, 99))));
        assert_eq!(parse_range("bytes=500-", 1000), Some(Some((500, 999))));
        assert_eq!(parse_range("bytes=0-0", 1000), Some(Some((0, 0))));
        assert_eq!(parse_range(" bytes=10-20 ", 1000), Some(Some((10, 20))));

        // An end past the file is clamped rather than refused — ffmpeg asks for
        // more than exists when it probes near the tail, and RFC 9110 says the
        // server clamps.
        assert_eq!(parse_range("bytes=900-5000", 1000), Some(Some((900, 999))));

        // Suffix form: the last N bytes. This is how ffmpeg reads an MP4 moov
        // atom that sits at the end of the file, so it is the first request of
        // playback for a large class of torrents — and it is why the whole tail
        // of the file has to be reachable before any of the middle.
        assert_eq!(parse_range("bytes=-100", 1000), Some(Some((900, 999))));
        assert_eq!(parse_range("bytes=-5000", 1000), Some(Some((0, 999))));

        // Present but unsatisfiable → 416, which ffmpeg handles. Answering 200
        // with the whole file instead would have it decode from the wrong offset.
        assert_eq!(parse_range("bytes=1000-", 1000), Some(None));
        assert_eq!(parse_range("bytes=50-20", 1000), Some(None));
        assert_eq!(parse_range("bytes=-0", 1000), Some(None));

        // Not understood → None, served as the whole file.
        assert_eq!(parse_range("items=0-1", 1000), None);
        assert_eq!(parse_range("bytes=0-10,20-30", 1000), None);
        assert_eq!(parse_range("bytes=abc-", 1000), None);
    }

    /// ROADMAP 21's spike #1, as a test: stand the whole thing up against a real
    /// swarm and read bytes out of the middle of the file — which is what a seek
    /// is, and the only part of this feature that cannot be reasoned about.
    ///
    /// ```bash
    /// FP_TEST_MAGNET=1 cargo test --lib torrent::tests::sintel_smoke -- --nocapture
    /// ```
    ///
    /// Off by default: it needs the network, a live swarm and up to a minute,
    /// none of which belong in a normal `cargo test`. Sintel is the fixture the
    /// BitTorrent world uses for exactly this — a Creative Commons film, kept
    /// seeded for testing.
    ///
    /// `FP_TEST_MAGNET` also takes a **real magnet**, and `FP_TEST_FILE_INDEX`
    /// picks a file out of it, which turns this into the diagnostic for "is this
    /// torrent's file broken or is it us". With `ffprobe` on PATH it also dumps
    /// the container's own view of the stream — chapters included, which is the
    /// metadata most likely to be wrong in a rip and the hardest to attribute by
    /// eye.
    ///
    /// Its sibling `swarm_probe` answers the other half — *why* a torrent is not
    /// downloading, which this one can only report as a read that never returns.
    #[test]
    fn sintel_smoke() {
        let Ok(arg) = std::env::var("FP_TEST_MAGNET") else {
            return;
        };
        const SINTEL: &str = "magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10\
            &dn=Sintel&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce\
            &tr=udp%3A%2F%2Fexplodie.org%3A6969";
        let magnet = if arg == "1" { SINTEL.to_string() } else { arg };

        // Tauri's runtime rather than one of our own: `ensure_started` spawns
        // the server with `tauri::async_runtime::spawn`, and running the session
        // on a second runtime would test a configuration that never ships.
        tauri::async_runtime::block_on(async {
            let dir = std::env::temp_dir().join("frameplayer-torrent-smoke");
            std::fs::create_dir_all(&dir).unwrap();
            let dirs = Dirs::single(dir.clone());
            let service = Arc::new(TorrentService::default());

            let t0 = std::time::Instant::now();
            // Never seeds, matching the shipped default — a test must not
            // quietly upload to strangers.
            let info = service
                .add(&dirs, magnet, false, false)
                .await
                .expect("resolve failed");
            println!("resolved in {:?}: {:?}", t0.elapsed(), info.name);
            for f in &info.files {
                println!("  [{}] {} ({} bytes)\n      {}", f.index, f.path, f.size, f.url);
            }

            let video = match std::env::var("FP_TEST_FILE_INDEX") {
                Ok(i) => {
                    let i: usize = i.parse().expect("FP_TEST_FILE_INDEX is not a number");
                    info.files
                        .iter()
                        .find(|f| f.index == i)
                        .expect("no such file index")
                        .clone()
                }
                Err(_) => info
                    .files
                    .iter()
                    .max_by_key(|f| f.size)
                    .expect("no files")
                    .clone(),
            };
            println!("\n--- streaming [{}] {} ---", video.index, video.path);

            // Two reads that between them prove the contract: the tail (what
            // ffmpeg asks for first on an MP4, and the request that has to reach
            // the far end of a file nobody has downloaded) and a chunk from the
            // middle (a cold seek).
            // Clamped to the file: a torrent's small files (Sintel ships ten
            // subtitles of ~1.5 KB) are what make the "already complete" path
            // cheap to exercise, and a fixed 64 KB would simply fail on them.
            let chunk = (64 * 1024u64).min(video.size);
            for (label, range) in [
                ("tail", format!("bytes=-{chunk}")),
                ("middle", {
                    let mid = (video.size / 2).min(video.size - chunk);
                    format!("bytes={}-{}", mid, mid + chunk - 1)
                }),
            ] {
                let t = std::time::Instant::now();
                let res = reqwest::Client::new()
                    .get(&video.url)
                    .header("Range", &range)
                    .send()
                    .await
                    .expect("request failed");
                assert_eq!(res.status().as_u16(), 206, "{label}: expected 206");
                let bytes = res.bytes().await.expect("body failed");
                println!("{label} ({range}): {} bytes in {:?}", bytes.len(), t.elapsed());
                assert_eq!(bytes.len() as u64, chunk, "{label}: short read");
            }

            // The folder is the info hash, and `list` is what the start screen
            // reads. Checking both here is what proves the storage UI is looking
            // at the same thing the streaming code wrote.
            let listed = service.list(&dirs);
            println!("\n--- torrent_list ---");
            for row in &listed {
                println!("  {} hash={:?} size={}", row.folder, row.info_hash, row.size);
            }
            assert!(
                listed
                    .iter()
                    .any(|r| r.info_hash.as_deref() == Some(info.info_hash.as_str())),
                "the streaming torrent is missing from torrent_list"
            );

            let status = service.status(&info.info_hash, video.index).await;
            println!(
                "state={} peers={} down={:.1} KB/s file={}/{}",
                status.state,
                status.peers,
                status.down_bps / 1024.0,
                status.file_done,
                status.file_size
            );

            // What the container itself says, read through the very server mpv
            // uses. This is the half that attributes a complaint: if a chapter
            // list is wrong *here*, it is wrong in the file, because ffprobe and
            // mpv read it with the same libavformat over the same bytes.
            // `FP_TEST_PROBE=all` walks every file instead — which is what
            // answers "which episode of this season is the broken one", a
            // question that cannot be asked of a single file at a time.
            let probe = std::env::var("FP_TEST_PROBE").unwrap_or_default();
            let targets: Vec<&TorrentFile> = match probe.as_str() {
                "" => vec![],
                "all" => info.files.iter().collect(),
                _ => vec![&video],
            };
            for f in targets {
                println!("\n--- ffprobe [{}] {} ---", f.index, f.path);
                let out = std::process::Command::new("ffprobe")
                    .args([
                        "-v", "error",
                        "-show_entries",
                        "format=duration,start_time:chapter=start_time,end_time",
                        "-of", "compact=p=1:nk=0",
                        &f.url,
                    ])
                    .output();
                match out {
                    Ok(o) => {
                        print!("{}", String::from_utf8_lossy(&o.stdout));
                        let err = String::from_utf8_lossy(&o.stderr);
                        if !err.trim().is_empty() {
                            println!("stderr: {}", err.trim());
                        }
                    }
                    Err(e) => println!("ffprobe unavailable: {e}"),
                }
            }
        });
    }

    /// Watch a swarm for a minute: peers found, peers connected, rate, pieces.
    ///
    /// ```bash
    /// FP_TEST_SWARM='magnet:?xt=…' cargo test --lib torrent::tests::swarm_probe -- --nocapture
    /// ```
    ///
    /// `sintel_smoke` proves a torrent streams; this one exists for when it does
    /// not, because the two things that answer "why" — how many peers the tracker
    /// and the DHT actually produced, and how many of them we are talking to —
    /// are invisible from a read that simply never returns. A swarm the tracker
    /// says has twenty seeders and a client that sits at one connected peer is a
    /// different problem from an empty swarm, and they look identical in the UI.
    ///
    /// Off by default and network-bound, like `sintel_smoke`.
    #[test]
    fn swarm_probe() {
        let Ok(magnet) = std::env::var("FP_TEST_SWARM") else {
            return;
        };
        tauri::async_runtime::block_on(async {
            let dir = std::env::temp_dir().join("frameplayer-swarm-probe");
            std::fs::create_dir_all(&dir).unwrap();
            let dirs = Dirs::single(dir.clone());
            let service = Arc::new(TorrentService::default());

            let info = service
                .add(&dirs, magnet, false, false)
                .await
                .expect("resolve failed");
            let file = info
                .files
                .iter()
                .max_by_key(|f| f.size)
                .expect("no files")
                .clone();
            println!(
                "{:?}\n  [{}] {} ({} bytes)",
                info.name, file.index, file.path, file.size
            );

            // Started the way playback starts it: one Range request for the head,
            // left running. Selecting the file is the request's own doing.
            let url = file.url.clone();
            tauri::async_runtime::spawn(async move {
                match reqwest::Client::new()
                    .get(&url)
                    .header("Range", "bytes=0-1048575")
                    .send()
                    .await
                {
                    Ok(r) => {
                        let status = r.status();
                        let n = r.bytes().await.map(|b| b.len()).unwrap_or(0);
                        println!("  [read] {status} {n} bytes");
                    }
                    Err(e) => println!("  [read] failed: {e}"),
                }
            });

            let (session, handle) = {
                let inner = service.inner.lock().await;
                (
                    inner.session.clone().unwrap(),
                    inner.torrents[&info.info_hash].handle.clone(),
                )
            };
            let api = librqbit::Api::new(session, None);
            let id = librqbit::api::TorrentIdOrHash::Id(handle.id());

            for i in 0..60 {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let s = service.status(&info.info_hash, file.index).await;
                let bands = service.buffered(&info.info_hash, file.index).await;
                let agg = handle
                    .stats()
                    .live
                    .map(|l| {
                        let p = l.snapshot.peer_stats;
                        format!(
                            "queued={} connecting={} live={} dead={} not_needed={}",
                            p.queued, p.connecting, p.live, p.dead, p.not_needed
                        )
                    })
                    .unwrap_or_default();
                println!(
                    "{i:>3}s state={:<12} seen={:<4} down={:>8.1} KB/s file={}/{} bands={} {agg} err={:?}",
                    s.state,
                    s.peers_seen,
                    s.down_bps / 1024.0,
                    s.file_done,
                    s.file_size,
                    bands.len(),
                    s.error
                );
            }

            // Per-peer counters: which of the seen peers were tried, how often
            // they errored, and whether any chunk ever arrived from them.
            // `PeerStatsFilter` is not exported, but it is `Deserialize` and
            // the call site pins the type — so it is built from JSON rather than
            // named. The alternative is no per-peer view at all.
            let filter = serde_json::from_str(r#"{"state":"All"}"#).unwrap();
            if let Ok(snap) = api.api_peer_stats(id, filter) {
                println!("\n--- per peer ---");
                for (addr, st) in &snap.peers {
                    let c = &st.counters;
                    println!(
                        "  {addr:<24} {:<12} attempts={} conns={} errors={} chunks={} bytes={}",
                        st.state,
                        c.connection_attempts,
                        c.connections,
                        c.errors,
                        c.fetched_chunks,
                        c.fetched_bytes
                    );
                }
            }
        });
    }

    /// The one field wanted out of an announce response, and the reason it is
    /// scanned rather than deserialized: a strict parser is exactly what made
    /// librqbit throw whole responses away (see vendor/README.md). A wrong
    /// answer here is silent — no peers, and a magnet that never resolves.
    #[test]
    fn announce_peers_parsing() {
        // Verbatim shape from a real tracker: no `complete`, no `incomplete`.
        let mut real = b"d8:intervali3595e12:min intervali3595e5:peers12:".to_vec();
        real.extend_from_slice(&[93, 100, 177, 140, 0x7F, 0xA1]);
        real.extend_from_slice(&[5, 77, 195, 179, 0xA7, 0x30]);
        real.push(b'e');
        assert_eq!(
            compact_peers(&real),
            vec![
                "93.100.177.140:32673".parse::<SocketAddr>().unwrap(),
                "5.77.195.179:42800".parse::<SocketAddr>().unwrap(),
            ]
        );

        // A port of zero is not somewhere anything can be reached.
        let mut zero = b"d5:peers6:".to_vec();
        zero.extend_from_slice(&[1, 2, 3, 4, 0, 0]);
        zero.push(b'e');
        assert_eq!(compact_peers(&zero), Vec::<SocketAddr>::new());

        // Everything that is not a compact list degrades to "no peers" rather
        // than to garbage addresses: the dictionary form, a truncated body, a
        // length that is not a whole number of records, and no field at all.
        assert!(compact_peers(b"d5:peersld2:ip9:127.0.0.1eee").is_empty());
        assert!(compact_peers(b"d5:peers12:abc").is_empty());
        assert!(compact_peers(b"d5:peers7:abcdefge").is_empty());
        assert!(compact_peers(b"d8:intervali60ee").is_empty());
        assert!(compact_peers(b"").is_empty());
    }

    /// The info hash reaches the announce as raw bytes, and half of them are
    /// not printable. Getting the decode wrong asks the tracker about a
    /// different torrent, which answers cheerfully with nothing.
    #[test]
    fn info_hash_bytes() {
        assert_eq!(
            hex_bytes("378032034812493fd0e8a83e746323800a24078f").unwrap(),
            vec![
                0x37, 0x80, 0x32, 0x03, 0x48, 0x12, 0x49, 0x3F, 0xD0, 0xE8, 0xA8, 0x3E, 0x74,
                0x63, 0x23, 0x80, 0x0A, 0x24, 0x07, 0x8F
            ]
        );
        // Upper case is the same torrent; anything that is not a hash is none.
        assert_eq!(
            hex_bytes("4C0DD90150D41A5B647AA78EA828B2942C43AF45"),
            hex_bytes("4c0dd90150d41a5b647aa78ea828b2942c43af45")
        );
        assert!(hex_bytes("").is_none());
        assert!(hex_bytes("not a hash").is_none());
        assert!(hex_bytes("378032034812493fd0e8a83e746323800a24078").is_none());

        // And the bytes have to survive being put in a URL: `%3F` is a `?` in
        // the middle of an info hash, and unescaped it would truncate the query.
        assert_eq!(
            urlencode_bytes(&hex_bytes("378032034812493fd0e8a83e746323800a24078f").unwrap()),
            "7%802%03H%12I%3F%D0%E8%A8%3Etc%23%80%0A%24%07%8F"
        );
    }

    /// Which directory names count as ours. The folder is the info hash (see
    /// `add`), and this is also what `torrent_forget` validates against, so it
    /// must not accept anything a torrent could be named.
    #[test]
    fn info_hash_folders() {
        assert!(is_info_hash("08ada5a7a6183aae1e09d831df6748d566095a10"));
        assert!(is_info_hash("0AEA879D465BF5169AE9AC370C33B69834713D92"));
        // 39 and 41 characters, and a non-hex letter in the right place.
        assert!(!is_info_hash("08ada5a7a6183aae1e09d831df6748d566095a1"));
        assert!(!is_info_hash("08ada5a7a6183aae1e09d831df6748d566095a100"));
        assert!(!is_info_hash("08ada5a7a6183aae1e09d831df6748d566095g10"));
        // Real torrent names, which must never be mistaken for ours.
        assert!(!is_info_hash("Dutton.Ranch.S01.2026.WEB-DLRip-AVC.x264.se"));
        assert!(!is_info_hash(""));
        assert!(!is_info_hash(".."));
    }

    /// A folder names its torrent **and** identifies it, and this is the pair
    /// of functions that has to agree about the second half. A name that does
    /// not round-trip is a torrent whose data cannot be found — the folder is
    /// still there, but nothing maps it back to an info hash, so it reads as a
    /// leftover from a layout nobody uses and offers only to be deleted.
    #[test]
    fn folders_name_their_torrent_and_still_identify_it() {
        const HASH: &str = "08ada5a7a6183aae1e09d831df6748d566095a10";

        // Both layouts answer, and the old one keeps working with no migration.
        assert_eq!(folder_hash(HASH).as_deref(), Some(HASH));
        assert_eq!(
            folder_hash("Dutton.Ranch.S01 [08ADA5A7A6183AAE1E09D831DF6748D566095A10]").as_deref(),
            Some(HASH),
            "the hash is the identity and its case is not part of it"
        );

        // Anything that is not ours must not be read as ours: `forget` deletes
        // by this answer, and `find_folder` hands data to whoever asks.
        assert_eq!(folder_hash("Dutton.Ranch.S01.2026.WEB-DLRip"), None);
        assert_eq!(folder_hash(""), None);
        assert_eq!(folder_hash(".."), None);
        // Brackets a *name* could carry, with nothing that parses inside them.
        assert_eq!(folder_hash("Some Show [1080p]"), None);
        assert_eq!(folder_hash("[08ada5a7a6183aae1e09d831df6748d566095a10]"), None);

        for name in [
            "Dutton.Ranch.S01.2026.WEB-DLRip-AVC.x264",
            // Every character Windows forbids, plus the separators, plus our
            // own brackets — none of which may reach a path.
            "A<B>C:D\"E/F\\G|H?I*J [K]",
            // A trailing dot is dropped by Win32 after the folder is created,
            // so the path we build afterwards would miss it.
            "Show Name...",
            // A reserved device name is not a legal folder in any directory.
            "NUL",
            "com1.2026",
            // Long, and not ASCII: a byte-wise cut here is invalid UTF-8.
            &"Сезон первый в очень длинном названии ".repeat(4),
        ] {
            let folder = folder_name(HASH, Some(name));
            assert_eq!(
                folder_hash(&folder).as_deref(),
                Some(HASH),
                "{name:?} produced {folder:?}, which does not identify its torrent"
            );
            let stem = folder.strip_suffix(&format!(" [{HASH}]")).unwrap();
            assert!(
                !stem.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*', '[', ']']),
                "{folder:?} carries a character that cannot be in a path"
            );
            assert!(!stem.ends_with(['.', ' ']), "{folder:?} ends in a dropped character");
            assert!(stem.chars().count() <= 81, "{folder:?} is not capped");
            for reserved in ["CON", "PRN", "AUX", "NUL", "COM1"] {
                assert!(
                    !stem.split('.').next().unwrap().eq_ignore_ascii_case(reserved),
                    "{folder:?} is a reserved device name"
                );
            }
        }

        // Nothing to name it with — a `.torrent` URL, or metadata that will not
        // parse — falls back to the layout that needs no name at all.
        assert_eq!(folder_name(HASH, None), HASH);
        assert_eq!(folder_name(HASH, Some("   ")), HASH);
    }

    /// **The viewer points the player at their film library, and nothing of
    /// theirs is listed, measured or deleted.**
    ///
    /// This is the test the whole chosen-root feature exists behind. Every
    /// destructive path in the module is aimed at a directory that is now
    /// somebody's media drive, and each of them is asked here to leave alone a
    /// folder it did not create — including the sweep that used to remove every
    /// entry it found, which was correct while the directory was ours alone and
    /// is a way to erase a film collection the moment it is not.
    #[test]
    fn a_chosen_root_protects_what_it_did_not_create() {
        const HASH: &str = "08ada5a7a6183aae1e09d831df6748d566095a10";
        let base = std::env::temp_dir().join("frameplayer-chosen-root");
        let _ = std::fs::remove_dir_all(&base);

        let state = base.join("cache");
        let library = base.join("Films");
        // Ours, in the chosen root, exactly as `add` would have written it.
        let ours = library.join(format!("Season [{HASH}]"));
        // Theirs: a folder, and a loose file beside it.
        let theirs = library.join("Holiday 2019");
        let their_file = library.join("notes.txt");
        // A folder from the older name-based layout, which lives in the state
        // directory and must stay deletable there.
        let legacy = state.join("Some.Release.2024.WEB-DL");
        for d in [&state, &library, &ours, &theirs, &legacy] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(ours.join("ep1.mkv"), b"ours").unwrap();
        std::fs::write(theirs.join("beach.mp4"), b"theirs").unwrap();
        std::fs::write(&their_file, b"theirs").unwrap();
        std::fs::write(legacy.join("film.mkv"), b"legacy").unwrap();

        let dirs = Dirs {
            state: state.clone(),
            root: library.clone(),
            roots: vec![library.clone(), state.clone()],
        };
        let service = Arc::new(TorrentService::default());

        // Listing: ours from the chosen root, the legacy folder from the state
        // directory, and nothing of theirs — not even measured, since walking a
        // media drive to print a number is its own kind of rude.
        let listed = service.list(&dirs);
        let folders: Vec<&str> = listed.iter().map(|r| r.folder.as_str()).collect();
        assert!(folders.contains(&format!("Season [{HASH}]").as_str()));
        assert!(folders.contains(&"Some.Release.2024.WEB-DL"));
        assert!(!folders.contains(&"Holiday 2019"), "listed a folder we did not create");
        assert_eq!(folders.len(), 2, "listed something unexpected: {folders:?}");

        // Deleting: refused for theirs, and the data is still there afterwards.
        let forget = |p: &std::path::Path| {
            tauri::async_runtime::block_on(service.forget(&dirs, &p.to_string_lossy()))
        };
        assert!(forget(&theirs).is_err(), "offered to delete a folder we did not create");
        assert!(theirs.join("beach.mp4").is_file());
        // Not a root of ours at all, and a traversal that lands inside one.
        assert!(forget(&base).is_err());
        assert!(forget(&library.join("..").join("Films").join("Holiday 2019")).is_err());
        assert!(theirs.is_dir());

        // The sweep: same rule, and the loose file in a chosen root is theirs
        // too. Only the state directory is ours to tidy file by file.
        clear_all(&dirs);
        assert!(theirs.join("beach.mp4").is_file(), "cleared a folder we did not create");
        assert!(their_file.is_file(), "cleared a file we did not write");
        assert!(!ours.exists(), "did not clear our own torrent");
        assert!(!legacy.exists(), "did not clear the older layout in our own directory");

        let _ = std::fs::remove_dir_all(&base);
    }

    /// **Deleting the episodes you have watched keeps the ones you have not**,
    /// and keeps the torrent openable afterwards — which is the whole point of
    /// pruning rather than forgetting.
    #[test]
    fn watched_episodes_go_and_the_rest_stays() {
        const HASH: &str = "08ada5a7a6183aae1e09d831df6748d566095a10";
        let base = std::env::temp_dir().join("frameplayer-prune-files");
        let _ = std::fs::remove_dir_all(&base);

        let folder = base.join(format!("Season [{HASH}]"));
        let extras = folder.join("extras");
        std::fs::create_dir_all(&extras).unwrap();
        for (path, bytes) in [
            (folder.join("ep1.mkv"), &b"watched"[..]),
            (folder.join("ep2.mkv"), &b"watched"[..]),
            (folder.join("ep3.mkv"), &b"not yet"[..]),
            // In a subfolder, to prove the walk reaches it and then takes the
            // empty directory with it.
            (extras.join("ep0.mkv"), &b"watched"[..]),
        ] {
            std::fs::write(path, bytes).unwrap();
        }
        // Ours, and it must survive: the torrent is being pruned, not forgotten,
        // and this is what keeps reopening it free.
        let meta = meta_path(&base, HASH);
        std::fs::create_dir_all(meta.parent().unwrap()).unwrap();
        std::fs::write(&meta, b"metadata").unwrap();

        let dirs = Dirs::single(base.clone());
        let service = Arc::new(TorrentService::default());
        let names = ["ep1.mkv".to_string(), "ep2.mkv".to_string(), "ep0.mkv".to_string()];
        let freed = tauri::async_runtime::block_on(service.forget_files(
            &dirs,
            &folder.to_string_lossy(),
            &names,
        ))
        .unwrap();

        assert!(!folder.join("ep1.mkv").exists());
        assert!(!folder.join("ep2.mkv").exists());
        assert!(folder.join("ep3.mkv").is_file(), "deleted an episode nobody watched");
        assert!(!extras.exists(), "left an empty directory behind");
        assert!(folder.is_dir(), "removed the torrent instead of pruning it");
        assert!(meta.is_file(), "threw away the metadata that makes reopening free");
        assert!(freed > 0);

        // The same guards as `forget`: a folder we did not create, and a
        // traversal, are refused before anything is unlinked.
        let outside = base.join("Their Films");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("ep1.mkv"), b"theirs").unwrap();
        let dirs = Dirs {
            state: base.join("cache"),
            root: base.clone(),
            roots: vec![base.clone()],
        };
        std::fs::create_dir_all(&dirs.state).unwrap();
        let refused = tauri::async_runtime::block_on(service.forget_files(
            &dirs,
            &outside.to_string_lossy(),
            &names,
        ));
        assert!(refused.is_err(), "pruned a folder we did not create");
        assert!(outside.join("ep1.mkv").is_file());

        let _ = std::fs::remove_dir_all(&base);
    }

    /// `torrent_forget` deletes a directory by name, so the name is the only
    /// thing standing between it and the rest of the disk. Escapes are refused
    /// before anything is touched.
    #[test]
    fn forget_refuses_escapes() {
        let base = std::env::temp_dir().join("frameplayer-forget-guard");
        std::fs::create_dir_all(base.join("victim")).unwrap();
        let service = Arc::new(TorrentService::default());

        for bad in [
            "",
            "..",
            "../victim",
            "../../etc",
            "a/b",
            "a\\b",
            ".hidden",
            "sub/../../victim",
        ] {
            let r = tauri::async_runtime::block_on(service.forget(&Dirs::single(base.clone()), &base.join(bad).to_string_lossy()));
            assert!(r.is_err(), "{bad:?} should have been refused");
        }
        // The guard is not merely returning errors — nothing was removed.
        assert!(base.join("victim").is_dir());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// **Opening a torrent must not cost the weight of the torrent.**
    ///
    /// librqbit stretches every file to its full length at initialization
    /// unless told which ones are wanted, and on NTFS — where a file is not
    /// sparse unless somebody asks — that allocation is real: a season occupied
    /// all of itself while two episodes had been fetched. `only_files:
    /// Some(vec![])` at `add` is what prevents it, and this is the assertion
    /// that fails if anybody removes it.
    ///
    /// Length rather than allocated blocks, deliberately: `set_len` is what the
    /// change is about, and a length is the same fact on every filesystem,
    /// while blocks are free on APFS and would make this pass on the platform
    /// that never had the problem.
    #[test]
    fn an_unopened_torrent_allocates_nothing() {
        tauri::async_runtime::block_on(async {
            let base = std::env::temp_dir().join("frameplayer-prealloc-torrent");
            let _ = std::fs::remove_dir_all(&base);
            let stage = base.join("stage");
            std::fs::create_dir_all(&stage).unwrap();

            let payload: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
            std::fs::write(stage.join("ep1.mkv"), &payload).unwrap();
            std::fs::write(stage.join("ep2.mkv"), &payload).unwrap();

            let created = librqbit::create_torrent(
                &stage,
                librqbit::CreateTorrentOptions {
                    name: Some("season"),
                    piece_length: Some(32 * 1024),
                },
            )
            .await
            .unwrap();
            let hash = created.info_hash().as_string();

            // The metadata is cached, so nothing has to be resolved; the data is
            // thrown away, so this is a torrent nobody has downloaded yet.
            let meta = meta_path(&base, &hash);
            std::fs::create_dir_all(meta.parent().unwrap()).unwrap();
            std::fs::write(&meta, created.as_bytes().unwrap()).unwrap();
            std::fs::remove_dir_all(&stage).unwrap();

            let service = Arc::new(TorrentService::default());
            let info = service
                .add(&Dirs::single(base.clone()), format!("magnet:?xt=urn:btih:{hash}"), false, false)
                .await
                .expect("add failed");
            assert_eq!(info.files.len(), 2);

            // **`add` returns while the torrent is still `Initializing`**, and
            // the preallocation this test is about happens at the *end* of that
            // phase — so checking the files straight after `add` finds them
            // zero-length whatever `only_files` says, and the test passes
            // without the code it exists to pin. (Established by removing the
            // fix and watching it stay green.) Waiting is the whole assertion.
            let handle = {
                let inner = service.inner.lock().await;
                inner.torrents[&info.info_hash].handle.clone()
            };
            service.wait_ready(&handle).await.expect("never initialized");

            for file in &info.files {
                let path = folder_for(&Dirs::single(base.clone()), &hash, None).join(&file.path);
                let meta = std::fs::metadata(&path)
                    .unwrap_or_else(|e| panic!("no file at {}: {e}", path.display()));
                assert_eq!(
                    meta.len(),
                    0,
                    "{} was preallocated to {} bytes — only_files is not empty at add",
                    file.path,
                    meta.len()
                );
            }

            let _ = std::fs::remove_dir_all(&base);
        });
    }

    /// **A file already on disk is served without touching the swarm**, proved
    /// offline: a torrent built here from a local file, its metadata put in the
    /// cache, and the data already sitting in the folder the info hash names.
    ///
    /// Runs in a normal `cargo test` precisely because it needs no network —
    /// which is the point. The two claims it pins down are the ones that make a
    /// rewatch instant, and both are easy to break by reordering `select`:
    /// nothing is downloaded (the torrent never leaves `paused`, so no peer is
    /// ever contacted) and the bytes served are the right ones.
    #[test]
    fn complete_file_needs_no_swarm() {
        tauri::async_runtime::block_on(async {
            let base = std::env::temp_dir().join("frameplayer-offline-torrent");
            let _ = std::fs::remove_dir_all(&base);
            let stage = base.join("stage");
            std::fs::create_dir_all(&stage).unwrap();

            // Deterministic, and larger than one piece so the read spans a few.
            let payload: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
            std::fs::write(stage.join("clip.mkv"), &payload).unwrap();

            let created = librqbit::create_torrent(
                &stage,
                librqbit::CreateTorrentOptions {
                    name: Some("offline"),
                    piece_length: Some(32 * 1024),
                },
            )
            .await
            .unwrap();
            let hash = created.info_hash().as_string();

            // Put the data where `add` will look for it, and the metadata where
            // the cache lives — together these stand in for "watched yesterday".
            std::fs::rename(&stage, base.join(&hash)).unwrap();
            let meta = meta_path(&base, &hash);
            std::fs::create_dir_all(meta.parent().unwrap()).unwrap();
            std::fs::write(&meta, created.as_bytes().unwrap()).unwrap();

            let service = Arc::new(TorrentService::default());
            // A magnet with no trackers: if anything reached for the network,
            // there is nowhere for it to go and this would hang rather than pass.
            let info = service
                .add(&Dirs::single(base.clone()), format!("magnet:?xt=urn:btih:{hash}"), false, false)
                .await
                .expect("add failed");
            assert_eq!(info.files.len(), 1);

            let file = &info.files[0];
            let res = reqwest::Client::new()
                .get(&file.url)
                .header("Range", "bytes=100000-100999")
                .send()
                .await
                .expect("request failed");
            assert_eq!(res.status().as_u16(), 206);
            let body = res.bytes().await.unwrap();
            assert_eq!(
                &body[..],
                &payload[100_000..101_000],
                "served the wrong bytes"
            );

            // A complete file is an ordinary file on disk, which is what makes
            // seekbar previews possible for a torrent at all.
            let local = service
                .local_path(&Dirs::single(base.clone()), &info.info_hash, file.index)
                .await
                .expect("local_path found nothing");
            // **Staged under the old layout and found under the new one.** The
            // folder went in as a bare info hash — which is what a season
            // downloaded by an earlier build looks like — and `add` renamed it
            // the moment it could read the torrent's name. So this asserts the
            // migration as well as the lookup, and it is spelled out rather
            // than resolved with `folder_for`, which would agree with whatever
            // the code did.
            assert_eq!(
                local.path,
                base.join(format!("offline [{hash}]"))
                    .join("clip.mkv")
                    .to_string_lossy()
            );
            assert!(local.complete, "a fully seeded file reported incomplete");

            // The load-bearing assertion: still paused, so `select` never
            // unpaused it and no peer was ever contacted for a file we had.
            let status = service.status(&info.info_hash, file.index).await;
            assert_eq!(status.state, "paused", "a complete file started the swarm");
            assert_eq!(status.peers, 0);
            assert_eq!(status.file_done, payload.len() as u64);

            let _ = std::fs::remove_dir_all(&base);
        });
    }

    /// The debug string `api_dump_haves` hands back. Pinned by a test because
    /// it is a third-party `Debug` impl rather than a documented format — if a
    /// bitvec upgrade changes it, this fails instead of the seekbar quietly
    /// losing its buffer map.
    #[test]
    fn haves_parsing() {
        // Verbatim from a real run (see the module doc on `buffered`).
        let real = "BitSlice<u8, bitvec::order::Msb0> { addr: 0x10a9ee760, head: 000,                     bits: 10 } [1, 0, 1, 1, 0, 0, 0, 1, 1, 0]";
        assert_eq!(
            parse_haves(real),
            vec![true, false, true, true, false, false, false, true, true, false]
        );
        // Degrades to "nothing is buffered" rather than to a wrong map.
        assert_eq!(parse_haves("nonsense"), Vec::<bool>::new());
        assert_eq!(parse_haves(""), Vec::<bool>::new());
    }

    /// Pieces cover the whole torrent, so a file's first and last are usually
    /// shared with its neighbours. Getting that intersection wrong would draw
    /// buffer that belongs to another file — and, worse, let a thumbnail be
    /// decoded from a hole.
    #[test]
    fn buffered_ranges() {
        // 10-byte pieces. The file sits at 15..35, so it spans pieces 1..=3 and
        // owns only half of piece 1 and half of piece 3.
        let (piece, off, len) = (10u64, 15u64, 20u64);

        // Nothing.
        assert_eq!(file_ranges(&[false; 4], piece, off, len), vec![]);

        // Only piece 2 — the middle of the file, and neither end.
        let r = file_ranges(&[false, false, true, false], piece, off, len);
        assert_eq!(r.len(), 1);
        assert!((r[0].0 - 0.25).abs() < 1e-9, "{r:?}");
        assert!((r[0].1 - 0.75).abs() < 1e-9, "{r:?}");

        // Pieces 1 and 2 are adjacent and must merge into one band, clamped to
        // the start of the file rather than the start of piece 1.
        let r = file_ranges(&[false, true, true, false], piece, off, len);
        assert_eq!(r.len(), 1, "adjacent pieces did not merge: {r:?}");
        assert!((r[0].0).abs() < 1e-9, "{r:?}");
        assert!((r[0].1 - 0.75).abs() < 1e-9, "{r:?}");

        // A gap stays a gap.
        let r = file_ranges(&[false, true, false, true], piece, off, len);
        assert_eq!(r.len(), 2, "{r:?}");

        // Everything: exactly 0..1, not past the end of the file.
        let r = file_ranges(&[true; 4], piece, off, len);
        assert_eq!(r.len(), 1);
        assert!((r[0].0).abs() < 1e-9 && (r[0].1 - 1.0).abs() < 1e-9, "{r:?}");

        // Degenerate inputs must not panic or divide by zero.
        assert_eq!(file_ranges(&[true], 0, 0, 10), vec![]);
        assert_eq!(file_ranges(&[true], 10, 0, 0), vec![]);
    }

    /// **A restored torrent must come back still, and must not be re-hashed.**
    ///
    /// Both halves of enabling `fastresume`, pinned offline. The first is the
    /// one risk persistence carries: measured, a torrent that was *live* when
    /// the session ended is restored live and would start talking to peers for
    /// something nobody asked to watch. The second is the whole reason for it —
    /// without the resume data librqbit re-verifies the entire torrent on every
    /// open, holes included.
    #[test]
    fn restored_torrents_start_paused() {
        tauri::async_runtime::block_on(async {
            let base = std::env::temp_dir().join("frameplayer-resume-test");
            let _ = std::fs::remove_dir_all(&base);
            let stage = base.join("stage");
            std::fs::create_dir_all(&stage).unwrap();
            let payload: Vec<u8> = (0..2_000_000u32).map(|i| (i % 251) as u8).collect();
            std::fs::write(stage.join("clip.mkv"), &payload).unwrap();
            let created = librqbit::create_torrent(
                &stage,
                librqbit::CreateTorrentOptions { name: Some("resume"), piece_length: Some(65536) },
            )
            .await
            .unwrap();
            let hash = created.info_hash().as_string();
            std::fs::rename(&stage, base.join(&hash)).unwrap();
            let meta = meta_path(&base, &hash);
            std::fs::create_dir_all(meta.parent().unwrap()).unwrap();
            std::fs::write(&meta, created.as_bytes().unwrap()).unwrap();
            let magnet = format!("magnet:?xt=urn:btih:{hash}");

            // First run: just add it, so the store is seeded.
            let first = Arc::new(TorrentService::default());
            let info = first.add(&Dirs::single(base.clone()), magnet.clone(), false, false).await.unwrap();
            first.shutdown_session().await;

            // Then say it was RUNNING when the app closed. Writing that into the
            // store is stricter than trying to get a torrent live offline, and
            // it is the state that matters: librqbit honours the flag, so
            // without `pause_restored` this comes back live and starts talking
            // to peers for something nobody asked to watch.
            let store = base.join(SESSION_DIR);
            let mut patched = false;
            for entry in std::fs::read_dir(&store).unwrap().flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                if text.contains("\"is_paused\":true") {
                    std::fs::write(&path, text.replace("\"is_paused\":true", "\"is_paused\":false"))
                        .unwrap();
                    patched = true;
                }
            }
            assert!(patched, "the session store did not record a paused flag");

            // Second run: a fresh service over the same store.
            let second = Arc::new(TorrentService::default());
            second.add(&Dirs::single(base.clone()), magnet, false, false).await.unwrap();
            let status = second.status(&info.info_hash, 0).await;
            assert_eq!(
                status.state, "paused",
                "a torrent recorded as running came back running"
            );
            assert_eq!(status.peers, 0);
            // And the resume data survived, so it knows it holds the file
            // without having hashed a byte of it this time.
            assert_eq!(status.file_done, payload.len() as u64);
            second.shutdown_session().await;

            let _ = std::fs::remove_dir_all(&base);
        });
    }

    /// A deleted torrent must not come back — and it did, once per restart.
    ///
    /// The whole failure in one test, offline: seed the store by adding a
    /// torrent, delete its data the way `forget` does when there is no session
    /// to delete from, then build another session over the same store. Without
    /// `prune_orphaned_store` the restore recreates the folder and every file in
    /// it, which the start screen then lists as a nameless zero-byte torrent
    /// with no magnet — the frontend dropped the magnet when the viewer deleted
    /// it, and it is not coming back.
    #[test]
    fn deleted_torrent_does_not_come_back() {
        tauri::async_runtime::block_on(async {
            let base = std::env::temp_dir().join("frameplayer-ghost-test");
            let _ = std::fs::remove_dir_all(&base);
            let stage = base.join("stage");
            std::fs::create_dir_all(&stage).unwrap();
            std::fs::write(stage.join("clip.mkv"), vec![7u8; 300_000]).unwrap();
            let created = librqbit::create_torrent(
                &stage,
                librqbit::CreateTorrentOptions {
                    name: Some("ghost"),
                    piece_length: Some(65536),
                },
            )
            .await
            .unwrap();
            let hash = created.info_hash().as_string();
            std::fs::rename(&stage, base.join(&hash)).unwrap();
            let meta = meta_path(&base, &hash);
            std::fs::create_dir_all(meta.parent().unwrap()).unwrap();
            std::fs::write(&meta, created.as_bytes().unwrap()).unwrap();
            let magnet = format!("magnet:?xt=urn:btih:{hash}");

            // First run: opened once, which is what puts it in the store.
            let first = Arc::new(TorrentService::default());
            first.add(&Dirs::single(base.clone()), magnet.clone(), false, false).await.unwrap();
            first.shutdown_session().await;
            let store = base.join(SESSION_DIR).join("session.json");
            assert!(
                std::fs::read_to_string(&store).unwrap().contains(&hash),
                "the store did not record the torrent, so this proves nothing"
            );

            // Deleted from the start screen in a later run, where the session
            // had never been built — so `forget` finds no session and only the
            // directory goes.
            let second = Arc::new(TorrentService::default());
            // By the folder's own name, exactly as the start screen passes it
            // back from `list` — which by now is the readable one, since the
            // open above renamed it.
            let folder = find_folder(std::slice::from_ref(&base), &hash).expect("no folder for the torrent");
            let folder = folder.file_name().unwrap().to_string_lossy().into_owned();
            second.forget(&Dirs::single(base.clone()), &base.join(&folder).to_string_lossy()).await.unwrap();
            assert!(find_folder(std::slice::from_ref(&base), &hash).is_none());
            assert!(!meta.exists(), "the cached metadata outlived the torrent");

            // The next session is where it used to reappear.
            let third = Arc::new(TorrentService::default());
            third.ensure_started(&Dirs::single(base.clone()), false, false).await.unwrap();
            assert!(
                find_folder(std::slice::from_ref(&base), &hash).is_none(),
                "the deleted torrent was restored and its folder recreated"
            );
            assert!(
                !std::fs::read_to_string(&store).unwrap().contains(&hash),
                "the deleted torrent is still in the session store"
            );
            assert!(!base.join(SESSION_DIR).join(format!("{hash}.bitv")).exists());
            third.shutdown_session().await;

            let _ = std::fs::remove_dir_all(&base);
        });
    }

    /// A torrent on this disk names itself, with no record on our side.
    ///
    /// What the start screen used to show for one it had not opened *itself* was
    /// "Unnamed torrent · No link saved", and the row's button was dead — even
    /// though the folder is the info hash and the metadata is cached right
    /// beside it. That is not a rare state: the record lives in localStorage,
    /// which is per webview, while this directory is per identifier, so two
    /// builds of the player share the data and not the names.
    #[test]
    fn disk_names_itself() {
        tauri::async_runtime::block_on(async {
            let base = std::env::temp_dir().join("frameplayer-naming-test");
            let _ = std::fs::remove_dir_all(&base);
            let stage = base.join("stage");
            std::fs::create_dir_all(&stage).unwrap();
            std::fs::write(stage.join("clip.mkv"), vec![1u8; 200_000]).unwrap();
            let created = librqbit::create_torrent(
                &stage,
                librqbit::CreateTorrentOptions {
                    name: Some("The Show S01 [1080p]"),
                    piece_length: Some(65536),
                },
            )
            .await
            .unwrap();
            let hash = created.info_hash().as_string();
            std::fs::rename(&stage, base.join(&hash)).unwrap();

            let service = Arc::new(TorrentService::default());
            let unnamed = service.list(&Dirs::single(base.clone()));
            assert_eq!(unnamed.len(), 1);
            assert_eq!(
                unnamed[0].name, None,
                "a name was produced with no metadata to produce it from"
            );

            // The cache `add` writes for its own sake — no session, no swarm,
            // and now the row can be read as well as measured.
            let meta = meta_path(&base, &hash);
            std::fs::create_dir_all(meta.parent().unwrap()).unwrap();
            std::fs::write(&meta, created.as_bytes().unwrap()).unwrap();
            let named = service.list(&Dirs::single(base.clone()));
            assert_eq!(named[0].name.as_deref(), Some("The Show S01 [1080p]"));
            assert_eq!(named[0].info_hash.as_deref(), Some(hash.as_str()));

            let _ = std::fs::remove_dir_all(&base);
        });
    }

    /// The other half: with a session running, deleting takes the torrent out
    /// of its store there and then, rather than leaving it for the next start.
    ///
    /// The case is a torrent the session **restored** — which is every torrent
    /// on disk that has not been opened yet this run, and therefore the one the
    /// old code missed: it looked the hash up in `inner.torrents`, where only
    /// `add` puts anything.
    #[test]
    fn forget_removes_a_restored_torrent_from_the_store() {
        tauri::async_runtime::block_on(async {
            let base = std::env::temp_dir().join("frameplayer-forget-test");
            let _ = std::fs::remove_dir_all(&base);
            let stage = base.join("stage");
            std::fs::create_dir_all(&stage).unwrap();
            std::fs::write(stage.join("clip.mkv"), vec![3u8; 300_000]).unwrap();
            let created = librqbit::create_torrent(
                &stage,
                librqbit::CreateTorrentOptions {
                    name: Some("forget"),
                    piece_length: Some(65536),
                },
            )
            .await
            .unwrap();
            let hash = created.info_hash().as_string();
            std::fs::rename(&stage, base.join(&hash)).unwrap();
            // The metadata cache, so the add resolves from disk and this test
            // needs no swarm.
            let meta = meta_path(&base, &hash);
            std::fs::create_dir_all(meta.parent().unwrap()).unwrap();
            std::fs::write(&meta, created.as_bytes().unwrap()).unwrap();

            let first = Arc::new(TorrentService::default());
            first
                .add(&Dirs::single(base.clone()), format!("magnet:?xt=urn:btih:{hash}"), false, false)
                .await
                .unwrap();
            first.shutdown_session().await;

            // A later run that opens some other torrent, so the session exists
            // and this one is in it only because it was restored.
            let second = Arc::new(TorrentService::default());
            second.ensure_started(&Dirs::single(base.clone()), false, false).await.unwrap();
            let folder = find_folder(std::slice::from_ref(&base), &hash).expect("no folder for the torrent");
            let folder = folder.file_name().unwrap().to_string_lossy().into_owned();
            second.forget(&Dirs::single(base.clone()), &base.join(&folder).to_string_lossy()).await.unwrap();

            let store = base.join(SESSION_DIR).join("session.json");
            assert!(
                !std::fs::read_to_string(&store).unwrap().contains(&hash),
                "a restored torrent was deleted from disk but not from the store"
            );
            second.shutdown_session().await;

            let _ = std::fs::remove_dir_all(&base);
        });
    }

    /// The identity the frontend reads back out of the URL. If this shape
    /// changes, `parseTorrentUrl` in source.ts changes with it.
    #[test]
    fn urls_carry_the_identity() {
        let url = stream_url(51234, "abcdef", 3, "Season 1/Ep 3 [1080p].mkv");
        assert_eq!(
            url,
            "http://127.0.0.1:51234/t/abcdef/3/Ep%203%20%5B1080p%5D.mkv"
        );
        // The extension survives encoding — ffmpeg probes by it.
        assert!(url.ends_with(".mkv"));
    }
}

/// The file of a torrent as it sits on disk, **without loading the session**.
///
/// The start screen needs this and only this: a path it can decode a poster
/// from, for a card whose entry is a `torrent:` id. Going through
/// `torrent_local_path` would mean adding the torrent — a DHT-joining,
/// peer-connecting session — to draw a thumbnail, which is the opposite of the
/// rule that nothing opens until it is asked for.
///
/// Everything needed is already on disk: the cached `.torrent` gives the file's
/// relative name and its length, the hash gives the folder, and completeness is
/// the same allocated-blocks measure `torrent_list` uses to report size. An
/// incomplete file is refused rather than returned, because its holes read back
/// as **zeros** — a poster decoded from one is a black rectangle presented as a
/// frame of the film.
#[tauri::command]
pub fn torrent_offline_file(
    app: tauri::AppHandle,
    info_hash: String,
    index: usize,
) -> Option<LocalFile> {
    if !is_info_hash(&info_hash) {
        return None;
    }
    let hash = info_hash.to_ascii_lowercase();
    let dirs = TorrentService::download_dir(&app).ok()?;
    let meta = meta_path(&dirs.state, &hash);
    let bytes = match std::fs::read(&meta) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[poster] no cached metadata for {hash}: {e}");
            return None;
        }
    };
    let parsed = match librqbit::torrent_from_bytes::<librqbit::ByteBuf>(&bytes) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[poster] cached metadata for {hash} unreadable: {e:#}");
            return None;
        }
    };
    let info = parsed.info;

    // Single-file torrents have no file list; the torrent itself is the file.
    let (rel, length) = match info.iter_file_details().ok()?.nth(index) {
        Some(f) => (
            f.filename
                .iter_components()
                .flatten()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(std::path::MAIN_SEPARATOR_STR),
            f.len,
        ),
        None => return None,
    };
    let path = folder_for(&dirs, &hash, None).join(&rel);
    let Ok(meta) = std::fs::metadata(&path) else {
        eprintln!("[poster] {hash}/{index}: no file at {}", path.display());
        return None;
    };
    if meta.len() != length {
        eprintln!(
            "[poster] {hash}/{index}: {} is {} bytes, torrent says {length}",
            path.display(),
            meta.len()
        );
        return None;
    }
    // On Windows librqbit never marks the file sparse, so the allocated size is
    // the full length whatever has arrived and this test cannot tell an empty
    // file from a finished one — hence the caller treats a poster that fails to
    // decode as "no poster yet" rather than as an error.
    let complete = file_disk_size(&meta) + (length / 64) >= length;
    if !complete {
        eprintln!(
            "[poster] {hash}/{index}: only {} of {length} bytes allocated — incomplete",
            file_disk_size(&meta)
        );
    }
    complete.then(|| LocalFile {
        path: path.to_string_lossy().into_owned(),
        complete,
    })
}

