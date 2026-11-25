//! libmpv behind the `native:mpv` module, in the shape `tauri-plugin-libmpv` gives the interface:
//! `init` (options + observed properties), `command`, `set_property`, `get_property`, and every
//! mpv event as one JSON object (`{ event: "property-change", name, data, id }`, …) on the
//! module's `event` event, which the JS bridge re-emits as Tauri's `mpv-event-main`.
//!
//! The Windows picture path is a D3D11 producer:
//! mpv's render API on a D3D11 device of our own (on Skathi's adapter), into a ring of shared
//! half-float textures, each with a shared fence, handed to `<sk-surface name="video">` with
//! `present_shared_fenced`. Routes (`FPUI_ROUTE`; `sklayer` unless it says otherwise):
//! - `split`: the picture as in `video`; subtitles and mpv's OSD go to
//!   `<sk-surface name="overlay">` above it, drawn for the element's size. Needs a libmpv with
//!   `MPV_RENDER_PARAM_D3D11_OVERLAY_FBO` (`patches/mpv-d3d11-overlay.diff`, gpu-next renderer);
//!   with any other libmpv the route falls back to `video` at the first frame.
//! - `video`: a ring of 2; the element's whole layout rendered at about the video's own pixel
//!   count (never above the element's), shown stretched over the element. Subtitles and mpv's
//!   OSD are in the picture, so they are scaled with it.
//! - `element`: the same ring at the element's size (sharp subtitles, more memory).
//! - `layer`: no `<sk-surface>` frames at all. mpv renders into the back buffer of a flip swap
//!   chain of our own, in a top-level window that Skathi's window is made the owned window of, so that it
//!   lies directly under it; the page is transparent over it (`backdrop: transparent`). The swap chain is
//!   HDR10 (R10G10B10A2, ST 2084 / BT.2020) while the element's display is in HDR, with mpv's SDR white at the
//!   desktop's SDR white level, and 8-bit sRGB otherwise (`FPUI_LAYER_SDR=1` forces that); it is made anew
//!   when the display's mode changes. Skathi's window stays 8-bit. No ring, no copy, no overlay pass (the
//!   picture is the element's size, so the text in it is sharp).
//! - `sklayer` (default): as `layer`, with the swap chain a visual in Skathi's own
//!   composition underlay (`<sk-layer name="video">`, `skathi::Layer`) instead of a window of ours.
//!
//! There is no ring while there is no picture (no file, or a file without video): mpv's frames are
//! then skipped, as they are for a hidden window. `FPUI_IDLE_RING=1` keeps the ring from the start.
//!
//! The overlay: mpv lays subtitles and OSD out for the element and draws the rows the
//! overlay element covers into an 8-bit texture of ours (premultiplied, sRGB-encoded); one D3D11
//! pass divides by alpha into a shared texture (Skathi composites straight alpha), which is
//! presented right before the picture of the same render. Only while the picture is scaled: at
//! the element's own size the text stays in the picture. The element is the bottom third of the
//! video's (`FPUI_OVL_BAND=0`: all of it) and grows to all of it while something is drawn above
//! that; the host follows whatever size the element has. Nothing is drawn or presented while the
//! text does not change, and both textures are freed `FPUI_OVL_DROP` ms (2000; 0: never) after
//! the last text.
//!
//! Pacing (`FPUI_SYNC`): `resample` (default): `video-sync=
//! display-resample`, mpv's repeats skipped, `report_swap` right after the present. `audio` is
//! the macOS port's `video-sync=audio`.
//!
//! Threads: the caller's (options, commands, properties), a render thread that owns the D3D11
//! immediate context for mpv and calls only `mpv_render_*`, mpv's event loop, and a control
//! thread that keeps `target-peak` on the element's headroom and reads the video's size.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use skathi::{SharedTexture, SurfaceError, SurfaceFormat, SurfaceHandle};
use windows::core::{Interface, PCSTR, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE, HMODULE, WAIT_OBJECT_0};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Direct3D12::ID3D12Device;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::DirectComposition::{IDCompositionDevice, IDCompositionRectangleClip, IDCompositionVisual};
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::UI::WindowsAndMessaging::*;

/// The ring (`FPUI_RING` overrides): a ring of 2 loses no frame and saves a texture (measured).
const RING: usize = 2;
/// How long a texture's release may take before the frame is given up.
const SLOT_TIMEOUT_MS: u32 = 1000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    /// As `layer`, with the swap chain as a visual in Skathi's own composition underlay
    /// (`<sk-layer name="video">`, `skathi::Layer`) instead of a window of ours under Skathi's.
    SkLayer,
    Layer,
    Split,
    Video,
    Element,
}

impl Route {
    pub fn from_env() -> Self {
        match std::env::var("FPUI_ROUTE").as_deref() {
            Ok("element") => Route::Element,
            Ok("video") => Route::Video,
            Ok("layer") => Route::Layer,
            Ok("split") => Route::Split,
            _ => Route::SkLayer,
        }
    }
}

/// `FPUI_SYNC=audio`: the macOS port's sync mode. Default: display-resample.
fn resample() -> bool {
    !std::env::var("FPUI_SYNC").is_ok_and(|v| v == "audio")
}

/// The video's display size after rotation (control thread → render thread), 0 when unknown.
#[derive(Default)]
struct VideoGeo {
    w: AtomicU32,
    h: AtomicU32,
}

/// What mpv needs to place the picture in a target of any size (video/out/aspect.c, `keepaspect`): the
/// control thread reads it every 50 ms, and the `sklayer` visual uses it to map the picture of a swap chain
/// of one size onto where mpv will put it in a box of another (`SkVisual::fit`). Zero sizes: not known.
#[derive(Clone, Copy, PartialEq)]
struct PicLayout {
    /// The display size and the decoded size, both after rotation (`video-out-params/dw|dh`, `w|h`).
    dw: i32,
    dh: i32,
    w: i32,
    h: i32,
    keepaspect: bool,
    /// 0 no, 1 yes, 2 downscale-big (`video-unscaled`).
    unscaled: i32,
    recenter: bool,
    panscan: f64,
    zoom: f64,
    pan: [f64; 2],
    align: [f64; 2],
    scale: [f64; 2],
    /// left, right, top, bottom (`video-margin-ratio-*`).
    margin: [f64; 4],
}

static PIC_LAYOUT: Mutex<PicLayout> = Mutex::new(PicLayout {
    dw: 0,
    dh: 0,
    w: 0,
    h: 0,
    keepaspect: true,
    unscaled: 0,
    recenter: false,
    panscan: 0.0,
    zoom: 0.0,
    pan: [0.0; 2],
    align: [0.0; 2],
    scale: [1.0; 2],
    margin: [0.0; 4],
});

impl PicLayout {
    /// The picture's rect in a target of `ww`×`wh`, before mpv clips it to the target: x0, y0, x1, y1.
    /// mp_get_src_dst_rects with `monitor_par` 1, in its own integer steps, so that the rect is the one mpv
    /// draws to the pixel. `None` without a video or with `keepaspect=no` (the picture fills any box).
    fn rect(&self, ww: i32, wh: i32) -> Option<[f64; 4]> {
        if !self.keepaspect || self.dw <= 0 || self.dh <= 0 || ww <= 0 || wh <= 0 {
            return None;
        }
        let margin = |a: f64, b: f64, size: i32| -> (i32, i32) {
            let (m0, m1) = (((a * size as f64) as i32).clamp(0, size), ((b * size as f64) as i32).clamp(0, size));
            if m0 + m1 >= size {
                (0, (size - 1).max(0))
            } else {
                (m0, m1)
            }
        };
        let (mx, my) = (margin(self.margin[0], self.margin[1], ww), margin(self.margin[2], self.margin[3], wh));
        let (vw, vh) = (ww - mx.0 - mx.1, wh - my.0 - my.1);
        // aspect_calc_panscan
        let (dw, dh) = (self.dw, self.dh);
        let mut fwidth = vw;
        let mut fheight = (vw as f32 / dw as f32 * dh as f32) as i32;
        if fheight > vh || fheight < self.h {
            let tmpw = (vh as f32 / dh as f32 * dw as f32) as i32;
            if tmpw <= vw {
                fheight = vh;
                fwidth = tmpw;
            }
        }
        let mut area = vh - fheight;
        let (mut f_w, mut f_h) = (fwidth as f64 / fheight.max(1) as f64, 1.0);
        if area == 0 {
            area = vw - fwidth;
            f_w = 1.0;
            f_h = fheight as f64 / fwidth.max(1) as f64;
        }
        if self.unscaled != 0 {
            area = 0;
            if self.unscaled != 2 || (dw <= vw && dh <= vh) {
                fwidth = dw;
                fheight = dh;
            }
        }
        let pan_f = self.panscan as f32 as f64;
        let sw = (fwidth as f64 + area as f64 * pan_f * f_w) as i32;
        let sh = (fheight as f64 + area as f64 * pan_f * f_h) as i32;
        // src_dst_split_scaling
        let split = |dst: i32, scaled: i32, align: f64, pan: f64, scale: f64| -> (i32, i32) {
            let scaled = ((scaled as f32 * (2f32.powf(self.zoom as f32) * scale as f32)) as i64).max(1);
            let mut align = align as f32;
            if self.recenter && dst as i64 >= scaled {
                align = 0.0;
            }
            let align = (align + 1.0) / 2.0;
            let start = ((dst as i64 - scaled) as f32 * align + pan as f32 * scaled as f32) as i32;
            (start, start + scaled as i32)
        };
        let (x0, x1) = split(vw, sw, self.align[0], self.pan[0], self.scale[0]);
        let (y0, y1) = split(vh, sh, self.align[1], self.pan[1], self.scale[1]);
        Some([(x0 + mx.0) as f64, (y0 + my.0) as f64, (x1 + mx.0) as f64, (y1 + my.0) as f64])
    }
}

/// The picture target for an element of `elem` pixels: the element's aspect, scaled so the video
/// (contain-fitted, unzoomed) gets about its own pixel count; never above the element. The same
/// rule as the macOS port's, so zoom, pan, crop and rotation stay mpv's one layout.
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
    (((ew * k).round() as u32).max(2), ((eh * k).round() as u32).max(2))
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
struct D3d11Init {
    device: *mut c_void,
}

#[repr(C)]
struct D3d11Fbo {
    tex: *mut c_void,
}

/// mpv_d3d11_overlay_fbo of `patches/mpv-d3d11-overlay.diff`.
#[repr(C)]
struct D3d11OverlayFbo {
    struct_size: usize,
    tex: *mut c_void,
    layout_w: c_int,
    layout_h: c_int,
    offset_x: c_int,
    offset_y: c_int,
    force: c_int,
    /// Out: 0 nothing to draw, 1 drawn, 2 unchanged, 3 something to draw and no texture; what
    /// was put in (-1) when the libmpv does not know the parameter.
    rendered: c_int,
    /// Out: x0, y0, x1, y1 of everything drawn, in layout pixels.
    bbox: [c_int; 4],
}

/// mpv_render_frame_info.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct FrameInfo {
    flags: u64,
    target_time: i64,
}

const PARAM_END: c_int = 0;
const PARAM_API_TYPE: c_int = 1;
const PARAM_NEXT_FRAME_INFO: c_int = 11;
const PARAM_BLOCK_FOR_TARGET_TIME: c_int = 12;
const PARAM_SKIP_RENDERING: c_int = 13;
const PARAM_D3D11_INIT_PARAMS: c_int = 21;
const PARAM_D3D11_FBO: c_int = 22;
/// MPV_RENDER_PARAM_RENDERER of mpv PR #18845 (client API 2.7): "gpu-next" or "gpu". Passed only
/// with `FPUI_RENDERER`; the shipping libmpv (2.6) has the gpu renderer alone.
const PARAM_RENDERER: c_int = 23;
/// Ours, not upstream's (`patches/mpv-d3d11-overlay.diff`).
const PARAM_D3D11_OVERLAY_FBO: c_int = 0x4650;
const UPDATE_FRAME: u64 = 1;
const FRAME_REDRAW: u64 = 2;
const FRAME_REPEAT: u64 = 4;

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
    render_report_swap: unsafe extern "C" fn(Handle),
    render_get_info: unsafe extern "C" fn(Handle, RenderParam) -> c_int,
    render_free: unsafe extern "C" fn(Handle),
}

unsafe impl Send for Api {}
unsafe impl Sync for Api {}

impl Api {
    fn load(dll: &str) -> Result<Self, String> {
        unsafe {
            let lib = libloading::Library::new(dll).map_err(|e| format!("loading libmpv from {dll}: {e}"))?;
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
                render_report_swap: sym!("mpv_render_context_report_swap"),
                render_get_info: sym!("mpv_render_context_get_info"),
                render_free: sym!("mpv_render_context_free"),
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
unsafe fn read_value(format: c_int, data: *mut c_void) -> Value {
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
                Value::String(CStr::from_ptr(p).to_string_lossy().into_owned())
            }
        }
        _ => Value::Null,
    }
}

/// One mpv event as the plugin's JSON, or `None` for the ones it does not forward.
unsafe fn event_json(e: &MpvEvent) -> Option<Value> {
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
            let data = if p.format == FORMAT_NONE { Value::Null } else { read_value(p.format, p.data) };
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

// ---- the D3D11 producer --------------------------------------------------------------------------

struct Slot {
    tex: ID3D11Texture2D,
    fence: ID3D11Fence,
    shared: SharedTexture,
    /// The next unused fence value: a present takes `next` (wait) and `next + 1` (release).
    next: u64,
    /// The release value of the last present Skathi accepted; 0 before any.
    last_release: u64,
}

struct Wake {
    pending: Mutex<bool>,
    cond: Condvar,
    /// A render even when mpv has no new frame (a new ring, a resize, a refused still picture).
    force: AtomicBool,
}

impl Wake {
    fn kick(&self, force: bool) {
        if force {
            self.force.store(true, Ordering::Relaxed);
        }
        *self.pending.lock().unwrap() = true;
        self.cond.notify_one();
    }
}

extern "C" fn on_update(ctx: *mut c_void) {
    let wake = unsafe { &*(ctx as *const Wake) };
    wake.kick(false);
}

/// 0 = starting, 1 = ready, 2 = failed.
struct RenderState {
    phase: AtomicU64,
    error: Mutex<String>,
}

/// `f` until it stops answering `NoDevice` (the window has not drawn yet), for at most 15 s.
fn retry<T>(stop: &AtomicBool, what: &str, mut f: impl FnMut() -> Result<T, SurfaceError>) -> Result<T, String> {
    let t = Instant::now();
    loop {
        match f() {
            Ok(v) => return Ok(v),
            Err(SurfaceError::NoDevice(_)) if t.elapsed() < Duration::from_secs(15) && !stop.load(Ordering::Relaxed) => {
                std::thread::sleep(Duration::from_millis(2))
            }
            Err(e) => return Err(format!("{what}: {e}")),
        }
    }
}

/// A D3D11 device of our own on the adapter Skathi renders on: the shared textures are opened on
/// its D3D12 device.
unsafe fn own_device(stop: &AtomicBool, surface: &SurfaceHandle) -> Result<ID3D11Device, String> {
    let d12 = retry(stop, "d3d12_device", || surface.d3d12_device())?;
    let raw = d12.as_raw();
    let luid = ID3D12Device::from_raw_borrowed(&raw).ok_or("no D3D12 device")?.GetAdapterLuid();
    let factory: IDXGIFactory4 = CreateDXGIFactory1().map_err(|e| e.to_string())?;
    let adapter: IDXGIAdapter = factory.EnumAdapterByLuid(luid).map_err(|e| format!("EnumAdapterByLuid: {e}"))?;
    // Not D3D11_CREATE_DEVICE_SINGLETHREADED: with it d3d11va falls back to a copying decoder.
    let flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT;
    let levels = [D3D_FEATURE_LEVEL_12_1, D3D_FEATURE_LEVEL_12_0, D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
    let mut dev = None;
    D3D11CreateDevice(&adapter, D3D_DRIVER_TYPE_UNKNOWN, HMODULE::default(), flags, Some(&levels), D3D11_SDK_VERSION, Some(&mut dev), None, None)
        .map_err(|e| format!("D3D11CreateDevice: {e}"))?;
    dev.ok_or_else(|| "D3D11CreateDevice gave no device".to_string())
}

unsafe fn make_slot(stop: &AtomicBool, dev: &ID3D11Device, w: u32, h: u32, format: DXGI_FORMAT) -> Result<Slot, String> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: w,
        Height: h,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
        CPUAccessFlags: 0,
        // No keyed mutex: the fence is the synchronisation.
        MiscFlags: (D3D11_RESOURCE_MISC_SHARED.0 | D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0) as u32,
    };
    let mut tex = None;
    dev.CreateTexture2D(&desc, None, Some(&mut tex)).map_err(|e| format!("CreateTexture2D {w}x{h}: {e}"))?;
    let tex = tex.ok_or("no texture")?;
    let th = tex
        .cast::<IDXGIResource1>()
        .and_then(|r| r.CreateSharedHandle(None, (DXGI_SHARED_RESOURCE_READ | DXGI_SHARED_RESOURCE_WRITE).0, PCWSTR::null()))
        .map_err(|e| format!("CreateSharedHandle(texture): {e}"))?;
    let mut fence: Option<ID3D11Fence> = None;
    let made = dev.cast::<ID3D11Device5>().and_then(|d5| d5.CreateFence(0, D3D11_FENCE_FLAG_SHARED, &mut fence));
    let fence = match (made, fence) {
        (Ok(()), Some(f)) => f,
        (e, _) => {
            let _ = CloseHandle(th);
            return Err(format!("CreateFence: {e:?}"));
        }
    };
    let fh = match fence.CreateSharedHandle(None, GENERIC_ALL.0, PCWSTR::null()) {
        Ok(h) => h,
        Err(e) => {
            let _ = CloseHandle(th);
            return Err(format!("CreateSharedHandle(fence): {e}"));
        }
    };
    // `open` answers NoDevice until the window has drawn once.
    let opened = retry(stop, "SharedTexture::open", || SharedTexture::open(th.0 as usize as u64))
        .and_then(|s| s.with_fence(fh.0 as usize as u64).map_err(|e| format!("SharedTexture::with_fence: {e}")));
    // The handles stay ours to close; Skathi holds its own references to the objects.
    let _ = CloseHandle(th);
    let _ = CloseHandle(fh);
    Ok(Slot { tex, fence, shared: opened?, next: 1, last_release: 0 })
}

unsafe fn make_ring(stop: &AtomicBool, dev: &ID3D11Device, n: usize, size: (u32, u32)) -> Result<Vec<Slot>, String> {
    (0..n).map(|_| make_slot(stop, dev, size.0, size.1, DXGI_FORMAT_R16G16B16A16_FLOAT)).collect()
}

// ---- the layer under the window --------------------------------------------------------

/// Skathi's window, for the layer window's WM_CLOSE (a tool that closes "the main window" of the process finds
/// the layer: it is the one without an owner).
static LAYER_ABOVE: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

unsafe extern "system" fn layer_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match m {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_CLOSE => {
            let above = LAYER_ABOVE.load(Ordering::Relaxed);
            if above != 0 {
                let _ = PostMessageW(Some(HWND(above as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(h, m, w, l),
    }
}

// ---- the layer as a visual in Skathi's underlay ---------------------------------------

/// The render thread's wake, for `place` (UI thread) to ask for a turn.
static SK_WAKE: std::sync::atomic::AtomicPtr<Wake> = std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

/// What `<sk-layer name="video">` and the render thread share. The composition objects are Skathi's
/// window's (cloned in `attach`); the swap chain is ours, made by the render thread on its own device.
#[derive(Default)]
pub struct SkVisual {
    device: Option<IDCompositionDevice>,
    underlay: Option<IDCompositionVisual>,
    /// The swap chain's visual, the child of `frame`: its transform maps the chain's picture onto the box.
    visual: Option<IDCompositionVisual>,
    /// The visual in the underlay: no transform, clipped to the element's box (`clip`), so that a chain
    /// scaled past the box (its bars, or a zoomed picture) is cut where mpv would cut it.
    frame: Option<IDCompositionVisual>,
    clip: Option<IDCompositionRectangleClip>,
    added: bool,
    chain: Option<IDXGISwapChain1>,
    chain_size: (u32, u32),
    /// The element's box in device pixels of the window: x, y, width, height. `None`: not painted.
    bounds: Option<[i32; 4]>,
    /// When `place` last changed the box's size: while that is under `SETTLE` ago the swap chain keeps
    /// its size and the visual maps its picture onto the box (`fit`, `Layer::target_visual`).
    resized_at: Option<Instant>,
    pub attaches: u32,
    pub places: u32,
    pub hides: u32,
    pub no_composition: bool,
    /// FPUI_FS_ANIM: the fullscreen transition as an animation of this visual (`fs_enter`, `fs_exit`).
    fs: FsAnim,
    /// A black visual far larger than any window, behind the video in the underlay (`make_backstop`). This is
    /// the visual in the underlay, untransformed, holding the scaled black one (`backstop_black`), so that its
    /// clip (`back_clip`) is in window pixels.
    backstop: Option<IDCompositionVisual>,
    backstop_black: Option<IDCompositionVisual>,
    backstop_chain: Option<IDXGISwapChain1>,
    /// The backstop's clip while a fullscreen transition runs: the box the picture is animated in. Outside it the
    /// window (full screen through the whole transition) is transparent, as the window it is becoming is, rather
    /// than black from edge to edge around the moving picture. `None` on the holder otherwise (`back_clip_set`).
    back_clip: Option<IDCompositionRectangleClip>,
    back_clipped: bool,
    /// FPUI_FS_ANIM: which fullscreen change runs and which the page asked for last (`FsQueue`).
    pub queue: FsQueue,
}
unsafe impl Send for SkVisual {}

/// The fullscreen requests, one transition at a time. The page asks for the state it wants;
/// this decides what the stub does next, and the stub carries it out (its two frames, the window, `fs_enter`/
/// `fs_exit`) and comes back with `next` once `fs_busy` reads `false`. A request while a transition runs only
/// replaces `want`: when the running one is over, the next starts if the window is not in that state, so only the
/// last request counts and an even number of toggles cancels out. `target` is what `is_fullscreen` answers
/// meanwhile, so that the page's mirror (set from it on every resize) is never put back to the state on screen.
///
/// Before: a second request ran beside the first. An enter inside an exit's 220 ms had that exit's timer restore
/// the window afterwards (the page fullscreen, the window not); `fs_enter` over a pending way out replaced
/// `restore` with the full-screen box; `fs_exit` over a running way in restarted from the full box.
#[derive(Default)]
pub struct FsQueue {
    want: Option<bool>,
    /// `Some(true)` entering, `Some(false)` leaving, `None` idle.
    running: Option<bool>,
}

impl FsQueue {
    /// The page asks for `on`; `actual` is the window's state now. `"enter"`/`"exit"`: start that transition;
    /// `"queued"`: one runs, this is taken up when it is over; `"none"`: the window is already there.
    pub fn request(&mut self, on: bool, actual: bool) -> &'static str {
        self.want = Some(on);
        if self.running.is_some() {
            return "queued";
        }
        self.next(actual)
    }

    /// The running transition is over (or was given up before it moved anything): what comes after it.
    pub fn next(&mut self, actual: bool) -> &'static str {
        match self.want {
            Some(w) if w != actual => {
                self.running = Some(w);
                if w { "enter" } else { "exit" }
            }
            _ => {
                self.want = None;
                self.running = None;
                "none"
            }
        }
    }

    /// Whether the transition that is about to move (`on`) is still what the page wants: a toggle that came back
    /// within its first two frames leaves nothing to do.
    pub fn wanted(&self, on: bool) -> bool {
        self.want == Some(on)
    }

    /// The state the window is heading to while anything is running or waiting; `None` when idle.
    pub fn target(&self) -> Option<bool> {
        self.running.and(self.want)
    }
}

/// `FPUI_FS_ANIM=1` (the default): a fullscreen transition with no black. The swap chain keeps its size;
/// the visual's offset and scale are animated by DirectComposition from the video's old box to its new
/// one; the chain is resized once, `SETTLE` after the animation ends.
#[derive(Default)]
struct FsAnim {
    /// Entering: the video's box on screen before, and the animation's length, waiting for the first
    /// `place` with another size.
    pending: Option<([i32; 4], u32, Instant)>,
    /// The video's box on screen before entering, the target of the way out.
    restore: Option<[i32; 4]>,
    /// The window, for its client origin.
    hwnd: isize,
    /// While an animation runs, `apply` leaves the transform to it.
    until: Option<Instant>,
    /// Leaving: the window's restore is still to come (`fs_landed`), and the fullscreen window's client origin.
    landing: bool,
    /// When the way out started (`fs_busy`: a landing that never comes holds nothing for long).
    exit_at: Option<Instant>,
    full_origin: (i32, i32),
    /// Entering: the windowed client origin, to see the window's move to the screen's (`fs_landed`).
    enter_origin: (i32, i32),
    runs: u32,
}

/// How the visual shows a swap chain of one size in a box of another: scale x and y, offset x and y.
type Fit = [f32; 4];

unsafe fn client_origin(hwnd: isize) -> (i32, i32) {
    let mut p = windows::Win32::Foundation::POINT::default();
    let _ = windows::Win32::Graphics::Gdi::ClientToScreen(HWND(hwnd as *mut _), &mut p);
    (p.x, p.y)
}

impl SkVisual {
    /// How a chain of `chain` pixels is shown in the box `b` (window px): the picture mpv drew into the chain
    /// goes, with one scale for both axes, to where mpv will draw it in a chain of the box's size, so that it
    /// keeps its shape through any change of the box and ends where the resized chain's first frame puts it.
    /// The rest of the chain (its bars) is black, as is what it leaves of the box (the backstop); `frame`'s
    /// clip cuts what goes past the box. A stretch of the whole chain to the box, as before, when the layout is
    /// not known or mpv itself fills the box (`keepaspect=no`); none when the sizes are the same.
    fn fit(chain: (u32, u32), b: [f64; 4]) -> Fit {
        let (cw, ch) = (chain.0 as f64, chain.1 as f64);
        if cw <= 0.0 || ch <= 0.0 {
            return [1.0, 1.0, b[0] as f32, b[1] as f32];
        }
        let stretch = [(b[2] / cw) as f32, (b[3] / ch) as f32, b[0] as f32, b[1] as f32];
        if (b[2] - cw).abs() < 0.5 && (b[3] - ch).abs() < 0.5 {
            return stretch;
        }
        let layout = *PIC_LAYOUT.lock().unwrap();
        let (Some(p0), Some(p1)) = (layout.rect(chain.0 as i32, chain.1 as i32), layout.rect(b[2].round() as i32, b[3].round() as i32)) else { return stretch };
        let (w0, h0, w1, h1) = (p0[2] - p0[0], p0[3] - p0[1], p1[2] - p1[0], p1[3] - p1[1]);
        if w0 < 1.0 || h0 < 1.0 {
            return stretch;
        }
        // The longer side carries the scale: mpv's rects are whole pixels, and the error is smaller there.
        let s = if w0 >= h0 { w1 / w0 } else { h1 / h0 };
        let ox = b[0] + (p1[0] + p1[2]) / 2.0 - s * (p0[0] + p0[2]) / 2.0;
        let oy = b[1] + (p1[1] + p1[3]) / 2.0 - s * (p0[1] + p0[3]) / 2.0;
        [s as f32, s as f32, ox as f32, oy as f32]
    }

    /// Animate the visual from `from` to `to` (boxes in window px) over `ms`, ease-out cubic, on the
    /// compositor's clock from the next commit. The chain keeps its size until `SETTLE` after the end. The
    /// picture goes from where it is in `from` to where mpv will put it in `to` (`fit`): scale and offset move
    /// on one curve, so the picture's rect is a blend of the two and keeps its shape; the clip follows the box.
    unsafe fn animate(&mut self, from: [i32; 4], to: [i32; 4], ms: u32) -> bool {
        use windows::Win32::Graphics::DirectComposition::IDCompositionAnimation;
        let (Some(device), Some(visual), Some(clip)) = (&self.device, &self.visual, &self.clip) else { return false };
        let (cw, ch) = self.chain_size;
        if cw == 0 || ch == 0 || ms == 0 {
            return false;
        }
        let t = ms as f64 / 1000.0;
        // value(s) = a + d·(3u − 3u² + u³), u = s/T: ease-out cubic as DirectComposition's polynomial in s.
        let curve = |a: f32, b: f32| -> windows::core::Result<IDCompositionAnimation> {
            let anim = device.CreateAnimation()?;
            let d = (b - a) as f64;
            anim.AddCubic(0.0, a, (3.0 * d / t) as f32, (-3.0 * d / (t * t)) as f32, (d / (t * t * t)) as f32)?;
            anim.End(t, b)?;
            Ok(anim)
        };
        let f = |b: [i32; 4]| [b[0] as f64, b[1] as f64, b[2] as f64, b[3] as f64];
        let (a, b) = (Self::fit((cw, ch), f(from)), Self::fit((cw, ch), f(to)));
        let r = (|| -> windows::core::Result<()> {
            let scale = device.CreateScaleTransform()?;
            scale.SetScaleX(&curve(a[0], b[0])?)?;
            scale.SetScaleY(&curve(a[1], b[1])?)?;
            visual.SetTransform(&scale)?;
            visual.SetOffsetX(&curve(a[2], b[2])?)?;
            visual.SetOffsetY(&curve(a[3], b[3])?)?;
            clip.SetLeft(&curve(from[0] as f32, to[0] as f32)?)?;
            clip.SetTop(&curve(from[1] as f32, to[1] as f32)?)?;
            clip.SetRight(&curve((from[0] + from[2]) as f32, (to[0] + to[2]) as f32)?)?;
            clip.SetBottom(&curve((from[1] + from[3]) as f32, (to[1] + to[3]) as f32)?)?;
            // The black behind the picture moves with it (KNOWN-ISSUES.md, "Our own fullscreen transition"): the window is full screen for the
            // whole transition, and an unclipped backstop made all of it black around the moving picture.
            if let (Some(holder), Some(bc)) = (&self.backstop, &self.back_clip) {
                bc.SetLeft(&curve(from[0] as f32, to[0] as f32)?)?;
                bc.SetTop(&curve(from[1] as f32, to[1] as f32)?)?;
                bc.SetRight(&curve((from[0] + from[2]) as f32, (to[0] + to[2]) as f32)?)?;
                bc.SetBottom(&curve((from[1] + from[3]) as f32, (to[1] + to[3]) as f32)?)?;
                holder.SetClip(bc)?;
            }
            Ok(())
        })();
        self.back_clipped = self.backstop.is_some() && self.back_clip.is_some();
        if let Err(e) = r {
            eprintln!("[frameplayer mpv] fs-anim: {e}");
            return false;
        }
        let end = Instant::now() + Duration::from_millis(ms as u64);
        self.fs.until = Some(end);
        // `SETTLE` counts from the end of the animation (an instant in the future reads as 0 elapsed).
        self.resized_at = Some(end);
        self.fs.runs += 1;
        eprintln!("[frameplayer mpv] fs-anim {}: {from:?} -> {to:?} over {ms} ms, chain {cw}x{ch}, fit {a:?} -> {b:?}", self.fs.runs);
        true
    }

    /// FPUI_FS_ANIM, entering: remember the video's box on screen; the next `place` with another size animates.
    pub unsafe fn fs_enter(&mut self, hwnd: isize, ms: u32) -> bool {
        let Some([x, y, w, h]) = self.bounds else { return false };
        let (ox, oy) = client_origin(hwnd);
        let screen = [x + ox, y + oy, w, h];
        self.fs.hwnd = hwnd;
        self.fs.enter_origin = (ox, oy);
        self.fs.restore = Some(screen);
        self.fs.pending = Some((screen, ms, Instant::now()));
        true
    }

    /// FPUI_FS_ANIM, leaving: the window is still fullscreen; the visual shrinks to where it was before
    /// entering, and the caller restores the window when `ms` is up. `false`: nothing to animate to.
    pub unsafe fn fs_exit(&mut self, hwnd: isize, ms: u32) -> bool {
        let (Some(cur), Some(r)) = (self.bounds, self.fs.restore) else { return false };
        let (ox, oy) = client_origin(hwnd);
        let to = [r[0] - ox, r[1] - oy, r[2], r[3]];
        if !self.animate(cur, to, ms) {
            return false;
        }
        self.fs.hwnd = hwnd;
        self.fs.full_origin = (ox, oy);
        // The page does not change while the window is still fullscreen, so no frame of Skathi's carries it.
        if let Some(d) = &self.device {
            let _ = d.Commit();
        }
        self.fs.landing = true;
        self.fs.exit_at = Some(Instant::now());
        true
    }

    /// FPUI_FS_ANIM: whether the transition the stub started last is still under way here (tauri-stub.ts
    /// `setFullscreen` starts the next one only after this reads `false`). The way in waiting for its first
    /// `place` or animating; the way out animating or waiting for the window's restore (`fs_landed`); either way
    /// the swap chain not yet the box's size (`SETTLE`, then a frame rendered at that size). A second transition
    /// started inside any of these took its box from the first one's state: `fs_enter` over a pending way out
    /// replaced `restore` with the full-screen box, `fs_exit` over a running way in restarted from the full box
    /// (one frame of the picture past where it was, measured), and the exit's restore landing after a new way in
    /// left the window and the page apart. Each wait is bounded, so a window that never moves does not
    /// hold the next request for longer than a second.
    pub fn fs_busy(&self) -> bool {
        let now = Instant::now();
        let waiting = self.fs.pending.is_some_and(|(_, _, at)| at.elapsed() < Duration::from_secs(1));
        let animating = self.fs.until.is_some_and(|t| now < t);
        let landing = self.fs.landing && self.fs.exit_at.is_some_and(|t| t.elapsed() < Duration::from_secs(1));
        let settling = self.bounds.is_some_and(|b| b[2] > 0 && b[3] > 0 && (b[2] as u32, b[3] as u32) != self.chain_size)
            && self.chain.is_some()
            && self.resized_at.is_some_and(|t| t.elapsed() < SETTLE + Duration::from_millis(400));
        waiting || animating || landing || settling
    }

    /// FPUI_FS_ANIM: the window has just moved (WM_WINDOWPOSCHANGED, on its thread). The visual's offset is in
    /// the window's coordinates, so until Skathi places the layer again the picture rides along with the
    /// window: leaving, four desktop frames of a displaced picture; entering, three to four of the windowed
    /// picture in the screen's top-left corner (both measured). Put it back at its box on screen, in the new
    /// coordinates, and commit now, with the window's own move.
    pub unsafe fn fs_landed(&mut self) {
        let (ox, oy) = client_origin(self.fs.hwnd);
        // Entering: the window is at the screen's origin and the animation waits for its first `place`.
        if let Some((from, _, at)) = self.fs.pending {
            if (ox, oy) != self.fs.enter_origin && at.elapsed() < Duration::from_secs(2) && self.fs.until.is_none() {
                let b = [from[0] - ox, from[1] - oy, from[2], from[3]];
                self.bounds = Some(b);
                // The window is full screen from this commit on, and the animation starts at the first `place`:
                // until then the black stays the old box's too.
                self.back_clip_set(Some(b));
                self.apply(true);
            }
            return;
        }
        if !self.fs.landing {
            return;
        }
        let Some(r) = self.fs.restore else { return };
        if (ox, oy) == self.fs.full_origin {
            return; // still the fullscreen window
        }
        // The window has moved: the animation is over whatever its clock says (the page's timer and this one's
        // `until` are two clocks; skipping here left the landing to Skathi's next placement).
        let early = self.fs.until.map(|t| t.saturating_duration_since(Instant::now()));
        eprintln!("[frameplayer mpv] fs-anim landed: origin {:?} -> {:?}, animation left {early:?}", self.fs.full_origin, (ox, oy));
        self.fs.landing = false;
        self.fs.until = None;
        self.bounds = Some([r[0] - ox, r[1] - oy, r[2], r[3]]);
        self.apply(true);
    }

    /// The backstop's clip (`back_clip`): a box in window pixels, or none (the whole window). Not committed here.
    unsafe fn back_clip_set(&mut self, b: Option<[i32; 4]>) {
        let (Some(holder), Some(bc)) = (&self.backstop, &self.back_clip) else { return };
        match b {
            Some([x, y, w, h]) => {
                let _ = bc.SetLeft2(x as f32);
                let _ = bc.SetTop2(y as f32);
                let _ = bc.SetRight2((x + w) as f32);
                let _ = bc.SetBottom2((y + h) as f32);
                self.back_clipped = holder.SetClip(bc).is_ok();
            }
            None => {
                let _ = holder.SetClip(None::<&windows::Win32::Graphics::DirectComposition::IDCompositionClip>);
                self.back_clipped = false;
            }
        }
    }

    /// The picture's place (`fit`) and the clip for the box, and the visual in or out of the underlay.
    /// `commit`: only from the render thread. On the UI thread (`attach`, `place`) Skathi commits the device
    /// itself right after presenting the frame that carried the placement (`Layer::presented`); a commit of
    /// ours there put the video's new box on screen a frame before the page around it whenever Skathi dropped
    /// that present, which it does while a resize is in progress (KNOWN-ISSUES.md, "Live resize: frames out of order").
    unsafe fn apply(&mut self, commit: bool) {
        let animating = self.fs.until.is_some_and(|t| Instant::now() < t);
        // The backstop's clip goes once nothing holds it: not while the animation runs, nor while the way out
        // waits for the window's restore (`landing`), nor while the way in waits for its first `place`
        // (`pending`, at most its two seconds).
        let waiting = self.fs.pending.is_some_and(|(_, _, at)| at.elapsed() < Duration::from_secs(2));
        if self.back_clipped && !animating && !self.fs.landing && !waiting {
            self.back_clip_set(None);
        }
        let (Some(device), Some(underlay), Some(visual), Some(frame), Some(clip)) = (&self.device, &self.underlay, &self.visual, &self.frame, &self.clip) else { return };
        // FPUI_FS_ANIM: the transform belongs to the animation until it ends.
        if animating {
            if commit {
                let _ = device.Commit();
            }
            return;
        }
        self.fs.until = None;
        match self.bounds {
            Some([x, y, w, h]) => {
                let [sx, sy, ox, oy] = Self::fit(self.chain_size, [x as f64, y as f64, w as f64, h as f64]);
                let _ = visual.SetOffsetX2(ox);
                let _ = visual.SetOffsetY2(oy);
                let _ = visual.SetTransform2(&windows_numerics::Matrix3x2 { M11: sx, M12: 0.0, M21: 0.0, M22: sy, M31: 0.0, M32: 0.0 });
                let _ = clip.SetLeft2(x as f32);
                let _ = clip.SetTop2(y as f32);
                let _ = clip.SetRight2((x + w) as f32);
                let _ = clip.SetBottom2((y + h) as f32);
                if !self.added && self.chain.is_some() {
                    // In front of the backstop when there is one; with no reference, `true` puts a visual behind
                    // every sibling (measured: the backstop added with `false` covered the picture).
                    self.added = match &self.backstop {
                        Some(back) => underlay.AddVisual(frame, true, back),
                        None => underlay.AddVisual(frame, true, None::<&IDCompositionVisual>),
                    }
                    .is_ok();
                }
            }
            None => {
                if self.added {
                    let _ = underlay.RemoveVisual(frame);
                    self.added = false;
                }
            }
        }
        if commit {
            let _ = device.Commit();
        }
    }
}

/// The `skathi::Layer` of `<sk-layer name="video">`. Both calls come on Skathi's UI thread inside the paint.
pub struct SkVideoLayer(pub Arc<Mutex<SkVisual>>);

impl skathi::Layer for SkVideoLayer {
    fn attach(&mut self, host: &skathi::LayerHost) {
        let mut s = self.0.lock().unwrap();
        s.attaches += 1;
        unsafe {
            if let (true, Some(underlay), Some(frame)) = (s.added, &s.underlay, &s.frame) {
                let _ = underlay.RemoveVisual(frame);
            }
            if let (Some(underlay), Some(back)) = (&s.underlay, &s.backstop) {
                let _ = underlay.RemoveVisual(back);
            }
            s.backstop = None;
            s.backstop_black = None;
            s.back_clip = None;
            s.back_clipped = false;
            s.added = false;
            s.visual = None;
            s.frame = None;
            s.clip = None;
            let Some(c) = host.composition else {
                s.no_composition = true;
                s.device = None;
                s.underlay = None;
                eprintln!("[frameplayer mpv] sk-layer attach {}: window {} has no DirectComposition; nothing can be shown under it", s.attaches, host.window);
                return;
            };
            let device = IDCompositionDevice::from_raw_borrowed(&c.device.as_ptr()).cloned();
            let underlay = IDCompositionVisual::from_raw_borrowed(&c.underlay.as_ptr()).cloned();
            let visual = device.as_ref().and_then(|d| d.CreateVisual().ok());
            if let (Some(v), Some(chain)) = (&visual, &s.chain) {
                let _ = v.SetContent(chain);
            }
            // The chain's visual inside a clipped one (`SkVisual::frame`).
            let framed = device.as_ref().zip(visual.as_ref()).and_then(|(d, v)| {
                let frame = d.CreateVisual().ok()?;
                let clip = d.CreateRectangleClip().ok()?;
                frame.SetClip(&clip).ok()?;
                frame.AddVisual(v, false, None::<&IDCompositionVisual>).ok()?;
                Some((frame, clip))
            });
            eprintln!("[frameplayer mpv] sk-layer attach {}: window {}, device {:?}, underlay {:?}, visual made {}, clipped {}", s.attaches, host.window, c.device, c.underlay, visual.is_some(), framed.is_some());
            s.device = device;
            s.underlay = underlay;
            s.visual = visual;
            (s.frame, s.clip) = framed.unzip();
            s.apply(false);
        }
    }

    fn place(&mut self, placement: Option<skathi::LayerPlacement>) {
        let mut s = self.0.lock().unwrap();
        let bounds = placement.map(|p| [p.bounds[0].round() as i32, p.bounds[1].round() as i32, p.bounds[2].round().max(1.0) as i32, p.bounds[3].round().max(1.0) as i32]);
        match bounds {
            Some(_) => s.places += 1,
            None => s.hides += 1,
        }
        if s.places + s.hides <= 6 || bounds.is_none() != s.bounds.is_none() {
            eprintln!("[frameplayer mpv] sk-layer place {}: {:?}", s.places + s.hides, placement.map(|p| (p.bounds, p.clip, p.scale, p.opacity)));
        }
        let mut animated = false;
        if let (Some(a), Some(b)) = (s.bounds, bounds) {
            if a[2] != b[2] || a[3] != b[3] {
                // An animation's end is later than now: the settle counts from there.
                if !s.fs.until.is_some_and(|t| Instant::now() < t) {
                    s.resized_at = Some(Instant::now());
                }
                // FPUI_FS_ANIM, entering: the first new size is the fullscreen box; animate to it from the
                // video's old box on screen, in this window's new coordinates.
                if let Some((from, ms, at)) = s.fs.pending.take() {
                    if at.elapsed() < Duration::from_secs(2) {
                        let (ox, oy) = unsafe { client_origin(s.fs.hwnd) };
                        animated = unsafe { s.animate([from[0] - ox, from[1] - oy, from[2], from[3]], b, ms) };
                    }
                }
            }
        }
        s.bounds = bounds;
        if !animated {
            unsafe { s.apply(false) };
        }
        drop(s);
        let wake = SK_WAKE.load(Ordering::Relaxed);
        if !wake.is_null() {
            unsafe { &*wake }.kick(true);
        }
    }
}

/// Whether the layer is an HDR10 swap chain or an 8-bit one: HDR10 while the display the element
/// is on is in HDR, 8-bit sRGB otherwise, and `None` until the display is known. `FPUI_LAYER_SDR=1` forces
/// the 8-bit path on any display.
fn layer_hdr(display: &skathi::SurfaceDisplay) -> Option<bool> {
    if std::env::var("FPUI_LAYER_SDR").is_ok_and(|v| v == "1") {
        return Some(false);
    }
    display.known.then_some(display.hdr)
}

/// A window of ours directly under Skathi's, with a flip swap chain mpv renders into.
struct Layer {
    /// `sklayer`: no window of ours; the swap chain is a composition one, shown by Skathi's underlay.
    sk: Option<Arc<Mutex<SkVisual>>>,
    /// The swap chain's kind: `Some(true)` HDR10, `Some(false)` 8-bit sRGB.
    hdr: Option<bool>,
    above: HWND,
    win: HWND,
    factory: IDXGIFactory2,
    chain: Option<IDXGISwapChain1>,
    size: (u32, u32),
    rect: (i32, i32, i32, i32),
}

/// How long the element's box must keep its size before the `sklayer` swap chain is made that size.
const SETTLE: Duration = Duration::from_millis(120);

/// `Layer::make_backstop`, unless `FPUI_BACKSTOP=0`.
fn backstop_on() -> bool {
    std::env::var("FPUI_BACKSTOP").map_or(true, |v| v != "0")
}

impl Layer {
    /// `sklayer`, after a successful present. When the present went to a new chain of the settled size
    /// (`target_visual`), the visual takes it as its content, with the stretch for its size, in one commit of
    /// ours: content and transform change together, after the chain's first frame was handed to the
    /// compositor (and `wait` more, a refresh, so that it is the frame composed). The old chain goes.
    unsafe fn presented_size(&self, wait: Duration) {
        let (Some(sk), Some(chain)) = (&self.sk, &self.chain) else { return };
        if sk.lock().unwrap().chain.as_ref() == Some(chain) {
            return;
        }
        std::thread::sleep(wait);
        let mut s = sk.lock().unwrap();
        if let Some(v) = &s.visual {
            let _ = v.SetContent(chain);
        }
        s.chain = Some(chain.clone());
        s.chain_size = self.size;
        s.apply(true);
    }

    unsafe fn new(dev: &ID3D11Device, above: HWND) -> Result<Self, String> {
        let class = windows::core::w!("fpui-layer");
        let wc = WNDCLASSW { lpfnWndProc: Some(layer_proc), lpszClassName: class, ..Default::default() };
        RegisterClassW(&wc);
        let win = CreateWindowExW(WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW, class, PCWSTR::null(), WS_POPUP, 0, 0, 64, 64, None, None, Some(wc.hInstance), None)
            .map_err(|e| format!("CreateWindowExW (layer): {e}"))?;
        LAYER_ABOVE.store(above.0 as isize, Ordering::Relaxed);
        // An owned window is always above its owner: Skathi's window becomes ours.
        SetWindowLongPtrW(above, GWLP_HWNDPARENT, win.0 as isize);
        let factory: IDXGIFactory2 = dev.cast::<IDXGIDevice>().and_then(|d| d.GetAdapter()).and_then(|a| a.GetParent()).map_err(|e| format!("IDXGIFactory2: {e}"))?;
        Ok(Self { sk: None, hdr: None, above, win, factory, chain: None, size: (0, 0), rect: (0, 0, 0, 0) })
    }

    unsafe fn new_visual(dev: &ID3D11Device, sk: Arc<Mutex<SkVisual>>) -> Result<Self, String> {
        let factory: IDXGIFactory2 = dev.cast::<IDXGIDevice>().and_then(|d| d.GetAdapter()).and_then(|a| a.GetParent()).map_err(|e| format!("IDXGIFactory2: {e}"))?;
        Ok(Self { sk: Some(sk), hdr: None, above: HWND::default(), win: HWND::default(), factory, chain: None, size: (0, 0), rect: (0, 0, 0, 0) })
    }

    /// `sklayer`: the swap chain at the element's size (as `place` last gave it), made or resized here and
    /// handed to the visual; the back buffer to render into.
    unsafe fn target_visual(&mut self, dev: &ID3D11Device, hdr: Option<bool>) -> Result<Option<ID3D11Texture2D>, String> {
        let sk = self.sk.clone().unwrap();
        let mut s = sk.lock().unwrap();
        let (Some([_, _, w, h]), Some(want_hdr)) = (s.bounds, hdr) else { return Ok(None) };
        if s.visual.is_none() {
            return Ok(None);
        }
        if s.backstop.is_none() && backstop_on() {
            if let Err(e) = self.make_backstop(dev, &mut s) {
                eprintln!("[frameplayer mpv] sk-layer backstop: {e}");
            }
        }
        let size = (w as u32, h as u32);
        if self.chain.is_some() && self.hdr != Some(want_hdr) {
            if let Some(v) = &s.visual {
                let _ = v.SetContent(None::<&windows::core::IUnknown>);
            }
            s.chain = None;
            self.chain = None;
        }
        if self.chain.is_none() {
            let chain = self.make_chain(dev, size, want_hdr)?;
            if let Some(v) = &s.visual {
                let _ = v.SetContent(&chain);
            }
            s.chain = Some(chain.clone());
            s.chain_size = size;
            self.hdr = Some(want_hdr);
            self.chain = Some(chain);
            self.size = size;
            s.apply(true);
        } else if size != self.size && !s.resized_at.is_some_and(|t| t.elapsed() < SETTLE) {
            // The box has kept this size for `SETTLE`: a chain of that size, rendered and presented before the
            // visual takes it (`presented_size`), so content and stretch change in one commit. Until then the
            // chain keeps its size and the visual stretches it to the box, and that stretch goes on screen
            // only in Skathi's own commit, with the frame of the page that carried the box. A chain resized
            // with every step had its new-size frame presented on our clock while the window took the step
            // on Skathi's: shrinking, the picture was a step smaller than the window for a desktop frame,
            // the desktop showing through at its edge (measured, KNOWN-ISSUES.md, "Live resize: frames out of order").
            self.chain = Some(self.make_chain(dev, size, want_hdr)?);
            self.size = size;
            // Rendered even when nothing new is playing (paused).
            let wake = SK_WAKE.load(Ordering::Relaxed);
            if !wake.is_null() {
                (*wake).force.store(true, Ordering::Relaxed);
            }
        }
        let Some(chain) = &self.chain else { return Ok(None) };
        chain.GetBuffer::<ID3D11Texture2D>(0).map(Some).map_err(|e| format!("GetBuffer: {e}"))
    }

    /// A live resize on Windows shows each step's window frame a desktop frame before Skathi's content
    /// of that size arrives (measured on a page with no video too: KNOWN-ISSUES.md, "Live resize: a strip and a one-frame shift at the edge"). The window is
    /// transparent, so the strip in between showed whatever is behind it. This visual is opaque black,
    /// 16000 px square around the client origin, behind everything in the underlay: the strip is black
    /// instead, the colour of the player's own edges. A 16x16 swap chain cleared once and stretched;
    /// DirectComposition clips it to the window. `FPUI_BACKSTOP=0` leaves it out.
    unsafe fn make_backstop(&self, dev: &ID3D11Device, s: &mut SkVisual) -> Result<(), String> {
        let (Some(device), Some(underlay)) = (s.device.clone(), s.underlay.clone()) else { return Ok(()) };
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: 16,
            Height: 16,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            AlphaMode: DXGI_ALPHA_MODE_IGNORE,
            ..Default::default()
        };
        let chain = self.factory.CreateSwapChainForComposition(dev, &desc, None).map_err(|e| format!("CreateSwapChainForComposition: {e}"))?;
        let buf: ID3D11Texture2D = chain.GetBuffer(0).map_err(|e| format!("GetBuffer: {e}"))?;
        let mut rtv = None;
        dev.CreateRenderTargetView(&buf, None, Some(&mut rtv)).map_err(|e| format!("CreateRenderTargetView: {e}"))?;
        let ctx = dev.GetImmediateContext().map_err(|e| format!("GetImmediateContext: {e}"))?;
        ctx.ClearRenderTargetView(rtv.as_ref().unwrap(), &[0.0, 0.0, 0.0, 1.0]);
        chain.Present(0, DXGI_PRESENT(0)).ok().map_err(|e| format!("Present: {e}"))?;
        let visual = device.CreateVisual().map_err(|e| format!("CreateVisual: {e}"))?;
        visual.SetContent(&chain).map_err(|e| format!("SetContent: {e}"))?;
        let _ = visual.SetOffsetX2(-8000.0);
        let _ = visual.SetOffsetY2(-8000.0);
        let _ = visual.SetTransform2(&windows_numerics::Matrix3x2 { M11: 1000.0, M12: 0.0, M21: 0.0, M22: 1000.0, M31: 0.0, M32: 0.0 });
        // In an untransformed holder, whose clip is in window pixels (`SkVisual::back_clip`).
        let holder = device.CreateVisual().map_err(|e| format!("CreateVisual: {e}"))?;
        holder.AddVisual(&visual, false, None::<&IDCompositionVisual>).map_err(|e| format!("AddVisual: {e}"))?;
        let clip = device.CreateRectangleClip().ok();
        // Behind every sibling (`true` with no reference); `apply` adds the video in front of it.
        underlay.AddVisual(&holder, true, None::<&IDCompositionVisual>).map_err(|e| format!("AddVisual: {e}"))?;
        let _ = device.Commit();
        eprintln!("[frameplayer mpv] sk-layer backstop: black, behind the video");
        s.backstop = Some(holder);
        s.backstop_black = Some(visual);
        s.back_clip = clip;
        s.back_clipped = false;
        s.backstop_chain = Some(chain);
        Ok(())
    }

    unsafe fn make_chain(&self, dev: &ID3D11Device, size: (u32, u32), hdr: bool) -> Result<IDXGISwapChain1, String> {
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: size.0,
            Height: size.1,
            Format: if hdr { DXGI_FORMAT_R10G10B10A2_UNORM } else { DXGI_FORMAT_B8G8R8A8_UNORM },
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            AlphaMode: DXGI_ALPHA_MODE_IGNORE,
            ..Default::default()
        };
        let chain = self.factory.CreateSwapChainForComposition(dev, &desc, None).map_err(|e| format!("CreateSwapChainForComposition (sk-layer): {e}"))?;
        if hdr {
            let space = chain.cast::<IDXGISwapChain3>().and_then(|c3| c3.SetColorSpace1(DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020));
            eprintln!("[frameplayer mpv] sk-layer: {}x{} R10G10B10A2, HDR10 color space: {space:?}", size.0, size.1);
        } else {
            eprintln!("[frameplayer mpv] sk-layer: {}x{} B8G8R8A8, sRGB (8-bit)", size.0, size.1);
        }
        Ok(chain)
    }

    /// Keeps the window on Skathi's client area and the swap chain at its size; the back buffer to render into.
    unsafe fn target(&mut self, dev: &ID3D11Device, hdr: Option<bool>) -> Result<Option<ID3D11Texture2D>, String> {
        if self.sk.is_some() {
            return self.target_visual(dev, hdr);
        }
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            DispatchMessageW(&msg);
        }
        let mut c = RECT::default();
        let mut p = windows::Win32::Foundation::POINT::default();
        if GetClientRect(self.above, &mut c).is_err() || !windows::Win32::Graphics::Gdi::ClientToScreen(self.above, &mut p).as_bool() {
            return Ok(None);
        }
        let shown = IsWindowVisible(self.above).as_bool() && !IsIconic(self.above).as_bool() && c.right > 0 && c.bottom > 0;
        if !shown {
            if self.rect != (0, 0, 0, 0) {
                let _ = ShowWindow(self.win, SW_HIDE);
                self.rect = (0, 0, 0, 0);
            }
            return Ok(None);
        }
        let want = (p.x, p.y, c.right, c.bottom);
        if want != self.rect {
            let _ = SetWindowPos(self.win, Some(self.above), want.0, want.1, want.2, want.3, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            self.rect = want;
        }
        let size = (c.right as u32, c.bottom as u32);
        // The display's mode is not known yet, or it changed (HDR switched by the user, the window moved to
        // another monitor): a flip swap chain cannot change its kind in place, so it is made anew.
        let Some(want_hdr) = hdr else { return Ok(None) };
        if self.chain.is_some() && self.hdr != Some(want_hdr) {
            self.chain = None;
        }
        if self.chain.is_none() {
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: size.0,
                Height: size.1,
                Format: if want_hdr { DXGI_FORMAT_R10G10B10A2_UNORM } else { DXGI_FORMAT_B8G8R8A8_UNORM },
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                ..Default::default()
            };
            let chain = self.factory.CreateSwapChainForHwnd(dev, self.win, &desc, None, None).map_err(|e| format!("CreateSwapChainForHwnd (layer): {e}"))?;
            if want_hdr {
                let space = chain.cast::<IDXGISwapChain3>().and_then(|c3| c3.SetColorSpace1(DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020));
                eprintln!("[frameplayer mpv] layer: {}x{} R10G10B10A2, HDR10 color space: {space:?}", size.0, size.1);
            } else {
                eprintln!("[frameplayer mpv] layer: {}x{} B8G8R8A8, sRGB (8-bit)", size.0, size.1);
            }
            self.hdr = Some(want_hdr);
            self.chain = Some(chain);
            self.size = size;
        } else if let (true, Some(chain)) = (size != self.size, self.chain.as_ref()) {
            chain.ResizeBuffers(0, size.0, size.1, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0)).map_err(|e| format!("ResizeBuffers (layer): {e}"))?;
            self.size = size;
            eprintln!("[frameplayer mpv] layer: {}x{}", size.0, size.1);
        }
        self.chain.as_ref().unwrap().GetBuffer::<ID3D11Texture2D>(0).map(Some).map_err(|e| format!("GetBuffer: {e}"))
    }
}

impl Drop for Layer {
    fn drop(&mut self) {
        self.chain = None;
        if let Some(sk) = self.sk.take() {
            let mut s = sk.lock().unwrap();
            if let Some(v) = &s.visual {
                let _ = unsafe { v.SetContent(None::<&windows::core::IUnknown>) };
            }
            s.chain = None;
            s.chain_size = (0, 0);
            unsafe { s.apply(true) };
            return;
        }
        unsafe {
            LAYER_ABOVE.store(0, Ordering::Relaxed);
            let _ = DestroyWindow(self.win);
        }
    }
}

// ---- the overlay ---------------------------------------------------------------------------------

/// libplacebo's overlay pass leaves premultiplied alpha; Skathi composites a surface as straight
/// alpha. One texel in, one out: no sampler, no constants.
const UNPREMUL_HLSL: &str = "
Texture2D<float4> src : register(t0);
float4 vs(uint id : SV_VertexID) : SV_Position {
    float2 uv = float2((id << 1) & 2, id & 2);
    return float4(uv * float2(2, -2) + float2(-1, 1), 0, 1);
}
float4 ps(float4 pos : SV_Position) : SV_Target {
    float4 c = src.Load(int3(pos.xy, 0));
    if (c.a <= 0) return float4(0, 0, 0, 0);
    c.rgb = saturate(c.rgb / c.a);
    return c;
}
";

struct Unpremul {
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
}

impl Unpremul {
    unsafe fn new(dev: &ID3D11Device) -> Result<Self, String> {
        let compile = |entry: &[u8], target: &[u8]| -> Result<Vec<u8>, String> {
            let (mut code, mut errors) = (None, None);
            let r = D3DCompile(
                UNPREMUL_HLSL.as_ptr().cast(),
                UNPREMUL_HLSL.len(),
                PCSTR::null(),
                None,
                None,
                PCSTR(entry.as_ptr()),
                PCSTR(target.as_ptr()),
                0,
                0,
                &mut code,
                Some(&mut errors),
            );
            let text = |b: &ID3DBlob| std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize()).to_vec();
            match (r, code) {
                (Ok(()), Some(code)) => Ok(text(&code)),
                (e, _) => Err(format!("D3DCompile: {e:?} {}", errors.as_ref().map(|b| String::from_utf8_lossy(&text(b)).into_owned()).unwrap_or_default())),
            }
        };
        let (vs_code, ps_code) = (compile(b"vs\0", b"vs_5_0\0")?, compile(b"ps\0", b"ps_5_0\0")?);
        let (mut vs, mut ps) = (None, None);
        dev.CreateVertexShader(&vs_code, None, Some(&mut vs)).map_err(|e| format!("CreateVertexShader: {e}"))?;
        dev.CreatePixelShader(&ps_code, None, Some(&mut ps)).map_err(|e| format!("CreatePixelShader: {e}"))?;
        Ok(Self { vs: vs.ok_or("no vertex shader")?, ps: ps.ok_or("no pixel shader")? })
    }

    /// `src` into `dst`, both `size`. mpv's renderer sets every state it uses and asks for the
    /// rest at its defaults (render_d3d11.h), so the context is left cleared.
    unsafe fn run(&self, ctx: &ID3D11DeviceContext, src: &ID3D11ShaderResourceView, dst: &ID3D11RenderTargetView, size: (u32, u32)) {
        ctx.ClearState();
        ctx.OMSetRenderTargets(Some(&[Some(dst.clone())]), None);
        ctx.RSSetViewports(Some(&[D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: size.0 as f32, Height: size.1 as f32, MinDepth: 0.0, MaxDepth: 1.0 }]));
        ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        ctx.VSSetShader(&self.vs, None);
        ctx.PSSetShader(&self.ps, None);
        ctx.PSSetShaderResources(0, Some(&[Some(src.clone())]));
        ctx.Draw(3, 0);
        ctx.ClearState();
    }
}

/// The overlay's two textures: mpv's target (ours alone) and the shared one Skathi copies from.
/// A ring of one: text changes a few times a second at most, and the slot's release is waited for.
struct Overlay {
    /// The rows of the element the textures cover: `size.1` rows from `offset_y` down.
    size: (u32, u32),
    offset_y: u32,
    target: ID3D11Texture2D,
    target_view: ID3D11ShaderResourceView,
    slot: Slot,
    slot_view: ID3D11RenderTargetView,
}

unsafe fn make_overlay(stop: &AtomicBool, dev: &ID3D11Device, size: (u32, u32), offset_y: u32) -> Result<Overlay, String> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: size.0,
        Height: size.1,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut target = None;
    dev.CreateTexture2D(&desc, None, Some(&mut target)).map_err(|e| format!("CreateTexture2D (overlay) {}x{}: {e}", size.0, size.1))?;
    let target = target.ok_or("no texture")?;
    let mut target_view = None;
    dev.CreateShaderResourceView(&target, None, Some(&mut target_view)).map_err(|e| format!("CreateShaderResourceView: {e}"))?;
    let slot = make_slot(stop, dev, size.0, size.1, DXGI_FORMAT_B8G8R8A8_UNORM)?;
    let mut slot_view = None;
    dev.CreateRenderTargetView(&slot.tex, None, Some(&mut slot_view)).map_err(|e| format!("CreateRenderTargetView: {e}"))?;
    Ok(Overlay { size, offset_y, target, target_view: target_view.ok_or("no view")?, slot, slot_view: slot_view.ok_or("no view")? })
}

/// `FPUI_OVL_BAND=0`: the overlay element is always the whole video element.
fn band_wanted() -> bool {
    !std::env::var("FPUI_OVL_BAND").is_ok_and(|v| v == "0")
}

/// How long after the last text the overlay's textures are freed (`FPUI_OVL_DROP`, ms; 0: never).
fn overlay_drop_after() -> Option<Duration> {
    let ms: u64 = std::env::var("FPUI_OVL_DROP").ok().and_then(|v| v.parse().ok()).unwrap_or(2000);
    (ms > 0).then(|| Duration::from_millis(ms))
}

/// `SKIP_RENDERING`: mpv does its bookkeeping for the frame and nothing is drawn.
unsafe fn skip_frame(api: &Api, ctx: Handle) {
    let (mut skip, mut block): (c_int, c_int) = (1, 0);
    let mut params = [
        RenderParam { kind: PARAM_SKIP_RENDERING, data: (&mut skip as *mut c_int).cast() },
        RenderParam { kind: PARAM_BLOCK_FOR_TARGET_TIME, data: (&mut block as *mut c_int).cast() },
        RenderParam { kind: PARAM_END, data: std::ptr::null_mut() },
    ];
    (api.render_render)(ctx, params.as_mut_ptr());
    (api.render_report_swap)(ctx);
}

#[allow(clippy::too_many_arguments)]
fn render_thread(
    api: Arc<Api>,
    mpv: Raw,
    surface: SurfaceHandle,
    overlay_surface: Option<SurfaceHandle>,
    ui: Option<skathi::Handle>,
    stop: Arc<AtomicBool>,
    state: Arc<RenderState>,
    route: Route,
    geo: Arc<VideoGeo>,
    stats: Arc<Stats>,
    window: isize,
    sk_visual: Arc<Mutex<SkVisual>>,
) {
    let fail = |e: String| {
        eprintln!("[frameplayer mpv] {e}");
        *state.error.lock().unwrap() = e;
        state.phase.store(2, Ordering::Release);
    };
    let ring_n: usize = std::env::var("FPUI_RING").ok().and_then(|v| v.parse().ok()).unwrap_or(RING).max(1);
    let skip_repeats = resample();
    unsafe {
        let dev = match own_device(&stop, &surface) {
            Ok(d) => d,
            Err(e) => return fail(e),
        };
        let (ctx, ctx4) = match dev.GetImmediateContext().and_then(|c| c.cast::<ID3D11DeviceContext4>().map(|c4| (c, c4))) {
            Ok(c) => c,
            Err(e) => return fail(format!("immediate context: {e}")),
        };
        let slot_event = match CreateEventW(None, false, false, PCWSTR::null()) {
            Ok(e) => e,
            Err(e) => return fail(format!("CreateEventW: {e}")),
        };
        let api_type = CString::new("d3d11").unwrap();
        let mut init = D3d11Init { device: dev.as_raw() };
        let renderer = std::env::var("FPUI_RENDERER").ok().and_then(|r| CString::new(r).ok());
        let mut params = vec![
            RenderParam { kind: PARAM_API_TYPE, data: api_type.as_ptr() as *mut c_void },
            RenderParam { kind: PARAM_D3D11_INIT_PARAMS, data: (&mut init as *mut D3d11Init).cast() },
        ];
        if let Some(r) = &renderer {
            params.push(RenderParam { kind: PARAM_RENDERER, data: r.as_ptr() as *mut c_void });
        }
        params.push(RenderParam { kind: PARAM_END, data: std::ptr::null_mut() });
        let mut rctx: Handle = std::ptr::null_mut();
        let e = (api.render_create)(&mut rctx, mpv.0, params.as_mut_ptr());
        if e != 0 {
            let _ = CloseHandle(slot_event);
            return fail(format!("mpv_render_context_create (d3d11): {}", api.err(e)));
        }
        let wake: &'static Wake = Box::leak(Box::new(Wake { pending: Mutex::new(false), cond: Condvar::new(), force: AtomicBool::new(false) }));
        (api.render_set_update_callback)(rctx, Some(on_update), (wake as *const Wake) as *mut c_void);
        // (The surface's format is set below, and not at all on the layer route: a half-float surface is what
        // switches Skathi's window to half-float output.)
        // A resize, a visibility change or a new device: look again at once, and draw the frame anew.
        let _watch = surface.on_change(move |_, _| wake.kick(true));
        // The overlay: on until the libmpv turns out not to know the parameter.
        if route != Route::Layer && route != Route::SkLayer {
            surface.set_format(SurfaceFormat::Rgba16Float);
        }
        let unpremul = match (route == Route::Split, &overlay_surface) {
            (true, Some(o)) => {
                o.set_format(SurfaceFormat::Bgra8);
                match Unpremul::new(&dev) {
                    Ok(u) => Some(u),
                    Err(e) => {
                        eprintln!("[frameplayer mpv] overlay off: {e}");
                        None
                    }
                }
            }
            _ => None,
        };
        let _watch_overlay = overlay_surface.as_ref().filter(|_| unpremul.is_some()).map(|o| o.on_change(move |_, _| wake.kick(true)));
        let mut split = unpremul.is_some();
        stats.split.store(split, Ordering::Relaxed);
        let drop_after = overlay_drop_after();
        let mut overlay: Option<Overlay> = None;
        // On screen now / the next render has to draw the text again / when text was last seen /
        // the element asked to be the whole video element, since when the text has fitted the
        // band again, and when the page was last told.
        let (mut ovl_shown, mut ovl_force, mut ovl_last_text) = (false, false, Instant::now());
        let (mut ovl_full, mut ovl_calm, mut ovl_told) = (!band_wanted(), None::<Instant>, None::<(bool, Instant)>);
        eprintln!(
            "[frameplayer mpv] D3D11 render context (renderer {}); route {route:?}, ring {ring_n}, {}",
            renderer.as_ref().map_or("not passed".into(), |r| r.to_string_lossy().into_owned()),
            if skip_repeats { "display-resample, repeats skipped" } else { "video-sync=audio" }
        );
        state.phase.store(1, Ordering::Release);

        let idle_ring = std::env::var("FPUI_IDLE_RING").is_ok_and(|v| v == "1");
        // The layer under the window instead of the surface's ring.
        if route == Route::SkLayer {
            SK_WAKE.store(wake as *const Wake as *mut Wake, Ordering::Relaxed);
        }
        let mut layer = match (route == Route::Layer, window) {
            _ if route == Route::SkLayer => match Layer::new_visual(&dev, sk_visual.clone()) {
                Ok(l) => Some(l),
                Err(e) => return fail(e),
            },
            (true, 0) => return fail("the layer route needs the window's handle".into()),
            (true, h) => match Layer::new(&dev, HWND(h as *mut _)) {
                Ok(l) => Some(l),
                Err(e) => return fail(e),
            },
            _ => None,
        };
        let mut ring: Vec<Slot> = Vec::new();
        let mut size = (0u32, 0u32);
        let mut index = 0usize;
        let mut logged_errors = 0u32;
        let mut last_hidden_log = Instant::now();
        // The running average of a turn (texture wait, render, present), and the last refresh
        // period seen, for a turn that finds none.
        let mut render_time = Duration::ZERO;
        let mut last_period = Duration::from_micros(4167);
        while !stop.load(Ordering::Relaxed) {
            {
                let mut pending = wake.pending.lock().unwrap();
                if !*pending {
                    pending = wake.cond.wait_timeout(pending, Duration::from_millis(50)).unwrap().0;
                }
                *pending = false;
            }
            let info = surface.info();
            let shown = info.visible && info.width > 0 && info.height > 0;
            let elem = (info.width, info.height);
            let video = (geo.w.load(Ordering::Relaxed), geo.h.load(Ordering::Relaxed));
            let want = if route == Route::Element { elem } else { picture_size(elem, video) };
            if !shown && ovl_shown {
                ovl_shown = false;
            }
            // No picture, no ring: on the start screen mpv has nothing but one black frame,
            // and a ring for it is two element-sized half-float textures plus Skathi's copies of them (and the
            // window's switch to half-float output) behind an interface that covers them. The ring is made when
            // the video's size is known and dropped when the file is gone. FPUI_IDLE_RING=1: as before.
            if let Some(layer) = layer.as_mut() {
                // The whole turn of the layer route: pace, then render into the back buffer and present it.
                // sklayer: the display as Skathi tells the embedder (Handle::display), not through the surface.
                let shown_on = match (&ui, route == Route::SkLayer) {
                    (Some(ui), true) => ui.display().unwrap_or(info.display),
                    _ => info.display,
                };
                let target = match layer.target(&dev, layer_hdr(&shown_on)) {
                    Ok(t) => t,
                    Err(e) => {
                        if logged_errors < 20 {
                            logged_errors += 1;
                            eprintln!("[frameplayer mpv] {e}");
                        }
                        None
                    }
                };
                let update = (api.render_update)(rctx) & UPDATE_FRAME != 0;
                let force = target.is_some() && wake.force.swap(false, Ordering::Relaxed);
                if !(update || force) {
                    continue;
                }
                // A forced turn (a new size, a redraw) is not paced: the picture of the new size is late already,
                // and the wait was up to a refresh more of the old frame stretched (KNOWN-ISSUES.md, "Live resize: frames out of order").
                if skip_repeats && !force {
                    let d = info.display;
                    match (d.last_vblank, d.refresh_period) {
                        (Some(vblank), Some(period)) => {
                            let p = (period.as_nanos() as u64).max(1);
                            let phase = (0.5 - render_time.as_secs_f64() / period.as_secs_f64()).max(0.0);
                            let first = vblank + (p as f64 * phase) as u64;
                            let now = skathi::monotonic_ns();
                            let at = first + now.saturating_sub(first).div_ceil(p) * p;
                            std::thread::sleep(Duration::from_nanos(at.saturating_sub(now)));
                            last_period = period;
                        }
                        _ => std::thread::sleep(d.refresh_period.unwrap_or(last_period)),
                    }
                }
                let mut fi = FrameInfo::default();
                (api.render_get_info)(rctx, RenderParam { kind: PARAM_NEXT_FRAME_INFO, data: (&mut fi as *mut FrameInfo).cast() });
                let flags = if fi.flags == 0 { FRAME_REDRAW } else { fi.flags };
                let repeat = skip_repeats && flags & FRAME_REPEAT != 0 && flags & FRAME_REDRAW == 0 && !force;
                let has_video = video.0 > 0 && video.1 > 0;
                let Some(tex) = target.filter(|_| !repeat && has_video) else {
                    skip_frame(&api, rctx);
                    stats.skipped.fetch_add(1, Ordering::Relaxed);
                    continue;
                };
                let began = Instant::now();
                let mut fbo = D3d11Fbo { tex: tex.as_raw() };
                let mut block: c_int = 0;
                let mut params = [
                    RenderParam { kind: PARAM_D3D11_FBO, data: (&mut fbo as *mut D3d11Fbo).cast() },
                    RenderParam { kind: PARAM_BLOCK_FOR_TARGET_TIME, data: (&mut block as *mut c_int).cast() },
                    RenderParam { kind: PARAM_END, data: std::ptr::null_mut() },
                ];
                let e = (api.render_render)(rctx, params.as_mut_ptr());
                drop(tex);
                if e < 0 {
                    stats.render_errors.fetch_add(1, Ordering::Relaxed);
                }
                match layer.chain.as_ref().unwrap().Present(0, DXGI_PRESENT(0)).ok() {
                    Ok(()) => {
                        layer.presented_size(info.display.refresh_period.unwrap_or(last_period));
                        stats.presented.fetch_add(1, Ordering::Relaxed);
                        stats.ring_w.store(layer.size.0, Ordering::Relaxed);
                        stats.ring_h.store(layer.size.1, Ordering::Relaxed);
                    }
                    Err(e) => {
                        stats.present_errors.fetch_add(1, Ordering::Relaxed);
                        if logged_errors < 20 {
                            logged_errors += 1;
                            eprintln!("[frameplayer mpv] layer present: {e}");
                        }
                    }
                }
                render_time = (render_time * 7 + began.elapsed()) / 8;
                (api.render_report_swap)(rctx);
                continue;
            }
            let has_picture = idle_ring || (video.0 > 0 && video.1 > 0);
            if !has_picture && !ring.is_empty() {
                ring.clear();
                size = (0, 0);
                surface.clear();
                if let (true, Some(o)) = (ovl_shown, &overlay_surface) {
                    o.clear();
                }
                ovl_shown = false;
                overlay = None;
                stats.ring_w.store(0, Ordering::Relaxed);
                stats.ring_h.store(0, Ordering::Relaxed);
                eprintln!("[frameplayer mpv] ring dropped: no picture");
            }
            if shown && has_picture && (ring.is_empty() || want != size) {
                // Presents in flight keep their own references to the old textures and fences, and
                // every one of them had its `wait` value signalled.
                match make_ring(&stop, &dev, ring_n, want) {
                    Ok(slots) => {
                        ring = slots;
                        size = want;
                        index = 0;
                        stats.rebuilds.fetch_add(1, Ordering::Relaxed);
                        stats.ring_w.store(size.0, Ordering::Relaxed);
                        stats.ring_h.store(size.1, Ordering::Relaxed);
                        wake.force.store(true, Ordering::Relaxed);
                        eprintln!("[frameplayer mpv] ring: {}x{} x{ring_n} (element {}x{}, video {}x{})", size.0, size.1, elem.0, elem.1, video.0, video.1);
                    }
                    Err(e) => {
                        eprintln!("[frameplayer mpv] ring {}x{}: {e}; retrying", want.0, want.1);
                        ring.clear();
                        size = (0, 0);
                        std::thread::sleep(Duration::from_millis(20));
                        wake.kick(true);
                        continue;
                    }
                }
            }
            let update = (api.render_update)(rctx) & UPDATE_FRAME != 0;
            let force = shown && wake.force.swap(false, Ordering::Relaxed);
            if !(update || force) {
                continue;
            }
            // Pacing, as Skathi's documentation has it (and as measured): with display-resample mpv draws once per refresh and takes its clock from
            // how long each turn takes, so each turn starts at a fixed point of the refresh, early
            // enough that the present lands near its middle. Without this nothing holds mpv back:
            // a skipped frame returns at once, and a hidden window played a 30 s clip in under 2 s.
            if skip_repeats {
                let d = info.display;
                match (d.last_vblank, d.refresh_period) {
                    (Some(vblank), Some(period)) => {
                        let p = (period.as_nanos() as u64).max(1);
                        let phase = (0.5 - render_time.as_secs_f64() / period.as_secs_f64()).max(0.0);
                        let first = vblank + (p as f64 * phase) as u64;
                        let now = skathi::monotonic_ns();
                        let target = first + now.saturating_sub(first).div_ceil(p) * p;
                        std::thread::sleep(Duration::from_nanos(target.saturating_sub(now)));
                        last_period = period;
                    }
                    _ => std::thread::sleep(d.refresh_period.unwrap_or(last_period)),
                }
            }
            // What mpv is about to draw: a new video frame, a repeat of the last one (display-sync
            // draws once per refresh) or a redraw. No frame waiting (a forced render): a redraw.
            let mut fi = FrameInfo::default();
            (api.render_get_info)(rctx, RenderParam { kind: PARAM_NEXT_FRAME_INFO, data: (&mut fi as *mut FrameInfo).cast() });
            let flags = if fi.flags == 0 { FRAME_REDRAW } else { fi.flags };
            // Nothing can show it, or a repeat is the picture already on screen: mpv is told the
            // frame was drawn (so its count of refreshes goes on), nothing is rendered or presented.
            let repeat = skip_repeats && flags & FRAME_REPEAT != 0 && flags & FRAME_REDRAW == 0 && !force;
            if !shown || ring.is_empty() || repeat {
                if !shown && last_hidden_log.elapsed() > Duration::from_secs(2) {
                    last_hidden_log = Instant::now();
                    eprintln!("[frameplayer mpv] nothing to show on: element {}x{} visible {} display known {}", info.width, info.height, info.visible, info.display.known);
                }
                skip_frame(&api, rctx);
                stats.skipped.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            // The next texture, free once its last accepted present was released.
            let began = Instant::now();
            let slot = &mut ring[index];
            if slot.fence.GetCompletedValue() < slot.last_release {
                let free = slot.fence.SetEventOnCompletion(slot.last_release, slot_event).is_ok()
                    && WaitForSingleObject(slot_event, SLOT_TIMEOUT_MS) == WAIT_OBJECT_0;
                if !free {
                    eprintln!("[frameplayer mpv] texture {index} was not released within {SLOT_TIMEOUT_MS} ms; frame not rendered");
                    stats.timeouts.fetch_add(1, Ordering::Relaxed);
                    wake.force.store(true, Ordering::Relaxed);
                    continue;
                }
            }
            // The overlay is for a scaled picture: at the element's own size the text in the
            // picture is as sharp, and a second surface would only cost memory.
            let osurf = overlay_surface.as_ref().filter(|_| split && size != elem);
            if osurf.is_none() && (ovl_shown || overlay.is_some()) {
                if let (true, Some(o)) = (ovl_shown, &overlay_surface) {
                    o.clear();
                }
                ovl_shown = false;
                overlay = None;
            }
            // The overlay element: the video element's width and its bottom rows, or all of it.
            let band = match osurf.map(|o| o.info()) {
                Some(i) if i.width == elem.0 && i.height > 0 && i.height <= elem.1 => (i.width, i.height),
                _ => elem,
            };
            let band_y = elem.1 - band.1;
            if overlay.as_ref().is_some_and(|o| o.size != band || o.offset_y != band_y) {
                overlay = None;
            }
            // The page is told which of the two the text needs, until the element has followed.
            if let (Some(_), Some(ui)) = (osurf, &ui) {
                let is_full = band == elem;
                if is_full != ovl_full && ovl_told.is_none_or(|(full, at)| full != ovl_full || at.elapsed() > Duration::from_millis(250)) {
                    let _ = ui.emit_to("mpv", "overlay", &json!({ "full": ovl_full }));
                    ovl_told = Some((ovl_full, Instant::now()));
                }
            }
            let mut ov = osurf.is_some().then(|| D3d11OverlayFbo {
                struct_size: std::mem::size_of::<D3d11OverlayFbo>(),
                tex: overlay.as_ref().map_or(std::ptr::null_mut(), |o| o.target.as_raw()),
                layout_w: elem.0 as c_int,
                layout_h: elem.1 as c_int,
                offset_x: 0,
                offset_y: band_y as c_int,
                force: c_int::from(ovl_force),
                rendered: -1,
                bbox: [0; 4],
            });
            let mut fbo = D3d11Fbo { tex: slot.tex.as_raw() };
            let mut block: c_int = 0;
            let mut params = vec![
                RenderParam { kind: PARAM_D3D11_FBO, data: (&mut fbo as *mut D3d11Fbo).cast() },
                RenderParam { kind: PARAM_BLOCK_FOR_TARGET_TIME, data: (&mut block as *mut c_int).cast() },
            ];
            if let Some(ov) = ov.as_mut() {
                params.push(RenderParam { kind: PARAM_D3D11_OVERLAY_FBO, data: (ov as *mut D3d11OverlayFbo).cast() });
            }
            params.push(RenderParam { kind: PARAM_END, data: std::ptr::null_mut() });
            let e = (api.render_render)(rctx, params.as_mut_ptr());
            if e < 0 {
                stats.render_errors.fetch_add(1, Ordering::Relaxed);
                if logged_errors < 5 {
                    logged_errors += 1;
                    eprintln!("[frameplayer mpv] render: {}", api.err(e));
                }
            }
            // The overlay of this render: (wait, release) when the shared texture got new text.
            let mut ovl_present: Option<(u64, u64)> = None;
            let mut again = false;
            if let (Some(ov), Some(osurf), true) = (ov.as_ref(), osurf, e >= 0) {
                if ov.rendered != 0 && ov.rendered != -1 {
                    ovl_last_text = Instant::now();
                    // Text above the band: the element has to be the whole one. Back to the band
                    // once the text has stayed inside its rows for a second.
                    let band_top = if band_wanted() { (elem.1 - elem.1.div_ceil(3)) as c_int } else { 0 };
                    if ov.bbox[1] < band_top {
                        ovl_full = true;
                        ovl_calm = None;
                    } else if ovl_full && band_wanted() && ovl_calm.get_or_insert_with(Instant::now).elapsed() > Duration::from_secs(1) {
                        ovl_full = false;
                    }
                }
                match ov.rendered {
                    -1 => {
                        // This libmpv ignored the parameter: subtitles and OSD are in the picture.
                        eprintln!("[frameplayer mpv] this libmpv has no D3D11 overlay target (or the renderer is not gpu-next): subtitles stay in the picture");
                        split = false;
                        stats.split.store(false, Ordering::Relaxed);
                        overlay = None;
                    }
                    0 => {
                        if ovl_shown {
                            osurf.clear();
                            ovl_shown = false;
                        }
                        if band_wanted() {
                            ovl_full = false;
                        }
                        if overlay.is_some() && drop_after.is_some_and(|d| ovl_last_text.elapsed() >= d) {
                            overlay = None;
                            stats.ovl_drops.fetch_add(1, Ordering::Relaxed);
                            // Skathi's own copies of the last text stay (22 MB screen-sized): `clear()`
                            // does not free them, and neither did a 16x16 transparent frame.
                            eprintln!("[frameplayer mpv] overlay textures freed");
                        }
                    }
                    3 => {
                        // Text and no texture: made now, drawn in a render of its own right away.
                        match make_overlay(&stop, &dev, band, band_y) {
                            Ok(o) => {
                                overlay = Some(o);
                                stats.ovl_allocs.fetch_add(1, Ordering::Relaxed);
                                stats.ovl_w.store(band.0, Ordering::Relaxed);
                                stats.ovl_h.store(band.1, Ordering::Relaxed);
                                eprintln!("[frameplayer mpv] overlay: {}x{} from row {band_y} (element {}x{})", band.0, band.1, elem.0, elem.1);
                            }
                            Err(e) => {
                                if logged_errors < 20 {
                                    logged_errors += 1;
                                    eprintln!("[frameplayer mpv] overlay {}x{}: {e}", band.0, band.1);
                                }
                                std::thread::sleep(Duration::from_millis(20));
                            }
                        }
                        ovl_force = true;
                        again = true;
                    }
                    1 => {
                        ovl_force = false;
                        if let (Some(o), Some(un)) = (overlay.as_mut(), unpremul.as_ref()) {
                            // The shared texture is free once Skathi's copy of the last text is done.
                            let free = o.slot.fence.GetCompletedValue() >= o.slot.last_release
                                || (o.slot.fence.SetEventOnCompletion(o.slot.last_release, slot_event).is_ok()
                                    && WaitForSingleObject(slot_event, SLOT_TIMEOUT_MS) == WAIT_OBJECT_0);
                            if free {
                                un.run(&ctx, &o.target_view, &o.slot_view, o.size);
                                let (wait, release) = (o.slot.next, o.slot.next + 1);
                                o.slot.next += 2;
                                if let Err(e) = ctx4.Signal(&o.slot.fence, wait) {
                                    eprintln!("[frameplayer mpv] Signal (overlay): {e}");
                                }
                                ovl_present = Some((wait, release));
                            } else {
                                stats.timeouts.fetch_add(1, Ordering::Relaxed);
                                ovl_force = true;
                                again = true;
                            }
                        }
                    }
                    _ => {
                        // Unchanged. Not on screen (a refused present, a cleared surface): draw it again.
                        stats.ovl_unchanged.fetch_add(1, Ordering::Relaxed);
                        if !ovl_shown {
                            ovl_force = true;
                            again = true;
                        }
                    }
                }
            }
            // Hand over: signal `wait` (always, even after a failure), flush, present.
            let slot = &mut ring[index];
            let (wait, release) = (slot.next, slot.next + 1);
            slot.next += 2;
            if let Err(e) = ctx4.Signal(&slot.fence, wait) {
                eprintln!("[frameplayer mpv] Signal: {e}");
            }
            ctx.Flush();
            // The text first, the picture right after it: both are of one mpv render.
            if let (Some((wait, release)), Some(o), Some(osurf)) = (ovl_present, overlay.as_mut(), osurf) {
                match osurf.present_shared_fenced(&o.slot.shared, wait, release) {
                    Ok(_) => {
                        o.slot.last_release = release;
                        ovl_shown = true;
                        stats.ovl_presented.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => {
                        if matches!(e, SurfaceError::Busy) {
                            stats.ovl_busy.fetch_add(1, Ordering::Relaxed);
                        } else if logged_errors < 20 {
                            logged_errors += 1;
                            eprintln!("[frameplayer mpv] overlay present: {e}");
                        }
                        if matches!(e, SurfaceError::DeviceChanged) {
                            overlay = None;
                        }
                        ovl_force = true;
                        again = true;
                    }
                }
            }
            if again {
                wake.kick(true);
            }
            match surface.present_shared_fenced(&slot.shared, wait, release) {
                Ok(_) => {
                    slot.last_release = release;
                    index = (index + 1) % ring.len();
                    stats.presented.fetch_add(1, Ordering::Relaxed);
                }
                Err(SurfaceError::Busy) => {
                    // The frame is dropped; a still picture must get through, so ask again.
                    stats.busy.fetch_add(1, Ordering::Relaxed);
                    std::thread::sleep(Duration::from_millis(2));
                    wake.kick(true);
                }
                Err(e) => {
                    stats.present_errors.fetch_add(1, Ordering::Relaxed);
                    if logged_errors < 20 {
                        logged_errors += 1;
                        eprintln!("[frameplayer mpv] present: {e}");
                    }
                    if matches!(e, SurfaceError::DeviceChanged) {
                        // The renderer has another device: the textures have to be opened on it.
                        ring.clear();
                        size = (0, 0);
                        std::thread::sleep(Duration::from_millis(20));
                        wake.kick(true);
                    }
                }
            }
            render_time = (render_time * 7 + began.elapsed()) / 8;
            // One flip report per render, right after the present (Skathi's documented recipe).
            (api.render_report_swap)(rctx);
        }
        drop(layer);
        drop(_watch);
        drop(_watch_overlay);
        drop(overlay);
        drop(unpremul);
        ring.clear();
        (api.render_set_update_callback)(rctx, None, std::ptr::null_mut());
        (api.render_free)(rctx);
        let _ = CloseHandle(HANDLE(slot_event.0));
        eprintln!(
            "[frameplayer mpv] render thread ended: presented {} busy {} present errors {} render errors {} skipped {} slot timeouts {} ring rebuilds {}; overlay presented {} unchanged {} busy {} made {} freed {}",
            stats.presented.load(Ordering::Relaxed),
            stats.busy.load(Ordering::Relaxed),
            stats.present_errors.load(Ordering::Relaxed),
            stats.render_errors.load(Ordering::Relaxed),
            stats.skipped.load(Ordering::Relaxed),
            stats.timeouts.load(Ordering::Relaxed),
            stats.rebuilds.load(Ordering::Relaxed),
            stats.ovl_presented.load(Ordering::Relaxed),
            stats.ovl_unchanged.load(Ordering::Relaxed),
            stats.ovl_busy.load(Ordering::Relaxed),
            stats.ovl_allocs.load(Ordering::Relaxed),
            stats.ovl_drops.load(Ordering::Relaxed)
        );
    }
}

// ---- the module ----------------------------------------------------------------------------------

/// The producer's counters, read by the `producerStats` probe.
#[derive(Default)]
pub struct Stats {
    presented: AtomicU64,
    busy: AtomicU64,
    present_errors: AtomicU64,
    render_errors: AtomicU64,
    skipped: AtomicU64,
    timeouts: AtomicU64,
    rebuilds: AtomicU64,
    ring_w: AtomicU32,
    ring_h: AtomicU32,
    split: AtomicBool,
    ovl_presented: AtomicU64,
    ovl_unchanged: AtomicU64,
    ovl_busy: AtomicU64,
    ovl_allocs: AtomicU64,
    ovl_drops: AtomicU64,
    ovl_w: AtomicU32,
    ovl_h: AtomicU32,
}

struct Live {
    api: Arc<Api>,
    handle: Raw,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    render: Option<JoinHandle<()>>,
}

pub struct Core {
    /// The full path of libmpv-2.dll.
    pub dll: String,
    pub mute: bool,
    pub route: Route,
    pub ui: OnceLock<skathi::Handle>,
    pub surface: OnceLock<SurfaceHandle>,
    pub overlay: OnceLock<SurfaceHandle>,
    /// Skathi's window (HWND), for the layer route.
    pub window: std::sync::atomic::AtomicIsize,
    /// `sklayer`: what `<sk-layer name="video">` and the render thread share.
    pub sk_visual: Arc<Mutex<SkVisual>>,
    /// `sklayer`: the main window's display as `Builder::on_display_change` last gave it.
    pub display: Arc<Mutex<Option<skathi::SurfaceDisplay>>>,
    pub stats: Arc<Stats>,
    live: Mutex<Option<Live>>,
}

impl Core {
    pub fn new(dll: String, mute: bool, route: Route) -> Self {
        Self { dll, mute, route, ui: OnceLock::new(), surface: OnceLock::new(), overlay: OnceLock::new(), window: std::sync::atomic::AtomicIsize::new(0), sk_visual: Arc::default(), display: Arc::default(), stats: Arc::new(Stats::default()), live: Mutex::new(None) }
    }

    fn handle(&self) -> Result<(Arc<Api>, Raw), String> {
        self.live.lock().unwrap().as_ref().map(|l| (l.api.clone(), l.handle)).ok_or_else(|| "mpv is not initialized".to_string())
    }

    /// Removes Skathi's own D3D12 device in this process, for exercising device loss. Not exported
    /// to the page; kept for a debugging build.
    #[allow(dead_code)]
    pub fn lose_device(&self) -> String {
        use windows::core::Interface;
        let Some(surface) = self.overlay.get().or(self.surface.get()) else { return "no surface".into() };
        match surface.d3d12_device() {
            Ok(d) => unsafe {
                let raw = d.as_raw();
                match ID3D12Device::from_raw_borrowed(&raw).map(|d| d.cast::<windows::Win32::Graphics::Direct3D12::ID3D12Device5>()) {
                    Some(Ok(d5)) => {
                        d5.RemoveDevice();
                        format!("RemoveDevice called; reason now {:?}", d5.GetDeviceRemovedReason())
                    }
                    Some(Err(e)) => format!("no ID3D12Device5: {e}"),
                    None => "no D3D12 device".into(),
                }
            },
            Err(e) => format!("d3d12_device: {e}"),
        }
    }

    pub fn stats(&self) -> Value {
        let s = &self.stats;
        let n = |a: &AtomicU64| a.load(Ordering::Relaxed);
        let info = self.surface.get().map(|s| s.info());
        let surface = info.map(|i| json!({ "width": i.width, "height": i.height, "visible": i.visible, "displayKnown": i.display.known, "headroom": i.content_headroom(), "hdr": i.display.hdr, "refreshHz": i.display.refresh_hz }));
        json!({
            "surface": surface,
            "presented": n(&s.presented), "busy": n(&s.busy), "presentErrors": n(&s.present_errors),
            "renderErrors": n(&s.render_errors), "skipped": n(&s.skipped), "slotTimeouts": n(&s.timeouts),
            "ringRebuilds": n(&s.rebuilds), "ringWidth": s.ring_w.load(Ordering::Relaxed), "ringHeight": s.ring_h.load(Ordering::Relaxed),
            "overlay": { "on": s.split.load(Ordering::Relaxed), "presented": n(&s.ovl_presented), "unchanged": n(&s.ovl_unchanged), "busy": n(&s.ovl_busy),
                "made": n(&s.ovl_allocs), "freed": n(&s.ovl_drops), "width": s.ovl_w.load(Ordering::Relaxed), "height": s.ovl_h.load(Ordering::Relaxed),
                "element": self.overlay.get().map(|o| o.info()).map(|i| json!({ "width": i.width, "height": i.height, "visible": i.visible })) },
            "route": format!("{:?}", self.route), "sync": if resample() { "display-resample" } else { "audio" },
        })
    }

    pub fn init(&self, options: Vec<(String, Value)>, observed: Vec<(String, String)>) -> Result<(), String> {
        let mut live = self.live.lock().unwrap();
        if live.is_some() {
            return Err("mpv is already initialized".into());
        }
        let surface = self.surface.get().cloned().ok_or("no video surface")?;
        let api = Arc::new(Api::load(&self.dll)?);
        let handle = Raw(unsafe { (api.create)() });
        if handle.0.is_null() {
            return Err("mpv_create failed".into());
        }
        let destroy = |api: &Api| unsafe { (api.terminate_destroy)(handle.0) };

        // The interface's options, then what this host needs whatever they say: mpv draws through
        // the render API (no window of its own), into a linear-light extended sRGB surface whose
        // 1.0 is SDR white at 203 nits (the options the D3D11 producer was measured with).
        let mut all: Vec<(String, String)> = options
            .iter()
            .filter(|(k, _)| !matches!(k.as_str(), "wid" | "vo" | "gpu-api" | "gpu-context"))
            .filter_map(|(k, v)| text(v).map(|v| (k.clone(), v)))
            .collect();
        for (k, v) in [
            ("vo", "libmpv"),
            ("target-trc", "linear"),
            ("target-prim", "bt.709"),
            ("hdr-compute-peak", "no"),
            ("target-peak", "203"),
            ("video-sync", if resample() { "display-resample" } else { "audio" }),
        ] {
            all.push((k.into(), v.into()));
        }
        // The 8-bit layer's transfer: the interface's own `target-trc` (version 1's colour setting; mpv's `auto`,
        // no adaptation, when it sets none), as version 1 draws SDR on an SDR display. A forced `srgb` re-encoded
        // the picture: on an HDR desktop, composed as SDR, the shadows came out lifted (a ramp's 1/11 step
        // at 2.93 nits against Media Player's 1.94, measured). `FPUI_SDR_TRC` sets it for measuring.
        let sdr_trc = std::env::var("FPUI_SDR_TRC").ok().or_else(|| all.iter().rev().find(|(k, v)| k == "target-trc" && v != "linear").map(|(_, v)| v.clone())).unwrap_or_else(|| "auto".into());
        if self.route == Route::Layer || self.route == Route::SkLayer {
            // The layer's target is set by the control thread once the display is known (HDR10 or 8-bit sRGB);
            // until then mpv has nothing to draw into. The start is the 8-bit target, which is right on any display.
            for (k, v) in [("target-trc", sdr_trc.as_str()), ("target-prim", "bt.709"), ("target-peak", "auto")] {
                all.push((k.into(), v.into()));
            }
        }
        if self.mute {
            for (k, v) in [("ao", "null"), ("mute", "yes"), ("volume", "0")] {
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
        if std::env::var("FPUI_MPV_LOG").is_ok_and(|v| v == "1") {
            eprintln!("[frameplayer mpv] options: {all:?}");
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
                eprintln!("[frameplayer mpv] observe {name}: {}", api.err(e));
            }
        }

        let stop = Arc::new(AtomicBool::new(false));
        let state = Arc::new(RenderState { phase: AtomicU64::new(0), error: Mutex::new(String::new()) });
        let geo = Arc::new(VideoGeo::default());
        let render = {
            let (api, surface, stop, state) = (api.clone(), surface.clone(), stop.clone(), state.clone());
            let (route, geo, stats) = (self.route, geo.clone(), self.stats.clone());
            let (overlay, ui) = (self.overlay.get().cloned(), self.ui.get().cloned());
            let window = self.window.load(Ordering::Relaxed);
            let sk_visual = self.sk_visual.clone();
            std::thread::Builder::new()
                .name("mpv-render".into())
                .spawn(move || render_thread(api, handle, surface, overlay, ui, stop, state, route, geo, stats, window, sk_visual))
                .unwrap()
        };
        // vo=libmpv needs the render context before a file opens. The device comes once the window
        // has drawn (up to 15 s inside the thread).
        let began = Instant::now();
        while state.phase.load(Ordering::Acquire) == 0 && began.elapsed() < Duration::from_secs(20) {
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
                    if let (Some(ui), Some(json)) = (&ui, unsafe { event_json(event) }) {
                        let _ = ui.emit_to("mpv", "event", &json);
                    }
                })
                .unwrap()
        };
        // target-peak follows the element's content headroom (1.0 = SDR white = 203 nits), the
        // display rate follows the element's display (display-resample needs it: mpv has no window
        // to ask), and the render thread needs the video's display size for the picture target.
        let control = {
            let (api, stop) = (api.clone(), stop.clone());
            let layer_route = self.route == Route::Layer || self.route == Route::SkLayer;
            // sklayer: the display comes from Builder::on_display_change, not from the surface.
            let told = (self.route == Route::SkLayer).then(|| self.display.clone());
            std::thread::Builder::new()
                .name("mpv-control".into())
                .spawn(move || {
                    let handle = handle;
                    let (mut peak_set, mut hz_set) = (203.0f64, 0.0f64);
                    let mut layer_set = (-1i64, 0i64, 0i64);
                    let int = |name: &str| -> i64 {
                        let n = cstr(name).unwrap();
                        let mut v: i64 = 0;
                        let e = unsafe { (api.get_property)(handle.0, n.as_ptr(), FORMAT_INT64, (&mut v as *mut i64).cast()) };
                        if e < 0 { 0 } else { v }
                    };
                    let dbl = |name: &str, or: f64| -> f64 {
                        let n = cstr(name).unwrap();
                        let mut v: f64 = 0.0;
                        let e = unsafe { (api.get_property)(handle.0, n.as_ptr(), FORMAT_DOUBLE, (&mut v as *mut f64).cast()) };
                        if e < 0 { or } else { v }
                    };
                    let flag = |name: &str, or: bool| -> bool {
                        let n = cstr(name).unwrap();
                        let mut v: c_int = 0;
                        let e = unsafe { (api.get_property)(handle.0, n.as_ptr(), FORMAT_FLAG, (&mut v as *mut c_int).cast()) };
                        if e < 0 { or } else { v != 0 }
                    };
                    let text = |name: &str| -> Option<String> {
                        let n = cstr(name).unwrap();
                        let mut p: *mut c_char = std::ptr::null_mut();
                        let e = unsafe { (api.get_property)(handle.0, n.as_ptr(), FORMAT_STRING, (&mut p as *mut *mut c_char).cast()) };
                        if e < 0 || p.is_null() {
                            return None;
                        }
                        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
                        unsafe { (api.free)(p.cast()) };
                        Some(s)
                    };
                    let set = |name: &str, value: &str| {
                        let (n, v) = (cstr(name).unwrap(), cstr(value).unwrap());
                        unsafe { (api.set_property_string)(handle.0, n.as_ptr(), v.as_ptr()) };
                    };
                    while !stop.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(50));
                        let (mut w, mut h) = (int("video-out-params/dw").max(0) as u32, int("video-out-params/dh").max(0) as u32);
                        let rot90 = (int("video-out-params/rotate") + int("video-rotate")).rem_euclid(180) == 90;
                        if rot90 {
                            std::mem::swap(&mut w, &mut h);
                        }
                        geo.w.store(w, Ordering::Relaxed);
                        geo.h.store(h, Ordering::Relaxed);
                        if layer_route {
                            let (mut sw, mut sh) = (int("video-out-params/w") as i32, int("video-out-params/h") as i32);
                            if rot90 {
                                std::mem::swap(&mut sw, &mut sh);
                            }
                            let unscaled = match text("video-unscaled").as_deref() {
                                Some("yes") => 1,
                                Some("downscale-big") => 2,
                                _ => 0,
                            };
                            let layout = PicLayout {
                                dw: w as i32,
                                dh: h as i32,
                                w: sw,
                                h: sh,
                                keepaspect: flag("keepaspect", true),
                                unscaled,
                                recenter: flag("video-recenter", false),
                                panscan: dbl("panscan", 0.0),
                                zoom: dbl("video-zoom", 0.0),
                                pan: [dbl("video-pan-x", 0.0), dbl("video-pan-y", 0.0)],
                                align: [dbl("video-align-x", 0.0), dbl("video-align-y", 0.0)],
                                scale: [dbl("video-scale-x", 1.0), dbl("video-scale-y", 1.0)],
                                margin: [dbl("video-margin-ratio-left", 0.0), dbl("video-margin-ratio-right", 0.0), dbl("video-margin-ratio-top", 0.0), dbl("video-margin-ratio-bottom", 0.0)],
                            };
                            *PIC_LAYOUT.lock().unwrap() = layout;
                        }
                        let info = surface.info();
                        let peak = (203.0 * info.content_headroom() as f64).max(203.0);
                        if layer_route {
                            // HDR10 layer: PQ in BT.2020 to the display's peak, with SDR white at the
                            // desktop's SDR white level (Windows' "SDR content brightness", as Skathi reads it for the
                            // element's display), so that SDR video and the interface's white are the same light;
                            // 37.4's option. 8-bit layer: sRGB, mpv tone-maps HDR content down. Re-applied whenever
                            // the display's mode, SDR white or peak changes.
                            let d = match &told {
                                Some(t) => match *t.lock().unwrap() {
                                    Some(d) => d,
                                    None => continue,
                                },
                                None => info.display,
                            };
                            if let Some(hdr) = layer_hdr(&d) {
                                let white = d.sdr_white_nits.filter(|w| *w >= 40.0).unwrap_or(203.0).round();
                                let peak_nits = std::env::var("FPUI_LAYER_PEAK").ok().and_then(|v| v.parse::<f32>().ok()).or(d.peak_nits).filter(|p| *p >= white).unwrap_or(1000.0).round();
                                let key = if hdr { (1, white as i64, peak_nits as i64) } else { (0, 0, 0) };
                                if key != layer_set {
                                    if hdr {
                                        set("target-trc", "pq");
                                        set("target-prim", "bt.2020");
                                        set("target-peak", &format!("{peak_nits:.0}"));
                                        set("hdr-reference-white", &format!("{white:.0}"));
                                    } else {
                                        set("target-trc", &sdr_trc);
                                        set("target-prim", "bt.709");
                                        set("target-peak", "auto");
                                        set("hdr-reference-white", "auto");
                                    }
                                    eprintln!(
                                        "[frameplayer mpv] layer target: {} (display hdr {}, SDR white {:?} nits, peak {:?} nits{})",
                                        if hdr { format!("HDR10, SDR white {white:.0}, peak {peak_nits:.0}") } else { format!("8-bit sRGB, target-trc {sdr_trc}") },
                                        d.hdr, d.sdr_white_nits, d.peak_nits, if told.is_some() { "; from on_display_change" } else { "" }
                                    );
                                    layer_set = key;
                                }
                            }
                        }
                        if !layer_route && (peak - peak_set).abs() > 0.005 * peak_set {
                            // An integer option: "203.0" is refused.
                            set("target-peak", &format!("{peak:.0}"));
                            peak_set = peak;
                        }
                        if let (true, Some(hz)) = (resample(), info.display.refresh_hz) {
                            let hz = hz as f64;
                            if (hz - hz_set).abs() > 0.0005 {
                                set("display-fps-override", &format!("{hz:.3}"));
                                hz_set = hz;
                            }
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
        // FPUI_MUTE: a toggle or a step of the volume must not undo it (the interface toggles
        // with `cycle`, never by writing the property).
        if self.mute && matches!(name, "cycle" | "cycle-values" | "add" | "multiply" | "set") {
            if let Some(Value::String(p)) = args.first() {
                if matches!(p.as_str(), "mute" | "volume" | "ao-mute" | "ao-volume" | "ao") {
                    return Ok(());
                }
            }
        }
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
        let mut v = text(value).unwrap_or_default();
        // Muted whatever the interface asks for (FPUI_MUTE=1): the options alone would not hold,
        // the interface sets both from its saved preferences after a file loads.
        if self.mute && matches!(name, "mute" | "volume" | "ao") {
            v = match name {
                "mute" => "yes",
                "volume" => "0",
                _ => "null",
            }
            .into();
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
            let value = read_value(code, data);
            if code == FORMAT_STRING {
                let p = *(data as *const *mut c_char);
                if !p.is_null() {
                    (api.free)(p.cast());
                }
            }
            Ok(value)
        }
    }

    pub fn destroy(&self) {
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
