//! Native window chrome on macOS.
//!
//! On Windows the window is fully frameless (`decorations: false`) and all the
//! chrome is drawn in HTML. On macOS that does not work, for two reasons:
//!
//! 1. A frameless window has square corners — the system only rounds windows
//!    with the `Titled` style.
//! 2. During the `toggleFullScreen:` animation a frameless window gets a
//!    standard title bar mixed in, and it visibly pops in and out.
//!
//! Both are solved the same way: the window stays `Titled` but with
//! `FullSizeContentView` and a transparent title bar — the content fills the
//! whole window, no title is drawn, and the system window buttons stay where
//! macOS users expect them, on the left.

use objc2::rc::Retained;
use objc2::Message;
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameDarkAqua, NSToolbar, NSWindow,
    NSWindowAnimationBehavior, NSWindowButton, NSWindowCollectionBehavior, NSWindowStyleMask,
    NSWindowTitleVisibility, NSWindowToolbarStyle,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};

const TRAFFIC_LIGHTS: [NSWindowButton; 3] = [
    NSWindowButton::CloseButton,
    NSWindowButton::MiniaturizeButton,
    NSWindowButton::ZoomButton,
];

fn ns_window(window: &tauri::WebviewWindow) -> Option<Retained<NSWindow>> {
    let ptr = window.ns_window().ok()? as *mut NSWindow;
    if ptr.is_null() {
        return None;
    }
    // Tauri hands out a borrowed pointer to a live window — take another
    // reference rather than ownership.
    unsafe { Retained::retain(ptr) }
}

/// Main thread only (AppKit forgives nothing else).
pub fn apply(window: &tauri::WebviewWindow) {
    let Some(mtm) = MainThreadMarker::new() else {
        log_warn("macos_chrome::apply called off the main thread");
        return;
    };
    let Some(ns) = ns_window(window) else {
        log_warn("could not obtain the NSWindow");
        return;
    };

    // Titled brings back the system corner rounding and removes the popping
    // title bar during the fullscreen transition; FullSizeContentView gives the
    // whole window area to our webview.
    ns.setStyleMask(
        NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable
            | NSWindowStyleMask::FullSizeContentView,
    );
    ns.setTitlebarAppearsTransparent(true);
    ns.setTitleVisibility(NSWindowTitleVisibility::Hidden);

    // The window's own background. Without it the window has no fill at all
    // (`transparent: true` makes it transparent so mpv shows through the
    // webview), and at show time the system draws the traffic lights before the
    // webview composites its first frame — a screen recording shows a couple of
    // frames with the buttons hanging in empty space.
    //
    // The background sits BELOW both the mpv view and the webview, so it covers
    // nothing: it only shows where nobody painted. The color is the same
    // #101016 as the `.player.backdrop` fill in the frontend.
    //
    // setOpaque(true) is deliberately avoided: an opaque window loses the
    // system corner rounding and the edge shadow.
    ns.setBackgroundColor(Some(&window_fill()));

    // Force the dark appearance instead of following the system. The player's
    // chrome is dark whatever the system theme is, and the frame view draws
    // itself to match the *appearance*, not the background color: in light
    // mode it puts a bright highlight along the window's top edge, far stronger
    // than the shadow on the other three sides and clearly visible against the
    // dark fill. Captured both ways — the line all but disappears under
    // `darkAqua`. (It is not the toolbar's doing: the same line is there with no
    // toolbar at all, and `titlebarSeparatorStyle` changes it by not one pixel.)
    // Nothing in the frontend keys off `prefers-color-scheme`, so this only
    // affects system-drawn parts.
    let dark = unsafe { NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua) };
    ns.setAppearance(dark.as_deref());

    // No window-appear animation. This dates from when the traffic lights were
    // placed by hand and corrected *after* AppKit had moved them: during the
    // growth animation relayouts come in bursts, and the correction was a frame
    // behind by construction, so in slow motion the green button visibly hopped
    // to its default spot and back. Nothing is corrected any more (see the
    // traffic-lights section), so the line is no longer load-bearing — it stays
    // because an instant window suits a player, and re-enabling the animation is
    // a visual change to make on purpose rather than as a side effect.
    ns.setAnimationBehavior(NSWindowAnimationBehavior::None);
    // Dragging is handled by the frontend (data-tauri-drag-region), so the
    // window must not also move when its background is dragged.
    ns.setMovableByWindowBackground(false);

    // The traffic lights: an empty toolbar makes the title bar tall enough that
    // AppKit gives them the inset we want, and keeps owning their layout and
    // hover behavior. See the section comment below for why not to move them
    // by hand.
    install_toolbar(&ns, mtm);
    toolbar_off_in_fullscreen(&ns);
    mask_fullscreen_transitions(window, &ns);
    keep_corner_in_live_resize(window, &ns);
}

/// The window's own fill, `#101016` — see `apply`.
fn window_fill() -> Retained<objc2_app_kit::NSColor> {
    objc2_app_kit::NSColor::colorWithSRGBRed_green_blue_alpha(
        16.0 / 255.0,
        16.0 / 255.0,
        22.0 / 255.0,
        1.0,
    )
}

/// Grow the title bar so AppKit gives the window buttons a roomier inset.
fn install_toolbar(ns: &NSWindow, mtm: MainThreadMarker) {
    let toolbar = NSToolbar::new(mtm);
    toolbar.setAllowsUserCustomization(false);
    if set_toolbar_guarded(ns, Some(&toolbar)) {
        ns.setToolbarStyle(NSWindowToolbarStyle::Unified);
    }
}

/// Take the toolbar off, if it is still on.
fn remove_toolbar(ns: &NSWindow) {
    if ns.toolbar().is_some() {
        set_toolbar_guarded(ns, None);
    }
}

/// `setToolbar`, with an Objective-C exception turned into a log line.
///
/// Swapping the toolbar tears the title bar's views down and rebuilds them, and
/// on macOS 26 that has thrown from inside AppKit: a view leaving the window
/// removed a key-value observer from the NSWindow that was not registered
/// (`-[NSObject(NSKeyValueObserverRegistration) _removeObserver:forProperty:]`,
/// under `-[NSThemeFrame _showHideToolbar:…]`). The observer is AppKit's own —
/// nothing here observes the window — so there is no imbalance on our side to
/// fix. What made it fatal is where it happened: in the `WillEnterFullScreen`
/// notification, inside `NSPerformVisuallyAtomicChange`, called from tao's Rust
/// dispatch closure. The exception unwound into Rust frames, which cannot
/// unwind it, and the process aborted. Caught here it costs at most a title bar
/// that is the wrong height until the next swap.
fn set_toolbar_guarded(ns: &NSWindow, toolbar: Option<&NSToolbar>) -> bool {
    let swap = std::panic::AssertUnwindSafe(|| ns.setToolbar(toolbar));
    match objc2::exception::catch(swap) {
        Ok(()) => true,
        Err(exception) => {
            let what = exception.map_or_else(|| "nil".to_owned(), |e| e.to_string());
            log_warn(&format!("setToolbar threw: {what}"));
            false
        }
    }
}

/// Whether `enter_fullscreen` hid the traffic lights on its way in, so the
/// `WillEnterFullScreen` handler knows to show them again. Only what it hid
/// comes back: buttons the idle UI had already hidden stay the way they were.
static HIDDEN_FOR_ENTRY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Go fullscreen with the toolbar already off, so AppKit's transition never has
/// to take it away. Main thread only. Answers `false` when it did not start the
/// transition, and the caller should go through tao instead — where the
/// `WillEnterFullScreen` handler still removes the toolbar, which is also the
/// path for the green button and ⌃⌘F.
///
/// Taking the toolbar away mid-transition is what crashed (see
/// `set_toolbar_guarded`); doing it beforehand means the title bar is rebuilt
/// outside `NSPerformVisuallyAtomicChange`, in a window no transition is
/// holding snapshots of. Two things about the order are measured, not chosen.
/// The traffic lights are hidden first: removing the toolbar drops them from
/// (19, 33) to the system's (9, 9), and on its own that hop was on screen for
/// three frames before the zoom began. And the transition is started in the
/// **same** main-thread turn, by `toggleFullScreen:` here rather than by a
/// second IPC call: the hidden buttons and the short title bar are then never
/// committed to the screen — recorded, the frames are indistinguishable from
/// the old path — and the buttons come back in `WillEnterFullScreen`, in the
/// strip where they belong. tao sees this the way it sees the green button.
pub fn enter_fullscreen(window: &tauri::WebviewWindow) -> bool {
    use std::sync::atomic::Ordering;

    let Some(ns) = ns_window(window) else {
        return false;
    };
    if ns.styleMask().contains(NSWindowStyleMask::FullScreen) {
        return true;
    }
    // Without it `toggleFullScreen:` does nothing (the mini player takes it
    // away), and a window left with no toolbar and no buttons would be worse
    // than tao reporting the same no-op.
    if !ns
        .collectionBehavior()
        .contains(NSWindowCollectionBehavior::FullScreenPrimary)
    {
        return false;
    }
    let mut hid = false;
    for kind in TRAFFIC_LIGHTS {
        if let Some(btn) = ns.standardWindowButton(kind) {
            if !btn.isHidden() {
                btn.setHidden(true);
                hid = true;
            }
        }
    }
    HIDDEN_FOR_ENTRY.store(hid, Ordering::Relaxed);
    remove_toolbar(&ns);
    ns.toggleFullScreen(None);
    true
}

/// Take the toolbar away for the duration of fullscreen.
///
/// A window that has one shows it in fullscreen permanently, as a tall opaque
/// band across the top — and the buttons on it only become visible once the
/// system menu bar is revealed, so until then the band carries nothing at all.
/// Without a toolbar macOS falls back to what fullscreen should look like: the
/// thin strip that slides down when the pointer reaches the top edge, with the
/// window buttons at their standard place in it (measured: the title-bar host
/// goes 66 pt → 32 pt and the buttons to (9, 9), tracking rect with them).
///
/// Restoring the toolbar has to wait for `DidExitFullScreen`, and the buttons
/// have to be hidden until it does. Measured: setting the toolbar on
/// `WillExitFullScreen` has no effect at all — AppKit ignores it while the
/// transition is in flight, and the window animates back and sits there for
/// several frames with the short title bar before `DidExit` arrives. So the
/// buttons are visibly at the system position first and jump to ours after.
/// There is no earlier hook; the only fix is to not show the intermediate
/// state, hence hiding them for the duration and unhiding once the toolbar is
/// back — they then appear already in the right place.
///
/// What comes back is `BUTTONS_VISIBLE`, the frontend's last instruction, and
/// not whatever `isHidden` happens to say. AppKit shows the buttons itself in
/// the fullscreen strip, so reading the button back made an idle-faded UI
/// return with its traffic lights on, which the next idle pass then blinked
/// away again.
fn toolbar_off_in_fullscreen(ns: &NSWindow) {
    use block2::RcBlock;
    use objc2_app_kit::{
        NSWindowDidExitFullScreenNotification, NSWindowWillEnterFullScreenNotification,
        NSWindowWillExitFullScreenNotification,
    };
    use objc2_foundation::{NSNotification, NSNotificationCenter};

    let center = NSNotificationCenter::defaultCenter();

    let win = ns.retain();
    let entering = RcBlock::new(move |_: core::ptr::NonNull<NSNotification>| {
        // Already off when `enter_fullscreen` started this; still on for the
        // green button, ⌃⌘F and tao's own path.
        remove_toolbar(&win);
        if HIDDEN_FOR_ENTRY.swap(false, std::sync::atomic::Ordering::Relaxed) {
            for kind in TRAFFIC_LIGHTS {
                if let Some(btn) = win.standardWindowButton(kind) {
                    btn.setHidden(false);
                }
            }
        }
    });

    let win = ns.retain();
    let leaving = RcBlock::new(move |_: core::ptr::NonNull<NSNotification>| {
        for kind in TRAFFIC_LIGHTS {
            if let Some(btn) = win.standardWindowButton(kind) {
                btn.setHidden(true);
            }
        }
    });

    let win = ns.retain();
    let left = RcBlock::new(move |_: core::ptr::NonNull<NSNotification>| {
        if let Some(mtm) = MainThreadMarker::new() {
            install_toolbar(&win, mtm);
        }
        let visible = BUTTONS_VISIBLE.load(std::sync::atomic::Ordering::Relaxed);
        for kind in TRAFFIC_LIGHTS {
            if let Some(btn) = win.standardWindowButton(kind) {
                btn.setHidden(!visible);
            }
        }
    });

    // Scoped to this window: the veil window toggles fullscreen throughout a
    // transition and none of that is our business.
    for (name, block) in [
        (
            unsafe { NSWindowWillEnterFullScreenNotification },
            &entering,
        ),
        (unsafe { NSWindowWillExitFullScreenNotification }, &leaving),
        (unsafe { NSWindowDidExitFullScreenNotification }, &left),
    ] {
        let _ = unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(name), Some(ns), None, block)
        };
    }
}

/// Trackpad scroll gesture phase. The DOM has nothing like it: while the
/// fingers rest motionless no `wheel` events arrive at all, so "a pause inside
/// the gesture" is indistinguishable from its end. Hence reading NSEvent.phase
/// directly and telling the frontend whether the fingers are still down —
/// without it, scrubbing resumed playback every time the fingers stopped.
///
/// The monitor is local: it sees the event before delivery to the webview and
/// returns it untouched, so ordinary scrolling keeps working.
pub fn watch_scroll_phase<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventPhase};
    use tauri::Emitter;

    let app = app.clone();
    let handler = RcBlock::new(move |event: core::ptr::NonNull<NSEvent>| -> *mut NSEvent {
        let phase = unsafe { event.as_ref().phase() };
        // Began/Changed/Stationary — fingers on the trackpad. Ended/Canceled —
        // lifted. Inertia after lifting arrives with phase = None and does not
        // count as either.
        let down = phase.contains(NSEventPhase::Began)
            || phase.contains(NSEventPhase::Changed)
            || phase.contains(NSEventPhase::Stationary);
        // `Cancelled` with two Ls is Apple's spelling of the variant, not ours —
        // an identifier from AppKit rather than a word, and the one place in
        // this file that does not follow the project's American spelling.
        let up = phase.contains(NSEventPhase::Ended) || phase.contains(NSEventPhase::Cancelled);
        if down || up {
            let _ = app.emit("frameplayer://scroll-phase", down);
        }
        // Return the event as-is, or scrolling never reaches the webview.
        event.as_ptr()
    });

    // The monitor lives for the whole process; nothing to release.
    let _ = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::ScrollWheel, &handler)
    };
}

// ---- Pinch to resize -------------------------------------------------------
//
// What a trackpad pinch does is decided here and nowhere else, because the web
// view cannot be given the choice. Measured on a bare WKWebView and then with
// the player's own settings (wry leaves `allowsMagnification` off): for every
// `NSEventTypeMagnify` the page gets a Safari `gesturechange` *and* a `wheel`
// with `ctrlKey` whose `deltaY` is −100 × magnification — the ctrl+wheel the
// zoom already reads — and the modifiers do not survive the trip: with ⌥ held,
// every one of 69 native events carried it and none of the 222 wheel events on
// the page did. A local monitor returning nil keeps both from the page; the
// rotate events that ride along with a pinch still reach it, as a
// `gesturechange` with a scale of 1 and no wheel, which nothing reads.
//
// So the monitor is the switch. The frontend says what a pinch means right now
// (`set_pinch_mode`: the setting, and whether the window is the picture's — not
// fullscreen, not maximized, not the mini player, nothing filling the window
// that a resize around the center would draw twice); ⌥ is read off the event;
// and the decision is taken once, at `Began`, for the whole gesture. Passed
// through, the gesture is the zoom it always was.
//
// The frame is `window_shape::pinch_frame` of the start frame and the running
// scale, set in the same main-thread turn the event arrived in — a round trip
// through the web view for each of 120 events a second is the lag IINA does
// not have. The bars go away for the length of it (`say_resizing`), the same
// as for an edge being dragged and for the same reason.

/// The frontend's last word on what a pinch does: the window's size, or
/// nothing of ours (the gesture goes to the web view, which zooms the picture).
static PINCH_RESIZES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// How far from the edges of the screen's visible area a pinch stops, in
/// points — the frontend's `SCREEN_PADDING`, for the same reason (a window
/// flush with the edge looks cropped) and for one more: a window taken to the
/// visible area's largest frame of its shape is *zoomed* as far as AppKit is
/// concerned (`isZoomed`, which tao reports as maximized), and the shell then
/// refuses to drag it. Found by pinching to the limit: the next pinch zoomed the
/// picture and the window would not move.
const PINCH_MARGIN: f64 = 24.0;

/// The gesture under way, decided at its first event. `Theirs` is remembered
/// so that a modifier let go mid-gesture does not turn a zoom into a resize.
#[derive(Clone, Copy)]
enum Pinch {
    Theirs,
    Ours {
        /// The frame the gesture began with; every step is scaled from it.
        start: NSRect,
        /// The product of `1 + magnification` over the events so far.
        scale: f64,
        /// The window has moved: the bars are away and a glide was superseded.
        moved: bool,
    },
}

static PINCH: std::sync::Mutex<Option<Pinch>> = std::sync::Mutex::new(None);

/// What a pinch means from now on. Any thread.
pub fn set_pinch_mode(resize: bool) {
    PINCH_RESIZES.store(resize, std::sync::atomic::Ordering::Relaxed);
}

/// One magnify event of a gesture that is ours: scale the window about the
/// center it started with. Main thread only.
fn pinch_step(window: &tauri::WebviewWindow, pinch: &mut Pinch, magnification: f64, ended: bool) {
    use crate::window_shape::{pinch_frame, say_resizing, supersede_glide, Frame};

    let Pinch::Ours { start, scale, moved } = pinch else {
        return;
    };
    if magnification.is_finite() {
        *scale *= 1.0 + magnification;
    }
    if let Some(ns) = ns_window(window) {
        let rect = |r: NSRect| Frame {
            x: r.origin.x,
            y: r.origin.y,
            w: r.size.width,
            h: r.size.height,
        };
        // The screen the window is mostly on, and its area short of the menu
        // bar, the Dock and the margin — in the coordinates the frame is in.
        let area = ns
            .screen()
            .or_else(|| MainThreadMarker::new().and_then(objc2_app_kit::NSScreen::mainScreen))
            .map(|s| rect(s.visibleFrame()))
            .filter(|a| a.w > 2.0 * PINCH_MARGIN && a.h > 2.0 * PINCH_MARGIN)
            .map(|a| Frame {
                x: a.x + PINCH_MARGIN,
                y: a.y + PINCH_MARGIN,
                w: a.w - 2.0 * PINCH_MARGIN,
                h: a.h - 2.0 * PINCH_MARGIN,
            });
        if let Some(area) = area {
            let min = ns.minSize();
            let pinched = pinch_frame(rect(*start), *scale, (min.width, min.height), area);
            // What the window took, not what was asked: see `Pinched::scale`.
            *scale = pinched.scale;
            let to = pinched.frame;
            let cur = rect(ns.frame());
            // Whole points against whole points; the window's own frame is one
            // already, and a frame it is already at is not sent (see
            // `pinch_frame`).
            if to != cur {
                if !*moved {
                    *moved = true;
                    supersede_glide();
                    say_resizing(true);
                }
                ns.setFrame_display(
                    NSRect::new(NSPoint::new(to.x, to.y), NSSize::new(to.w, to.h)),
                    true,
                );
            }
        }
    }
    if ended && *moved {
        say_resizing(false);
    }
}

/// Watch the trackpad for a pinch, and take it when it is ours. Main thread.
/// Not generic over the runtime like the scroll monitor: this one reaches the
/// window itself, and `ns_window` is written for the one runtime there is.
pub fn watch_pinch(app: &tauri::AppHandle) {
    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventPhase};
    use std::sync::atomic::Ordering;
    use tauri::Manager;

    let app = app.clone();
    let handler = RcBlock::new(move |event: core::ptr::NonNull<NSEvent>| -> *mut NSEvent {
        let e = unsafe { event.as_ref() };
        let phase = e.phase();
        let began = phase.contains(NSEventPhase::Began);
        let ended = phase.contains(NSEventPhase::Ended) || phase.contains(NSEventPhase::Cancelled);
        let mut pinch = PINCH.lock().unwrap_or_else(|e| e.into_inner());
        // Decided at the first event of a gesture — `Began`, or whatever
        // arrives first if that one was missed — and held to its end.
        if began || pinch.is_none() {
            let alt = e.modifierFlags().contains(NSEventModifierFlags::Option);
            // Ours only with a window to scale; without one the gesture is
            // the web view's, as it is with ⌥ or with the frontend saying so.
            let start = (PINCH_RESIZES.load(Ordering::Relaxed) && !alt)
                .then(|| app.get_webview_window("main"))
                .flatten()
                .and_then(|win| ns_window(&win))
                .map(|ns| ns.frame());
            *pinch = Some(match start {
                Some(start) => Pinch::Ours { start, scale: 1.0, moved: false },
                None => Pinch::Theirs,
            });
        }
        let ours = matches!(*pinch, Some(Pinch::Ours { .. }));
        if ours {
            if let (Some(p), Some(win)) = (pinch.as_mut(), app.get_webview_window("main")) {
                pinch_step(&win, p, e.magnification(), ended);
            }
        }
        if ended {
            *pinch = None;
        }
        if ours {
            // Eaten: the web view never sees it, so nothing zooms.
            std::ptr::null_mut()
        } else {
            event.as_ptr()
        }
    });

    // For the life of the process, like the scroll monitor above.
    let _ = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::Magnify, &handler)
    };
}

/// The frontend's last instruction to `set_buttons_visible`, i.e. whether the
/// UI is meant to be on screen at all. Kept because AppKit's own `isHidden` is
/// not a record of that: it shows the buttons itself in fullscreen, so anything
/// restoring visibility afterwards has to restore the *intent*.
static BUTTONS_VISIBLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

// ---- Traffic lights ------------------------------------------------------
//
// The system window buttons are deliberately *not* moved by hand here, and that
// is the whole design. There is no positioning API — they belong to the private
// frame view — so the obvious approach is to set their frames and re-set them
// whenever AppKit puts them back. That was tried at length and it does not work:
//
// * AppKit resets them on every title-bar relayout, and the causes cannot be
//   enumerated (`setTitle` alone did it every single time).
// * Position is only half of it. The ×/−/+ glyphs come from `NSTrackingArea`s
//   that AppKit lays out from its own model of where the traffic light is, never
//   from the buttons' frames — so moved buttons keep the hover region of unmoved
//   ones. Measured: the glyphs lit up 7.2 pt above and left of the buttons, and
//   the zoom button, past the stale rect's right edge, never responded at all.
// * That cannot be repaired. Replacing the tracking area with a correctly
//   positioned copy of itself (rect/options/owner/userInfo are all public) fixes
//   the geometry and kills hover *completely* — AppKit evidently keys on the
//   identity of the area object it created, so an equivalent one is ignored and
//   the glyphs then appear only while a button is held down.
//
// So the buttons stay AppKit's. What we change is the *title bar*: an empty
// `NSToolbar` grows it from 28 to 66 pt, and AppKit then places the buttons
// itself at (19, 33) — the roomier inset a tall title bar wants — and lays out
// their tracking rects to match. Measured to hold in the window, across resizes,
// after `setTitle`, and in fullscreen, where the buttons keep the system's own
// placement and behavior. None of it needs maintaining, and no other code has
// to know the buttons are special.
//
// The one thing to watch: this puts a 66 pt system view over the player's own
// top bar. It does not swallow clicks — measured, anything below 10 pt from the
// top edge hit-tests to the content view, so `data-tauri-drag-region` and the
// HTML top bar keep working — but that follows from `FullSizeContentView` plus a
// transparent, item-less toolbar. Give the toolbar items, or drop
// FullSizeContentView, and it stops being true.

/// Hide/show the system window buttons together with the rest of the UI: once
/// the title bar has faded out on idle, leftover traffic lights look alien.
/// Called from the `window_buttons` command — main thread only.
///
/// Answers with **whether the pointer is on the traffic lights**, which the
/// frontend cannot work out for itself: hovering them stops the webview from
/// receiving mousemove, so its own guess is a latch on the last position the
/// pointer was seen at and stays raised long after the pointer has gone. That
/// answer is what decides whether the cursor may be hidden (`cursorEffect` in
/// chrome.svelte.ts), and it is measured here against the buttons' real frames.
pub fn set_buttons_visible(window: &tauri::WebviewWindow, visible: bool) -> bool {
    let Some(ns) = ns_window(window) else {
        return false;
    };
    // Recording the intent and acting on it are two different things, and the
    // record comes first — it is what the buttons should look like in a normal
    // window, and `toolbar_off_in_fullscreen` replays it after rebuilding the
    // title bar. Every bail-out below skips only the acting.
    BUTTONS_VISIBLE.store(visible, std::sync::atomic::Ordering::Relaxed);
    // In fullscreen the buttons belong to the strip that slides down from the
    // top edge, which appears and goes on the system's terms. Blanking them
    // there on idle emptied a strip the user had deliberately pulled down, and
    // did it while the pointer rested on a control they were about to click.
    // Read once, before the bail-outs: it is the answer as much as a condition,
    // and in fullscreen it is still the truth — the buttons are in the strip the
    // user pulled down, and a pointer resting on them there wants a cursor just
    // as much.
    let on_buttons = pointer_over_buttons(&ns);
    if !visible && ns.styleMask().contains(NSWindowStyleMask::FullScreen) {
        return on_buttons;
    }
    // Cursor over the buttons means the user is working with them: the native
    // window-placement popup (on the green one) is showing, or a tooltip is
    // about to. Hiding them now is not allowed — the popup is anchored to the
    // button and follows it into the corner when it disappears. It is also just
    // the expected macOS behavior.
    if !visible && on_buttons {
        return true;
    }
    // `setHidden` asks for a title-bar relayout, which used to be a problem
    // worth working around; now that AppKit owns the buttons' placement, the
    // relayout simply puts them back where they belong.
    for kind in TRAFFIC_LIGHTS {
        if let Some(btn) = ns.standardWindowButton(kind) {
            btn.setHidden(!visible);
        }
    }
    on_buttons
}

/// Cursor inside the traffic-light area (with a little slack — the popup on the
/// green button appears slightly below it, and moving the cursor towards that
/// popup must not count as leaving the buttons).
fn pointer_over_buttons(ns: &NSWindow) -> bool {
    use objc2_app_kit::NSEvent;

    const PAD: f64 = 6.0;

    let mut area: Option<NSRect> = None;
    for button in TRAFFIC_LIGHTS {
        let Some(btn) = ns.standardWindowButton(button) else {
            continue;
        };
        // The button frame is in its superview's coordinates — convert to window ones.
        let f = btn.convertRect_toView(btn.bounds(), None);
        area = Some(match area {
            None => f,
            Some(a) => {
                let x0 = a.origin.x.min(f.origin.x);
                let y0 = a.origin.y.min(f.origin.y);
                let x1 = (a.origin.x + a.size.width).max(f.origin.x + f.size.width);
                let y1 = (a.origin.y + a.size.height).max(f.origin.y + f.size.height);
                NSRect::new(NSPoint::new(x0, y0), NSSize::new(x1 - x0, y1 - y0))
            }
        });
    }
    let Some(area) = area else {
        return false;
    };

    let mouse = NSEvent::mouseLocation();
    let p = ns.convertPointFromScreen(mouse);
    p.x >= area.origin.x - PAD
        && p.x <= area.origin.x + area.size.width + PAD
        && p.y >= area.origin.y - PAD
        && p.y <= area.origin.y + area.size.height + PAD
}

// ---- Holding the picture's shape -----------------------------------------

/// Hold the content to a shape while the viewer resizes the window, or let it
/// go. Called from the `window_shape_lock` command — main thread only.
///
/// `contentAspectRatio` is AppKit's own constraint and everything about it
/// below is measured, with a probe window driven by posted mouse events:
///
/// * It binds the **live resize and the zoom button**, and nothing else.
///   `setContentSize` with another shape is accepted as given, so setting a
///   ratio does not reshape the window — fitting it is still the frontend's
///   job — and fullscreen fills the screen as it always did, which is why
///   nothing here looks at the fullscreen notifications.
/// * **The ratio outranks the minimum size.** With a minimum of 480×320 and
///   16:9, the window was dragged down to 480×270: AppKit clamps the proposed
///   size per axis and applies the ratio afterwards. So the minimum has to be
///   a size *of this shape* before the ratio goes on, which the frontend sees
///   to (`floorForShape`), through tao, so the minimum keeps one owner.
/// * There is no "none" to set. The ratio and the resize increments are one
///   constraint with two spellings, and writing the increments as a single
///   point is how the ratio comes off (read back: it is then 0×0).
pub fn set_shape_lock(window: &tauri::WebviewWindow, shape: Option<(f64, f64)>) {
    let Some(ns) = ns_window(window) else {
        return;
    };
    match shape {
        Some((w, h)) => ns.setContentAspectRatio(NSSize::new(w, h)),
        None => ns.setResizeIncrements(NSSize::new(1.0, 1.0)),
    }
}

// ---- Which corner a live resize keeps ------------------------------------
//
// What is decided is in `window_shape::keep_corner`; this is how the decision
// gets in. AppKit's live resize sets every frame through the window's own
// `setFrame:display:` (measured: twelve calls for twelve drag events, each one
// proposed afresh from the frame the resize began with), so a frame rewritten
// there is the frame the window takes, and the next proposal does not argue
// with it. Tried on a window of our own first and then in the player, with the
// pointer driven by posted events: by the right edge the top-left corner does
// not move by a point for the length of the drag, by the left or the top one
// only the edge in hand does.
//
// The method is added to tao's window class rather than the window moved to a
// subclass of it. `TaoWindow` overrides three methods and this is not one of
// them, so adding it is an override and nothing of tao's is replaced; changing
// the object's class instead would be a second user of the trick the mini
// player already depends on (`set_float_over_fullscreen`), and the two would
// have to know about each other. It follows that the corner is *not* kept while
// the mini player is on — the window is an NSPanel then, and this is not in
// its table.

/// The frame the live resize under way began with, and what the pointer took
/// hold of where that could be read; `None` between resizes.
static RESIZE_FROM: std::sync::Mutex<Option<(NSRect, Option<crate::window_shape::Held>)>> =
    std::sync::Mutex::new(None);

/// A fullscreen transition is under way, which AppKit also reports as a live
/// resize; see the `WillStartLiveResize` handler.
static FS_TRANSITION: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The window whose corner is kept: the class has other instances (the veil).
static KEPT_WINDOW: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

type SetFrame =
    unsafe extern "C-unwind" fn(&NSWindow, objc2::runtime::Sel, NSRect, objc2::runtime::Bool);

/// `NSWindow`'s own `setFrame:display:`, which ours ends in.
static NEXT_SET_FRAME: std::sync::OnceLock<SetFrame> = std::sync::OnceLock::new();

unsafe extern "C-unwind" fn set_frame_keeping_corner(
    this: &NSWindow,
    cmd: objc2::runtime::Sel,
    frame: NSRect,
    display: objc2::runtime::Bool,
) {
    use crate::window_shape::{keep_corner, UpFrame};
    use std::sync::atomic::Ordering;

    let mut frame = frame;
    let ours = KEPT_WINDOW.load(Ordering::Relaxed) == this as *const NSWindow as usize;
    if ours && this.inLiveResize() {
        let start = *RESIZE_FROM.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((start, held)) = start {
            let up = |r: NSRect| UpFrame {
                x: r.origin.x,
                y: r.origin.y,
                w: r.size.width,
                h: r.size.height,
            };
            let kept = keep_corner(up(start), up(frame), held);
            frame = NSRect::new(NSPoint::new(kept.x, kept.y), NSSize::new(kept.w, kept.h));
        }
    }
    if let Some(next) = NEXT_SET_FRAME.get() {
        unsafe { next(this, cmd, frame, display) };
    }
}

/// How far either side of an edge AppKit takes a press for a resize, and how
/// far into the window a corner reaches. Measured on macOS 26 with events
/// posted to a bare titled, resizable window: a drag resized from 2 pt inside
/// to 2 pt outside an edge (not from 4), and from a corner inset up to 8 pt
/// (not 12). Kept a point wider each: a press taken for a resize that is not
/// one costs the bars a blink, since its mouse-up comes back through the
/// monitor.
const EDGE_BAND: f64 = 3.0;
const CORNER_BAND: f64 = 10.0;

/// Tell the frontend a resize is under way the moment the button goes down on
/// an edge. `WillStartLiveResize` is too late for that: AppKit posts it with the
/// event *after* the press — the first drag, or the release — so a press held
/// still said nothing. The press itself does reach a local monitor at once, and
/// whether it lands on an edge is geometry, as there is no asking AppKit.
fn say_resizing_on_press() {
    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventType, NSWindowStyleMask};
    use std::sync::atomic::Ordering;

    let handler = RcBlock::new(move |event: core::ptr::NonNull<NSEvent>| -> *mut NSEvent {
        let e = unsafe { event.as_ref() };
        // A local monitor runs on the main thread, inside the event loop.
        let window = e.window(unsafe { MainThreadMarker::new_unchecked() });
        let ours = window.as_deref().is_some_and(|w| {
            KEPT_WINDOW.load(Ordering::Relaxed) == w as *const NSWindow as usize
        });
        if !ours {
            return event.as_ptr();
        }
        if e.r#type() == NSEventType::LeftMouseUp {
            // A resize swallows its own mouse-up, and `DidEndLiveResize` says
            // it is over; one that arrives here was a press on the picture.
            crate::window_shape::say_resizing(false);
            return event.as_ptr();
        }
        let window = window.unwrap();
        let mask = window.styleMask();
        if !mask.contains(NSWindowStyleMask::Resizable)
            || mask.contains(NSWindowStyleMask::FullScreen)
            || FS_TRANSITION.load(Ordering::Relaxed)
        {
            return event.as_ptr();
        }
        let size = window.frame().size;
        let at = e.locationInWindow();
        let (dx, dy) = (at.x.min(size.width - at.x), at.y.min(size.height - at.y));
        let near_edge = |d: f64| d.abs() <= EDGE_BAND;
        let within = |d: f64| d >= -EDGE_BAND;
        let edge = (near_edge(dx) && within(dy)) || (near_edge(dy) && within(dx));
        let corner = (0.0..=CORNER_BAND).contains(&dx) && (0.0..=CORNER_BAND).contains(&dy);
        if edge || corner {
            crate::window_shape::say_resizing(true);
        }
        event.as_ptr()
    });
    // For the life of the process, like the other monitors.
    let _ = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(
            NSEventMask::LeftMouseDown | NSEventMask::LeftMouseUp,
            &handler,
        )
    };
}

fn keep_corner_in_live_resize(window: &tauri::WebviewWindow, ns: &NSWindow) {
    use block2::RcBlock;
    use objc2::runtime::AnyObject;
    use objc2::sel;
    use objc2_app_kit::{
        NSEventType, NSWindowDidEndLiveResizeNotification, NSWindowDidEnterFullScreenNotification,
        NSWindowDidExitFullScreenNotification, NSWindowWillEnterFullScreenNotification,
        NSWindowWillExitFullScreenNotification, NSWindowWillStartLiveResizeNotification,
    };
    use objc2_foundation::{NSNotification, NSNotificationCenter};
    use std::sync::atomic::Ordering;

    let class = unsafe { &*(ns as *const NSWindow).cast::<AnyObject>() }.class();
    let sel = sel!(setFrame:display:);
    // Whatever the window's class inherits today. If tao comes to define the
    // method itself, the add below fails and the window resizes as AppKit has
    // it — a regression in feel, never in function.
    let Some(method) = class.superclass().and_then(|s| s.instance_method(sel)) else {
        log_warn("no setFrame:display: to build on; live resize keeps AppKit's corner");
        return;
    };
    let next: SetFrame = unsafe { std::mem::transmute(method.implementation()) };
    let ours: objc2::runtime::Imp =
        unsafe { std::mem::transmute(set_frame_keeping_corner as SetFrame) };
    let added = unsafe {
        objc2::ffi::class_addMethod(
            (class as *const objc2::runtime::AnyClass).cast_mut(),
            sel,
            ours,
            objc2::ffi::method_getTypeEncoding(method),
        )
    };
    if !added.as_bool() {
        log_warn("the window's class has its own setFrame:display:; live resize keeps AppKit's corner");
        return;
    }
    let _ = NEXT_SET_FRAME.set(next);
    crate::window_shape::report_to(window);
    KEPT_WINDOW.store(ns as *const NSWindow as usize, Ordering::Relaxed);
    say_resizing_on_press();

    let center = NSNotificationCenter::defaultCenter();

    let win = ns.retain();
    let starting = RcBlock::new(move |_: core::ptr::NonNull<NSNotification>| {
        // Only a resize the pointer is doing. AppKit also calls a fullscreen
        // transition a live resize, and there the event at hand is whatever
        // came last — the mouse-up of the double click that asked for it, a
        // key, a message — so the corner "held" was read off a click in the
        // middle of the picture, and leaving fullscreen put the window back
        // with its left edge at the screen's. Measured: a window entering
        // from x = 700 came back at x = 0, from every position tried.
        let press = win.currentEvent().filter(|e| {
            matches!(e.r#type(), NSEventType::LeftMouseDown | NSEventType::LeftMouseDragged)
        });
        let Some(press) = press.filter(|_| !FS_TRANSITION.load(Ordering::Relaxed)) else {
            *RESIZE_FROM.lock().unwrap_or_else(|e| e.into_inner()) = None;
            return;
        };
        let frame = win.frame();
        // The press that began it is the event being handled as this is sent.
        let at = press.locationInWindow();
        let held = Some(crate::window_shape::held_axes(
            (frame.size.width, frame.size.height),
            (at.x, at.y),
        ));
        *RESIZE_FROM.lock().unwrap_or_else(|e| e.into_inner()) = Some((frame, held));
        // Said when the button goes down on the edge, before the window has
        // changed: the first frames of a drag are the ones the bars trail
        // worst. A press let go without a drag hides them for as long as it
        // was held, and `ended` brings them straight back.
        crate::window_shape::say_resizing(true);
    });
    let ended = RcBlock::new(move |_: core::ptr::NonNull<NSNotification>| {
        *RESIZE_FROM.lock().unwrap_or_else(|e| e.into_inner()) = None;
        crate::window_shape::say_resizing(false);
    });
    let transition = RcBlock::new(|_: core::ptr::NonNull<NSNotification>| {
        FS_TRANSITION.store(true, Ordering::Relaxed);
    });
    let settled = RcBlock::new(|_: core::ptr::NonNull<NSNotification>| {
        FS_TRANSITION.store(false, Ordering::Relaxed);
    });
    for (name, block) in [
        (unsafe { NSWindowWillEnterFullScreenNotification }, &transition),
        (unsafe { NSWindowWillExitFullScreenNotification }, &transition),
        (unsafe { NSWindowDidEnterFullScreenNotification }, &settled),
        (unsafe { NSWindowDidExitFullScreenNotification }, &settled),
    ] {
        let _ = unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(name), Some(ns), None, block)
        };
    }
    for (name, block) in [
        (unsafe { NSWindowWillStartLiveResizeNotification }, &starting),
        (unsafe { NSWindowDidEndLiveResizeNotification }, &ended),
    ] {
        let _ = unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(name), Some(ns), None, block)
        };
    }
}

// ---- The picture going dark ----------------------------------------------

/// The view mpv draws into: the content view's child that is not the web view.
/// Found by what it is not, because its own class is the embedder's to name
/// (`swift.View` today, over a `MetalLayer`) and the web view's is wry's.
fn video_view(ns: &NSWindow) -> Option<Retained<objc2_app_kit::NSView>> {
    let content = ns.contentView()?;
    let found = content
        .subviews()
        .iter()
        .find(|view| !view.class().name().to_string_lossy().contains("WebView"));
    found
}

/// How much of the picture is showing, 0 to 1. Main thread only.
pub fn video_alpha(window: &tauri::WebviewWindow) -> Option<f64> {
    let ns = ns_window(window)?;
    Some(video_view(&ns)?.alphaValue())
}

/// Show that much of the picture; `false` where there is no view to ask. Main
/// thread only.
///
/// What is under the picture while it is less than whole is the window's own
/// fill, which is `#101016` and not black — the right color for the moment
/// before the web view's first frame, and a visible gray next to the black of
/// a letterbox. So the fill is black for as long as the picture is not whole,
/// and put back the moment it is: by then nothing of it shows.
pub fn set_video_alpha(window: &tauri::WebviewWindow, alpha: f64) -> bool {
    let Some(ns) = ns_window(window) else {
        return false;
    };
    let Some(view) = video_view(&ns) else {
        return false;
    };
    view.setAlphaValue(alpha);
    // Under the fullscreen mask the fill stays black whatever the picture
    // does; the mask puts the right one back when it lifts.
    if !TRANSITION_MASKED.load(std::sync::atomic::Ordering::Relaxed) {
        restore_fill(&ns);
    }
    true
}

/// The fill for what the picture is doing: black while it is less than whole,
/// the window's own color once it is — see `set_video_alpha`.
fn restore_fill(ns: &NSWindow) {
    let dark = video_view(ns).is_some_and(|v| v.alphaValue() < 1.0);
    let fill = if dark {
        objc2_app_kit::NSColor::blackColor()
    } else {
        window_fill()
    };
    ns.setBackgroundColor(Some(&fill));
}

// ---- Masking the fullscreen transition -----------------------------------
//
// AppKit does not animate the window into fullscreen, it animates pictures of
// it: a snapshot of the window as it was, stretched towards the screen, and one
// of the window as it will be, cross-faded in over it. Our window is two
// surfaces that paint on their own schedules — mpv's view underneath, the web
// view over it, each a frame or two behind the window's frame in its own way —
// so the snapshots catch them mid-change. Reported, depending on where the
// window stood before: the picture stretched tall for a moment, and the title
// bar twice, once at the top of the screen and once in the middle of it (the
// "before" picture at the window's old place, the "after" one at the screen's).
//
// Nothing about the snapshots can be steered, so the window gives them nothing
// to catch: the content view (the picture and the web view together) goes to
// alpha 0 over a black window fill *before* the transition starts, and comes
// back after `Did{Enter,Exit}FullScreen`, once both surfaces have had time to
// take the new size. The transition is then a black rectangle growing into the
// screen, which is also what the Windows side does with its veil and shutter.
//
// "Before it starts" is the part that needs care, because the "before" picture
// is taken from what is on the screen, and a change made in the same main-
// thread turn as `toggleFullScreen:` has not reached the screen yet. So:
//
// * the frontend masks first (`window_fullscreen_mask`, which waits for the
//   black to be on screen) and only then asks for fullscreen;
// * every other way in — ⌃⌘F, the menu, tao's `setFullscreen` — reaches
//   `toggleFullScreen:`, which is overridden on tao's window class the way
//   `setFrame:display:` is (see "Which corner a live resize keeps"): when the
//   window is not masked yet it masks and sends the toggle on `MASK_LEAD`
//   later instead;
// * whatever still gets past both (a system path that does not go through
//   `toggleFullScreen:`) is masked on `WillEnter`/`WillExit`, late for the
//   "before" picture but in time for the rest of it.
//
// It must not be able to stay on. A transition that fails says so only to the
// window's delegate (`windowDidFailToEnterFullScreen:` has no notification),
// and the delegate is tao's, so `MASK_CEILING` is what lifts the mask when no
// `Did…` arrives — whether the transition failed or was never started.

/// The mask is down, or on its way down.
static TRANSITION_MASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A toggle was deferred behind the mask and has not been sent yet; further
/// toggles until then are the same request again.
static TOGGLE_DEFERRED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Which mask is the newest, so a fade-in finishing late does not put the
/// window's fill back under a mask that came down after it started.
static MASK_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Which scheduled lift is the newest: a later one replaces an earlier one
/// (the settle after `Did…` replaces the ceiling armed by the mask).
static LIFT_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The window that is masked: tao's class has other instances (the veil).
static MASKED_WINDOW: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The same window as Tauri has it, which is what the timers below come back
/// through. Deliberately not a selector sent to the window with
/// `performSelector:afterDelay:` — tried first, and neither the deferred toggle
/// nor the lift ever arrived, which left the window black for good.
static MASK_HANDLE: std::sync::OnceLock<tauri::WebviewWindow> = std::sync::OnceLock::new();

/// How long the black is given to reach the screen before the transition is
/// started: three frames at 60 Hz, which is a commit plus a composite with a
/// frame to spare.
pub const MASK_LEAD: std::time::Duration = std::time::Duration::from_millis(50);

/// After `Did{Enter,Exit}FullScreen`: the web view relays out a frame or two
/// behind the window and mpv reconfigures its surface for the new size.
const MASK_SETTLE: std::time::Duration = std::time::Duration::from_millis(120);

/// The fade back in, in seconds.
const MASK_FADE: f64 = 0.15;

/// The last way back.
const MASK_CEILING: std::time::Duration = std::time::Duration::from_secs(3);

type ToggleImp =
    unsafe extern "C-unwind" fn(&NSWindow, objc2::runtime::Sel, *mut objc2::runtime::AnyObject);

/// `NSWindow`'s own `toggleFullScreen:`.
static NEXT_TOGGLE: std::sync::OnceLock<ToggleImp> = std::sync::OnceLock::new();

fn is_masked_window(ns: &NSWindow) -> bool {
    MASKED_WINDOW.load(std::sync::atomic::Ordering::Relaxed) == ns as *const NSWindow as usize
}

/// What the mask did, on the dev build's stderr — a transition cannot be
/// stepped through in a debugger, and this one has already failed once in a
/// way only its order of events could explain.
fn mask_trace(what: &str) {
    if cfg!(debug_assertions) {
        eprintln!("[macos_chrome] fullscreen mask: {what}");
    }
}

/// Run `f` on the main thread with the window, `delay` from now.
fn on_main_after(delay: std::time::Duration, f: impl FnOnce(&NSWindow) + Send + 'static) {
    let Some(window) = MASK_HANDLE.get().cloned() else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;
        let win = window.clone();
        let sent = window.run_on_main_thread(move || {
            if let Some(ns) = ns_window(&win) {
                f(&ns);
            }
        });
        if sent.is_err() {
            mask_trace("could not reach the main thread");
        }
    });
}

/// Lift the mask `delay` from now, unless another lift is scheduled after this.
fn lift_after(delay: std::time::Duration) {
    use std::sync::atomic::Ordering;

    let mine = LIFT_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    on_main_after(delay, move |ns| {
        if LIFT_GEN.load(Ordering::SeqCst) == mine {
            lift_transition_mask(ns);
        }
    });
}

/// Take the window's content off the screen. Main thread only.
fn mask_transition(ns: &NSWindow) {
    use objc2_app_kit::{NSAnimatablePropertyContainer, NSAnimationContext};
    use std::sync::atomic::Ordering;

    mask_trace("down");
    MASK_GEN.fetch_add(1, Ordering::SeqCst);
    TRANSITION_MASKED.store(true, Ordering::Relaxed);
    ns.setBackgroundColor(Some(&objc2_app_kit::NSColor::blackColor()));
    if let Some(content) = ns.contentView() {
        // Through the animator with no duration, so a fade-in still running
        // from the previous transition is replaced rather than left to finish
        // over the black.
        let changes = block2::RcBlock::new(move |ctx: core::ptr::NonNull<NSAnimationContext>| {
            unsafe { ctx.as_ref() }.setDuration(0.0);
            content.animator().setAlphaValue(0.0);
        });
        NSAnimationContext::runAnimationGroup(&changes);
    }
    frame_chrome_shown(ns, false);
    lift_after(MASK_CEILING);
}

/// The part of the window AppKit draws itself, which the content view's alpha
/// does not reach and the transition's snapshots carry: the title bar with the
/// traffic lights (seen in the zoom as a strip wider than the window, and as
/// buttons on the black), and the light rim along the window's edge, which is
/// drawn with its shadow. Main thread only.
///
/// The title bar goes by its container's alpha, not by hiding the buttons: the
/// buttons' `isHidden` is already three people's business (`enter_fullscreen`,
/// the fullscreen notifications and `set_buttons_visible`), and an alpha on
/// the view above them leaves every one of those as it was.
fn frame_chrome_shown(ns: &NSWindow, shown: bool) {
    let alpha = if shown { 1.0 } else { 0.0 };
    if let Some(bar) = titlebar_container(ns) {
        bar.setAlphaValue(alpha);
    }
    ns.setHasShadow(shown);
}

/// The view holding the title bar and its buttons: `NSTitlebarContainerView`,
/// two levels above the close button. Found through the button because it is
/// AppKit's private view, and checked by name so a different hierarchy in a
/// later macOS hides nothing rather than the wrong thing.
fn titlebar_container(ns: &NSWindow) -> Option<Retained<objc2_app_kit::NSView>> {
    let button = ns.standardWindowButton(NSWindowButton::CloseButton)?;
    // `superview` is unsafe because the view does not retain its superview;
    // the window's own hierarchy keeps every one of these alive, and each is
    // retained as it is handed back.
    let mut view = unsafe { button.superview() };
    while let Some(v) = view {
        if v.class().name().to_string_lossy().contains("TitlebarContainer") {
            return Some(v);
        }
        view = unsafe { v.superview() };
    }
    None
}

/// Put the content back, fading. Main thread only.
fn lift_transition_mask(ns: &NSWindow) {
    use objc2_app_kit::{NSAnimatablePropertyContainer, NSAnimationContext};
    use std::sync::atomic::Ordering;

    if !TRANSITION_MASKED.swap(false, Ordering::Relaxed) {
        return;
    }
    mask_trace("up");
    frame_chrome_shown(ns, true);
    let Some(content) = ns.contentView() else {
        restore_fill(ns);
        return;
    };
    let mask = MASK_GEN.load(Ordering::SeqCst);
    let changes = block2::RcBlock::new(move |ctx: core::ptr::NonNull<NSAnimationContext>| {
        unsafe { ctx.as_ref() }.setDuration(MASK_FADE);
        content.animator().setAlphaValue(1.0);
    });
    let win = ns.retain();
    let done = block2::RcBlock::new(move || {
        if MASK_GEN.load(Ordering::SeqCst) == mask && !TRANSITION_MASKED.load(Ordering::Relaxed) {
            restore_fill(&win);
        }
    });
    NSAnimationContext::runAnimationGroup_completionHandler(&changes, Some(&done));
}

/// Mask ahead of a transition the caller is about to start, for the
/// `window_fullscreen_mask` command. `true` when it came down now and the
/// caller has to give it `MASK_LEAD` to reach the screen. Main thread only.
pub fn mask_fullscreen_transition(window: &tauri::WebviewWindow) -> bool {
    use std::sync::atomic::Ordering;

    let Some(ns) = ns_window(window) else {
        return false;
    };
    if !is_masked_window(&ns) || TRANSITION_MASKED.load(Ordering::Relaxed) {
        return false;
    }
    mask_transition(&ns);
    true
}

unsafe extern "C-unwind" fn toggle_behind_mask(
    this: &NSWindow,
    cmd: objc2::runtime::Sel,
    sender: *mut objc2::runtime::AnyObject,
) {
    use std::sync::atomic::Ordering;

    let Some(next) = NEXT_TOGGLE.get() else {
        return;
    };
    if !is_masked_window(this) || TRANSITION_MASKED.load(Ordering::Relaxed) {
        if is_masked_window(this) {
            mask_trace("toggle");
        }
        unsafe { next(this, cmd, sender) };
        return;
    }
    if TOGGLE_DEFERRED.swap(true, Ordering::Relaxed) {
        return;
    }
    mask_trace("toggle deferred behind the mask");
    mask_transition(this);
    on_main_after(MASK_LEAD, |ns| {
        if TOGGLE_DEFERRED.swap(false, Ordering::Relaxed) {
            // Through the override again, which the mask now lets pass — and
            // which is a plain `toggleFullScreen:` if the window has changed
            // its class meanwhile (the mini player's NSPanel).
            ns.toggleFullScreen(None);
        }
    });
}

fn mask_fullscreen_transitions(window: &tauri::WebviewWindow, ns: &NSWindow) {
    use block2::RcBlock;
    use objc2::runtime::AnyObject;
    use objc2::sel;
    use objc2_app_kit::{
        NSWindowDidEnterFullScreenNotification, NSWindowDidExitFullScreenNotification,
        NSWindowWillEnterFullScreenNotification, NSWindowWillExitFullScreenNotification,
    };
    use objc2_foundation::{NSNotification, NSNotificationCenter};
    use std::sync::atomic::Ordering;

    if MASK_HANDLE.set(window.clone()).is_err() {
        return;
    }
    MASKED_WINDOW.store(ns as *const NSWindow as usize, Ordering::Relaxed);

    let class = unsafe { &*(ns as *const NSWindow).cast::<AnyObject>() }.class();
    let toggle = sel!(toggleFullScreen:);
    match class.superclass().and_then(|s| s.instance_method(toggle)) {
        Some(method) => {
            let next: ToggleImp = unsafe { std::mem::transmute(method.implementation()) };
            let _ = NEXT_TOGGLE.set(next);
            let ours: objc2::runtime::Imp =
                unsafe { std::mem::transmute(toggle_behind_mask as ToggleImp) };
            let added = unsafe {
                objc2::ffi::class_addMethod(
                    (class as *const objc2::runtime::AnyClass).cast_mut(),
                    toggle,
                    ours,
                    objc2::ffi::method_getTypeEncoding(method),
                )
            };
            if !added.as_bool() {
                // tao has come to define it itself: the notifications below
                // still mask, only later.
                log_warn("the window's class has its own toggleFullScreen:; masking from WillEnter only");
            }
        }
        None => log_warn("no toggleFullScreen: to build on; masking from WillEnter only"),
    }

    let center = NSNotificationCenter::defaultCenter();

    let win = ns.retain();
    let starting = RcBlock::new(move |_: core::ptr::NonNull<NSNotification>| {
        mask_trace("will change");
        if !TRANSITION_MASKED.load(Ordering::Relaxed) {
            mask_transition(&win);
        }
    });
    let finished = RcBlock::new(move |_: core::ptr::NonNull<NSNotification>| {
        mask_trace("did change");
        lift_after(MASK_SETTLE);
    });
    for (name, block) in [
        (unsafe { NSWindowWillEnterFullScreenNotification }, &starting),
        (unsafe { NSWindowWillExitFullScreenNotification }, &starting),
        (unsafe { NSWindowDidEnterFullScreenNotification }, &finished),
        (unsafe { NSWindowDidExitFullScreenNotification }, &finished),
    ] {
        let _ = unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(name), Some(ns), None, block)
        };
    }
}

// The window's frame in the coordinates the frontend has: top-left origin,
// y growing downwards, physical pixels. The conversion is tao's own, to the
// letter — `outerPosition` comes out of `CGDisplayPixelsHigh` of the main
// display and the window's own scale factor, so what goes back in has to be
// un-done with the same two numbers or the window lands a menu bar away from
// where it was asked to be.
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGMainDisplayID() -> u32;
    fn CGDisplayPixelsHigh(display: u32) -> usize;
}

fn main_display_height() -> f64 {
    unsafe { CGDisplayPixelsHigh(CGMainDisplayID()) as f64 }
}

/// Where the window is. Main thread only.
pub fn frame(window: &tauri::WebviewWindow) -> Option<crate::window_shape::Frame> {
    let ns = ns_window(window)?;
    let f = ns.frame();
    let scale = ns.backingScaleFactor();
    Some(crate::window_shape::Frame {
        x: f.origin.x * scale,
        y: (main_display_height() - (f.origin.y + f.size.height)) * scale,
        w: f.size.width * scale,
        h: f.size.height * scale,
    })
}

/// Put the window at a frame, position and size in one change. Main thread
/// only. `setFrame:display:` answers to neither the minimum size nor the
/// aspect ratio — both bind what the viewer does, not what the window is told.
pub fn set_frame(window: &tauri::WebviewWindow, frame: crate::window_shape::Frame) {
    let Some(ns) = ns_window(window) else {
        return;
    };
    let scale = ns.backingScaleFactor();
    if scale <= 0.0 {
        return;
    }
    let (w, h) = (frame.w / scale, frame.h / scale);
    let origin = NSPoint::new(frame.x / scale, main_display_height() - frame.y / scale - h);
    ns.setFrame_display(NSRect::new(origin, NSSize::new(w, h)), true);
}

// ---- Floating over other apps' fullscreen --------------------------------
//
// Floating over ANOTHER application's fullscreen space is the one thing a
// system PiP panel does that an always-on-top window does not, and what it
// takes is not what it looks like. Measured, with a probe app that put labeled
// windows in every configuration and screenshotted them over a fullscreen
// TextEdit:
//
// * The window LEVEL is irrelevant. A plain NSWindow does not make it onto the
//   space at floating (3), modal (8), main-menu (24), status (25), pop-up (101)
//   or even the shielding level — the last of which is above everything the
//   system itself draws.
// * `CanJoinAllSpaces | FullScreenAuxiliary` is necessary and NOT sufficient.
//   Every window in the probe had it.
// * What decides it is the window's CLASS together with the style mask: an
//   NSPanel carrying `NonactivatingPanel` floats, at level 3. Either half alone
//   fails — a plain NSWindow with the bit does not float, and an NSPanel
//   without it does not either.
// * The class can be changed after the fact: `object_setClass` to NSPanel on an
//   already-created window works and is reversible (both verified over a
//   fullscreen app, including that the restored window stops floating).
//
// That last point is what makes this possible at all, because the window is
// tao's `TaoWindow`, an NSWindow subclass we do not create and cannot ask for a
// panel. Promoting sideways to NSPanel is only sound because NSPanel adds
// nothing to NSWindow — measured, `class_getInstanceSize` is 520 for both and
// NSPanel declares zero ivars of its own — so the object's memory layout is
// untouched and only the method table changes. The size is re-checked at
// runtime anyway; if a future macOS makes NSPanel bigger, this does nothing
// instead of corrupting the window.
//
// What is given up while promoted is `TaoWindow`'s three overrides:
// `canBecomeKeyWindow`/`canBecomeMainWindow` (which return tao's `focusable`
// ivar) and `sendEvent:` (which implements `movableByWindowBackground`). The
// first two fall back to NSPanel's defaults — key yes, main no — and the third
// is not in use, since `apply()` turns `movableByWindowBackground` off and
// dragging goes through `data-tauri-drag-region`.
//
// One rule the promotion imposes on the rest of the app: tao's `focusable`
// ivar cannot be found while the window is an NSPanel, and tao reads it by
// name, so **nothing may call `set_focusable` while mini is on** — it would
// panic inside tao rather than fail quietly. Nothing does today.

/// What the window looked like before it was promoted, so it can be put back
/// exactly. `None` = not promoted.
struct SavedWindow {
    class: &'static objc2::runtime::AnyClass,
    style: NSWindowStyleMask,
    behavior: NSWindowCollectionBehavior,
}

static SAVED: std::sync::Mutex<Option<SavedWindow>> = std::sync::Mutex::new(None);

/// Let the window sit over *other applications'* fullscreen spaces, for the
/// mini player. See the section comment above for what was measured.
///
/// Main thread only.
pub fn set_float_over_fullscreen(window: &tauri::WebviewWindow, on: bool) {
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::ClassType;
    use objc2_app_kit::NSPanel;

    let Some(ns) = ns_window(window) else {
        log_warn("could not obtain the NSWindow");
        return;
    };
    let obj = (&*ns as *const NSWindow).cast::<AnyObject>().cast_mut();
    let mut saved = SAVED.lock().unwrap_or_else(|e| e.into_inner());

    if on {
        if saved.is_some() {
            return; // Already promoted; re-entering would save the panel state.
        }
        let was = unsafe { &*obj }.class();
        let panel = NSPanel::class();
        // The object was allocated at its own class's size. Shrinking the
        // declared size is harmless, growing it is memory corruption — so this
        // is the one thing that must hold, and it is checked rather than
        // assumed. (objc2's safe `set_class` asserts the sizes are *equal*,
        // which a subclass with an ivar would fail, hence the raw call.)
        if panel.instance_size() > was.instance_size() {
            log_warn("NSPanel is larger than the window's class — not promoting");
            return;
        }
        let style = ns.styleMask();
        let behavior = ns.collectionBehavior();

        unsafe { objc2::ffi::object_setClass(obj, panel as *const AnyClass) };
        ns.setStyleMask(style | NSWindowStyleMask::NonactivatingPanel);
        // Panels hide themselves when the app deactivates, which is the exact
        // opposite of the point here — and switching to the fullscreen app IS a
        // deactivation.
        ns.setHidesOnDeactivate(false);

        let mut wanted = behavior;
        // The flags sit in different exclusivity groups; AppKit takes at most
        // one from each, so what shares a group has to come off. Notably
        // `FullScreenPrimary` — the flag that makes a window fullscreen-capable
        // at all — shares a group with `FullScreenAuxiliary`.
        wanted.remove(
            NSWindowCollectionBehavior::MoveToActiveSpace
                | NSWindowCollectionBehavior::FullScreenPrimary
                | NSWindowCollectionBehavior::FullScreenNone
                | NSWindowCollectionBehavior::FullScreenAllowsTiling,
        );
        wanted.insert(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                // A 420 px window is not a tile candidate. Windows that cannot
                // go fullscreen themselves can still be dropped into somebody
                // else's fullscreen tile, which is not what floating above it
                // is meant to mean.
                | NSWindowCollectionBehavior::FullScreenDisallowsTiling,
        );
        ns.setCollectionBehavior(wanted);

        *saved = Some(SavedWindow {
            class: was,
            style,
            behavior,
        });
    } else if let Some(prev) = saved.take() {
        // Exactly the reverse order, and the class last: everything above was
        // set through NSPanel's implementation.
        ns.setCollectionBehavior(prev.behavior);
        ns.setStyleMask(prev.style);
        unsafe { objc2::ffi::object_setClass(obj, prev.class as *const AnyClass) };
    }
}

/// EDR state of the display the window is on: (capable, headroom right now).
///
/// macOS has no "HDR on/off" switch like Windows — instead the screen has
/// *brightness headroom* (EDR headroom) above SDR white. It is dynamic: on
/// built-in displays it drops to almost 1.0 when SDR brightness is at maximum
/// and grows again when it is turned down. Hence:
///
/// * `supported` — potential headroom, a property of the display itself;
/// * `enabled` — headroom available right now (this is what decides whether
///   real HDR output happens or mpv tone-maps anyway).
///
/// Main thread only.
pub fn edr_headroom(window: &tauri::WebviewWindow) -> Option<(bool, bool)> {
    let ns = ns_window(window)?;
    // The window may not be on screen yet (hidden until the first show) — then
    // use the main screen, which is the same display in the vast majority of cases.
    let screen = ns.screen().or_else(|| {
        let mtm = MainThreadMarker::new()?;
        objc2_app_kit::NSScreen::mainScreen(mtm)
    })?;
    let potential = screen.maximumPotentialExtendedDynamicRangeColorComponentValue();
    let current = screen.maximumExtendedDynamicRangeColorComponentValue();
    // A strict "> 1.0" would catch float noise; 1% counts as real headroom.
    Some((potential > 1.01, current > 1.01))
}

fn log_warn(msg: &str) {
    eprintln!("[macos_chrome] {msg}");
}
