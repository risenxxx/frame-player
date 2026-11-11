//! The picture on Linux: libmpv's OpenGL render API into a `GtkGLArea` placed
//! *under* the transparent `WebKitWebView`, both inside one `GtkOverlay`, so
//! GTK composites the two layers client-side — on X11 and on Wayland alike.
//! (`--wid` cannot do this: Wayland has no foreign-window embedding at all,
//! and on X11 mpv's child window would sit over the web view, not under it.)
//!
//! Everything here runs on the GTK main thread, except `update_cb`, which
//! libmpv calls from its own thread and which only schedules a redraw.
//!
//! The widget tree is rebuilt once per window, from
//!   GtkWindow > GtkBox (tao's default vbox) > WebKitWebView
//! into
//!   GtkWindow > GtkOverlay > [GtkGLArea, WebKitWebView (overlay child)]
//! and **the web view's grandparent has to stay the GtkWindow**:
//! tauri-runtime-wry's undecorated-resize handler does
//! `webview.parent().parent().downcast::<gtk::Window>().unwrap()` on every
//! button press, so an Overlay inside the vbox aborts the app on the first
//! click. The vbox leaves the tree (a GTK menubar packed into it would never
//! show; this app has none on Linux).
//!
//! The order rules (each one cost a run of the prototype — docs/linux.md):
//! signals are connected before the area enters the tree (the window may be
//! mapped already, and then realize happens inside `show_all`); the render
//! context exists before the first `loadfile` (`vo=libmpv` refuses to open
//! without one — `attach` returns only once it does); and it is freed with
//! the area's GL context current, before the core is destroyed.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void, CStr};
use std::rc::Rc;
use std::sync::Arc;

use glib::translate::ToGlibPtr;
use gtk::prelude::*;

use super::ffi::{self, Lib};

const GL_DRAW_FRAMEBUFFER_BINDING: u32 = 0x8CA6;
const GL_COLOR_BUFFER_BIT: u32 = 0x4000;

type ClearColorFn = unsafe extern "C" fn(f32, f32, f32, f32);
type ClearFn = unsafe extern "C" fn(u32);

/// `get_proc_address` for libmpv, through the loader that owns the current
/// GdkGLContext: GTK 3's X11 backend is GLX-only, its Wayland backend EGL.
/// libepoxy (GTK's own GL dispatch) exports no lookup function to use instead.
struct ProcLoader {
    _lib: libloading::Library,
    get: unsafe extern "C" fn(*const c_char) -> *mut c_void,
}

impl ProcLoader {
    fn new(egl: bool) -> Result<Box<ProcLoader>, String> {
        let (soname, sym): (&str, &[u8]) = if egl {
            ("libEGL.so.1", b"eglGetProcAddress\0")
        } else {
            ("libGL.so.1", b"glXGetProcAddressARB\0")
        };
        // SAFETY: the system GL loader, already mapped by GTK in this process.
        let lib = unsafe { libloading::Library::new(soname) }.map_err(|e| format!("{soname}: {e}"))?;
        // SAFETY: both functions are `void *(*)(const char *)`.
        let get = unsafe { lib.get::<unsafe extern "C" fn(*const c_char) -> *mut c_void>(sym) }
            .map(|s| *s)
            .map_err(|e| format!("{soname}: {e}"))?;
        Ok(Box::new(ProcLoader { _lib: lib, get }))
    }

    fn lookup(&self, name: &CStr) -> *mut c_void {
        // SAFETY: a NUL-terminated name; the loader is alive.
        unsafe { (self.get)(name.as_ptr()) }
    }
}

unsafe extern "C" fn get_proc_address(ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    // SAFETY: `ctx` is the boxed ProcLoader the render context was created
    // with, which outlives it; `name` is mpv's NUL-terminated string.
    unsafe { (*(ctx as *const ProcLoader)).lookup(CStr::from_ptr(name)) }
}

/// What libmpv's update callback is handed: a weak, thread-safe reference to
/// the area, so a late callback for a widget already gone does nothing.
struct UpdateCtx {
    area: glib::SendWeakRef<gtk::GLArea>,
}

unsafe extern "C" fn update_cb(ctx: *mut c_void) {
    // SAFETY: `ctx` is the boxed UpdateCtx registered with the callback; it is
    // dropped only after the callback is unregistered. No mpv call here.
    let area = unsafe { &*(ctx as *const UpdateCtx) }.area.clone();
    glib::MainContext::default().invoke(move || {
        if let Some(a) = area.upgrade() {
            a.queue_render();
        }
    });
}

/// The render state of one window's video layer. Lives on the GTK thread.
struct Gl {
    lib: Arc<Lib>,
    /// The core whose picture this area shows; NULL between cores.
    mpv: Cell<*mut ffi::mpv_handle>,
    rctx: Cell<*mut ffi::mpv_render_context>,
    loader: RefCell<Option<Box<ProcLoader>>>,
    update: RefCell<Option<Box<UpdateCtx>>>,
    get_integerv: Cell<Option<unsafe extern "C" fn(u32, *mut c_int)>>,
    /// glClearColor + glClear, to paint black while no core is attached (the
    /// last frame would otherwise stay in the area's buffer).
    clear: Cell<Option<(ClearColorFn, ClearFn)>>,
    error: RefCell<Option<String>>,
    /// `FP_RENDER_STATS`: renders per second and their cost, on stderr.
    stats: Option<RefCell<RenderStats>>,
}

/// What `FP_RENDER_STATS=1` reports every two seconds: how often the area was
/// drawn, what a render cost on the GTK thread, and the FBO's device size —
/// the numbers that tell a slow picture from a slow main loop.
struct RenderStats {
    since: std::time::Instant,
    renders: u32,
    total: std::time::Duration,
    worst: std::time::Duration,
}

impl RenderStats {
    fn new() -> Self {
        Self { since: std::time::Instant::now(), renders: 0, total: Default::default(), worst: Default::default() }
    }

    fn record(&mut self, took: std::time::Duration, w: c_int, h: c_int) {
        self.renders += 1;
        self.total += took;
        self.worst = self.worst.max(took);
        let elapsed = self.since.elapsed();
        if elapsed.as_secs_f64() < 2.0 {
            return;
        }
        eprintln!(
            "[libmpv] {:.1} renders/s, {:.2} ms avg, {:.2} ms worst, FBO {w}x{h}",
            f64::from(self.renders) / elapsed.as_secs_f64(),
            self.total.as_secs_f64() * 1000.0 / f64::from(self.renders),
            self.worst.as_secs_f64() * 1000.0,
        );
        *self = Self::new();
    }
}

impl Gl {
    /// Create the render context for the current core. GL must be current.
    fn create(&self, area: &gtk::GLArea) {
        if self.mpv.get().is_null() || !self.rctx.get().is_null() {
            return;
        }
        if let Err(e) = self.try_create(area) {
            eprintln!("[libmpv] no render context: {e}");
            *self.error.borrow_mut() = Some(e);
        }
    }

    fn try_create(&self, area: &gtk::GLArea) -> Result<(), String> {
        area.make_current();
        if let Some(e) = area.error() {
            return Err(format!("OpenGL is not available: {e}"));
        }
        let ctx = area.context().ok_or("the GL area has no GL context")?;
        let display = ctx.display();
        let display_type = display.as_ref().map(|d| d.type_().name().to_string()).unwrap_or_default();
        let wayland = ctx.type_().name().contains("Wayland") || display_type.contains("Wayland");
        let loader = ProcLoader::new(wayland)?;
        let giv = loader.lookup(c"glGetIntegerv");
        if giv.is_null() {
            return Err("glGetIntegerv not found".into());
        }
        // SAFETY: glGetIntegerv's prototype is void(GLenum, GLint *).
        self.get_integerv.set(Some(unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn(u32, *mut c_int)>(giv) }));
        let (cc, cl) = (loader.lookup(c"glClearColor"), loader.lookup(c"glClear"));
        if !cc.is_null() && !cl.is_null() {
            // SAFETY: the GL prototypes of the two names, resolved by the GL
            // loader GTK itself uses, which stays mapped for the process.
            self.clear.set(Some(unsafe {
                (std::mem::transmute::<*mut c_void, ClearColorFn>(cc), std::mem::transmute::<*mut c_void, ClearFn>(cl))
            }));
        }
        self.driver_options(&loader);

        // The X11 display, for hardware-decoding interop (VA-API, VDPAU over
        // GLX). Not the Wayland one: given a `wl_display`, mpv's VA-API
        // interop calls vaInitialize on it while the render context is being
        // created, and Ubuntu 24.04's libva-wayland crashed the process there
        // (SIGSEGV inside libva-wayland under a compositor that offers no
        // wl_drm — a headless Weston; some real compositor and driver pairs
        // offer none either). Without it Wayland decodes in hardware through
        // the copy path (`vaapi-copy`, its own DRM device) or in software, and
        // starting up does not depend on the compositor.
        let native_display = if wayland { None } else { display.as_ref().and_then(x11_display) };

        let mut init = ffi::mpv_opengl_init_params {
            get_proc_address: Some(get_proc_address),
            get_proc_address_ctx: &*loader as *const ProcLoader as *mut c_void,
        };
        // Advanced control: frame timing and `mpv_render_context_update`
        // (with BLOCK_FOR_TARGET_TIME off in `render`).
        let mut advanced: c_int = 1;
        let mut params = vec![
            ffi::mpv_render_param { type_: ffi::MPV_RENDER_PARAM_API_TYPE, data: c"opengl".as_ptr() as *mut c_void },
            ffi::mpv_render_param {
                type_: ffi::MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: &mut init as *mut _ as *mut c_void,
            },
            ffi::mpv_render_param {
                type_: ffi::MPV_RENDER_PARAM_ADVANCED_CONTROL,
                data: &mut advanced as *mut _ as *mut c_void,
            },
        ];
        if let Some(p) = native_display {
            params.push(ffi::mpv_render_param { type_: ffi::MPV_RENDER_PARAM_X11_DISPLAY, data: p });
        }
        params.push(ffi::mpv_render_param { type_: ffi::MPV_RENDER_PARAM_INVALID, data: std::ptr::null_mut() });

        let mut rctx: *mut ffi::mpv_render_context = std::ptr::null_mut();
        // SAFETY: GL is current; the core is live; every param points at a
        // local alive for the call; the loader box outlives the context.
        let r = unsafe { (self.lib.render_context_create)(&mut rctx, self.mpv.get(), params.as_mut_ptr()) };
        if r < 0 {
            return Err(format!("mpv_render_context_create: {}", self.lib.error_text(r)));
        }
        let update = Box::new(UpdateCtx { area: area.downgrade().into() });
        // SAFETY: `rctx` was just created; the box is kept until the callback
        // is unregistered in `free`.
        unsafe {
            (self.lib.render_context_set_update_callback)(rctx, Some(update_cb), &*update as *const UpdateCtx as *mut c_void)
        };
        self.rctx.set(rctx);
        *self.loader.borrow_mut() = Some(loader);
        *self.update.borrow_mut() = Some(update);
        *self.error.borrow_mut() = None;
        eprintln!("[libmpv] render context created ({})", if wayland { "EGL" } else { "GLX" });
        Ok(())
    }

    /// What two GL drivers need from mpv, set before the render context exists.
    ///
    /// **A software rasteriser draws mpv's intermediate FBOs in 8 bits.** mpv
    /// renders through an `rgba16f` framebuffer by default, and on llvmpipe
    /// that came out black for a whole run in 9 starts of 26 — every render,
    /// read back from the area's own FBO, with mpv's state otherwise identical
    /// to a good run's. With `fbo-format=rgba8`, 0 of 16. A machine with no GPU
    /// driver (a virtual machine, a broken driver install) gets exactly this
    /// renderer, and a picture with less precision beats none. An mpv.conf
    /// `fbo-format` loses to this only on a software renderer.
    ///
    /// **NVK decodes into mapped buffers** (`vd-lavc-dr=yes`). Its texture
    /// upload lays every frame out in tiles on the CPU, inside the render on
    /// the GTK thread: a software-decoded 4K 10-bit frame cost 20 ms there, and
    /// a 60 fps file ran at 30 with half its frames dropped (RTX 5080, Mesa
    /// 26.0, zink). With direct rendering the decoder writes where the GPU
    /// reads: 0.3 ms, 60 renders a second, nothing dropped. Only while the
    /// option is mpv's `auto`, so an mpv.conf line still wins.
    fn driver_options(&self, loader: &ProcLoader) {
        const GL_RENDERER: u32 = 0x1F01;
        let f = loader.lookup(c"glGetString");
        if f.is_null() {
            return;
        }
        // SAFETY: glGetString's prototype is const GLubyte *(GLenum); GL is
        // current (make_current above) and the string is static.
        let renderer = unsafe {
            let get = std::mem::transmute::<*mut c_void, unsafe extern "C" fn(u32) -> *const c_char>(f);
            ffi::cstr_lossy(get(GL_RENDERER))
        };
        if renderer.contains("llvmpipe") || renderer.contains("softpipe") {
            // SAFETY: live core handle, static C strings.
            let r = unsafe { (self.lib.set_property_string)(self.mpv.get(), c"fbo-format".as_ptr(), c"rgba8".as_ptr()) };
            eprintln!("[libmpv] software renderer ({renderer}): fbo-format=rgba8 ({})", if r < 0 { self.lib.error_text(r) } else { "set".into() });
        }
        if renderer.contains("NVK") && self.string_property(c"vd-lavc-dr").as_deref() == Some("auto") {
            // SAFETY: live core handle, static C strings.
            let r = unsafe { (self.lib.set_property_string)(self.mpv.get(), c"vd-lavc-dr".as_ptr(), c"yes".as_ptr()) };
            eprintln!("[libmpv] NVK ({renderer}): vd-lavc-dr=yes ({})", if r < 0 { self.lib.error_text(r) } else { "set".into() });
        }
    }

    /// A property of the current core as a string, `None` if mpv refused it.
    fn string_property(&self, name: &CStr) -> Option<String> {
        let mut out: *mut c_char = std::ptr::null_mut();
        // SAFETY: live core handle; STRING writes a char* freed with mpv_free.
        let r = unsafe {
            (self.lib.get_property)(self.mpv.get(), name.as_ptr(), ffi::MPV_FORMAT_STRING, &mut out as *mut _ as *mut c_void)
        };
        if r < 0 || out.is_null() {
            return None;
        }
        // SAFETY: a NUL-terminated string mpv allocated for us.
        let value = unsafe { ffi::cstr_lossy(out) };
        // SAFETY: allocated by mpv, freed once.
        unsafe { (self.lib.free)(out as *mut c_void) };
        Some(value)
    }

    /// Free the render context. GL must be current (or the area unrealized
    /// already, in which case there is nothing left to free).
    fn free(&self) {
        let rctx = self.rctx.replace(std::ptr::null_mut());
        if rctx.is_null() {
            return;
        }
        // SAFETY: a context we created and nobody else frees; the callback is
        // unregistered first so its box may go.
        unsafe {
            (self.lib.render_context_set_update_callback)(rctx, None, std::ptr::null_mut());
            (self.lib.render_context_free)(rctx);
        }
        self.update.borrow_mut().take();
        self.loader.borrow_mut().take();
        eprintln!("[libmpv] render context freed");
    }

    fn render(&self, area: &gtk::GLArea) {
        let rctx = self.rctx.get();
        if rctx.is_null() {
            if let Some((color, clear)) = self.clear.get() {
                // SAFETY: GL is current inside the render signal.
                unsafe {
                    color(0.0, 0.0, 0.0, 1.0);
                    clear(GL_COLOR_BUFFER_BIT);
                }
            }
            return;
        }
        let Some(giv) = self.get_integerv.get() else { return };
        let mut fbo: c_int = 0;
        // SAFETY: GL is current inside the render signal.
        unsafe { giv(GL_DRAW_FRAMEBUFFER_BINDING, &mut fbo) };
        // Device pixels: GTK's FBO is the allocation times the scale.
        let scale = area.scale_factor();
        let alloc = area.allocation();
        let mut target = ffi::mpv_opengl_fbo {
            fbo,
            w: alloc.width() * scale,
            h: alloc.height() * scale,
            internal_format: 0,
        };
        // GTK treats the area's FBO as y-up.
        let mut flip: c_int = 1;
        // Never sleep the GTK thread until the frame's target time: on a
        // starved main loop that wait made every render come out black.
        let mut block: c_int = 0;
        let mut params = [
            ffi::mpv_render_param { type_: ffi::MPV_RENDER_PARAM_OPENGL_FBO, data: &mut target as *mut _ as *mut c_void },
            ffi::mpv_render_param { type_: ffi::MPV_RENDER_PARAM_FLIP_Y, data: &mut flip as *mut _ as *mut c_void },
            ffi::mpv_render_param {
                type_: ffi::MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME,
                data: &mut block as *mut _ as *mut c_void,
            },
            ffi::mpv_render_param { type_: ffi::MPV_RENDER_PARAM_INVALID, data: std::ptr::null_mut() },
        ];
        let started = self.stats.as_ref().map(|_| std::time::Instant::now());
        // SAFETY: GL current, live context, params alive for the calls.
        unsafe {
            (self.lib.render_context_update)(rctx);
            (self.lib.render_context_render)(rctx, params.as_mut_ptr());
            // GTK composites the FBO once this handler returns; the closest
            // honest "swap" there is.
            (self.lib.render_context_report_swap)(rctx);
        }
        if let (Some(stats), Some(started)) = (&self.stats, started) {
            stats.borrow_mut().record(started.elapsed(), target.w, target.h);
        }
    }
}

/// The `Display *` of an X11 GdkDisplay.
fn x11_display(display: &gdk::Display) -> Option<*mut c_void> {
    let this = libloading::os::unix::Library::this();
    // SAFETY: GDK's own `Display *(GdkDisplay *)`, linked into this process;
    // the caller asks only for an X11 display.
    let f = unsafe {
        this.get::<unsafe extern "C" fn(*mut gdk::ffi::GdkDisplay) -> *mut c_void>(b"gdk_x11_display_get_xdisplay\0")
    }
    .ok()?;
    let ptr: *mut gdk::ffi::GdkDisplay = display.to_glib_none().0;
    // SAFETY: as above; the display outlives the process's windows.
    let p = unsafe { f(ptr) };
    (!p.is_null()).then_some(p)
}

struct Layer {
    area: gtk::GLArea,
    gl: Rc<Gl>,
}

thread_local! {
    /// One video layer per window label, on the GTK thread only.
    static LAYERS: RefCell<HashMap<String, Layer>> = RefCell::new(HashMap::new());
}

/// Build the overlay around `webview` (once per window) and wire the area.
fn install(label: &str, webview: &webkit2gtk::WebView, lib: Arc<Lib>) -> Result<Layer, String> {
    let vbox = webview.parent().ok_or("the web view has no parent")?;
    let window = vbox.parent().ok_or("the web view's parent has no parent")?;
    let window: gtk::Window = window
        .downcast()
        .map_err(|w| format!("the web view's grandparent is a {}, not a window", w.type_().name()))?;
    let vbox: gtk::Container = vbox.downcast().map_err(|_| "the web view's parent is not a container")?;

    let gl = Rc::new(Gl {
        lib,
        mpv: Cell::new(std::ptr::null_mut()),
        rctx: Cell::new(std::ptr::null_mut()),
        loader: RefCell::new(None),
        update: RefCell::new(None),
        get_integerv: Cell::new(None),
        clear: Cell::new(None),
        error: RefCell::new(None),
        stats: std::env::var_os("FP_RENDER_STATS").map(|_| RefCell::new(RenderStats::new())),
    });

    let area = gtk::GLArea::new();
    area.set_has_alpha(false);
    area.set_has_depth_buffer(false);
    area.set_has_stencil_buffer(false);
    // Drawn when mpv has a frame (and on resize): a paused video costs nothing.
    area.set_auto_render(false);
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.set_can_focus(false);

    // Connected before the area enters the tree — see the module comment.
    let g = gl.clone();
    area.connect_realize(move |area| g.create(area));
    let g = gl.clone();
    area.connect_render(move |area, _| {
        g.render(area);
        glib::Propagation::Stop
    });
    let g = gl.clone();
    // RUN_LAST: this runs while the GL context still exists.
    area.connect_unrealize(move |area| {
        area.make_current();
        g.free();
    });
    let owned = label.to_string();
    area.connect_destroy(move |_| {
        LAYERS.with(|l| l.borrow_mut().remove(&owned));
    });

    let overlay = gtk::Overlay::new();
    overlay.add(&area);
    vbox.remove(webview);
    overlay.add_overlay(webview);
    overlay.set_overlay_pass_through(webview, false);
    // The overlay draws its two children itself, the web view through
    // `gtk_widget_draw`. GTK 3 painting with GL marks every widget drawn over
    // GL content that was already flushed (the area's blit) as needing a blend
    // (`gdk_window_mark_paint_from_clip`), and a blend region is paid for by
    // uploading the window's cairo surface into a texture — twice a frame, the
    // whole window, since the web view covers it. A cairo context marked for
    // drawing, which is what `gtk_widget_draw` hands down, skips the marking;
    // WebKit's texture is still blended over the video by GL. Measured on a
    // 4K screen at 150 % (a 5120×2880 buffer) on NVK, where the upload is a
    // CPU tiling copy: 13 renders a second with the controls up, against the
    // video's 30.
    let (a, w) = (area.clone(), webview.clone());
    overlay.connect_draw(move |overlay, cr| {
        overlay.propagate_draw(&a, cr);
        if w.is_drawable() {
            let (x, y) = w.translate_coordinates(overlay, 0, 0).unwrap_or((0, 0));
            cr.save().ok();
            cr.translate(f64::from(x), f64::from(y));
            w.draw(cr);
            cr.restore().ok();
        }
        glib::Propagation::Stop
    });
    window.remove(&vbox);
    window.add(&overlay);
    overlay.show_all();
    // Taking the web view out of the tree unset it as the window's focus
    // widget, and nothing gives it back until a click: every hotkey was dead
    // from launch until the viewer clicked into the window (measured — Space
    // reached nothing until the first click on the seekbar).
    webview.grab_focus();
    Ok(Layer { area, gl })
}

/// The window's layer, built now if it does not exist yet.
fn layer(label: &str, webview: &webkit2gtk::WebView, lib: Arc<Lib>) -> Result<(gtk::GLArea, Rc<Gl>), String> {
    if let Some(pair) = LAYERS.with(|l| l.borrow().get(label).map(|x| (x.area.clone(), x.gl.clone()))) {
        return Ok(pair);
    }
    let layer = install(label, webview, lib)?;
    let pair = (layer.area.clone(), layer.gl.clone());
    LAYERS.with(|l| l.borrow_mut().insert(label.to_string(), layer));
    Ok(pair)
}

/// Build the window's overlay ahead of mpv, from the code that creates the
/// window, before its page starts loading — the order the prototype proved.
/// Rebuilding the tree later takes the web view out of the window under a
/// page that is already drawing, which WebKit survives by tearing down and
/// rebuilding its surface; nothing needs it to, so nothing asks it to. `attach`
/// still builds the overlay itself when this was not called.
pub fn prepare(label: &str, webview: &webkit2gtk::WebView, lib: Arc<Lib>) -> Result<(), String> {
    layer(label, webview, lib).map(|_| ())
}

/// Give the window's video layer to the core `mpv` and create its render
/// context — now, even if the window is not shown yet (the area is realized
/// on demand). Returns only once the context exists, because the frontend
/// calls `loadfile` as soon as `init` resolves.
pub fn attach(label: &str, webview: &webkit2gtk::WebView, lib: Arc<Lib>, mpv: *mut ffi::mpv_handle) -> Result<(), String> {
    // A second core for the same window (mpv re-created after a failed init,
    // say) finds the overlay there and reuses it, never nests another.
    let (area, gl) = layer(label, webview, lib)?;
    gl.free();
    gl.mpv.set(mpv);
    *gl.error.borrow_mut() = None;
    if area.is_realized() {
        gl.create(&area);
    } else {
        // Realizing an unmapped widget realizes its ancestors and creates the
        // GL context without showing anything; the handler does the rest.
        area.realize();
    }
    if gl.rctx.get().is_null() {
        gl.mpv.set(std::ptr::null_mut());
        let why = gl.error.borrow_mut().take().unwrap_or_else(|| "the video area could not be realized".into());
        return Err(why);
    }
    area.queue_render();
    Ok(())
}

/// Take the window's video layer away from its core: the render context is
/// freed with GL current. Must run before the core is destroyed.
pub fn detach(label: &str) {
    LAYERS.with(|l| {
        if let Some(layer) = l.borrow().get(label) {
            if layer.area.is_realized() {
                layer.area.make_current();
            }
            layer.gl.free();
            layer.gl.mpv.set(std::ptr::null_mut());
            // Paint black: the last frame stays in GTK's buffer otherwise.
            layer.area.queue_render();
        }
    });
}
