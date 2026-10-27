use super::*;

use std::time::Duration;

use super::dirs::{find_folder, folder_for, meta_path, Dirs};
use super::session::{Encryption, SESSION_DIR};

/// Held by every test that builds a session. `listen_port` probes a port
/// and the session binds it a moment later, so two sessions starting at
/// once both see 42800 free and the second fails with "address already in
/// use" — a race the player never runs (it has one session), but the test
/// harness runs tests on parallel threads and it failed about one run in
/// two. Poisoning is ignored: one failed test must not fail the rest.
fn one_session_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    static SESSION: std::sync::Mutex<()> = std::sync::Mutex::new(());
    SESSION.lock().unwrap_or_else(|e| e.into_inner())
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
    let _session = one_session_at_a_time();
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

        // `FP_TEST_MSE=only` is what proves the encryption interoperates
        // with the real world rather than with our own tests: forced, every
        // peer that will not do MSE is dropped, so bytes arriving at all
        // means they arrived over an encrypted stream from somebody else's
        // client. `off` is the control.
        let prefs = SessionPrefs {
            encryption: match std::env::var("FP_TEST_MSE").unwrap_or_default().as_str() {
                "only" => Encryption::Only,
                "off" => Encryption::Off,
                _ => Encryption::On,
            },
            // `FP_TEST_ROUTE=direct` (or an interface name) is how the
            // bypass is checked against a real swarm with a VPN up: bytes
            // arriving means they arrived past the tunnel.
            route: match std::env::var("FP_TEST_ROUTE").unwrap_or_default().as_str() {
                "" => crate::net_route::Route::Auto,
                "direct" => crate::net_route::Route::Direct,
                name => crate::net_route::Route::Iface { name: name.into() },
            },
            ..Default::default()
        };
        let t0 = std::time::Instant::now();
        // Never seeds, matching the shipped default — a test must not
        // quietly upload to strangers.
        let info = service.add(&dirs, magnet, &prefs).await.expect("resolve failed");
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

        // **The buffer map, through the same call the seekbar polls**, and
        // asserted rather than printed because this is where a silent wrong
        // answer lives. The bitfield arrives as bits rather than as a
        // number: a bit order that disagreed with the piece index would
        // still produce a plausible list of bands, in the wrong places, and
        // read as a slow swarm rather than as a bug. The two reads above
        // are what make it checkable — the **tail** was fetched, so a
        // correct map has to end at the end of the file, and `file_ranges`
        // clamps the last piece to exactly 1.0.
        let bands = service.buffered(&info.info_hash, video.index).await;
        println!("buffered: {bands:?}");
        assert!(!bands.is_empty(), "nothing buffered after two reads");
        for (s, e) in &bands {
            assert!(
                (0.0..=1.0).contains(s) && (0.0..=1.0).contains(e) && s < e,
                "band out of range: {s}..{e}"
            );
        }
        assert!(
            bands.windows(2).all(|w| w[0].1 < w[1].0),
            "bands are unsorted or touching, which the merge should have joined"
        );
        assert!(
            bands.last().unwrap().1 > 0.999,
            "the tail was read but the map does not reach the end of the file: {bands:?}"
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
    let _session = one_session_at_a_time();
    tauri::async_runtime::block_on(async {
        let dir = std::env::temp_dir().join("frameplayer-swarm-probe");
        std::fs::create_dir_all(&dir).unwrap();
        let dirs = Dirs::single(dir.clone());
        let service = Arc::new(TorrentService::default());

        let info = service
            .add(&dirs, magnet, &SessionPrefs::default())
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
    let _session = one_session_at_a_time();
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
                trackers: Vec::new(),
            },
            &librqbit::spawn_utils::BlockingSpawner::new(1),
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
            .add(&Dirs::single(base.clone()), format!("magnet:?xt=urn:btih:{hash}"), &SessionPrefs::default())
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
    let _session = one_session_at_a_time();
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
                trackers: Vec::new(),
            },
            &librqbit::spawn_utils::BlockingSpawner::new(1),
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
            .add(&Dirs::single(base.clone()), format!("magnet:?xt=urn:btih:{hash}"), &SessionPrefs::default())
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
    let _session = one_session_at_a_time();
    tauri::async_runtime::block_on(async {
        let base = std::env::temp_dir().join("frameplayer-resume-test");
        let _ = std::fs::remove_dir_all(&base);
        let stage = base.join("stage");
        std::fs::create_dir_all(&stage).unwrap();
        let payload: Vec<u8> = (0..2_000_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(stage.join("clip.mkv"), &payload).unwrap();
        let created = librqbit::create_torrent(
            &stage,
            librqbit::CreateTorrentOptions {
                name: Some("resume"),
                piece_length: Some(65536),
                trackers: Vec::new(),
            },
            &librqbit::spawn_utils::BlockingSpawner::new(1),
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
        let info = first.add(&Dirs::single(base.clone()), magnet.clone(), &SessionPrefs::default()).await.unwrap();
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
        second.add(&Dirs::single(base.clone()), magnet, &SessionPrefs::default()).await.unwrap();
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
    let _session = one_session_at_a_time();
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
                trackers: Vec::new(),
            },
            &librqbit::spawn_utils::BlockingSpawner::new(1),
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
        first.add(&Dirs::single(base.clone()), magnet.clone(), &SessionPrefs::default()).await.unwrap();
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
        third.ensure_started(&Dirs::single(base.clone()), &SessionPrefs::default()).await.unwrap();
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
    let _session = one_session_at_a_time();
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
                trackers: Vec::new(),
            },
            &librqbit::spawn_utils::BlockingSpawner::new(1),
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
    let _session = one_session_at_a_time();
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
                trackers: Vec::new(),
            },
            &librqbit::spawn_utils::BlockingSpawner::new(1),
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
            .add(&Dirs::single(base.clone()), format!("magnet:?xt=urn:btih:{hash}"), &SessionPrefs::default())
            .await
            .unwrap();
        first.shutdown_session().await;

        // A later run that opens some other torrent, so the session exists
        // and this one is in it only because it was restored.
        let second = Arc::new(TorrentService::default());
        second.ensure_started(&Dirs::single(base.clone()), &SessionPrefs::default()).await.unwrap();
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
