//! Torrent streaming (ROADMAP 21): a magnet link becomes a URL mpv can open.
//!
//! The architecture is the settled one — peerflix, Stremio and webtorrent all
//! work this way. A torrent client inside the process runs a local HTTP server;
//! mpv opens `http://127.0.0.1:<port>/…` and its Range requests *are* the signal
//! for "the viewer is here", which the client turns into piece priority.
//! [librqbit](https://lib.rs/crates/librqbit) already does that half: it blocks
//! a read until the piece arrives and prioritises what is being streamed.
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

/// Read size for one body chunk. Matches librqbit's own streaming handler; a
/// piece is typically 1–16 MB, so this is well inside one and the reader blocks
/// on piece arrival rather than on the buffer.
const STREAM_CHUNK: usize = 65536;

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
    /// Set when `folder` is a well-formed info hash.
    pub info_hash: Option<String>,
    pub size: u64,
    /// Currently loaded in the session — deleting it would pull the file out
    /// from under mpv, so the row offering to is disabled rather than clever.
    pub active: bool,
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
}

impl TorrentService {
    /// Where the pieces land.
    ///
    /// The cache directory rather than Downloads: this is a side effect of
    /// watching, not a download the viewer asked to keep, and a cache directory
    /// is the one place an OS and a user both understand as disposable.
    fn download_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
        let dir = app
            .path()
            .app_cache_dir()
            .map_err(|e| format!("no cache dir: {e}"))?
            .join("torrents");
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {dir:?}: {e}"))?;
        Ok(dir)
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
        dir: PathBuf,
        seeding: bool,
    ) -> Result<(Arc<Session>, u16), String> {
        let port = self.ensure_server().await?;
        {
            let inner = self.inner.lock().await;
            // Only reuse a session that matches the current preference.
            // `disable_upload` is fixed when the session is built, so a session
            // that seeds cannot be talked out of it — see `set_seeding`.
            if let Some(session) = inner.session.clone() {
                if inner.seeding == seeding {
                    return Ok((session, port));
                }
            }
        }
        self.shutdown_session().await;

        let session = Session::new_with_opts(
            dir,
            SessionOptions {
                // Nothing is remembered between runs on purpose: a list of
                // torrents resumed at startup is a background peer-to-peer
                // client, which is not what this app is. Partial *data* still
                // survives on disk and is picked up by `overwrite` below.
                persistence: None,
                // Off by default, and that default is a safety decision rather
                // than a technical one: in Germany and several other
                // jurisdictions the exposure from *uploading* copyrighted
                // material is categorically worse than from downloading it, and
                // a video player must not opt its users into that silently.
                // What the flag does is thorough — librqbit stops advertising
                // which pieces it has (no bitfield, no `have`), refuses piece
                // requests outright, and drops peers once the file is complete.
                disable_upload: !seeding,
                ..Default::default()
            },
        )
        .await
        .map_err(|e| format!("torrent session failed: {e:#}"))?;

        let mut inner = self.inner.lock().await;
        inner.session = Some(session.clone());
        inner.seeding = seeding;
        Ok((session, port))
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
    /// about the one behaviour with legal weight attached to it. That costs the
    /// current stream — the session goes with it — which is the honest trade and
    /// is what the settings hint warns about.
    ///
    /// Turning it **on** needs no such urgency, but goes down the same path so
    /// there is only one rule to reason about.
    pub async fn set_seeding(&self, seeding: bool) -> bool {
        let changed = {
            let inner = self.inner.lock().await;
            inner.session.is_some() && inner.seeding != seeding
        };
        if changed {
            self.shutdown_session().await;
        }
        changed
    }

    /// Resolve a magnet (or a `.torrent` URL/path) into its file list.
    ///
    /// Added **paused with nothing selected**: this call answers "what is in
    /// here", and answering it must not start a download. What turns a file into
    /// traffic is a request for it arriving at the server.
    pub async fn add(
        self: &Arc<Self>,
        dir: PathBuf,
        source: String,
        seeding: bool,
    ) -> Result<TorrentInfo, String> {
        let (session, port) = self.ensure_started(dir.clone(), seeding).await?;

        // The folder has to be decided *before* the torrent is added, and it is
        // named after the info hash — which a magnet already carries in its
        // `xt=urn:btih:`. Parsing it costs nothing; asking librqbit would mean
        // resolving the magnet twice, and a resolve is a ten-second DHT lookup.
        //
        // A `.torrent` link has no hash to read, so there the folder falls back
        // to librqbit's own default (the torrent's name). Such an entry simply
        // shows up in `torrent_list` as one that cannot be resumed, which is the
        // same treatment folders from the older layout get.
        let source = source.trim().to_string();
        let hinted_hash = librqbit::Magnet::parse(&source)
            .ok()
            .and_then(|m| m.as_id20())
            .map(|id| id.as_string());

        let opts = || AddTorrentOptions {
            paused: true,
            // Reuse whatever of this torrent is already in the cache directory.
            // Without it, re-opening a magnet watched yesterday errors on the
            // existing files instead of continuing from them.
            overwrite: true,
            // **The folder is the info hash**, rather than librqbit's default of
            // the torrent's own name. Three reasons, and the first is the one
            // that decides it: `ManagedTorrentOptions.output_folder` is
            // `pub(crate)`, so a name-derived folder could never be *read back*
            // and the mapping from "this torrent" to "this directory" would have
            // to be reconstructed by guessing. It also removes every question a
            // torrent name raises as a path — length limits, `/` in the name,
            // two releases that share one — and it makes `torrent_forget` cheap
            // to validate: a legal folder here is exactly 40 hex characters.
            output_folder: hinted_hash
                .as_ref()
                .map(|h| dir.join(h).to_string_lossy().into_owned()),
            ..Default::default()
        };

        // **A magnet already opened once is never resolved again.** The metadata
        // is the torrent file, and once we have it there is nothing left to ask
        // the swarm for — but librqbit runs with `persistence: None` (a session
        // that resumes torrents at startup is a background BitTorrent client,
        // which this app is not), so it forgets the torrent between runs and
        // would go back to the DHT every time. Measured, that lookup is ~10 s,
        // and it was being paid even to reopen a season already sitting on disk.
        // Caching the bytes ourselves keeps the session stateless and makes a
        // reopen instant.
        let cached = hinted_hash
            .as_ref()
            .and_then(|h| std::fs::read(meta_path(&dir, h)).ok());

        let mut added = None;
        if let Some(bytes) = cached {
            match tokio::time::timeout(
                RESOLVE_TIMEOUT,
                session.add_torrent(AddTorrent::from_bytes(bytes), Some(opts())),
            )
            .await
            {
                Ok(Ok(r)) => added = Some(r),
                // A truncated or stale cache file must not make the torrent
                // unopenable — the magnet is still the source of truth.
                Ok(Err(e)) => eprintln!("[torrent] cached metadata unusable, resolving: {e:#}"),
                Err(_) => eprintln!("[torrent] cached metadata timed out, resolving"),
            }
        }

        let added = match added {
            Some(r) => r,
            None => tokio::time::timeout(
                RESOLVE_TIMEOUT,
                session.add_torrent(AddTorrent::from_url(source.as_str()), Some(opts())),
            )
            .await
            .map_err(|_| "resolve_timeout".to_string())?
            .map_err(|e| format!("{e:#}"))?,
        };

        let handle = match added {
            AddTorrentResponse::Added(_, h) | AddTorrentResponse::AlreadyManaged(_, h) => h,
            AddTorrentResponse::ListOnly(_) => return Err("torrent not started".into()),
        };

        // Keep the metadata for next time. Best-effort: failing to write it
        // costs a DHT lookup on the next open and nothing else.
        if let Ok(bytes) = handle.with_metadata(|m| m.torrent_bytes.clone()) {
            let path = meta_path(&dir, &handle.info_hash().as_string());
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(path, &bytes);
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
        // large torrent, and holding it would serialise every other request —
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
    pub async fn local_path(&self, dir: &std::path::Path, info_hash: &str, index: usize) -> Option<LocalFile> {
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
        let path = dir.join(info_hash.to_ascii_lowercase()).join(rel);
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
    pub async fn list(&self, dir: &std::path::Path) -> Vec<TorrentOnDisk> {
        let live: HashSet<String> = {
            let inner = self.inner.lock().await;
            inner.torrents.keys().cloned().collect()
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let Some(folder) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if folder.starts_with('.') {
                continue;
            }
            let info_hash = is_info_hash(&folder).then(|| folder.to_ascii_lowercase());
            out.push(TorrentOnDisk {
                active: info_hash.as_ref().is_some_and(|h| live.contains(h)),
                size: dir_size(&entry.path()),
                info_hash,
                folder,
            });
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
    /// `add` changes, and the "folder name is the info hash" invariant that
    /// `torrent_list` and `torrent_forget` rely on survives intact — which it
    /// would not if the two were merged into one folder instead. That is also
    /// what keeps deletion unambiguous, the thing qBittorrent's shared-save-path
    /// approach gives up.
    pub async fn relocate(
        &self,
        dir: &std::path::Path,
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
        let from = dir.join(&old_hash);
        let to = dir.join(&new_hash);
        if !from.is_dir() {
            return Err("nothing to move".into());
        }
        if to.exists() {
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
        let _ = std::fs::remove_file(meta_path(dir, &old_hash));
        Ok(())
    }

    /// Drop a torrent from the session and delete its data.
    ///
    /// Both halves, in that order: removing the directory while librqbit still
    /// holds the files open would leave it writing pieces into a folder that no
    /// longer exists, which on some filesystems recreates it.
    pub async fn forget(&self, dir: &std::path::Path, folder: &str) -> Result<u64, String> {
        // A delete-by-name command that accepts any name is one wrong argument
        // away from removing something else — the same lock `subs_delete_file`
        // puts on its extension. The name must be a single path component.
        if folder.is_empty()
            || folder.contains(['/', '\\'])
            || folder.contains("..")
            || folder.starts_with('.')
        {
            return Err("bad folder".into());
        }
        let path = dir.join(folder);
        if !path.starts_with(dir) || !path.is_dir() {
            return Err("not a torrent folder".into());
        }

        if is_info_hash(folder) {
            let hash = folder.to_ascii_lowercase();
            let (session, handle) = {
                let mut inner = self.inner.lock().await;
                let session = inner.session.clone();
                (session, inner.torrents.remove(&hash))
            };
            if let (Some(session), Some(entry)) = (session, handle) {
                // `delete_files: false` — the directory is ours to remove below,
                // and doing it here would depend on librqbit agreeing with us
                // about which files belong to this torrent.
                let _ = session
                    .delete(librqbit::api::TorrentIdOrHash::Id(entry.handle.id()), false)
                    .await;
            }
        }

        let size = dir_size(&path);
        std::fs::remove_dir_all(&path).map_err(|e| format!("{e}"))?;
        Ok(size)
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
            .header(header::CONTENT_TYPE, mime_for(&name));

        let range = req
            .headers()
            .get(header::RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| parse_range(v, total));

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

        if req.method() == Method::HEAD {
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
/// a dependency: this escapes one file name for one loopback URL.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
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
/// Only the single-range `bytes=` form: that is what ffmpeg sends, and a
/// multipart response to a media player would be answering a question nobody
/// asked.
fn parse_range(value: &str, total: u64) -> Option<Option<(u64, u64)>> {
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
) -> Result<TorrentInfo, String> {
    let dir = TorrentService::download_dir(&app)?;
    service.inner().clone().add(dir, source, seeding).await
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
    let dir = TorrentService::download_dir(&app)?;
    Ok(service.local_path(&dir, &info_hash, index).await)
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
    let dir = TorrentService::download_dir(&app)?;
    Ok(service.list(&dir).await)
}

#[tauri::command]
pub async fn torrent_relocate(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<TorrentService>>,
    old_hash: String,
    new_hash: String,
) -> Result<(), String> {
    let dir = TorrentService::download_dir(&app)?;
    service.relocate(&dir, &old_hash, &new_hash).await
}

#[tauri::command]
pub async fn torrent_forget(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<TorrentService>>,
    folder: String,
) -> Result<u64, String> {
    let dir = TorrentService::download_dir(&app)?;
    service.forget(&dir, &folder).await
}

/// Delete everything the torrent cache holds.
///
/// Streaming writes the pieces to disk, so a few films fill a cache directory
/// the viewer never chose to fill. Offered next to the thumbnail cache in
/// settings, for the same reason.
#[tauri::command]
pub async fn torrent_clear_cache(app: tauri::AppHandle) -> Result<u64, String> {
    let dir = TorrentService::download_dir(&app)?;
    let mut freed = 0u64;
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(0);
    };
    for entry in entries.flatten() {
        let size = dir_size(&entry.path());
        let removed = if entry.path().is_dir() {
            std::fs::remove_dir_all(entry.path()).is_ok()
        } else {
            std::fs::remove_file(entry.path()).is_ok()
        };
        if removed {
            freed += size;
        }
    }
    Ok(freed)
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

fn dir_size(path: &std::path::Path) -> u64 {
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
            let dir_for_list = dir.clone();
            let service = Arc::new(TorrentService::default());

            let t0 = std::time::Instant::now();
            // Never seeds, matching the shipped default — a test must not
            // quietly upload to strangers.
            let info = service
                .add(dir, magnet, false)
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
            let listed = service.list(&dir_for_list).await;
            println!("\n--- torrent_list ---");
            for row in &listed {
                println!(
                    "  {} hash={:?} size={} active={}",
                    row.folder, row.info_hash, row.size, row.active
                );
            }
            assert!(
                listed
                    .iter()
                    .any(|r| r.info_hash.as_deref() == Some(info.info_hash.as_str()) && r.active),
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
            let r = tauri::async_runtime::block_on(service.forget(&base, bad));
            assert!(r.is_err(), "{bad:?} should have been refused");
        }
        // The guard is not merely returning errors — nothing was removed.
        assert!(base.join("victim").is_dir());
        let _ = std::fs::remove_dir_all(&base);
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
                .add(base.clone(), format!("magnet:?xt=urn:btih:{hash}"), false)
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
                .local_path(&base, &info.info_hash, file.index)
                .await
                .expect("local_path found nothing");
            assert_eq!(local.path, base.join(&hash).join("clip.mkv").to_string_lossy());
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
