//! Making the system find a copy of the player that no installer put there.
//!
//! Two things in an ordinary installation come from the NSIS installer rather
//! than from the player: the `frameplayer://` protocol, which is what a
//! watch-together invitation is, and the video file associations, which is what
//! "Open with" and a double click rely on. A portable copy has no installer, and
//! **Windows has no folder-local way to register either** — a protocol handler
//! and a file type are machine-side registrations by definition. So a portable
//! copy writes them itself, under `HKEY_CURRENT_USER` like the installer does,
//! and can take them back out again.
//!
//! Two consequences worth naming rather than discovering:
//!
//! - With an ordinary installation beside a portable copy, **whichever ran last
//!   owns the association**. There is no arrangement in which both can own it.
//! - If a portable folder is simply deleted, its keys stay behind pointing at a
//!   path that is gone, and an invitation link then does nothing. The switch in
//!   the settings sheet is how that is undone while the copy still exists; the
//!   previous value of everything displaced is kept beside it so that taking
//!   the registration back puts the machine back.
//!
//! The protocol half is the deep-link plugin's own `register`/`unregister`,
//! which writes exactly what the installer writes (verified against a real
//! installation's keys, down to `URL:<identifier> protocol`). The associations
//! are written here, out of `bundle.fileAssociations` — and read from the
//! **configuration file itself**, embedded with `include_str!`, rather than
//! from `app.config()`. That is not a preference: `ToTokens for BundleConfig`
//! writes `file_associations` into the binary as `quote!(None)`, so at runtime
//! the field is always empty. Found the way such things are found — the protocol
//! registered itself and sixteen extensions silently did not. Reading the file
//! keeps one list instead of two, so an extension added to the bundle is an
//! extension this registers.

use tauri::AppHandle;

/// Whether the shell currently points at *this* executable. The settings switch
/// shows the registration's real state rather than what was last asked for:
/// another copy may have taken it since.
#[tauri::command]
pub fn shell_handler_state(app: AppHandle) -> bool {
    #[cfg(windows)]
    {
        imp::is_ours(&app)
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        false
    }
}

/// Take the registration, or give it back.
#[tauri::command]
pub fn shell_handler_set(app: AppHandle, on: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        imp::set(&app, on)
    }
    #[cfg(not(windows))]
    {
        let _ = (app, on);
        Err("only Windows registers handlers this way".into())
    }
}

#[cfg(windows)]
mod imp {
    use super::AppHandle;
    use tauri_plugin_deep_link::DeepLinkExt;
    use windows_registry::CURRENT_USER;

    /// Where a displaced value is kept. The same name the NSIS installer's own
    /// association macro uses, so that the two agree about what "the value
    /// before Frame Player" means and an uninstall still restores it.
    fn backup_name(prog_id: &str) -> String {
        format!("{prog_id}_backup")
    }

    /// Where the ProgId's own previous command line is kept. The ProgId is
    /// shared with an ordinary installation — both copies call it `Video` — so
    /// taking the registration overwrites *its* command, and giving the
    /// registration back has to put that command back rather than delete a key
    /// the installed copy still depends on.
    const PREVIOUS_COMMAND: &str = "fp-previous-command";

    fn classes(path: &str) -> String {
        format!("Software\\Classes\\{path}")
    }

    /// The executable, as the shell should spell it. `dunce` is what the
    /// deep-link plugin uses for the same job — a `\\?\` prefix in a registry
    /// command line is not something Explorer handles.
    fn exe() -> Result<String, String> {
        let path = tauri::utils::platform::current_exe().map_err(|e| e.to_string())?;
        Ok(dunce::simplified(&path).display().to_string())
    }

    /// One association, as the configuration declares it: the identifier the
    /// extensions point at, what it is called, and which extensions.
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Assoc {
        /// The bundler's own rule is the association's name, or the first
        /// extension when it has none — and this configuration always names
        /// one, so a missing name drops the entry rather than inventing a
        /// ProgId the installer would not have used.
        name: Option<String>,
        description: Option<String>,
        /// Without the leading dot, which is how the configuration spells them
        /// and the form every key below wants.
        ext: Vec<String>,
    }

    #[derive(serde::Deserialize)]
    struct ConfFile {
        bundle: ConfBundle,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ConfBundle {
        #[serde(default)]
        file_associations: Vec<Assoc>,
    }

    /// The associations as the configuration declares them. Parsed once from the
    /// file embedded at compile time; see the note at the top of this file for
    /// why `app.config()` cannot answer this.
    fn associations() -> &'static [Assoc] {
        static PARSED: std::sync::OnceLock<Vec<Assoc>> = std::sync::OnceLock::new();
        PARSED.get_or_init(|| {
            let text = include_str!("../tauri.conf.json");
            match serde_json::from_str::<ConfFile>(text) {
                Ok(conf) => conf
                    .bundle
                    .file_associations
                    .into_iter()
                    .filter(|a| a.name.is_some() && !a.ext.is_empty())
                    .collect(),
                // Unreachable in a build that exists: tauri-build parses the
                // same file and fails the build before this could.
                Err(e) => {
                    eprintln!("[shell] cannot read the file associations: {e}");
                    Vec::new()
                }
            }
        })
    }

    impl Assoc {
        fn prog_id(&self) -> &str {
            self.name.as_deref().unwrap_or_default()
        }
    }

    pub fn is_ours(app: &AppHandle) -> bool {
        let Ok(exe) = exe() else { return false };
        let command = format!("\"{exe}\" \"%1\"");
        // The protocol is the half that matters most — an invitation link is
        // the only thing that cannot be done any other way — so it decides.
        let protocol = app
            .deep_link()
            .is_registered(super::super::DEEP_LINK_SCHEME)
            .unwrap_or(false);
        if !protocol {
            return false;
        }
        // And it has to be *this* copy, not another one that registered the
        // same scheme.
        CURRENT_USER
            .open(classes(&format!(
                "{}\\shell\\open\\command",
                super::super::DEEP_LINK_SCHEME
            )))
            .and_then(|key| key.get_string(""))
            .map(|value| value.eq_ignore_ascii_case(&command))
            .unwrap_or(false)
    }

    pub fn set(app: &AppHandle, on: bool) -> Result<(), String> {
        if on {
            take(app)?;
        } else {
            give_back(app)?;
        }
        notify_shell();
        Ok(())
    }

    fn take(app: &AppHandle) -> Result<(), String> {
        let exe = exe()?;
        let command = format!("\"{exe}\" \"%1\"");
        let icon = format!("{exe},0");

        app.deep_link()
            .register(super::super::DEEP_LINK_SCHEME)
            .map_err(|e| format!("the protocol: {e}"))?;

        let product = app
            .config()
            .product_name
            .clone()
            .unwrap_or_else(|| "Frame Player".into());
        for assoc in associations() {
            let key = CURRENT_USER
                .create(classes(assoc.prog_id()))
                .map_err(|e| format!("{}: {e}", assoc.prog_id()))?;
            key.set_string("", assoc.description.as_deref().unwrap_or_default())
                .map_err(|e| format!("{}: {e}", assoc.prog_id()))?;
            write(&format!("{}\\DefaultIcon", assoc.prog_id()), "", &icon)?;
            write(&format!("{}\\shell", assoc.prog_id()), "", "open")?;
            write(
                &format!("{}\\shell\\open", assoc.prog_id()),
                "",
                &format!("Open with {product}"),
            )?;
            // Quoted, unlike the installer's own line: the product directory
            // has a space in it, and an unquoted command is then resolved by
            // the shell trying one space after another.
            let command_path = format!("{}\\shell\\open\\command", assoc.prog_id());
            // Whose command this was, if it was anybody's and not already ours.
            if let Ok(existing) = CURRENT_USER
                .open(classes(&command_path))
                .and_then(|k| k.get_string(""))
            {
                if existing != command && key.get_string(PREVIOUS_COMMAND).is_err() {
                    let _ = key.set_string(PREVIOUS_COMMAND, existing);
                }
            }
            write(&command_path, "", &command)?;

            for ext in &assoc.ext {
                let path = classes(&format!(".{ext}"));
                let key = CURRENT_USER
                    .create(&path)
                    .map_err(|e| format!(".{ext}: {e}"))?;
                let previous = key.get_string("").unwrap_or_default();
                // Only the first time, so that what is remembered is the value
                // from before any copy of this player, not the one the last
                // copy left.
                let backup = backup_name(assoc.prog_id());
                if key.get_string(&backup).is_err() && previous != assoc.prog_id() {
                    let _ = key.set_string(&backup, &previous);
                }
                key.set_string("", assoc.prog_id())
                    .map_err(|e| format!(".{ext}: {e}"))?;
            }
        }
        Ok(())
    }

    fn give_back(app: &AppHandle) -> Result<(), String> {
        // Best effort throughout: a key somebody else has already changed is
        // not ours to insist on, and refusing to give back half a registration
        // would leave the viewer with no way to finish.
        let _ = app.deep_link().unregister(super::super::DEEP_LINK_SCHEME);
        let _ = CURRENT_USER.remove_tree(classes(super::super::DEEP_LINK_SCHEME));

        for assoc in associations() {
            let backup = backup_name(assoc.prog_id());
            for ext in &assoc.ext {
                let path = classes(&format!(".{ext}"));
                let Ok(key) = CURRENT_USER.create(&path) else {
                    continue;
                };
                // Only undo what is still ours.
                if key.get_string("").unwrap_or_default() != assoc.prog_id() {
                    continue;
                }
                // Only when something was displaced. No backup means the
                // extension already named this ProgId before we touched it —
                // which on a machine that also has an ordinary installation is
                // *its* registration, since both copies use the same name. So
                // the value is left exactly as it is: emptying it was the first
                // version of this and it would have taken the installed copy's
                // association down with the portable one. A ProgId key that no
                // longer exists is simply no association, which Windows handles;
                // an empty one is a broken entry.
                if let Ok(previous) = key.get_string(&backup) {
                    let _ = key.set_string("", previous);
                    let _ = key.remove_value(&backup);
                }
            }
            // The ProgId goes back to whoever had it, and is removed only if
            // nobody did.
            match CURRENT_USER
                .open(classes(assoc.prog_id()))
                .and_then(|k| k.get_string(PREVIOUS_COMMAND))
            {
                Ok(command) => {
                    let _ = write(
                        &format!("{}\\shell\\open\\command", assoc.prog_id()),
                        "",
                        &command,
                    );
                    if let Ok(key) = CURRENT_USER.open(classes(assoc.prog_id())) {
                        let _ = key.remove_value(PREVIOUS_COMMAND);
                    }
                }
                Err(_) => {
                    let _ = CURRENT_USER.remove_tree(classes(assoc.prog_id()));
                }
            }
        }
        Ok(())
    }

    fn write(path: &str, name: &str, value: &str) -> Result<(), String> {
        let key = CURRENT_USER
            .create(classes(path))
            .map_err(|e| format!("{path}: {e}"))?;
        key.set_string(name, value)
            .map_err(|e| format!("{path}: {e}"))
    }

    /// Tell Explorer that the associations moved. Without it the change shows
    /// up whenever Explorer next happens to reload them, which reads as the
    /// switch not having worked.
    fn notify_shell() {
        use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
        // SAFETY: the documented "everything changed" notification, which takes
        // no item pointers.
        unsafe {
            SHChangeNotify(
                SHCNE_ASSOCCHANGED as i32,
                SHCNF_IDLIST,
                std::ptr::null(),
                std::ptr::null(),
            );
        }
    }
}
