//! macOS: the `native:mpv` module and the start of the app.
//!
//! The module carries libmpv in the shape `tauri-plugin-libmpv` gives the interface (`mpv.rs`), and
//! the player's small commands under their Tauri names in camelCase. `tauri-stub.ts` routes the
//! interface's `invoke` calls here.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::{child, commands, fullscreen, live_resize, mpv, now_playing, quit, subtitle_fonts, sys, thumbs, velo, vk};

/// The memory levers measured as the final combination (`FPUI_LEAN=1`, the default with the
/// `lean` feature). Each one stays overridable by its own variable.
const LEAN: &[(&str, &str)] = &[
    ("FPUI_OVL_RING", "1"),
    ("FPUI_OVL_DROP", "24"),
    // Not FPUI_OVL_BAND=auto: a line that appears above the band is missing for ~2 frames until the
    // page grows it.
    ("FPUI_NOJIT", "1"),
    ("FPUI_RING", "1"),
    ("FPUI_METALLIB", "1"),
];

struct MpvModule {
    core: Arc<mpv::Core>,
    pending: Mutex<Vec<String>>,
}

/// The events are the same on both platforms, so the page can listen to all of them: `overlay` is
/// Windows' (never sent here), `band`, `resizing` and `fullscreen` are macOS'.
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

    /// Windows' resize strips; the page shows none on macOS (AppKit resizes from the frame).
    pub fn start_resize(&self, _direction: String) -> bool {
        false
    }

    /// FPUI_FS_ANIM=1: non-zero, so the stub hides the
    /// interface and the veil for the transition and asks Skathi for AppKit's own (`system`) one;
    /// mpv's drawable keeps its size until the transition is over (live_resize.rs, child.rs
    /// `freeze`). The length is AppKit's (~550 ms, measured); this is how long the interface stays
    /// away after the request.
    pub fn fs_anim_ms(&self) -> u32 {
        if crate::fullscreen::mode() == crate::fullscreen::Mode::Fade { 0 } else { 600 }
    }

    /// macOS: the page's fullscreen request as the state it wants; the host queues it while a transition
    /// runs (fullscreen.m `fpui_fs_want`).
    pub fn fs_want(&self, on: bool) {
        crate::fullscreen::want(on);
    }

    /// The page asks for the state it wants and the host queues it (180eafd's contract; fullscreen.m
    /// `fpui_fs_want`), with the custom transition; the fade has none.
    pub fn fs_queued(&self) -> bool {
        self.fs_anim_ms() > 0
    }

    /// The state the window is heading to while a transition runs or a request waits, `null` when idle
    /// (`is_fullscreen`, fullscreen.m `fpui_fs_target`).
    pub fn fs_target(&self) -> Value {
        crate::fullscreen::target().map_or(Value::Null, Value::Bool)
    }

    /// Windows animates the picture itself; here AppKit does, so there is nothing to start.
    pub fn fs_anim_enter(&self, _ms: u32) -> bool {
        false
    }

    /// `false`: leave fullscreen at once, AppKit animates the way out.
    pub fn fs_anim_exit(&self, _ms: u32) -> bool {
        false
    }

    pub fn double_click_time(&self) -> u32 {
        sys::double_click_ms()
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

    /// now_playing.rs `now_playing_start`: the Now Playing command handlers, once. Their commands
    /// arrive on the `remote` event.
    pub fn now_playing_start(&self) {
        let core = self.core.clone();
        now_playing::start(std::sync::Arc::new(move |command: Value| {
            if let Some(ui) = core.ui.get() {
                let _ = ui.emit_to("mpv", "remote", &command);
            }
        }));
    }

    /// now_playing.rs `now_playing_set`, without the cover.
    pub fn now_playing_set(&self, info: Value) -> Result<(), String> {
        let info: now_playing::NowPlaying = serde_json::from_value(info).map_err(|e| e.to_string())?;
        now_playing::set(info);
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

    /// Whether mpv draws into a native layer under the page (`FPUI_ROUTE=child`). The backdrop is
    /// already pinned by the host there, so the page need not ask for it.
    pub fn layer_route(&self) -> bool {
        false
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

    /// The system open panel; empty when cancelled.
    #[task]
    pub fn open_dialog(&self, multiple: bool, directory: bool) -> Vec<String> {
        sys::open_panel(multiple, directory)
    }
}

pub fn main() {
    // Velopack's hooks run here, before anything else (velo.rs).
    velo::startup();
    // The app's own Vulkan loader and MoltenVK, never a system one (vk.rs).
    vk::default_env();
    // Before any thread starts. FPUI_LEAN=1: every memory lever that is on in the final build, each
    // still overridable by its own variable. The `lean` feature makes it the default; FPUI_LEAN=0
    // turns it off.
    let lean = match std::env::var("FPUI_LEAN") {
        Ok(v) => v == "1",
        Err(_) => cfg!(feature = "lean"),
    };
    if lean {
        for (k, v) in LEAN {
            if std::env::var_os(k).is_none() {
                std::env::set_var(k, v);
            }
        }
    }
    // FPUI_NOJIT=1: JavaScriptCore without its JIT and WebAssembly (it reads JSC_* at start).
    if std::env::var("FPUI_NOJIT").is_ok_and(|v| v == "1") {
        std::env::set_var("JSC_useJIT", "false");
        std::env::set_var("JSC_useWebAssembly", "false");
    }
    // FPUI_MTL_LOG=1: Metal's allocated size every 2 s; FPUI_MTL_LOG=<ms>: every <ms> ms.
    if let Ok(v) = std::env::var("FPUI_MTL_LOG") {
        let ms = if v == "1" { 2000 } else { v.parse().unwrap_or(0) };
        if ms > 0 {
            sys::log_metal_allocated(std::time::Duration::from_millis(ms));
        }
    }
    // `frameplayer --thumb-selftest OUT_DIR FILE…` (the `selftest` feature, never in a package): the
    // thumbnail commands on real files, no window.
    #[cfg(feature = "selftest")]
    {
        let args: Vec<String> = std::env::args().collect();
        if args.get(1).map(String::as_str) == Some("--thumb-selftest") {
            std::process::exit(thumbs::selftest(&args[2..]));
        }
    }
    // The default route draws through our Vulkan render API (mpv.rs `Route`), which a stock
    // libmpv does not have; FPUI_ROUTE=gl works with version 1's libmpv.
    let route = mpv::Route::from_env();
    let lib_dir = std::env::var("MPV_LIB_DIR").unwrap_or_else(|_| {
        let exe = std::env::current_exe().ok();
        // An installed copy keeps libmpv in Contents/Frameworks, a development build beside itself.
        let dir = exe.as_deref().and_then(|e| e.parent()).map(|d| {
            let frameworks = d.join("../Frameworks");
            if frameworks.join("libmpv.dylib").exists() { frameworks } else { d.to_path_buf() }
        });
        dir.unwrap_or_default().to_string_lossy().into_owned()
    });
    let mute = std::env::var("FPUI_MUTE").is_ok_and(|v| v == "1");
    let files: Vec<String> = std::env::args()
        .skip(1)
        .filter_map(|a| std::fs::canonicalize(&a).ok().or_else(|| a.contains("://").then(|| PathBuf::from(&a))))
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let core = Arc::new(mpv::Core::new(lib_dir, mute, route));
    let module = MpvModule { core: core.clone(), pending: Mutex::new(files) };
    let hidden = std::env::var("FPUI_HIDDEN").is_ok_and(|v| v == "1");
    let builder = skathi::Builder::new()
        .visible(!hidden)
        .activate(!hidden)
        .payload_file(commands::payload_file())
        .data_dir(commands::data_dir())
        .module("mpv", module)
        .module("cast", crate::cast_module::CastModule);
    // The child route: the main window's backdrop pinned by the embedder, the display heard from
    // Skathi, mpv's layer under <sk-layer name="video">.
    let child_route = route == mpv::Route::Child;
    let builder = if child_route {
        builder.backdrop(std::env::var("FPUI_BACKDROP").unwrap_or_else(|_| "transparent".into()))
    } else {
        builder
    };
    let builder = if child_route { builder.on_display_change(child::display_changed) } else { builder };
    // The window's frame source (skathi#49). FPUI_FRAME_SOURCE=auto|link|next; unset = Skathi's
    // default (Auto).
    let builder = match std::env::var("FPUI_FRAME_SOURCE").ok().as_deref() {
        Some("auto") => builder.metal_frame_source(skathi::MetalFrameSource::Auto),
        Some("link") => builder.metal_frame_source(skathi::MetalFrameSource::DisplayLink),
        Some("next") => builder.metal_frame_source(skathi::MetalFrameSource::NextDrawable),
        _ => builder,
    };
    let builder = builder.setup({
        let core = core.clone();
        move |host| {
            sys::dark_appearance();
            if child_route {
                host.layer("video", || Box::new(child::VideoLayer));
            }
            let _ = core.ui.set(host.handle());
            // The green button and AppKit's menu item take the page's way in (fullscreen.rs).
            let ui2 = core.clone();
            fullscreen::install(Box::new(move || {
                if let Some(ui) = ui2.ui.get() {
                    let _ = ui.emit_to("mpv", "fullscreen", &serde_json::json!({}));
                }
            }));
            // A live resize by the pointer, as version 1's `frameplayer://window-resizing`.
            let ui = core.clone();
            live_resize::watch(Arc::new(move |on| {
                if let Some(ui) = ui.ui.get() {
                    let _ = ui.emit_to("mpv", "resizing", &serde_json::json!({ "on": on }));
                }
            }));
            // Quitting stops the picture and the sound before Skathi's quit runs (quit.rs).
            let stop = core.clone();
            quit::install(Box::new(move || {
                child::hide();
                stop.halt();
            }));
            let _ = core.surface.set(host.surface("video"));
            let _ = core.overlay.set(host.surface("overlay"));
            Ok(())
        }
    });
    let done = core.clone();
    if let Err(e) = builder.run_on_main_thread(move |exit| {
        done.destroy();
        i32::from(!exit.clean)
    }) {
        eprintln!("{e}");
        velo::log(&format!("start failed: {e}"));
        std::process::exit(2);
    }
}
