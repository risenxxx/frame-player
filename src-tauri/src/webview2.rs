//! Making sure there is an engine to draw the interface with.
//!
//! The whole player is HTML over mpv's view, so without the WebView2 runtime
//! there is no window at all — not a degraded one, none: nothing to put a
//! message in either. An ordinary installation never meets this, because the
//! NSIS installer provisions the runtime (`webviewInstallMode`, the downloaded
//! bootstrapper by default). A **portable copy has no installer**, and that is
//! what this file is for.
//!
//! Three routes were on the table and the trade between them is not about size:
//!
//! - *Carry the runtime in the archive.* About 180 MB unpacked, nothing to
//!   download, no trace outside the folder — but a portable copy's file set
//!   would then differ from an installation's, which means it could no longer
//!   share the update payload and would need an artifact and a manifest key of
//!   its own. A lot of machinery for a case that Windows 10 and 11 almost never
//!   present.
//! - *Install the Evergreen runtime when it is missing*, which is what runs
//!   here: a two-megabyte bootstrapper, silent, no elevation (it installs for
//!   the current user when it cannot have the machine), and the same trace the
//!   ordinary installer leaves anyway.
//! - *Say so and open the download page*, which is the fallback for when the
//!   above cannot work — no network, a refused install, a locked-down machine.
//!
//! It runs for **both** kinds of copy, not only portable ones: an installation
//! whose runtime was removed afterwards heals itself the same way, and there is
//! nothing to gain from letting that one die instead.
//!
//! All of it happens before `tauri::Builder` runs, because the runtime has to
//! exist before the window does. So there is no webview to report progress in
//! and no single-instance guard yet either — a notice goes up first, through the
//! one interface available at that point, and the bootstrapper serializes
//! concurrent runs itself.

/// Microsoft's permanent link to the Evergreen bootstrapper. A redirector
/// rather than a versioned file, which is the point: it is the address
/// Microsoft documents for exactly this and does not go stale.
const BOOTSTRAPPER: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";

/// Where somebody sent here by hand would go.
const DOWNLOAD_PAGE: &str = "https://developer.microsoft.com/microsoft-edge/webview2/";

/// Ensure there is a WebView2 runtime, or explain why there is not.
///
/// Returns `false` when the player should give up: the caller then exits rather
/// than building a window that cannot be drawn.
pub fn ensure() -> bool {
    #[cfg(not(windows))]
    {
        true
    }
    #[cfg(windows)]
    {
        imp::ensure()
    }
}

#[cfg(windows)]
mod imp {
    use super::{BOOTSTRAPPER, DOWNLOAD_PAGE};

    /// What the loader says when it cannot find a runtime. Asked rather than
    /// guessed at from a registry key: the same call is already what the
    /// settings footer shows (`webview_version` in lib.rs), and it knows about
    /// the preview channels and the fixed-version folder as well.
    fn installed() -> Option<String> {
        tauri::webview_version().ok()
    }

    pub fn ensure() -> bool {
        if let Some(version) = installed() {
            return {
                eprintln!("[webview2] runtime {version}");
                true
            };
        }
        eprintln!("[webview2] no runtime found");

        // A notice first. A silent minute with no window is indistinguishable
        // from a player that failed to start, and the viewer would double-click
        // again.
        if !ask(
            "Frame Player needs the Microsoft Edge WebView2 runtime, which is not \
             installed on this computer.\n\nIt will be downloaded and installed now. \
             This takes a minute and needs no administrator rights.",
        ) {
            return false;
        }

        match install() {
            Ok(()) => {}
            Err(e) => {
                eprintln!("[webview2] could not install the runtime: {e}");
                return give_up();
            }
        }
        match installed() {
            Some(version) => {
                eprintln!("[webview2] runtime {version} after installing");
                true
            }
            // The installer said it succeeded and the loader still finds
            // nothing. Nothing further to try from here.
            None => {
                eprintln!("[webview2] the runtime is still not found after installing");
                give_up()
            }
        }
    }

    /// The fallback: say what is missing and open the page. Always returns
    /// false — the caller exits.
    fn give_up() -> bool {
        tell(
            "Frame Player could not install the Microsoft Edge WebView2 runtime \
             automatically.\n\nThe download page will open now. Install the runtime \
             and start Frame Player again.",
        );
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", DOWNLOAD_PAGE])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
        false
    }

    /// Fetch the bootstrapper and run it. Synchronous by necessity — there is no
    /// runtime to await on yet — so it borrows a single-threaded one for the
    /// download and blocks on the installer.
    fn install() -> Result<(), String> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("no runtime for the download: {e}"))?;
        let bytes = runtime.block_on(async {
            let response = reqwest::Client::builder()
                .build()
                .map_err(|e| e.to_string())?
                .get(BOOTSTRAPPER)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !response.status().is_success() {
                return Err(format!("the bootstrapper returned {}", response.status()));
            }
            response.bytes().await.map_err(|e| e.to_string())
        })?;

        // Into the temp directory and not beside the executable: a portable
        // copy's folder is the one place that must not collect debris, and this
        // file is of no use after the install.
        let path = std::env::temp_dir().join("MicrosoftEdgeWebview2Setup.exe");
        std::fs::write(&path, &bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))?;

        let status = std::process::Command::new(&path)
            // `/install` on its own would show the installer's own window;
            // silent is what makes this one click rather than three.
            .args(["/silent", "/install"])
            .status()
            .map_err(|e| format!("cannot run the bootstrapper: {e}"))?;
        let _ = std::fs::remove_file(&path);
        if !status.success() {
            return Err(format!("the bootstrapper exited with {status}"));
        }
        Ok(())
    }

    // --- the only interface there is before a window exists -----------------

    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDOK, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MB_OKCANCEL,
    };

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn message(text: &str, style: u32) -> i32 {
        let body = wide(text);
        let title = wide("Frame Player");
        // SAFETY: two NUL-terminated UTF-16 buffers that outlive the call, and
        // a null owner window, which is what a process with no window has.
        unsafe { MessageBoxW(std::ptr::null_mut(), body.as_ptr(), title.as_ptr(), style) }
    }

    /// OK or Cancel, where Cancel means the viewer would rather not.
    fn ask(text: &str) -> bool {
        message(text, MB_OKCANCEL | MB_ICONINFORMATION) == IDOK
    }

    fn tell(text: &str) {
        message(text, MB_OK | MB_ICONERROR);
    }
}
