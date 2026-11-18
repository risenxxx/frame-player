//! The player's registration with the Windows shell: the file types, the
//! `magnet:` candidacy and — for a portable copy — the `frameplayer://`
//! protocol.
//!
//! **The file types are written here for every kind of copy**, not by the
//! installer. Tauri's own NSIS template maps every extension of an association
//! to one ProgId named after it, which is what we shipped until issue #76: all
//! sixteen extensions were one type called `Video`, so Explorer sorted an `.mkv`
//! and an `.mp4` together and described both as "Video file"; nothing wrote
//! `Capabilities\FileAssociations`, so the player was missing from Settings →
//! Default apps; and nothing offered it for audio at all. `tauri.windows.conf.json`
//! therefore empties `bundle.fileAssociations` for the installer, and:
//!
//! - an installer run calls the executable with `--register-shell` (and its
//!   uninstaller with `--unregister-shell`) — see installer-hooks.nsh;
//! - an installed copy updated in place, which no installer ran for, finds its
//!   registration missing or older than [`imp::SCHEME`] at start and writes it
//!   ([`refresh`]);
//! - a portable copy writes it when the viewer turns its settings switch on.
//!
//! One implementation, so the three cannot drift apart.
//!
//! **Each extension is its own ProgId**, `FramePlayer.<ext>`, described as
//! "MKV video file" and so on. An association whose `rank` is `Alternate` (the
//! audio one) only *offers* the player — `OpenWithProgids` and the capabilities
//! list — and never takes the extension's default: a music player the viewer
//! already has keeps its files. The video extensions take the default the way
//! the installer always did. Either way Windows' own `UserChoice` outranks it,
//! which is what the Default apps page writes.
//!
//! Why a portable copy registers at all: **Windows has no folder-local way to
//! register a protocol handler or a file type** — both are machine-side by
//! definition. Two consequences worth naming rather than discovering:
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
//! installation's keys, down to `URL:<identifier> protocol`). The extensions are
//! read from `bundle.fileAssociations` in the **configuration file itself**,
//! embedded with `include_str!`, rather than from `app.config()`. That is not a
//! preference: `ToTokens for BundleConfig` writes `file_associations` into the
//! binary as `quote!(None)`, so at runtime the field is always empty. Reading
//! the file keeps one list instead of two — the same one macOS builds its
//! `Info.plist` from.
//!
//! **`magnet:` is offered, not taken.** A torrent client the viewer already
//! has owns `Software\Classes\magnet`, and nothing here touches that key.
//! The player enters itself as one *candidate* for the scheme instead — a
//! `RegisteredApplications` entry whose `Capabilities\URLAssociations` name a
//! ProgId of its own — which puts it in Settings → Default apps and in the
//! "open with" picker, and changes nothing until the viewer picks it.

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

/// `--register-shell` / `--unregister-shell`: what the installer and the
/// uninstaller run the executable with. Checked first thing in `run()`, before
/// anything else the process would start — it writes the keys and exits, with no
/// window and no single-instance guard to hand the arguments to a running copy.
/// Returns whether the process was started for this and should exit now.
pub fn handle_install_flag() -> bool {
    #[cfg(windows)]
    {
        let register = std::env::args().skip(1).any(|a| a == "--register-shell");
        let unregister = std::env::args().skip(1).any(|a| a == "--unregister-shell");
        if !register && !unregister {
            return false;
        }
        match imp::exe() {
            Ok(exe) => {
                if register {
                    if let Err(e) = imp::register(&exe) {
                        eprintln!("[shell] --register-shell: {e}");
                    }
                } else {
                    imp::unregister(&exe);
                }
                imp::notify_shell();
            }
            Err(e) => eprintln!("[shell] cannot name this executable: {e}"),
        }
        true
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// At start: bring this copy's registration up to date if it owns one that an
/// older version wrote — an installed copy updated in place, which no installer
/// ran for, or a portable copy whose switch was on before the format changed.
/// Off the main thread; a hundred-odd registry writes are not worth a delay to
/// the window.
pub fn refresh(app: &AppHandle) {
    #[cfg(windows)]
    {
        let app = app.clone();
        std::thread::spawn(move || imp::refresh(&app));
    }
    #[cfg(not(windows))]
    {
        let _ = app;
    }
}

#[cfg(windows)]
mod imp {
    use super::AppHandle;
    use tauri_plugin_deep_link::DeepLinkExt;
    use windows_registry::CURRENT_USER;

    /// What the registration looks like, as a number. Raise it whenever what
    /// `register` writes changes, and every copy that owns a registration
    /// rewrites it at its next start. `1` was the installer's single `Video`
    /// ProgId, which never wrote a marker.
    pub const SCHEME: &str = "2";

    /// One value per executable that has registered itself, holding the
    /// [`SCHEME`] it wrote. Per executable because an installed copy and a
    /// portable one may both have been here, and each answers only for itself.
    const MARKERS: &str = "Software\\FramePlayer\\Associations";

    /// The ProgId every video extension shared before [`SCHEME`] 2, written by
    /// Tauri's template from the association's `name`.
    const LEGACY_PROG_ID: &str = "Video";

    /// Where a displaced value is kept. The same name the NSIS template's own
    /// association macro uses, so that the two agree about what "the value
    /// before Frame Player" means.
    fn backup_name(prog_id: &str) -> String {
        format!("{prog_id}_backup")
    }

    /// Where the ProgId's own previous command line is kept. The ProgIds are
    /// shared with an ordinary installation — both copies use the same names —
    /// so taking the registration overwrites *its* command, and giving the
    /// registration back has to put that command back rather than delete a key
    /// the installed copy still depends on.
    const PREVIOUS_COMMAND: &str = "fp-previous-command";

    /// The ProgId a `magnet:` link is opened through when the viewer has picked
    /// the player for it. Never `Classes\magnet` itself — see the top of this
    /// file.
    const MAGNET_PROG_ID: &str = "FramePlayer.Magnet";
    /// Where the player's capabilities are described, and the name it is
    /// listed under in `RegisteredApplications`.
    const CAPABILITIES: &str = "Software\\FramePlayer\\Capabilities";
    const REGISTERED_NAME: &str = "Frame Player";
    const REGISTERED_APPLICATIONS: &str = "Software\\RegisteredApplications";

    fn classes(path: &str) -> String {
        format!("Software\\Classes\\{path}")
    }

    /// The executable, as the shell should spell it. `dunce` is what the
    /// deep-link plugin uses for the same job — a `\\?\` prefix in a registry
    /// command line is not something Explorer handles.
    pub fn exe() -> Result<String, String> {
        let path = tauri::utils::platform::current_exe().map_err(|e| e.to_string())?;
        Ok(dunce::simplified(&path).display().to_string())
    }

    /// Quoted: the product directory has a space in it, and an unquoted command
    /// is then resolved by the shell trying one space after another.
    fn command_for(exe: &str) -> String {
        format!("\"{exe}\" \"%1\"")
    }

    /// One association, as the configuration declares it.
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Assoc {
        /// "Video file" — the tail of each extension's type name.
        description: Option<String>,
        /// Without the leading dot, which is how the configuration spells them.
        ext: Vec<String>,
        /// macOS's `LSHandlerRank`, read here for the same meaning: `Alternate`
        /// is a type the player can open but should not claim.
        rank: Option<String>,
    }

    impl Assoc {
        fn takes_default(&self) -> bool {
            self.rank.as_deref() != Some("Alternate")
        }
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
    /// why `app.config()` cannot answer this. The base file, not the Windows
    /// one: that one empties the list so the installer writes nothing itself.
    fn associations() -> &'static [Assoc] {
        static PARSED: std::sync::OnceLock<Vec<Assoc>> = std::sync::OnceLock::new();
        PARSED.get_or_init(|| {
            let text = include_str!("../tauri.conf.json");
            match serde_json::from_str::<ConfFile>(text) {
                Ok(conf) => conf
                    .bundle
                    .file_associations
                    .into_iter()
                    .filter(|a| !a.ext.is_empty())
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

    fn prog_id(ext: &str) -> String {
        format!("FramePlayer.{}", ext.to_ascii_lowercase())
    }

    /// "MKV video file": what Explorer's Type column shows, and what it sorts
    /// by — one name for every extension was the second half of issue #76.
    fn type_name(ext: &str, assoc: &Assoc) -> String {
        let kind = assoc.description.as_deref().unwrap_or("File").to_lowercase();
        format!("{} {kind}", ext.to_ascii_uppercase())
    }

    pub fn is_ours(app: &AppHandle) -> bool {
        let Ok(exe) = exe() else { return false };
        let command = command_for(&exe);
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
        command_at(&format!(
            "{}\\shell\\open\\command",
            super::super::DEEP_LINK_SCHEME
        ))
        .map(|value| value.eq_ignore_ascii_case(&command))
            .unwrap_or(false)
    }

    pub fn set(app: &AppHandle, on: bool) -> Result<(), String> {
        let exe = exe()?;
        if on {
            app.deep_link()
                .register(super::super::DEEP_LINK_SCHEME)
                .map_err(|e| format!("the protocol: {e}"))?;
            register(&exe)?;
        } else {
            // Best effort throughout: a key somebody else has already changed
            // is not ours to insist on, and refusing to give back half a
            // registration would leave the viewer with no way to finish.
            let _ = app.deep_link().unregister(super::super::DEEP_LINK_SCHEME);
            let _ = CURRENT_USER.remove_tree(classes(super::super::DEEP_LINK_SCHEME));
            unregister(&exe);
        }
        notify_shell();
        Ok(())
    }

    pub fn refresh(app: &AppHandle) {
        let Ok(exe) = exe() else { return };
        let marker = CURRENT_USER
            .open(MARKERS)
            .and_then(|k| k.get_string(&exe));
        if marker.as_deref() == Ok(SCHEME) {
            return;
        }
        // Ours to rewrite when: this copy registered an older scheme; or it is
        // an installation, whose registration is the installer's and so always
        // wanted; or the pre-marker `Video` ProgId still opens this executable,
        // which is a portable copy whose switch was on before the change.
        let owned = marker.is_ok()
            || crate::portable::installed(app)
            || legacy_points_at(&exe);
        if !owned {
            return;
        }
        match register(&exe) {
            Ok(()) => eprintln!("[shell] registration brought up to scheme {SCHEME}"),
            Err(e) => eprintln!("[shell] registration refresh failed: {e}"),
        }
        notify_shell();
    }

    /// The file types and the `magnet:` candidacy — everything but the
    /// protocol, which an installation gets from the installer's deep-link
    /// keys and a portable copy from [`set`].
    pub fn register(exe: &str) -> Result<(), String> {
        let command = command_for(exe);
        let icon = format!("{exe},0");

        let caps = CURRENT_USER
            .create(CAPABILITIES)
            .map_err(|e| format!("{CAPABILITIES}: {e}"))?;
        caps.set_string("ApplicationName", REGISTERED_NAME)
            .and_then(|()| caps.set_string("ApplicationDescription", REGISTERED_NAME))
            .map_err(|e| format!("{CAPABILITIES}: {e}"))?;
        let file_caps = CURRENT_USER
            .create(format!("{CAPABILITIES}\\FileAssociations"))
            .map_err(|e| format!("{CAPABILITIES}: {e}"))?;

        for assoc in associations() {
            for ext in &assoc.ext {
                let id = prog_id(ext);
                write_prog_id(&id, &type_name(ext, assoc), &icon, &command)?;

                let ext_path = classes(&format!(".{ext}"));
                let key = CURRENT_USER
                    .create(&ext_path)
                    .map_err(|e| format!(".{ext}: {e}"))?;
                // On the "Open with" list whether or not it is the default.
                CURRENT_USER
                    .create(format!("{ext_path}\\OpenWithProgids"))
                    .and_then(|k| k.set_string(&id, ""))
                    .map_err(|e| format!(".{ext}: {e}"))?;
                if assoc.takes_default() {
                    let previous = key.get_string("").unwrap_or_default();
                    let backup = backup_name(&id);
                    if key.get_string(&backup).is_err() && previous != id {
                        // What to remember is the value from before any copy of
                        // this player — so when the extension is still on the
                        // old shared ProgId, the value *it* displaced. That one
                        // is left where it is: an uninstaller written before
                        // this change restores from it.
                        let original = if previous == LEGACY_PROG_ID {
                            key.get_string(backup_name(LEGACY_PROG_ID))
                                .unwrap_or_default()
                        } else {
                            previous
                        };
                        // And never one of the player's own names. Tauri's
                        // template backs up whatever the extension held, so an
                        // install over an install left `Video_backup = Video`;
                        // carried over, it restored the extension to a ProgId
                        // `drop_legacy` deletes a moment later.
                        let original = if is_players_prog_id(&original) {
                            String::new()
                        } else {
                            original
                        };
                        let _ = key.set_string(&backup, original);
                    }
                    key.set_string("", &id)
                        .map_err(|e| format!(".{ext}: {e}"))?;
                }
                file_caps
                    .set_string(format!(".{ext}"), &id)
                    .map_err(|e| format!("{CAPABILITIES}: {e}"))?;
            }
        }

        write_prog_id(MAGNET_PROG_ID, "Magnet link", &icon, &command)?;
        CURRENT_USER
            .create(format!("{CAPABILITIES}\\URLAssociations"))
            .and_then(|k| k.set_string("magnet", MAGNET_PROG_ID))
            .map_err(|e| format!("{CAPABILITIES}: {e}"))?;
        CURRENT_USER
            .create(REGISTERED_APPLICATIONS)
            .and_then(|k| k.set_string(REGISTERED_NAME, CAPABILITIES))
            .map_err(|e| format!("{REGISTERED_APPLICATIONS}: {e}"))?;

        drop_legacy(exe);
        CURRENT_USER
            .create(MARKERS)
            .and_then(|k| k.set_string(exe, SCHEME))
            .map_err(|e| format!("{MARKERS}: {e}"))
    }

    /// Undo [`register`] for this executable. A ProgId whose command is no
    /// longer ours belongs to the copy that took it since, and is left alone
    /// along with everything pointing at it.
    pub fn unregister(exe: &str) {
        let command = command_for(exe);
        let mut gone = true;
        for assoc in associations() {
            for ext in &assoc.ext {
                let id = prog_id(ext);
                match give_back_prog_id(&id, &command) {
                    GiveBack::NotOurs => {
                        gone = false;
                        continue;
                    }
                    GiveBack::Restored => gone = false,
                    GiveBack::Removed => {}
                }
                let ext_path = classes(&format!(".{ext}"));
                let Ok(key) = CURRENT_USER.create(&ext_path) else {
                    continue;
                };
                // Only when something was displaced. No backup means the
                // extension already named this ProgId before we touched it —
                // which on a machine that also has an ordinary installation is
                // *its* registration, since both copies use the same name. So
                // the value is left as it is: a ProgId key that no longer exists
                // is simply no association, which Windows handles; an empty one
                // is a broken entry.
                if key.get_string("").ok().as_deref() == Some(id.as_str()) {
                    if let Ok(previous) = key.get_string(backup_name(&id)) {
                        // Nothing before us, or a name of ours that no longer
                        // exists: the extension goes back to having no default.
                        if previous.is_empty() || is_players_prog_id(&previous) {
                            let _ = key.remove_value("");
                        } else {
                            let _ = key.set_string("", previous);
                        }
                    }
                }
                let _ = key.remove_value(backup_name(&id));
                if let Ok(open_with) = open_rw(&format!("{ext_path}\\OpenWithProgids")) {
                    let _ = open_with.remove_value(&id);
                }
            }
        }
        match give_back_prog_id(MAGNET_PROG_ID, &command) {
            GiveBack::Removed => {}
            GiveBack::Restored | GiveBack::NotOurs => gone = false,
        }
        // The capabilities and the listing describe whichever copy is still
        // registered under the same names; withdrawn only when none is.
        if gone {
            let _ = CURRENT_USER.remove_tree(CAPABILITIES);
            if let Ok(key) = CURRENT_USER.create(REGISTERED_APPLICATIONS) {
                let _ = key.remove_value(REGISTERED_NAME);
            }
        }
        if let Ok(key) = open_rw(MARKERS) {
            let _ = key.remove_value(exe);
        }
    }

    enum GiveBack {
        /// Another copy's command is in it now.
        NotOurs,
        /// The copy whose command we displaced has it back.
        Restored,
        /// Nobody had it before us, and it is gone.
        Removed,
    }

    fn give_back_prog_id(id: &str, command: &str) -> GiveBack {
        let path = format!("{id}\\shell\\open\\command");
        match command_at(&path) {
            Ok(current) if current.eq_ignore_ascii_case(command) => {}
            // Already gone is as good as removed.
            Err(_) => return GiveBack::Removed,
            Ok(_) => return GiveBack::NotOurs,
        }
        match CURRENT_USER
            .open(classes(id))
            .and_then(|k| k.get_string(PREVIOUS_COMMAND))
        {
            Ok(previous) => {
                let _ = write(&path, "", &previous);
                if let Ok(key) = open_rw(&classes(id)) {
                    let _ = key.remove_value(PREVIOUS_COMMAND);
                }
                GiveBack::Restored
            }
            Err(_) => {
                let _ = CURRENT_USER.remove_tree(classes(id));
                GiveBack::Removed
            }
        }
    }

    /// A ProgId with its icon and its one verb, keeping the command it
    /// displaces when that command was another copy's.
    fn write_prog_id(id: &str, name: &str, icon: &str, command: &str) -> Result<(), String> {
        let key = CURRENT_USER
            .create(classes(id))
            .map_err(|e| format!("{id}: {e}"))?;
        key.set_string("", name).map_err(|e| format!("{id}: {e}"))?;
        write(&format!("{id}\\DefaultIcon"), "", icon)?;
        write(&format!("{id}\\shell"), "", "open")?;
        write(
            &format!("{id}\\shell\\open"),
            "",
            &format!("Open with {REGISTERED_NAME}"),
        )?;
        let command_path = format!("{id}\\shell\\open\\command");
        if let Ok(existing) = command_at(&command_path) {
            if !existing.eq_ignore_ascii_case(command)
                && key.get_string(PREVIOUS_COMMAND).is_err()
            {
                let _ = key.set_string(PREVIOUS_COMMAND, existing);
            }
        }
        write(&command_path, "", command)
    }

    /// Whether the old shared ProgId opens this executable — in the installer's
    /// spelling (unquoted) or a portable copy's (quoted) alike.
    fn legacy_points_at(exe: &str) -> bool {
        command_at(&format!("{LEGACY_PROG_ID}\\shell\\open\\command"))
            .map(|c| c.to_lowercase().contains(&exe.to_lowercase()))
            .unwrap_or(false)
    }

    /// Retire the shared `Video` ProgId once it is this copy's: given back to
    /// the copy it was taken from, if any, or removed. One that opens another
    /// copy is that copy's to retire when it updates.
    fn drop_legacy(exe: &str) {
        if !legacy_points_at(exe) {
            return;
        }
        let path = format!("{LEGACY_PROG_ID}\\shell\\open\\command");
        match CURRENT_USER
            .open(classes(LEGACY_PROG_ID))
            .and_then(|k| k.get_string(PREVIOUS_COMMAND))
        {
            Ok(previous) => {
                let _ = write(&path, "", &previous);
                if let Ok(key) = open_rw(&classes(LEGACY_PROG_ID)) {
                    let _ = key.remove_value(PREVIOUS_COMMAND);
                }
            }
            Err(_) => {
                let _ = CURRENT_USER.remove_tree(classes(LEGACY_PROG_ID));
            }
        }
    }

    /// An existing key, opened so that a value can be removed from it.
    /// `CURRENT_USER.open` is read-only, and a `remove_value` through it fails
    /// with access denied — which `let _ =` used to swallow, leaving every
    /// "Open with" entry and the marker behind on uninstall. `create` would
    /// work too, but would also make the key when it is not there.
    fn open_rw(path: &str) -> windows_registry::Result<windows_registry::Key> {
        CURRENT_USER.options().read().write().open(path)
    }

    /// The legacy shared ProgId, or one of the `FramePlayer.*` names: never a
    /// value worth restoring an extension to.
    fn is_players_prog_id(value: &str) -> bool {
        value.eq_ignore_ascii_case(LEGACY_PROG_ID)
            || value.to_ascii_lowercase().starts_with("frameplayer.")
    }

    /// The command a `Classes\<path>` key holds; `path` ends in `\command`.
    fn command_at(path: &str) -> windows_registry::Result<String> {
        CURRENT_USER
            .open(classes(path))
            .and_then(|k| k.get_string(""))
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
    pub fn notify_shell() {
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
