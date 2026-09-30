//! Holding the window to the picture's shape while the viewer resizes it.
//!
//! "Match video aspect ratio" used to mean one resize when a file opened, after
//! which any edge could be dragged into a shape the picture does not have — a
//! setting that read as on over a window showing black bars. The shape is now a
//! constraint on the resize itself, and each platform has its own place for
//! one:
//!
//! - **macOS** has it as a window property (`contentAspectRatio`), applied by
//!   AppKit inside its own live resize — see `macos_chrome::set_shape_lock`.
//! - **Windows** has no such property. The sizing loop offers every rectangle
//!   it is about to use in `WM_SIZING`, and whoever answers may rewrite it; tao
//!   does not answer that message at all, so a subclass of our own does.
//!
//! Both resize paths on Windows end in that loop — the HTML edge strips
//! (`startResizeDragging`) and Tauri's invisible border helper each post
//! `WM_NCLBUTTONDOWN` to the main window — which is why one handler covers them.
//!
//! What this deliberately does **not** constrain: maximizing, fullscreen and
//! the system's snap layouts. None of them goes through `WM_SIZING`, and each
//! is a shape the viewer asked for by name.
//!
//! The other half of the file is how the window *gets* to a new shape
//! (`window_frame_glide`): the position and the size as one frame, over time —
//! and the picture going dark and coming back around it (`window_video_fade`).

// ---- Saying that a resize is under way -----------------------------------
//
// The web content is pinned to the window's top-left corner and drawn a frame
// or two behind the window's frame (measured on a bare web view in a window of
// its own: what is anchored to the far edge is 14 to 44 px from where it should
// be, for up to two frames, at every step of a live resize). So for as long as
// an edge is being dragged the interface is somewhere it should not be, and
// when the corner itself moves, the part of it that should be standing still
// is drawn at two places in turn. WebKit cannot be told to pin it elsewhere.
// The frontend takes the bars away for the length of the resize instead
// (`chrome.resizing`), and this is how it hears of one: a live resize runs in
// the system's own loop, and the web view is told about sizes, never about a
// drag beginning or ending.

/// Where the reports go. Set once, by whichever platform half installs itself.
#[cfg(any(windows, target_os = "macos"))]
static FRONTEND: std::sync::OnceLock<tauri::WebviewWindow> = std::sync::OnceLock::new();

/// What the frontend was last told.
#[cfg(any(windows, target_os = "macos"))]
static RESIZING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub const RESIZING_EVENT: &str = "frameplayer://window-resizing";

#[cfg(any(windows, target_os = "macos"))]
pub fn report_to(window: &tauri::WebviewWindow) {
    let _ = FRONTEND.set(window.clone());
}

/// A resize by hand has begun to change the window, or has ended. Said once
/// per change, however many frames the resize sets.
#[cfg(any(windows, target_os = "macos"))]
pub fn say_resizing(on: bool) {
    use tauri::Emitter;

    if RESIZING.swap(on, std::sync::atomic::Ordering::Relaxed) == on {
        return;
    }
    if let Some(window) = FRONTEND.get() {
        let _ = window.emit(RESIZING_EVENT, on);
    }
}

/// A rectangle in screen coordinates, edges as `WM_SIZING` hands them over.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Which edge or corner the viewer has hold of.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
impl Grip {
    fn moves_left(self) -> bool {
        matches!(self, Grip::Left | Grip::TopLeft | Grip::BottomLeft)
    }

    fn moves_top(self) -> bool {
        matches!(self, Grip::Top | Grip::TopLeft | Grip::TopRight)
    }
}

/// Rewrite the rectangle a drag proposes so that the **content** inside it has
/// the shape `ratio` (width over height).
///
/// `frame` is what the window adds around its content on each axis, and `min`
/// is the smallest outer size the window may take; both in the rectangle's own
/// pixels.
///
/// Three decisions, none of them arbitrary:
///
/// - **An edge drives its own axis** and the other one follows. A corner moves
///   both, so it takes whichever of the two asks for the larger window: the
///   corner then stays under the pointer on the axis that moved furthest, where
///   taking the smaller would leave the pointer outside the window it is
///   dragging.
/// - **What is not being dragged does not move.** The opposite edge or corner is
///   the anchor; for a single edge, the axis that merely follows grows from the
///   window's top-left, which is where every other resize of this window grows
///   from.
/// - **The minimum is applied to the shape, not to each axis.** Clamped per
///   axis, a wide picture pulled down to the minimum height keeps shrinking in
///   width and the shape is lost exactly where it is hardest to get back. So the
///   floor is the smallest size *of this shape* that covers both minimums.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
pub fn hold_shape(drag: Bounds, grip: Grip, ratio: f64, frame: (i32, i32), min: (i32, i32)) -> Bounds {
    if !ratio.is_finite() || ratio <= 0.0 {
        return drag;
    }
    let proposed_w = f64::from((drag.right - drag.left - frame.0).max(1));
    let proposed_h = f64::from((drag.bottom - drag.top - frame.1).max(1));

    let mut w = match grip {
        Grip::Left | Grip::Right => proposed_w,
        Grip::Top | Grip::Bottom => proposed_h * ratio,
        _ => proposed_w.max(proposed_h * ratio),
    };

    let min_w = f64::from((min.0 - frame.0).max(1));
    let min_h = f64::from((min.1 - frame.1).max(1));
    w = w.max(min_w).max(min_h * ratio);

    let w = w.round() as i32;
    // Not below the minimum by a rounding: `w` was rounded first, and for a
    // width that came from `min_h * ratio` the division can land a pixel short.
    let h = ((f64::from(w) / ratio).round() as i32).max(min_h as i32);

    let outer_w = w + frame.0;
    let outer_h = h + frame.1;
    let (left, right) = if grip.moves_left() {
        (drag.right - outer_w, drag.right)
    } else {
        (drag.left, drag.left + outer_w)
    };
    let (top, bottom) = if grip.moves_top() {
        (drag.bottom - outer_h, drag.bottom)
    } else {
        (drag.top, drag.top + outer_h)
    };
    Bounds { left, top, right, bottom }
}

/// The shape in force, as the bits of an `f64` ratio; zero for none. Written by
/// the command on a worker thread and read inside the window procedure, hence
/// an atomic rather than anything that could make the sizing loop wait.
#[cfg(windows)]
static RATIO: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Any number of our own: subclasses are keyed by procedure *and* id, so this
/// only has to be the same in `install` and in the removal.
#[cfg(windows)]
const SUBCLASS_ID: usize = 0x4650_5348;

/// Hold the window to `width`:`height` while it is resized by hand, or let it
/// go when either is zero.
///
/// The frontend owns the decision (`syncWindowShape` in window-prefs.svelte.ts)
/// and the minimum size that goes with it; this only tells the platform.
#[tauri::command]
#[cfg_attr(not(any(windows, target_os = "macos")), allow(unused_variables))]
pub fn window_shape_lock(window: tauri::WebviewWindow, width: u32, height: u32) {
    let shape = (width > 0 && height > 0).then_some((f64::from(width), f64::from(height)));
    #[cfg(target_os = "macos")]
    {
        // AppKit is main-thread only, and commands arrive on a worker.
        let win = window.clone();
        let _ = window.run_on_main_thread(move || {
            crate::macos_chrome::set_shape_lock(&win, shape);
        });
    }
    #[cfg(windows)]
    {
        let bits = shape.map_or(0, |(w, h)| (w / h).to_bits());
        RATIO.store(bits, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Put the `WM_SIZING` handler on the window. Called once, from `setup`, which
/// is the thread that owns the window — a subclass cannot be installed from any
/// other.
#[cfg(windows)]
pub fn install(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::UI::Shell::SetWindowSubclass;

    let Ok(hwnd) = window.hwnd() else { return };
    report_to(window);
    let ok = unsafe { SetWindowSubclass(hwnd.0 as _, Some(sizing_proc), SUBCLASS_ID, 0) };
    if ok == 0 {
        eprintln!("[window_shape] could not subclass the window; resizing stays free-form");
    }
}

#[cfg(windows)]
unsafe extern "system" fn sizing_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
    _id: usize,
    _data: usize,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{WM_EXITSIZEMOVE, WM_NCDESTROY, WM_SIZING};

    match msg {
        // The loop that sizes the window also moves it, and says which only by
        // what it sends: a move never sends `WM_SIZING`.
        WM_EXITSIZEMOVE => say_resizing(false),
        WM_SIZING => {
            say_resizing(true);
            let ratio = f64::from_bits(RATIO.load(std::sync::atomic::Ordering::Relaxed));
            let rect = lparam as *mut RECT;
            if ratio > 0.0 && !rect.is_null() {
                if let Some(grip) = grip_of(wparam) {
                    // SAFETY: for WM_SIZING the system passes a RECT it owns for
                    // the duration of the message, and expects it rewritten in
                    // place.
                    let r = unsafe { &mut *rect };
                    let drag = Bounds { left: r.left, top: r.top, right: r.right, bottom: r.bottom };
                    let held = hold_shape(
                        drag,
                        grip,
                        ratio,
                        unsafe { frame_extent(hwnd) },
                        unsafe { min_track(hwnd) },
                    );
                    r.left = held.left;
                    r.top = held.top;
                    r.right = held.right;
                    r.bottom = held.bottom;
                    // TRUE: the message was processed and the rectangle is ours.
                    return 1;
                }
            }
        }
        WM_NCDESTROY => {
            unsafe { RemoveWindowSubclass(hwnd, Some(sizing_proc), SUBCLASS_ID) };
        }
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

#[cfg(windows)]
fn grip_of(wparam: windows_sys::Win32::Foundation::WPARAM) -> Option<Grip> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        WMSZ_BOTTOM, WMSZ_BOTTOMLEFT, WMSZ_BOTTOMRIGHT, WMSZ_LEFT, WMSZ_RIGHT, WMSZ_TOP,
        WMSZ_TOPLEFT, WMSZ_TOPRIGHT,
    };
    Some(match wparam as u32 {
        WMSZ_LEFT => Grip::Left,
        WMSZ_RIGHT => Grip::Right,
        WMSZ_TOP => Grip::Top,
        WMSZ_BOTTOM => Grip::Bottom,
        WMSZ_TOPLEFT => Grip::TopLeft,
        WMSZ_TOPRIGHT => Grip::TopRight,
        WMSZ_BOTTOMLEFT => Grip::BottomLeft,
        WMSZ_BOTTOMRIGHT => Grip::BottomRight,
        _ => return None,
    })
}

/// What the window adds around its content, per axis. Asked of the window
/// rather than assumed to be zero: the window is undecorated, but whether that
/// leaves a frame is tao's decision and has changed between its versions.
#[cfg(windows)]
unsafe fn frame_extent(hwnd: windows_sys::Win32::Foundation::HWND) -> (i32, i32) {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowRect};

    let mut outer = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let mut inner = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    if unsafe { GetWindowRect(hwnd, &mut outer) } == 0 || unsafe { GetClientRect(hwnd, &mut inner) } == 0 {
        return (0, 0);
    }
    (
        ((outer.right - outer.left) - (inner.right - inner.left)).max(0),
        ((outer.bottom - outer.top) - (inner.bottom - inner.top)).max(0),
    )
}

/// The minimum tracking size, from the one place that knows it: tao answers
/// `WM_GETMINMAXINFO` from the constraints the frontend set, the mini player's
/// included. Zero where nothing is set.
#[cfg(windows)]
unsafe fn min_track(hwnd: windows_sys::Win32::Foundation::HWND) -> (i32, i32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageW, MINMAXINFO, WM_GETMINMAXINFO};

    let mut info = MINMAXINFO::default();
    unsafe { SendMessageW(hwnd, WM_GETMINMAXINFO, 0, &mut info as *mut MINMAXINFO as _) };
    (info.ptMinTrackSize.x.max(0), info.ptMinTrackSize.y.max(0))
}

/// A window frame as AppKit has it: the origin is the bottom-left corner and y
/// grows upwards, so the top edge is `y + h`.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UpFrame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Where a frame proposed during a live resize goes, so that the window keeps
/// its **top-left corner** whenever the edge being dragged allows it.
///
/// With a ratio in force AppKit grows the axis that merely follows *around its
/// center*: drag the right edge and the top and the bottom both move (measured:
/// +120 across gave +34 up and −34 down), and the same whether the ratio comes
/// from `contentAspectRatio` or from `windowWillResize:toSize:`. The window's
/// top-left corner is where the web view's content is pinned, and that content
/// is a frame or two behind the window — so with the corner moving, every part
/// of the interface that should be standing still is drawn at two places in
/// turn. A free window never had this for the right and bottom edges; holding
/// the ratio gave it to every grip except the bottom-right corner.
///
/// An axis the viewer has no hold of is one that follows, and it is put on its
/// top or left edge; one that is being dragged is left to the drag. Which is
/// which comes from where the pointer went down (`held_axes`), not from the
/// proposal: the first frame of a resize has been seen proposed with the
/// following axis on its *bottom* edge rather than around its center, and
/// read off the proposal that frame looks like a top edge being dragged — two
/// points of jump at the start of the drag. Where the pointer's place is not
/// known, the proposal is what there is: an axis with both of its edges moved
/// follows. Either way a free window is not touched, since nothing in it
/// follows.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub fn keep_corner(start: UpFrame, proposed: UpFrame, held: Option<Held>) -> UpFrame {
    const STILL: f64 = 0.5;
    let moved = |a: f64, b: f64| (a - b).abs() > STILL;
    let follows_across = match held {
        Some(held) => !held.across && moved(proposed.w, start.w),
        None => moved(proposed.x, start.x) && moved(proposed.x + proposed.w, start.x + start.w),
    };
    let follows_up = match held {
        Some(held) => !held.up && moved(proposed.h, start.h),
        None => moved(proposed.y, start.y) && moved(proposed.y + proposed.h, start.y + start.h),
    };
    let mut kept = proposed;
    if follows_across {
        kept.x = start.x;
    }
    if follows_up {
        kept.y = start.y + start.h - proposed.h;
    }
    kept
}

/// Which axes the viewer has hold of in a live resize.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Held {
    /// The left or the right edge.
    pub across: bool,
    /// The top or the bottom edge.
    pub up: bool,
}

/// What the pointer took hold of, from where in the window it went down.
///
/// The band in which a press counts as a corner is wider than any the system
/// uses, on purpose. The two ways of being wrong are not alike: a corner taken
/// for an edge pins an axis the viewer is dragging, and the window grows away
/// from the pointer; an edge taken for a corner leaves the axis that follows
/// where AppKit puts it, which is what the window did before any of this.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub fn held_axes(size: (f64, f64), at: (f64, f64)) -> Held {
    const CORNER: f64 = 40.0;
    let across = at.0.min(size.0 - at.0);
    let up = at.1.min(size.1 - at.1);
    if across < CORNER && up < CORNER {
        return Held { across: true, up: true };
    }
    Held { across: across <= up, up: up < across }
}

// ---- Moving the frame as one thing ---------------------------------------
//
// A fit changes the window's size and its position, and the two are one change:
// the window takes the new picture's shape *around its own center*. Through the
// window API they are two calls — `setSize` grows the window from its top-left
// corner, `setPosition` then pulls it back — each queued to the main thread on
// its own, and nothing makes the two land in one frame. Once is a jump either
// way; twenty times in a row, which is what a morph is, it is a window that
// may show every step twice. Neither tao nor Tauri has a call that takes both,
// hence this one.
//
// It is stepped from here rather than from the frontend for the same reason:
// one main-thread turn per step, with nothing between the two halves. The
// steps are driven by elapsed time, not counted, so a busy main thread
// stretches them and not the animation.
//
// Measured on macOS, in the player, from a 2:1 film to a square clip and back
// (a thread reading the frame every 4 ms): some twenty steps over 210 ms, 10 ms
// apart at the median and 21 at the worst, the center within a point of where
// it started; a screen recording shows the picture centered in the window at
// every step and nothing uncovered at the edges.

/// A window's outer frame: its top-left corner and its size, in the physical
/// pixels `outerPosition` and `outerSize` report.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// In and out: the window starts from rest and comes to rest. The mini player's
/// glide eases out only, because that window is being *caught* by an edge and
/// its motion belongs next to the gesture; nothing is holding this one.
#[cfg_attr(not(any(windows, target_os = "macos", test)), allow(dead_code))]
pub fn ease(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// The frame `k` of the way from one to the other, on whole pixels.
///
/// The center and the size are what is interpolated, and the corner follows
/// from them. Rounding the four edges separately lets the width and the corner
/// round in opposite directions, and the side that should be standing still
/// ticks back and forth by a pixel for the length of the animation.
#[cfg_attr(not(any(windows, target_os = "macos", test)), allow(dead_code))]
pub fn between(from: Frame, to: Frame, k: f64) -> Frame {
    if k >= 1.0 {
        return to;
    }
    let mix = |a: f64, b: f64| a + (b - a) * k;
    let w = mix(from.w, to.w).round();
    let h = mix(from.h, to.h).round();
    let cx = mix(from.x + from.w / 2.0, to.x + to.w / 2.0);
    let cy = mix(from.y + from.h / 2.0, to.y + to.h / 2.0);
    Frame { x: (cx - w / 2.0).round(), y: (cy - h / 2.0).round(), w, h }
}

// ---- Pinch to resize -------------------------------------------------------
//
// A trackpad pinch scales the window about its own center, one frame per
// magnify event, from the frame the gesture began with (`macos_chrome::
// watch_pinch` is where the events come in; this is the arithmetic). Each step
// is computed from the start frame and the scale the gesture has accumulated,
// never from the previous step: spread and brought back, the fingers put the
// window exactly where it was, and a step lost to a busy main thread costs
// nothing.
//
// Whole points, and the frame is not sent at all when it is the one the window
// already has — measured, not a nicety: on macOS 26.5 a fractional frame whose
// other axis matched the current size exactly (1500.75 wide at the height the
// window already had, pinned against the screen) came out of AppKit's own
// `_setFrameCommon:` as a frame with `y` and `height` NaN and tripped a Swift
// `isFinite` precondition in the code that moves the needs-display region — a
// silent `brk`, nothing in any log. Three crashes on real pinches, none once
// the frames were whole and the no-ops skipped, over a dozen gestures.

/// Where a pinch has taken the window, and how far it really went.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pinched {
    pub frame: Frame,
    /// The scale the window took — `scale` as asked, or where the floor or
    /// the ceiling stopped it, before rounding. **The gesture has to carry this
    /// one on, not the one it asked for**: at the limit the fingers keep
    /// spreading and the window stands still, and if that surplus is banked,
    /// closing them again first pays it back, invisibly, before the window
    /// moves at all (reported exactly so). Before rounding, or a slow pinch
    /// under half a point a step would be thrown away at every step and never
    /// add up.
    pub scale: f64,
}

/// Where a pinch has taken the window: `start` scaled about its own center by
/// `scale`, in `start`'s shape, no smaller than `min` (as a size of that shape,
/// not per axis), no larger than `area` and inside it, on whole units.
///
/// The area is at least where the window already is: one the viewer put past
/// the margin — zoomed with the green button, dragged to the edge — is not
/// pulled back by a pinch, it only ever grows from there or shrinks in place.
/// The area wins over the minimum where the two disagree: a shape that cannot
/// be had on this screen at the minimum is the fit's problem (`shapeFits`),
/// and a window held to a size larger than its screen would be no answer here.
/// Anything meaningless — a scale that is not a positive finite number — leaves
/// the window where the gesture found it.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub fn pinch_frame(start: Frame, scale: f64, min: (f64, f64), area: Frame) -> Pinched {
    if !scale.is_finite() || scale <= 0.0 || start.w <= 0.0 || start.h <= 0.0 {
        return Pinched { frame: start, scale: 1.0 };
    }
    let area = {
        let x = area.x.min(start.x);
        let y = area.y.min(start.y);
        Frame {
            x,
            y,
            w: (area.x + area.w).max(start.x + start.w) - x,
            h: (area.y + area.h).max(start.y + start.h) - y,
        }
    };
    let mut scale = scale;
    let floor = f64::max(
        if min.0 > 0.0 { min.0 / (start.w * scale) } else { 0.0 },
        if min.1 > 0.0 { min.1 / (start.h * scale) } else { 0.0 },
    );
    if floor > 1.0 {
        scale *= floor;
    }
    let ceiling = f64::min(area.w / (start.w * scale), area.h / (start.h * scale));
    if ceiling < 1.0 && ceiling > 0.0 {
        scale *= ceiling;
    }
    let w = start.w * scale;
    let h = start.h * scale;
    // The size first and the corner from the center it keeps, each rounded on
    // its own — rounding four edges lets a size and a corner round apart and a
    // still edge tick (see `between`).
    let w = w.round();
    let h = h.round();
    let mut x = (start.x + start.w / 2.0 - w / 2.0).round();
    let mut y = (start.y + start.h / 2.0 - h / 2.0).round();
    // Inside the area, the far edge first and the near one last: where the
    // window is wider than the area by a rounding, the near edge is the one to
    // keep.
    if x + w > area.x + area.w {
        x = area.x + area.w - w;
    }
    if y + h > area.y + area.h {
        y = area.y + area.h - h;
    }
    if x < area.x {
        x = area.x;
    }
    if y < area.y {
        y = area.y;
    }
    Pinched { frame: Frame { x, y, w, h }, scale }
}

/// Which glide is the newest. One that finds a newer number has been
/// superseded and stops where it is: the newer one starts from there.
#[cfg(any(windows, target_os = "macos"))]
static GLIDE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Something other than a glide has taken hold of the frame — a pinch — and
/// whatever glide is in flight stops where it is.
#[cfg(target_os = "macos")]
pub fn supersede_glide() {
    GLIDE.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

/// Between two steps. Shorter than a frame of any display this runs on; the
/// steps that land inside one frame cost a main-thread turn each and nothing
/// else.
#[cfg(any(windows, target_os = "macos"))]
const GLIDE_STEP: std::time::Duration = std::time::Duration::from_millis(8);

/// Take the window to a frame over `ms` milliseconds, or at once for zero.
///
/// Answers `true` when the window is at that frame or something newer has taken
/// the frame over on the way — a later glide, or a pinch, either of which is
/// now the one deciding where the window goes — and `false` on a platform with
/// no way to do it, or with no window to do it to, where the frontend falls
/// back to the two calls it used to make. A superseded glide used to answer
/// `false` too, which sent the frontend's fallback after the window a pinch had
/// just taken hold of.
#[tauri::command]
#[cfg_attr(not(any(windows, target_os = "macos")), allow(unused_variables))]
pub async fn window_frame_glide(
    window: tauri::WebviewWindow,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    ms: u32,
) -> bool {
    #[cfg(any(windows, target_os = "macos"))]
    {
        use std::sync::atomic::Ordering;

        let mine = GLIDE.fetch_add(1, Ordering::SeqCst) + 1;
        let to = Frame { x, y, w: width, h: height };
        let Some(from) = on_main(&window, frame_now).await.flatten() else {
            return false;
        };
        let length = f64::from(ms) / 1000.0;
        let started = std::time::Instant::now();
        loop {
            if GLIDE.load(Ordering::SeqCst) != mine {
                return true;
            }
            let t = if length > 0.0 {
                (started.elapsed().as_secs_f64() / length).min(1.0)
            } else {
                1.0
            };
            let step = between(from, to, ease(t));
            if on_main(&window, move |w| set_frame(w, step)).await.is_none() {
                return false;
            }
            if t >= 1.0 {
                return true;
            }
            tokio::time::sleep(GLIDE_STEP).await;
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        false
    }
}

/// Which fade of the picture is the newest; see `GLIDE`.
#[cfg(target_os = "macos")]
static FADE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How far a fade has got, `t` of the way through. Going dark it is quick at
/// first — the frame being replaced is the thing to get off the screen — and
/// coming back it is slow at first: the bars beside a new picture are widest at
/// the start of the window's morph, which is the part of it to keep dark.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub fn fade_curve(t: f64, lifting: bool) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if lifting {
        t * t
    } else {
        1.0 - (1.0 - t) * (1.0 - t)
    }
}

/// Fade the picture itself to `to` (0 is dark, 1 is whole) over `ms`.
///
/// The dark between two files, on macOS. It is the video view's own opacity
/// rather than a fill in the web view because a fill in the web view is late:
/// the web content is a frame or two behind the window's frame, so while the
/// window grows there is a strip along the growing edge that the fill has not
/// reached yet, and the picture shows through it at full brightness (measured
/// on a recording; making the fill larger than the window does not help, the
/// content is clipped to the size it was last laid out at). The video view
/// resizes with the window, in the same turn.
///
/// `Ok(true)` when the picture is at `to`, `Ok(false)` when a newer fade took
/// over, and an error where there is no such view to fade — on which the
/// frontend draws the dark itself.
#[tauri::command]
#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
pub async fn window_video_fade(window: tauri::WebviewWindow, to: f64, ms: u32) -> Result<bool, ()> {
    #[cfg(target_os = "macos")]
    {
        use std::sync::atomic::Ordering;

        let mine = FADE.fetch_add(1, Ordering::SeqCst) + 1;
        let to = to.clamp(0.0, 1.0);
        let from = on_main(&window, crate::macos_chrome::video_alpha)
            .await
            .flatten()
            .ok_or(())?;
        let length = f64::from(ms) / 1000.0;
        let started = std::time::Instant::now();
        loop {
            if FADE.load(Ordering::SeqCst) != mine {
                return Ok(false);
            }
            let t = if length > 0.0 {
                (started.elapsed().as_secs_f64() / length).min(1.0)
            } else {
                1.0
            };
            let alpha = if t >= 1.0 { to } else { from + (to - from) * fade_curve(t, to > from) };
            let set = on_main(&window, move |w| crate::macos_chrome::set_video_alpha(w, alpha)).await;
            if set != Some(true) {
                return Err(());
            }
            if t >= 1.0 {
                return Ok(true);
            }
            tokio::time::sleep(GLIDE_STEP).await;
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(())
    }
}

/// Run on the thread that owns the window and wait for the answer:
/// `run_on_main_thread` by itself returns as soon as the closure is queued.
#[cfg(any(windows, target_os = "macos"))]
async fn on_main<T: Send + 'static>(
    window: &tauri::WebviewWindow,
    f: impl FnOnce(&tauri::WebviewWindow) -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let win = window.clone();
    window
        .run_on_main_thread(move || {
            let _ = tx.send(f(&win));
        })
        .ok()?;
    rx.await.ok()
}

#[cfg(target_os = "macos")]
fn frame_now(window: &tauri::WebviewWindow) -> Option<Frame> {
    crate::macos_chrome::frame(window)
}

#[cfg(target_os = "macos")]
fn set_frame(window: &tauri::WebviewWindow, frame: Frame) {
    crate::macos_chrome::set_frame(window, frame);
}

#[cfg(windows)]
fn frame_now(window: &tauri::WebviewWindow) -> Option<Frame> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;

    let hwnd = window.hwnd().ok()?;
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    if unsafe { GetWindowRect(hwnd.0 as _, &mut r) } == 0 {
        return None;
    }
    Some(Frame {
        x: f64::from(r.left),
        y: f64::from(r.top),
        w: f64::from(r.right - r.left),
        h: f64::from(r.bottom - r.top),
    })
}

#[cfg(windows)]
fn set_frame(window: &tauri::WebviewWindow, frame: Frame) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER,
    };

    let Ok(hwnd) = window.hwnd() else { return };
    unsafe {
        SetWindowPos(
            hwnd.0 as _,
            std::ptr::null_mut(),
            frame.x as i32,
            frame.y as i32,
            frame.w as i32,
            frame.h as i32,
            SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDE: f64 = 16.0 / 9.0;
    const NO_FRAME: (i32, i32) = (0, 0);
    const NO_MIN: (i32, i32) = (0, 0);

    fn at(left: i32, top: i32, w: i32, h: i32) -> Bounds {
        Bounds { left, top, right: left + w, bottom: top + h }
    }

    fn size(b: Bounds) -> (i32, i32) {
        (b.right - b.left, b.bottom - b.top)
    }

    #[test]
    fn a_side_edge_drives_the_width_and_the_height_follows() {
        // The right edge pulled out to 1600 wide, the height left where it was.
        let held = hold_shape(at(100, 100, 1600, 450), Grip::Right, WIDE, NO_FRAME, NO_MIN);
        assert_eq!(held, at(100, 100, 1600, 900));
    }

    #[test]
    fn a_top_or_bottom_edge_drives_the_height() {
        let held = hold_shape(at(100, 100, 800, 900), Grip::Bottom, WIDE, NO_FRAME, NO_MIN);
        assert_eq!(held, at(100, 100, 1600, 900));
    }

    #[test]
    fn the_edge_opposite_the_grip_never_moves() {
        let drag = at(100, 100, 1600, 450);
        for (grip, fixed_right, fixed_bottom) in [
            (Grip::Left, true, false),
            (Grip::Top, false, true),
            (Grip::TopLeft, true, true),
            (Grip::TopRight, false, true),
            (Grip::BottomLeft, true, false),
            (Grip::BottomRight, false, false),
        ] {
            let held = hold_shape(drag, grip, WIDE, NO_FRAME, NO_MIN);
            if fixed_right {
                assert_eq!(held.right, drag.right, "{grip:?}");
            } else {
                assert_eq!(held.left, drag.left, "{grip:?}");
            }
            if fixed_bottom {
                assert_eq!(held.bottom, drag.bottom, "{grip:?}");
            } else {
                assert_eq!(held.top, drag.top, "{grip:?}");
            }
        }
    }

    #[test]
    fn a_corner_takes_the_larger_of_the_two_windows_it_is_asked_for() {
        // Dragged mostly sideways: the width decides.
        let sideways = hold_shape(at(0, 0, 1600, 500), Grip::BottomRight, WIDE, NO_FRAME, NO_MIN);
        assert_eq!(size(sideways), (1600, 900));
        // Dragged mostly down: the height does.
        let down = hold_shape(at(0, 0, 900, 900), Grip::BottomRight, WIDE, NO_FRAME, NO_MIN);
        assert_eq!(size(down), (1600, 900));
    }

    #[test]
    fn the_minimum_is_a_size_of_the_shape_rather_than_a_clamp_per_axis() {
        // 480x320 is the window's minimum and is not 16:9. Pulled all the way
        // in, the window stops at the smallest 16:9 that covers both.
        let held = hold_shape(at(0, 0, 480, 320), Grip::Right, WIDE, NO_FRAME, (480, 320));
        assert_eq!(size(held), (569, 320));
        // An upright picture is limited by the width instead.
        let upright = hold_shape(at(0, 0, 480, 320), Grip::Bottom, 9.0 / 16.0, NO_FRAME, (480, 320));
        assert_eq!(size(upright), (480, 853));
    }

    #[test]
    fn the_height_never_lands_a_pixel_under_the_minimum() {
        for ratio in [2.39, 1.85, WIDE, 4.0 / 3.0, 1.0, 0.75, 9.0 / 16.0] {
            for min in [(480, 320), (240, 135), (481, 321)] {
                let held = hold_shape(at(0, 0, 1, 1), Grip::BottomRight, ratio, NO_FRAME, min);
                let (w, h) = size(held);
                assert!(w >= min.0 && h >= min.1, "{ratio} {min:?} gave {w}x{h}");
            }
        }
    }

    #[test]
    fn the_shape_is_the_contents_and_the_frame_is_added_around_it() {
        // A frame of 16x39: the content must be 16:9, the outer size is not.
        let held = hold_shape(at(0, 0, 1616, 400), Grip::Right, WIDE, (16, 39), NO_MIN);
        assert_eq!(size(held), (1616, 939));
    }

    #[test]
    fn a_glide_starts_and_ends_at_rest_and_never_turns_back() {
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        assert!((ease(0.5) - 0.5).abs() < 1e-9);
        let mut last = 0.0;
        for i in 0..=100 {
            let k = ease(f64::from(i) / 100.0);
            assert!(k >= last, "turned back at {i}");
            last = k;
        }
        // Out of range is the end it is nearest to, not an extrapolation.
        assert_eq!(ease(-1.0), 0.0);
        assert_eq!(ease(2.0), 1.0);
    }

    #[test]
    fn a_glide_ends_on_the_frame_it_was_given() {
        let from = Frame { x: 100.0, y: 100.0, w: 1414.0, h: 707.0 };
        let to = Frame { x: 307.5, y: -46.0, w: 999.0, h: 999.0 };
        assert_eq!(between(from, to, 1.0), to);
        assert_eq!(between(from, to, 0.0), from);
    }

    #[test]
    fn a_morph_around_the_center_keeps_the_center() {
        // 2:1 to 1:1 at the same area, around (807, 453).
        let from = Frame { x: 100.0, y: 100.0, w: 1414.0, h: 706.0 };
        let to = Frame { x: 307.0, y: -47.0, w: 1000.0, h: 1000.0 };
        for i in 0..=50 {
            let f = between(from, to, ease(f64::from(i) / 50.0));
            // Whole pixels, so half of one is the most it can be off by.
            assert!((f.x + f.w / 2.0 - 807.0).abs() <= 0.5, "x at step {i}: {f:?}");
            assert!((f.y + f.h / 2.0 - 453.0).abs() <= 0.5, "y at step {i}: {f:?}");
            assert_eq!(f.x, f.x.round());
            assert_eq!(f.w, f.w.round());
        }
    }

    fn up(x: f64, y: f64, w: f64, h: f64) -> UpFrame {
        UpFrame { x, y, w, h }
    }

    #[test]
    fn the_axis_that_follows_is_put_back_on_its_top_or_left_edge() {
        let start = up(600.0, 450.0, 800.0, 450.0);
        // The right edge dragged out by 120: AppKit's proposal, as measured.
        for held in [Some(Held { across: true, up: false }), None] {
            let kept = keep_corner(start, up(600.0, 416.0, 920.0, 518.0), held);
            assert_eq!(kept, up(600.0, 382.0, 920.0, 518.0));
            assert_eq!(kept.y + kept.h, start.y + start.h, "the top edge");
        }
        // The bottom edge dragged down by 80.
        for held in [Some(Held { across: false, up: true }), None] {
            let kept = keep_corner(start, up(530.0, 371.0, 940.0, 529.0), held);
            assert_eq!(kept, up(600.0, 371.0, 940.0, 529.0));
        }
    }

    #[test]
    fn the_first_frame_of_a_drag_is_not_taken_for_another_drag() {
        // As seen in the player: the right edge in hand, and the first proposal
        // two points taller with the *bottom* where it was. From the proposal
        // alone that is the top edge being dragged.
        let start = up(261.0, 282.0, 1206.0, 603.0);
        let first = up(261.0, 282.0, 1210.0, 605.0);
        let by_the_right = Some(Held { across: true, up: false });
        let kept = keep_corner(start, first, by_the_right);
        assert_eq!(kept.y + kept.h, 885.0);
        assert_eq!(keep_corner(start, first, None), first);
    }

    #[test]
    fn where_the_pointer_went_down_says_what_is_held() {
        let size = (1200.0, 600.0);
        assert_eq!(held_axes(size, (1199.0, 300.0)), Held { across: true, up: false });
        assert_eq!(held_axes(size, (1.0, 300.0)), Held { across: true, up: false });
        assert_eq!(held_axes(size, (600.0, 1.0)), Held { across: false, up: true });
        assert_eq!(held_axes(size, (600.0, 599.0)), Held { across: false, up: true });
        for corner in [(3.0, 3.0), (1197.0, 3.0), (3.0, 597.0), (1197.0, 597.0), (1199.0, 30.0)] {
            assert_eq!(held_axes(size, corner), Held { across: true, up: true }, "{corner:?}");
        }
        // Just outside the window, where the system's own band reaches.
        assert_eq!(held_axes(size, (1203.0, 300.0)), Held { across: true, up: false });
    }

    #[test]
    fn the_edge_that_is_dragged_is_left_to_the_drag() {
        let start = up(600.0, 450.0, 800.0, 450.0);
        // The left edge: the right one stays, and the height follows.
        for held in [Some(Held { across: true, up: false }), None] {
            let kept = keep_corner(start, up(480.0, 416.0, 920.0, 518.0), held);
            assert_eq!(kept.x, 480.0);
            assert_eq!(kept.x + kept.w, 1400.0);
            assert_eq!(kept.y + kept.h, 900.0);
        }
        // The top edge: the bottom stays, and the width follows from the left.
        for held in [Some(Held { across: false, up: true }), None] {
            let kept = keep_corner(start, up(530.0, 450.0, 940.0, 529.0), held);
            assert_eq!(kept, up(600.0, 450.0, 940.0, 529.0));
        }
    }

    #[test]
    fn a_corner_keeps_the_corner_opposite() {
        let start = up(600.0, 450.0, 800.0, 450.0);
        // Bottom-left dragged out: the top-right corner is where it was.
        let proposed = up(564.0, 430.0, 836.0, 470.0);
        for held in [Some(Held { across: true, up: true }), None] {
            assert_eq!(keep_corner(start, proposed, held), proposed);
        }
    }

    #[test]
    fn a_free_window_is_not_touched() {
        let start = up(600.0, 450.0, 800.0, 450.0);
        // Each edge in turn, with no ratio: only the edge in hand has moved.
        for (proposed, across, up_held) in [
            (up(600.0, 450.0, 920.0, 450.0), true, false),
            (up(600.0, 370.0, 800.0, 530.0), false, true),
            (up(480.0, 450.0, 920.0, 450.0), true, false),
            (up(600.0, 450.0, 800.0, 530.0), false, true),
            (up(480.0, 450.0, 920.0, 530.0), true, true),
            (start, true, false),
        ] {
            assert_eq!(keep_corner(start, proposed, None), proposed);
            let held = Some(Held { across, up: up_held });
            assert_eq!(keep_corner(start, proposed, held), proposed, "{held:?}");
        }
    }

    #[test]
    fn a_shrinking_window_keeps_the_same_corner() {
        let start = up(600.0, 450.0, 800.0, 450.0);
        for held in [Some(Held { across: true, up: false }), None] {
            let kept = keep_corner(start, up(600.0, 534.0, 500.0, 281.0), held);
            assert_eq!(kept.y + kept.h, 900.0);
            assert_eq!(kept.x, 600.0);
        }
    }

    #[test]
    fn a_fade_goes_dark_fast_and_comes_back_slowly() {
        for lifting in [true, false] {
            assert_eq!(fade_curve(0.0, lifting), 0.0);
            assert_eq!(fade_curve(1.0, lifting), 1.0);
            let mut last = 0.0;
            for i in 0..=50 {
                let k = fade_curve(f64::from(i) / 50.0, lifting);
                assert!(k >= last);
                last = k;
            }
        }
        assert!(fade_curve(0.5, false) > 0.7);
        assert!(fade_curve(0.5, true) < 0.3);
    }

    #[test]
    fn a_ratio_that_means_nothing_leaves_the_drag_alone() {
        let drag = at(10, 20, 700, 300);
        for ratio in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(hold_shape(drag, Grip::Right, ratio, NO_FRAME, NO_MIN), drag);
        }
    }

    // ---- pinch_frame ----

    fn frame(x: f64, y: f64, w: f64, h: f64) -> Frame {
        Frame { x, y, w, h }
    }

    /// A 1728×997 work area starting 87 up, as a laptop with its Dock reports it.
    const DESK: Frame = Frame { x: 0.0, y: 87.0, w: 1728.0, h: 997.0 };
    const NO_FLOOR: (f64, f64) = (0.0, 0.0);

    #[test]
    fn a_pinch_scales_about_the_center() {
        let start = frame(400.0, 300.0, 800.0, 450.0);
        let out = pinch_frame(start, 1.5, NO_FLOOR, DESK).frame;
        // 1200×675 around the center (800, 525): y wants 187.5 and rounds up.
        assert_eq!(out, frame(200.0, 188.0, 1200.0, 675.0));
        assert_eq!(out.x + out.w / 2.0, start.x + start.w / 2.0);
        assert!((out.y + out.h / 2.0 - (start.y + start.h / 2.0)).abs() <= 0.5);
    }

    #[test]
    fn brought_back_to_one_the_window_is_where_it_started() {
        let start = frame(535.0, 443.0, 429.0, 285.0);
        assert_eq!(pinch_frame(start, 1.0, NO_FLOOR, DESK).frame, start);
        // Scaled up and down again, the same — from the start frame, not from
        // the last step.
        assert_eq!(pinch_frame(start, 3.0 / 3.0, NO_FLOOR, DESK).frame, start);
    }

    #[test]
    fn every_edge_lands_on_a_whole_unit() {
        let start = frame(100.5, 100.0, 801.0, 450.0);
        for scale in [0.77, 1.1, 1.3333, 2.4142] {
            let out = pinch_frame(start, scale, NO_FLOOR, DESK).frame;
            for v in [out.x, out.y, out.w, out.h] {
                assert_eq!(v, v.round(), "scale {scale}: {out:?}");
            }
        }
    }

    #[test]
    fn the_floor_is_a_size_of_the_shape() {
        // 800×450 pinched down to a tenth; the minimum is 480×320, which is
        // not 16:9 — the smallest window of this shape that covers both is
        // 569×320, exactly `floorForShape`'s answer.
        let out = pinch_frame(frame(400.0, 300.0, 800.0, 450.0), 0.1, (480.0, 320.0), DESK);
        assert_eq!((out.frame.w, out.frame.h), (569.0, 320.0));
        // And the scale the window took is the floor's, not the tenth asked.
        assert!((out.scale - 320.0 / 450.0).abs() < 1e-9, "{}", out.scale);
    }

    #[test]
    fn the_ceiling_is_the_axis_that_hits_first_and_the_shape_is_kept() {
        let out = pinch_frame(frame(400.0, 300.0, 800.0, 450.0), 3.0, NO_FLOOR, DESK);
        // 2400×1350 wanted; the width runs out first at 1728, and the height
        // follows the shape (972) rather than filling the 997.
        assert_eq!((out.frame.w, out.frame.h), (1728.0, 972.0));
        assert_eq!(out.scale, 1728.0 / 800.0);
        let out = out.frame;
        assert!(out.y >= DESK.y && out.y + out.h <= DESK.y + DESK.h, "{out:?}");
        // A taller shape runs out of height first and keeps its width short
        // (a start inside the area: one past it widens the area, see below).
        let tall = pinch_frame(frame(400.0, 200.0, 450.0, 800.0), 3.0, NO_FLOOR, DESK).frame;
        assert_eq!((tall.w, tall.h), ((997.0 * 450.0 / 800.0f64).round(), 997.0));
    }

    #[test]
    fn the_window_stays_inside_the_area_when_its_center_is_near_an_edge() {
        let out = pinch_frame(frame(1500.0, 800.0, 200.0, 112.0), 3.0, NO_FLOOR, DESK).frame;
        assert!(out.x >= DESK.x && out.x + out.w <= DESK.x + DESK.w, "{out:?}");
        assert!(out.y >= DESK.y && out.y + out.h <= DESK.y + DESK.h, "{out:?}");
        assert_eq!((out.w, out.h), (600.0, 336.0));
    }

    #[test]
    fn a_window_already_past_the_margin_is_not_pulled_back() {
        // Zoomed with the green button: flush with the screen, past the area
        // a pinch keeps to. Spread further, it stays; brought in, it shrinks
        // in place.
        let zoomed = frame(0.0, 87.0, 1728.0, 997.0);
        let margin = frame(24.0, 111.0, 1680.0, 949.0);
        assert_eq!(pinch_frame(zoomed, 1.0, NO_FLOOR, margin).frame, zoomed);
        assert_eq!(pinch_frame(zoomed, 1.3, NO_FLOOR, margin).frame, zoomed);
        let smaller = pinch_frame(zoomed, 0.9, NO_FLOOR, margin).frame;
        assert_eq!((smaller.w, smaller.h), (1555.0, 897.0));
        assert!((smaller.x + smaller.w / 2.0 - 864.0).abs() <= 0.5, "{smaller:?}");
    }

    #[test]
    fn the_area_wins_over_a_minimum_that_does_not_fit_it() {
        let small = frame(0.0, 0.0, 500.0, 300.0);
        let out = pinch_frame(frame(10.0, 10.0, 400.0, 240.0), 0.5, (800.0, 480.0), small).frame;
        assert!(out.w <= small.w && out.h <= small.h, "{out:?}");
        assert!(out.x >= 0.0 && out.y >= 0.0);
    }

    #[test]
    fn a_scale_that_means_nothing_leaves_the_window_alone() {
        let start = frame(100.0, 100.0, 800.0, 450.0);
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(pinch_frame(start, scale, NO_FLOOR, DESK), Pinched { frame: start, scale: 1.0 });
        }
    }

    #[test]
    fn spreading_past_the_limit_banks_nothing() {
        // At the ceiling the fingers keep spreading: the scale the gesture
        // carries on is the ceiling's, so the first step back in moves the
        // window at once rather than paying off what was never shown.
        let start = frame(400.0, 300.0, 800.0, 450.0);
        let at_limit = pinch_frame(start, 3.0, NO_FLOOR, DESK);
        let further = pinch_frame(start, at_limit.scale * 1.05, NO_FLOOR, DESK);
        assert_eq!(further.frame, at_limit.frame);
        assert_eq!(further.scale, at_limit.scale);
        let back = pinch_frame(start, further.scale * 0.95, NO_FLOOR, DESK).frame;
        assert!(back.w < at_limit.frame.w, "{back:?} vs {:?}", at_limit.frame);
    }
}
