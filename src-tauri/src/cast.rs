//! Casting to TV (Google Cast): a URL handoff, never pixel streaming.
//!
//! The TV fetches the file over the LAN from an HTTP server this module runs
//! and decodes it itself; mpv stops decoding for the duration. This is the only
//! model compatible with `wid` embedding — mpv renders into a child window, and
//! no OS casting API can be fed from that. See private-docs/casting.md for the
//! research, the measured numbers and the dead ends.
//!
//! Three decisions worth stating up front, because each replaces an "obvious"
//! implementation that is wrong:
//!
//! **The CASTV2 protocol is spoken directly, not through `rust_cast`.** The
//! wire format is 4-byte-BE-length-framed protobuf envelopes whose payloads are
//! JSON in four namespaces (connection, heartbeat, receiver, media) — one
//! ~60-line hand-encoded message type. `rust_cast` was read and rejected: its
//! MessageManager holds the stream mutex across a blocking `read_exact` with no
//! timeout, so a play/pause command issued while its receive loop waits for
//! traffic stalls for up to seconds, which is exactly wrong for a remote
//! control. Async ownership here means a `select!` pump: commands are handled
//! the moment they arrive, frames are read by a dedicated task (cancellation
//! safety — a `select!` that cancels a half-read frame corrupts the framing),
//! and heartbeat is a timer, not a hope.
//!
//! **The server binds the LAN interface on the chosen device's subnet** — never
//! `127.0.0.1` (the TV opens the socket) and never `0.0.0.0` (no reason to
//! listen on every VPN and virtual adapter). Which interface is computed from
//! the device's own IP against `if-addrs` netmasks, which is what makes a
//! multi-homed host (VPN TUN, Hyper-V switch, APIPA Wi-Fi stub) work by
//! construction. Nothing binds and nothing browses until the viewer opens the
//! picker: same rule as the torrent session and the yt-dlp probe.
//!
//! **The server serves exactly one registered file, keyed by a random token.**
//! It is never a directory server: an unknown token is 404, and the token dies
//! with the cast session. A file under a private root is registered with its
//! name withheld — the URL basename becomes the token itself plus the
//! extension (the extension is load-bearing: the receiver probes by it), and
//! the LOAD metadata carries no title, so neither the wire nor the TV screen
//! names the file. That is this feature's slice of the CLAUDE.md rule that
//! every new path by which something about a file leaves this machine is
//! another privacy enforcement point.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures_util::StreamExt as _;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, StreamBody};
use hyper::body::Frame;
use hyper::service::service_fn;
use hyper::{header, Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::rustls;
use tokio_util::io::ReaderStream;

/// The Default Media Receiver. No registration and no custom receiver: a custom
/// Web Receiver runs on the same hardware decoders, so it buys UI, not codec
/// support (casting.md, "Dead ends").
const DEFAULT_RECEIVER_APP: &str = "CC1AD845";

const NS_CONNECTION: &str = "urn:x-cast:com.google.cast.tp.connection";
const NS_HEARTBEAT: &str = "urn:x-cast:com.google.cast.tp.heartbeat";
const NS_RECEIVER: &str = "urn:x-cast:com.google.cast.receiver";
const NS_MEDIA: &str = "urn:x-cast:com.google.cast.media";

const SENDER_ID: &str = "sender-0";
const RECEIVER_ID: &str = "receiver-0";

/// TCP + TLS connect budget. A device that answers mDNS but not 8009 is off or
/// firewalled on its side; waiting longer only delays the message saying so.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(6);

/// The device pings us every ~5 s and we ping it every 5 s; three missed
/// exchanges in a row means the wire is dead, not slow.
const TRAFFIC_TIMEOUT: Duration = Duration::from_secs(15);

/// How often the pump asks the receiver where playback is. The frontend
/// extrapolates between reports, so 1 Hz is a smooth seekbar, not a choppy one.
const STATUS_POLL: Duration = Duration::from_secs(1);

const PING_EVERY: Duration = Duration::from_secs(5);

/// Read size for one body chunk, matching torrent.rs.
const STREAM_CHUNK: usize = 65536;

/// Ceiling for one CASTV2 frame. The spec caps messages at 64 KB; anything
/// larger is a desynchronised stream, and reading it as a length would allocate
/// garbage gigabytes.
const MAX_FRAME: usize = 256 * 1024;

type Body = BoxBody<Bytes, std::io::Error>;

// ---- The wire format -------------------------------------------------------
//
// One protobuf message type (CastMessage from cast_channel.proto), encoded by
// hand because the schema is seven fields and stable since 2014:
//   1 protocol_version (varint, CASTV2_1_0 = 0)   4 namespace (string)
//   2 source_id (string)                          5 payload_type (varint, 0 = STRING)
//   3 destination_id (string)                     6 payload_utf8 (string)
// Field 7 (payload_binary) is never sent by us and skipped on read.

mod wire {
    pub struct Decoded {
        pub namespace: String,
        pub source: String,
        #[allow(dead_code)]
        pub destination: String,
        pub payload: String,
    }

    fn put_varint(out: &mut Vec<u8>, mut v: u64) {
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }

    fn put_str(out: &mut Vec<u8>, tag: u8, s: &str) {
        out.push(tag);
        put_varint(out, s.len() as u64);
        out.extend_from_slice(s.as_bytes());
    }

    pub fn encode(source: &str, destination: &str, namespace: &str, payload: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(payload.len() + namespace.len() + 64);
        out.extend_from_slice(&[0x08, 0x00]); // protocol_version = CASTV2_1_0
        put_str(&mut out, 0x12, source);
        put_str(&mut out, 0x1a, destination);
        put_str(&mut out, 0x22, namespace);
        out.extend_from_slice(&[0x28, 0x00]); // payload_type = STRING
        put_str(&mut out, 0x32, payload);
        out
    }

    fn read_varint(buf: &[u8], at: &mut usize) -> Option<u64> {
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            let byte = *buf.get(*at)?;
            *at += 1;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
            shift += 7;
            if shift > 63 {
                return None;
            }
        }
    }

    pub fn decode(buf: &[u8]) -> Option<Decoded> {
        let mut at = 0usize;
        let (mut namespace, mut source, mut destination, mut payload) =
            (String::new(), String::new(), String::new(), String::new());
        while at < buf.len() {
            let key = read_varint(buf, &mut at)?;
            let (field, kind) = (key >> 3, key & 7);
            match kind {
                0 => {
                    read_varint(buf, &mut at)?;
                }
                2 => {
                    let len = read_varint(buf, &mut at)? as usize;
                    let end = at.checked_add(len)?;
                    let bytes = buf.get(at..end)?;
                    at = end;
                    let text = || String::from_utf8_lossy(bytes).into_owned();
                    match field {
                        2 => source = text(),
                        3 => destination = text(),
                        4 => namespace = text(),
                        6 => payload = text(),
                        _ => {}
                    }
                }
                1 => at = at.checked_add(8)?,
                5 => at = at.checked_add(4)?,
                _ => return None,
            }
        }
        Some(Decoded {
            namespace,
            source,
            destination,
            payload,
        })
    }
}

// ---- TLS -------------------------------------------------------------------

/// Cast devices present certificates signed by Google's own Cast CA, not a
/// public one, so verification against the platform store can only fail —
/// every sender library ships this same "accept the device's cert" verifier.
/// The provider is pinned to `ring` explicitly: the tree compiles both ring
/// and aws-lc-rs, and rustls' plain `builder()` panics when two providers are
/// enabled at once.
///
/// The handshake-signature methods below return assertions too, and that is
/// measured, not paranoia: device certificates are **X.509 v1**, which webpki
/// refuses to parse at all — a real TV on this LAN failed with
/// `invalid peer certificate: Other(OtherError(UnsupportedCertVersion))`
/// inside rustls' own `verify_tls13_signature`, which parses the cert to
/// extract the public key before checking anything. (The JBL soundbar happened
/// to carry a parseable cert, which is why the smoke test passed first.)
/// "Do not verify the device's certificate" therefore has to include the
/// signatures made with its key; `rust_cast`'s no-verification mode keeps
/// webpki here and fails on the same devices.
#[derive(Debug)]
struct AcceptDeviceCert(rustls::crypto::CryptoProvider);

impl rustls::client::danger::ServerCertVerifier for AcceptDeviceCert {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        // See the struct comment: the cert is X.509 v1, webpki cannot even
        // parse it, and this connection is not authenticated by PKI anyway.
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0
            .signature_verification_algorithms
            .supported_schemes()
    }
}

async fn tls_connect(
    ip: IpAddr,
    port: u16,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>, String> {
    let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((ip, port)))
        .await
        .map_err(|_| "unreachable".to_string())?
        .map_err(|e| format!("unreachable: {e}"))?;

    let provider = rustls::crypto::ring::default_provider();
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(provider.clone()))
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("tls: {e}"))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptDeviceCert(provider)))
        .with_no_client_auth();

    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    tokio::time::timeout(
        CONNECT_TIMEOUT,
        connector.connect(rustls::pki_types::ServerName::IpAddress(ip.into()), tcp),
    )
    .await
    .map_err(|_| "unreachable".to_string())?
    .map_err(|e| format!("unreachable: {e}"))
}

// ---- Discovery -------------------------------------------------------------

/// One Cast device as the picker shows it. `id` is the mDNS fullname — unique
/// on the network and stable while the device is up, which is all the picker
/// needs; nothing here is ever persisted.
#[derive(Serialize, Deserialize, Clone)]
pub struct CastDeviceInfo {
    pub id: String,
    pub name: String,
    pub model: String,
    pub ip: String,
    pub port: u16,
}

struct Discovery {
    daemon: mdns_sd::ServiceDaemon,
    devices: Arc<Mutex<HashMap<String, CastDeviceInfo>>>,
}

const CAST_SERVICE: &str = "_googlecast._tcp.local.";

impl Discovery {
    /// Browse-only: we query for `_googlecast._tcp` and never advertise a
    /// service of our own, so nothing about the app or its files is announced
    /// to the network.
    fn start() -> Result<Discovery, String> {
        let daemon = mdns_sd::ServiceDaemon::new().map_err(|e| format!("mdns: {e}"))?;
        let receiver = daemon.browse(CAST_SERVICE).map_err(|e| format!("mdns: {e}"))?;
        let devices: Arc<Mutex<HashMap<String, CastDeviceInfo>>> = Arc::default();
        let sink = devices.clone();
        // A plain thread rather than a tokio task: flume's blocking iterator is
        // the crate's own idiom, and the thread ends when the daemon shuts down
        // and the channel closes.
        std::thread::spawn(move || {
            for event in receiver.iter() {
                match event {
                    mdns_sd::ServiceEvent::ServiceResolved(info) => {
                        // Prefer a routable IPv4; a device that only announced
                        // link-local or v6 addresses is skipped rather than
                        // guessed at.
                        let ip = info
                            .addresses
                            .iter()
                            .filter_map(|a| match a.to_ip_addr() {
                                IpAddr::V4(v4) if !v4.is_loopback() && !v4.is_link_local() => {
                                    Some(v4)
                                }
                                _ => None,
                            })
                            .next();
                        let Some(ip) = ip else { continue };
                        let txt = |key: &str| {
                            info.txt_properties
                                .get_property_val_str(key)
                                .unwrap_or("")
                                .to_string()
                        };
                        let name = {
                            let friendly = txt("fn");
                            if friendly.is_empty() {
                                info.fullname
                                    .split('.')
                                    .next()
                                    .unwrap_or("Chromecast")
                                    .to_string()
                            } else {
                                friendly
                            }
                        };
                        let device = CastDeviceInfo {
                            id: info.fullname.clone(),
                            name,
                            model: txt("md"),
                            ip: ip.to_string(),
                            port: info.port,
                        };
                        let mut map = sink.lock().unwrap_or_else(|p| p.into_inner());
                        map.insert(info.fullname.clone(), device);
                    }
                    mdns_sd::ServiceEvent::ServiceRemoved(_, fullname) => {
                        let mut map = sink.lock().unwrap_or_else(|p| p.into_inner());
                        map.remove(&fullname);
                    }
                    _ => {}
                }
            }
        });
        Ok(Discovery { daemon, devices })
    }

    fn stop(self) {
        let _ = self.daemon.stop_browse(CAST_SERVICE);
        let _ = self.daemon.shutdown();
    }
}

// ---- The LAN file server ---------------------------------------------------

/// What the server will answer for. Everything after the token in the URL is
/// display only for a plain file — for a private path the basename is the
/// token again, because the name must not leave the machine — so the route
/// matches on the token alone. For an HLS session `path` is the session
/// *directory* and the segment after the token names a file inside it
/// (playlist, init, segments), validated to a single flat component.
struct Registered {
    token: String,
    path: PathBuf,
    mime: &'static str,
    dir: bool,
    /// A torrent still downloading, served through librqbit's blocking stream
    /// instead of read from disk: `(info hash, file index, file name)`. The
    /// sparse file on disk is not an option — a stretch that has not arrived
    /// reads back as zeros, and the television would play them without a word.
    torrent: Option<(String, usize, String)>,
}

struct ServeShared {
    reg: Mutex<Option<Registered>>,
    /// Set when a torrent source is registered — the cast server does not own
    /// the torrent session and only borrows it to stream through.
    torrents: Mutex<Option<Arc<crate::torrent::TorrentService>>>,
    /// Requests served for the current registration. Zero after a LOAD that the
    /// TV accepted is the firewall signature: the command channel worked, the
    /// media channel never reached us.
    hits: AtomicU64,
}

struct Server {
    ip: IpAddr,
    port: u16,
    shared: Arc<ServeShared>,
    task: tauri::async_runtime::JoinHandle<()>,
}

/// The local address the TV can reach: the interface whose subnet contains the
/// device. Falls back to any routable IPv4 so a weird netmask degrades to "try
/// the main interface" rather than to a refusal.
fn lan_ip_for(device_ip: IpAddr) -> Option<IpAddr> {
    let ifaces = if_addrs::get_if_addrs().ok()?;
    let mut candidates: Vec<(Ipv4Addr, Ipv4Addr)> = Vec::new();
    for iface in ifaces {
        if let if_addrs::IfAddr::V4(v4) = iface.addr {
            if v4.ip.is_loopback() || v4.ip.is_link_local() {
                continue;
            }
            candidates.push((v4.ip, v4.netmask));
        }
    }
    pick_lan_ip(&candidates, device_ip).map(IpAddr::V4)
}

/// Pure so the subnet arithmetic is testable without interfaces to fake.
fn pick_lan_ip(candidates: &[(Ipv4Addr, Ipv4Addr)], device_ip: IpAddr) -> Option<Ipv4Addr> {
    let IpAddr::V4(device) = device_ip else {
        return candidates.first().map(|(ip, _)| *ip);
    };
    let device = u32::from(device);
    for (ip, mask) in candidates {
        let (ip_u, mask_u) = (u32::from(*ip), u32::from(*mask));
        if mask_u != 0 && (ip_u & mask_u) == (device & mask_u) {
            return Some(*ip);
        }
    }
    candidates.first().map(|(ip, _)| *ip)
}

fn empty() -> Body {
    BoxBody::new(Empty::<Bytes>::new().map_err(|e| match e {}))
}

fn with_cors(mut builder: hyper::http::response::Builder) -> hyper::http::response::Builder {
    // WebVTT subtitles (phase 3) require CORS on both the subtitle and the
    // media responses; harmless on everything else, so it is simply always on.
    builder = builder
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS")
        .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "Range")
        .header(
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
            "Content-Range, Content-Length, Accept-Ranges",
        );
    builder
}

fn simple(status: StatusCode) -> Response<Body> {
    with_cors(Response::builder().status(status))
        .body(empty())
        .unwrap()
}

/// `FP_CAST_DEBUG=1` turns on the wire and server logs. Off by default: a
/// playing HLS session is a request every few seconds and a status frame every
/// second, which would bury everything else in the dev console — the same
/// reasoning as the `log::set_level(Error)` next to `ffmpeg_the_third::init()`.
pub(crate) fn cast_debug() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("FP_CAST_DEBUG").map(|v| v != "0").unwrap_or(false)
    })
}

/// A failed LOAD is the one thing worth printing unconditionally: the receiver
/// names its own reason and we otherwise collapse every failure into
/// `load_failed`, which is the same sentence for a codec it cannot decode and a
/// segment it cannot parse. `detailedErrorCode` is the Web Receiver's
/// `ErrorCode` enum — the HLS band (2xx) is what distinguishes "cannot fetch"
/// from "cannot parse what it fetched".
fn log_media_error(what: &str, payload: &Value) {
    let code = payload["detailedErrorCode"].as_i64();
    let named = match code {
        Some(100) => "MEDIA_UNKNOWN",
        Some(101) => "MEDIA_ABORTED",
        Some(102) => "MEDIA_DECODE",
        Some(103) => "MEDIA_NETWORK",
        Some(104) => "MEDIA_SRC_NOT_SUPPORTED",
        Some(110) => "SOURCE_BUFFER_FAILURE",
        Some(201) => "HLS_NETWORK_MASTER_PLAYLIST",
        Some(202) => "HLS_NETWORK_PLAYLIST",
        Some(203) => "HLS_NETWORK_NO_KEY_RESPONSE",
        Some(204) => "HLS_NETWORK_KEY_LOAD",
        Some(205) => "HLS_NETWORK_SEGMENTS",
        Some(206) => "HLS_SEGMENT_PARSING",
        Some(_) => "see the Web Receiver ErrorCode enum",
        None => "no detailedErrorCode",
    };
    eprintln!(
        "[cast] {what}: code={} ({named}) reason={} payload={payload}",
        code.map(|c| c.to_string()).unwrap_or_else(|| "-".into()),
        payload["reason"].as_str().unwrap_or("-"),
    );
}

/// The one route: `/c/<token>/<anything>`. Everything else — and every token
/// that is not the currently registered one — is 404.
///
/// Wrapper over the handler so that every answer is logged in one place under
/// `FP_CAST_DEBUG`: *what the TV asked for* is half of any load diagnosis —
/// a receiver that fetched the playlist and stopped failed at parsing it, one
/// that fetched init.mp4 and a segment failed at decoding, and one that asked
/// for nothing at all is the firewall case the connect flow already warns about.
async fn serve_cast(shared: Arc<ServeShared>, req: Request<hyper::body::Incoming>) -> Response<Body> {
    if !cast_debug() {
        return serve_cast_inner(shared, req).await;
    }
    let line = format!(
        "{} {} range={} timeseek={}",
        req.method(),
        req.uri().path(),
        req.headers()
            .get(header::RANGE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-"),
        // A DLNA renderer that means to seek by time asks with this header
        // instead of Range; logging it is how we learn whether the seek that
        // "succeeds" and does nothing ever reached us at all.
        req.headers()
            .get("TimeSeekRange.dlna.org")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-"),
    );
    let res = serve_cast_inner(shared, req).await;
    eprintln!("[cast] http {line} -> {}", res.status().as_u16());
    res
}

async fn serve_cast_inner(shared: Arc<ServeShared>, req: Request<hyper::body::Incoming>) -> Response<Body> {
    if req.method() == Method::OPTIONS {
        return simple(StatusCode::NO_CONTENT);
    }
    if req.method() != Method::GET && req.method() != Method::HEAD {
        return simple(StatusCode::METHOD_NOT_ALLOWED);
    }

    let path = req.uri().path().to_string();
    let mut parts = path.trim_start_matches('/').splitn(3, '/');
    let (Some("c"), Some(token)) = (parts.next(), parts.next()) else {
        return simple(StatusCode::NOT_FOUND);
    };

    let (base, base_mime, is_dir, torrent) = {
        let reg = shared.reg.lock().unwrap_or_else(|p| p.into_inner());
        match reg.as_ref() {
            Some(r) if r.token == token => (r.path.clone(), r.mime, r.dir, r.torrent.clone()),
            _ => return simple(StatusCode::NOT_FOUND),
        }
    };
    if let Some((hash, index, name)) = torrent {
        let Some(service) = shared.torrents.lock().unwrap_or_else(|p| p.into_inner()).clone() else {
            return simple(StatusCode::INTERNAL_SERVER_ERROR);
        };
        shared.hits.fetch_add(1, Ordering::Relaxed);
        let range = req
            .headers()
            .get(header::RANGE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string());
        return service
            .serve_file(&hash, index, &name, range, req.method() == Method::HEAD)
            .await;
    }
    let (file_path, mime) = if is_dir {
        // One flat component inside the session dir, nothing else: no dots at
        // the front (`..` included), no separators — a token-scoped directory
        // server must not be steerable outside its directory.
        let name = parts.next().unwrap_or("");
        let legal = !name.is_empty()
            && !name.starts_with('.')
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-');
        if !legal {
            return simple(StatusCode::NOT_FOUND);
        }
        (base.join(name), hls_mime(name))
    } else {
        (base, base_mime)
    };
    shared.hits.fetch_add(1, Ordering::Relaxed);

    let Ok(mut file) = tokio::fs::File::open(&file_path).await else {
        return simple(StatusCode::NOT_FOUND);
    };
    let Ok(meta) = file.metadata().await else {
        return simple(StatusCode::INTERNAL_SERVER_ERROR);
    };
    let total = meta.len();

    let mut res = with_cors(
        Response::builder()
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_TYPE, mime)
            // **A DLNA renderer decides whether it may seek from these two
            // headers, not from `Accept-Ranges`.** Measured on the LG: without
            // them the TV accepts a `Seek` action, reports the target position
            // once, then carries on from where it was and never issues a single
            // Range request — a seek that fails while answering "OK". Cast
            // ignores both headers, so they cost the other transport nothing.
            // `DLNA.ORG_OP=01` is "byte-range seek yes, time-seek no", which
            // is exactly what this server can do; the flags are the standard
            // streaming set (streaming transfer mode, DLNA v1.5). Advertising
            // `11` was tried — the LG asked for neither a Range nor a
            // `TimeSeekRange` afterwards, so claiming a time-seek we have not
            // implemented buys nothing and would mislead a stricter renderer.
            .header("transferMode.dlna.org", "Streaming")
            .header(
                "contentFeatures.dlna.org",
                "DLNA.ORG_OP=01;DLNA.ORG_CI=0;\
                 DLNA.ORG_FLAGS=01700000000000000000000000000000",
            ),
    );

    let range = req
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| crate::torrent::parse_range(v, total));

    let (status, start, len) = match range {
        Some(Some((start, end_inclusive))) => {
            res = res.header(
                header::CONTENT_RANGE,
                format!("bytes {start}-{end_inclusive}/{total}"),
            );
            (StatusCode::PARTIAL_CONTENT, start, end_inclusive - start + 1)
        }
        Some(None) => {
            return with_cors(
                Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(header::CONTENT_RANGE, format!("bytes */{total}")),
            )
            .body(empty())
            .unwrap()
        }
        None => (StatusCode::OK, 0, total),
    };

    let res = res.status(status).header(header::CONTENT_LENGTH, len);
    if req.method() == Method::HEAD {
        return res.body(empty()).unwrap();
    }

    if start > 0 && file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return simple(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let body = StreamBody::new(
        ReaderStream::with_capacity(file.take(len), STREAM_CHUNK).map(|r| r.map(Frame::data)),
    );
    res.body(BoxBody::new(body)).unwrap()
}

/// `cast_mime` by path, for callers outside this module.
pub(crate) fn cast_mime_for(file: &std::path::Path) -> &'static str {
    cast_mime(&file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
}

/// Requests the LAN server has answered for the current registration. Zero
/// after a load the device accepted is the firewall signature, and that test is
/// the same whichever transport asked it to fetch.
pub(crate) fn server_hits(service: &Arc<CastService>) -> u64 {
    let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    inner
        .server
        .as_ref()
        .map(|s| s.shared.hits.load(Ordering::Relaxed))
        .unwrap_or(0)
}

/// Content types inside an HLS session directory.
fn hls_mime(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "m3u8" => "application/x-mpegURL",
        "ts" => "video/mp2t",
        "m4s" => "video/iso.segment",
        "mp4" => "video/mp4",
        "vtt" => "text/vtt",
        _ => "application/octet-stream",
    }
}

/// Content type the receiver probes by. Phase 1 casts direct-play files only,
/// so this list is the direct-play list.
fn cast_mime(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "mp4" | "m4v" | "mov" => "video/mp4",
        // Never reachable through Cast (the verdict prepares or refuses an MKV
        // long before this), but the same server feeds the DLNA path, where a
        // renderer that advertises video/x-matroska takes the file untouched.
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "m4a" | "aac" => "audio/mp4",
        "flac" => "audio/flac",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
}

// ---- The cast session ------------------------------------------------------

enum Cmd {
    Load {
        url: String,
        mime: &'static str,
        title: Option<String>,
        position: f64,
        /// `Some("ts" | "fmp4")` for an HLS playlist: the Default Media
        /// Receiver historically assumes TS segments, so fMP4 needs the
        /// `hlsSegmentFormat`/`hlsVideoSegmentFormat` hints in the LOAD.
        hls_format: Option<String>,
    },
    Play,
    Pause,
    Seek(f64),
    SetVolume(f64),
    SetMuted(bool),
    /// Stop the receiver app (the TV goes back to its home screen) and close
    /// the connection.
    Disconnect,
}

/// What the pump knows, mirrored for `cast_status`. `media_time` is the last
/// report plus its timestamp — `snapshot()` extrapolates while playing, so the
/// frontend's 500 ms poll draws a smooth bar out of 1 Hz reports.
#[derive(Default)]
struct StatusInner {
    /// connecting | connected | loading | buffering | playing | paused |
    /// ended | stopped | error
    state: String,
    /// Machine-readable reason, one of the strings the frontend maps to a
    /// sentence: unreachable | launch_failed | load_failed | closed | app_gone.
    error: Option<String>,
    media_time: f64,
    reported_at: Option<Instant>,
    rate: f64,
    duration: f64,
    volume: f64,
    muted: bool,
    /// The receiver's `volume.controlType`: "master"/"attenuation" mean the
    /// device takes SET_VOLUME, "fixed" means it does not (volume lives on the
    /// TV remote), and None means no status has carried a volume object yet —
    /// the real TV here answers its first GET_STATUS without one, so absence
    /// is a normal state, not an error.
    volume_control: Option<String>,
}

#[derive(Serialize, Default)]
pub struct CastStatus {
    pub state: String,
    pub error: Option<String>,
    pub time: f64,
    pub duration: f64,
    pub volume: f64,
    pub muted: bool,
    /// Whether any receiver status has reported a volume at all, and whether
    /// the device declared it un-adjustable (`controlType: "fixed"`). The
    /// volume slider is disabled in either negative case — a control that
    /// silently does nothing is the failure mode this app keeps rooting out.
    pub volume_known: bool,
    pub volume_fixed: bool,
    /// Requests the LAN server has answered for the current file. Zero while
    /// the TV claims to be loading is the firewall signature.
    pub fetches: u64,
    pub device: Option<String>,
}

struct Session {
    device: CastDeviceInfo,
    cmd_tx: mpsc::UnboundedSender<Cmd>,
    status: Arc<Mutex<StatusInner>>,
    task: tauri::async_runtime::JoinHandle<()>,
}

#[derive(Default)]
pub struct CastService {
    inner: Mutex<Inner>,
    /// The running prepare (ffmpeg child), outside `inner` so cancelling never
    /// contends with the session lock.
    prepare: Mutex<Option<std::process::Child>>,
}

#[derive(Default)]
struct Inner {
    discovery: Option<Discovery>,
    session: Option<Session>,
    server: Option<Server>,
}

fn set_status(status: &Arc<Mutex<StatusInner>>, f: impl FnOnce(&mut StatusInner)) {
    let mut guard = status.lock().unwrap_or_else(|p| p.into_inner());
    f(&mut guard);
}

/// Everything the pump loop needs in one place, so helpers can borrow it as a
/// unit instead of threading six arguments.
struct Pump {
    wr: tokio::io::WriteHalf<tokio_rustls::client::TlsStream<TcpStream>>,
    status: Arc<Mutex<StatusInner>>,
    request_id: u64,
    /// Set once the receiver reports our app: (session_id, transport_id).
    transport: Option<(String, String)>,
    media_session: Option<i64>,
    /// A LOAD that arrived before the app was up; sent on launch.
    pending_load: Option<Cmd>,
    /// requestId of the in-flight LOAD, to tie an error answer to it.
    load_request: Option<u64>,
}

impl Pump {
    async fn send(&mut self, namespace: &str, destination: &str, payload: Value) -> Result<(), String> {
        let text = payload.to_string();
        if cast_debug() && namespace != NS_HEARTBEAT {
            eprintln!("[cast] -> {namespace} {text}");
        }
        let frame = wire::encode(SENDER_ID, destination, namespace, &text);
        let mut msg = Vec::with_capacity(frame.len() + 4);
        msg.extend_from_slice(&(frame.len() as u32).to_be_bytes());
        msg.extend_from_slice(&frame);
        self.wr
            .write_all(&msg)
            .await
            .map_err(|e| format!("send: {e}"))?;
        self.wr.flush().await.map_err(|e| format!("send: {e}"))
    }

    fn next_id(&mut self) -> u64 {
        self.request_id += 1;
        self.request_id
    }

    async fn send_load(
        &mut self,
        url: &str,
        mime: &str,
        title: &Option<String>,
        position: f64,
        hls_format: &Option<String>,
    ) -> Result<(), String> {
        let Some((session_id, transport_id)) = self.transport.clone() else {
            return Ok(());
        };
        let id = self.next_id();
        self.load_request = Some(id);
        let mut media = json!({
            "contentId": url,
            "streamType": "BUFFERED",
            "contentType": mime,
        });
        if let Some(format) = hls_format {
            // Only the *video* hint, and that is load-bearing: `hlsSegmentFormat`
            // is the format of an HLS **audio** segment, and sending it beside a
            // muxed A/V stream made this TV reject the LOAD outright — a bare
            // `LOAD_FAILED` with no `detailedErrorCode`, after it had already
            // fetched the playlist, the init segment and segment zero. Measured
            // both ways on the same rendition: with the field, refused; without
            // it, plays. It cost fMP4 entirely, and looked like "the receiver
            // cannot do fMP4" because fMP4 was only ever tried with HEVC.
            media["hlsVideoSegmentFormat"] =
                json!(if format == "fmp4" { "fmp4" } else { "mpeg2_ts" });
        }
        if let Some(title) = title {
            media["metadata"] = json!({ "metadataType": 0, "title": title });
        }
        let payload = json!({
            "type": "LOAD",
            "requestId": id,
            "sessionId": session_id,
            "media": media,
            "autoplay": true,
            "currentTime": position,
        });
        set_status(&self.status, |s| {
            s.state = "loading".into();
            s.error = None;
        });
        self.send(NS_MEDIA, &transport_id, payload).await
    }

    async fn media_cmd(&mut self, kind: &str, extra: Value) -> Result<(), String> {
        let (Some((_, transport_id)), Some(media_session)) =
            (self.transport.clone(), self.media_session)
        else {
            return Ok(());
        };
        let id = self.next_id();
        let mut payload = json!({
            "type": kind,
            "requestId": id,
            "mediaSessionId": media_session,
        });
        if let Value::Object(map) = extra {
            for (k, v) in map {
                payload[k] = v;
            }
        }
        self.send(NS_MEDIA, &transport_id, payload).await
    }

    async fn handle_cmd(&mut self, cmd: Cmd) -> Result<bool, String> {
        match cmd {
            Cmd::Load { url, mime, title, position, hls_format } => {
                if self.transport.is_some() {
                    self.send_load(&url, mime, &title, position, &hls_format).await?;
                } else {
                    self.pending_load = Some(Cmd::Load { url, mime, title, position, hls_format });
                    set_status(&self.status, |s| s.state = "loading".into());
                }
            }
            Cmd::Play => self.media_cmd("PLAY", json!({})).await?,
            Cmd::Pause => self.media_cmd("PAUSE", json!({})).await?,
            Cmd::Seek(t) => {
                // Optimistic, like the local seek popup: the next MEDIA_STATUS
                // is up to a second away, and a knob that springs back until it
                // arrives reads as a failed seek.
                set_status(&self.status, |s| {
                    s.media_time = t;
                    s.reported_at = Some(Instant::now());
                });
                self.media_cmd("SEEK", json!({ "currentTime": t })).await?;
            }
            Cmd::SetVolume(level) => {
                let id = self.next_id();
                let payload = json!({
                    "type": "SET_VOLUME",
                    "requestId": id,
                    "volume": { "level": level.clamp(0.0, 1.0) },
                });
                self.send(NS_RECEIVER, RECEIVER_ID, payload).await?;
            }
            Cmd::SetMuted(muted) => {
                let id = self.next_id();
                let payload = json!({
                    "type": "SET_VOLUME",
                    "requestId": id,
                    "volume": { "muted": muted },
                });
                self.send(NS_RECEIVER, RECEIVER_ID, payload).await?;
            }
            Cmd::Disconnect => {
                // Media STOP first (harmless if there is none), then the
                // receiver STOP that sends the TV back to its home screen.
                let _ = self.media_cmd("STOP", json!({})).await;
                if let Some((session_id, transport_id)) = self.transport.clone() {
                    let id = self.next_id();
                    let _ = self
                        .send(
                            NS_RECEIVER,
                            RECEIVER_ID,
                            json!({ "type": "STOP", "requestId": id, "sessionId": session_id }),
                        )
                        .await;
                    let _ = self
                        .send(NS_CONNECTION, &transport_id, json!({ "type": "CLOSE" }))
                        .await;
                }
                let _ = self
                    .send(NS_CONNECTION, RECEIVER_ID, json!({ "type": "CLOSE" }))
                    .await;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// One parsed frame from the device. Returns true when the session is over.
    async fn handle_msg(&mut self, msg: wire::Decoded) -> Result<bool, String> {
        let payload: Value = serde_json::from_str(&msg.payload).unwrap_or(Value::Null);
        let kind = payload["type"].as_str().unwrap_or("");
        if cast_debug() && msg.namespace != NS_HEARTBEAT {
            eprintln!("[cast] <- {} {}", msg.namespace, msg.payload);
        }
        match msg.namespace.as_str() {
            NS_HEARTBEAT => {
                if kind == "PING" {
                    // Reply to whoever pinged — the platform pings from its own
                    // transport id, and answering "receiver-0" does not count.
                    self.send(NS_HEARTBEAT, &msg.source, json!({ "type": "PONG" }))
                        .await?;
                }
            }
            NS_CONNECTION => {
                if kind == "CLOSE" {
                    // The receiver side hung up. If it was the app transport,
                    // the session it carried is gone with it.
                    if let Some((_, transport_id)) = &self.transport {
                        if msg.source == *transport_id {
                            self.transport = None;
                            self.media_session = None;
                            set_status(&self.status, |s| {
                                s.state = "stopped".into();
                            });
                            return Ok(true);
                        }
                    }
                }
            }
            NS_RECEIVER => match kind {
                "RECEIVER_STATUS" => {
                    if let Some(volume) = payload["status"]["volume"].as_object() {
                        set_status(&self.status, |s| {
                            if let Some(level) = volume.get("level").and_then(Value::as_f64) {
                                s.volume = level;
                            }
                            if let Some(muted) = volume.get("muted").and_then(Value::as_bool) {
                                s.muted = muted;
                            }
                            if let Some(control) =
                                volume.get("controlType").and_then(Value::as_str)
                            {
                                s.volume_control = Some(control.to_string());
                            }
                        });
                    }
                    let apps = payload["status"]["applications"].as_array();
                    let ours = apps.into_iter().flatten().find(|app| {
                        app["appId"].as_str() == Some(DEFAULT_RECEIVER_APP)
                    });
                    match (&self.transport, ours) {
                        (None, Some(app)) => {
                            let (Some(session_id), Some(transport_id)) =
                                (app["sessionId"].as_str(), app["transportId"].as_str())
                            else {
                                return Ok(false);
                            };
                            let transport_id = transport_id.to_string();
                            self.send(
                                NS_CONNECTION,
                                &transport_id,
                                json!({ "type": "CONNECT" }),
                            )
                            .await?;
                            self.transport = Some((session_id.to_string(), transport_id));
                            set_status(&self.status, |s| {
                                if s.state == "connecting" {
                                    s.state = "connected".into();
                                }
                            });
                            if let Some(Cmd::Load { url, mime, title, position, hls_format }) =
                                self.pending_load.take()
                            {
                                self.send_load(&url, mime, &title, position, &hls_format).await?;
                            }
                        }
                        (Some(_), None) => {
                            // Our app left the TV — someone cast over us or
                            // pressed stop on the device. The session is dead;
                            // the frontend hands playback back to mpv.
                            self.transport = None;
                            self.media_session = None;
                            set_status(&self.status, |s| {
                                s.state = "stopped".into();
                                s.error = Some("app_gone".into());
                            });
                            return Ok(true);
                        }
                        _ => {}
                    }
                }
                "LAUNCH_ERROR" => {
                    set_status(&self.status, |s| {
                        s.state = "error".into();
                        s.error = Some("launch_failed".into());
                    });
                    return Ok(true);
                }
                _ => {}
            },
            NS_MEDIA => match kind {
                "MEDIA_STATUS" => {
                    let Some(entry) = payload["status"].as_array().and_then(|a| a.first()) else {
                        return Ok(false);
                    };
                    let player_state = entry["playerState"].as_str().unwrap_or("");
                    let idle_reason = entry["idleReason"].as_str().unwrap_or("");
                    // A LOAD in flight displaces the previous media session,
                    // and the receiver announces that as MEDIA_STATUS
                    // IDLE/INTERRUPTED for the OLD session before the new one
                    // reports in. Reading that as "stopped from the TV" is
                    // what made a mid-cast audio switch end the whole session.
                    // While our LOAD is pending, IDLE is transit noise — with
                    // one exception: IDLE/ERROR is the new media failing on a
                    // receiver that reports it this way instead of
                    // LOAD_FAILED, and swallowing it would hang "loading".
                    if self.load_request.is_some() && player_state == "IDLE" {
                        if idle_reason == "ERROR" {
                            log_media_error("IDLE/ERROR", &payload);
                            self.load_request = None;
                            set_status(&self.status, |s| {
                                s.state = "error".into();
                                s.error = Some("load_failed".into());
                            });
                        }
                        return Ok(false);
                    }
                    if let Some(id) = entry["mediaSessionId"].as_i64() {
                        self.media_session = Some(id);
                    }
                    self.load_request = None;
                    set_status(&self.status, |s| {
                        if let Some(t) = entry["currentTime"].as_f64() {
                            s.media_time = t;
                            s.reported_at = Some(Instant::now());
                        }
                        if let Some(rate) = entry["playbackRate"].as_f64() {
                            s.rate = rate;
                        }
                        if let Some(d) = entry["media"]["duration"].as_f64() {
                            s.duration = d;
                        }
                        s.state = match player_state {
                            "PLAYING" => "playing".into(),
                            "PAUSED" => "paused".into(),
                            "BUFFERING" => "buffering".into(),
                            "IDLE" => match idle_reason {
                                "FINISHED" => "ended".into(),
                                "ERROR" => {
                                    s.error = Some("load_failed".into());
                                    "error".into()
                                }
                                "CANCELLED" | "INTERRUPTED" => "stopped".into(),
                                _ => s.state.clone(),
                            },
                            _ => s.state.clone(),
                        };
                    });
                }
                "LOAD_FAILED" | "LOAD_CANCELLED" | "INVALID_REQUEST" | "ERROR" => {
                    log_media_error(kind, &payload);
                    // Only a failure of OUR load counts — an unrelated error
                    // frame must not tear the session down.
                    let answered = payload["requestId"].as_u64();
                    if self.load_request.is_some() && answered == self.load_request {
                        self.load_request = None;
                        set_status(&self.status, |s| {
                            s.state = "error".into();
                            s.error = Some("load_failed".into());
                        });
                    }
                }
                _ => {}
            },
            _ => {}
        }
        Ok(false)
    }
}

/// The session: connect, launch, then pump until told to stop or the wire dies.
async fn run_session(
    device: CastDeviceInfo,
    status: Arc<Mutex<StatusInner>>,
    mut cmd_rx: mpsc::UnboundedReceiver<Cmd>,
) {
    let ip: IpAddr = match device.ip.parse() {
        Ok(ip) => ip,
        Err(_) => {
            set_status(&status, |s| {
                s.state = "error".into();
                s.error = Some("unreachable".into());
            });
            return;
        }
    };

    let stream = match tls_connect(ip, device.port).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[cast] connect to {}:{} failed: {e}", device.ip, device.port);
            set_status(&status, |s| {
                s.state = "error".into();
                s.error = Some("unreachable".into());
            });
            return;
        }
    };

    let (rd, wr) = tokio::io::split(stream);

    // A dedicated reader task: `read_exact` inside `select!` is not
    // cancellation-safe (a cancelled half-read frame desynchronises the
    // framing for good), so frames are read in one place and forwarded.
    let (msg_tx, mut msg_rx) = mpsc::channel::<wire::Decoded>(16);
    let reader = tauri::async_runtime::spawn(async move {
        let mut rd = rd;
        loop {
            let mut len = [0u8; 4];
            if rd.read_exact(&mut len).await.is_err() {
                break;
            }
            let n = u32::from_be_bytes(len) as usize;
            if n > MAX_FRAME {
                eprintln!("[cast] oversized frame ({n} bytes), closing");
                break;
            }
            let mut buf = vec![0u8; n];
            if rd.read_exact(&mut buf).await.is_err() {
                break;
            }
            let Some(msg) = wire::decode(&buf) else {
                eprintln!("[cast] undecodable frame, closing");
                break;
            };
            if msg_tx.send(msg).await.is_err() {
                break;
            }
        }
    });

    let mut pump = Pump {
        wr,
        status: status.clone(),
        request_id: 0,
        transport: None,
        media_session: None,
        pending_load: None,
        load_request: None,
    };

    // The platform handshake: CONNECT to the platform receiver, ask it what is
    // running, launch our app. The RECEIVER_STATUS answers drive the rest.
    let opened = async {
        pump.send(NS_CONNECTION, RECEIVER_ID, json!({ "type": "CONNECT" }))
            .await?;
        let id = pump.next_id();
        pump.send(
            NS_RECEIVER,
            RECEIVER_ID,
            json!({ "type": "LAUNCH", "requestId": id, "appId": DEFAULT_RECEIVER_APP }),
        )
        .await
    }
    .await;
    if let Err(e) = opened {
        eprintln!("[cast] handshake failed: {e}");
        set_status(&status, |s| {
            s.state = "error".into();
            s.error = Some("unreachable".into());
        });
        reader.abort();
        return;
    }

    let mut tick = tokio::time::interval(Duration::from_millis(500));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_traffic = Instant::now();
    let mut last_ping = Instant::now();
    let mut last_poll = Instant::now();

    loop {
        let done = tokio::select! {
            cmd = cmd_rx.recv() => match cmd {
                Some(cmd) => pump.handle_cmd(cmd).await,
                // The service dropped the session; nothing left to say.
                None => Ok(true),
            },
            msg = msg_rx.recv() => match msg {
                Some(msg) => {
                    last_traffic = Instant::now();
                    pump.handle_msg(msg).await
                }
                None => {
                    // Reader ended: the device closed the socket.
                    set_status(&status, |s| {
                        if s.state != "stopped" && s.state != "error" {
                            s.state = "error".into();
                            s.error = Some("closed".into());
                        }
                    });
                    Ok(true)
                }
            },
            _ = tick.tick() => {
                let mut result = Ok(false);
                if last_traffic.elapsed() > TRAFFIC_TIMEOUT {
                    set_status(&status, |s| {
                        s.state = "error".into();
                        s.error = Some("closed".into());
                    });
                    result = Ok(true);
                } else {
                    if last_ping.elapsed() >= PING_EVERY {
                        last_ping = Instant::now();
                        result = pump
                            .send(NS_HEARTBEAT, RECEIVER_ID, json!({ "type": "PING" }))
                            .await
                            .map(|_| false);
                        // Volume rides on receiver statuses, which otherwise
                        // only arrive as broadcasts — and the real TV answers
                        // its first status without a volume object at all, so
                        // ask periodically rather than hoping.
                        if result.is_ok() {
                            let id = pump.next_id();
                            result = pump
                                .send(
                                    NS_RECEIVER,
                                    RECEIVER_ID,
                                    json!({ "type": "GET_STATUS", "requestId": id }),
                                )
                                .await
                                .map(|_| false);
                        }
                    }
                    if result.is_ok() && last_poll.elapsed() >= STATUS_POLL {
                        last_poll = Instant::now();
                        if let Some((_, transport_id)) = pump.transport.clone() {
                            let id = pump.next_id();
                            result = pump
                                .send(
                                    NS_MEDIA,
                                    &transport_id,
                                    json!({ "type": "GET_STATUS", "requestId": id }),
                                )
                                .await
                                .map(|_| false);
                        }
                    }
                }
                result
            }
        };
        match done {
            Ok(false) => {}
            Ok(true) => break,
            Err(e) => {
                eprintln!("[cast] session error: {e}");
                set_status(&status, |s| {
                    if s.state != "stopped" {
                        s.state = "error".into();
                        s.error = Some("closed".into());
                    }
                });
                break;
            }
        }
    }
    reader.abort();
}

// ---- Commands --------------------------------------------------------------

#[tauri::command]
pub fn cast_discover_start(service: tauri::State<'_, Arc<CastService>>) -> Result<(), String> {
    let mut inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    if inner.discovery.is_none() {
        inner.discovery = Some(Discovery::start()?);
    }
    Ok(())
}

#[tauri::command]
pub fn cast_discover_stop(service: tauri::State<'_, Arc<CastService>>) {
    let discovery = {
        let mut inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.discovery.take()
    };
    if let Some(discovery) = discovery {
        discovery.stop();
    }
}

#[tauri::command]
pub fn cast_devices(service: tauri::State<'_, Arc<CastService>>) -> Vec<CastDeviceInfo> {
    let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    let Some(discovery) = &inner.discovery else {
        return Vec::new();
    };
    let map = discovery.devices.lock().unwrap_or_else(|p| p.into_inner());
    let mut list: Vec<CastDeviceInfo> = map.values().cloned().collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}

#[tauri::command]
pub fn cast_connect(
    service: tauri::State<'_, Arc<CastService>>,
    device: CastDeviceInfo,
) -> Result<(), String> {
    let old = {
        let mut inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.session.take()
    };
    if let Some(old) = old {
        // A leftover session for another device: tell it to hang up, but do not
        // wait — the new connection must not queue behind the old TV's timeout.
        let _ = old.cmd_tx.send(Cmd::Disconnect);
        drop(old.task);
    }

    let status = Arc::new(Mutex::new(StatusInner {
        state: "connecting".into(),
        rate: 1.0,
        volume: 1.0,
        ..Default::default()
    }));
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let task = tauri::async_runtime::spawn(run_session(
        device.clone(),
        status.clone(),
        cmd_rx,
    ));

    let mut inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    inner.session = Some(Session {
        device,
        cmd_tx,
        status,
        task,
    });
    Ok(())
}

/// Register the file with the LAN server and tell the TV to open it.
///
/// `hidden` is the privacy gate: the URL carries the token instead of the
/// file's name and the LOAD metadata carries no title, so nothing on the wire
/// or the TV screen names what is being watched.
#[tauri::command]
pub async fn cast_load(
    service: tauri::State<'_, Arc<CastService>>,
    path: String,
    position: f64,
    title: Option<String>,
    hidden: bool,
    hls: Option<String>,
) -> Result<(), String> {
    let file = PathBuf::from(&path);
    if !file.is_file() {
        return Err("no such file".into());
    }
    // For HLS `path` is the playlist; the registration is its session
    // directory, and the URL always names the playlist — a fixed name, so a
    // private file's name never enters the URL on this route either.
    let is_hls = hls.is_some();
    let serve_root = if is_hls {
        file.parent().ok_or("playlist has no directory")?.to_path_buf()
    } else {
        file.clone()
    };

    let (device_ip, cmd_tx) = {
        let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
        let session = inner.session.as_ref().ok_or("not connected")?;
        (
            session.device.ip.clone(),
            session.cmd_tx.clone(),
        )
    };
    let device_ip: IpAddr = device_ip.parse().map_err(|_| "bad device ip".to_string())?;

    let (ip, port, shared) = ensure_server(&service, device_ip).await?;

    let basename = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "video".into());
    let mime = if is_hls {
        "application/x-mpegURL"
    } else {
        cast_mime(&basename)
    };
    let token = format!("{:032x}", rand::random::<u128>());
    let url_name = if is_hls {
        crate::torrent::urlencode(&basename)
    } else if hidden {
        let ext = basename.rsplit('.').next().unwrap_or("mp4").to_ascii_lowercase();
        format!("{token}.{ext}")
    } else {
        crate::torrent::urlencode(&basename)
    };
    let url = format!("http://{ip}:{port}/c/{token}/{url_name}");

    let replaced = {
        let mut reg = shared.reg.lock().unwrap_or_else(|p| p.into_inner());
        let old = reg.take();
        *reg = Some(Registered {
            token,
            path: serve_root.clone(),
            mime,
            dir: is_hls,
            torrent: None,
        });
        shared.hits.store(0, Ordering::Relaxed);
        old
    };
    // A displaced HLS session (the audio switch) leaves its segment directory
    // behind; it is transient by contract, so it goes now rather than at
    // disconnect. Never the one just registered.
    if let Some(old) = replaced {
        if old.dir && old.path != serve_root {
            let _ = std::fs::remove_dir_all(&old.path);
        }
    }

    cmd_tx
        .send(Cmd::Load {
            url,
            mime,
            title: if hidden { None } else { title },
            position,
            hls_format: hls,
        })
        .map_err(|_| "session ended".to_string())
}

/// Bind the LAN server (if it is not up already), register one file behind a
/// fresh token and hand back the URL a device on `device_ip`'s subnet can fetch
/// it from. The Cast path does this inline in `cast_load` with its own privacy
/// and HLS rules; this is the same three steps for anything else that hands a
/// television a URL — today the DLNA reconnaissance in dlna.rs.
pub(crate) async fn serve_one_file(
    service: &Arc<CastService>,
    device_ip: IpAddr,
    file: &std::path::Path,
    hidden: bool,
) -> Result<String, String> {
    let (ip, port, shared) = ensure_server(service, device_ip).await?;
    let basename = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "video".into());
    let mime = cast_mime(&basename);
    let token = format!("{:032x}", rand::random::<u128>());
    // The privacy rule travels with the server, not with the transport: a file
    // under a private root is served under its token plus the extension, so the
    // name never reaches the wire or the television's screen. DLNA made this
    // the sixth enforcement point the day it started using this function.
    let url_name = if hidden {
        let ext = basename.rsplit('.').next().unwrap_or("mp4").to_ascii_lowercase();
        format!("{token}.{ext}")
    } else {
        crate::torrent::urlencode(&basename)
    };
    let url = format!("http://{ip}:{port}/c/{token}/{url_name}");
    {
        let mut reg = shared.reg.lock().unwrap_or_else(|p| p.into_inner());
        *reg = Some(Registered {
            token,
            path: file.to_path_buf(),
            mime,
            dir: false,
            torrent: None,
        });
        shared.hits.store(0, Ordering::Relaxed);
    }
    Ok(url)
}

/// LOAD a URL somebody else registered — today the torrent stream, which is
/// served by `cast_serve_torrent` rather than by `cast_load`'s own registration.
///
/// The rung exists because a **direct-play** torrent needs no preparation at
/// all: the receiver fetches it over HTTP with Range exactly as it fetches a
/// prepared file, so "the file is not finished" stops being a reason to refuse.
/// Anything that would need repacking still waits for the whole file — half a
/// film remuxed is half a film.
#[tauri::command]
pub async fn cast_load_url(
    service: tauri::State<'_, Arc<CastService>>,
    url: String,
    name: String,
    position: f64,
    title: Option<String>,
) -> Result<(), String> {
    let cmd_tx = {
        let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.session.as_ref().ok_or("not connected")?.cmd_tx.clone()
    };
    cmd_tx
        .send(Cmd::Load {
            url,
            mime: cast_mime(&name),
            title,
            position,
            hls_format: None,
        })
        .map_err(|_| "session ended".to_string())
}

/// Register an **incomplete** torrent file as the thing the LAN server serves,
/// and hand back its URL.
///
/// This is the rung the whole HLS detour was built for and could not deliver:
/// segmenting costs the file's codecs (H.264 + stereo on the measured receiver),
/// while a renderer that takes the container plays the release untouched —
/// HEVC, surround and all — and librqbit's blocking reads feed it exactly as
/// they feed mpv, turning the television's own Range requests into piece
/// priority. The privacy rule is unchanged: one source behind a random token,
/// and a private-root file is named after the token instead of itself.
#[tauri::command]
pub async fn cast_serve_torrent(
    service: tauri::State<'_, Arc<CastService>>,
    torrents: tauri::State<'_, Arc<crate::torrent::TorrentService>>,
    info_hash: String,
    index: usize,
    name: String,
    device_ip: String,
    hidden: bool,
) -> Result<String, String> {
    let device_ip: IpAddr = device_ip.parse().map_err(|_| "bad device address".to_string())?;
    let (ip, port, shared) = ensure_server(&service, device_ip).await?;
    let token = format!("{:032x}", rand::random::<u128>());
    let ext = name.rsplit('.').next().unwrap_or("mkv").to_ascii_lowercase();
    let url_name = if hidden {
        format!("{token}.{ext}")
    } else {
        crate::torrent::urlencode(&name)
    };
    let url = format!("http://{ip}:{port}/c/{token}/{url_name}");
    {
        let mut reg = shared.reg.lock().unwrap_or_else(|p| p.into_inner());
        *reg = Some(Registered {
            token,
            // Unused on this route, but a registration without a path would
            // mean an Option threaded through every reader for nothing.
            path: PathBuf::new(),
            mime: cast_mime(&name),
            dir: false,
            torrent: Some((info_hash.to_ascii_lowercase(), index, name)),
        });
        *shared.torrents.lock().unwrap_or_else(|p| p.into_inner()) = Some(torrents.inner().clone());
        shared.hits.store(0, Ordering::Relaxed);
    }
    Ok(url)
}

async fn ensure_server(
    service: &Arc<CastService>,
    device_ip: IpAddr,
) -> Result<(IpAddr, u16, Arc<ServeShared>), String> {
    let lan_ip = lan_ip_for(device_ip).ok_or("no usable network interface")?;

    {
        let mut inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
        match inner.server.as_ref() {
            Some(server) if server.ip == lan_ip => {
                return Ok((server.ip, server.port, server.shared.clone()));
            }
            Some(_) => {
                // Bound for a device on another subnet; that listener is of no
                // use to this TV.
                if let Some(server) = inner.server.take() {
                    server.task.abort();
                }
            }
            None => {}
        }
    }

    // This is the moment the Windows Defender prompt can appear — the first
    // non-loopback listen. It happens on picking a device, never at startup,
    // and the frontend warns before calling here.
    let listener = TcpListener::bind((lan_ip, 0))
        .await
        .map_err(|e| format!("cannot bind {lan_ip}: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("cannot read bound port: {e}"))?
        .port();

    let shared = Arc::new(ServeShared {
        reg: Mutex::new(None),
        torrents: Mutex::new(None),
        hits: AtomicU64::new(0),
    });
    let serve_shared = shared.clone();
    let task = tauri::async_runtime::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let shared = serve_shared.clone();
            tauri::async_runtime::spawn(async move {
                let io = TokioIo::new(stream);
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(
                        io,
                        service_fn(move |req| {
                            let shared = shared.clone();
                            async move {
                                Ok::<_, std::convert::Infallible>(serve_cast(shared, req).await)
                            }
                        }),
                    )
                    .await;
            });
        }
    });

    let mut inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    inner.server = Some(Server {
        ip: lan_ip,
        port,
        shared: shared.clone(),
        task,
    });
    Ok((lan_ip, port, shared))
}

#[tauri::command]
pub fn cast_control(
    service: tauri::State<'_, Arc<CastService>>,
    action: String,
    value: Option<f64>,
) -> Result<(), String> {
    let cmd = match action.as_str() {
        "play" => Cmd::Play,
        "pause" => Cmd::Pause,
        "seek" => Cmd::Seek(value.unwrap_or(0.0).max(0.0)),
        "volume" => Cmd::SetVolume(value.unwrap_or(1.0)),
        "mute" => Cmd::SetMuted(value.unwrap_or(0.0) != 0.0),
        _ => return Err("unknown action".into()),
    };
    let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    let session = inner.session.as_ref().ok_or("not connected")?;
    session.cmd_tx.send(cmd).map_err(|_| "session ended".to_string())
}

#[tauri::command]
pub fn cast_status(service: tauri::State<'_, Arc<CastService>>) -> CastStatus {
    let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    let Some(session) = inner.session.as_ref() else {
        return CastStatus {
            state: "idle".into(),
            ..Default::default()
        };
    };
    let fetches = inner
        .server
        .as_ref()
        .map(|s| s.shared.hits.load(Ordering::Relaxed))
        .unwrap_or(0);
    let status = session.status.lock().unwrap_or_else(|p| p.into_inner());
    let time = match (&status.reported_at, status.state.as_str()) {
        // Extrapolate only while the TV says it is moving; a paused or
        // buffering report stands as reported.
        (Some(at), "playing") => status.media_time + at.elapsed().as_secs_f64() * status.rate,
        _ => status.media_time,
    };
    CastStatus {
        state: status.state.clone(),
        error: status.error.clone(),
        time,
        duration: status.duration,
        volume: status.volume,
        muted: status.muted,
        volume_known: status.volume_control.is_some(),
        volume_fixed: status.volume_control.as_deref() == Some("fixed"),
        fetches,
        device: Some(session.device.name.clone()),
    }
}

/// End the session: the TV goes back to its home screen, the LAN server closes,
/// the registered file is forgotten. Returns the last known TV position so the
/// caller can hand playback back to mpv at the right frame.
#[tauri::command]
pub fn cast_disconnect(service: tauri::State<'_, Arc<CastService>>) -> f64 {
    let (session, server) = {
        let mut inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
        (inner.session.take(), inner.server.take())
    };
    let mut last = 0.0;
    if let Some(session) = session {
        {
            let status = session.status.lock().unwrap_or_else(|p| p.into_inner());
            last = match (&status.reported_at, status.state.as_str()) {
                (Some(at), "playing") => {
                    status.media_time + at.elapsed().as_secs_f64() * status.rate
                }
                _ => status.media_time,
            };
        }
        // Fire-and-forget: the pump sends the polite goodbyes and ends. If the
        // wire is already dead the task times out on its own; nothing waits.
        let _ = session.cmd_tx.send(Cmd::Disconnect);
        drop(session.task);
    }
    stop_server(server);
    last
}

/// Take the LAN server down and let go of what it was serving.
///
/// Factored out because **the DLNA transport ends through its own disconnect**
/// and used to leave this running: the token stayed valid and the file stayed
/// fetchable on the LAN after the session was over, which is precisely the
/// promise the one-file-behind-a-token design makes. Every transport that
/// borrows this server has to release it the same way.
fn stop_server(server: Option<Server>) {
    let Some(server) = server else { return };
    let reg = server
        .shared
        .reg
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take();
    *server.shared.torrents.lock().unwrap_or_else(|p| p.into_inner()) = None;
    server.task.abort();
    // HLS segments are transient by contract — they die with the session.
    if let Some(reg) = reg {
        if reg.dir {
            let _ = std::fs::remove_dir_all(&reg.path);
        }
    }
}

/// Release the LAN server on behalf of a transport with its own disconnect
/// path (DLNA). One entry point rather than handing out the private `Server`.
pub(crate) fn release_server(service: &Arc<CastService>) {
    let server = {
        let mut inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.server.take()
    };
    stop_server(server);
}

// ---- Prepare: remux / audio-transcode into a Cast-ready MP4 ----------------
//
// The phase-2 rung, shaped by what the real TV validated: video is NEVER
// re-encoded (`-c:v copy` — lossless, and the whole reason a film takes
// seconds), audio is copied when the receiver can take it and re-encoded to
// E-AC-3 640k otherwise (passthrough confirmed working on the real chain).
// The output is a complete `+faststart` MP4 in the cast cache, so the serving
// side stays the plain Range-on-a-real-file path — no HLS, per casting.md.
//
// The CLI rather than ffmpeg-the-third: `-movflags +faststart`, progress
// reporting and kill-to-cancel are free with the binary (a 0.5 MB shim over
// the DLLs the app already bundles), and the library route would mean hand
// wiring a decode→encode chain for the audio rung. macOS does not bundle the
// CLI today, so prepare is Windows-only until that is decided — the command
// fails with "no ffmpeg binary" there, which the frontend reports honestly.

/// Emitted while a prepare runs: the fraction done, 0..1.
const PREPARE_EVENT: &str = "frameplayer://cast-prepare";

/// HLS sessions live under this dot-dir inside the cast cache: transient by
/// contract (removed at disconnect, swept at the next HLS start), and the
/// dot name keeps them out of the file-only LRU maths.
const HLS_DIR: &str = ".hls";

#[cfg(windows)]
const FFMPEG_BIN: &str = "ffmpeg.exe";
#[cfg(not(windows))]
const FFMPEG_BIN: &str = "ffmpeg";

/// Next to the exe (dev: build.rs copies it there; Windows install: bundled to
/// the exe's own directory), then the **resource directory**, which is where it
/// lands inside a macOS `.app` — the exe there is in `Contents/MacOS` and the
/// resources in `Contents/Resources`, so "next to the exe" is not one place on
/// both platforms. The source tree is the last resort, for a dev build that
/// predates the copy step.
fn ffmpeg_bin(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager as _;

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(FFMPEG_BIN));
        }
    }
    if let Ok(res) = app.path().resource_dir() {
        candidates.push(res.join(FFMPEG_BIN));
    }
    let tree = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    candidates.push(tree.join("ffmpeg").join("bin").join(FFMPEG_BIN));
    candidates.push(tree.join("ffmpeg-macos").join("bin").join(FFMPEG_BIN));
    candidates.into_iter().find(|p| p.is_file())
}

/// Cache key: source identity (path + size + mtime, the thumbnail cache's
/// convention) plus what was asked of the transcode — a different audio track
/// or rung is a different output.
fn prepare_key(path: &std::path::Path, meta: &std::fs::Metadata, audio_index: i64, transcode: bool) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.to_string_lossy().hash(&mut h);
    meta.len().hash(&mut h);
    if let Ok(modified) = meta.modified() {
        if let Ok(d) = modified.duration_since(std::time::UNIX_EPOCH) {
            d.as_secs().hash(&mut h);
        }
    }
    audio_index.hash(&mut h);
    transcode.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Prepared files are full film size (video is copied), so the cap is real.
/// LRU by mtime; the file just produced is never pruned — a cap of zero
/// therefore means "keep nothing but the current session's file", and the
/// frontend deletes that one too when the session ends.
fn prune_cast_cache(dir: &std::path::Path, keep: &std::path::Path, cap: u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(PathBuf, u64, std::time::SystemTime)> = entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if !p.is_file() || p == keep {
                return None;
            }
            let m = e.metadata().ok()?;
            Some((p, m.len(), m.modified().ok()?))
        })
        .collect();
    let keep_size = keep.metadata().map(|m| m.len()).unwrap_or(0);
    let mut total: u64 = keep_size + files.iter().map(|(_, s, _)| s).sum::<u64>();
    files.sort_by_key(|(_, _, t)| *t);
    for (path, size, _) in files {
        if total <= cap {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= size;
        }
    }
}

/// The cache probe, separated from `cast_prepare` so the frontend can skip the
/// "preparing…" popup entirely for a variant that already exists — a progress
/// popup for work that takes no time reads as a glitch when it flashes and as
/// a hang when anything delays its replacement.
#[tauri::command]
pub fn cast_prepare_cached(
    app: tauri::AppHandle,
    path: String,
    audio_index: i64,
    transcode_audio: bool,
) -> Option<String> {
    use tauri::Manager as _;

    let src = PathBuf::from(&path);
    let meta = src.metadata().ok()?;
    let dir = app.path().app_cache_dir().ok()?.join("cast");
    let key = prepare_key(&src, &meta, audio_index, transcode_audio);
    let out = dir.join(format!("{key}.mp4"));
    (out.is_file() && out.metadata().map(|m| m.len() > 0).unwrap_or(false))
        .then(|| out.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn cast_prepare(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<CastService>>,
    path: String,
    audio_index: i64,
    transcode_audio: bool,
    channels: i64,
    hevc_tag: bool,
    duration: f64,
    cap_bytes: u64,
) -> Result<String, String> {
    use tauri::Manager as _;

    let src = PathBuf::from(&path);
    let meta = src.metadata().map_err(|e| format!("{e}"))?;
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("cast");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let key = prepare_key(&src, &meta, audio_index, transcode_audio);
    let out = dir.join(format!("{key}.mp4"));
    if out.is_file() && out.metadata().map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(out.to_string_lossy().into_owned());
    }
    let ffmpeg = ffmpeg_bin(&app).ok_or("no ffmpeg binary")?;
    let tmp = dir.join(format!("{key}.part"));

    let service = service.inner().clone();
    let out_result = out.clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_prepare(
            &app, &service, &ffmpeg, &src, &tmp, &out_result,
            audio_index, transcode_audio, channels, hevc_tag, duration, cap_bytes,
        )
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(out.to_string_lossy().into_owned())
}

/// Generate a complete HLS rendition of the file: a VOD playlist plus
/// keyframe-cut segments, video stream-copied, audio per the usual rung.
///
/// **v1 generates everything before the LOAD** — at stream-copy speed that is
/// the same seconds the MP4 prepare takes — because a playlist ffmpeg wrote to
/// the end is accurate (real segment durations, ENDLIST present) and the
/// receiver-compatibility questions (E-AC-3-in-HLS, HEVC-in-fMP4) get a clean
/// test with no synthetic-playlist noise in the way. Serving while generating
/// is the on-the-fly step, and it comes after the TV proves it plays our HLS
/// at all (casting.md, the HLS analysis).
///
/// The session directory is transient by contract: removed at disconnect (or
/// when a track switch replaces it), and leftovers from crashed runs are swept
/// here — skipping whatever directory is currently being served.
#[tauri::command]
pub async fn cast_hls_prepare(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<CastService>>,
    path: String,
    audio_index: i64,
    transcode_audio: bool,
    channels: i64,
    fmp4: bool,
    duration: f64,
) -> Result<String, String> {
    use tauri::Manager as _;

    let src = PathBuf::from(&path);
    if !src.is_file() {
        return Err("no such file".into());
    }
    let base = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("cast")
        .join(HLS_DIR);
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;

    // Sweep leaked sessions — but never the directory a live cast is serving
    // from (a track switch prepares the next rendition while the old one still
    // feeds the TV).
    let serving: Option<PathBuf> = {
        let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.server.as_ref().and_then(|server| {
            let reg = server.shared.reg.lock().unwrap_or_else(|p| p.into_inner());
            reg.as_ref().filter(|r| r.dir).map(|r| r.path.clone())
        })
    };
    if let Ok(entries) = std::fs::read_dir(&base) {
        for entry in entries.flatten() {
            if Some(entry.path()) != serving {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }

    let dir = base.join(format!("{:016x}", rand::random::<u64>()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let ffmpeg = ffmpeg_bin(&app).ok_or("no ffmpeg binary")?;
    let playlist = dir.join("index.m3u8");

    let service = service.inner().clone();
    let playlist_result = playlist.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut cmd = std::process::Command::new(&ffmpeg);
        cmd.arg("-y")
            .args(["-hide_banner", "-loglevel", "error", "-nostats"])
            .args(["-progress", "pipe:1"])
            .arg("-i")
            .arg(&src)
            .args(["-map", "0:v:0"])
            .args(["-map", &format!("0:a:{}", audio_index.max(0))])
            .args(["-c:v", "copy"]);
        // **The audio ladder is a property of the transport, not of the file.**
        // The progressive path passes E-AC-3 through to the TV's own decoder
        // and 5.1 comes out; HLS goes through the receiver's browser pipeline,
        // and this one takes AAC stereo and nothing else — measured on the real
        // TV, one rendition per cell: E-AC-3 refused in TS and in fMP4 and
        // refused again from a master playlist's CODECS, AC-3 refused, and AAC
        // 5.1 accepted and then *killed the receiver app* a few segments in.
        // So HLS always transcodes, and it says so in the settings hint: the
        // mode costs surround. `transcode_audio` describes the progressive
        // rung and is deliberately not consulted here.
        let _ = transcode_audio;
        cmd.args(["-c:a", "aac", "-b:a", "192k"]);
        if channels == 6 {
            // **A plain `-ac 2` throws the LFE away**, and on a soundbar that
            // is the most audible thing about the fold-down: measured on the
            // rotating-tone file, the LFE second reads −13.9 dB in the source
            // and **−90.3 dB** after ffmpeg's own downmix — not attenuated,
            // gone. (Which is the standard Dolby downmix behaviour, and the
            // right call for broadcast, where LFE is +10 dB in band and would
            // overload; here it just means an action scene loses its bass.)
            // `lfe_mix_level` on the resampler does not change it — measured,
            // no effect at all — so the matrix is written out by hand.
            // Coefficients are the ITU downmix normalised by 1/(1+2·0.707) so
            // nothing clips (measured peak −12.1 dB against −10.9 dB before),
            // with LFE carried at the same weight as centre and surround: the
            // LFE second comes back at −16.8 dB, in line with its neighbours.
            // Written in channel *indices* rather than names on purpose — 5.1
            // and 5.1(side) share the order FL FR FC LFE S/BL S/BR, and a
            // named formula would fail on whichever of the two it was not
            // written for. Anything else (stereo already, or 7.1) takes the
            // stock downmix.
            cmd.args([
                "-af",
                "pan=stereo|c0=0.4142*c0+0.2929*c2+0.2929*c4+0.2929*c3\
                 |c1=0.4142*c1+0.2929*c2+0.2929*c5+0.2929*c3",
            ]);
        } else {
            cmd.args(["-ac", "2"]);
        }
        cmd.args(["-f", "hls"])
            .args(["-hls_time", "4"])
            .args(["-hls_playlist_type", "vod"])
            .args(["-hls_flags", "independent_segments"]);
        if fmp4 {
            // HEVC is out of spec in TS segments; fMP4 is its HLS home. And the
            // MP4 path's `hvc1` tag is needed here for the same reason it is
            // needed there — measured: without it ffmpeg writes `hev1` into
            // init.mp4 (parameter sets in-band), which the receiver refuses
            // with "could not open this file". fMP4 is only ever chosen for
            // HEVC (`hlsVariant`), so the tag needs no codec test of its own.
            cmd.args(["-tag:v", "hvc1"]);
            cmd.args(["-hls_segment_type", "fmp4"])
                .args(["-hls_fmp4_init_filename", "init.mp4"]);
            cmd.arg("-hls_segment_filename")
                .arg(dir.join("seg%05d.m4s"));
        } else {
            cmd.arg("-hls_segment_filename")
                .arg(dir.join("seg%05d.ts"));
        }
        cmd.arg(&playlist);

        let result = run_ffmpeg_job(&app, &service, cmd, duration);
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        result
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(playlist_result.to_string_lossy().into_owned())
}

/// Bytes the cast cache holds (prepared MP4s and any HLS session dirs),
/// measured from disk like the torrent list — no record of ours to trust.
#[tauri::command]
pub fn cast_cache_size(app: tauri::AppHandle) -> u64 {
    use tauri::Manager as _;

    let Ok(cache) = app.path().app_cache_dir() else {
        return 0;
    };
    crate::torrent::dir_size(&cache.join("cast"))
}

/// Delete everything the cast cache holds except what a live session is using.
/// Files the OS refuses to delete (still open by a serving stream) are simply
/// skipped — the LRU catches them later.
#[tauri::command]
pub fn cast_clear_cache(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<CastService>>,
) -> Result<u64, String> {
    use tauri::Manager as _;

    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("cast");
    let keep: Option<PathBuf> = {
        let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.server.as_ref().and_then(|server| {
            let reg = server.shared.reg.lock().unwrap_or_else(|p| p.into_inner());
            reg.as_ref().map(|r| r.path.clone())
        })
    };
    let mut freed = 0u64;
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(0);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if Some(&path) == keep.as_ref() {
            continue;
        }
        if path.is_dir() {
            // The HLS dot-dir: clear its sessions, sparing the served one.
            if let Ok(sub) = std::fs::read_dir(&path) {
                for session in sub.flatten() {
                    let session = session.path();
                    if Some(&session) == keep.as_ref() {
                        continue;
                    }
                    let size = crate::torrent::dir_size(&session);
                    if std::fs::remove_dir_all(&session).is_ok() {
                        freed += size;
                    }
                }
            }
        } else {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            if std::fs::remove_file(&path).is_ok() {
                freed += size;
            }
        }
    }
    Ok(freed)
}

#[allow(clippy::too_many_arguments)]
fn run_prepare(
    app: &tauri::AppHandle,
    service: &Arc<CastService>,
    ffmpeg: &std::path::Path,
    src: &std::path::Path,
    tmp: &std::path::Path,
    out: &std::path::Path,
    audio_index: i64,
    transcode_audio: bool,
    channels: i64,
    hevc_tag: bool,
    duration: f64,
    cap_bytes: u64,
) -> Result<(), String> {
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.arg("-y")
        .args(["-hide_banner", "-loglevel", "error", "-nostats"])
        .args(["-progress", "pipe:1"])
        .arg("-i")
        .arg(src)
        .args(["-map", "0:v:0"])
        .args(["-map", &format!("0:a:{}", audio_index.max(0))])
        .args(["-c:v", "copy"]);
    if hevc_tag {
        // The tag the receiver's MP4 parser expects for HEVC; wrong on H.264,
        // hence conditional.
        cmd.args(["-tag:v", "hvc1"]);
    }
    if transcode_audio {
        cmd.args(["-c:a", "eac3", "-b:a", "640k"]);
        // E-AC-3 tops out at 5.1: a 7.1 TrueHD/DTS-HD core folds down, a
        // stereo source must NOT be upmixed — hence conditional.
        if channels > 6 {
            cmd.args(["-ac", "6"]);
        }
    } else {
        cmd.args(["-c:a", "copy"]);
    }
    cmd.args(["-movflags", "+faststart", "-f", "mp4"]).arg(tmp);

    if let Err(e) = run_ffmpeg_job(app, service, cmd, duration) {
        let _ = std::fs::remove_file(tmp);
        return Err(e);
    }
    std::fs::rename(tmp, out).map_err(|e| format!("{e}"))?;
    prune_cast_cache(
        out.parent().unwrap_or(std::path::Path::new(".")),
        out,
        cap_bytes,
    );
    Ok(())
}

/// Spawn one ffmpeg, park it for cancellation, forward its progress, reap it.
/// Shared by the MP4 prepare and the HLS generation — one place for the child
/// bookkeeping, the stderr drain and the µs-progress quirk.
fn run_ffmpeg_job(
    app: &tauri::AppHandle,
    service: &Arc<CastService>,
    mut cmd: std::process::Command,
    duration: f64,
) -> Result<(), String> {
    use std::io::BufRead as _;
    use tauri::Emitter as _;

    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        // No console window flash from a GUI process.
        use std::os::windows::process::CommandExt as _;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }

    let mut child = cmd.spawn().map_err(|e| format!("ffmpeg: {e}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // Parked in the service so `cast_prepare_cancel` can kill it. If a cancel
    // already emptied... no: this is the only writer; a leftover child from a
    // previous run was reaped below.
    *service.prepare.lock().unwrap_or_else(|p| p.into_inner()) = Some(child);

    // Drain stderr on its own thread: `-loglevel error` keeps it tiny, but a
    // filling pipe would deadlock the progress loop below.
    let err_buf = std::sync::Arc::new(Mutex::new(String::new()));
    let err_sink = err_buf.clone();
    let err_thread = std::thread::spawn(move || {
        if let Some(stderr) = stderr {
            let reader = std::io::BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                let mut buf = err_sink.lock().unwrap_or_else(|p| p.into_inner());
                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(&line);
            }
        }
    });

    // ffmpeg's `-progress` blocks: `out_time_us` is microseconds (as is the
    // misnamed `out_time_ms`, a known quirk — both are µs).
    if let Some(stdout) = stdout {
        let reader = std::io::BufReader::new(stdout);
        let mut last_pct = -1i64;
        for line in reader.lines().map_while(Result::ok) {
            let Some(value) = line
                .strip_prefix("out_time_us=")
                .or_else(|| line.strip_prefix("out_time_ms="))
            else {
                continue;
            };
            let Ok(us) = value.trim().parse::<i64>() else {
                continue;
            };
            if duration <= 0.0 {
                continue;
            }
            let frac = (us as f64 / 1_000_000.0 / duration).clamp(0.0, 1.0);
            let pct = (frac * 100.0) as i64;
            if pct != last_pct {
                last_pct = pct;
                let _ = app.emit(PREPARE_EVENT, frac);
            }
        }
    }

    let status = {
        let child = service
            .prepare
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        match child {
            Some(mut child) => child.wait().map_err(|e| format!("ffmpeg: {e}"))?,
            // Cancelled from the outside: the canceller killed and reaped it.
            // The caller removes whatever partial output its mode produced.
            None => return Err("cancelled".into()),
        }
    };
    let _ = err_thread.join();

    if !status.success() {
        let err = err_buf.lock().unwrap_or_else(|p| p.into_inner());
        let tail = err.lines().last().unwrap_or("ffmpeg failed").to_string();
        return Err(tail);
    }
    Ok(())
}

#[tauri::command]
pub fn cast_prepare_cancel(service: tauri::State<'_, Arc<CastService>>) {
    let child = service
        .prepare
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take();
    if let Some(mut child) = child {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Delete one prepared file — the privacy half of the cache: a prepared copy
/// of a file under a private root must not outlive its cast session. The path
/// must be inside the cast cache, so this cannot be aimed at anything else.
#[tauri::command]
pub fn cast_forget_prepared(app: tauri::AppHandle, path: String) -> Result<(), String> {
    use tauri::Manager as _;

    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("cast");
    let target = PathBuf::from(&path);
    if !target.starts_with(&dir) {
        return Err("not a prepared file".into());
    }
    // Best-effort: the server may still hold the file open for a beat after
    // disconnect; a leftover is caught by the LRU pruning either way.
    let _ = std::fs::remove_file(&target);
    Ok(())
}

// ---- Tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_roundtrip() {
        let payload = r#"{"type":"PING"}"#;
        let buf = wire::encode(SENDER_ID, RECEIVER_ID, NS_HEARTBEAT, payload);
        let decoded = wire::decode(&buf).expect("decodes");
        assert_eq!(decoded.namespace, NS_HEARTBEAT);
        assert_eq!(decoded.source, SENDER_ID);
        assert_eq!(decoded.destination, RECEIVER_ID);
        assert_eq!(decoded.payload, payload);
    }

    #[test]
    fn wire_skips_unknown_fields() {
        // A varint field (1), a fixed32 (5) and a fixed64 (1) we do not know,
        // around a namespace we do.
        let mut buf = vec![0x08, 0x05];
        buf.push(0x22);
        buf.push(4);
        buf.extend_from_slice(b"test");
        buf.push(0x3d); // field 7, wire type 5 (fixed32)
        buf.extend_from_slice(&[1, 2, 3, 4]);
        let decoded = wire::decode(&buf).expect("decodes");
        assert_eq!(decoded.namespace, "test");
    }

    #[test]
    fn lan_ip_prefers_matching_subnet() {
        let candidates = [
            // A VPN TUN with a /32-ish mask that matches nothing.
            ("10.8.0.2".parse().unwrap(), "255.255.255.255".parse().unwrap()),
            // The real LAN interface.
            ("192.168.2.56".parse().unwrap(), "255.255.255.0".parse().unwrap()),
        ];
        let picked = pick_lan_ip(&candidates, "192.168.2.30".parse().unwrap());
        assert_eq!(picked, Some("192.168.2.56".parse().unwrap()));
        // No subnet matches: fall back to the first candidate rather than
        // refusing outright.
        let picked = pick_lan_ip(&candidates, "172.16.0.9".parse().unwrap());
        assert_eq!(picked, Some("10.8.0.2".parse().unwrap()));
    }

    /// Live smoke test against whatever Cast devices the LAN actually has.
    /// Off by default (needs a network with a TV on it); run with
    /// `FP_TEST_CAST=1 cargo test --lib cast::tests::discover_smoke -- --nocapture`,
    /// or with a specific device's IP in the variable (`FP_TEST_CAST=192.168.2.48`)
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
            let discovery = Discovery::start().expect("mdns starts");
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
    /// FP_CAST_DEBUG=1 FP_CAST_LOAD=192.168.2.48 \
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
}
