//! Quitting stops the picture and the sound at once, then tears down (quit.m).
//!
//! mpv was destroyed only in `run_on_main_thread`'s exit callback (app.rs), which runs after
//! Skathi's whole quit: the page's `pagehide`, the JS runtime's end, storage and IndexedDB flushes.
//! The window went first and mpv kept playing until then. Now the first sign of a quit pauses mpv
//! and hides its layer, on the main thread, before any of that runs.

use std::ffi::c_int;
use std::sync::OnceLock;
use std::time::Duration;

use crate::sys::on_main;

extern "C" {
    fn fpui_quit_install(halt: extern "C" fn()) -> c_int;
}

static HALT: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

extern "C" fn halt() {
    if let Some(h) = HALT.get() {
        h();
    }
}

/// `stop` runs once, on the main thread, at the first quit or close of the main window.
pub fn install(stop: Box<dyn Fn() + Send + Sync>) {
    let _ = HALT.set(stop);
    // gpui sets its delegate as the app starts; setup may run before it.
    std::thread::spawn(|| {
        for _ in 0..50 {
            if on_main(Duration::from_secs(2), || unsafe { fpui_quit_install(halt) }) == Some(1) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        eprintln!("[fpui quit] no application delegate to install on");
    });
}
