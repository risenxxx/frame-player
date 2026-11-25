//! macOS glue: the IOSurfaces mpv renders into, their MTLSharedEvents, the main queue, and the
//! open panel, the clipboard, the Trash and the mouse state.

use std::ffi::{c_char, c_void, CStr};
use std::sync::mpsc;
use std::time::Duration;

use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};

pub type Id = *mut AnyObject;

#[link(name = "IOSurface", kind = "framework")]
extern "C" {
    static kIOSurfaceWidth: *const c_void;
    static kIOSurfaceHeight: *const c_void;
    static kIOSurfaceBytesPerElement: *const c_void;
    static kIOSurfacePixelFormat: *const c_void;
    fn IOSurfaceCreate(properties: *const c_void) -> *mut c_void;
}

#[link(name = "Metal", kind = "framework")]
extern "C" {
    fn MTLCreateSystemDefaultDevice() -> Id;
}

extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_async_f(queue: *const c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
    fn CFRelease(cf: *const c_void);
}

/// `'RGhA'`: 64-bit RGBA, IEEE half floats (kCVPixelFormatType_64RGBAHalf).
const RGHA: u32 = 0x5247_6841;
/// `'BGRA'`: 32-bit BGRA, 8 bits per channel (the 8-bit picture and overlay of an SDR file).
const BGRA: u32 = 0x4247_5241;

unsafe fn number(value: i64) -> Id {
    msg_send![class!(NSNumber), numberWithLongLong: value]
}

/// An IOSurface mpv renders into and Skathi copies from.
pub struct IoSurface(pub *mut c_void);

unsafe impl Send for IoSurface {}
unsafe impl Sync for IoSurface {}

impl IoSurface {
    pub fn rgba_half(width: u32, height: u32) -> Self {
        Self::make(width, height, 8, RGHA)
    }

    pub fn bgra(width: u32, height: u32) -> Self {
        Self::make(width, height, 4, BGRA)
    }

    fn make(width: u32, height: u32, bpe: i64, format: u32) -> Self {
        unsafe {
            let keys = [kIOSurfaceWidth, kIOSurfaceHeight, kIOSurfaceBytesPerElement, kIOSurfacePixelFormat];
            let values = [number(width as i64), number(height as i64), number(bpe), number(format as i64)];
            let dict: Id = msg_send![
                class!(NSDictionary),
                dictionaryWithObjects: values.as_ptr(),
                forKeys: keys.as_ptr(),
                count: 4usize
            ];
            let surface = IOSurfaceCreate(dict.cast());
            assert!(!surface.is_null(), "IOSurfaceCreate failed");
            Self(surface)
        }
    }
}

impl Drop for IoSurface {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) }
    }
}

/// Logs the system default MTLDevice's `currentAllocatedSize` every `every` (stderr).
pub fn log_metal_allocated(every: Duration) {
    std::thread::spawn(move || unsafe {
        let device = MTLCreateSystemDefaultDevice() as usize;
        let t0 = std::time::Instant::now();
        loop {
            let n: usize = msg_send![device as Id, currentAllocatedSize];
            eprintln!("[fpui mtl] t={:.3}s currentAllocatedSize {:.1} MiB", t0.elapsed().as_secs_f64(), n as f64 / 1048576.0);
            std::thread::sleep(every);
        }
    });
}

/// An `MTLSharedEvent` made on the system default device, signalled from the CPU.
pub struct SharedEvent(pub Id);

unsafe impl Send for SharedEvent {}
unsafe impl Sync for SharedEvent {}

impl SharedEvent {
    pub fn new() -> Self {
        unsafe {
            static DEVICE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
            let device = *DEVICE.get_or_init(|| MTLCreateSystemDefaultDevice() as usize) as Id;
            let event: Id = msg_send![device, newSharedEvent];
            assert!(!event.is_null(), "newSharedEvent failed");
            Self(event)
        }
    }

    pub fn signal(&self, value: u64) {
        unsafe { msg_send![self.0, setSignaledValue: value] }
    }

    /// Blocks until the event reaches `value` (macOS 12+).
    pub fn wait(&self, value: u64, timeout: Duration) -> bool {
        let ok: Bool = unsafe { msg_send![self.0, waitUntilSignaledValue: value, timeoutMS: timeout.as_millis() as u64] };
        ok.as_bool()
    }
}

impl Drop for SharedEvent {
    fn drop(&mut self) {
        unsafe {
            let _: () = msg_send![self.0, release];
        }
    }
}

/// Runs `work` on the main thread's dispatch queue and returns its result, or `None` after
/// `timeout` (the main thread is busy or gone).
pub fn on_main<T: Send + 'static>(timeout: Duration, work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    extern "C" fn run(context: *mut c_void) {
        let work = unsafe { Box::from_raw(context as *mut Box<dyn FnOnce() + Send>) };
        work();
    }
    let (tx, rx) = mpsc::channel();
    let boxed: Box<dyn FnOnce() + Send> = Box::new(move || {
        let _ = tx.send(work());
    });
    unsafe {
        dispatch_async_f(&raw const _dispatch_main_q, Box::into_raw(Box::new(boxed)).cast(), run);
    }
    rx.recv_timeout(timeout).ok()
}

#[link(name = "AppKit", kind = "framework")]
extern "C" {
    static NSAppearanceNameDarkAqua: Id;
}

/// The dark appearance for the whole app, as version 1 gives its window (macos_chrome.rs): AppKit
/// draws what it draws itself (the traffic lights, the frame's edge, the open panel) to the
/// appearance, not to our fill. Under the light one an inactive window's lights are a light grey
/// composited `plusD` (plus-darker) and come out black on the dark title bar, nearly invisible;
/// under the dark one they are a dark grey composited `plusL`, as in version 1 (measured).
/// Skathi's `appWindow.theme` sets Windows' dark mode only. Nothing in the page
/// reads `prefers-color-scheme`. Asynchronous: from the main thread it lands on the next turn.
pub fn dark_appearance() {
    on_main(Duration::ZERO, || unsafe {
        let dark: Id = msg_send![class!(NSAppearance), appearanceNamed: NSAppearanceNameDarkAqua];
        let app: Id = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, setAppearance: dark];
    });
}

unsafe fn ns_string(string: Id) -> String {
    if string.is_null() {
        return String::new();
    }
    let utf8: *const c_char = msg_send![string, UTF8String];
    if utf8.is_null() {
        String::new()
    } else {
        CStr::from_ptr(utf8).to_string_lossy().into_owned()
    }
}

/// The system open panel, modal, on the main thread. Empty when cancelled.
pub fn open_panel(multiple: bool, directories: bool) -> Vec<String> {
    on_main(Duration::from_secs(24 * 3600), move || unsafe {
        let panel: Id = msg_send![class!(NSOpenPanel), openPanel];
        let _: () = msg_send![panel, setCanChooseFiles: Bool::new(!directories)];
        let _: () = msg_send![panel, setCanChooseDirectories: Bool::new(directories)];
        let _: () = msg_send![panel, setAllowsMultipleSelection: Bool::new(multiple)];
        let answer: isize = msg_send![panel, runModal];
        if answer != 1 {
            return Vec::new();
        }
        let urls: Id = msg_send![panel, URLs];
        let n: usize = msg_send![urls, count];
        (0..n)
            .map(|i| {
                let url: Id = msg_send![urls, objectAtIndex: i];
                let path: Id = msg_send![url, path];
                ns_string(path)
            })
            .collect()
    })
    .unwrap_or_default()
}

unsafe fn ns_from(s: &str) -> Id {
    let c = std::ffi::CString::new(s).unwrap_or_default();
    msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()]
}

/// PNG bytes onto the general pasteboard (`public.png`), on the main thread.
pub fn clipboard_png(bytes: Vec<u8>) -> Result<(), String> {
    on_main(Duration::from_secs(5), move || unsafe {
        let board: Id = msg_send![class!(NSPasteboard), generalPasteboard];
        let _: isize = msg_send![board, clearContents];
        let data: Id = msg_send![class!(NSData), dataWithBytes: bytes.as_ptr().cast::<c_void>(), length: bytes.len()];
        let ok: Bool = msg_send![board, setData: data, forType: ns_from("public.png")];
        if ok.as_bool() { Ok(()) } else { Err("the clipboard refused the image".to_string()) }
    })
    .unwrap_or_else(|| Err("the main thread did not answer".to_string()))
}

/// `-[NSFileManager trashItemAtURL:resultingItemURL:error:]`: to the Trash or an error, never a delete.
pub fn trash(path: &str) -> Result<(), String> {
    unsafe {
        let url: Id = msg_send![class!(NSURL), fileURLWithPath: ns_from(path)];
        let manager: Id = msg_send![class!(NSFileManager), defaultManager];
        let mut error: Id = std::ptr::null_mut();
        let ok: Bool = msg_send![manager, trashItemAtURL: url, resultingItemURL: std::ptr::null_mut::<Id>(), error: &mut error];
        if ok.as_bool() {
            return Ok(());
        }
        if error.is_null() {
            return Err("the file was not moved to the Trash".to_string());
        }
        let text: Id = msg_send![error, localizedDescription];
        Err(ns_string(text))
    }
}

/// Whether the primary mouse button is down now (`+[NSEvent pressedMouseButtons]`, bit 0).
pub fn primary_button_down() -> bool {
    let buttons: usize = unsafe { msg_send![class!(NSEvent), pressedMouseButtons] };
    buttons & 1 != 0
}

/// The system double-click interval in ms (`+[NSEvent doubleClickInterval]`), 500 if unreadable.
pub fn double_click_ms() -> u32 {
    let secs: f64 = unsafe { msg_send![class!(NSEvent), doubleClickInterval] };
    if secs.is_finite() && secs > 0.0 { (secs * 1000.0).round() as u32 } else { 500 }
}
