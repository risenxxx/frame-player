//! Fullscreen on macOS: one path for every way in, and the transition it takes.
//!
//! Every way in reaches the page's `toggleFullscreen`/`exitFullscreen` (chrome.svelte.ts): the
//! double-click, F, Esc, the player's menu item, and now also the green button and AppKit's own menu
//! item and shortcut, whose `toggleFullScreen:` (fullscreen.m) is handed to the page as the `fullscreen`
//! event. The page hides the interface and asks the stub, which asks Skathi; that call has no sender and
//! goes through.
//!
//! `FPUI_FS_ANIM`: `0` Skathi's fade through black; `1` AppKit's animation both ways with mpv's
//! drawable held (child.rs `freeze`); `custom`, the default, the live window's frame animated by us
//! both ways through AppKit's custom-window hooks, with no snapshot, as bare mpv does.

use std::ffi::{c_char, c_int, c_void, CStr};
use std::sync::OnceLock;
use std::time::Duration;

use objc2::runtime::AnyObject;
use objc2::{class, msg_send};

use crate::sys::on_main;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Fade,
    System,
    Custom,
}

pub fn mode() -> Mode {
    static MODE: OnceLock<Mode> = OnceLock::new();
    *MODE.get_or_init(|| match std::env::var("FPUI_FS_ANIM").as_deref() {
        Ok("0") => Mode::Fade,
        Ok("1") => Mode::System,
        Ok("custom") => Mode::Custom,
        _ => DEFAULT,
    })
}

/// What an installed beta does.
const DEFAULT: Mode = Mode::Custom;

extern "C" {
    fn object_getClass(obj: *const AnyObject) -> *const c_void;
    fn class_getName(cls: *const c_void) -> *const c_char;
    fn fpui_fs_install(window: *mut c_void, request: extern "C" fn(), step: extern "C" fn(), custom: c_int);
    fn fpui_fs_want(on: c_int);
    fn fpui_fs_target() -> c_int;
}

static REQUEST: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Each step of the custom animation: mpv's view follows the window in the same frame.
extern "C" fn step() {
    crate::child::follow();
}

extern "C" fn request() {
    if let Some(r) = REQUEST.get() {
        r();
    }
}

/// The page's wanted state (the stub's `setFullscreen`), applied now or after the transition under way
/// (fullscreen.m `fpui_fs_want`). Main thread, asynchronously.
pub fn want(on: bool) {
    on_main(Duration::ZERO, move || unsafe { fpui_fs_want(c_int::from(on)) });
}

/// The state the window is heading to while a transition runs or a request waits, `None` when idle
/// (fullscreen.m `fpui_fs_target`). Read from the calling thread: the page's JavaScript runs on the main
/// thread, where a hop to it would wait for itself (it timed out, measured); the state is written on the
/// main thread only.
pub fn target() -> Option<bool> {
    match unsafe { fpui_fs_target() } {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

/// `request` runs when the green button or AppKit's menu asks for fullscreen (main thread). The main
/// window is the app's first `GPUIWindow` (Skathi's window class); setup runs once it exists.
pub fn install(request_fn: Box<dyn Fn() + Send + Sync>) {
    let _ = REQUEST.set(request_fn);
    // Setup runs before the window is in `NSApp.windows`: look for it a few times.
    std::thread::spawn(|| {
        for _ in 0..50 {
            let done = on_main(Duration::from_secs(2), || unsafe {
                let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
                let windows: *mut AnyObject = msg_send![app, windows];
                let n: usize = msg_send![windows, count];
                for i in 0..n {
                    let w: *mut AnyObject = msg_send![windows, objectAtIndex: i];
                    let name = CStr::from_ptr(class_getName(object_getClass(w)));
                    if name.to_bytes().ends_with(b"GPUIWindow") {
                        fpui_fs_install(w.cast(), request, step, c_int::from(mode() == Mode::Custom));
                        return true;
                    }
                }
                false
            });
            if done == Some(true) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        eprintln!("[fpui fs] no GPUIWindow to install on");
    });
}
