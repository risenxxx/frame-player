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
//! (`window_frame_glide`): the position and the size as one frame, over time.

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
    use windows_sys::Win32::UI::WindowsAndMessaging::{WM_NCDESTROY, WM_SIZING};

    match msg {
        WM_SIZING => {
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

/// Which glide is the newest. One that finds a newer number has been
/// superseded and stops where it is: the newer one starts from there.
#[cfg(any(windows, target_os = "macos"))]
static GLIDE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Between two steps. Shorter than a frame of any display this runs on; the
/// steps that land inside one frame cost a main-thread turn each and nothing
/// else.
#[cfg(any(windows, target_os = "macos"))]
const GLIDE_STEP: std::time::Duration = std::time::Duration::from_millis(8);

/// Take the window to a frame over `ms` milliseconds, or at once for zero.
///
/// Answers `true` when the window is at that frame, `false` when it is not —
/// superseded by a newer glide, or on a platform with no way to do it, where
/// the frontend falls back to the two calls it used to make.
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
                return false;
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

    #[test]
    fn a_ratio_that_means_nothing_leaves_the_drag_alone() {
        let drag = at(10, 20, 700, 300);
        for ratio in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(hold_shape(drag, Grip::Right, ratio, NO_FRAME, NO_MIN), drag);
        }
    }
}
