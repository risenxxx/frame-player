//! Loading libmpv on Windows ourselves, before the plugin does, so that a
//! failure has a reason attached to it.
//!
//! The chain is plugin → `lib\libmpv-wrapper.dll` → `lib\libmpv-2.dll`, and the
//! last link is a `LoadLibraryExW` inside the wrapper. When it fails the
//! wrapper writes the reason to stderr — which a GUI process does not have —
//! and returns NULL, and the plugin turns that into "Failed to create mpv
//! instance": a message that names neither the file nor the cause, under which
//! the old hint sent a viewer to create `src-tauri\lib` by hand.
//!
//! The cause that was actually met is **`vulkan-1.dll`**. Upstream's Windows
//! builds of mpv (libmpv-2.dll and mpv.exe alike) import it directly rather
//! than loading it on demand, and Windows does not ship it: a GPU driver puts it
//! in System32. A virtual machine, the Basic Display Adapter, or a GPU too old
//! for a Vulkan driver (Intel before Skylake, NVIDIA Fermi) has none, and on
//! those machines libmpv cannot be loaded at all — although mpv on Windows draws
//! through D3D11 and never needs Vulkan to work, only to be *loadable*.
//!
//! So the loader is bundled (`lib\vulkan-1.dll`, Khronos' own, from LunarG's
//! runtime) and used **only when the system has none**: a driver's loader is
//! the one that knows that driver, so it wins whenever it exists. The bundled
//! copy cannot sit beside the executable for the same reason — the application
//! directory is searched before System32 and it would always win. Instead it is
//! loaded by full path before libmpv is, and the import resolves to it because
//! the loader reuses a module already in the process by its name.
//!
//! libmpv is then loaded here too. If that fails the system's own error and
//! the direct imports that cannot be found are kept for the frontend
//! (`libmpv_load_error`), which says so in place of the plugin's message. If it
//! succeeds, the wrapper's later load only takes another reference.

#![deny(clippy::undocumented_unsafe_blocks)]

use serde::Serialize;

/// Why libmpv-2.dll could not be loaded, as the frontend shows it.
#[derive(Clone, Debug, Serialize)]
pub struct LoadFailure {
    /// The Win32 error: 126 for a missing dependency, 127 for a missing entry
    /// point, 193 for a damaged or wrong-architecture file.
    pub code: u32,
    /// The system's own wording of it, in the system's language.
    pub message: String,
    /// The direct imports the system cannot find. Empty when the cause is not
    /// a missing file — or is one level further down, which this does not chase.
    pub missing: Vec<String>,
}

#[cfg(windows)]
static FAILURE: std::sync::OnceLock<Option<LoadFailure>> = std::sync::OnceLock::new();

/// Load the Vulkan loader (if the system lacks one) and libmpv. Called once,
/// before the builder, so it has run before the frontend asks mpv to start.
pub fn preload() {
    #[cfg(windows)]
    {
        let failure = imp::preload();
        if let Some(f) = &failure {
            eprintln!(
                "[libmpv] could not load libmpv-2.dll: {} ({}){}",
                f.message,
                f.code,
                if f.missing.is_empty() {
                    String::new()
                } else {
                    format!(", missing {}", f.missing.join(", "))
                }
            );
        }
        let _ = FAILURE.set(failure);
    }
}

/// What `preload` met, for the frontend's "could not start mpv" screen.
/// Always `None` outside Windows, and on Windows when libmpv loaded.
#[tauri::command]
pub fn libmpv_load_error() -> Option<LoadFailure> {
    #[cfg(windows)]
    {
        FAILURE.get().cloned().flatten()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// The DLL names a PE image imports directly, in table order. `None` when the
/// bytes are not a PE image this can read. Pure, so it is tested here on every
/// platform rather than only where it runs.
#[cfg_attr(not(windows), allow(dead_code))]
fn import_names(pe: &[u8]) -> Option<Vec<String>> {
    let u16_at = |o: usize| pe.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |o: usize| pe.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));

    if pe.get(0..2)? != b"MZ" {
        return None;
    }
    let nt = u32_at(0x3c)? as usize;
    if pe.get(nt..nt + 4)? != b"PE\0\0" {
        return None;
    }
    let coff = nt + 4;
    let sections = u16_at(coff + 2)? as usize;
    let opt_size = u16_at(coff + 16)? as usize;
    let opt = coff + 20;
    // The data directories start at 96 in PE32 and 112 in PE32+; the import
    // table is the second of them.
    let dirs = match u16_at(opt)? {
        0x10b => opt + 96,
        0x20b => opt + 112,
        _ => return None,
    };
    let import_rva = u32_at(dirs + 8)?;
    if import_rva == 0 {
        return Some(Vec::new());
    }

    let table = opt + opt_size;
    let to_offset = |rva: u32| -> Option<usize> {
        (0..sections).find_map(|i| {
            let s = table + i * 40;
            let size = u32_at(s + 8)?.max(u32_at(s + 16)?);
            let va = u32_at(s + 12)?;
            let raw = u32_at(s + 20)?;
            (rva >= va && rva < va.checked_add(size)?).then(|| (rva - va + raw) as usize)
        })
    };

    let mut names = Vec::new();
    let mut desc = to_offset(import_rva)?;
    // Each descriptor is 20 bytes and the table ends with an all-zero one;
    // the name's RVA is at 12.
    loop {
        let name_rva = u32_at(desc + 12)?;
        if name_rva == 0 {
            break;
        }
        let start = to_offset(name_rva)?;
        let len = pe.get(start..)?.iter().position(|&b| b == 0)?;
        names.push(String::from_utf8_lossy(&pe[start..start + len]).into_owned());
        desc += 20;
    }
    Some(names)
}

#[cfg(windows)]
mod imp {
    use super::{import_names, LoadFailure};
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use windows_sys::Win32::Foundation::{GetLastError, LocalFree};
    use windows_sys::Win32::System::Diagnostics::Debug::{
        FormatMessageW, FORMAT_MESSAGE_ALLOCATE_BUFFER, FORMAT_MESSAGE_FROM_SYSTEM,
        FORMAT_MESSAGE_IGNORE_INSERTS,
    };
    use windows_sys::Win32::System::LibraryLoader::{
        GetModuleHandleW, LoadLibraryExW, LOAD_LIBRARY_AS_DATAFILE, LOAD_LIBRARY_SEARCH_SYSTEM32,
    };

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    /// Load and keep for the life of the process; the handle is never freed.
    fn load(name: &std::ffi::OsStr, flags: u32) -> Result<(), u32> {
        let w = wide(name);
        // SAFETY: a NUL-terminated UTF-16 name alive for the call, the
        // reserved file handle null as required. What the library runs on
        // load is its own: loading it is the point.
        let h = unsafe { LoadLibraryExW(w.as_ptr(), std::ptr::null_mut(), flags) };
        if h.is_null() {
            // SAFETY: reads this thread's last error, nothing else.
            Err(unsafe { GetLastError() })
        } else {
            Ok(())
        }
    }

    /// Where the plugin looks for the wrapper, and therefore where the wrapper
    /// looks for libmpv: `<exe>\lib` first, then beside the executable.
    fn lib_dir() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let dir = exe.parent()?;
        [dir.join("lib"), dir.to_path_buf()]
            .into_iter()
            .find(|d| d.join("libmpv-2.dll").exists())
    }

    pub fn preload() -> Option<LoadFailure> {
        let Some(dir) = lib_dir() else {
            // ERROR_FILE_NOT_FOUND: the wrapper will not find it either.
            return Some(LoadFailure {
                code: 2,
                message: system_message(2),
                missing: vec!["libmpv-2.dll".into()],
            });
        };

        // System32 only: the application directory and the current directory
        // are exactly where a loader should not be picked up from.
        let vulkan = std::ffi::OsStr::new("vulkan-1.dll");
        match load(vulkan, LOAD_LIBRARY_SEARCH_SYSTEM32) {
            Ok(()) => eprintln!("[libmpv] using the system's vulkan-1.dll"),
            Err(_) => {
                let bundled = dir.join("vulkan-1.dll");
                match load(bundled.as_os_str(), 0) {
                    Ok(()) => eprintln!("[libmpv] no system vulkan-1.dll, using the bundled one"),
                    Err(code) => eprintln!("[libmpv] no vulkan-1.dll at all ({code})"),
                }
            }
        }

        let libmpv = dir.join("libmpv-2.dll");
        let code = load(libmpv.as_os_str(), 0).err()?;
        Some(LoadFailure {
            code,
            message: system_message(code),
            missing: missing_imports(&libmpv),
        })
    }

    /// The direct imports of `image` that cannot be found where Windows would
    /// look for them. Opened as data files, so nothing found runs any code.
    /// API sets (`api-ms-win-*`, `ext-ms-*`) are virtual names the loader
    /// resolves by itself and are not files to find.
    fn missing_imports(image: &Path) -> Vec<String> {
        let Ok(bytes) = std::fs::read(image) else {
            return Vec::new();
        };
        import_names(&bytes)
            .unwrap_or_default()
            .into_iter()
            .filter(|n| {
                let l = n.to_ascii_lowercase();
                !l.starts_with("api-ms-win-") && !l.starts_with("ext-ms-")
            })
            .filter(|n| {
                let name = std::ffi::OsStr::new(n);
                // Already in the process — the bundled vulkan-1.dll, loaded
                // from a folder no search would visit — counts as found.
                let wide_name = wide(name);
                // SAFETY: a NUL-terminated UTF-16 name alive for the call; the
                // handle is only compared with null, never used.
                let loaded = !unsafe { GetModuleHandleW(wide_name.as_ptr()) }.is_null();
                !loaded && load(name, LOAD_LIBRARY_AS_DATAFILE).is_err()
            })
            .collect()
    }

    fn system_message(code: u32) -> String {
        let mut buf: *mut u16 = std::ptr::null_mut();
        // SAFETY: with ALLOCATE_BUFFER the "buffer" argument is where the
        // system writes the address of one it allocates, so it is `&mut buf`
        // cast as the API asks; no source, no inserts.
        let len = unsafe {
            FormatMessageW(
                FORMAT_MESSAGE_ALLOCATE_BUFFER
                    | FORMAT_MESSAGE_FROM_SYSTEM
                    | FORMAT_MESSAGE_IGNORE_INSERTS,
                std::ptr::null(),
                code,
                0,
                &mut buf as *mut *mut u16 as *mut u16,
                0,
                std::ptr::null(),
            )
        };
        if len == 0 || buf.is_null() {
            return format!("error {code}");
        }
        // SAFETY: the system allocated `len` UTF-16 units at `buf` and nothing
        // frees them before the `LocalFree` below, after the copy.
        let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(buf, len as usize) });
        // SAFETY: the buffer ALLOCATE_BUFFER handed us, which is ours to free
        // with LocalFree, once.
        unsafe { LocalFree(buf as _) };
        // The frontend puts the code after it, so not the closing full stop.
        text.trim().trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::import_names;

    /// A minimal PE32+ image: one section at RVA 0x1000, file offset 0x200,
    /// holding an import table that names `dlls`.
    fn image(dlls: &[&str]) -> Vec<u8> {
        let mut pe = vec![0u8; 0x400];
        pe[0..2].copy_from_slice(b"MZ");
        pe[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        pe[0x40..0x44].copy_from_slice(b"PE\0\0");
        let coff = 0x44;
        pe[coff + 2..coff + 4].copy_from_slice(&1u16.to_le_bytes());
        pe[coff + 16..coff + 18].copy_from_slice(&240u16.to_le_bytes());
        let opt = coff + 20;
        pe[opt..opt + 2].copy_from_slice(&0x20bu16.to_le_bytes());
        pe[opt + 112 + 8..opt + 112 + 12].copy_from_slice(&0x1000u32.to_le_bytes());
        let sec = opt + 240;
        pe[sec + 8..sec + 12].copy_from_slice(&0x200u32.to_le_bytes());
        pe[sec + 12..sec + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        pe[sec + 16..sec + 20].copy_from_slice(&0x200u32.to_le_bytes());
        pe[sec + 20..sec + 24].copy_from_slice(&0x200u32.to_le_bytes());

        // Descriptors first (plus the terminator), the names after them.
        let mut name_at = 0x200 + 20 * (dlls.len() + 1);
        for (i, d) in dlls.iter().enumerate() {
            let desc = 0x200 + 20 * i;
            let rva = (name_at - 0x200 + 0x1000) as u32;
            pe[desc + 12..desc + 16].copy_from_slice(&rva.to_le_bytes());
            pe[name_at..name_at + d.len()].copy_from_slice(d.as_bytes());
            name_at += d.len() + 1;
        }
        pe
    }

    #[test]
    fn reads_the_import_table() {
        let pe = image(&["KERNEL32.dll", "vulkan-1.dll", "api-ms-win-crt-heap-l1-1-0.dll"]);
        assert_eq!(
            import_names(&pe).unwrap(),
            vec!["KERNEL32.dll", "vulkan-1.dll", "api-ms-win-crt-heap-l1-1-0.dll"]
        );
    }

    #[test]
    fn an_image_without_imports_has_none() {
        assert_eq!(import_names(&image(&[])).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn refuses_what_is_not_a_pe_image() {
        assert!(import_names(b"not an image").is_none());
        let mut pe = image(&["a.dll"]);
        pe[0x40] = b'X';
        assert!(import_names(&pe).is_none());
        // A table pointing outside every section.
        let mut pe = image(&["a.dll"]);
        pe[0x44 + 20 + 112 + 8..0x44 + 20 + 112 + 12].copy_from_slice(&0x9000u32.to_le_bytes());
        assert!(import_names(&pe).is_none());
    }
}
