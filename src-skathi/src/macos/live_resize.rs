//! A live resize the pointer is doing, reported to the page as version 1's
//! `frameplayer://window-resizing` (macos_chrome.rs `keep_corner_in_live_resize`): the interface's
//! `chrome.resizing`, which takes every bar and floating surface away for the length of the drag and
//! brings them back on release.
//!
//! Skathi reports no live resize, so the page kept its bars up through one, and their work cost the
//! frames: with the interface shown a 1080p resize ran at 96–107 fps with frames over 33 ms, against
//! 113–117 and none with it hidden (measured).
//!
//! AppKit also calls a fullscreen transition a live resize; as in version 1, only a resize that a
//! press began counts, and none while a fullscreen transition is under way.

use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use block2::RcBlock;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};

use crate::sys::{on_main, Id};

#[link(name = "AppKit", kind = "framework")]
extern "C" {
    static NSWindowWillStartLiveResizeNotification: Id;
    static NSWindowDidEndLiveResizeNotification: Id;
    static NSWindowWillEnterFullScreenNotification: Id;
    static NSWindowWillExitFullScreenNotification: Id;
    static NSWindowDidEnterFullScreenNotification: Id;
    static NSWindowDidExitFullScreenNotification: Id;
}

/// NSEventTypeLeftMouseDown, NSEventTypeLeftMouseDragged.
const LEFT_DOWN: usize = 1;
const LEFT_DRAGGED: usize = 6;
/// NSWindowStyleMaskResizable.
const RESIZABLE: usize = 1 << 3;

static TRANSITION: AtomicBool = AtomicBool::new(false);
static RESIZING: AtomicBool = AtomicBool::new(false);

pub type Report = Arc<dyn Fn(bool) + Send + Sync>;

/// Observes every resizable window of the app; `report` hears each change of state.
pub fn watch(report: Report) {
    on_main(Duration::ZERO, move || unsafe {
        let center: Id = msg_send![class!(NSNotificationCenter), defaultCenter];
        let observe = |name: Id, block: &RcBlock<dyn Fn(NonNull<AnyObject>)>| {
            let token: Id = msg_send![center, addObserverForName: name, object: std::ptr::null_mut::<AnyObject>(),
                queue: std::ptr::null_mut::<AnyObject>(), usingBlock: &**block];
            // Kept for the life of the app.
            let _: Id = msg_send![token, retain];
        };
        let say = {
            let report = report.clone();
            move |on: bool| {
                if RESIZING.swap(on, Ordering::Relaxed) != on {
                    report(on);
                }
            }
        };
        let start: RcBlock<dyn Fn(NonNull<AnyObject>)> = RcBlock::new({
            let say = say.clone();
            move |note: NonNull<AnyObject>| {
                let window: Id = msg_send![note.as_ptr(), object];
                let mask: usize = msg_send![window, styleMask];
                if mask & RESIZABLE == 0 || TRANSITION.load(Ordering::Relaxed) {
                    return;
                }
                // The press that began it is the event being handled as this is sent.
                let event: Id = msg_send![window, currentEvent];
                if event.is_null() {
                    return;
                }
                let kind: usize = msg_send![event, type];
                if kind == LEFT_DOWN || kind == LEFT_DRAGGED {
                    say(true);
                }
            }
        });
        let end: RcBlock<dyn Fn(NonNull<AnyObject>)> = RcBlock::new({
            let say = say.clone();
            move |_| say(false)
        });
        // Through a fullscreen transition (fullscreen.rs `mode`, not the fade) mpv's drawable keeps
        // a size and Core Animation scales the live layer (child.rs `freeze`); the stub hides the
        // interface (`data-fs-anim`). The drawable keeps the size it had before the transition.
        let anim = crate::fullscreen::mode() != crate::fullscreen::Mode::Fade;
        let enter: RcBlock<dyn Fn(NonNull<AnyObject>)> = RcBlock::new({
            move |_| {
                TRANSITION.store(true, Ordering::Relaxed);
                if anim {
                    crate::child::freeze(false);
                }
            }
        });
        let exit: RcBlock<dyn Fn(NonNull<AnyObject>)> = RcBlock::new({
            move |_| {
                TRANSITION.store(true, Ordering::Relaxed);
                if anim {
                    crate::child::freeze(true);
                }
            }
        });
        let settled: RcBlock<dyn Fn(NonNull<AnyObject>)> = RcBlock::new({
            move |_| {
                TRANSITION.store(false, Ordering::Relaxed);
                if anim {
                    crate::child::thaw();
                }
            }
        });
        observe(NSWindowWillStartLiveResizeNotification, &start);
        observe(NSWindowDidEndLiveResizeNotification, &end);
        observe(NSWindowWillEnterFullScreenNotification, &enter);
        observe(NSWindowWillExitFullScreenNotification, &exit);
        observe(NSWindowDidEnterFullScreenNotification, &settled);
        observe(NSWindowDidExitFullScreenNotification, &settled);
    });
}
