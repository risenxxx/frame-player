//! The librqbit session: the preferences baked into it, building it, tearing it down.

use std::sync::Arc;
use std::time::Duration;

use librqbit::{Session, SessionOptions};

use super::dirs::Dirs;
use super::{Inner, TorrentService};

/// librqbit's own session store, inside our cache so `torrent_clear_cache`
/// takes it with everything else. A dot name so `torrent_list` skips it — it is
/// ours, not a torrent occupying space.
pub(super) const SESSION_DIR: &str = ".session";

/// Ports asked for, in order, before falling back to whatever the OS gives.
///
/// A stable port is what makes a router mapping worth anything — UPnP renews
/// the one it mapped, and somebody who forwarded a port by hand forwarded a
/// number — so this is a range of our own rather than an ephemeral port.
const LISTEN_PORTS: std::ops::Range<u16> = 42800..42900;

/// The port the incoming listener asks for.
///
/// **The walk over the range is ours now.** 8.x took a `listen_port_range` and
/// bound the first port that was free; 9.x takes one address, and a listener
/// that cannot bind is fatal to the whole session — which for this player means
/// the torrent feature is dead until the app is restarted, and the seeding and
/// port-forwarding switches rebuild the session often enough for that to matter.
///
/// Falling back to `0` is the important half rather than the tidy one: a stable
/// port is only *nicer*, while a session that will not start takes everything
/// with it. The probe is IPv4, which is the family another BitTorrent client on
/// this machine would be holding; a port that frees up between the probe and
/// the bind is a race nobody can close from here, and it costs the same error
/// 8.x gave when its whole range was busy.
fn listen_port() -> u16 {
    LISTEN_PORTS
        .clone()
        .find(|p| std::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, *p)).is_ok())
        .unwrap_or(0)
}

/// How long `pause_restored` waits for one restored torrent to leave
/// `Initializing`. Generous in tries and tiny in step: with `fastresume` there
/// is nothing to hash, so this normally settles on the first or second look,
/// and the bound only matters for a store that has gone stale.
const RESTORE_SETTLE_TRIES: usize = 100;
const RESTORE_SETTLE_STEP: Duration = Duration::from_millis(20);

/// The preferences that are baked into a session when it is built.
///
/// **One struct rather than three parameters, because they share the rule that
/// matters about them**: not one of them can be changed on a live session, so
/// changing any means tearing it down and losing the running torrent. Spelled
/// out as separate terms, the reuse check in `ensure_started` is one `&&` per
/// preference, and the one added next is the one somebody forgets — which fails
/// in the quietest way available, as a setting that appears to save and does
/// nothing at all until the app is restarted. Here it is one `==`.
#[derive(Clone, Default, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPrefs {
    /// Upload to other peers while streaming. Off by default — see `set_seeding`.
    pub seeding: bool,
    /// Ask the router to forward the listening port. See `set_port_forward`.
    pub port_forward: bool,
    /// How peer connections are encrypted. See `Encryption`.
    pub encryption: Encryption,
    /// `socks5://[user:pass@]host:port`, or empty for a direct connection.
    ///
    /// **What it covers is outgoing TCP and the HTTP announces, and nothing
    /// else.** librqbit sends peer connections through the proxy and builds its
    /// tracker client with it, but the DHT and the UDP trackers are their own
    /// sockets — SOCKS5 carries UDP only through an ASSOCIATE that neither side
    /// uses here — so those keep going out the ordinary way. That is the half
    /// the settings hint has to say out loud rather than leave to be discovered.
    pub proxy: String,
    /// Which interface the swarm traffic leaves by — the system route (the
    /// default, and through the VPN when one is up), the way out *without* the
    /// VPN, or one named interface. See `net_route`.
    ///
    /// **Everything librqbit opens is scoped by it** — peer TCP and uTP, the
    /// DHT, the UDP and HTTP trackers, local discovery, the UPnP forwarder — and
    /// so is `announce_peers`, the one tracker request of our own. What is not:
    /// the loopback server mpv reads from, casting, the catalog, the feeds.
    /// And the one thing to say wherever it is offered: past the VPN the swarm
    /// sees this machine's own address.
    #[serde(default)]
    pub route: crate::net_route::Route,
}

/// Whether to obfuscate the peer stream (MSE/PE), in this player's own words.
///
/// **A vocabulary of ours rather than librqbit's**, for the reason every
/// persisted value in this app has one: this is written to localStorage and
/// travels over the Tauri boundary, so tying it to a third-party enum's variant
/// names would break somebody's stored preference the day that enum is renamed
/// — or the day the patch carrying it is dropped.
///
/// What it buys is the thing a plaintext BitTorrent handshake cannot survive on
/// a filtered network: the first bytes on the wire are a Diffie-Hellman
/// exchange rather than `\x13BitTorrent protocol`, so a middlebox matching on
/// the protocol has nothing to match. What it does **not** hide is who is being
/// talked to, nor the DHT and the trackers, which are their own plaintext
/// sockets — see `SessionPrefs::proxy` for the other half of that story.
#[derive(Clone, Copy, Default, PartialEq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Encryption {
    /// Plaintext only, which is what every release before this one did.
    Off,
    /// **The default**: prefer MSE and fall back to a plaintext redial when a
    /// peer will not do it. Nothing is lost against `Off` — an old peer is
    /// still reached — so there is no reason to make anyone find this setting.
    #[default]
    On,
    /// MSE or nothing: a peer that will not encrypt is dropped. For a network
    /// where the plaintext handshake is what gets cut, and the honest cost is
    /// the peers that do not speak it.
    Only,
}

impl Encryption {
    fn mode(self) -> librqbit::MseMode {
        match self {
            Encryption::Off => librqbit::MseMode::Disabled,
            Encryption::On => librqbit::MseMode::Enabled,
            Encryption::Only => librqbit::MseMode::Forced,
        }
    }
}

impl TorrentService {
    /// The session and the HTTP server, created on first use.
    ///
    /// Takes the directory rather than the `AppHandle` so the whole path —
    /// session, server, streaming — can be exercised by a test against a real
    /// swarm without a Tauri app around it (see `sintel_smoke`).
    pub(super) async fn ensure_started(
        self: &Arc<Self>,
        dirs: &Dirs,
        prefs: &SessionPrefs,
    ) -> Result<(Arc<Session>, u16), String> {
        // **The session's own directory is `state`, never the chosen root.**
        // librqbit's `output_folder` here is only a default for torrents added
        // without one, and ours always carry an explicit folder — so what this
        // actually decides is where the store and its resume data live, which
        // belong with the player's bookkeeping and not in somebody's film
        // library. It also means changing the root never moves the session.
        let dir = dirs.state.clone();
        let port = self.ensure_server().await?;
        // Resolved on every call, not only when the preference changes: the
        // interface "past the VPN" names is whichever one is the way out right
        // now, and a session still bound to yesterday's is a session that
        // cannot send. It is a handful of syscalls.
        let route = prefs.route.clone();
        let device = tauri::async_runtime::spawn_blocking(move || crate::net_route::resolve(&route))
            .await
            .map_err(|e| e.to_string())??;
        {
            let inner = self.inner.lock().await;
            // Only reuse a session that matches the current preferences. Every
            // one of them is fixed when the session is built, so a session that
            // seeds cannot be talked out of it, one built without a mapping
            // cannot grow one, and one built without a proxy cannot start using
            // it — see `SessionPrefs` and `set_seeding`.
            if let Some(session) = inner.session.clone() {
                if inner.prefs == *prefs && inner.device == device {
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
                dht: Some(librqbit::DhtSessionConfig {
                    persistence: None,
                    ..Default::default()
                }),
                // Off by default, and that default is a safety decision rather
                // than a technical one: in Germany and several other
                // jurisdictions the exposure from *uploading* copyrighted
                // material is categorically worse than from downloading it, and
                // a video player must not opt its users into that silently.
                // What the flag does is thorough — librqbit stops advertising
                // which pieces it has (no bitfield, no `have`), refuses piece
                // requests outright, and drops peers once the file is complete.
                disable_upload: !prefs.seeding,
                // Without a listener the session announces `port=0`, and
                // trackers refuse that outright (opentrackr answers "Port
                // can't be 0", the tracker 403s) — so every tracker announce was
                // silently worthless and the DHT was the *only* peer source,
                // which is exactly the redundancy failure that made its
                // Windows death (see vendor/README.md) a total outage. A real
                // port also lets NAT-ed seeds connect to us, which is how a
                // home-seeded swarm often reaches a leecher at all.
                listen: Some(librqbit::ListenerOptions {
                    // One address where 8.x took a range — see `listen_port`
                    // for why the walk over that range is now ours. Unspecified
                    // IPv6 is upstream's own default and asks for a dual-stack
                    // socket, so this listens on both families.
                    listen_addr: (std::net::Ipv6Addr::UNSPECIFIED, listen_port()).into(),
                    // **Off by default and opt-in, because it changes the
                    // machine rather than the app**: a mapping makes this port
                    // reachable from the internet for as long as the session
                    // lives. What it buys is measured and large — of ~30
                    // addresses one tracker announce returned, 20–22 never
                    // answered a SYN, i.e. they are behind NAT and can only
                    // ever be reached if they dial us. A reachable client turns
                    // those from unreachable into possible. See upnp.rs, which
                    // is what lets the setting say whether the router actually
                    // did it: librqbit's forwarder reports to nobody, and a
                    // switch that cannot tell is worse than none.
                    enable_upnp_port_forwarding: prefs.port_forward,
                    ..Default::default()
                }),
                // **The one setting here that sends the traffic somewhere else
                // entirely.** It is opt-in and empty by default, because a
                // proxy is somebody's own — a VPN provider's SOCKS5 endpoint,
                // a VPS — and there is no sane guess to make on their behalf.
                // What it buys is a route for the peer traffic that is not the
                // system's, which is the honest half of "send torrents around
                // the VPN": the packets still leave through whatever tunnel is
                // up, but they leave as one connection to one host. What it
                // does not buy is invisibility — the payload inside is
                // unencrypted BitTorrent, so a network filtering on the
                // protocol rather than on the destination is unaffected. See
                // `SessionPrefs::proxy` for what stays outside it.
                // **What a filtered network cannot match on.** The handshake
                // that used to open every peer connection is `\x13BitTorrent
                // protocol` in the clear, and cutting it mid-stream is what DPI
                // on several ISPs actually does — measured on one such swarm as
                // a peer that completes a TCP connection and then dies. With
                // MSE the first bytes are a Diffie-Hellman exchange instead.
                // Default `Enabled` rather than `Forced` because the fallback
                // costs nothing: a peer that will not encrypt is redialled in
                // the clear, so this is never worse than what shipped before.
                mse_mode: prefs.encryption.mode(),
                connect: Some(librqbit::ConnectionOptions {
                    proxy_url: (!prefs.proxy.is_empty()).then(|| prefs.proxy.clone()),
                    ..Default::default()
                }),
                // **A descriptor per file of every torrent in the session is
                // what librqbit's own storage costs**, and persistence puts
                // every torrent ever added into the session — so the bill is
                // the whole cache directory rather than the film being
                // watched. It is paid in the one currency this process cannot
                // overdraw quietly: see `raise_fd_limit` in lib.rs for what
                // macOS does to a media stack reached for the first time with
                // a full table. `torrent_storage` opens a file when it is read
                // and closes it when the budget says so; everything on disk
                // stays exactly as upstream leaves it.
                default_storage_factory: Some(crate::torrent_storage::storage_factory()),
                // `None` is the system route. A name scopes every socket the
                // session opens to that interface — see `SessionPrefs::route`.
                bind_device_name: device.clone(),
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
        inner.prefs = prefs.clone();
        inner.device = device;
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
            .and_then(|s| s.listen_addr().map(|a| a.port()))
            .unwrap_or(0)
    }

    /// Stop the session and forget every torrent in it.
    ///
    /// The only way to stop seeding, because `disable_upload` is baked into the
    /// session when it is built and librqbit offers no way to change it after.
    /// Everything already fetched stays on disk, so re-opening the magnet
    /// continues rather than starting over — what ends is the peer traffic,
    /// which is the entire point of doing this.
    pub(super) async fn shutdown_session(&self) {
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
        self.rebuild_if(|inner| inner.prefs.seeding != seeding).await
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
        self.rebuild_if(|inner| inner.prefs.port_forward != on).await
    }

    /// Apply the proxy preference, on the same terms as the two above.
    ///
    /// The proxy is chosen when the session is built and there is no way to
    /// change it afterwards, so switching one on — or off, which is the
    /// direction that matters — costs the running torrent. That is the honest
    /// trade rather than an implementation detail: a field reading "through
    /// this proxy" while pieces keep arriving straight from the swarm would be
    /// a lie about where this machine's traffic is going, which is the whole
    /// reason somebody typed an address into it.
    pub async fn set_proxy(&self, url: String) -> bool {
        self.rebuild_if(|inner| inner.prefs.proxy != url).await
    }

    /// Apply the encryption preference, on the same terms as the three above:
    /// the mode is fixed when the session is built, so changing it costs the
    /// running torrent. Cheapest of the four to change one's mind about, and
    /// the only one where the *middle* setting is the answer for almost
    /// everybody — see `Encryption`.
    pub async fn set_encryption(&self, mode: Encryption) -> bool {
        self.rebuild_if(|inner| inner.prefs.encryption != mode).await
    }

    /// Apply the route preference, on the same terms as the four above: the
    /// interface is bound when the session is built. Leaving the VPN has the
    /// urgency the proxy has in reverse — a setting reading "through the VPN"
    /// while pieces still arrive past it would be a lie about the one thing
    /// somebody turned a VPN on for.
    pub async fn set_route(&self, route: crate::net_route::Route) -> bool {
        self.rebuild_if(|inner| inner.prefs.route != route).await
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
