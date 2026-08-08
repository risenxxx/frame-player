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
    let bg = objc2_app_kit::NSColor::colorWithSRGBRed_green_blue_alpha(
        16.0 / 255.0,
        16.0 / 255.0,
        22.0 / 255.0,
        1.0,
    );
    ns.setBackgroundColor(Some(&bg));

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
}

/// Grow the title bar so AppKit gives the window buttons a roomier inset.
fn install_toolbar(ns: &NSWindow, mtm: MainThreadMarker) {
    let toolbar = NSToolbar::new(mtm);
    toolbar.setAllowsUserCustomization(false);
    ns.setToolbar(Some(&toolbar));
    ns.setToolbarStyle(NSWindowToolbarStyle::Unified);
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
        win.setToolbar(None);
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
pub fn set_buttons_visible(window: &tauri::WebviewWindow, visible: bool) {
    let Some(ns) = ns_window(window) else {
        return;
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
    if !visible && ns.styleMask().contains(NSWindowStyleMask::FullScreen) {
        return;
    }
    // Cursor over the buttons means the user is working with them: the native
    // window-placement popup (on the green one) is showing, or a tooltip is
    // about to. Hiding them now is not allowed — the popup is anchored to the
    // button and follows it into the corner when it disappears. It is also just
    // the expected macOS behavior.
    if !visible && pointer_over_buttons(&ns) {
        return;
    }
    // `setHidden` asks for a title-bar relayout, which used to be a problem
    // worth working around; now that AppKit owns the buttons' placement, the
    // relayout simply puts them back where they belong.
    for kind in TRAFFIC_LIGHTS {
        if let Some(btn) = ns.standardWindowButton(kind) {
            btn.setHidden(!visible);
        }
    }
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
