//! Moving the file being watched to the system trash.
//!
//! **To the trash and nowhere else.** A key that deletes what is on screen is a
//! key that will be pressed by mistake, and the one thing that makes that
//! survivable is that the file can be dragged back out of the Trash or the
//! Recycle Bin. So every path below either recycles or fails; none of them may
//! fall back to deleting, and the frontend reports the failure instead.
//!
//! No crate for it: both halves are one system call away through bindings the
//! player already links (`objc2-foundation` on macOS, `windows-sys` on
//! Windows), and the `trash` crate's macOS default — asking Finder over Apple
//! Events — would raise an Automation permission prompt the first time the key
//! is pressed.

use std::path::Path;

use crate::thumb_service::{release_file, ThumbState};

/// Move one local file to the trash.
///
/// Refuses anything that is not an existing regular file at an absolute path:
/// the frontend already keeps URLs and torrent streams away from here, and this
/// is the second lock on the same door — a relative path would resolve against
/// whatever the process's working directory happens to be.
///
/// The storyboard decoder is let go of the file first. It keeps its own handle
/// open for as long as the file is the current one, and on Windows an open
/// handle without `FILE_SHARE_DELETE` is what makes the Recycle Bin refuse.
#[tauri::command]
pub async fn trash_file(state: tauri::State<'_, ThumbState>, path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.is_absolute() {
        return Err("not an absolute path".into());
    }
    let meta = std::fs::metadata(p).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a file".into());
    }
    release_file(&state, &path);
    move_to_trash(p)
}

#[cfg(target_os = "macos")]
fn move_to_trash(path: &Path) -> Result<(), String> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};

    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    // Fails, rather than deleting, on a volume with no trash (a network share,
    // some external disks) — which is exactly the behaviour wanted.
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, None)
        .map_err(|e| e.localizedDescription().to_string())
}

#[cfg(windows)]
fn move_to_trash(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumePathNameW};
    use windows_sys::Win32::System::WindowsProgramming::DRIVE_FIXED;
    use windows_sys::Win32::UI::Shell::{
        SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
        FOF_WANTNUKEWARNING, FO_DELETE, SHFILEOPSTRUCTW,
    };

    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();

    // `FOF_ALLOWUNDO` means "recycle if you can", and where there is no Recycle
    // Bin — a network share, a USB stick — the shell deletes outright, silently
    // under `FOF_NOCONFIRMATION`. Only fixed drives carry one, so anything else
    // is refused before the shell is asked at all.
    let mut root = vec![0u16; 1024];
    let mut z = wide.clone();
    z.push(0);
    let ok = unsafe { GetVolumePathNameW(z.as_ptr(), root.as_mut_ptr(), root.len() as u32) };
    if ok == 0 || unsafe { GetDriveTypeW(root.as_ptr()) } != DRIVE_FIXED {
        return Err("no recycle bin on this drive".into());
    }

    // `pFrom` is a list: each path NUL-terminated and the list closed by a
    // second NUL. A single terminator reads whatever follows in memory as more
    // paths to delete.
    let mut from = wide;
    from.extend([0, 0]);
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        // `FOF_WANTNUKEWARNING` overrides `FOF_NOCONFIRMATION` for the one case
        // left on a fixed drive — the bin switched off, or the file too big for
        // it — so the system asks instead of destroying the file unannounced.
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_WANTNUKEWARNING | FOF_NOERRORUI | FOF_SILENT)
            as u16,
        ..Default::default()
    };
    let code = unsafe { SHFileOperationW(&mut op) };
    if code != 0 {
        return Err(format!("SHFileOperation failed: {code:#x}"));
    }
    if op.fAnyOperationsAborted != 0 {
        return Err("cancelled".into());
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", windows)))]
fn move_to_trash(_path: &Path) -> Result<(), String> {
    Err("not supported on this platform".into())
}
