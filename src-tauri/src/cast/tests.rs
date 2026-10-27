//! The tests that cross the submodules: the live smoke tests and probes
//! against real devices, and the URL privacy contract of `cast_load`.

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::io::AsyncReadExt;

use super::*;
use super::castv2::{tls_connect, wire, Pump, NS_CONNECTION, NS_RECEIVER, RECEIVER_ID};

/// `FP_TEST_CAST_IP=<address> cargo test --lib cast::tests::unicast_probe_smoke -- --nocapture`
/// — the unicast half of discovery against one real device, no multicast
/// involved; `FP_TEST_CAST_IP=sweep` runs one whole round over the LAN
/// instead and reports what it found and how long it took. Off without the
/// variable: it needs a device on the network.
#[test]
fn unicast_probe_smoke() {
    use futures_util::StreamExt as _;
    let Ok(target) = std::env::var("FP_TEST_CAST_IP") else { return };
    let rt = tokio::runtime::Runtime::new().unwrap();
    if target == "sweep" {
        let targets = crate::lan_sweep::sweep_targets(&[]);
        let started = std::time::Instant::now();
        let n = targets.len();
        let found: Vec<super::CastDeviceInfo> = rt.block_on(
            futures_util::stream::iter(targets)
                .map(super::discovery::probe_cast)
                .buffer_unordered(super::discovery::SWEEP_PARALLEL)
                .filter_map(|d| async move { d })
                .collect(),
        );
        println!("{n} addresses in {:?}", started.elapsed());
        for d in &found {
            println!("{} at {}", d.name, d.ip);
        }
        assert!(!found.is_empty(), "nothing answered the sweep");
        return;
    }
    let ip: std::net::Ipv4Addr = target.parse().expect("an IPv4 address or `sweep`");
    let device = rt.block_on(super::discovery::probe_cast(ip)).expect("a Cast receiver at that address");
    println!("{} ({}) at {}:{}", device.name, device.id, device.ip, device.port);
}

/// Live smoke test against whatever Cast devices the LAN actually has.
/// Off by default (needs a network with a TV on it); run with
/// `FP_TEST_CAST=1 cargo test --lib cast::tests::discover_smoke -- --nocapture`,
/// or with a specific device's IP in the variable (`FP_TEST_CAST=192.0.2.48`)
/// to skip discovery and test the TLS + status path against that device —
/// which is how the X.509-v1 certificate fix was verified against the TV
/// whose cert webpki refuses.
///
/// Deliberately stops short of LAUNCH: connect + GET_STATUS proves mDNS,
/// TLS, the frame codec and the JSON parse against real hardware without
/// changing anything on the device's screen.
#[tokio::test]
async fn discover_smoke() {
    let Ok(target) = std::env::var("FP_TEST_CAST") else {
        return;
    };
    let (ip, port) = if let Ok(ip) = target.parse::<IpAddr>() {
        println!("[cast] targeting {ip}:8009 directly");
        (ip, 8009)
    } else {
        let discovery = Discovery::start(Vec::new()).expect("mdns starts");
        tokio::time::sleep(Duration::from_secs(5)).await;
        let devices: Vec<CastDeviceInfo> = {
            let map = discovery.devices.lock().unwrap();
            map.values().cloned().collect()
        };
        discovery.stop();
        println!("[cast] devices found: {}", devices.len());
        for d in &devices {
            println!("[cast]   {} ({}) at {}:{}", d.name, d.model, d.ip, d.port);
        }
        let Some(device) = devices.first() else {
            println!("[cast] no devices on this network — smoke test ends here");
            return;
        };
        (device.ip.parse().unwrap(), device.port)
    };

    let stream = tls_connect(ip, port).await.expect("tls connects");
    let (mut rd, wr) = tokio::io::split(stream);
    let mut pump = Pump {
        wr,
        status: Arc::new(Mutex::new(StatusInner::default())),
        request_id: 0,
        transport: None,
        media_session: None,
        pending_load: None,
        load_request: None,
    };
    pump.send(NS_CONNECTION, RECEIVER_ID, json!({ "type": "CONNECT" }))
        .await
        .expect("CONNECT sends");
    let id = pump.next_id();
    pump.send(
        NS_RECEIVER,
        RECEIVER_ID,
        json!({ "type": "GET_STATUS", "requestId": id }),
    )
    .await
    .expect("GET_STATUS sends");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < deadline, "no RECEIVER_STATUS within 5 s");
        let mut len = [0u8; 4];
        tokio::time::timeout(Duration::from_secs(5), rd.read_exact(&mut len))
            .await
            .expect("read")
            .expect("read");
        let n = u32::from_be_bytes(len) as usize;
        let mut buf = vec![0u8; n];
        rd.read_exact(&mut buf).await.expect("read body");
        let msg = wire::decode(&buf).expect("decodes");
        println!("[cast] <- {} {}", msg.namespace, msg.payload);
        if msg.namespace == NS_RECEIVER {
            let v: Value = serde_json::from_str(&msg.payload).unwrap();
            assert_eq!(v["type"].as_str(), Some("RECEIVER_STATUS"));
            println!(
                "[cast] volume level = {:?}, apps = {:?}",
                v["status"]["volume"]["level"],
                v["status"]["applications"]
                    .as_array()
                    .map(|a| a.len())
                    .unwrap_or(0)
            );
            break;
        }
    }
}

/// A whole cast driven from a test: bind the LAN server, register the
/// media, LAUNCH + LOAD against a real device, print every frame and every
/// HTTP request, then stop the receiver. Off by default; it takes over the
/// TV for the duration.
///
/// ```bash
/// FP_CAST_DEBUG=1 FP_CAST_LOAD=192.0.2.48 \
///   FP_CAST_FILE=/path/to/index.m3u8 \
///   cargo test --lib cast::tests::load_probe -- --nocapture
/// ```
///
/// `FP_CAST_FILE` takes a plain media file (progressive, served as one
/// file) or an HLS playlist (its directory is served and the segment
/// format is read off the playlist, so `ts` and `fmp4` renditions need no
/// flag). `FP_CAST_SECONDS` is how long to watch before stopping (default
/// 20). Why this exists: every receiver question so far — which container,
/// which codec, whether the TV fetched anything at all — costs a GUI run,
/// a file switch and a copy-pasted log, when the answer is one LOAD and
/// the frames that follow it.
#[tokio::test]
async fn load_probe() {
    let Ok(target) = std::env::var("FP_CAST_LOAD") else {
        return;
    };
    // Everything below spawns through tauri's async_runtime, which would
    // otherwise build a second runtime and hand this one's TcpListener to
    // a reactor that is not driving it.
    tauri::async_runtime::set(tokio::runtime::Handle::current());

    let file = PathBuf::from(
        std::env::var("FP_CAST_FILE").expect("FP_CAST_FILE=<media file or .m3u8>"),
    );
    assert!(file.is_file(), "no such file: {}", file.display());
    let seconds: u64 = std::env::var("FP_CAST_SECONDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);

    let is_hls = file.extension().map(|e| e == "m3u8").unwrap_or(false);
    // The rendition says which it is: `.m4s` segments are fMP4, and an
    // EXT-X-MAP is the same statement in the playlist's own words.
    let hls_format = is_hls.then(|| {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        if text.contains(".m4s") || text.contains("EXT-X-MAP") {
            "fmp4".to_string()
        } else {
            "ts".to_string()
        }
    });
    let (serve_root, name, mime) = if is_hls {
        (
            file.parent().unwrap().to_path_buf(),
            "index.m3u8".to_string(),
            "application/x-mpegURL",
        )
    } else {
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        let mime = cast_mime(&name);
        (file.clone(), name, mime)
    };

    let device = CastDeviceInfo {
        id: "probe".into(),
        name: "probe".into(),
        model: String::new(),
        ip: target.clone(),
        port: 8009,
        video: None,
    };
    let ip: IpAddr = target.parse().expect("FP_CAST_LOAD=<device ip>");

    let service = Arc::new(CastService::default());
    let (srv_ip, port, shared) = ensure_server(&service, ip).await.expect("server binds");
    let token = format!("{:032x}", rand::random::<u128>());
    let url = format!("http://{srv_ip}:{port}/c/{token}/{name}");
    {
        let mut reg = shared.reg.lock().unwrap();
        *reg = Some(Registered {
            token,
            path: serve_root.clone(),
            mime,
            dir: is_hls,
            torrent: None,
            caption: None,
        });
    }
    println!(
        "[probe] serving {} as {url}\n[probe] mime={mime} hls={hls_format:?}",
        serve_root.display()
    );

    let status = Arc::new(Mutex::new(StatusInner {
        state: "connecting".into(),
        rate: 1.0,
        ..Default::default()
    }));
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let session = tauri::async_runtime::spawn(run_session(device, status.clone(), cmd_rx));

    cmd_tx
        .send(Cmd::Load {
            url,
            mime,
            title: Some("probe".into()),
            position: 0.0,
            hls_format,
        })
        .expect("session takes the load");

    let mut last = String::new();
    for _ in 0..(seconds * 2) {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let s = status.lock().unwrap();
        let line = format!(
            "state={} error={:?} t={:.1} dur={:.1} fetches={}",
            s.state,
            s.error,
            s.media_time,
            s.duration,
            shared.hits.load(Ordering::Relaxed)
        );
        drop(s);
        if line != last {
            println!("[probe] {line}");
            last = line;
        }
    }

    let fetches = shared.hits.load(Ordering::Relaxed);
    let final_state = {
        let s = status.lock().unwrap();
        format!("{} {:?}", s.state, s.error)
    };
    println!("[probe] VERDICT: {final_state}, {fetches} HTTP requests served");
    let _ = cmd_tx.send(Cmd::Disconnect);
    tokio::time::sleep(Duration::from_millis(700)).await;
    session.abort();
}

#[test]
fn hidden_name_is_the_token() {
    // The privacy contract for the URL: a hidden file's basename is the
    // token plus the extension, never the real name. Exercised through the
    // same formatting cast_load uses.
    let token = format!("{:032x}", 0xdead_beefu128);
    let name = format!("{token}.mkv");
    assert!(!name.contains("My Private Film"));
    assert!(name.ends_with(".mkv"));
    assert_eq!(name.len(), 32 + 4);
}
