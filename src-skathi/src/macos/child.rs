//! The same layer, now placed by Skathi: `<sk-layer name="video">` and the
//! `skathi::Layer` trait (attach: add our view below the page's view in its superview; place: the
//! element's box from the paint that draws it). HDR headroom from `Builder::on_display_change`.
//!
//! mpv's picture in a CAMetalLayer of our own under Skathi's
//! view, not in `<sk-surface>`. The layer is a layer-hosting NSView positioned NSWindowBelow
//! Skathi's GPUIView, autoresizing with the content view (the player's video element is the
//! whole window). The render thread (mpv.rs `render_thread_child`) imports each drawable's
//! IOSurface into Vulkan as mpv's target (`FPUI_CHILD_COPY=1`: one slot of our own, copied into
//! the drawable instead), and a Metal command buffer waits for mpv's release value on the slot's
//! MTLSharedEvent and presents the drawable. Skathi's window stays 8-bit SDR whatever plays.
//!
//! SDR files: BGRA8Unorm, sRGB, no EDR. HDR files (the control thread's `hdr`): BGR10A2Unorm tagged
//! BT.2100 PQ with HDR10 EDR metadata, macOS mapping it to the headroom.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{class, msg_send};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGColor, CGColorSpace};
use objc2_metal::{MTLCommandBuffer, MTLCommandQueue, MTLCreateSystemDefaultDevice, MTLDevice, MTLPixelFormat};
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer, CATransaction};
use raw_window_handle::RawWindowHandle;

pub struct Child {
    pub layer: Retained<CAMetalLayer>,
    pub queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    /// The layer-hosting NSView (retained by us; moved between superviews by `attach`).
    view: usize,
}
unsafe impl Send for Child {}
unsafe impl Sync for Child {}

static CHILD: OnceLock<Child> = OnceLock::new();
/// The headroom mpv's target follows (f64 bits), from `on_display_change`.
static HEADROOM: AtomicU64 = AtomicU64::new(0x3ff0_0000_0000_0000); // 1.0
/// The drawable size the last placement calls for, in device pixels (w << 32 | h).
static WANT: AtomicU64 = AtomicU64::new(0);
static T0: OnceLock<Instant> = OnceLock::new();
static ATTACHES: AtomicU32 = AtomicU32::new(0);
static PLACES: AtomicU32 = AtomicU32::new(0);
static HIDES: AtomicU32 = AtomicU32::new(0);
/// FPUI_FS_ANIM: a fullscreen transition is under way and the drawable keeps its size.
static FROZEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The size the placements asked for while frozen.
static HELD: AtomicU64 = AtomicU64::new(0);

/// FPUI_FS_ANIM: hold mpv's drawable at its size and let Core Animation scale the picture, keeping
/// its aspect (black around it), until `thaw`. Main thread.
pub fn freeze(fill: bool) {
    let Some(c) = CHILD.get() else { return };
    HELD.store(WANT.load(Ordering::Relaxed), Ordering::Relaxed);
    FROZEN.store(true, Ordering::Relaxed);
    unsafe {
        // Fit on the way in; fill on the way out, as mpv (`layerContentsPlacement`): the held drawable is the
        // fullscreen one, letterbox bars and all, and fitting it into the window kept those bars (a picture
        // 572 pt tall in a 640 pt window, then a snap, measured); filling crops them.
        let gravity = if fill { c"resizeAspectFill" } else { c"resizeAspect" };
        let g: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: gravity.as_ptr()];
        let _: () = msg_send![&*c.layer, setContentsGravity: g];
        // Skathi places the layer again only after the transition, so AppKit's live half of its
        // cross-fade showed the picture at its old box in a corner of the full screen (measured). The video element is the whole window: let AppKit size the view with it.
        let v = c.view as *mut AnyObject;
        let sup: *mut AnyObject = msg_send![v, superview];
        if !sup.is_null() {
            let b: CGRect = msg_send![sup, bounds];
            let _: () = msg_send![v, setFrame: b];
            let _: () = msg_send![v, setAutoresizingMask: 2usize | 16usize]; // width | height sizable
            set_page_hidden(sup, v, true);
        }
    }
    eprintln!("[fpui fs] t={:.0}ms freeze at {:?}", ms(), wanted_size(&c.layer));
}

/// The page's views (Skathi's `GPUI…` siblings of the video's view) hidden for a fullscreen transition, and
/// the video layer's own background black meanwhile, for the bars. Hidden, not merely transparent: a
/// transparent page still presents a frame per display frame, and the layer's animation stalled behind
/// them (an enter held for 250 ms, measured). But AppKit takes the first responder from a view it hides
/// and does not give it back: every key after the first transition (F, Esc) reached the window, not the
/// page — no responder, a beep, the page's state out of step (measured). So the
/// first responder is handed back to the page at once, and again when it is shown. Main thread.
unsafe fn set_page_hidden(sup: *mut AnyObject, video: *mut AnyObject, hidden: bool) {
    let subs: *mut AnyObject = msg_send![sup, subviews];
    let n: usize = msg_send![subs, count];
    for i in 0..n {
        let s: *mut AnyObject = msg_send![subs, objectAtIndex: i];
        if s == video {
            continue;
        }
        let cls: *const AnyObject = msg_send![s, class];
        let name: *mut AnyObject = msg_send![cls, description];
        let c: *const std::ffi::c_char = msg_send![name, UTF8String];
        if c.is_null() || !std::ffi::CStr::from_ptr(c).to_bytes().starts_with(b"GPUI") {
            continue;
        }
        let window: *mut AnyObject = msg_send![s, window];
        let was: *mut AnyObject = if window.is_null() { std::ptr::null_mut() } else { msg_send![window, firstResponder] };
        let _: () = msg_send![s, setHidden: hidden];
        if !window.is_null() {
            let now: *mut AnyObject = msg_send![window, firstResponder];
            // The page had it (or nobody has it now): the page gets it back.
            if now != was || now == window {
                let is_page = was == s || now == window;
                if is_page {
                    let _: bool = msg_send![window, makeFirstResponder: s];
                }
            }
        }
    }
    let layer: *mut AnyObject = msg_send![video, layer];
    if hidden {
        let black = CGColor::new_generic_gray(0.0, 1.0);
        let _: () = msg_send![layer, setBackgroundColor: &*black as *const CGColor];
    } else {
        let _: () = msg_send![layer, setBackgroundColor: std::ptr::null::<CGColor>()];
    }
}

/// While held, the view to its superview's bounds now (the custom fullscreen animation's steps:
/// autoresizing did not reach it in time). Main thread.
pub fn follow() {
    let Some(c) = CHILD.get() else { return };
    if !FROZEN.load(Ordering::Relaxed) {
        return;
    }
    unsafe {
        let v = c.view as *mut AnyObject;
        let sup: *mut AnyObject = msg_send![v, superview];
        if sup.is_null() {
            return;
        }
        // The window's size, not the superview's: Skathi resizes that one a frame or more later, and
        // the picture trailed the growing window by a black strip (measured).
        let window: *mut AnyObject = msg_send![v, window];
        let content: *mut AnyObject = if window.is_null() { sup } else { msg_send![window, contentView] };
        let cb: CGRect = msg_send![content, bounds];
        let b = CGRect::new(CGPoint::new(0.0, 0.0), cb.size);
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        let _: () = msg_send![v, setFrame: b];
        CATransaction::commit();
    }
}

/// The one real resize at the end of the transition.
pub fn thaw() {
    let Some(c) = CHILD.get() else { return };
    if !FROZEN.swap(false, Ordering::Relaxed) {
        return;
    }
    unsafe {
        let g: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: c"resize".as_ptr()];
        let _: () = msg_send![&*c.layer, setContentsGravity: g];
        // Skathi's placement owns the frame again.
        let v = c.view as *mut AnyObject;
        let _: () = msg_send![v, setAutoresizingMask: 0usize];
        let sup: *mut AnyObject = msg_send![v, superview];
        if !sup.is_null() {
            set_page_hidden(sup, v, false);
        }
    }
    let want = HELD.load(Ordering::Relaxed);
    if WANT.swap(want, Ordering::Relaxed) != want {
        if let Some(k) = KICK.get() {
            k();
        }
    }
    eprintln!("[fpui fs] t={:.0}ms thaw to {:?}", ms(), wanted_size(&c.layer));
}

/// Quitting (quit.rs): the picture goes with the window, whatever mpv still draws. Main thread.
pub fn hide() {
    let Some(c) = CHILD.get() else { return };
    unsafe {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        let _: () = msg_send![c.view as *mut AnyObject, setHidden: true];
        CATransaction::commit();
    }
}

/// Called after a placement changes the size: the render thread draws again at once.
pub static KICK: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

pub fn get() -> Option<&'static Child> {
    CHILD.get()
}

pub fn headroom() -> f64 {
    f64::from_bits(HEADROOM.load(Ordering::Relaxed))
}

pub fn ms() -> f64 {
    T0.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

/// `Builder::on_display_change` (UI thread). The layer decides HDR by `hdr` and
/// `potential_headroom`, as skathi's docs say (`headroom` stays 1.0 until EDR is on screen).
pub fn display_changed(window: u64, d: skathi::SurfaceDisplay) {
    let h = if d.hdr { (d.potential_headroom.max(d.headroom) as f64).max(1.0) } else { 1.0 };
    HEADROOM.store(h.to_bits(), Ordering::Relaxed);
    eprintln!(
        "[fpui display] t={:.0}ms window {window}: known {} hdr {} headroom {:.2} potential {:.2} output {:.2}/{} sdr_white {:?} peak {:?} refresh {:?} -> mpv headroom {h:.2}",
        ms(), d.known, d.hdr, d.headroom, d.potential_headroom, d.output_headroom, d.output_hdr, d.sdr_white_nits, d.peak_nits, d.refresh_hz
    );
}

/// The layer under `<sk-layer name="video">`. One element, one mpv picture: every instance drives
/// the one CAMetalLayer.
pub struct VideoLayer;

impl skathi::Layer for VideoLayer {
    fn attach(&mut self, host: &skathi::LayerHost) {
        let n = ATTACHES.fetch_add(1, Ordering::Relaxed) + 1;
        let Some(RawWindowHandle::AppKit(a)) = host.handle else {
            eprintln!("[fpui layer] t={:.0}ms attach #{n}: window {} without an AppKit handle", ms(), host.window);
            return;
        };
        let skview = a.ns_view.as_ptr() as *mut AnyObject;
        unsafe {
            let superview: *mut AnyObject = msg_send![skview, superview];
            let window: *mut AnyObject = msg_send![skview, window];
            if superview.is_null() {
                eprintln!("[fpui layer] t={:.0}ms attach #{n}: the page's view has no superview", ms());
                return;
            }
            let c = match CHILD.get() {
                Some(c) => c,
                None => match make(window) {
                    Ok(c) => {
                        let _ = CHILD.set(c);
                        CHILD.get().unwrap()
                    }
                    Err(e) => {
                        eprintln!("[fpui layer] attach #{n}: {e}");
                        return;
                    }
                },
            };
            let v = c.view as *mut AnyObject;
            let old: *mut AnyObject = msg_send![v, superview];
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            if !old.is_null() {
                let _: () = msg_send![v, removeFromSuperview];
            }
            let _: () = msg_send![superview, addSubview: v, positioned: -1isize, relativeTo: skview];
            let _: () = msg_send![v, setHidden: true];
            if !window.is_null() {
                let scale: f64 = msg_send![window, backingScaleFactor];
                c.layer.setContentsScale(scale);
            }
            CATransaction::commit();
            let flipped: bool = msg_send![superview, isFlipped];
            let sklayer: *mut AnyObject = msg_send![skview, layer];
            let skmax: usize = if sklayer.is_null() { 0 } else { msg_send![sklayer, maximumDrawableCount] };
            eprintln!("[fpui layer] page view's layer maximumDrawableCount {skmax}");
            let wcls: *const AnyObject = if window.is_null() { std::ptr::null() } else { msg_send![window, class] };
            let wname = if wcls.is_null() { "?".to_string() } else {
                let s: *mut AnyObject = msg_send![window, className];
                let p: *const std::ffi::c_char = msg_send![s, UTF8String];
                std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
            };
            eprintln!(
                "[fpui layer] t={:.0}ms attach #{n}: window {} ({wname} {:p}), page view {:p}, superview {:p} flipped {flipped}, moved from {:p}",
                ms(), host.window, window, skview, superview, old
            );
        }
    }

    fn place(&mut self, placement: Option<skathi::LayerPlacement>) {
        let Some(c) = CHILD.get() else { return };
        let v = c.view as *mut AnyObject;
        let Some(p) = placement else {
            let n = HIDES.fetch_add(1, Ordering::Relaxed) + 1;
            unsafe {
                CATransaction::begin();
                CATransaction::setDisableActions(true);
                let _: () = msg_send![v, setHidden: true];
                CATransaction::commit();
            }
            eprintln!("[fpui layer] t={:.0}ms hide #{n}", ms());
            return;
        };
        let n = PLACES.fetch_add(1, Ordering::Relaxed) + 1;
        // Through a fullscreen transition the view follows the window (`freeze`/`follow`), not
        // Skathi's layout, which lags it: only the size is kept for `thaw`.
        if FROZEN.load(Ordering::Relaxed) {
            let (pw, ph) = (p.bounds[2].round().max(0.0) as u64, p.bounds[3].round().max(0.0) as u64);
            HELD.store(pw << 32 | ph, Ordering::Relaxed);
            return;
        }
        unsafe {
            let sup: *mut AnyObject = msg_send![v, superview];
            if sup.is_null() {
                return;
            }
            let sb: CGRect = msg_send![sup, bounds];
            let flipped: bool = msg_send![sup, isFlipped];
            let s = p.scale.max(0.01) as f64;
            let [x, y, w, h] = p.bounds.map(|v| v as f64 / s);
            let fy = if flipped { y } else { sb.size.height - y - h };
            let frame = CGRect::new(CGPoint::new(x, fy), CGSize::new(w.max(0.0), h.max(0.0)));
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            let _: () = msg_send![v, setFrame: frame];
            let _: () = msg_send![v, setHidden: false];
            c.layer.setContentsScale(s);
            c.layer.setOpacity(p.opacity);
            let r = p.radii[0] as f64 / s;
            c.layer.setCornerRadius(r);
            c.layer.setMasksToBounds(r > 0.0);
            CATransaction::commit();
        }
        let (pw, ph) = (p.bounds[2].round().max(0.0) as u64, p.bounds[3].round().max(0.0) as u64);
        let want = pw << 32 | ph;
        let resized = WANT.swap(want, Ordering::Relaxed) != want;
        if resized {
            if let Some(k) = KICK.get() {
                k();
            }
        }
        if n <= 3 {
            eprintln!(
                "[fpui layer] t={:.0}ms place #{n}: bounds {:?} clip {:?} radii {:?} scale {} opacity {}",
                ms(), p.bounds, p.clip, p.radii, p.scale, p.opacity
            );
        }
    }
}

/// Main thread: the layer and its hosting view, made at the first attach.
unsafe fn make(window: *mut AnyObject) -> Result<Child, String> {
    let device = MTLCreateSystemDefaultDevice().ok_or("no Metal device")?;
    let layer = CAMetalLayer::new();
    layer.setDevice(Some(&device));
    layer.setOpaque(true);
    if !window.is_null() {
        let scale: f64 = msg_send![window, backingScaleFactor];
        layer.setContentsScale(scale);
    }
    // 2, was 3. mpv renders its first frames back to back at playback start, and with 3 allowed Core
    // Animation grew the pool to 3 (sometimes 4) screen-sized drawables, which it then kept for the layer's life
    // (26 MiB each at 3456x1994). At 2: 2 in every
    // measured run, 1080p and PQ, cadence unchanged. FPUI_CHILD_DRAWABLES=3 gives the old pool back.
    layer.setMaximumDrawableCount(std::env::var("FPUI_CHILD_DRAWABLES").ok().and_then(|v| v.parse().ok()).unwrap_or(2));
    layer.setAllowsNextDrawableTimeout(true);
    configure(&layer, false);
    // Drawables MoltenVK can import, and the copy route can blit into: set before the first drawable
    // exists (set later through the main queue, the first drawables were still framebuffer-only).
    // A framebuffer-only drawable's IOSurface is '&BGA' / '&RhA' (lossless-compressed), which MoltenVK
    // 1.4.2's VkImportMetalIOSurfaceInfoEXT refuses with VK_ERROR_INITIALIZATION_FAILED on every
    // frame; without framebufferOnly it is 'BGRA' / 'RGhA' and imports (measured, both formats).
    layer.setFramebufferOnly(false);
    let black = CGColor::new_srgb(0.0, 0.0, 0.0, 1.0);
    layer.setBackgroundColor(Some(&black));
    let v: *mut AnyObject = msg_send![class!(NSView), alloc];
    let v: *mut AnyObject = msg_send![v, initWithFrame: CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(1.0, 1.0))];
    let _: () = msg_send![v, setLayer: &*layer];
    let _: () = msg_send![v, setWantsLayer: true];
    let queue = device.newCommandQueue().ok_or("no command queue")?;
    Ok(Child { layer, queue, view: v as usize })
}

/// SDR: BGRA8Unorm, sRGB. HDR: BGR10A2Unorm, BT.2100 PQ, EDR.
pub fn configure(layer: &CAMetalLayer, hdr: bool) {
    unsafe {
        if hdr {
            // As bare mpv with `target-colorspace-hint` (MoltenVK's HDR10_ST2084): mpv writes PQ, the
            // layer says so, and macOS maps it to the display's headroom as QuickTime does.
            // 10-bit, as MoltenVK's HDR10_ST2084 swapchain: a half-float drawable tagged PQ came out with
            // the shadows crushed (dark frame median 0.0029 against QuickTime's 0.0216; 10-bit 0.0216,
            // measured).
            layer.setPixelFormat(MTLPixelFormat::BGR10A2Unorm);
            let cs = CGColorSpace::with_name(Some(objc2_core_graphics::kCGColorSpaceITUR_2100_PQ));
            layer.setColorspace(cs.as_deref());
            layer.setWantsExtendedDynamicRangeContent(true);
            // HDR10, 0-10000 nits, 100 nits = 1.0: what bare mpv's layer carries (measured).
            let md: *mut AnyObject = msg_send![class!(CAEDRMetadata), HDR10MetadataWithMinLuminance: 0.0f32,
                maxLuminance: 10000.0f32, opticalOutputScale: 100.0f32];
            let _: () = msg_send![layer, setEDRMetadata: md];
        } else {
            let _: () = msg_send![layer, setEDRMetadata: std::ptr::null_mut::<AnyObject>()];
            layer.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
            let cs = CGColorSpace::with_name(Some(objc2_core_graphics::kCGColorSpaceSRGB));
            layer.setColorspace(cs.as_deref());
            layer.setWantsExtendedDynamicRangeContent(false);
        }
    }
}

/// The drawable size the layer's bounds call for, in pixels.
pub fn wanted_size(_layer: &CAMetalLayer) -> (u32, u32) {
    let w = WANT.load(Ordering::Relaxed);
    ((w >> 32) as u32, (w & 0xffff_ffff) as u32)
}

pub fn set_drawable_size(layer: &CAMetalLayer, size: (u32, u32)) {
    let have = layer.drawableSize();
    if have.width as u32 != size.0 || have.height as u32 != size.1 {
        layer.setDrawableSize(CGSize::new(size.0 as f64, size.1 as f64));
    }
}

/// An IOSurface's pixel format as its four characters ('BGRA', '&BGA', …), for the logs.
pub fn iosurface_format(surface: *mut c_void) -> String {
    extern "C" {
        fn IOSurfaceGetPixelFormat(buffer: *mut c_void) -> u32;
    }
    if surface.is_null() {
        return "none".into();
    }
    let f = unsafe { IOSurfaceGetPixelFormat(surface) };
    f.to_be_bytes().iter().map(|&b| if b.is_ascii_graphic() { b as char } else { '?' }).collect()
}

/// The drawable texture's IOSurface (CoreAnimation's), or null.
pub fn drawable_iosurface(d: &ProtocolObject<dyn CAMetalDrawable>) -> *mut c_void {
    unsafe {
        let tex = d.texture();
        msg_send![&*tex, iosurface]
    }
}

/// GPU: wait for `value` on `event` (mpv's release), then present `drawable`.
pub fn present_after(c: &Child, drawable: &ProtocolObject<dyn CAMetalDrawable>, event: *mut c_void, value: u64) -> bool {
    unsafe {
        let Some(cb) = c.queue.commandBuffer() else { return false };
        let _: () = msg_send![&*cb, encodeWaitForEvent: event as *mut AnyObject, value: value];
        cb.presentDrawable(ProtocolObject::from_ref(drawable));
        cb.commit();
        true
    }
}

/// Copy mode (`FPUI_CHILD_COPY=1`): wait for mpv, blit our slot's texture into the drawable,
/// signal `signal` on the slot's event (the slot is free again), present.
pub fn copy_present(
    c: &Child,
    drawable: &ProtocolObject<dyn CAMetalDrawable>,
    src: &ProtocolObject<dyn objc2_metal::MTLTexture>,
    event: *mut c_void,
    wait: u64,
    signal: u64,
) -> bool {
    use objc2_metal::{MTLBlitCommandEncoder, MTLCommandEncoder};
    unsafe {
        let Some(cb) = c.queue.commandBuffer() else { return false };
        let _: () = msg_send![&*cb, encodeWaitForEvent: event as *mut AnyObject, value: wait];
        let Some(blit) = cb.blitCommandEncoder() else { return false };
        let dst = drawable.texture();
        blit.copyFromTexture_toTexture(src, &dst);
        blit.endEncoding();
        let _: () = msg_send![&*cb, encodeSignalEvent: event as *mut AnyObject, value: signal];
        cb.presentDrawable(ProtocolObject::from_ref(drawable));
        cb.commit();
        true
    }
}

/// Copy mode: a Metal texture on our slot's IOSurface.
pub fn texture_on(c: &Child, iosurface: *mut c_void, w: u32, h: u32, hdr: bool) -> Option<Retained<ProtocolObject<dyn objc2_metal::MTLTexture>>> {
    use objc2_metal::{MTLTextureDescriptor, MTLTextureUsage};
    unsafe {
        let device = c.queue.device();
        let fmt = if hdr { MTLPixelFormat::RGBA16Float } else { MTLPixelFormat::BGRA8Unorm };
        let desc = MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(fmt, w as usize, h as usize, false);
        desc.setUsage(MTLTextureUsage::ShaderRead);
        let t: Option<Retained<ProtocolObject<dyn objc2_metal::MTLTexture>>> =
            msg_send![&*device, newTextureWithDescriptor: &*desc, iosurface: iosurface, plane: 0usize];
        t
    }
}
