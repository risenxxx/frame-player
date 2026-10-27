//! DLNA/UPnP AV reconnaissance: is there a MediaRenderer on this network, and
//! what does it say it can play?
//!
//! Why this exists at all, with Cast already shipped: a DLNA renderer is the
//! **TV's own player**, not a browser pipeline, so its format list is the set
//! the television actually decodes — routinely MKV, HEVC and E-AC-3, i.e. the
//! releases our prepare rung exists to repack. And unlike Cast it answers a
//! real capability question: `ConnectionManager::GetProtocolInfo` returns the
//! Sink list, which is the API whose absence forced the per-cell probing in
//! casting.md.
//!
//! **The branch proved out and this is a shipping transport** — this file is
//! the command surface the frontend drives. What survives from
//! the reconnaissance is the parsing: there is still no XML crate, because what
//! is wanted out of these documents is a dozen leaf elements and a service list,
//! and a real parser is a dependency plus a licensing entry for that. The cost
//! of the decision is paid in [`xml::open_tag_end`] and [`xml::unescape`], which handle
//! the shapes vendors actually emit — attributes, namespace prefixes, empty
//! elements, entities — and in the fixtures under each `mod tests`, which are the
//! only place a device we do not own can be reproduced. If the string scanning
//! ever has to grow a third special case, that is the signal to take the
//! dependency instead.
//!
//! Everything here is written for the general renderer rather than for the one
//! television it was measured on: the MIME spelling, the `Seek` unit and the
//! resolution of relative control URLs are all negotiated or derived from what
//! the device said, not assumed.
//!
//! Run it against the LAN with `FP_DLNA_PROBE=1 bun run tauri:macos` — it has to
//! run inside the app rather than from `cargo test`, because on macOS 15+ a
//! process without Local Network permission has its multicast silently dropped
//! and every answer here would be a false negative.

use std::net::IpAddr;
use std::time::Duration;

// The `#[tauri::command]` functions stay in this file: `generate_handler!` in
// lib.rs resolves each one's hidden `__cmd__*` macros by the path it is given
// (`dlna::dlna_load`), which a plain `pub use` of the function does not carry.
mod avtransport;
mod caption;
mod control;
mod description;
mod discovery;
mod headless;
mod metadata;
mod xml;

pub use headless::{probe, selftest};
pub(crate) use control::http;
pub(crate) use description::absolute;
pub(crate) use discovery::{header, ssdp_sockets};
pub(crate) use xml::{chunks, tag};

use avtransport::{load_url, parse_hms, seek_to};
use caption::attach_subtitle;
use control::{protocol_info, soap, summarize_sink};
use description::describe;
use discovery::collect_renderers;
use metadata::{extension_of, media_duration, negotiate_mime};

pub(crate) const SSDP_ADDR: &str = "239.255.255.250:1900";
const AVTRANSPORT: &str = "urn:schemas-upnp-org:service:AVTransport:1";
const CONNECTION_MANAGER: &str = "urn:schemas-upnp-org:service:ConnectionManager:1";
const RENDERING_CONTROL: &str = "urn:schemas-upnp-org:service:RenderingControl:1";

// ---- The transport ---------------------------------------------------------
//
// DLNA is the third way this player reaches a television, and the one that
// asks the least of us: the renderer is the TV's **own** player, so a release
// it already decodes — measured: MKV, 4K HEVC Main-10 HDR10, E-AC-3 5.1 —
// plays untouched, with seeking, from the same Range server the Cast path uses.
// What it costs is that every device answers differently; hence the ladder is
// read from `GetProtocolInfo` rather than assumed.
//
// The command surface deliberately mirrors cast/mod.rs (`*_connect`, `*_load`,
// `*_status`, `*_control`, `*_disconnect`) and reports the same status shape,
// so the frontend keeps one store and one casting screen and only chooses which
// command name to call.

/// One renderer as the picker shows it, and as `dlna_connect` takes it back.
#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
pub struct DlnaDeviceInfo {
    pub id: String,
    pub name: String,
    pub model: String,
    pub ip: String,
    pub control_url: String,
    pub rendering_url: Option<String>,
    /// MIME types the renderer says it accepts — the capability answer Cast
    /// never gives, and what the compatibility line in the picker is built on.
    pub mimes: Vec<String>,
}

#[derive(Default)]
struct DlnaState {
    /// Same vocabulary as cast/castv2.rs: connecting | loading | playing | paused |
    /// buffering | ended | stopped | error.
    state: String,
    error: Option<String>,
    time: f64,
    reported_at: Option<std::time::Instant>,
    duration: f64,
    volume: f64,
    volume_known: bool,
    /// Whether a non-zero level was ever reported — see the poll for why a
    /// perpetual zero is read as "no volume control here" rather than "muted".
    saw_volume: bool,
    /// A command was just sent and its optimistic state must survive the
    /// renderer's lagging reports until then. **This is the pause bug**: the LG
    /// answers `Pause` with a proper response and then keeps saying `PLAYING`
    /// for a poll or two (after a load it says `TRANSITIONING` for six), so the
    /// next status overwrote the optimism and the button sprang back — the same
    /// shape as the seekbar's `seekSettling`, and the same answer.
    settle_until: Option<std::time::Instant>,
    /// Where a seek was aimed while the renderer was paused, and until when to
    /// trust that over the renderer's own answer. **Measured: a paused renderer
    /// keeps reporting the position it had before the seek** until playback
    /// resumes, so believing it would drag the knob back to where the film was
    /// — the same "jumps and returns" the chapter list used to show, from the
    /// other end.
    seek_target: Option<(f64, std::time::Instant)>,
    /// Which `Seek` unit this renderer answered to, once one has. `None` means
    /// the ladder in [`seek_to`] has not been walked yet.
    seek_unit: Option<&'static str>,
    /// Whether the renderer has ever reported `PLAYING` this session. Until it
    /// has, a `TransportStatus` of `ERROR_OCCURRED` is a failed load; after it
    /// has, it is noise from a device that recovered.
    saw_playing: bool,
}

/// How long an optimistic state outranks the renderer's own report. Long
/// enough to cover this TV's observed lag, short enough that a command the
/// device actually refused shows up as one flicker rather than a stuck button.
const SETTLE: Duration = Duration::from_millis(2500);

/// How close to the duration counts as "the film finished" rather than "someone
/// pressed stop". A renderer reports the position it last decoded, which lags
/// the end by a beat, and the last seconds of a file are credits nobody stops
/// deliberately.
const END_SLACK: f64 = 8.0;

/// How long a paused seek's target outranks the renderer's own position. Long
/// enough to cover a viewer studying the frame before pressing play, bounded so
/// a renderer that never catches up cannot freeze the knob for good.
const PAUSED_SEEK_HOLD: Duration = Duration::from_secs(120);

/// How long to keep asking a renderer to start before calling the load failed.
/// Deliberately bounded well inside the frontend's own "fetched and never
/// played" timeout: once the television is preparing, judging whether anything
/// ever appears is that detector's job and not this one's.
const PLAY_BUDGET: Duration = Duration::from_secs(12);

/// Between `Play` attempts. A renderer that refuses because it is still
/// bringing its player up answers at once, so this pause is the whole cost of
/// a retry.
const PLAY_RETRY: Duration = Duration::from_millis(1200);

struct DlnaSession {
    device: DlnaDeviceInfo,
    state: Arc<Mutex<DlnaState>>,
    poll: tauri::async_runtime::JoinHandle<()>,
}

#[derive(Default)]
struct DlnaInner {
    /// Discovery runs only while the picker is open, exactly like the mDNS
    /// browse: nothing about this app is on the network until asked.
    discovery: Option<(tauri::async_runtime::JoinHandle<()>, Arc<Mutex<Vec<DlnaDeviceInfo>>>)>,
    session: Option<DlnaSession>,
}

#[derive(Default)]
pub struct DlnaService {
    inner: Mutex<DlnaInner>,
}

use std::sync::{Arc, Mutex};

/// Claim a state now and hold it against the renderer's lagging reports;
/// returns what it was, so a refused command can put it back.
fn arm(state: &Arc<Mutex<DlnaState>>, next: &str) -> String {
    let mut guard = state.lock().unwrap_or_else(|p| p.into_inner());
    let previous = guard.state.clone();
    guard.state = next.to_string();
    guard.settle_until = Some(std::time::Instant::now() + SETTLE);
    previous
}

fn set_state(state: &Arc<Mutex<DlnaState>>, f: impl FnOnce(&mut DlnaState)) {
    let mut guard = state.lock().unwrap_or_else(|p| p.into_inner());
    f(&mut guard);
}

#[tauri::command]
pub fn dlna_discover_start(
    service: tauri::State<'_, Arc<DlnaService>>,
    hints: Option<Vec<String>>,
) -> Result<(), String> {
    let mut inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    if inner.discovery.is_some() {
        return Ok(());
    }
    let hints = crate::lan_sweep::parse_hints(&hints.unwrap_or_default());
    let list: Arc<Mutex<Vec<DlnaDeviceInfo>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = list.clone();
    // Re-searched rather than searched once: SSDP replies are UDP and a device
    // that missed the first M-SEARCH would otherwise never appear.
    let task = tauri::async_runtime::spawn(async move {
        loop {
            let found = collect_renderers(Duration::from_secs(3), &hints, &sink).await;
            if !found.is_empty() {
                *sink.lock().unwrap_or_else(|p| p.into_inner()) = found;
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
    inner.discovery = Some((task, list));
    Ok(())
}

#[tauri::command]
pub fn dlna_discover_stop(service: tauri::State<'_, Arc<DlnaService>>) {
    let mut inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    if let Some((task, _)) = inner.discovery.take() {
        task.abort();
    }
}

#[tauri::command]
pub fn dlna_devices(service: tauri::State<'_, Arc<DlnaService>>) -> Vec<DlnaDeviceInfo> {
    let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    inner
        .discovery
        .as_ref()
        .map(|(_, list)| list.lock().unwrap_or_else(|p| p.into_inner()).clone())
        .unwrap_or_default()
}

/// Same shape as `CastStatus` — the frontend reads one store whichever
/// transport is live.
#[derive(serde::Serialize, Default)]
pub struct DlnaStatus {
    pub state: String,
    pub error: Option<String>,
    pub time: f64,
    pub duration: f64,
    pub volume: f64,
    pub muted: bool,
    pub volume_known: bool,
    pub volume_fixed: bool,
    pub fetches: u64,
    pub device: Option<String>,
}

#[tauri::command]
pub fn dlna_connect(
    service: tauri::State<'_, Arc<DlnaService>>,
    device: DlnaDeviceInfo,
) -> Result<(), String> {
    let mut inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(old) = inner.session.take() {
        old.poll.abort();
    }
    let state = Arc::new(Mutex::new(DlnaState {
        state: "connecting".into(),
        volume: 1.0,
        ..Default::default()
    }));
    let poll_state = state.clone();
    let control = device.control_url.clone();
    let rendering = device.rendering_url.clone();
    // The renderer is polled rather than subscribed to: GENA eventing would
    // mean running a callback HTTP server and renewing subscriptions, for a
    // position that arrives once a second either way.
    let poll = tauri::async_runtime::spawn(async move {
        let client = http();
        loop {
            tokio::time::sleep(Duration::from_millis(900)).await;
            let loaded = {
                let s = poll_state.lock().unwrap_or_else(|p| p.into_inner());
                !matches!(s.state.as_str(), "connecting" | "stopped" | "error")
            };
            if !loaded {
                continue;
            }
            if let Ok(info) = soap(client, &control, AVTRANSPORT, "GetTransportInfo", "").await {
                if crate::logs::verbose() {
                    eprintln!("[dlna] GetTransportInfo -> {}", info.replace('\n', " "));
                }
                let transport = tag(&info, "CurrentTransportState").unwrap_or("").to_string();
                // **A renderer that failed says so, and we were not listening.**
                // `CurrentTransportStatus` goes to `ERROR_OCCURRED` when the
                // device gave up on the stream — which it reports in the same
                // breath as a transport state of `STOPPED` or even `PLAYING`,
                // so the state alone cannot tell a failure from somebody
                // pressing stop. Read before the first `PLAYING` it is a load
                // that died, and saying so beats waiting out the frontend's
                // "fetched and never played" timeout; read afterwards it is a
                // device that stumbled and recovered, and acting on it would
                // tear down a session that is still watchable.
                let failed = tag(&info, "CurrentTransportStatus")
                    .is_some_and(|s| s.eq_ignore_ascii_case("ERROR_OCCURRED"));
                set_state(&poll_state, |s| {
                    if transport == "PLAYING" {
                        s.saw_playing = true;
                    }
                    if failed && !s.saw_playing {
                        s.state = "error".into();
                        s.error = Some("load_failed".into());
                        s.settle_until = None;
                        return;
                    }
                    if s.settle_until.map(|t| t > std::time::Instant::now()).unwrap_or(false) {
                        return;
                    }
                    s.settle_until = None;
                    s.state = match transport.as_str() {
                        "PLAYING" => "playing".into(),
                        "PAUSED_PLAYBACK" | "PAUSED_RECORDING" => "paused".into(),
                        "TRANSITIONING" => "buffering".into(),
                        // A renderer that reaches the end simply stops, so
                        // "finished" and "stopped from the TV" arrive as one
                        // state — and they must not stay one, or pressing stop
                        // on the remote would advance the queue to the next
                        // episode. The position decides: within the last few
                        // seconds of a known duration it is the end of a film,
                        // anything earlier is somebody stopping it.
                        "STOPPED" | "NO_MEDIA_PRESENT" => {
                            let finished = s.duration > 0.0 && s.time >= s.duration - END_SLACK;
                            if finished { "ended".into() } else { "stopped".into() }
                        }
                        _ => s.state.clone(),
                    };
                });
            }
            if let Ok(pos) = soap(client, &control, AVTRANSPORT, "GetPositionInfo", "").await {
                // **`RelTime` is optional and plenty of renderers answer it with
                // the literal `NOT_IMPLEMENTED`**, which parses as nothing and
                // leaves the seekbar frozen at zero for the whole film. `AbsTime`
                // carries the same number on a single-track stream, so it is the
                // fallback rather than a second feature.
                let time = tag(&pos, "RelTime")
                    .and_then(parse_hms)
                    .or_else(|| tag(&pos, "AbsTime").and_then(parse_hms));
                let dur = tag(&pos, "TrackDuration").and_then(parse_hms);
                set_state(&poll_state, |s| {
                    if let Some(t) = time {
                        // A pending paused seek outranks a report that has not
                        // caught up with it; the moment one lands near the
                        // target — or the deadline passes — the renderer is
                        // believed again.
                        let stale = match s.seek_target {
                            Some((target, until)) => {
                                if (t - target).abs() < 3.0 || std::time::Instant::now() > until {
                                    s.seek_target = None;
                                    false
                                } else {
                                    true
                                }
                            }
                            None => false,
                        };
                        if !stale {
                            s.time = t;
                            s.reported_at = Some(std::time::Instant::now());
                        }
                    }
                    if let Some(d) = dur {
                        if d > 0.0 {
                            s.duration = d;
                        }
                    }
                });
            }
            if let Some(rc) = &rendering {
                // **A renderer having RenderingControl does not mean it will
                // let us use it.** Measured on the LG: `GetVolume` answers UPnP
                // **606 "Action not authorized"**, i.e. the service is present
                // and closed to third-party senders. So the reading is
                // authoritative in both directions — a failure marks the volume
                // *unknown*, which disables the slider and routes keys and the
                // wheel to the existing "volume lives on the TV's own remote"
                // explanation, exactly as a Cast device with `controlType:
                // fixed` does. Leaving a stale value there would show a slider
                // that moves and changes nothing.
                let args = "<Channel>Master</Channel>";
                let reading = match soap(client, rc, RENDERING_CONTROL, "GetVolume", args).await {
                    Ok(v) => {
                        if crate::logs::verbose() {
                            eprintln!("[dlna] GetVolume -> {}", v.replace('\n', " "));
                        }
                        tag(&v, "CurrentVolume").and_then(|s| s.parse::<f64>().ok())
                    }
                    Err(e) => {
                        if crate::logs::verbose() {
                            eprintln!("[dlna] {e}");
                        }
                        None
                    }
                };
                // **A reading of zero from a television that is audibly not
                // silent is not a volume.** Measured on the LG: `GetVolume`
                // answers 200 with `<CurrentVolume>0</CurrentVolume>` while the
                // set plays at almost full, and the same action asked from
                // curl comes back UPnP **606 "Action not authorized"** — the
                // service is there and its Master channel is not wired to the
                // speakers. So a level is trusted once a non-zero one has been
                // seen, and until then the slider stays disabled and keys and
                // wheel give the existing "volume lives on the TV's own remote"
                // answer. A device that is genuinely at zero costs one such
                // message; the alternative is a slider that moves and does
                // nothing, or worse, one that jumps the room to full.
                set_state(&poll_state, |s| match reading {
                    Some(level) => {
                        if level > 0.0 {
                            s.saw_volume = true;
                        }
                        s.volume = (level / 100.0).clamp(0.0, 1.0);
                        s.volume_known = s.saw_volume;
                    }
                    None => s.volume_known = false,
                });
            }
        }
    });
    inner.session = Some(DlnaSession { device, state, poll });
    Ok(())
}

#[tauri::command]
pub async fn dlna_load(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<DlnaService>>,
    cast_service: tauri::State<'_, Arc<crate::cast::CastService>>,
    path: String,
    position: f64,
    title: Option<String>,
    hidden: bool,
    subtitle: Option<String>,
) -> Result<(), String> {
    let (ip, advertised) = {
        let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
        let device = &inner.session.as_ref().ok_or("not connected")?.device;
        (device.ip.clone(), device.mimes.clone())
    };
    let file = std::path::PathBuf::from(&path);
    if !file.is_file() {
        return Err("no such file".into());
    }
    let ip: std::net::IpAddr = ip.parse().map_err(|_| "bad device address".to_string())?;
    let url = crate::cast::serve_one_file(&cast_service, ip, &file, hidden).await?;
    // `cast_mime_for` answers for the Cast ladder, where anything but MP4 and
    // WebM is prepared first — so it says `application/octet-stream` for the
    // containers only this transport ever carries. Ask the renderer instead.
    let mime = negotiate_mime(
        &advertised,
        &extension_of(&path),
        crate::cast::cast_mime_for(&file),
    );
    let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    let duration = media_duration(&file);
    let caption = attach_subtitle(&app, &cast_service, subtitle).await;
    load_url(&service, url, mime, size, duration, position, title, caption).await
}

/// The same load for a URL somebody else registered — the torrent stream, where
/// there is no file to open and the size and duration come from the torrent and
/// from mpv instead of from a container header.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn dlna_load_url(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<DlnaService>>,
    cast_service: tauri::State<'_, Arc<crate::cast::CastService>>,
    url: String,
    mime: String,
    size: u64,
    duration: f64,
    position: f64,
    title: Option<String>,
    subtitle: Option<String>,
) -> Result<(), String> {
    let duration = (duration > 0.0).then_some(duration);
    let caption = attach_subtitle(&app, &cast_service, subtitle).await;
    load_url(&service, url, mime, size, duration, position, title, caption).await
}

#[tauri::command]
pub fn dlna_status(
    service: tauri::State<'_, Arc<DlnaService>>,
    cast_service: tauri::State<'_, Arc<crate::cast::CastService>>,
) -> DlnaStatus {
    let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
    let Some(session) = inner.session.as_ref() else {
        return DlnaStatus {
            state: "idle".into(),
            ..Default::default()
        };
    };
    let s = session.state.lock().unwrap_or_else(|p| p.into_inner());
    // Extrapolated between the renderer's one-second reports, exactly as the
    // Cast status is, or the seekbar steps once a second.
    let time = match (&s.reported_at, s.state.as_str()) {
        (Some(at), "playing") => s.time + at.elapsed().as_secs_f64(),
        _ => s.time,
    };
    DlnaStatus {
        state: s.state.clone(),
        error: s.error.clone(),
        time,
        duration: s.duration,
        volume: s.volume,
        muted: false,
        volume_known: s.volume_known,
        // A renderer with no RenderingControl cannot take a volume; one that has
        // it is adjustable, and DLNA has no "fixed" declaration to read.
        volume_fixed: session.device.rendering_url.is_none(),
        fetches: crate::cast::server_hits(&cast_service),
        device: Some(session.device.name.clone()),
    }
}

#[tauri::command]
pub async fn dlna_control(
    service: tauri::State<'_, Arc<DlnaService>>,
    action: String,
    value: Option<f64>,
) -> Result<(), String> {
    let (control, rendering, state) = {
        let inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
        let session = inner.session.as_ref().ok_or("not connected")?;
        (
            session.device.control_url.clone(),
            session.device.rendering_url.clone(),
            session.state.clone(),
        )
    };
    let client = http();
    match action.as_str() {
        // **The optimism has to be armed before the round trip, not after it.**
        // A SOAP call to the television takes a few hundred milliseconds, and
        // the frontend polls twice a second: set the state afterwards and a
        // poll lands in the gap, reads the state the command is in the middle
        // of changing, and the button flips to the new icon, back to the old
        // one and forward again. Arming first closes the window; a refusal
        // rolls the state back, which is the only reason the previous value is
        // captured at all.
        "play" | "pause" => {
            let playing = action == "play";
            let previous = arm(&state, if playing { "playing" } else { "paused" });
            let call = if playing {
                soap(client, &control, AVTRANSPORT, "Play", "<Speed>1</Speed>").await
            } else {
                soap(client, &control, AVTRANSPORT, "Pause", "").await
            };
            if let Err(e) = call {
                set_state(&state, |s| {
                    s.state = previous;
                    s.settle_until = None;
                });
                return Err(e);
            }
        }
        "seek" => {
            let target = value.unwrap_or(0.0).max(0.0);
            // Same rule as above, and for the knob rather than the icon: the
            // position is claimed before the call, or a poll in the gap drags
            // it back to where the film was.
            let was_paused = {
                let s = state.lock().unwrap_or_else(|p| p.into_inner());
                s.state == "paused"
            };
            set_state(&state, |s| {
                s.time = target;
                s.reported_at = Some(std::time::Instant::now());
                s.settle_until = Some(std::time::Instant::now() + SETTLE);
            });
            if was_paused {
                // **This renderer refuses to seek while paused** — measured,
                // UPnP 501 "Action Failed" — so the standard controller remedy
                // applies: resume, seek, pause again. Verified end to end
                // against the television: the position moves, the state comes
                // back paused and the TV fetches the new byte range. It costs a
                // fraction of a second of playback, which is the honest price
                // of a seek on a device that has no other way to do one; the
                // alternative is a seekbar that refuses to work whenever the
                // film is paused, which is when a viewer most often reaches for
                // it.
                let _ = soap(client, &control, AVTRANSPORT, "Play", "<Speed>1</Speed>").await;
                tokio::time::sleep(Duration::from_millis(150)).await;
                let seeked = seek_to(client, &control, &state, target).await;
                tokio::time::sleep(Duration::from_millis(150)).await;
                let _ = soap(client, &control, AVTRANSPORT, "Pause", "").await;
                set_state(&state, |s| {
                    s.state = "paused".into();
                    s.settle_until = Some(std::time::Instant::now() + SETTLE);
                    s.time = target;
                    s.reported_at = Some(std::time::Instant::now());
                    s.seek_target = Some((target, std::time::Instant::now() + PAUSED_SEEK_HOLD));
                });
                seeked?;
            } else {
                seek_to(client, &control, &state, target).await?;
            }
        }
        "volume" => {
            let rc = rendering.ok_or("device has no volume control")?;
            let level = (value.unwrap_or(0.0).clamp(0.0, 1.0) * 100.0).round() as u32;
            let args = format!("<Channel>Master</Channel><DesiredVolume>{level}</DesiredVolume>");
            if let Err(e) = soap(client, &rc, RENDERING_CONTROL, "SetVolume", &args).await {
                // Refused (606 on the measured television): stop claiming the control exists,
                // so the slider disables itself on the next poll instead of
                // moving without effect.
                set_state(&state, |s| s.volume_known = false);
                return Err(e);
            }
            set_state(&state, |s| s.volume = level as f64 / 100.0);
        }
        "mute" => {
            let rc = rendering.ok_or("device has no volume control")?;
            let on = value.unwrap_or(0.0) > 0.5;
            let args = format!(
                "<Channel>Master</Channel><DesiredMute>{}</DesiredMute>",
                if on { 1 } else { 0 }
            );
            soap(client, &rc, RENDERING_CONTROL, "SetMute", &args).await?;
        }
        _ => return Err(format!("unknown action {action}")),
    }
    Ok(())
}

/// Stop the renderer and tear the session down; returns the last position, so
/// the handback to mpv lands where the television was.
#[tauri::command]
pub async fn dlna_disconnect(
    service: tauri::State<'_, Arc<DlnaService>>,
    cast_service: tauri::State<'_, Arc<crate::cast::CastService>>,
) -> Result<f64, String> {
    // The LAN server is shared with the Cast transport and was left running
    // when a DLNA session ended — the token outliving its session is the one
    // thing this server promises not to do.
    crate::cast::release_server(&cast_service);
    let session = {
        let mut inner = service.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.session.take()
    };
    let Some(session) = session else {
        return Ok(0.0);
    };
    session.poll.abort();
    let last = {
        let s = session.state.lock().unwrap_or_else(|p| p.into_inner());
        s.time
    };
    let client = http();
    let _ = soap(client, &session.device.control_url, AVTRANSPORT, "Stop", "").await;
    Ok(last)
}

// ---- Diagnosis --------------------------------------------------------------

/// One line of a device report.
///
/// **`id` rather than a title**: this report is shown to a viewer, in a
/// localised window, and Rust cannot reach the dictionary — the same split the
/// macOS menu lives with. So the check names itself with a stable id the
/// frontend translates, and `detail` carries what only this side knows: the
/// addresses, the lists, the device's own error text. Data, not prose.
#[derive(serde::Serialize)]
pub struct CheckLine {
    pub id: String,
    /// ok | warn | fail | info — the report colors its rule from this.
    pub state: String,
    pub detail: String,
}

fn line(id: &str, state: &str, detail: impl Into<String>) -> CheckLine {
    CheckLine {
        id: id.to_string(),
        state: state.to_string(),
        detail: detail.into(),
    }
}

/// Everything this player can learn about a device without playing anything.
///
/// The reason it exists: when a cast fails on someone else's television, the
/// difference between "the network cannot reach it", "the receiver refused the
/// file" and "the device does not do this at all" is invisible from the outside
/// and decides everything. Every check here is one we already make somewhere in
/// the normal flow — this is those checks, run on demand, with their answers
/// written down instead of consumed.
///
/// It deliberately stops short of playing: launching an app on a television to
/// answer a diagnostic question is a surprise, and the fetch test that needs it
/// is a separate, announced step.
#[tauri::command]
pub async fn cast_diagnose(
    ip: String,
    cast_port: Option<u16>,
    dlna_location: Option<String>,
) -> Vec<CheckLine> {
    let mut out = Vec::new();
    let Ok(addr): Result<IpAddr, _> = ip.parse() else {
        out.push(line("address", "fail", ip.clone()));
        return out;
    };

    // Which of our own interfaces would serve this device, and therefore which
    // one the television has to be able to reach back on.
    match crate::cast::lan_ip_for_device(addr) {
        Some(local) => out.push(line("subnet", "ok", local.to_string())),
        None => out.push(line("subnet", "fail", String::new())),
    }

    if let Some(port) = cast_port {
        let t = std::time::Instant::now();
        match tokio::time::timeout(
            Duration::from_secs(5),
            tokio::net::TcpStream::connect((addr, port)),
        )
        .await
        {
            Ok(Ok(_)) => out.push(line("cast_port", "ok", format!("{addr}:{port} · {} ms", t.elapsed().as_millis()))),
            Ok(Err(e)) => out.push(line("cast_port", "fail", format!("{addr}:{port} · {e}"))),
            Err(_) => out.push(line("cast_port", "timeout", format!("{addr}:{port}"))),
        }
        match crate::cast::probe_receiver(addr, port).await {
            Ok(summary) => out.push(line("cast_handshake", "ok", summary)),
            Err(e) => out.push(line("cast_handshake", "fail", e)),
        }
    } else {
        out.push(line("cast_absent", "info", String::new()));
    }

    let Some(location) = dlna_location else {
        out.push(line("dlna_absent", "info", String::new()));
        return out;
    };

    let client = http();
    let Some(renderer) = describe(client, &location).await else {
        out.push(line("dlna_description", "fail", location.clone()));
        return out;
    };
    out.push(line("dlna_renderer", "ok", format!("{} · {} {}", renderer.friendly_name, renderer.manufacturer, renderer.model)));
    match &renderer.avtransport {
        Some(url) => out.push(line("avtransport", "ok", url.clone())),
        None => out.push(line("avtransport", "fail", String::new())),
    }

    if let Some(cm) = &renderer.connection_manager {
        match protocol_info(client, cm).await {
            Some(sink) => {
                let mimes = summarize_sink(&sink);
                let video: Vec<&String> = mimes.keys().filter(|m| m.starts_with("video/")).collect();
                out.push(line(
                    "formats",
                    if video.is_empty() { "warn" } else { "ok" },
                    format!(
                        "{} · {}",
                        mimes.len(),
                        video.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                    ),
                ));
            }
            None => out.push(line("formats", "warn", String::new())),
        }
    }

    // The two capabilities users notice missing, both read from the device's own
    // description rather than discovered by failing.
    if let Some(scpd) = &renderer.avtransport_scpd {
        if let Ok(r) = client.get(scpd).timeout(Duration::from_secs(5)).send().await {
            let xml = r.text().await.unwrap_or_default();
            let actions: Vec<String> = xml
                .split("<action>")
                .skip(1)
                .filter_map(|c| tag(c, "name").map(|s| s.to_string()))
                .collect();
            let seekable = actions.iter().any(|a| a == "Seek");
            out.push(line(
                "seeking",
                if seekable { "ok" } else { "warn" },
                actions.join(", "),
            ));
        }
    }
    if let Some(rc) = &renderer.rendering_control {
        let args = "<Channel>Master</Channel>";
        match soap(client, rc, RENDERING_CONTROL, "GetVolume", args).await {
            Ok(v) => {
                let level = tag(&v, "CurrentVolume").unwrap_or("?").to_string();
                out.push(line(
                    "volume",
                    if level == "0" { "warn" } else { "ok" },
                    level,
                ));
            }
            Err(e) => out.push(line("volume", "warn", e)),
        }
    }
    out
}

#[cfg(test)]
mod discovery_tests;
