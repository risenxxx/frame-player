//! The CASTV2 client: the wire format, TLS to the device's 8009, and the
//! session pump that speaks to the receiver.

use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::rustls;

use super::discovery::CastDeviceInfo;

/// The Default Media Receiver. No registration and no custom receiver: a custom
/// Web Receiver runs on the same hardware decoders, so it buys UI, not codec
/// support (casting.md, "Dead ends").
const DEFAULT_RECEIVER_APP: &str = "CC1AD845";

pub(super) const NS_CONNECTION: &str = "urn:x-cast:com.google.cast.tp.connection";
const NS_HEARTBEAT: &str = "urn:x-cast:com.google.cast.tp.heartbeat";
pub(super) const NS_RECEIVER: &str = "urn:x-cast:com.google.cast.receiver";
const NS_MEDIA: &str = "urn:x-cast:com.google.cast.media";

const SENDER_ID: &str = "sender-0";
pub(super) const RECEIVER_ID: &str = "receiver-0";

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

/// Ceiling for one CASTV2 frame. The spec caps messages at 64 KB; anything
/// larger is a desynchronised stream, and reading it as a length would allocate
/// garbage gigabytes.
const MAX_FRAME: usize = 256 * 1024;

// ---- The wire format -------------------------------------------------------
//
// One protobuf message type (CastMessage from cast_channel.proto), encoded by
// hand because the schema is seven fields and stable since 2014:
//   1 protocol_version (varint, CASTV2_1_0 = 0)   4 namespace (string)
//   2 source_id (string)                          5 payload_type (varint, 0 = STRING)
//   3 destination_id (string)                     6 payload_utf8 (string)
// Field 7 (payload_binary) is never sent by us and skipped on read.

pub(super) mod wire {
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

pub(super) async fn tls_connect(
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

// ---- The cast session ------------------------------------------------------

pub(super) enum Cmd {
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
pub(super) struct StatusInner {
    /// connecting | connected | loading | buffering | playing | paused |
    /// ended | stopped | error
    pub(super) state: String,
    /// Machine-readable reason, one of the strings the frontend maps to a
    /// sentence: unreachable | launch_failed | load_failed | closed | app_gone.
    pub(super) error: Option<String>,
    pub(super) media_time: f64,
    pub(super) reported_at: Option<Instant>,
    pub(super) rate: f64,
    pub(super) duration: f64,
    pub(super) volume: f64,
    pub(super) muted: bool,
    /// The receiver's `volume.controlType`: "master"/"attenuation" mean the
    /// device takes SET_VOLUME, "fixed" means it does not (volume lives on the
    /// TV remote), and None means no status has carried a volume object yet —
    /// one measured television answers its first GET_STATUS without one, so absence
    /// is a normal state, not an error.
    pub(super) volume_control: Option<String>,
}

fn set_status(status: &Arc<Mutex<StatusInner>>, f: impl FnOnce(&mut StatusInner)) {
    let mut guard = status.lock().unwrap_or_else(|p| p.into_inner());
    f(&mut guard);
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

/// Everything the pump loop needs in one place, so helpers can borrow it as a
/// unit instead of threading six arguments.
pub(super) struct Pump {
    pub(super) wr: tokio::io::WriteHalf<tokio_rustls::client::TlsStream<TcpStream>>,
    pub(super) status: Arc<Mutex<StatusInner>>,
    pub(super) request_id: u64,
    /// Set once the receiver reports our app: (session_id, transport_id).
    pub(super) transport: Option<(String, String)>,
    pub(super) media_session: Option<i64>,
    /// A LOAD that arrived before the app was up; sent on launch.
    pub(super) pending_load: Option<Cmd>,
    /// requestId of the in-flight LOAD, to tie an error answer to it.
    pub(super) load_request: Option<u64>,
}

impl Pump {
    pub(super) async fn send(&mut self, namespace: &str, destination: &str, payload: Value) -> Result<(), String> {
        let text = payload.to_string();
        if crate::logs::verbose() && namespace != NS_HEARTBEAT {
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

    pub(super) fn next_id(&mut self) -> u64 {
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
        if crate::logs::verbose() && msg.namespace != NS_HEARTBEAT {
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
                                "CANCELED" | "INTERRUPTED" => "stopped".into(),
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
pub(super) async fn run_session(
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
    // cancellation-safe (a canceled half-read frame desynchronises the
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
                        // only arrive as broadcasts — and a measured television answers
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

/// Connect, CONNECT, GET_STATUS — the smoke test the kept `discover_smoke`
/// runs, as something the diagnosis can call. **Deliberately stops short of
/// LAUNCH**: answering a diagnostic question by starting an app on someone's
/// television is a surprise, and everything that can be learned without it is
/// learned here.
pub(crate) async fn probe_receiver(ip: IpAddr, port: u16) -> Result<String, String> {
    let stream = tls_connect(ip, port).await?;
    let (rd, wr) = tokio::io::split(stream);
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
        .await?;
    let id = pump.next_id();
    pump.send(
        NS_RECEIVER,
        RECEIVER_ID,
        json!({ "type": "GET_STATUS", "requestId": id }),
    )
    .await?;

    let mut rd = rd;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(6);
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            return Err("no RECEIVER_STATUS within 6 s".into());
        }
        let mut len = [0u8; 4];
        if tokio::time::timeout(left, rd.read_exact(&mut len)).await.is_err() {
            return Err("no RECEIVER_STATUS within 6 s".into());
        }
        let n = u32::from_be_bytes(len) as usize;
        if n > MAX_FRAME {
            return Err("oversized frame".into());
        }
        let mut buf = vec![0u8; n];
        if rd.read_exact(&mut buf).await.is_err() {
            return Err("connection closed mid-frame".into());
        }
        let Some(msg) = wire::decode(&buf) else {
            continue;
        };
        let payload: Value = serde_json::from_str(&msg.payload).unwrap_or(Value::Null);
        if payload["type"].as_str() != Some("RECEIVER_STATUS") {
            continue;
        }
        let volume = payload["status"]["volume"]["controlType"]
            .as_str()
            .unwrap_or("not reported");
        let apps = payload["status"]["applications"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);
        // Data, not prose: this string ends up in a localised report window,
        // where the sentence belongs to the frontend and only the device's own
        // answers belong here.
        return Ok(format!("volume={volume} apps={apps}"));
    }
}

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
}
