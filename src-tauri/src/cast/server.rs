//! The LAN file server: one registered file (or HLS directory) behind a
//! random token, for every transport that hands a television a URL.

use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use futures_util::StreamExt as _;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, StreamBody};
use hyper::body::Frame;
use hyper::service::service_fn;
use hyper::{header, Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::net::TcpListener;
use tokio_util::io::ReaderStream;

use super::CastService;

// ---- The LAN file server ---------------------------------------------------

/// Read size for one body chunk, matching torrent/http.rs.
const STREAM_CHUNK: usize = 65536;

type Body = BoxBody<Bytes, std::io::Error>;

/// What the server will answer for. Everything after the token in the URL is
/// display only for a plain file — for a private path the basename is the
/// token again, because the name must not leave the machine — so the route
/// matches on the token alone. For an HLS session `path` is the session
/// *directory* and the segment after the token names a file inside it
/// (playlist, init, segments), validated to a single flat component.
pub(super) struct Registered {
    pub(super) token: String,
    pub(super) path: PathBuf,
    pub(super) mime: &'static str,
    pub(super) dir: bool,
    /// A torrent still downloading, served through librqbit's blocking stream
    /// instead of read from disk: `(info hash, file index, file name)`. The
    /// sparse file on disk is not an option — a stretch that has not arrived
    /// reads back as zeros, and the television would play them without a word.
    pub(super) torrent: Option<(String, usize, String)>,
    /// A subtitle file handed to a DLNA renderer beside the video, under a
    /// token of its own (`attach_caption`). It dies with the registration it
    /// was attached to, like everything else this server answers for.
    pub(super) caption: Option<Caption>,
}

pub(super) struct Caption {
    token: String,
    path: PathBuf,
    /// Where the renderer fetches it — also what the video's answer names in
    /// `CaptionInfo.sec` when a Samsung asks for it.
    url: String,
}

pub(super) struct ServeShared {
    pub(super) reg: Mutex<Option<Registered>>,
    /// Set when a torrent source is registered — the cast server does not own
    /// the torrent session and only borrows it to stream through.
    pub(super) torrents: Mutex<Option<Arc<crate::torrent::TorrentService>>>,
    /// Requests served for the current registration. Zero after a LOAD that the
    /// TV accepted is the firewall signature: the command channel worked, the
    /// media channel never reached us.
    pub(super) hits: AtomicU64,
    /// Every address that has sent a request for the current registration, in
    /// order of first request. Who asks is the one thing that tells a URL handed to
    /// a receiver from a file the Mac is playing itself — AirPlay reads it.
    pub(super) peers: Mutex<Vec<IpAddr>>,
    /// The address the server is bound to, which is this machine's own.
    own_ip: IpAddr,
    /// Requests for the current registration from anybody but this machine.
    /// AirPlay's player fetches the URL itself while the viewer chooses a
    /// receiver, so `hits` cannot say whether the receiver went past an HLS
    /// master playlist — and a receiver that refuses a variant fetches the
    /// master and nothing else (measured), which is the whole signal.
    pub(super) remote_hits: AtomicU64,
}

pub(super) struct Server {
    ip: IpAddr,
    port: u16,
    pub(super) shared: Arc<ServeShared>,
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

/// The one route: `/c/<token>/<anything>`. Everything else — and every token
/// that is not the currently registered one — is 404.
///
/// Wrapper over the handler so that every answer is logged in one place under
/// the verbose log (`logs::verbose`): *what the TV asked for* is half of any
/// load diagnosis — a receiver that fetched the playlist and stopped failed at parsing it, one
/// that fetched init.mp4 and a segment failed at decoding, and one that asked
/// for nothing at all is the firewall case the connect flow already warns about.
async fn serve_cast(shared: Arc<ServeShared>, req: Request<hyper::body::Incoming>, peer: IpAddr) -> Response<Body> {
    // Per request, not per connection: a receiver keeps its connection alive
    // across a change of file, and the list is cleared on every registration —
    // recorded at accept, the next episode's fetches would come from nobody.
    {
        let mut peers = shared.peers.lock().unwrap_or_else(|p| p.into_inner());
        if peers.len() < 16 && !peers.contains(&peer) {
            peers.push(peer);
        }
    }
    if peer != shared.own_ip {
        shared.remote_hits.fetch_add(1, Ordering::Relaxed);
    }
    if !crate::logs::verbose() {
        return serve_cast_inner(shared, req).await;
    }
    let line = format!(
        "{peer} {} {} range={} timeseek={}",
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

    let (base, base_mime, is_dir, torrent, caption) = {
        let reg = shared.reg.lock().unwrap_or_else(|p| p.into_inner());
        match reg.as_ref() {
            Some(r) if r.token == token => (
                r.path.clone(),
                r.mime,
                r.dir,
                r.torrent.clone(),
                r.caption.as_ref().map(|c| c.url.clone()),
            ),
            Some(Registered { caption: Some(c), .. }) if c.token == token => {
                (c.path.clone(), caption_mime(&c.path), false, None, None)
            }
            _ => return simple(StatusCode::NOT_FOUND),
        }
    };
    // **Samsung asks for the subtitle in a request header, not in the
    // metadata.** A set that sends `getcaptionInfo.sec: 1` with its request
    // for the video reads the subtitle's URL off `CaptionInfo.sec` in the
    // answer — the convention every DLNA server that serves one to a Samsung
    // follows. Answered only when asked: nobody else knows the header.
    let caption_header = caption.filter(|_| {
        req.headers()
            .get("getcaptionInfo.sec")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.trim() == "1")
    });
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
        let mut res = service
            .serve_file(&hash, index, &name, range, req.method() == Method::HEAD)
            .await;
        if let Some(value) = caption_header.and_then(|u| hyper::header::HeaderValue::from_str(&u).ok()) {
            res.headers_mut().insert("CaptionInfo.sec", value);
        }
        return res;
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

    let mut res = res.status(status).header(header::CONTENT_LENGTH, len);
    if let Some(url) = &caption_header {
        res = res.header("CaptionInfo.sec", url.as_str());
    }
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

/// What the LAN server is answering for right now — a file, or a session
/// directory — so a sweep of leftovers never takes the one being played.
pub(crate) fn served_path(service: &Arc<CastService>) -> Option<PathBuf> {
    let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    let server = inner.server.as_ref()?;
    let reg = server.shared.reg.lock().unwrap_or_else(|p| p.into_inner());
    reg.as_ref().map(|r| r.path.clone())
}

/// Requests for the current file from anybody but this machine.
#[cfg(target_os = "macos")]
pub(crate) fn server_remote_hits(service: &Arc<CastService>) -> u64 {
    let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    inner
        .server
        .as_ref()
        .map(|s| s.shared.remote_hits.load(Ordering::Relaxed))
        .unwrap_or(0)
}

/// Serve a whole flat directory behind one fresh token — an HLS rendition,
/// whose playlists name their segments by bare file name. The directory
/// server answers one path component and nothing with a leading dot, so the
/// token reaches nothing outside it.
#[cfg(target_os = "macos")]
pub(crate) async fn serve_dir(
    service: &Arc<CastService>,
    device_ip: IpAddr,
    dir: &std::path::Path,
    entry: &str,
) -> Result<String, String> {
    let (ip, port, shared) = ensure_server(service, device_ip).await?;
    let token = format!("{:032x}", rand::random::<u128>());
    let url = format!("http://{ip}:{port}/c/{token}/{entry}");
    let mut reg = shared.reg.lock().unwrap_or_else(|p| p.into_inner());
    *reg = Some(Registered {
        token,
        path: dir.to_path_buf(),
        mime: "application/x-mpegURL",
        dir: true,
        torrent: None,
        caption: None,
    });
    shared.hits.store(0, Ordering::Relaxed);
    shared.peers.lock().unwrap_or_else(|p| p.into_inner()).clear();
    shared.remote_hits.store(0, Ordering::Relaxed);
    Ok(url)
}

/// The addresses that have sent a request to the LAN server for the current file.
#[cfg(target_os = "macos")]
pub(crate) fn server_peers(service: &Arc<CastService>) -> Vec<IpAddr> {
    let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    inner
        .server
        .as_ref()
        .map(|s| s.shared.peers.lock().unwrap_or_else(|p| p.into_inner()).clone())
        .unwrap_or_default()
}

/// Serve a subtitle file beside whatever is registered now, under a token of
/// its own, and hand back its URL — the external subtitle a DLNA renderer is
/// told about alongside the video. Always named by its token, whatever the
/// file is called: the renderer reads the format off the extension and has no
/// use for the name, which therefore never leaves the machine.
pub(crate) fn attach_caption(service: &Arc<CastService>, file: &std::path::Path) -> Option<String> {
    let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    let server = inner.server.as_ref()?;
    let mut reg = server.shared.reg.lock().unwrap_or_else(|p| p.into_inner());
    let r = reg.as_mut()?;
    let token = format!("{:032x}", rand::random::<u128>());
    let ext = file
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_else(|| "srt".into());
    let url = format!("http://{}:{}/c/{token}/{token}.{ext}", server.ip, server.port);
    r.caption = Some(Caption { token, path: file.to_path_buf(), url: url.clone() });
    Some(url)
}

/// The subtitle formats a renderer is handed, by the types DLNA servers
/// announce them under.
pub(crate) fn caption_mime(file: &std::path::Path) -> &'static str {
    match file.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).as_deref() {
        Some("smi") | Some("sami") => "smi/caption",
        Some("vtt") => "text/vtt",
        _ => "text/srt",
    }
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
pub(super) fn cast_mime(name: &str) -> &'static str {
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

/// Bind the LAN server (if it is not up already), register one file behind a
/// fresh token and hand back the URL a device on `device_ip`'s subnet can fetch
/// it from. The Cast path does this inline in `cast_load` with its own privacy
/// and HLS rules; this is the same three steps for anything else that hands a
/// television a URL — today the DLNA reconnaissance in dlna/.
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
            caption: None,
        });
        shared.hits.store(0, Ordering::Relaxed);
        shared.peers.lock().unwrap_or_else(|p| p.into_inner()).clear();
        shared.remote_hits.store(0, Ordering::Relaxed);
    }
    Ok(url)
}

pub(super) async fn ensure_server(
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
        peers: Mutex::new(Vec::new()),
        own_ip: lan_ip,
        remote_hits: AtomicU64::new(0),
    });
    let serve_shared = shared.clone();
    let task = tauri::async_runtime::spawn(async move {
        loop {
            let Ok((stream, peer)) = listener.accept().await else {
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
                                Ok::<_, std::convert::Infallible>(serve_cast(shared, req, peer.ip()).await)
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

/// Take the LAN server down and let go of what it was serving.
///
/// Factored out because **the DLNA transport ends through its own disconnect**
/// and used to leave this running: the token stayed valid and the file stayed
/// fetchable on the LAN after the session was over, which is precisely the
/// promise the one-file-behind-a-token design makes. Every transport that
/// borrows this server has to release it the same way.
pub(super) fn stop_server(server: Option<Server>) {
    let Some(server) = server else { return };
    let reg = server
        .shared
        .reg
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take();
    *server.shared.torrents.lock().unwrap_or_else(|p| p.into_inner()) = None;
    server.task.abort();
    // HLS segments are transient by contract — they die with the session, and
    // so does a subtitle converted for a DLNA renderer (never the viewer's own
    // file, which is served from where it lies).
    if let Some(reg) = reg {
        if reg.dir {
            let _ = std::fs::remove_dir_all(&reg.path);
        }
        if let Some(c) = reg.caption {
            if is_converted_caption(&c.path) {
                let _ = std::fs::remove_file(&c.path);
            }
        }
    }
}

/// Converted subtitles live under this dot-dir of the casting cache, like the
/// HLS sessions, and like them are transient: one session's worth at a time.
pub(crate) const CAPTION_DIR: &str = ".subs";

fn is_converted_caption(path: &std::path::Path) -> bool {
    path.parent()
        .and_then(|p| p.file_name())
        .is_some_and(|n| n == CAPTION_DIR)
}

/// Which of our addresses a device on `device_ip` would fetch from — the same
/// answer `ensure_server` binds to, exposed for the diagnosis, where "no
/// interface shares a subnet with it" is the whole explanation for a cast that
/// never starts.
pub(crate) fn lan_ip_for_device(device_ip: IpAddr) -> Option<IpAddr> {
    lan_ip_for(device_ip)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lan_ip_prefers_matching_subnet() {
        let candidates = [
            // A VPN TUN with a /32-ish mask that matches nothing.
            ("10.8.0.2".parse().unwrap(), "255.255.255.255".parse().unwrap()),
            // The real LAN interface.
            ("192.0.2.10".parse().unwrap(), "255.255.255.0".parse().unwrap()),
        ];
        let picked = pick_lan_ip(&candidates, "192.0.2.48".parse().unwrap());
        assert_eq!(picked, Some("192.0.2.10".parse().unwrap()));
        // No subnet matches: fall back to the first candidate rather than
        // refusing outright.
        let picked = pick_lan_ip(&candidates, "172.16.0.9".parse().unwrap());
        assert_eq!(picked, Some("10.8.0.2".parse().unwrap()));
    }
}
