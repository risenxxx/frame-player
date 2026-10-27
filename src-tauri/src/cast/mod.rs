//! Casting to TV (Google Cast): a URL handoff, never pixel streaming.
//!
//! The TV fetches the file over the LAN from an HTTP server this module runs
//! and decodes it itself; mpv stops decoding for the duration. This is the only
//! model compatible with `wid` embedding — mpv renders into a child window, and
//! no OS casting API can be fed from that. See docs/casting.md for the
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
//! names the file. That is this feature's slice of the docs/rules/sources-and-privacy.md rule that
//! every new path by which something about a file leaves this machine is
//! another privacy enforcement point.

use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tokio::sync::mpsc;

// The `#[tauri::command]` functions stay in this file: `generate_handler!` in
// lib.rs resolves each one's hidden `__cmd__*` macros by the path it is given
// (`cast::cast_load`), which a plain `pub use` of the function does not carry.
mod castv2;
mod discovery;
mod prepare;
mod server;

pub use discovery::CastDeviceInfo;
pub use prepare::TrackSource;
pub(crate) use castv2::probe_receiver;
pub(crate) use prepare::{ffmpeg_bin, run_ffmpeg_job, HLS_DIR};
pub(crate) use server::{
    attach_caption, caption_mime, cast_mime_for, lan_ip_for_device, release_server,
    serve_one_file, served_path, server_hits, CAPTION_DIR,
};
#[cfg(target_os = "macos")]
pub(crate) use server::{serve_dir, server_peers, server_remote_hits};

use castv2::{run_session, Cmd, StatusInner};
use discovery::Discovery;
use prepare::{prepare_key, prune_cast_cache, run_prepare, tracks_command, tracks_key};
use server::{cast_mime, ensure_server, stop_server, Registered, Server};

// ---- The cast session ------------------------------------------------------

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
    /// The running prepare (ffmpeg child), outside `inner` so canceling never
    /// contends with the session lock.
    prepare: Mutex<Option<std::process::Child>>,
}

#[derive(Default)]
struct Inner {
    discovery: Option<Discovery>,
    session: Option<Session>,
    server: Option<Server>,
}

// ---- Commands --------------------------------------------------------------

#[tauri::command]
pub fn cast_discover_start(
    service: tauri::State<'_, Arc<CastService>>,
    hints: Option<Vec<String>>,
) -> Result<(), String> {
    let mut inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    if inner.discovery.is_none() {
        let hints = crate::lan_sweep::parse_hints(&hints.unwrap_or_default());
        inner.discovery = Some(Discovery::start(hints)?);
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
    inner.discovery.as_ref().map(Discovery::list).unwrap_or_default()
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
            caption: None,
        });
        shared.hits.store(0, Ordering::Relaxed);
        shared.peers.lock().unwrap_or_else(|p| p.into_inner()).clear();
        shared.remote_hits.store(0, Ordering::Relaxed);
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
            caption: None,
        });
        *shared.torrents.lock().unwrap_or_else(|p| p.into_inner()) = Some(torrents.inner().clone());
        shared.hits.store(0, Ordering::Relaxed);
        shared.peers.lock().unwrap_or_else(|p| p.into_inner()).clear();
        shared.remote_hits.store(0, Ordering::Relaxed);
    }
    Ok(url)
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

// ---- Prepare: remux / audio-transcode into a Cast-ready MP4 ----------------
//
// The commands; what they run is in prepare.rs.

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
    let src = PathBuf::from(&path);
    let meta = src.metadata().ok()?;
    let dir = crate::portable::cache_dir(&app).ok()?.join("cast");
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
    let src = PathBuf::from(&path);
    let meta = src.metadata().map_err(|e| format!("{e}"))?;
    let dir = crate::portable::cache_dir(&app)?.join("cast");
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

/// A copy with exactly the tracks asked for: the video stream-copied, the
/// audio tracks in the order given (each copied, or E-AC-3 where the receiver
/// lacks the codec), the first flagged default, and any text subtitle tracks as
/// `mov_text`. External files — a dub from the release's own folders, a
/// subtitle file — are extra inputs.
///
/// AirPlay asks for **one** audio track, the chosen one, and no subtitles:
/// measured on an LG set, the receiver played its first audio track whatever
/// the Mac selected and showed no `mov_text` subtitle at all, so the choice has
/// to be made in the copy (docs/airplay.md has the measurements and the HLS
/// route still to be researched). The subtitle half stays because it is the
/// same ffmpeg invocation and is pinned by the tests below.
///
/// `probe` answers only from the cache: `Some` if this exact copy exists.
#[tauri::command]
pub async fn cast_prepare_tracks(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<CastService>>,
    path: String,
    audio: Vec<TrackSource>,
    subs: Vec<TrackSource>,
    hevc_tag: bool,
    duration: f64,
    cap_bytes: u64,
    probe: bool,
) -> Result<Option<String>, String> {
    let src = PathBuf::from(&path);
    let meta = src.metadata().map_err(|e| format!("{e}"))?;
    let dir = crate::portable::cache_dir(&app)?.join("cast");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let key = tracks_key(&src, &meta, &audio, &subs, hevc_tag);
    let out = dir.join(format!("{key}.mp4"));
    if out.is_file() && out.metadata().map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(Some(out.to_string_lossy().into_owned()));
    }
    if probe {
        return Ok(None);
    }
    let ffmpeg = ffmpeg_bin(&app).ok_or("no ffmpeg binary")?;
    let tmp = dir.join(format!("{key}.part"));
    let cmd = tracks_command(&ffmpeg, &src, &tmp, &audio, &subs, hevc_tag);
    let service = service.inner().clone();
    let out_result = out.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(e) = run_ffmpeg_job(&app, &service, cmd, duration) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        std::fs::rename(&tmp, &out_result).map_err(|e| format!("{e}"))?;
        prune_cast_cache(out_result.parent().unwrap_or(std::path::Path::new(".")), &out_result, cap_bytes);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(Some(out.to_string_lossy().into_owned()))
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
    let src = PathBuf::from(&path);
    if !src.is_file() {
        return Err("no such file".into());
    }
    let base = crate::portable::cache_dir(&app)?.join("cast").join(HLS_DIR);
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
            // gone. (Which is the standard Dolby downmix behavior, and the
            // right call for broadcast, where LFE is +10 dB in band and would
            // overload; here it just means an action scene loses its bass.)
            // `lfe_mix_level` on the resampler does not change it — measured,
            // no effect at all — so the matrix is written out by hand.
            // Coefficients are the ITU downmix normalized by 1/(1+2·0.707) so
            // nothing clips (measured peak −12.1 dB against −10.9 dB before),
            // with LFE carried at the same weight as center and surround: the
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
    let Ok(cache) = crate::portable::cache_dir(&app) else {
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
    let dir = crate::portable::cache_dir(&app)?.join("cast");
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
    let dir = crate::portable::cache_dir(&app)?.join("cast");
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
mod tests;
