//! Adding a torrent, and selecting what the reader asks for.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent};

use super::announce::announce_peers;
use super::dirs::{folder_for, folder_name, meta_path, Dirs};
use super::http::stream_url;
use super::session::SessionPrefs;
use super::status::file_complete;
use super::{Entry, TorrentFile, TorrentInfo, TorrentService};

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

impl TorrentService {
    /// Resolve a magnet (or a `.torrent` URL/path) into its file list.
    ///
    /// Added **paused with nothing selected**: this call answers "what is in
    /// here", and answering it must not start a download. What turns a file into
    /// traffic is a request for it arriving at the server.
    pub async fn add(
        self: &Arc<Self>,
        dirs: &Dirs,
        source: String,
        prefs: &SessionPrefs,
    ) -> Result<TorrentInfo, String> {
        let (session, port) = self.ensure_started(dirs, prefs).await?;
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
                librqbit::torrent_from_bytes(bytes)
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
                .and_then(|h| std::fs::read(meta_path(dir, h)).ok()),
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
                let port = session.listen_addr().map(|a| a.port()).unwrap_or(0);
                // See `announce_peers`: a torrent added paused announces
                // `port=0`, which trackers answer with almost nothing, so the
                // peers for the metadata fetch are asked for directly.
                let device = self.inner.lock().await.device.clone();
                seed = Some(
                    announce_peers(&source, hash, port, &prefs.proxy, device.as_deref()).await,
                );
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
                let path = meta_path(dir, hash);
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(path, bytes);
            }
        }

        let meta_name = meta_bytes.as_deref().and_then(|b| {
            let meta = librqbit::torrent_from_bytes(b).ok()?;
            let name = String::from_utf8_lossy(meta.info.data.name?.as_ref())
                .trim()
                .to_string();
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
        // The same bytes answer two questions, which is why they are read back
        // from the handle rather than kept from above: what to cache, and which
        // trackers this torrent names.
        let mut trackers = Vec::new();
        if let Ok(bytes) = handle.with_metadata(|m| m.torrent_bytes.clone()) {
            trackers = trackers_of(&bytes);
            let path = meta_path(dir, &handle.info_hash().as_string());
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
            trackers,
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
    pub(super) async fn select(&self, info_hash: &str, index: usize) -> Result<Arc<ManagedTorrent>, String> {
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

        self.drop_from_session(&hash).await;
        match std::fs::rename(&from, &to) {
            Ok(()) => eprintln!("[torrent] {hash} renamed to {:?}", to.file_name()),
            Err(e) => eprintln!("[torrent] could not rename {hash}: {e}"),
        }
    }

    pub(super) async fn wait_ready(&self, handle: &Arc<ManagedTorrent>) -> Result<(), String> {
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

    /// Stop feeding a torrent — it is no longer what the player is watching.
    ///
    /// Pause rather than delete: the pieces already fetched stay in the cache,
    /// so re-opening the same magnet continues instead of starting over. What it
    /// does end is the uploading and the peer connections, which must not
    /// outlive the viewing.
    ///
    /// **The lock is dropped before the wait, and that is not tidiness.**
    /// `session.pause` reaches `ManagedTorrent::pause`, which blocks on the
    /// torrent's own state lock — a lock a stream read holds for the whole of
    /// its `pread`, and which upstream refuses outright while the storage is
    /// still borrowed. Held across that await, `inner` would be unavailable for
    /// as long as any of it takes, and **every** torrent command waits on
    /// `inner`: deleting a torrent, pruning its watched episodes, the status
    /// poll, opening the next magnet. This runs at exactly the wrong moment for
    /// that — leaving a film for the start screen — which is the click before
    /// the viewer reaches for the delete button beside it. Every other lock in
    /// this module is already taken this way; this was the one that was not.
    pub async fn release(&self, info_hash: &str) {
        let (session, handle) = {
            let inner = self.inner.lock().await;
            let (Some(session), Some(entry)) =
                (inner.session.clone(), inner.torrents.get(info_hash))
            else {
                return;
            };
            (session, entry.handle.clone())
        };
        if !handle.is_paused() {
            let _ = session.pause(&handle).await;
        }
    }
}

/// The trackers a torrent names, out of its own metadata.
///
/// **What this is for is somebody else's machine.** A magnet rebuilt from an
/// info hash alone (`magnetFor`) has no trackers in it, and that is exactly the
/// magnet a room hands to every guest when the host's own copy was not
/// remembered — a `.torrent` file, or history switched off. The host found the
/// swarm through a tracker whose address is sitting in these bytes; dropping it
/// leaves the guest with the DHT and nothing else, which on a network that
/// filters UDP is the difference between resolving in a second and timing out
/// after ninety.
///
/// Bounded and filtered, because `announce_list` is a list of *tiers* and a
/// public release routinely names dozens: what travels is a room message, and
/// four trackers is already more than `announce_peers` will ask.
fn trackers_of(bytes: &[u8]) -> Vec<String> {
    const MAX: usize = 6;
    let Ok(meta) = librqbit::torrent_from_bytes(bytes) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for raw in meta.iter_announce() {
        let url = String::from_utf8_lossy(raw.as_ref()).trim().to_string();
        // Only what a tracker client can actually speak. An unknown scheme is
        // dead weight in every magnet built from this afterwards.
        let usable = url.starts_with("http://")
            || url.starts_with("https://")
            || url.starts_with("udp://");
        if !usable || out.iter().any(|t| t == &url) {
            continue;
        }
        out.push(url);
        if out.len() == MAX {
            break;
        }
    }
    out
}
