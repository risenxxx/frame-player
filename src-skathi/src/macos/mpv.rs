//! libmpv behind the `native:mpv` module, in the shape `tauri-plugin-libmpv` gives the interface:
//! `init` (options + observed properties), `command`, `set_property`, `get_property`, and every
//! mpv event as one JSON object (`{ event: "property-change", name, data, id }`, …) on the
//! module's `event` event, which the JS bridge re-emits as Tauri's `mpv-event-main`.
//!
//! The picture, by route (`FPUI_ROUTE`; `child` unless it says otherwise):
//! - `gl`: mpv's OpenGL render API on a CGL context of our own, into a ring of 3 IOSurfaces the
//!   size of `<sk-surface name="video">` (`video-sync=audio`).
//! - `split`: our libmpv's Vulkan render API (the ring in `vk.rs`), a ring of
//!   2. The picture pass draws the element's whole layout scaled down to about the video's own
//!   pixel count (never above the element's), shown stretched over the element; subtitles and
//!   mpv's OSD go to an element-sized overlay ring of 2, un-premultiplied on the GPU and shown in
//!   `<sk-surface name="overlay">` above the video. Both images come from one mpv layout, so
//!   zoom, pan, crop and rotation are one transform.
//! - `element` / `video`: the same Vulkan ring, one pass, no overlay, at the element's size / at
//!   the scaled size (the references for the sharpness comparison).
//! - `child` (default): the same render API into a CAMetalLayer of our own under the page, placed
//!   by Skathi's `<sk-layer name="video">` (child.rs). No ring and no overlay pass.
//!
//! Threads: the caller's (options, commands, properties), a render thread that owns the GL context
//! and calls only `mpv_render_*` (render.h forbids the rest there), mpv's event loop, and a control
//! thread that keeps `target-peak` on the element's headroom.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use skathi::{SharedTexture, SurfaceError, SurfaceFormat, SurfaceHandle};

use crate::sys::{IoSurface, SharedEvent};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;

const RING: usize = 3;
/// The Vulkan routes' ring (`FPUI_RING` overrides).
const VK_RING: usize = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    Gl,
    Split,
    Element,
    Video,
    /// A CAMetalLayer of our own under Skathi's view (child.rs).
    Child,
}

impl Route {
    pub fn from_env() -> Self {
        match std::env::var("FPUI_ROUTE").as_deref() {
            Ok("gl") => Route::Gl,
            Ok("element") => Route::Element,
            Ok("video") => Route::Video,
            Ok("split") => Route::Split,
            _ => Route::Child,
        }
    }
}

/// The `target-trc` the page last asked for (SDR files), and whether the file playing is HDR.
static PAGE_TRC: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
static HDR_NOW: AtomicBool = AtomicBool::new(false);


/// The video's display size after rotation (control thread → render thread), 0 when unknown.
#[derive(Default)]
struct VideoGeo {
    w: std::sync::atomic::AtomicU32,
    h: std::sync::atomic::AtomicU32,
    /// The file is HDR (PQ or HLG), so the picture stays half float.
    hdr: AtomicBool,
}

/// The picture target for an element of `elem` pixels: the element's aspect, scaled so the video
/// (contain-fitted, unzoomed) gets about its own pixel count; never above the element.
fn picture_size(elem: (u32, u32), video: (u32, u32)) -> (u32, u32) {
    if video.0 == 0 || video.1 == 0 || elem.0 == 0 || elem.1 == 0 {
        return elem;
    }
    let (ew, eh) = (elem.0 as f64, elem.1 as f64);
    let (vw, vh) = (video.0 as f64, video.1 as f64);
    let fit_w = ew.min(eh * vw / vh);
    let k = (vw / fit_w).min(1.0);
    if k >= 0.999 {
        return elem;
    }
    (((ew * k).round() as u32).max(1), ((eh * k).round() as u32).max(1))
}

// ---- libmpv ------------------------------------------------------------------------------------

#[repr(C)]
struct MpvEvent {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *mut c_void,
}

#[repr(C)]
struct MpvEventProperty {
    name: *const c_char,
    format: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct MpvEventEndFile {
    reason: c_int,
    error: c_int,
    playlist_entry_id: i64,
    playlist_insert_id: i64,
    playlist_insert_num_entries: c_int,
}

#[repr(C)]
struct MpvEventStartFile {
    playlist_entry_id: i64,
}

#[repr(C)]
struct MpvEventClientMessage {
    num_args: c_int,
    args: *const *const c_char,
}

#[repr(C)]
struct MpvEventHook {
    name: *const c_char,
    id: u64,
}

#[repr(C)]
struct RenderParam {
    kind: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct GlInitParams {
    get_proc_address: extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
struct GlFbo {
    fbo: c_int,
    w: c_int,
    h: c_int,
    internal_format: c_int,
}

const PARAM_END: c_int = 0;
const PARAM_API_TYPE: c_int = 1;
const PARAM_OPENGL_INIT_PARAMS: c_int = 2;
const PARAM_OPENGL_FBO: c_int = 3;
const PARAM_FLIP_Y: c_int = 4;
const PARAM_ADVANCED_CONTROL: c_int = 10;
const PARAM_BLOCK_FOR_TARGET_TIME: c_int = 12;
const PARAM_SKIP_RENDERING: c_int = 13;
const UPDATE_FRAME: u64 = 1;

const FORMAT_NONE: c_int = 0;
const FORMAT_STRING: c_int = 1;
const FORMAT_FLAG: c_int = 3;
const FORMAT_INT64: c_int = 4;
const FORMAT_DOUBLE: c_int = 5;

type Handle = *mut c_void;

struct Api {
    _lib: libloading::Library,
    create: unsafe extern "C" fn() -> Handle,
    initialize: unsafe extern "C" fn(Handle) -> c_int,
    terminate_destroy: unsafe extern "C" fn(Handle),
    set_option_string: unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> c_int,
    set_property_string: unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> c_int,
    get_property: unsafe extern "C" fn(Handle, *const c_char, c_int, *mut c_void) -> c_int,
    observe_property: unsafe extern "C" fn(Handle, u64, *const c_char, c_int) -> c_int,
    free: unsafe extern "C" fn(*mut c_void),
    command: unsafe extern "C" fn(Handle, *const *const c_char) -> c_int,
    wait_event: unsafe extern "C" fn(Handle, f64) -> *mut MpvEvent,
    wakeup: unsafe extern "C" fn(Handle),
    error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    render_create: unsafe extern "C" fn(*mut Handle, Handle, *mut RenderParam) -> c_int,
    render_set_update_callback: unsafe extern "C" fn(Handle, Option<extern "C" fn(*mut c_void)>, *mut c_void),
    render_update: unsafe extern "C" fn(Handle) -> u64,
    render_render: unsafe extern "C" fn(Handle, *mut RenderParam) -> c_int,
    render_free: unsafe extern "C" fn(Handle),
    render_set_parameter: unsafe extern "C" fn(Handle, RenderParam) -> c_int,
}

unsafe impl Send for Api {}
unsafe impl Sync for Api {}

impl Api {
    fn load(dir: &str) -> Result<Self, String> {
        unsafe {
            let lib = libloading::Library::new(format!("{dir}/libmpv.dylib")).map_err(|e| format!("loading libmpv from {dir}: {e}"))?;
            macro_rules! sym {
                ($name:literal) => {
                    *lib.get(concat!($name, "\0").as_bytes()).map_err(|e| format!("{}: {e}", $name))?
                };
            }
            Ok(Self {
                create: sym!("mpv_create"),
                initialize: sym!("mpv_initialize"),
                terminate_destroy: sym!("mpv_terminate_destroy"),
                set_option_string: sym!("mpv_set_option_string"),
                set_property_string: sym!("mpv_set_property_string"),
                get_property: sym!("mpv_get_property"),
                observe_property: sym!("mpv_observe_property"),
                free: sym!("mpv_free"),
                command: sym!("mpv_command"),
                wait_event: sym!("mpv_wait_event"),
                wakeup: sym!("mpv_wakeup"),
                error_string: sym!("mpv_error_string"),
                render_create: sym!("mpv_render_context_create"),
                render_set_update_callback: sym!("mpv_render_context_set_update_callback"),
                render_update: sym!("mpv_render_context_update"),
                render_render: sym!("mpv_render_context_render"),
                render_free: sym!("mpv_render_context_free"),
                render_set_parameter: sym!("mpv_render_context_set_parameter"),
                _lib: lib,
            })
        }
    }

    fn err(&self, code: c_int) -> String {
        let text = unsafe { CStr::from_ptr((self.error_string)(code)) };
        text.to_string_lossy().into_owned()
    }
}

#[derive(Clone, Copy)]
struct Raw(Handle);
unsafe impl Send for Raw {}
unsafe impl Sync for Raw {}

fn cstr(s: &str) -> Result<CString, String> {
    CString::new(s).map_err(|_| format!("{s:?} contains a NUL"))
}

/// A JSON value as mpv's text: `true` is `yes`, a number its shortest form, a string itself.
fn text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::Bool(b) => Some(if *b { "yes" } else { "no" }.into()),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

fn format_code(name: &str) -> c_int {
    match name {
        "flag" => FORMAT_FLAG,
        "int64" => FORMAT_INT64,
        "double" => FORMAT_DOUBLE,
        // "node" is read as text: the interface never asks for it (the wrapper it was written
        // against crashed on it), and its text form is what a reader of it would need anyway.
        _ => FORMAT_STRING,
    }
}

/// Reads a value of `format` mpv wrote at `data`.
unsafe fn read_value(api: &Api, format: c_int, data: *mut c_void) -> Value {
    if data.is_null() {
        return Value::Null;
    }
    match format {
        FORMAT_FLAG => Value::Bool(*(data as *const c_int) != 0),
        FORMAT_INT64 => json!(*(data as *const i64)),
        FORMAT_DOUBLE => {
            let v = *(data as *const f64);
            if v.is_finite() {
                json!(v)
            } else {
                Value::Null
            }
        }
        FORMAT_STRING => {
            let p = *(data as *const *const c_char);
            if p.is_null() {
                Value::Null
            } else {
                let _ = api;
                Value::String(CStr::from_ptr(p).to_string_lossy().into_owned())
            }
        }
        _ => Value::Null,
    }
}

/// One mpv event as the plugin's JSON, or `None` for the ones it does not forward.
unsafe fn event_json(api: &Api, e: &MpvEvent) -> Option<Value> {
    let name = match e.event_id {
        1 => "shutdown",
        3 => "get-property-reply",
        4 => "set-property-reply",
        5 => "command-reply",
        6 => "start-file",
        7 => "end-file",
        8 => "file-loaded",
        11 => "idle",
        14 => "tick",
        16 => "client-message",
        17 => "video-reconfig",
        18 => "audio-reconfig",
        20 => "seek",
        21 => "playback-restart",
        22 => "property-change",
        24 => "queue-overflow",
        25 => "hook",
        _ => return None,
    };
    let mut out = json!({ "event": name });
    let o = out.as_object_mut().unwrap();
    match e.event_id {
        22 if !e.data.is_null() => {
            let p = &*(e.data as *const MpvEventProperty);
            o.insert("name".into(), Value::String(CStr::from_ptr(p.name).to_string_lossy().into_owned()));
            let data = if p.format == FORMAT_NONE { Value::Null } else { read_value(api, p.format, p.data) };
            o.insert("data".into(), data);
            o.insert("id".into(), json!(e.reply_userdata));
        }
        7 if !e.data.is_null() => {
            let f = &*(e.data as *const MpvEventEndFile);
            let reason = match f.reason {
                0 => "eof",
                2 => "stop",
                3 => "quit",
                4 => "error",
                5 => "redirect",
                _ => "unknown",
            };
            o.insert("reason".into(), json!(reason));
            o.insert("error".into(), json!(f.error));
            o.insert("playlist_entry_id".into(), json!(f.playlist_entry_id));
            o.insert("playlist_insert_id".into(), json!(f.playlist_insert_id));
            o.insert("playlist_insert_num_entries".into(), json!(f.playlist_insert_num_entries));
        }
        6 if !e.data.is_null() => {
            let f = &*(e.data as *const MpvEventStartFile);
            o.insert("playlist_entry_id".into(), json!(f.playlist_entry_id));
        }
        16 if !e.data.is_null() => {
            let m = &*(e.data as *const MpvEventClientMessage);
            let args: Vec<String> = (0..m.num_args.max(0) as usize)
                .map(|i| CStr::from_ptr(*m.args.add(i)).to_string_lossy().into_owned())
                .collect();
            o.insert("args".into(), json!(args));
        }
        25 if !e.data.is_null() => {
            let h = &*(e.data as *const MpvEventHook);
            o.insert("hook_id".into(), json!(h.id));
        }
        3..=5 => {
            o.insert("error".into(), json!(e.error));
            o.insert("id".into(), json!(e.reply_userdata));
        }
        _ => {}
    }
    Some(out)
}

// ---- CGL / OpenGL ------------------------------------------------------------------------------

#[link(name = "OpenGL", kind = "framework")]
extern "C" {
    fn CGLChoosePixelFormat(attribs: *const c_int, pix: *mut *mut c_void, npix: *mut c_int) -> c_int;
    fn CGLCreateContext(pix: *mut c_void, share: *mut c_void, ctx: *mut *mut c_void) -> c_int;
    fn CGLSetCurrentContext(ctx: *mut c_void) -> c_int;
    fn CGLDestroyPixelFormat(pix: *mut c_void) -> c_int;
    fn CGLDestroyContext(ctx: *mut c_void) -> c_int;
    #[allow(clippy::too_many_arguments)]
    fn CGLTexImageIOSurface2D(
        ctx: *mut c_void,
        target: u32,
        internal_format: u32,
        width: c_int,
        height: c_int,
        format: u32,
        kind: u32,
        surface: *mut c_void,
        plane: u32,
    ) -> c_int;
    fn glGenTextures(n: c_int, textures: *mut u32);
    fn glDeleteTextures(n: c_int, textures: *const u32);
    fn glBindTexture(target: u32, texture: u32);
    fn glGenFramebuffers(n: c_int, framebuffers: *mut u32);
    fn glDeleteFramebuffers(n: c_int, framebuffers: *const u32);
    fn glBindFramebuffer(target: u32, framebuffer: u32);
    fn glFramebufferTexture2D(target: u32, attachment: u32, textarget: u32, texture: u32, level: c_int);
    fn glCheckFramebufferStatus(target: u32) -> u32;
    fn glFlush();
    fn glFenceSync(condition: u32, flags: u32) -> *mut c_void;
    fn glClientWaitSync(sync: *mut c_void, flags: u32, timeout: u64) -> u32;
    fn glDeleteSync(sync: *mut c_void);
}

const GL_TEXTURE_RECTANGLE: u32 = 0x84F5;
const GL_RGBA16F: u32 = 0x881A;
const GL_RGBA: u32 = 0x1908;
const GL_HALF_FLOAT: u32 = 0x140B;
const GL_FRAMEBUFFER: u32 = 0x8D40;
const GL_COLOR_ATTACHMENT0: u32 = 0x8CE0;
const GL_FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
const GL_SYNC_GPU_COMMANDS_COMPLETE: u32 = 0x9117;
const GL_SYNC_FLUSH_COMMANDS_BIT: u32 = 1;

extern "C" fn gl_proc(_: *mut c_void, name: *const c_char) -> *mut c_void {
    // Every GL entry point is exported by OpenGL.framework itself.
    unsafe { libc::dlsym(libc::RTLD_DEFAULT, name) }
}

struct Slot {
    _surface: IoSurface,
    texture: u32,
    fbo: u32,
    event: SharedEvent,
    shared: SharedTexture,
    next: u64,
    accepted_release: u64,
}

unsafe fn make_slot(cgl: *mut c_void, w: u32, h: u32) -> Result<Slot, String> {
    let surface = IoSurface::rgba_half(w, h);
    let mut texture = 0;
    glGenTextures(1, &mut texture);
    glBindTexture(GL_TEXTURE_RECTANGLE, texture);
    let e = CGLTexImageIOSurface2D(cgl, GL_TEXTURE_RECTANGLE, GL_RGBA16F, w as c_int, h as c_int, GL_RGBA, GL_HALF_FLOAT, surface.0, 0);
    glBindTexture(GL_TEXTURE_RECTANGLE, 0);
    if e != 0 {
        glDeleteTextures(1, &texture);
        return Err(format!("CGLTexImageIOSurface2D: CGLError {e}"));
    }
    let mut fbo = 0;
    glGenFramebuffers(1, &mut fbo);
    glBindFramebuffer(GL_FRAMEBUFFER, fbo);
    glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_RECTANGLE, texture, 0);
    let status = glCheckFramebufferStatus(GL_FRAMEBUFFER);
    glBindFramebuffer(GL_FRAMEBUFFER, 0);
    if status != GL_FRAMEBUFFER_COMPLETE {
        return Err(format!("framebuffer incomplete: {status:#x}"));
    }
    let event = SharedEvent::new();
    let shared = SharedTexture::from_iosurface(surface.0)
        .and_then(|t| t.with_shared_event(event.0.cast()))
        .map_err(|e| format!("opening the IOSurface in Skathi: {e}"))?;
    Ok(Slot { _surface: surface, texture, fbo, event, shared, next: 0, accepted_release: 0 })
}

unsafe fn drop_slot(slot: Slot) {
    slot.event.wait(slot.accepted_release, Duration::from_secs(1));
    glDeleteFramebuffers(1, &slot.fbo);
    glDeleteTextures(1, &slot.texture);
}

struct Wake {
    pending: Mutex<bool>,
    cond: Condvar,
}

extern "C" fn on_update(ctx: *mut c_void) {
    let wake = unsafe { &*(ctx as *const Wake) };
    *wake.pending.lock().unwrap() = true;
    wake.cond.notify_one();
}

/// 0 = starting, 1 = ready, 2 = failed.
struct RenderState {
    phase: AtomicU64,
    error: Mutex<String>,
}

fn render_thread(api: Arc<Api>, mpv: Raw, surface: SurfaceHandle, stop: Arc<AtomicBool>, state: Arc<RenderState>) {
    let fail = |e: String| {
        eprintln!("[fpui mpv] {e}");
        *state.error.lock().unwrap() = e;
        state.phase.store(2, Ordering::Release);
    };
    unsafe {
        // kCGLPFAOpenGLProfile 99 = 3.2 core 0x3200, kCGLPFAAccelerated 73, kCGLPFAColorSize 8 = 24,
        // kCGLPFAAllowOfflineRenderers 96.
        let attribs: [c_int; 7] = [99, 0x3200, 73, 8, 24, 96, 0];
        let (mut pix, mut npix, mut cgl) = (std::ptr::null_mut(), 0, std::ptr::null_mut());
        let e = CGLChoosePixelFormat(attribs.as_ptr(), &mut pix, &mut npix);
        if e != 0 || pix.is_null() {
            return fail(format!("CGLChoosePixelFormat: {e}"));
        }
        let e = CGLCreateContext(pix, std::ptr::null_mut(), &mut cgl);
        CGLDestroyPixelFormat(pix);
        if e != 0 {
            return fail(format!("CGLCreateContext: {e}"));
        }
        CGLSetCurrentContext(cgl);

        let api_type = CString::new("opengl").unwrap();
        let mut init = GlInitParams { get_proc_address: gl_proc, get_proc_address_ctx: std::ptr::null_mut() };
        let mut advanced: c_int = 1;
        let mut params = [
            RenderParam { kind: PARAM_API_TYPE, data: api_type.as_ptr() as *mut c_void },
            RenderParam { kind: PARAM_OPENGL_INIT_PARAMS, data: (&mut init as *mut GlInitParams).cast() },
            RenderParam { kind: PARAM_ADVANCED_CONTROL, data: (&mut advanced as *mut c_int).cast() },
            RenderParam { kind: PARAM_END, data: std::ptr::null_mut() },
        ];
        let mut ctx: Handle = std::ptr::null_mut();
        let e = (api.render_create)(&mut ctx, mpv.0, params.as_mut_ptr());
        if e != 0 {
            CGLSetCurrentContext(std::ptr::null_mut());
            CGLDestroyContext(cgl);
            return fail(format!("mpv_render_context_create: {}", api.err(e)));
        }
        let wake = Box::leak(Box::new(Wake { pending: Mutex::new(false), cond: Condvar::new() }));
        (api.render_set_update_callback)(ctx, Some(on_update), (wake as *const Wake) as *mut c_void);
        surface.set_format(SurfaceFormat::Rgba16Float);
        state.phase.store(1, Ordering::Release);

        let mut ring: Vec<Slot> = Vec::new();
        let mut size = (0u32, 0u32);
        let mut index = 0usize;
        while !stop.load(Ordering::Relaxed) {
            {
                let mut pending = wake.pending.lock().unwrap();
                if !*pending {
                    pending = wake.cond.wait_timeout(pending, Duration::from_millis(50)).unwrap().0;
                }
                if !*pending {
                    continue;
                }
                *pending = false;
            }
            let flags = (api.render_update)(ctx);
            if flags & UPDATE_FRAME == 0 {
                continue;
            }
            let info = surface.info();
            if !info.visible || info.width == 0 || info.height == 0 {
                // Nothing can show it: mpv still does its bookkeeping for the frame.
                let mut skip: c_int = 1;
                let mut params = [
                    RenderParam { kind: PARAM_SKIP_RENDERING, data: (&mut skip as *mut c_int).cast() },
                    RenderParam { kind: PARAM_END, data: std::ptr::null_mut() },
                ];
                (api.render_render)(ctx, params.as_mut_ptr());
                continue;
            }
            if size != (info.width, info.height) || ring.is_empty() {
                for slot in ring.drain(..) {
                    drop_slot(slot);
                }
                size = (info.width, info.height);
                match (0..RING).map(|_| make_slot(cgl, size.0, size.1)).collect::<Result<Vec<_>, _>>() {
                    Ok(slots) => {
                        ring = slots;
                        index = 0;
                    }
                    Err(e) => {
                        eprintln!("[fpui mpv] ring {}x{}: {e}; retrying", size.0, size.1);
                        size = (0, 0);
                        *wake.pending.lock().unwrap() = true;
                        std::thread::sleep(Duration::from_millis(20));
                        continue;
                    }
                }
            }
            let slot = &mut ring[index];
            // Wait on the CPU, and only for the last release Skathi accepted for this slot.
            if slot.accepted_release != 0 && !slot.event.wait(slot.accepted_release, Duration::from_secs(1)) {
                eprintln!("[fpui mpv] slot {index} still in use after 1 s");
                continue;
            }
            let mut fbo = GlFbo { fbo: slot.fbo as c_int, w: size.0 as c_int, h: size.1 as c_int, internal_format: GL_RGBA16F as c_int };
            // FLIP_Y 0: the IOSurface's first row is the picture's top row, which Metal reads.
            let mut flip_y: c_int = 0;
            let mut block: c_int = 0;
            let mut params = [
                RenderParam { kind: PARAM_OPENGL_FBO, data: (&mut fbo as *mut GlFbo).cast() },
                RenderParam { kind: PARAM_FLIP_Y, data: (&mut flip_y as *mut c_int).cast() },
                RenderParam { kind: PARAM_BLOCK_FOR_TARGET_TIME, data: (&mut block as *mut c_int).cast() },
                RenderParam { kind: PARAM_END, data: std::ptr::null_mut() },
            ];
            let e = (api.render_render)(ctx, params.as_mut_ptr());
            if e != 0 {
                eprintln!("[fpui mpv] render: {}", api.err(e));
            }
            // The IOSurface must hold the frame before Metal reads it: flush, wait on the CPU for
            // the GL commands, then signal the surface's event from the CPU (even after a failure).
            glFlush();
            let fence = glFenceSync(GL_SYNC_GPU_COMMANDS_COMPLETE, 0);
            glClientWaitSync(fence, GL_SYNC_FLUSH_COMMANDS_BIT, 1_000_000_000);
            glDeleteSync(fence);
            let (wait, release) = (2 * slot.next + 1, 2 * slot.next + 2);
            slot.next += 1;
            slot.event.signal(wait);
            match surface.present_shared_fenced(&slot.shared, wait, release) {
                Ok(_) => {
                    slot.accepted_release = release;
                    index = (index + 1) % RING;
                }
                // Skipped; the slot stays ours for the next frame.
                Err(SurfaceError::Busy) => {}
                Err(e) => {
                    eprintln!("[fpui mpv] present: {e}");
                    if matches!(e, SurfaceError::DeviceChanged) {
                        size = (0, 0);
                    }
                }
            }
        }
        for slot in ring.drain(..) {
            drop_slot(slot);
        }
        (api.render_set_update_callback)(ctx, None, std::ptr::null_mut());
        (api.render_free)(ctx);
        CGLSetCurrentContext(std::ptr::null_mut());
        CGLDestroyContext(cgl);
    }
}

// ---- the Vulkan routes ---------------------------------------------------------------------------

/// The surface-format and overlay switches (defaults: half-float picture and overlay).
mod exp {
    fn on(name: &str, value: &str) -> bool {
        std::env::var(name).is_ok_and(|v| v == value)
    }
    /// FPUI_OVL_RING=n: the overlay ring's own size.
    pub fn ovl_ring() -> Option<usize> {
        std::env::var("FPUI_OVL_RING").ok().and_then(|v| v.parse().ok()).map(|n: usize| n.max(1))
    }
    /// FPUI_OVL_DROP=n: after n frames without an overlay, the overlay ring is replaced by a
    /// fresh one (IOSurfaces nobody wrote are not resident).
    pub fn ovl_drop() -> Option<u32> {
        std::env::var("FPUI_OVL_DROP").ok().and_then(|v| v.parse().ok())
    }
    /// FPUI_OVL_BAND=1: the overlay surface shows a bottom band only (the page sizes its element);
    /// mpv still renders the element-sized overlay, the Metal pass copies the band out of it.
    pub fn band() -> bool {
        on("FPUI_OVL_BAND", "1") || out8()
    }
    /// FPUI_OVL_OUT=bgra8: mpv's overlay stays half float; the Metal pass writes an 8-bit,
    /// sRGB-encoded copy (the band path, element-sized unless the page made the element a band),
    /// and the overlay surface is Bgra8.
    pub fn out8() -> bool {
        on("FPUI_OVL_OUT", "bgra8")
    }
    /// FPUI_OVL_BAND=auto: the overlay element is a bottom band whose height follows
    /// what mpv drew (a GPU scan of each drawn overlay); the page sets it from the `band` event.
    pub fn band_auto() -> bool {
        on("FPUI_OVL_BAND", "auto")
    }
    /// FPUI_STATS=1: present counts, render times and Skathi's frame feedback for the
    /// picture, logged every 5 s.
    pub fn stats() -> bool {
        on("FPUI_STATS", "1")
    }
}

/// The picture's frame feedback (FPUI_STATS).
#[derive(Default)]
struct Cadence {
    shown: u64,
    dropped: [u64; 4],
    last_at: u64,
    /// Distances between consecutive shown frames, in refreshes (index 0 = 1 refresh … 7 = 8+).
    gaps: [u64; 8],
}

use crate::vk::{self, Slot as VkSlot, Unpremul, VkEnv};

fn make_ring(env: &VkEnv, n: usize, size: (u32, u32), eight: bool) -> Result<Vec<VkSlot>, String> {
    let mut made = Vec::new();
    for _ in 0..n {
        match env.slot_fmt(size.0, size.1, true, eight) {
            Ok(slot) => made.push(slot),
            Err(e) => {
                // All or nothing: slots made before a refusal are freed, not leaked.
                for slot in made {
                    env.destroy(slot);
                }
                return Err(e);
            }
        }
    }
    Ok(made)
}

#[allow(clippy::too_many_arguments)]
fn render_thread_vk(
    api: Arc<Api>,
    mpv: Raw,
    surface: SurfaceHandle,
    overlay_surface: Option<SurfaceHandle>,
    stop: Arc<AtomicBool>,
    state: Arc<RenderState>,
    route: Route,
    geo: Arc<VideoGeo>,
    ui: Option<skathi::Handle>,
) {
    let fail = |e: String| {
        eprintln!("[fpui mpv] {e}");
        *state.error.lock().unwrap() = e;
        state.phase.store(2, Ordering::Release);
    };
    let ring_n: usize = std::env::var("FPUI_RING").ok().and_then(|v| v.parse().ok()).unwrap_or(VK_RING).max(1);
    let split = route == Route::Split && overlay_surface.is_some();
    let (band_on, out8) = (exp::band() || exp::band_auto(), exp::out8());
    let (band_auto, stats_on) = (exp::band_auto(), exp::stats());
    // The picture and the overlay follow the file (see `Core::start`'s control thread): 8-bit for
    // SDR, half float for HDR. A file starts as SDR until the control thread has read its gamma.
    let mut pic8 = !geo.hdr.load(Ordering::Relaxed);
    let mut ovl8 = pic8;
    let ovl_n = exp::ovl_ring().unwrap_or(ring_n);
    let drop_after = exp::ovl_drop();
    eprintln!("[fpui mpv] pic8 {pic8} ovl8 {ovl8} out8 {out8} ovl ring {ovl_n} drop {drop_after:?} band {band_on} band-auto {band_auto} stats {stats_on}");
    // The band: the fraction of the element's height the page is asked to give the
    // overlay element (bottom-aligned), and the frames since a smaller band would have done.
    let emit_band = |f: f64| {
        if let Some(ui) = &ui {
            let _ = ui.emit_to("mpv", "band", &json!({ "f": f }));
        }
    };
    const BAND_MIN: f64 = 0.25;
    const BAND_STEP: u32 = 64;
    const BAND_SHRINK_FRAMES: u32 = 90;
    let mut band_req = if band_auto { BAND_MIN } else { 1.0 };
    let (mut band_small_frames, mut band_small_max) = (0u32, 0.0f64);
    let mut band_seen_scans = 0u64;
    if band_auto {
        emit_band(band_req);
    }
    // FPUI_STATS
    let cadence = Arc::new(Mutex::new(Cadence::default()));
    let _watch = stats_on.then(|| {
        let c = cadence.clone();
        surface.on_frame(move |e| {
            let mut c = c.lock().unwrap();
            match e {
                skathi::FrameEvent::Presented { shown_at, refresh_interval, .. } => {
                    if c.last_at != 0 && !refresh_interval.is_zero() {
                        let n = ((shown_at.saturating_sub(c.last_at)) as f64 / refresh_interval.as_nanos() as f64).round() as usize;
                        c.gaps[n.clamp(1, 8) - 1] += 1;
                    }
                    c.last_at = shown_at;
                    c.shown += 1;
                }
                skathi::FrameEvent::Dropped { reason, .. } => {
                    let i = match reason {
                        skathi::FrameDropReason::Superseded => 0,
                        skathi::FrameDropReason::Hidden => 1,
                        skathi::FrameDropReason::DeviceLost => 2,
                        #[allow(unreachable_patterns)]
                        _ => 3,
                    };
                    c.dropped[i] += 1;
                }
            }
        })
    });
    let (mut st_rendered, mut st_ok, mut st_busy, mut st_err, mut st_ovl) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let (mut st_rt_sum, mut st_rt_max) = (Duration::ZERO, Duration::ZERO);
    let mut st_last = Instant::now();
    let t_start = Instant::now();
    unsafe {
        let mut env = match VkEnv::new() {
            Ok(env) => env,
            Err(e) => return fail(format!("Vulkan: {e}")),
        };
        eprintln!("[fpui mpv] Vulkan {} via {}; route {route:?}, ring {ring_n}", env.description, vk::library());
        let mut unpremul = if split {
            match Unpremul::new(!pic8) {
                Ok(u) => Some(u),
                Err(e) => return fail(e),
            }
        } else {
            None
        };
        let api_type = CString::new("vulkan").unwrap();
        let mut advanced: c_int = 1;
        let mut params = [
            RenderParam { kind: PARAM_API_TYPE, data: api_type.as_ptr() as *mut c_void },
            RenderParam { kind: vk::PARAM_VULKAN_INIT_PARAMS, data: env.init_params() },
            RenderParam { kind: PARAM_ADVANCED_CONTROL, data: (&mut advanced as *mut c_int).cast() },
            RenderParam { kind: PARAM_END, data: std::ptr::null_mut() },
        ];
        let mut ctx: Handle = std::ptr::null_mut();
        let e = (api.render_create)(&mut ctx, mpv.0, params.as_mut_ptr());
        if e != 0 {
            return fail(format!("mpv_render_context_create (vulkan): {}", api.err(e)));
        }
        let wake = Box::leak(Box::new(Wake { pending: Mutex::new(false), cond: Condvar::new() }));
        (api.render_set_update_callback)(ctx, Some(on_update), (wake as *const Wake) as *mut c_void);
        // mpv asks for a render only when it has a new frame, so a paused picture
        // was never drawn again when the element changed size (a fullscreen entered while paused
        // kept the old picture, stretched, and mpv's OSD size). A size change now wakes this
        // thread and forces one render of the current frame at the new size (mpv's render API
        // redraws its last frame when there is no new one). FPUI_RESIZE_REDRAW=0 turns it off.
        let resized = Arc::new(AtomicBool::new(false));
        let _size_watch = (std::env::var("FPUI_RESIZE_REDRAW").map_or(true, |v| v != "0")).then(|| {
            let resized = resized.clone();
            let wake_addr = wake as *const Wake as usize;
            let last = Mutex::new((0u32, 0u32));
            surface.on_change(move |_, e| {
                if let skathi::SurfaceEvent::Changed(i) = e {
                    let mut l = last.lock().unwrap();
                    if (i.width, i.height) != *l {
                        *l = (i.width, i.height);
                        resized.store(true, Ordering::Release);
                        let w = &*(wake_addr as *const Wake);
                        *w.pending.lock().unwrap() = true;
                        w.cond.notify_one();
                    }
                }
            })
        });
        surface.set_format(if pic8 { SurfaceFormat::Bgra8 } else { SurfaceFormat::Rgba16Float });
        if let Some(o) = &overlay_surface {
            o.set_format(if ovl8 || out8 { SurfaceFormat::Bgra8 } else { SurfaceFormat::Rgba16Float });
        }
        state.phase.store(1, Ordering::Release);

        let retire = |rings: &mut [&mut Vec<VkSlot>]| {
            if rings.iter().all(|r| r.is_empty()) {
                return;
            }
            // mpv first (its GPU work and release signals), then each slot once Skathi's copy of
            // its last accepted frame is done.
            (api.render_set_parameter)(ctx, RenderParam { kind: vk::PARAM_VULKAN_RETIRE, data: std::ptr::null_mut() });
            for r in rings.iter_mut() {
                for slot in r.drain(..) {
                    env.destroy(slot);
                }
            }
        };
        let mut pic: Vec<VkSlot> = Vec::new();
        let mut ovl: Vec<VkSlot> = Vec::new();
        let (mut pic_size, mut ovl_size) = ((0u32, 0u32), (0u32, 0u32));
        let (mut pi, mut oi) = (0usize, 0usize);
        let mut overlay_shown = false;
        let mut rebuild = false;
        let mut bands: Vec<vk::Band> = Vec::new();
        let mut band_size = (0u32, 0u32);
        let (mut idle_frames, mut ovl_dirty) = (0u32, false);
        while !stop.load(Ordering::Relaxed) {
            {
                let mut pending = wake.pending.lock().unwrap();
                if !*pending {
                    pending = wake.cond.wait_timeout(pending, Duration::from_millis(50)).unwrap().0;
                }
                if !*pending {
                    continue;
                }
                *pending = false;
            }
            let flags = (api.render_update)(ctx);
            let forced = resized.swap(false, Ordering::AcqRel);
            if flags & UPDATE_FRAME == 0 && !forced {
                continue;
            }
            if forced && flags & UPDATE_FRAME == 0 {
                eprintln!("[fpui mpv] element resized, redrawing the current frame ({} ms)", t_start.elapsed().as_millis());
            }
            let info = surface.info();
            if !info.visible || info.width == 0 || info.height == 0 {
                let mut skip: c_int = 1;
                let mut params = [
                    RenderParam { kind: PARAM_SKIP_RENDERING, data: (&mut skip as *mut c_int).cast() },
                    RenderParam { kind: PARAM_END, data: std::ptr::null_mut() },
                ];
                (api.render_render)(ctx, params.as_mut_ptr());
                continue;
            }
            // The surfaces' format follows the file (the control thread has already
            // moved mpv's target-trc and target-peak).
            let want8 = !geo.hdr.load(Ordering::Relaxed);
            if want8 != pic8 {
                pic8 = want8;
                ovl8 = want8;
                rebuild = true;
                if let Some(u) = unpremul.as_mut() {
                    u.set_linear(!pic8);
                }
                eprintln!("[fpui mpv] picture and overlay now {}", if pic8 { "8-bit (SDR)" } else { "half float (HDR)" });
            }
            if stats_on && st_last.elapsed() >= Duration::from_secs(5) {
                let c = cadence.lock().unwrap();
                let n = st_rendered.max(1);
                eprintln!(
                    "[fpui stats] rendered {st_rendered} presented {st_ok} busy {st_busy} err {st_err} overlay {st_ovl} render avg {:.2} ms max {:.2} ms | shown {} dropped sup/hid/lost/other {:?} gaps(1..8+) {:?}",
                    st_rt_sum.as_secs_f64() * 1000.0 / n as f64, st_rt_max.as_secs_f64() * 1000.0, c.shown, c.dropped, c.gaps
                );
                st_last = Instant::now();
            }
            let elem = (info.width, info.height);
            let video = (geo.w.load(Ordering::Relaxed), geo.h.load(Ordering::Relaxed));
            let want_pic = if route == Route::Element { elem } else { picture_size(elem, video) };
            let want_ovl = if split { elem } else { (0, 0) };
            let want_band = match (&overlay_surface, band_on && split) {
                (Some(o), true) => {
                    let i = o.info();
                    if i.width == elem.0 && i.height > 0 && i.height <= elem.1 { (i.width, i.height) } else { elem }
                }
                _ => (0, 0),
            };
            // A new band height alone remakes the bands, not the rings. The old bands go
            // once Skathi's copies of them are done (each slot's value is Skathi's release).
            if band_auto && !rebuild && !pic.is_empty() && want_pic == pic_size && want_ovl == ovl_size && want_band != band_size && !ovl.is_empty() {
                for slot in &ovl {
                    if slot.used {
                        env.wait(slot, slot.value, Duration::from_millis(250));
                    }
                }
                if overlay_shown {
                    if let Some(o) = &overlay_surface {
                        o.clear();
                    }
                    overlay_shown = false;
                }
                bands.clear();
                for slot in ovl.iter().filter(|_| want_band.1 < want_ovl.1) {
                    match vk::Band::new(slot, want_band.0, want_band.1, ovl8 || out8) {
                        Ok(b) => bands.push(b),
                        Err(e) => eprintln!("[fpui mpv] band: {e}"),
                    }
                }
                band_size = want_band;
                eprintln!("[fpui mpv] band {}x{} (offset {}), bands only, at overlay frame {st_ovl}, {} ms", want_band.0, want_band.1, want_ovl.1 - want_band.1, t_start.elapsed().as_millis());
            }
            if rebuild || pic.is_empty() || want_pic != pic_size || want_ovl != ovl_size || want_band != band_size {
                surface.set_format(if pic8 { SurfaceFormat::Bgra8 } else { SurfaceFormat::Rgba16Float });
                if let Some(o) = &overlay_surface {
                    o.set_format(if ovl8 || out8 { SurfaceFormat::Bgra8 } else { SurfaceFormat::Rgba16Float });
                }
                bands.clear();
                band_size = (0, 0);
                retire(&mut [&mut pic, &mut ovl]);
                if overlay_shown {
                    if let Some(o) = &overlay_surface {
                        o.clear();
                    }
                    overlay_shown = false;
                }
                let made = make_ring(&env, ring_n, want_pic, pic8).and_then(|p| {
                    if split {
                        match make_ring(&env, ovl_n, want_ovl, ovl8) {
                            Ok(o) => Ok((p, o)),
                            Err(e) => {
                                for slot in p {
                                    env.destroy(slot);
                                }
                                Err(e)
                            }
                        }
                    } else {
                        Ok((p, Vec::new()))
                    }
                });
                match made {
                    Ok((p, o)) => {
                        if band_on && split {
                            // A band of the element's full height is the element itself.
                            for slot in o.iter().filter(|_| !band_auto || want_band.1 < want_ovl.1) {
                                match vk::Band::new(slot, want_band.0, want_band.1, ovl8 || out8) {
                                    Ok(b) => bands.push(b),
                                    Err(e) => eprintln!("[fpui mpv] band: {e}"),
                                }
                            }
                            band_size = want_band;
                            eprintln!("[fpui mpv] band {}x{} (offset {})", want_band.0, want_band.1, want_ovl.1 - want_band.1);
                        }
                        ovl_dirty = false;
                        idle_frames = 0;
                        pic = p;
                        ovl = o;
                        pic_size = want_pic;
                        ovl_size = want_ovl;
                        pi = 0;
                        oi = 0;
                        rebuild = false;
                        eprintln!(
                            "[fpui mpv] ring: picture {}x{}{} x{ring_n} (element {}x{}, video {}x{})",
                            pic_size.0, pic_size.1,
                            if split { format!(", overlay {}x{}", ovl_size.0, ovl_size.1) } else { String::new() },
                            elem.0, elem.1, video.0, video.1
                        );
                    }
                    Err(e) => {
                        eprintln!("[fpui mpv] ring {}x{}: {e}; retrying", want_pic.0, want_pic.1);
                        pic_size = (0, 0);
                        *wake.pending.lock().unwrap() = true;
                        std::thread::sleep(Duration::from_millis(20));
                        continue;
                    }
                }
            }
            let mut fbo = pic[pi].fbo();
            let mut ov = split.then(|| vk::OverlayFbo {
                struct_size: std::mem::size_of::<vk::OverlayFbo>(),
                target: ovl[oi].fbo(),
                rendered: 0,
                video_rect: [0; 4],
                source_rect: [0; 4],
                picture_layout: vk::PICTURE_ELEMENT,
            });
            let mut block: c_int = 0;
            let mut params = vec![
                RenderParam { kind: vk::PARAM_VULKAN_FBO, data: (&mut fbo as *mut vk::Fbo).cast() },
                RenderParam { kind: PARAM_BLOCK_FOR_TARGET_TIME, data: (&mut block as *mut c_int).cast() },
            ];
            if let Some(ov) = ov.as_mut() {
                params.push(RenderParam { kind: vk::PARAM_VULKAN_OVERLAY_FBO, data: (ov as *mut vk::OverlayFbo).cast() });
            }
            params.push(RenderParam { kind: PARAM_END, data: std::ptr::null_mut() });
            let t0 = Instant::now();
            let e = (api.render_render)(ctx, params.as_mut_ptr());
            if stats_on {
                let dt = t0.elapsed();
                st_rendered += 1;
                st_rt_sum += dt;
                st_rt_max = st_rt_max.max(dt);
            }
            if e != 0 {
                eprintln!("[fpui mpv] render: {}", api.err(e));
                // GENERIC: an image was acquired and must be retired before reuse (render_vk.h).
                if e == -20 {
                    rebuild = true;
                    *wake.pending.lock().unwrap() = true;
                }
                continue;
            }
            // The picture: Skathi's copy waits for mpv's ready value and signals the next.
            {
                let slot = &mut pic[pi];
                slot.used = true;
                let (wait, release) = (fbo.release_value, fbo.release_value + 1);
                match surface.present_shared_fenced(slot.shared.as_ref().unwrap(), wait, release) {
                    Ok(_) => {
                        slot.value = release;
                        pi = (pi + 1) % pic.len();
                        st_ok += 1;
                    }
                    Err(e) => {
                        // Refused before anything was submitted: the next acquire is mpv's ready.
                        slot.value = wait;
                        if matches!(e, SurfaceError::Busy) {
                            st_busy += 1
                        } else {
                            st_err += 1
                        }
                        if !matches!(e, SurfaceError::Busy) {
                            eprintln!("[fpui mpv] present: {e}");
                        }
                        if matches!(e, SurfaceError::DeviceChanged) {
                            rebuild = true;
                        }
                    }
                }
            }
            // The overlay: mpv's ready → un-premultiply → Skathi's copy, all on the GPU.
            if let (Some(ov), Some(osurf), Some(un)) = (ov.as_ref(), overlay_surface.as_ref(), unpremul.as_ref()) {
                if ov.rendered == 1 {
                    idle_frames = 0;
                    ovl_dirty = true;
                    let banded = bands.len() == ovl.len();
                    let slot = &mut ovl[oi];
                    slot.used = true;
                    let ready = ov.target.release_value;
                    let ok = if banded {
                        un.run_band(slot, &mut bands[oi], ovl_size.1 - band_size.1, ready, ready + 1, band_auto)
                    } else {
                        un.run(slot, ready, ready + 1, band_auto)
                    };
                    st_ovl += 1;
                    // The band follows the drawn rows of the last completed scan. It
                    // grows at once and shrinks after BAND_SHRINK_FRAMES frames that fit a smaller one.
                    if band_auto {
                        let (bx, scans) = un.last_box();
                        if scans != band_seen_scans {
                            band_seen_scans = scans;
                            if let Some((top, _bottom)) = bx {
                                let h = ovl_size.1.max(1);
                                let need = (h.saturating_sub(top) + 16).div_ceil(BAND_STEP) * BAND_STEP;
                                let f = (need as f64 / h as f64).clamp(BAND_MIN, 1.0);
                                let f = (f * 1000.0).ceil() / 1000.0;
                                // Most of the element: the whole element, without a band copy.
                                let f = if f > 0.75 { 1.0 } else { f };
                                if f > band_req + 1e-9 {
                                    band_req = f;
                                    band_small_frames = 0;
                                    eprintln!("[fpui mpv] band grow to {f:.3} (top row {top} of {h}) at overlay frame {st_ovl}, {} ms", t_start.elapsed().as_millis());
                                    emit_band(f);
                                } else if f < band_req - 1e-9 {
                                    band_small_frames += 1;
                                    band_small_max = if band_small_frames == 1 { f } else { band_small_max.max(f) };
                                    if band_small_frames >= BAND_SHRINK_FRAMES {
                                        band_req = band_small_max;
                                        band_small_frames = 0;
                                        eprintln!("[fpui mpv] band shrink to {band_req:.3}");
                                        emit_band(band_req);
                                    }
                                } else {
                                    band_small_frames = 0;
                                }
                            }
                        }
                    }
                    if !ok {
                        eprintln!("[fpui mpv] overlay: the Metal pass was not committed");
                        slot.value = ready;
                        osurf.clear();
                        overlay_shown = false;
                    } else {
                        let shared = if banded { &bands[oi].shared } else { slot.shared.as_ref().unwrap() };
                        match osurf.present_shared_fenced(shared, ready + 1, ready + 2) {
                            Ok(_) => {
                                slot.value = ready + 2;
                                oi = (oi + 1) % ovl.len();
                                overlay_shown = true;
                            }
                            Err(e) => {
                                slot.value = ready + 1;
                                if !matches!(e, SurfaceError::Busy) {
                                    eprintln!("[fpui mpv] overlay present: {e}");
                                }
                            }
                        }
                    }
                } else {
                    if overlay_shown {
                        osurf.clear();
                        overlay_shown = false;
                    }
                    idle_frames = idle_frames.saturating_add(1);
                    // A fresh overlay ring once nothing was drawn for a while.
                    if let Some(n) = drop_after {
                        if ovl_dirty && idle_frames >= n {
                            bands.clear();
                            band_size = (0, 0);
                            retire(&mut [&mut ovl]);
                            match make_ring(&env, ovl_n, ovl_size, ovl8) {
                                Ok(o) => {
                                    ovl = o;
                                    oi = 0;
                                    ovl_dirty = false;
                                    eprintln!("[fpui mpv] overlay ring replaced after {idle_frames} idle frames");
                                }
                                Err(e) => {
                                    eprintln!("[fpui mpv] overlay ring: {e}");
                                    rebuild = true;
                                }
                            }
                        }
                    }
                }
            }
        }
        bands.clear();
        retire(&mut [&mut pic, &mut ovl]);
        (api.render_set_update_callback)(ctx, None, std::ptr::null_mut());
        (api.render_free)(ctx);
        drop(unpremul);
        drop(env);
    }
}


/// mpv renders straight into the child layer's drawables (or, `FPUI_CHILD_COPY=1`,
/// into one slot of our own that a Metal blit copies into the drawable). No Skathi surface, no
/// ring beyond the layer's own drawables.
fn render_thread_child(api: Arc<Api>, mpv: Raw, stop: Arc<AtomicBool>, state: Arc<RenderState>, geo: Arc<VideoGeo>) {
    use crate::child;
    let fail = |e: String| {
        eprintln!("[fpui mpv] {e}");
        *state.error.lock().unwrap() = e;
        state.phase.store(2, Ordering::Release);
    };
    let mut copy = std::env::var("FPUI_CHILD_COPY").is_ok_and(|v| v == "1");
    // A safety net, not a route: direct import failed for every drawable while the layer's drawables
    // were framebuffer-only (MoltenVK refuses their compressed IOSurfaces; child.rs now turns that
    // off), and fails briefly after a format change. After this many failures in a row
    // the route copies into the drawables, which costs a full-size slot of its own. Every failure is
    // logged with the surface's format, the switch says how many failed, and the stats line counts
    // them, so the net can never hide a regression of the direct route.
    const IMPORT_FAILS_TO_COPY: u32 = 10;
    let mut import_fails: u32 = 0;
    let mut st_import_fails: u64 = 0;
    let stats_on = exp::stats();
    unsafe {
        // Copy mode blits into the drawable; FPUI_CHILD_FBO=0 also asks for uncompressed drawables
        // CoreAnimation lets MoltenVK import (framebufferOnly drawables are '&BGA', lossless-compressed).
        let mut env = match VkEnv::new() {
            Ok(env) => env,
            Err(e) => return fail(format!("Vulkan: {e}")),
        };
        eprintln!("[fpui mpv] child layer, {} ({})", env.description, if copy { "copy" } else { "direct" });
        let api_type = CString::new("vulkan").unwrap();
        let mut advanced: c_int = 1;
        let mut params = [
            RenderParam { kind: PARAM_API_TYPE, data: api_type.as_ptr() as *mut c_void },
            RenderParam { kind: vk::PARAM_VULKAN_INIT_PARAMS, data: env.init_params() },
            RenderParam { kind: PARAM_ADVANCED_CONTROL, data: (&mut advanced as *mut c_int).cast() },
            RenderParam { kind: PARAM_END, data: std::ptr::null_mut() },
        ];
        let mut ctx: Handle = std::ptr::null_mut();
        let e = (api.render_create)(&mut ctx, mpv.0, params.as_mut_ptr());
        if e != 0 {
            return fail(format!("mpv_render_context_create (vulkan): {}", api.err(e)));
        }
        let wake = Box::leak(Box::new(Wake { pending: Mutex::new(false), cond: Condvar::new() }));
        (api.render_set_update_callback)(ctx, Some(on_update), (wake as *const Wake) as *mut c_void);
        state.phase.store(1, Ordering::Release);
        // The layer is made by the first attach of <sk-layer name="video">.
        let waited = Instant::now();
        let c = loop {
            if let Some(c) = child::get() {
                break c;
            }
            if stop.load(Ordering::Relaxed) || waited.elapsed() > Duration::from_secs(20) {
                return fail("no <sk-layer name=\"video\"> attached".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        eprintln!("[fpui mpv] layer there after {:.0} ms", waited.elapsed().as_secs_f64() * 1000.0);
        static KICKED: AtomicBool = AtomicBool::new(false);
        let wake_addr = wake as *const Wake as usize;
        let _ = child::KICK.set(Box::new(move || {
            KICKED.store(true, Ordering::Release);
            let w = &*(wake_addr as *const Wake);
            *w.pending.lock().unwrap() = true;
            w.cond.notify_one();
        }));
        let mut hdr = geo.hdr.load(Ordering::Relaxed);
        // The layer's format for what is known now, on the main thread (layer changes made on this
        // thread do not land).
        {
            let h = hdr;
            let _ = crate::sys::on_main(Duration::from_millis(500), move || {
                if let Some(c) = child::get() {
                    child::configure(&c.layer, h);
                }
            });
        }
        // direct: one slot per drawable IOSurface; copy: one slot and its Metal texture.
        // Each entry is (IOSurface, its imported slot, the frame it was last drawn
        // into). slot_ext CFRetains the IOSurface and MoltenVK's image holds it too, so a surface
        // Core Animation stops handing out would stay resident as long as its slot lived, i.e. until
        // the next resize. A slot not
        // handed out by nextDrawable for FPUI_CHILD_EVICT frames (default 48, 0 = never) is retired
        // in mpv and released. Both halves are needed (measured): the layer's maximumDrawableCount
        // (child.rs) sets the pool, since a surface still in Core Animation's pool stays resident
        // after this release (maximum 3: 2 slots, 3 resident); and at maximum 2 Core Animation
        // still handed out 3-4 distinct surfaces in the first frames of 6 of 13 runs, which only
        // this release gave back (2 resident in all 13).
        let mut slots: Vec<(usize, VkSlot, u64)> = Vec::new();
        let evict_after: u64 = std::env::var("FPUI_CHILD_EVICT").ok().and_then(|v| v.parse().ok()).unwrap_or(48);
        let mut frame_no: u64 = 0;
        let mut st_evicted = 0u64;
        let mut own: Option<(VkSlot, Retained<ProtocolObject<dyn objc2_metal::MTLTexture>>)> = None;
        let mut size = (0u32, 0u32);
        let (mut st_rendered, mut st_presented, mut st_nodrawable, mut st_err) = (0u64, 0u64, 0u64, 0u64);
        let (mut st_rt_sum, mut st_rt_max) = (Duration::ZERO, Duration::ZERO);
        let mut st_last = Instant::now();
        let retire_all = |env: &VkEnv, slots: &mut Vec<(usize, VkSlot, u64)>, own: &mut Option<(VkSlot, Retained<ProtocolObject<dyn objc2_metal::MTLTexture>>)>| {
            if slots.is_empty() && own.is_none() {
                return;
            }
            (api.render_set_parameter)(ctx, RenderParam { kind: vk::PARAM_VULKAN_RETIRE, data: std::ptr::null_mut() });
            for (_, slot, _) in slots.drain(..) {
                env.destroy(slot);
            }
            if let Some((slot, tex)) = own.take() {
                drop(tex);
                env.destroy(slot);
            }
        };
        while !stop.load(Ordering::Relaxed) {
            {
                let mut pending = wake.pending.lock().unwrap();
                if !*pending {
                    pending = wake.cond.wait_timeout(pending, Duration::from_millis(50)).unwrap().0;
                }
                if !*pending {
                    continue;
                }
                *pending = false;
            }
            let flags = (api.render_update)(ctx);
            let kicked = KICKED.swap(false, Ordering::AcqRel);
            if flags & UPDATE_FRAME == 0 && !kicked {
                continue;
            }
            let skip = || {
                let mut skip: c_int = 1;
                let mut params = [
                    RenderParam { kind: PARAM_SKIP_RENDERING, data: (&mut skip as *mut c_int).cast() },
                    RenderParam { kind: PARAM_END, data: std::ptr::null_mut() },
                ];
                (api.render_render)(ctx, params.as_mut_ptr());
            };
            let want = child::wanted_size(&c.layer);
            if want.0 == 0 || want.1 == 0 {
                skip();
                continue;
            }
            let now_hdr = geo.hdr.load(Ordering::Relaxed);
            if now_hdr != hdr || want != size {
                retire_all(&env, &mut slots, &mut own);
                if now_hdr != hdr {
                    hdr = now_hdr;
                    // On the main thread: a layer change made here sits in this thread's implicit
                    // CATransaction, which nothing commits (measured: EDR never took effect).
                    let h = hdr;
                    let _ = crate::sys::on_main(Duration::from_millis(500), move || {
                        if let Some(c) = child::get() {
                            child::configure(&c.layer, h);
                        }
                    });
                    eprintln!("[fpui mpv] layer now {}", if !hdr { "BGRA8, sRGB" } else { "BT.2100 PQ, EDR" });
                }
                child::set_drawable_size(&c.layer, want);
                size = want;
                eprintln!("[fpui mpv] drawables {}x{}", size.0, size.1);
            }
            if stats_on && st_last.elapsed() >= Duration::from_secs(5) {
                let n = st_rendered.max(1);
                eprintln!(
                    "[fpui stats] rendered {st_rendered} presented {st_presented} no-drawable {st_nodrawable} err {st_err} import-failed {st_import_fails} render avg {:.2} ms max {:.2} ms, slots {} evicted {st_evicted}{}",
                    st_rt_sum.as_secs_f64() * 1000.0 / n as f64, st_rt_max.as_secs_f64() * 1000.0, slots.len() + own.is_some() as usize, if copy { ", copy" } else { "" }
                );
                st_last = Instant::now();
            }
            let drawable = objc2::rc::autoreleasepool(|_| c.layer.nextDrawable());
            let Some(drawable) = drawable else {
                st_nodrawable += 1;
                skip();
                continue;
            };
            let eight = !hdr;
            let slot: &mut VkSlot = if copy {
                if own.is_none() {
                    match env.slot_fmt(size.0, size.1, false, eight) {
                        Ok(s) => match child::texture_on(c, s.surface.0, size.0, size.1, hdr) {
                            Some(t) => own = Some((s, t)),
                            None => {
                                env.destroy(s);
                                st_err += 1;
                                skip();
                                continue;
                            }
                        },
                        Err(e) => {
                            eprintln!("[fpui mpv] slot: {e}");
                            st_err += 1;
                            skip();
                            continue;
                        }
                    }
                }
                &mut own.as_mut().unwrap().0
            } else {
                let ios = child::drawable_iosurface(&drawable);
                if ios.is_null() {
                    return fail("a drawable without an IOSurface".into());
                }
                frame_no += 1;
                let i = match slots.iter().position(|(k, _, _)| *k == ios as usize) {
                    Some(i) => i,
                    None => match env.slot_ext(ios, size.0, size.1, eight) {
                        Ok(s) => {
                            import_fails = 0;
                            slots.push((ios as usize, s, frame_no));
                            eprintln!("[fpui mpv] new drawable surface {:#x}, slots {} (frame {frame_no})", ios as usize, slots.len());
                            if slots.len() > 8 {
                                eprintln!("[fpui mpv] {} drawable surfaces seen", slots.len());
                            }
                            slots.len() - 1
                        }
                        Err(e) => {
                            st_err += 1;
                            st_import_fails += 1;
                            import_fails += 1;
                            eprintln!(
                                "[fpui mpv] drawable import failed ({import_fails} in a row, {st_import_fails} in all): {e}; surface {:#x} '{}' {}x{}",
                                ios as usize, child::iosurface_format(ios), size.0, size.1
                            );
                            if import_fails >= IMPORT_FAILS_TO_COPY {
                                eprintln!("[fpui mpv] WARNING: {import_fails} drawable imports failed in a row ({st_import_fails} in all); FALLING BACK to copying into the drawables, the direct route is broken here");
                                copy = true;
                            }
                            skip();
                            continue;
                        }
                    },
                };
                slots[i].2 = frame_no;
                &mut slots[i].1
            };
            let mut fbo = slot.fbo();
            let mut block: c_int = 0;
            let mut params = [
                RenderParam { kind: vk::PARAM_VULKAN_FBO, data: (&mut fbo as *mut vk::Fbo).cast() },
                RenderParam { kind: PARAM_BLOCK_FOR_TARGET_TIME, data: (&mut block as *mut c_int).cast() },
                RenderParam { kind: PARAM_END, data: std::ptr::null_mut() },
            ];
            let t0 = Instant::now();
            let e = (api.render_render)(ctx, params.as_mut_ptr());
            let dt = t0.elapsed();
            st_rendered += 1;
            st_rt_sum += dt;
            st_rt_max = st_rt_max.max(dt);
            if e != 0 {
                eprintln!("[fpui mpv] render: {}", api.err(e));
                st_err += 1;
                if e == -20 {
                    retire_all(&env, &mut slots, &mut own);
                    size = (0, 0);
                    *wake.pending.lock().unwrap() = true;
                }
                continue;
            }
            slot.used = true;
            let ok = if copy {
                let tex = &own.as_ref().unwrap().1;
                let ev = own.as_ref().unwrap().0.event();
                let r = child::copy_present(c, &drawable, tex, ev, fbo.release_value, fbo.release_value + 1);
                own.as_mut().unwrap().0.value = if r { fbo.release_value + 1 } else { fbo.release_value };
                r
            } else {
                slot.value = fbo.release_value;
                child::present_after(c, &drawable, slot.event(), fbo.release_value)
            };
            if ok {
                st_presented += 1;
            } else {
                st_err += 1;
            }
            // Release surfaces Core Animation no longer hands out.
            if evict_after > 0 && !copy {
                while let Some(k) = slots.iter().position(|(_, _, last)| frame_no.saturating_sub(*last) > evict_after) {
                    let (ios, slot, last) = slots.swap_remove(k);
                    let mut image = slot.image;
                    // Waits for mpv's GPU work on that image and forgets mpv's wrapper of it.
                    (api.render_set_parameter)(ctx, RenderParam { kind: vk::PARAM_VULKAN_RETIRE, data: (&mut image as *mut ash::vk::Image).cast() });
                    env.destroy(slot);
                    st_evicted += 1;
                    eprintln!(
                        "[fpui mpv] released drawable surface {ios:#x}, unused for {} frames; slots {}",
                        frame_no - last, slots.len()
                    );
                }
            }
        }
        retire_all(&env, &mut slots, &mut own);
        (api.render_set_update_callback)(ctx, None, std::ptr::null_mut());
        (api.render_free)(ctx);
        drop(env);
    }
}

// ---- the module ----------------------------------------------------------------------------------

struct Live {
    api: Arc<Api>,
    handle: Raw,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    render: Option<JoinHandle<()>>,
}

pub struct Core {
    pub lib_dir: String,
    pub mute: bool,
    pub route: Route,
    pub ui: OnceLock<skathi::Handle>,
    pub surface: OnceLock<SurfaceHandle>,
    pub overlay: OnceLock<SurfaceHandle>,
    live: Mutex<Option<Live>>,
    /// `halt`'s pause, joined before the handle goes.
    halting: Mutex<Option<JoinHandle<()>>>,
}

impl Core {
    pub fn new(lib_dir: String, mute: bool, route: Route) -> Self {
        Self { lib_dir, mute, route, ui: OnceLock::new(), surface: OnceLock::new(), overlay: OnceLock::new(), live: Mutex::new(None), halting: Mutex::new(None) }
    }

    fn handle(&self) -> Result<(Arc<Api>, Raw), String> {
        self.live.lock().unwrap().as_ref().map(|l| (l.api.clone(), l.handle)).ok_or_else(|| "mpv is not initialized".to_string())
    }

    pub fn init(&self, options: Vec<(String, Value)>, observed: Vec<(String, String)>) -> Result<(), String> {
        let mut live = self.live.lock().unwrap();
        if live.is_some() {
            return Err("mpv is already initialized".into());
        }
        let surface = self.surface.get().cloned().ok_or("no video surface")?;
        let api = Arc::new(Api::load(&self.lib_dir)?);
        let handle = Raw(unsafe { (api.create)() });
        if handle.0.is_null() {
            return Err("mpv_create failed".into());
        }
        let destroy = |api: &Api| unsafe { (api.terminate_destroy)(handle.0) };

        // The interface's options, then what this host needs whatever they say: mpv draws through
        // the render API (no window of its own), into a linear-light extended sRGB surface whose
        // 1.0 is SDR white at 203 nits.
        let mut all: Vec<(String, String)> = options
            .iter()
            .filter(|(k, _)| !matches!(k.as_str(), "wid" | "vo" | "gpu-api" | "gpu-context"))
            .filter_map(|(k, v)| text(v).map(|v| (k.clone(), v)))
            .collect();
        // The page's own `target-trc` (version 1's SDR colour setting), given back after an HDR file.
        if let Some((_, v)) = all.iter().rev().find(|(k, _)| k == "target-trc") {
            *PAGE_TRC.lock().unwrap() = v.clone();
        }
        for (k, v) in [
            ("vo", "libmpv"),
            ("target-prim", "bt.709"),
            ("hdr-compute-peak", "no"),
            ("target-peak", "203"),
            ("video-sync", "audio"),
        ] {
            all.push((k.into(), v.into()));
        }
        if self.mute {
            for (k, v) in [("ao", "null"), ("mute", "yes")] {
                all.push((k.into(), v.into()));
            }
        }
        // FPUI_MPV_EXTRA ("k=v;k=v") goes last.
        if let Ok(extra) = std::env::var("FPUI_MPV_EXTRA") {
            for kv in extra.split(';').filter(|kv| !kv.is_empty()) {
                if let Some((k, v)) = kv.split_once('=') {
                    all.push((k.into(), v.into()));
                }
            }
        }
        for (k, v) in &all {
            let (kc, vc) = (cstr(k)?, cstr(v)?);
            let e = unsafe { (api.set_option_string)(handle.0, kc.as_ptr(), vc.as_ptr()) };
            if e < 0 {
                destroy(&api);
                return Err(format!("Error setting option '{k}' to '{v}': {}", api.err(e)));
            }
        }
        let e = unsafe { (api.initialize)(handle.0) };
        if e < 0 {
            destroy(&api);
            return Err(format!("mpv_initialize: {}", api.err(e)));
        }
        for (i, (name, format)) in observed.iter().enumerate() {
            let n = cstr(name)?;
            let e = unsafe { (api.observe_property)(handle.0, i as u64 + 1, n.as_ptr(), format_code(format)) };
            if e < 0 {
                eprintln!("[fpui mpv] observe {name}: {}", api.err(e));
            }
        }

        let stop = Arc::new(AtomicBool::new(false));
        let state = Arc::new(RenderState { phase: AtomicU64::new(0), error: Mutex::new(String::new()) });
        let geo = Arc::new(VideoGeo::default());
        let render = {
            let (api, surface, stop, state) = (api.clone(), surface.clone(), stop.clone(), state.clone());
            let (route, overlay, geo, ui) = (self.route, self.overlay.get().cloned(), geo.clone(), self.ui.get().cloned());
            std::thread::Builder::new()
                .name("mpv-render".into())
                .spawn(move || {
                    if route == Route::Gl {
                        render_thread(api, handle, surface, stop, state)
                    } else if route == Route::Child {
                        render_thread_child(api, handle, stop, state, geo)
                    } else {
                        render_thread_vk(api, handle, surface, overlay, stop, state, route, geo, ui)
                    }
                })
                .unwrap()
        };
        // vo=libmpv needs the render context before a file opens.
        let began = Instant::now();
        while state.phase.load(Ordering::Acquire) == 0 && began.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(2));
        }
        if state.phase.load(Ordering::Acquire) != 1 {
            stop.store(true, Ordering::Relaxed);
            let _ = render.join();
            destroy(&api);
            let e = state.error.lock().unwrap().clone();
            return Err(if e.is_empty() { "the render context did not start".into() } else { e });
        }

        let events = {
            let (api, ui, stop) = (api.clone(), self.ui.get().cloned(), stop.clone());
            std::thread::Builder::new()
                .name("mpv-events".into())
                .spawn(move || loop {
                    let handle = handle;
                    let event = unsafe { &*(api.wait_event)(handle.0, 0.5) };
                    if event.event_id == 1 {
                        if let Some(ui) = &ui {
                            let _ = ui.emit_to("mpv", "event", &json!({ "event": "shutdown" }));
                        }
                        break;
                    }
                    if event.event_id == 0 {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        continue;
                    }
                    if let (Some(ui), Some(json)) = (&ui, unsafe { event_json(&api, event) }) {
                        let _ = ui.emit_to("mpv", "event", &json);
                    }
                })
                .unwrap()
        };
        // target-peak follows the element's content headroom (1.0 = SDR white = 203 nits). The
        // Vulkan routes also need the video's display size for the picture target.
        let control = {
            let (api, stop, route) = (api.clone(), stop.clone(), self.route);
            std::thread::Builder::new()
                .name("mpv-control".into())
                .spawn(move || {
                    let handle = handle;
                    let mut set = 203.0f64;
                    let mut hdr = false;
                    let string = |name: &str| -> String {
                        let n = cstr(name).unwrap();
                        let mut p: *mut c_char = std::ptr::null_mut();
                        let e = unsafe { (api.get_property)(handle.0, n.as_ptr(), FORMAT_STRING, (&mut p as *mut *mut c_char).cast()) };
                        if e < 0 || p.is_null() {
                            return String::new();
                        }
                        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
                        unsafe { (api.free)(p.cast()) };
                        s
                    };
                    let int = |name: &str| -> i64 {
                        let n = cstr(name).unwrap();
                        let mut v: i64 = 0;
                        let e = unsafe { (api.get_property)(handle.0, n.as_ptr(), FORMAT_INT64, (&mut v as *mut i64).cast()) };
                        if e < 0 { 0 } else { v }
                    };
                    while !stop.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(50));
                        if route != Route::Gl {
                            let (mut w, mut h) = (int("video-out-params/dw").max(0) as u32, int("video-out-params/dh").max(0) as u32);
                            if (int("video-out-params/rotate") + int("video-rotate")).rem_euclid(180) == 90 {
                                std::mem::swap(&mut w, &mut h);
                            }
                            geo.w.store(w, Ordering::Relaxed);
                            geo.h.store(h, Ordering::Relaxed);
                        }
                        // The file decides (PQ/HLG = HDR). mpv's target moves first,
                        // then the render thread changes the surfaces' format.
                        //
                        // SDR: 8-bit surfaces on an sRGB layer, `target-trc` the page's own (version
                        // 1's "As in mpv" / "As in macOS"), `target-peak` 203. HDR (PQ/HLG): mpv writes
                        // PQ in BT.2020 untouched into a 10-bit BT.2100 PQ layer, and macOS maps it to
                        // the headroom, as QuickTime and bare mpv's `target-colorspace-hint` do. Two
                        // alternatives were measured and dropped: every file linear into 8-bit sRGB
                        // drawables (mid-grey at 0.29 where QuickTime shows 0.55, shadows crushed, HDR
                        // without EDR), and HDR tone-mapped by mpv to linear light, 1.0 = 203 nits
                        // (black lifted by libplacebo's 1000:1 target contrast, brighter than
                        // QuickTime elsewhere).
                        let g = string("video-params/gamma");
                        if !g.is_empty() {
                            let now = g == "pq" || g == "hlg";
                            if now != hdr {
                                hdr = now;
                                HDR_NOW.store(hdr, Ordering::Relaxed);
                                let back = { let t = PAGE_TRC.lock().unwrap(); if t.is_empty() { "auto".to_string() } else { t.clone() } };
                                let trc = if !hdr { back.as_str() } else { "pq" };
                                let prim = if hdr { "bt.2020" } else { "bt.709" };
                                for (n, v) in [("target-prim", prim), ("target-trc", trc)] {
                                    let (n, v) = (cstr(n).unwrap(), cstr(v).unwrap());
                                    unsafe { (api.set_property_string)(handle.0, n.as_ptr(), v.as_ptr()) };
                                }
                                geo.hdr.store(hdr, Ordering::Relaxed);
                                eprintln!("[fpui mpv] gamma {g}: {}", if hdr { "HDR" } else { "SDR" });
                            }
                        }
                        // An 8-bit sRGB picture is SDR; its peak is SDR white.
                        let sdr8 = !hdr;
                        let room = if route == Route::Child {
                            // The headroom from Builder::on_display_change.
                            crate::child::headroom()
                        } else {
                            surface.info().content_headroom() as f64
                        };
                        // PQ: no tone mapping of ours; macOS maps to the headroom.
                        let peak = if sdr8 { 203.0 } else { 10000.0 };
                        if (peak - set).abs() > 0.005 * set {
                            // An integer option: "203.0" is refused.
                            let (n, v) = (cstr("target-peak").unwrap(), cstr(&format!("{peak:.0}")).unwrap());
                            unsafe { (api.set_property_string)(handle.0, n.as_ptr(), v.as_ptr()) };
                            set = peak;
                            eprintln!("[fpui mpv] target-peak {peak:.0} (headroom {room:.2})");
                        }
                    }
                })
                .unwrap()
        };
        *live = Some(Live { api, handle, stop, threads: vec![events, control], render: Some(render) });
        Ok(())
    }

    pub fn command(&self, name: &str, args: &[Value]) -> Result<(), String> {
        let (api, handle) = self.handle()?;
        let mut owned = vec![cstr(name)?];
        for a in args {
            if let Some(t) = text(a) {
                owned.push(cstr(&t)?);
            }
        }
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|a| a.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        let e = unsafe { (api.command)(handle.0, ptrs.as_ptr()) };
        if e < 0 {
            return Err(format!("mpv command '{name}' failed: {}", api.err(e)));
        }
        Ok(())
    }

    pub fn set_property(&self, name: &str, value: &Value) -> Result<(), String> {
        let (api, handle) = self.handle()?;
        let v = text(value).unwrap_or_default();
        // The page's SDR curve: kept for SDR files, held back while an HDR file plays (PQ or linear).
        if name == "target-trc" {
            *PAGE_TRC.lock().unwrap() = v.clone();
            if HDR_NOW.load(Ordering::Relaxed) {
                return Ok(());
            }
        }
        let (n, vc) = (cstr(name)?, cstr(&v)?);
        let e = unsafe { (api.set_property_string)(handle.0, n.as_ptr(), vc.as_ptr()) };
        if e < 0 {
            return Err(format!("Failed to set property '{name}' to '{v}': {}", api.err(e)));
        }
        Ok(())
    }

    pub fn get_property(&self, name: &str, format: &str) -> Result<Value, String> {
        let (api, handle) = self.handle()?;
        let n = cstr(name)?;
        let code = format_code(format);
        unsafe {
            let mut buf = [0u64; 1];
            let data = buf.as_mut_ptr().cast::<c_void>();
            let e = (api.get_property)(handle.0, n.as_ptr(), code, data);
            if e < 0 {
                return Err(format!("Failed to get property '{name}': {}", api.err(e)));
            }
            let value = read_value(&api, code, data);
            if code == FORMAT_STRING {
                let p = *(data as *const *mut c_char);
                if !p.is_null() {
                    (api.free)(p.cast());
                }
            }
            Ok(value)
        }
    }

    /// Quitting (quit.rs): playback stops now, the teardown follows when Skathi's quit is done.
    /// Paused, not stopped: the page still reads the position it saves for the next start.
    /// On its own thread, so a core busy with a slow read cannot hold the main thread.
    pub fn halt(&self) {
        let Ok((api, handle)) = self.handle() else { return };
        let mut halting = self.halting.lock().unwrap();
        if halting.is_some() {
            return;
        }
        *halting = Some(std::thread::spawn(move || {
            let handle = handle; // the whole `Raw` (Send), not its pointer field
            let (n, v) = (c"pause", c"yes");
            unsafe { (api.set_property_string)(handle.0, n.as_ptr(), v.as_ptr()) };
        }));
    }

    pub fn destroy(&self) {
        if let Some(h) = self.halting.lock().unwrap().take() {
            let _ = h.join();
        }
        let Some(mut live) = self.live.lock().unwrap().take() else { return };
        live.stop.store(true, Ordering::Relaxed);
        // The render context goes before the handle it was made from.
        if let Some(r) = live.render.take() {
            let _ = r.join();
        }
        unsafe { (live.api.wakeup)(live.handle.0) };
        for t in live.threads.drain(..) {
            let _ = t.join();
        }
        unsafe { (live.api.terminate_destroy)(live.handle.0) };
    }
}
