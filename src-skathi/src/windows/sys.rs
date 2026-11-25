//! The Windows calls behind the player's small commands: the open dialog, the clipboard, the
//! Recycle Bin, the mouse. The macOS port's `sys.rs` also holds AppKit probes (window, panel,
//! synthetic events, Now Playing read-back); those have no Windows side and answer `null` in
//! `main.rs`.

use std::os::windows::ffi::OsStrExt;

use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, GetDoubleClickTime, VK_LBUTTON, VK_RBUTTON};
use windows::Win32::UI::Shell::{
    FileOpenDialog, IFileOpenDialog, IShellItem, SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOS_ALLOWMULTISELECT,
    FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FO_DELETE, SHFILEOPSTRUCTW, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetSystemMetrics, SM_SWAPBUTTON};

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

/// The system's file dialog (`plugin:dialog|open`); empty when cancelled. Runs on a thread of its
/// own with a single-threaded apartment, as the shell's dialog wants; the caller is a task thread.
pub fn open_panel(multiple: bool, directories: bool) -> Vec<String> {
    std::thread::spawn(move || unsafe {
        if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
            return Vec::new();
        }
        let picked = (|| -> windows::core::Result<Vec<String>> {
            let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            let mut options = dialog.GetOptions()? | FOS_FORCEFILESYSTEM;
            if multiple {
                options |= FOS_ALLOWMULTISELECT;
            }
            if directories {
                options |= FOS_PICKFOLDERS;
            }
            dialog.SetOptions(options)?;
            // Owned by whatever window is in front (ours, when a click opened it), so it is modal to it.
            let owner = GetForegroundWindow();
            dialog.Show(Some(owner))?;
            let items = dialog.GetResults()?;
            let mut out = Vec::new();
            for i in 0..items.GetCount()? {
                let item: IShellItem = items.GetItemAt(i)?;
                let name: PWSTR = item.GetDisplayName(SIGDN_FILESYSPATH)?;
                out.push(name.to_string().unwrap_or_default());
                CoTaskMemFree(Some(name.0 as *const _));
            }
            Ok(out)
        })();
        CoUninitialize();
        picked.unwrap_or_default()
    })
    .join()
    .unwrap_or_default()
}

/// The PNG mpv wrote, on the clipboard: as a DIB (what every program pastes) and as "PNG" (what
/// keeps the bytes). The player decodes to RGBA for arboard; this is the same result by hand.
pub fn clipboard_png(bytes: Vec<u8>) -> Result<(), String> {
    let rgba = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).map_err(|e| e.to_string())?.to_rgba8();
    let (w, h) = (rgba.width() as i32, rgba.height() as i32);
    // BITMAPINFOHEADER (40 bytes), 32-bit BGRA, bottom-up.
    let mut dib = Vec::with_capacity(40 + rgba.len());
    dib.extend_from_slice(&40u32.to_le_bytes());
    dib.extend_from_slice(&w.to_le_bytes());
    dib.extend_from_slice(&h.to_le_bytes());
    dib.extend_from_slice(&1u16.to_le_bytes());
    dib.extend_from_slice(&32u16.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    dib.extend_from_slice(&(rgba.len() as u32).to_le_bytes());
    dib.extend_from_slice(&[0u8; 16]);
    for row in rgba.rows().rev() {
        for p in row {
            dib.extend_from_slice(&[p.0[2], p.0[1], p.0[0], p.0[3]]);
        }
    }
    unsafe {
        let global = |data: &[u8]| -> Result<HGLOBAL, String> {
            let g = GlobalAlloc(GMEM_MOVEABLE, data.len()).map_err(|e| e.to_string())?;
            let p = GlobalLock(g) as *mut u8;
            if p.is_null() {
                let _ = GlobalFree(Some(g));
                return Err("GlobalLock failed".into());
            }
            std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
            let _ = GlobalUnlock(g);
            Ok(g)
        };
        OpenClipboard(Some(HWND::default())).map_err(|e| format!("OpenClipboard: {e}"))?;
        let result = (|| -> Result<(), String> {
            EmptyClipboard().map_err(|e| e.to_string())?;
            const CF_DIB: u32 = 8;
            // The clipboard owns a block it accepted; one it refused is ours to free.
            let g = global(&dib)?;
            if let Err(e) = SetClipboardData(CF_DIB, Some(HANDLE(g.0))) {
                let _ = GlobalFree(Some(g));
                return Err(format!("SetClipboardData(DIB): {e}"));
            }
            let png = RegisterClipboardFormatW(w!("PNG"));
            if png != 0 {
                let g = global(&bytes)?;
                if SetClipboardData(png, Some(HANDLE(g.0))).is_err() {
                    let _ = GlobalFree(Some(g));
                }
            }
            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
}

/// To the Recycle Bin or an error, never a delete: `FOF_ALLOWUNDO` without it being possible
/// (a network share, a full bin) fails rather than erasing, because `FOF_NOCONFIRMATION` only
/// answers the "send to the bin?" question.
pub fn trash(path: &str) -> Result<(), String> {
    // A double-NUL-terminated list of one.
    let mut from = wide(path);
    from.push(0);
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT).0 as u16,
        ..Default::default()
    };
    let code = unsafe { SHFileOperationW(&mut op) };
    if code != 0 {
        return Err(format!("the file was not moved to the Recycle Bin (error {code:#x})"));
    }
    if op.fAnyOperationsAborted.as_bool() {
        return Err("the move to the Recycle Bin was cancelled".into());
    }
    Ok(())
}

/// Whether the primary mouse button is down right now (the physical one, whichever side it is on).
pub fn primary_button_down() -> bool {
    unsafe {
        let key = if GetSystemMetrics(SM_SWAPBUTTON) != 0 { VK_RBUTTON } else { VK_LBUTTON };
        (GetAsyncKeyState(key.0 as i32) as u16 & 0x8000) != 0
    }
}

pub fn double_click_ms() -> u32 {
    unsafe { GetDoubleClickTime() }
}

/// The page's resize strips (`ResizeEdges.svelte`): the system's sizing loop for one edge, as a press on
/// the native border would start it. Skathi's `appWindow.startResizeDragging()` only logs "not
/// implemented" (its prelude, at a88f6683), so the strips showed the resize cursor 5 px inside the
/// window and swallowed the press; only the invisible border outside the window resized.
///
/// `WM_SYSCOMMAND` with `SC_SIZE | WMSZ_*` is what `DefWindowProc` itself sends for a press on `HTLEFT`
/// and the rest, and gpui passes it on to `DefWindowProc`. It is **posted**: the call comes from the
/// page, inside Skathi's frame draw, and a modal loop entered there freezes the window for the whole
/// drag (skathi#110). A press that ended before the message is handled starts nothing (the loop runs
/// only while the button is held, the same check gpui makes for a window move).
pub fn start_resize(hwnd: isize, direction: &str) -> bool {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SC_SIZE, WM_SYSCOMMAND};
    // WMSZ_LEFT … WMSZ_BOTTOMRIGHT.
    let edge = match direction {
        "West" => 1,
        "East" => 2,
        "North" => 3,
        "NorthWest" => 4,
        "NorthEast" => 5,
        "South" => 6,
        "SouthWest" => 7,
        "SouthEast" => 8,
        _ => return false,
    };
    if hwnd == 0 || !primary_button_down() {
        return false;
    }
    unsafe {
        // gpui holds the capture from the press; the sizing loop takes its own.
        let _ = ReleaseCapture();
        PostMessageW(Some(HWND(hwnd as *mut _)), WM_SYSCOMMAND, WPARAM(SC_SIZE as usize | edge), LPARAM(0)).is_ok()
    }
}

/// v1's `window_shape::say_resizing` on Windows: the page's `frameplayer://window-resizing` (through
/// the module's `resizing` event), so the bars step aside while an edge is dragged, as in version 1.
/// `true` on a press on a sizing edge (the native border) or the first `WM_SIZING` (the page's strips,
/// whose loop `start_resize` starts with `SC_SIZE`), `false` on `WM_EXITSIZEMOVE` or when the press's
/// loop returns. A move sends no `WM_SIZING`, so it says nothing.
static RESIZE_UI: std::sync::OnceLock<skathi::Handle> = std::sync::OnceLock::new();
static RESIZING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn say_resizing(on: bool) {
    if RESIZING.swap(on, std::sync::atomic::Ordering::Relaxed) == on {
        return;
    }
    if let Some(ui) = RESIZE_UI.get() {
        let _ = ui.emit_to("mpv", "resizing", &serde_json::json!({ "on": on }));
    }
}

/// The shape the page holds the window to while an edge is dragged (`window_shape_lock`, the same ratio Skathi's
/// `setAspectRatio` is given): content width over height, as f64 bits; 0 when nothing is held.
static ASPECT_HOLD: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn set_aspect_hold(ratio: f64) {
    let r = if ratio.is_finite() && ratio > 0.0 { ratio } else { 0.0 };
    ASPECT_HOLD.store(r.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// A corner dragged with the shape held: which side leads is decided here, before Skathi's `handle_sizing` fits
/// the rect. Skathi chooses between the width and the height by comparing the rect the
/// pointer asks for with the window's current size, which its own previous answer already snapped to the shape;
/// on a corner drag that answer is past the pointer on the side that did not lead, so the next message leads with
/// the other side and the window swings between a wide and a tall fit on every step (measured with a scripted
/// sizing loop: the lead changed on 162 of 180 messages, the edge moved against the pointer 160 times, up to 177 px off
/// it; seen as the picture doubling and the edge jumping). The rule here depends on the pointer and the shape
/// only: the side whose fit reaches the pointer on both axes leads, so the window grows over the pointer and
/// follows it without turning back. The other side of the rect is set to the window's current size, which makes
/// Skathi's comparison pick the leading side; its fit (with the minimum and maximum) does the rest. An edge (not a
/// corner) has only one side and is left alone.
unsafe fn corner_lead(hwnd: HWND, edge: u32, rect: &mut windows::Win32::Foundation::RECT) {
    use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowRect, WMSZ_BOTTOMLEFT, WMSZ_BOTTOMRIGHT, WMSZ_TOPLEFT, WMSZ_TOPRIGHT};
    let ratio = f64::from_bits(ASPECT_HOLD.load(std::sync::atomic::Ordering::Relaxed));
    if ratio <= 0.0 || !matches!(edge, WMSZ_TOPLEFT | WMSZ_TOPRIGHT | WMSZ_BOTTOMLEFT | WMSZ_BOTTOMRIGHT) {
        return;
    }
    let (mut frame, mut client) = (windows::Win32::Foundation::RECT::default(), windows::Win32::Foundation::RECT::default());
    if GetWindowRect(hwnd, &mut frame).is_err() || GetClientRect(hwnd, &mut client).is_err() {
        return;
    }
    let (fw, fh) = (frame.right - frame.left, frame.bottom - frame.top);
    let (bw, bh) = (fw - client.right, fh - client.bottom);
    let (w, h) = ((rect.right - rect.left - bw) as f64, (rect.bottom - rect.top - bh) as f64);
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let left = matches!(edge, WMSZ_TOPLEFT | WMSZ_BOTTOMLEFT);
    let top = matches!(edge, WMSZ_TOPLEFT | WMSZ_TOPRIGHT);
    if w / h >= ratio {
        // The width leads: the height it gives is at least the pointer's. The height asked for becomes the current.
        if top {
            rect.top = rect.bottom - fh;
        } else {
            rect.bottom = rect.top + fh;
        }
    } else if left {
        rect.left = rect.right - fw;
    } else {
        rect.right = rect.left + fw;
    }
}

unsafe extern "system" fn resizing_proc(hwnd: HWND, msg: u32, wparam: windows::Win32::Foundation::WPARAM, lparam: windows::Win32::Foundation::LPARAM, _id: usize, _data: usize) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{HTBOTTOMRIGHT, HTLEFT, WM_EXITSIZEMOVE, WM_NCDESTROY, WM_NCLBUTTONDOWN, WM_SIZING};
    match msg {
        WM_NCLBUTTONDOWN if (HTLEFT as usize..=HTBOTTOMRIGHT as usize).contains(&wparam.0) => {
            say_resizing(true);
            // SAFETY: this message, handed on unchanged from inside the subclass procedure.
            let r = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            say_resizing(false);
            return r;
        }
        WM_SIZING => {
            say_resizing(true);
            // SAFETY: WM_SIZING's lparam is the RECT being dragged, valid for the message.
            let rect = unsafe { &mut *(lparam.0 as *mut windows::Win32::Foundation::RECT) };
            // SAFETY: on the window's thread, with the message's own rect.
            unsafe { corner_lead(hwnd, wparam.0 as u32, rect) };
            // SAFETY: handed on, the rect adjusted only by `corner_lead`.
            return unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
        }
        WM_EXITSIZEMOVE => {
            say_resizing(false);
        }
        // The window has moved or changed size: FPUI_FS_ANIM's landing after an animated exit (mpv.rs `fs_landed`).
        windows::Win32::UI::WindowsAndMessaging::WM_WINDOWPOSCHANGED => {
            if let Some(f) = WINDOW_MOVED.get() {
                f();
            }
        }
        WM_NCDESTROY => {
            // SAFETY: removing this procedure from the window it was installed on.
            let _ = unsafe { RemoveWindowSubclass(hwnd, Some(resizing_proc), RESIZING_SUBCLASS) };
        }
        _ => {}
    }
    // SAFETY: as above.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

const RESIZING_SUBCLASS: usize = 0x4650_5253; // "FPRS"

/// Called on the window's thread after each `WM_WINDOWPOSCHANGED` (set once, before the subclass).
pub type WindowMoved = Box<dyn Fn() + Send + Sync>;
static WINDOW_MOVED: std::sync::OnceLock<WindowMoved> = std::sync::OnceLock::new();

pub fn on_window_moved(f: WindowMoved) {
    let _ = WINDOW_MOVED.set(f);
}

/// Put the subclass on the window. It has to run on the thread that owns the window (Skathi's
/// `setup`); `false` when the system refused it.
pub fn watch_resizing(hwnd: isize, ui: skathi::Handle) -> bool {
    use windows::Win32::UI::Shell::SetWindowSubclass;
    let _ = RESIZE_UI.set(ui);
    // SAFETY: `resizing_proc` has the SUBCLASSPROC signature and removes itself on WM_NCDESTROY.
    unsafe { SetWindowSubclass(HWND(hwnd as *mut _), Some(resizing_proc), RESIZING_SUBCLASS, 0) }.as_bool()
}
