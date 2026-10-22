//! AirPlay, handed to the system: an `AVPlayer` given the casting server's URL
//! and AVKit's route picker to choose the receiver. macOS only — see
//! docs/airplay.md for why there is no Windows half, and for what was measured
//! before any of this was written.
//!
//! The shape is the other two transports' on purpose: the same `CastStatus`
//! fields, the same four control actions, the same LAN server. What differs is
//! who chooses the television. There is no public API that lists AirPlay
//! receivers or picks one, so the menu's AirPlay row opens the **system's**
//! list — an invisible `AVRoutePickerView` laid over that row and pressed — and
//! the session exists from then on, waiting for the system to report an active
//! external route.
//!
//! `FP_AIRPLAY_PLAY=<file>` drives one session through the same functions the
//! commands call, logging each step — how the transport was measured, and how
//! it is checked against a new receiver.

use serde::{Deserialize, Serialize};

/// Where the menu row is, in the page's CSS pixels — the picker is put exactly
/// there so the system's list opens beside what the viewer clicked.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Anchor {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// `cast::CastStatus` plus the one thing only this transport needs: the
/// address that is fetching, which is how the frontend learns which television
/// was chosen (`AVPlayer` does not say on macOS) and names it from the DLNA or
/// Cast row with that address.
#[derive(Serialize, Default)]
pub struct AirPlayStatus {
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
    pub peer: Option<String>,
}

#[cfg(target_os = "macos")]
mod mac {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSButton, NSView, NSWindow};
    use objc2_av_foundation::{
        AVPlayer, AVPlayerItem, AVPlayerItemStatus, AVPlayerStatus, AVPlayerTimeControlStatus,
        AVRouteDetector,
    };
    use objc2_av_kit::{AVRoutePickerView, AVRoutePickerViewDelegate};
    use objc2_core_media::CMTime;
    use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL};
    use tauri::Manager as _;

    use super::{AirPlayStatus, Anchor};
    use crate::cast::CastService;

    const TAG: &str = "[airplay]";

    /// How long after the system's list closes a session may take to become an
    /// external route before it is called off. A television whose own AirPlay
    /// setting is off shows a message on its screen and tells the Mac nothing at
    /// all — no error, no state, no fetch — and a list closed without a pick
    /// looks exactly the same from here, so this timeout is the only way either
    /// ends. Measured: 1.5–3 s from a pick to `isExternalPlaybackActive`, and
    /// the list does **not** close on a pick — the viewer closes it later — so
    /// the route is normally external before this clock even starts. It was
    /// 15 s, which left a cancelled choice holding a session for that long.
    /// Unmeasured: a receiver that asks for a code typed on the Mac.
    const NOT_STARTED_AFTER: Duration = Duration::from_secs(4);

    /// A seek counts as landed once the reported position is this close to it.
    const SEEK_LANDED: f64 = 1.5;
    /// …or after this long, whatever the position says: a seek the receiver
    /// quietly refused must not pin the knob somewhere it is not.
    const SEEK_PATIENCE: Duration = Duration::from_secs(8);

    /// What survives a hop off the main thread: plain facts about the session.
    #[derive(Default)]
    struct Shared {
        /// Our address on the way out past any VPN, the server's binding.
        lan_ip: Option<IpAddr>,
        picker_closed_at: Option<Instant>,
        /// The route was external at some point: losing it now is a stop, not a
        /// session that never started.
        saw_external: bool,
        /// A session exists — the name browse outlives the menu for it.
        live: bool,
        /// The last state reported, for the wire log's one line per change.
        last_logged: String,
        /// The item's length as last reported while it was known. **At the end
        /// of an item AVPlayer reports a duration of zero** — measured, twice —
        /// so "paused at the end" has to be judged against what it said before.
        duration: f64,
        /// A seek sent and not yet seen to land. `currentTime` keeps answering
        /// the old position until the television has fetched the new one, so
        /// without this the knob jumps to the target, back, and to the target
        /// again — measured on the first seeks from the window.
        seek_target: Option<(f64, Instant)>,
    }

    static SHARED: Mutex<Option<Shared>> = Mutex::new(None);

    // ---- Names ------------------------------------------------------------
    //
    // macOS does not say which receiver the viewer chose; the receiver says it
    // by fetching, and the address it fetches from has a name in its own
    // Bonjour announcement. Reading that is the platform's service discovery,
    // not AirPlay: the instance name of `_airplay._tcp` is the name the set
    // shows in the system's list. It used to come from a DLNA or Cast row
    // with the same address, which named the set only when that row had been
    // found before the click — on a quick click it was "AirPlay".

    const AIRPLAY_SERVICE: &str = "_airplay._tcp.local.";

    static NAMES_DAEMON: Mutex<Option<mdns_sd::ServiceDaemon>> = Mutex::new(None);
    static NAMES: Mutex<Option<HashMap<IpAddr, String>>> = Mutex::new(None);

    /// Browse while the menu is up and while a session lives; the answers are
    /// kept after it stops, so a later session on the same set is named at once.
    fn names_start() {
        let mut d = NAMES_DAEMON.lock().unwrap_or_else(|p| p.into_inner());
        if d.is_some() {
            return;
        }
        let Ok(daemon) = mdns_sd::ServiceDaemon::new() else { return };
        let Ok(receiver) = daemon.browse(AIRPLAY_SERVICE) else { return };
        std::thread::spawn(move || {
            for event in receiver.iter() {
                if let mdns_sd::ServiceEvent::ServiceResolved(info) = event {
                    let name = info
                        .fullname
                        .strip_suffix(&format!(".{AIRPLAY_SERVICE}"))
                        .unwrap_or(&info.fullname)
                        .replace("\\.", ".")
                        .to_string();
                    let mut names = NAMES.lock().unwrap_or_else(|p| p.into_inner());
                    let names = names.get_or_insert_with(HashMap::new);
                    for addr in info.addresses.iter() {
                        names.insert(addr.to_ip_addr(), name.clone());
                    }
                }
            }
        });
        *d = Some(daemon);
    }

    fn names_stop() {
        if let Some(daemon) = NAMES_DAEMON.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = daemon.stop_browse(AIRPLAY_SERVICE);
            let _ = daemon.shutdown();
        }
    }

    fn name_of(ip: IpAddr) -> Option<String> {
        NAMES.lock().unwrap_or_else(|p| p.into_inner()).as_ref()?.get(&ip).cloned()
    }

    fn shared<T>(f: impl FnOnce(&mut Shared) -> T) -> T {
        let mut g = SHARED.lock().unwrap_or_else(|p| p.into_inner());
        f(g.get_or_insert_with(Shared::default))
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements and this class has
        // no Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "FPAirPlayPickerDelegate"]
        struct PickerDelegate;

        unsafe impl NSObjectProtocol for PickerDelegate {}

        unsafe impl AVRoutePickerViewDelegate for PickerDelegate {
            #[unsafe(method(routePickerViewWillBeginPresentingRoutes:))]
            fn will_begin(&self, _picker: &AVRoutePickerView) {
                if crate::cast::cast_debug() {
                    eprintln!("{TAG} the system's list is open");
                }
                shared(|s| s.picker_closed_at = None);
            }

            #[unsafe(method(routePickerViewDidEndPresentingRoutes:))]
            fn did_end(&self, picker: &AVRoutePickerView) {
                if crate::cast::cast_debug() {
                    eprintln!("{TAG} the system's list closed");
                }
                shared(|s| s.picker_closed_at = Some(Instant::now()));
                // An invisible view still takes clicks; once the list is gone it
                // must not sit over the page.
                picker.setHidden(true);
            }
        }
    );

    impl PickerDelegate {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(());
            unsafe { msg_send![super(this), init] }
        }
    }

    /// The objects of one session; main-thread-only, so they live in a
    /// main-thread local and every access is a hop there.
    struct Session {
        player: Retained<AVPlayer>,
        picker: Retained<AVRoutePickerView>,
        // Kept alive here: the picker holds its delegate weakly.
        _delegate: Retained<PickerDelegate>,
        unmuted: bool,
    }

    thread_local! {
        static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
        static DETECTOR: RefCell<Option<Retained<AVRouteDetector>>> = const { RefCell::new(None) };
    }

    /// The address an AirPlay receiver on this LAN can fetch from. There is no
    /// device address to choose an interface by — the system picks the
    /// receiver after the URL exists — so it is the way out of the machine past
    /// any VPN, and failing that the interface of the system route.
    fn lan_ip() -> Option<IpAddr> {
        let view = crate::net_route::view();
        if let Some(name) = view.direct.as_deref() {
            let addr = view
                .interfaces
                .iter()
                .find(|i| i.name == name)
                .and_then(|i| i.addr.as_deref())
                .and_then(|a| a.parse().ok());
            if addr.is_some() {
                return addr;
            }
        }
        let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
        sock.connect(SocketAddr::from((Ipv4Addr::new(1, 1, 1, 1), 53))).ok()?;
        Some(sock.local_addr().ok()?.ip())
    }

    async fn on_main<T: Send + 'static>(
        app: &tauri::AppHandle,
        f: impl FnOnce(MainThreadMarker) -> T + Send + 'static,
    ) -> Result<T, String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        app.run_on_main_thread(move || {
            let mtm = MainThreadMarker::new().expect("run_on_main_thread runs on the main thread");
            let _ = tx.send(f(mtm));
        })
        .map_err(|e| e.to_string())?;
        rx.await.map_err(|_| "main thread dropped the call".to_string())
    }

    fn content_view(window: &tauri::WebviewWindow) -> Option<Retained<NSView>> {
        let ptr = window.ns_window().ok()? as *mut NSWindow;
        if ptr.is_null() {
            return None;
        }
        let ns = unsafe { Retained::retain(ptr) }?;
        ns.contentView()
    }

    /// The anchor in the content view's coordinates. The web view covers the
    /// whole content view (the title bar is transparent and full-size), so a CSS
    /// pixel is a point and only the vertical axis may need turning over.
    fn frame_for(view: &NSView, a: Anchor) -> NSRect {
        let h = view.frame().size.height;
        let y = if view.isFlipped() { a.y } else { h - a.y - a.h };
        NSRect::new(NSPoint::new(a.x, y), NSSize::new(a.w.max(1.0), a.h.max(1.0)))
    }

    fn item_for(url: &str, position: f64, mtm: MainThreadMarker) -> Result<Retained<AVPlayerItem>, String> {
        let nsurl = NSURL::URLWithString(&NSString::from_str(url)).ok_or("bad url")?;
        let item = unsafe { AVPlayerItem::playerItemWithURL(&nsurl, mtm) };
        if position > 0.5 {
            unsafe { item.seekToTime_completionHandler(CMTime::with_seconds(position, 600), None) };
        }
        Ok(item)
    }

    /// The picker's own button. It is `AVRoutePickerButton`, a private subclass
    /// of `NSButton` (measured) — so what is relied on is only "a button",
    /// never the class name.
    fn picker_button(picker: &NSView) -> Option<Retained<NSButton>> {
        picker.subviews().iter().find_map(|sub| sub.downcast::<NSButton>().ok())
    }

    fn present(picker: &AVRoutePickerView) -> Result<(), String> {
        let button = picker_button(picker).ok_or("picker_unavailable")?;
        picker.setHidden(false);
        shared(|s| s.picker_closed_at = None);
        unsafe { button.performClick(None) };
        Ok(())
    }

    fn open_on_main(
        window: &tauri::WebviewWindow,
        url: &str,
        position: f64,
        anchor: Option<Anchor>,
        mtm: MainThreadMarker,
    ) -> Result<(), String> {
        let view = content_view(window).ok_or("no content view")?;
        close_on_main();

        let item = item_for(url, position, mtm)?;
        let player = unsafe { AVPlayer::playerWithPlayerItem(Some(&item), mtm) };
        unsafe {
            player.setAllowsExternalPlayback(true);
            player.setUsesExternalPlaybackWhileExternalScreenIsActive(true);
            // Playing muted while the viewer chooses is the measured
            // arrangement: the picked television picks up where the player is,
            // and the Mac makes no sound of its own meanwhile. Unmuted the
            // moment the route is external.
            player.setMuted(true);
        }

        let anchor = anchor.unwrap_or(Anchor { x: 90.0, y: 8.0, w: 40.0, h: 40.0 });
        let picker = unsafe {
            AVRoutePickerView::initWithFrame(AVRoutePickerView::alloc(mtm), frame_for(&view, anchor))
        };
        let delegate = PickerDelegate::new(mtm);
        unsafe {
            picker.setPlayer(Some(&player));
            picker.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        }
        // Invisible, not hidden: a hidden view presents nothing. The system's
        // list opens beside it as if the menu row had been its button.
        picker.setAlphaValue(0.0);
        view.addSubview(&picker);

        unsafe { player.play() };
        shared(|s| {
            s.saw_external = false;
            s.picker_closed_at = None;
            s.last_logged.clear();
            s.duration = 0.0;
            s.seek_target = None;
            s.live = true;
        });
        let result = present(&picker);
        SESSION.with(|s| {
            *s.borrow_mut() = Some(Session { player, picker, _delegate: delegate, unmuted: false });
        });
        result
    }

    fn close_on_main() -> f64 {
        SESSION.with(|s| {
            let Some(s) = s.borrow_mut().take() else { return 0.0 };
            let time = seconds(unsafe { s.player.currentTime() });
            unsafe {
                s.player.pause();
                s.player.replaceCurrentItemWithPlayerItem(None);
                s.player.setAllowsExternalPlayback(false);
                s.picker.setPlayer(None);
                s.picker.setDelegate(None);
            }
            s.picker.removeFromSuperview();
            time.max(0.0)
        })
    }

    fn seconds(t: CMTime) -> f64 {
        let s = unsafe { t.seconds() };
        if s.is_finite() { s } else { 0.0 }
    }

    fn status_on_main(lan_ip: Option<IpAddr>, peers: Vec<IpAddr>, hits: u64) -> AirPlayStatus {
        let peer = peers.into_iter().find(|p| Some(*p) != lan_ip);
        SESSION.with(|cell| {
            let mut cell = cell.borrow_mut();
            let Some(s) = cell.as_mut() else {
                return AirPlayStatus { state: "idle".into(), ..Default::default() };
            };
            let p = &s.player;
            let external = unsafe { p.isExternalPlaybackActive() };
            if external && !s.unmuted {
                unsafe { p.setMuted(false) };
                s.unmuted = true;
            }
            let saw = shared(|sh| {
                sh.saw_external |= external;
                sh.saw_external
            });
            let item = unsafe { p.currentItem() };
            let reported_time = seconds(unsafe { p.currentTime() });
            let time = shared(|sh| match sh.seek_target {
                Some((target, at)) if (reported_time - target).abs() > SEEK_LANDED && at.elapsed() < SEEK_PATIENCE => {
                    target
                }
                Some(_) => {
                    sh.seek_target = None;
                    reported_time
                }
                None => reported_time,
            });
            let reported = item.as_ref().map(|i| seconds(unsafe { i.duration() })).unwrap_or(0.0);
            let duration = shared(|sh| {
                if reported > 0.0 {
                    sh.duration = reported;
                }
                sh.duration
            });
            let failed = unsafe { p.status() } == AVPlayerStatus::Failed
                || item.as_ref().is_some_and(|i| unsafe { i.status() } == AVPlayerItemStatus::Failed);

            let (state, error) = if failed {
                ("error", Some("load_failed"))
            } else if !external {
                if saw {
                    ("stopped", None)
                } else if shared(|sh| sh.picker_closed_at.is_some_and(|t| t.elapsed() > NOT_STARTED_AFTER)) {
                    ("error", Some("not_started"))
                } else {
                    ("connecting", None)
                }
            } else {
                match unsafe { p.timeControlStatus() } {
                    AVPlayerTimeControlStatus::Playing => ("playing", None),
                    AVPlayerTimeControlStatus::Paused if duration > 0.0 && time >= duration - 1.0 => {
                        ("ended", None)
                    }
                    AVPlayerTimeControlStatus::Paused => ("paused", None),
                    _ => ("buffering", None),
                }
            };
            if crate::cast::cast_debug() {
                let line = format!("{state} {} peer={}", error.unwrap_or("-"), peer.map(|p| p.to_string()).unwrap_or("-".into()));
                shared(|sh| {
                    if sh.last_logged != line {
                        eprintln!("{TAG} {line} t={time:.1} dur={duration:.1}");
                        sh.last_logged = line;
                    }
                });
            }
            AirPlayStatus {
                state: state.into(),
                error: error.map(Into::into),
                time,
                duration,
                volume: 1.0,
                muted: false,
                // The television's volume is its own remote's: AVPlayer's volume
                // is the Mac's mix, and nothing measured says it reaches the set.
                volume_known: false,
                volume_fixed: true,
                // Only a fetch by somebody other than this Mac counts — the
                // player opens the URL itself while the viewer is choosing, and
                // those requests say nothing about the television's path to us.
                fetches: if peer.is_some() { hits } else { 0 },
                device: peer.and_then(name_of),
                peer: peer.map(|p| p.to_string()),
            }
        })
    }

    async fn serve(app: &tauri::AppHandle, path: &str, hidden: bool) -> Result<String, String> {
        let ip = lan_ip().ok_or("no usable network interface")?;
        shared(|s| s.lan_ip = Some(ip));
        let cast = app.state::<Arc<CastService>>().inner().clone();
        crate::cast::serve_one_file(&cast, ip, std::path::Path::new(path), hidden).await
    }

    pub async fn open(
        app: &tauri::AppHandle,
        path: String,
        position: f64,
        hidden: bool,
        anchor: Option<super::Anchor>,
    ) -> Result<(), String> {
        let url = serve(app, &path, hidden).await?;
        if crate::cast::cast_debug() {
            eprintln!("{TAG} open {url} at {position:.1}");
        }
        names_start();
        let window = app.get_webview_window("main").ok_or("no main window")?;
        on_main(app, move |mtm| open_on_main(&window, &url, position, anchor, mtm)).await?
    }

    /// Another file on the live route — the next episode, or the same film
    /// prepared with another audio track. The player and its route stay; only
    /// the item changes.
    pub async fn replace(app: &tauri::AppHandle, path: String, position: f64, hidden: bool) -> Result<(), String> {
        let url = serve(app, &path, hidden).await?;
        shared(|s| {
            s.duration = 0.0;
            s.seek_target = None;
        });
        on_main(app, move |mtm| {
            let item = item_for(&url, position, mtm)?;
            SESSION.with(|s| {
                let s = s.borrow();
                let s = s.as_ref().ok_or("not connected")?;
                unsafe {
                    s.player.replaceCurrentItemWithPlayerItem(Some(&item));
                    s.player.play();
                }
                Ok(())
            })
        })
        .await?
    }

    pub async fn status(app: &tauri::AppHandle) -> AirPlayStatus {
        let cast = app.state::<Arc<CastService>>().inner().clone();
        let peers = crate::cast::server_peers(&cast);
        let hits = crate::cast::server_hits(&cast);
        let lan_ip = shared(|s| s.lan_ip);
        on_main(app, move |_| status_on_main(lan_ip, peers, hits))
            .await
            .unwrap_or_else(|_| AirPlayStatus { state: "idle".into(), ..Default::default() })
    }

    pub async fn control(app: &tauri::AppHandle, action: String, value: Option<f64>) -> Result<(), String> {
        if crate::cast::cast_debug() {
            eprintln!("{TAG} control {action} {value:?}");
        }
        on_main(app, move |_| {
            SESSION.with(|s| {
                let s = s.borrow();
                let s = s.as_ref().ok_or("not connected")?;
                let p = &s.player;
                match action.as_str() {
                    "play" => unsafe { p.play() },
                    "pause" => unsafe { p.pause() },
                    // Measured forward and back: each lands, as a fresh Range
                    // request from the television at the new offset.
                    "seek" => {
                        let target = value.unwrap_or(0.0).max(0.0);
                        shared(|sh| sh.seek_target = Some((target, Instant::now())));
                        unsafe { p.seekToTime(CMTime::with_seconds(target, 600)) }
                    }
                    "volume" | "mute" => return Err("volume_fixed".to_string()),
                    _ => return Err("unknown action".to_string()),
                }
                Ok(())
            })
        })
        .await?
    }

    pub async fn disconnect(app: &tauri::AppHandle) -> f64 {
        let t = on_main(app, |_| close_on_main()).await.unwrap_or(0.0);
        shared(|s| s.live = false);
        names_stop();
        if crate::cast::cast_debug() {
            eprintln!("{TAG} disconnect at {t:.1}");
        }
        t
    }

    /// Route detection costs the system a browse, so it runs only while the
    /// cast menu is up — the same rule as the other two discoveries.
    pub async fn detect(app: &tauri::AppHandle, on: bool) {
        if on {
            names_start();
        } else if !shared(|s| s.live) {
            names_stop();
        }
        let _ = on_main(app, move |_| {
            DETECTOR.with(|d| {
                let mut d = d.borrow_mut();
                let det = d.get_or_insert_with(|| unsafe { AVRouteDetector::new() });
                unsafe { det.setRouteDetectionEnabled(on) };
            })
        })
        .await;
    }

    /// Whether there is any AirPlay receiver to offer — the row is hidden when
    /// there is none. Says nothing about which ones, and there is no public
    /// way to ask.
    pub async fn routes(app: &tauri::AppHandle) -> bool {
        on_main(app, |_| {
            DETECTOR.with(|d| d.borrow().as_ref().is_some_and(|d| unsafe { d.multipleRoutesDetected() }))
        })
        .await
        .unwrap_or(false)
    }

    /// One session, logged: the system's AirPlay list opens in the window's
    /// top-left corner, the viewer picks a television, and the script plays
    /// fifteen seconds, pauses, resumes, seeks a minute ahead and half a minute
    /// back, then stops.
    pub async fn selftest(app: tauri::AppHandle, path: String) {
        // The window has to exist before a picker can be put in it.
        tokio::time::sleep(Duration::from_secs(3)).await;
        detect(&app, true).await;
        if let Err(e) = open(&app, path, 0.0, false, None).await {
            eprintln!("{TAG} open failed: {e}");
            return;
        }
        eprintln!("{TAG} the AirPlay list is open in the top-left corner: pick a television");
        let mut last = String::new();
        let mut live_since: Option<u32> = None;
        for tick in 0..600u32 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let s = status(&app).await;
            let line = format!(
                "state={} error={} t={:.1} dur={:.1} fetches={} peer={} routes={}",
                s.state,
                s.error.as_deref().unwrap_or("-"),
                s.time,
                s.duration,
                s.fetches,
                s.peer.as_deref().unwrap_or("-"),
                routes(&app).await,
            );
            let key = format!("{} {} {:?}", s.state, s.peer.is_some(), s.error);
            if key != last || tick % 10 == 0 {
                eprintln!("{TAG} t+{tick}s {line}");
                last = key;
            }
            if matches!(s.state.as_str(), "error" | "stopped" | "ended") {
                break;
            }
            if s.state == "playing" && live_since.is_none() {
                live_since = Some(tick);
            }
            let Some(since) = live_since else { continue };
            let step = tick - since;
            let action = match step {
                15 => Some(("pause", None)),
                22 => Some(("play", None)),
                30 => Some(("seek", Some((s.time + 60.0).min(s.duration - 30.0)))),
                40 => Some(("seek", Some((s.time - 30.0).max(0.0)))),
                _ => None,
            };
            if let Some((action, value)) = action {
                eprintln!("{TAG} {action} {value:?} (at {:.1})", s.time);
                if let Err(e) = control(&app, action.into(), value).await {
                    eprintln!("{TAG} {action} failed: {e}");
                }
            }
            if step == 55 {
                break;
            }
        }
        let last_time = disconnect(&app).await;
        detect(&app, false).await;
        eprintln!("{TAG} done at {last_time:.1}");
    }
}

#[cfg(target_os = "macos")]
pub use mac::selftest;

#[cfg(not(target_os = "macos"))]
const UNSUPPORTED: &str = "AirPlay is macOS only";

#[tauri::command]
pub async fn airplay_open(
    app: tauri::AppHandle,
    path: String,
    position: f64,
    hidden: bool,
    anchor: Option<Anchor>,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return mac::open(&app, path, position, hidden, anchor).await;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, path, position, hidden, anchor);
        Err(UNSUPPORTED.into())
    }
}

#[tauri::command]
pub async fn airplay_replace(app: tauri::AppHandle, path: String, position: f64, hidden: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return mac::replace(&app, path, position, hidden).await;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, path, position, hidden);
        Err(UNSUPPORTED.into())
    }
}

#[tauri::command]
pub async fn airplay_status(app: tauri::AppHandle) -> AirPlayStatus {
    #[cfg(target_os = "macos")]
    return mac::status(&app).await;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        AirPlayStatus { state: "idle".into(), ..Default::default() }
    }
}

#[tauri::command]
pub async fn airplay_control(app: tauri::AppHandle, action: String, value: Option<f64>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return mac::control(&app, action, value).await;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, action, value);
        Err(UNSUPPORTED.into())
    }
}

#[tauri::command]
pub async fn airplay_disconnect(app: tauri::AppHandle) -> f64 {
    #[cfg(target_os = "macos")]
    return mac::disconnect(&app).await;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        0.0
    }
}

#[tauri::command]
pub async fn airplay_detect(app: tauri::AppHandle, on: bool) {
    #[cfg(target_os = "macos")]
    mac::detect(&app, on).await;
    #[cfg(not(target_os = "macos"))]
    let _ = (app, on);
}

#[tauri::command]
pub async fn airplay_routes(app: tauri::AppHandle) -> bool {
    #[cfg(target_os = "macos")]
    return mac::routes(&app).await;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        false
    }
}
