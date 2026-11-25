//! Windows: the `native:mpv` module and the start of the app, with mpv drawing through the D3D11
//! render API (see `mpv.rs`).
//!
//! `--sk-host-info` and `--sk-host-types` are Skathi's: nothing here refuses an unknown argument,
//! so `Builder` answers them.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::{commands, mpv, subtitle_fonts, sys, thumbs, velo};

struct MpvModule {
    core: Arc<mpv::Core>,
    pending: Mutex<Vec<String>>,
}

/// The events are the same on both platforms, so the page can listen to all of them: `band` and
/// `fullscreen` are macOS' (never sent here), `overlay` is Windows', `resizing` both (here from sys.rs's
/// subclass). An event the page subscribes to and the module lacks stops the app at start.
#[skathi::module(events(event = Value, remote = Value, band = Value, overlay = Value, resizing = Value, fullscreen = Value))]
impl MpvModule {
    /// The plugin's `init`: options in order, then `[name, format]` pairs to observe.
    pub fn init(&self, options: Vec<(String, Value)>, observed: Vec<(String, String)>) -> Result<(), String> {
        self.core.init(options, observed)
    }

    pub fn command(&self, name: String, args: Vec<Value>) -> Result<(), String> {
        self.core.command(&name, &args)
    }

    pub fn set_property(&self, name: String, value: Value) -> Result<(), String> {
        self.core.set_property(&name, &value)
    }

    pub fn get_property(&self, name: String, format: String) -> Result<Value, String> {
        self.core.get_property(&name, &format)
    }

    pub fn destroy(&self) {
        self.core.destroy();
    }

    /// The command line's files, once.
    pub fn take_pending_files(&self) -> Vec<String> {
        std::mem::take(&mut *self.pending.lock().unwrap())
    }

    // ---- The player's small commands (commands.rs), under their Tauri names in camelCase.

    pub fn folder_entries(&self, path: String) -> Result<Vec<String>, String> {
        commands::folder_entries(&path)
    }

    pub fn paths_exist(&self, paths: Vec<String>) -> Vec<bool> {
        commands::paths_exist(&paths)
    }

    #[task]
    pub fn external_track_dirs(&self, path: String) -> Value {
        commands::external_track_dirs(&path)
    }

    pub fn user_mpv_conf(&self) -> Result<Value, String> {
        commands::user_mpv_conf()
    }

    /// `value` null comments the key out.
    pub fn mpv_conf_set(&self, key: String, value: Value) -> Result<(), String> {
        let text = match &value {
            Value::Null => None,
            Value::String(s) => Some(s.clone()),
            other => Some(other.to_string()),
        };
        commands::mpv_conf_set(&key, text.as_deref())
    }

    pub fn third_party_notices(&self) -> Result<String, String> {
        commands::third_party_notices()
    }

    pub fn screenshot_dir(&self) -> Result<String, String> {
        commands::screenshot_dir()
    }

    pub fn screenshot_clip_path(&self) -> String {
        commands::screenshot_clip_path()
    }

    #[task]
    pub fn screenshot_to_clipboard(&self, path: String) -> Result<(), String> {
        commands::screenshot_to_clipboard(&path)
    }

    pub fn trash_file(&self, path: String) -> Result<(), String> {
        // trash.rs: the storyboard lets go of the file first, or it writes a cache for a file that is gone.
        thumbs::release_file(thumbs::state(), &path);
        commands::trash_file(&path)
    }

    pub fn primary_button_down(&self) -> bool {
        sys::primary_button_down()
    }

    pub fn double_click_time(&self) -> u32 {
        sys::double_click_ms()
    }

    /// `plugin:window|start_resize_dragging` (the page's resize strips): the system's sizing loop for
    /// that edge. `false` when nothing was started.
    pub fn start_resize(&self, direction: String) -> bool {
        sys::start_resize(self.core.window.load(std::sync::atomic::Ordering::Relaxed), &direction)
    }

    pub fn debug_log_state(&self) -> Value {
        commands::debug_log_state()
    }

    pub fn debug_log_set(&self, on: bool) -> Value {
        commands::debug_log_set(on)
    }

    pub fn subtitle_fonts_dir(&self) -> Result<String, String> {
        subtitle_fonts::subtitle_fonts_dir()
    }

    #[task]
    pub fn subtitle_fonts_list(&self) -> Result<Value, String> {
        subtitle_fonts::subtitle_fonts_list()
    }

    #[task]
    pub fn subtitle_font_add(&self, path: String) -> Result<Value, String> {
        subtitle_fonts::subtitle_font_add(&path)
    }

    pub fn subtitle_font_remove(&self, name: String) -> Result<(), String> {
        subtitle_fonts::subtitle_font_remove(&name)
    }

    /// now_playing.rs is the macOS Now Playing centre; the player has no Windows side to it
    /// (`now_playing_start` and `now_playing_set` do nothing there either). Kept so that the
    /// module's surface is the macOS port's.
    pub fn now_playing_start(&self) {}

    pub fn now_playing_set(&self, _info: Value) -> Result<(), String> {
        Ok(())
    }

    pub fn mpv_shader_cache_dir(&self) -> Result<String, String> {
        commands::mpv_shader_cache_dir()
    }

    /// lib.rs `zoom_pan`: the three properties back to back on one thread.
    pub fn zoom_pan(&self, z: f64, x: f64, y: f64) -> Result<(), String> {
        self.core.set_property("video-zoom", &Value::from(z))?;
        self.core.set_property("video-pan-x", &Value::from(x))?;
        self.core.set_property("video-pan-y", &Value::from(y))
    }

    /// FPUI_HIDDEN=1: the window is never shown (memory runs without a window on screen); the stub
    /// skips the interface's show/focus calls.
    pub fn hidden(&self) -> bool {
        std::env::var("FPUI_HIDDEN").is_ok_and(|v| v == "1")
    }

    /// What Tauri calls the resource directory: `lua/`, `shaders/` and the notices.
    pub fn resource_dir(&self) -> String {
        commands::resource_dir().to_string_lossy().into_owned()
    }

    /// Version 2's data directory, for the paths the interface asks Tauri for.
    pub fn data_dir(&self) -> String {
        commands::data_dir().to_string_lossy().into_owned()
    }

    /// Whether mpv draws into a layer under the window (the page then asks for a transparent backdrop).
    pub fn layer_route(&self) -> bool {
        matches!(self.core.route, mpv::Route::Layer | mpv::Route::SkLayer)
    }

    /// Read-only counters for the `sklayer` route: what `<sk-layer>` was told so far, and the display
    /// `on_display_change` last gave.
    pub fn sk_layer_stats(&self) -> Value {
        let s = self.core.sk_visual.lock().unwrap();
        let d = *self.core.display.lock().unwrap();
        serde_json::json!({ "attaches": s.attaches, "places": s.places, "hides": s.hides, "noComposition": s.no_composition,
            "display": d.map(|d| serde_json::json!({ "hdr": d.hdr, "sdrWhite": d.sdr_white_nits, "peak": d.peak_nits, "fullFrame": d.full_frame_nits, "potentialHeadroom": d.potential_headroom })) })
    }

    /// The fullscreen transition as an animation of the video's visual, without the page's black veil
    /// (mpv.rs `FsAnim`; `sklayer` only). The default; `FPUI_FS_ANIM=0` goes back to the fade through black, a number sets
    /// the length. The length in ms, 0 when off.
    pub fn fs_anim_ms(&self) -> u32 {
        if self.core.route != mpv::Route::SkLayer {
            return 0;
        }
        match std::env::var("FPUI_FS_ANIM") {
            Ok(v) if v == "1" => 220,
            Ok(v) => v.parse().unwrap_or(0),
            Err(_) => 220,
        }
    }

    /// Entering: remember the video's box on screen; the next size Skathi gives the layer animates.
    pub fn fs_anim_enter(&self, ms: u32) -> bool {
        let hwnd = self.core.window.load(std::sync::atomic::Ordering::Relaxed);
        unsafe { self.core.sk_visual.lock().unwrap().fs_enter(hwnd, ms) }
    }

    /// Leaving, while the window is still fullscreen: the visual shrinks to its box before entering. The
    /// page restores the window when `ms` is up; `false` when there is nothing to animate to.
    pub fn fs_anim_exit(&self, ms: u32) -> bool {
        let hwnd = self.core.window.load(std::sync::atomic::Ordering::Relaxed);
        unsafe { self.core.sk_visual.lock().unwrap().fs_exit(hwnd, ms) }
    }

    /// Whether the fullscreen transition started last is still under way in the host (mpv.rs `fs_busy`): the
    /// stub calls `fs_next` only once it reads `false`.
    pub fn fs_anim_busy(&self) -> bool {
        self.core.sk_visual.lock().unwrap().fs_busy()
    }

    /// The page asks for its fullscreen state through the host's queue (mpv.rs `FsQueue`): `true` when there is
    /// one, which is when the transition is animated (`fs_anim_ms` > 0). The stub then has the page ask at once.
    pub fn fs_queued(&self) -> bool {
        self.fs_anim_ms() > 0
    }

    /// A request for `on`, the window being `actual` now: "enter", "exit", "queued" or "none" (`FsQueue::request`).
    pub fn fs_request(&self, on: bool, actual: bool) -> String {
        self.core.sk_visual.lock().unwrap().queue.request(on, actual).into()
    }

    /// The transition the stub ran is over in the host (`fs_anim_busy` false), or was given up: what comes next.
    pub fn fs_next(&self, actual: bool) -> String {
        self.core.sk_visual.lock().unwrap().queue.next(actual).into()
    }

    /// Whether the transition about to move to `on` is still the last request.
    pub fn fs_wanted(&self, on: bool) -> bool {
        self.core.sk_visual.lock().unwrap().queue.wanted(on)
    }

    /// The state the window is heading to while a request runs or waits, `null` when idle (`is_fullscreen`).
    pub fn fs_target(&self) -> Value {
        self.core.sk_visual.lock().unwrap().queue.target().map_or(Value::Null, Value::Bool)
    }

    /// The shape the page holds the window to (`window_shape_lock`, beside Skathi's `setAspectRatio`), content
    /// width over height, 0 for none: a corner drag's leading side is decided by it (sys.rs `corner_lead`).
    pub fn set_aspect_hold(&self, ratio: f64) {
        sys::set_aspect_hold(ratio)
    }

    /// Read-only: the producer's counters (presents accepted, `Busy`, errors, the ring).
    pub fn producer_stats(&self) -> Value {
        self.core.stats()
    }

    // ---- thumb_service.rs (src/thumbs.rs): the storyboard, the hover frame, posters, crop, titles.
    // Bytes go to JS as a `Uint8Array` ([f64 key LE][JPEG], as the player's raw Response). What
    // touches the disk or decodes is a #[task], off the JS thread (thumb_start reads the cache file).

    #[task]
    pub fn thumb_start(&self, path: String, duration: f64) {
        // FPUI_NO_THUMBS=1 leaves the storyboard unbuilt (memory attribution).
        if std::env::var("FPUI_NO_THUMBS").is_ok_and(|v| v == "1") {
            return;
        }
        thumbs::thumb_start(path, duration)
    }

    #[task]
    pub fn thumb_get(&self, path: String, pos: f64, exact: bool) -> Result<Vec<u8>, String> {
        thumbs::thumb_get(path, pos, exact)
    }

    pub fn set_private_paths(&self, paths: Vec<String>, all: bool) {
        thumbs::set_private_paths(paths, all)
    }

    pub fn forget_thumbs(&self, path: String) {
        thumbs::forget_thumbs(path)
    }

    #[task]
    pub fn forget_thumbs_under(&self, folder: String) -> f64 {
        thumbs::forget_thumbs_under(folder) as f64
    }

    #[task]
    pub fn clear_thumb_cache(&self) {
        thumbs::clear_thumb_cache()
    }

    #[task]
    pub fn poster_frame(&self, path: String, pos: f64) -> Result<Vec<u8>, String> {
        thumbs::poster_frame(path, pos)
    }

    pub fn poster_saved(&self, id: String) -> Result<Vec<u8>, String> {
        thumbs::poster_saved(id)
    }

    #[task]
    pub fn poster_capture(&self, id: String, path: String, positions: Vec<f64>) -> Result<bool, String> {
        thumbs::poster_capture(id, path, positions)
    }

    /// `null` when there are no bars worth removing.
    #[task]
    pub fn crop_detect(&self, path: String, positions: Option<Vec<f64>>) -> Result<Value, String> {
        thumbs::crop_detect(path, positions).map(|r| serde_json::to_value(r).unwrap_or(Value::Null))
    }

    #[task]
    pub fn container_titles(&self, paths: Vec<String>) -> Vec<Option<String>> {
        thumbs::container_titles(paths)
    }

    /// The system's file dialog; empty when cancelled.
    #[task]
    pub fn open_dialog(&self, multiple: bool, directory: bool) -> Vec<String> {
        sys::open_panel(multiple, directory)
    }
}

/// `std::fs::canonicalize` answers a verbatim path (`\\?\C:\…`) on Windows; mpv and FFmpeg are
/// handed the ordinary form.
fn plain(p: PathBuf) -> String {
    let s = p.to_string_lossy().into_owned();
    match s.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s),
    }
}

pub fn main() {
    // Velopack's hooks (install, update, uninstall) run and exit here, before anything else.
    velo::startup();
    // `frameplayer --thumb-selftest OUT_DIR FILE…` (the `selftest` feature, never in a package): the
    // thumbnail commands on real files, no window.
    #[cfg(feature = "selftest")]
    {
        let args: Vec<String> = std::env::args().collect();
        if args.get(1).map(String::as_str) == Some("--thumb-selftest") {
            std::process::exit(thumbs::selftest(&args[2..]));
        }
    }
    let dll = std::env::var("MPV_DLL").unwrap_or_else(|_| {
        let exe = std::env::current_exe().unwrap_or_default();
        exe.parent().map(|d| d.join("libmpv-2.dll")).unwrap_or_default().to_string_lossy().into_owned()
    });
    // Sound on, as on macOS; FPUI_MUTE=1 silences mpv for test runs (ao=null, and the interface
    // cannot turn it back up).
    let mute = std::env::var("FPUI_MUTE").is_ok_and(|v| v == "1");
    let files: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .filter_map(|a| std::fs::canonicalize(&a).ok().map(plain).or_else(|| a.contains("://").then(|| a.clone())))
        .collect();
    let core = Arc::new(mpv::Core::new(dll, mute, mpv::Route::from_env()));
    let module = MpvModule { core: core.clone(), pending: Mutex::new(files) };
    let hidden = std::env::var("FPUI_HIDDEN").is_ok_and(|v| v == "1");
    let mut builder = skathi::Builder::new();
    // The backdrop is the host's on the route that composes under the window (skathi#48).
    if core.route == mpv::Route::SkLayer {
        builder = builder.backdrop("transparent");
    }
    let builder = builder
        .visible(!hidden)
        .activate(!hidden)
        .exact_frame_feedback(std::env::var("FPUI_EXACT_FEEDBACK").is_ok_and(|v| v == "1"))
        .payload_file(commands::payload_file())
        .data_dir(commands::data_dir())
        .module("mpv", module)
        .module("cast", crate::cast_module::CastModule)
        .on_display_change({
            // The window's display, when first known and when it changes (skathi#47).
            let (display, n) = (core.display.clone(), std::sync::atomic::AtomicU32::new(0));
            move |window, d| {
                let k = n.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                eprintln!(
                    "[fpui] on_display_change {k}: window {window}, known {}, hdr {}, potential_headroom {:.3}, sdr_white_nits {:?}, peak_nits {:?}, full_frame_nits {:?}, refresh {:?}",
                    d.known, d.hdr, d.potential_headroom, d.sdr_white_nits, d.peak_nits, d.full_frame_nits, d.refresh_hz
                );
                *display.lock().unwrap() = Some(d);
            }
        })
        .setup({
            let core = core.clone();
            move |host| {
                let _ = core.ui.set(host.handle());
                let _ = core.surface.set(host.surface("video"));
                let _ = core.overlay.set(host.surface("overlay"));
                // The layer of `<sk-layer name="video">` (skathi#44).
                if core.route == mpv::Route::SkLayer {
                    let shared = core.sk_visual.clone();
                    host.layer("video", move || Box::new(mpv::SkVideoLayer(shared.clone())));
                }
                // The window's handle, for the layer route.
                #[allow(deprecated)]
                if let Some(raw_window_handle::RawWindowHandle::Win32(h)) = host.unsafe_window_handle() {
                    core.window.store(h.hwnd.get(), std::sync::atomic::Ordering::Relaxed);
                    velo::set_window(h.hwnd.get());
                    // The bars step aside during a live resize (`resizing`), as version 1's WM_SIZING does.
                    // FPUI_RESIZING=0 leaves it out (the measurement's "before").
                    let sk = core.sk_visual.clone();
                    sys::on_window_moved(Box::new(move || unsafe { sk.lock().unwrap().fs_landed() }));
                    if std::env::var("FPUI_RESIZING").is_ok_and(|v| v == "0") {
                    } else if !sys::watch_resizing(h.hwnd.get(), host.handle()) {
                        eprintln!("[fpui] could not subclass the window: no `resizing` events");
                    }
                }
                Ok(())
            }
        });
    // Windows: Skathi runs on a thread of its own, and this one waits for the window to close.
    let ui = match builder.spawn() {
        Ok(ui) => ui,
        Err(e) => {
            eprintln!("{e}");
            velo::log(&format!("start failed: {e}"));
            std::process::exit(2);
        }
    };
    let exit = ui.wait();
    core.destroy();
    eprintln!("[frameplayer] exit clean={}", exit.clean);
    std::process::exit(i32::from(!exit.clean));
}
