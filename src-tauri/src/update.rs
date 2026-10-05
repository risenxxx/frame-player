//! The Windows update, applied in place: download, verify, swap the files,
//! relaunch. No installer window, no uninstall pass, and — unlike the path this
//! replaces — code after the call still runs.
//!
//! The installer path is still there and is still what a release hands to every
//! client older than this one, because `latest.json` keeps its `windows-x86_64`
//! key pointing at the setup executable. This one reads a **second** key,
//! `windows-x86_64-zip`, whose payload is the installation itself as a zip
//! (`scripts/pack-windows-update.ps1` builds it). Anything that makes the swap
//! unsafe falls back to that installer, which is why a mistake here costs a
//! release its *new* update path and never its working one.
//!
//! ## What Windows allows, measured
//!
//! | with the process running and the images loaded | |
//! |---|---|
//! | rename a loaded DLL inside its directory | yes |
//! | rename the running executable | yes |
//! | create a new file at the name just vacated | yes |
//! | **delete** the renamed image | **no**, access denied |
//! | delete it once the process has exited | yes |
//!
//! So the swap is a pair of renames per file — target to `*.fp-old`, incoming
//! into its place — and the leftovers can only be removed by a *later* process,
//! which is the whole reason the relaunched copy waits for its predecessor's
//! process id before sweeping. (A directory holding a loaded DLL *can* be
//! renamed, contrary to the usual telling of this; per-file is still what runs,
//! because it is what can be rolled back one file at a time.)
//!
//! ## The file set may not change
//!
//! The NSIS uninstaller deletes a list of paths fixed when it was written, and
//! only an installer run rewrites it. An in-place update that added or dropped
//! a path would therefore leave the uninstaller unable to clean up after
//! itself — and a release that renames a library (a major FFmpeg bump does)
//! would leave ninety megabytes behind after an uninstall, silently.
//!
//! So an in-place update is allowed only when the incoming file set is exactly
//! what is on disk; anything else is the installer's job, and the installer
//! rewrites the uninstaller as it always did. The set on disk is taken by
//! *walking the installation*, not from a manifest a previous version left
//! behind, so the very first in-place update works with no transition release.
//!
//! That same comparison is what makes this safe to ship: if the zip's layout
//! ever stops matching what the installer lays down, every client refuses the
//! swap and takes the installer. The failure mode of getting the packaging
//! wrong is last year's update path, not a broken installation.

#![cfg_attr(not(windows), allow(dead_code))]

use tauri::AppHandle;

/// The manifest key the in-place payload is published under. A key of its own
/// rather than a replacement: `windows-x86_64` stays the installer, so a client
/// that predates this file keeps updating exactly as it did.
const ZIP_TARGET: &str = "windows-x86_64-zip";

/// The payload's own description of itself, at the root of the zip and
/// deliberately *not* inside the folder that gets installed — see the file-set
/// rule above. Keyed by path, with a size and a digest for each.
const MANIFEST_NAME: &str = "update-manifest.json";

/// Where the payload is unpacked, inside the installation directory so that
/// every move into place is a rename on one volume rather than a copy.
const STAGING_DIR: &str = ".update";

/// What a replaced file is renamed to. Distinctive on purpose: the sweep
/// deletes anything carrying it, and `.old` would eventually meet somebody's
/// own file.
const OLD_SUFFIX: &str = ".fp-old";

/// The list of paths a swap is part-way through, written before the first
/// rename and removed after the last. Only a crash leaves it behind, and then
/// the next launch finishes the swap rather than leaving half a version on
/// disk. Lives in the staging directory, so clearing that clears this too.
const JOURNAL_NAME: &str = "swap.txt";

/// Passed to the relaunched copy so it can wait for this process to exit.
/// Starts with a dash, which is what keeps `pick_file_args` from reading it as
/// a file to open.
const AFTER_UPDATE_ARG: &str = "--after-update=";

/// Download progress, as whole percent. The frontend's update button reads it.
pub const PROGRESS_EVENT: &str = "frameplayer://update-progress";

/// How long the relaunched copy waits for its predecessor before sweeping
/// anyway. Generous: the predecessor exits immediately after spawning it, and
/// the only cost of being wrong is leftovers that the launch after this one
/// removes.
const PREDECESSOR_WAIT_MS: u32 = 15_000;

/// A rename that loses to an antivirus scanner holding a just-written file is
/// worth retrying; one that loses to a loaded image is not, and reports.
const RENAME_TRIES: usize = 10;
const RENAME_PAUSE: std::time::Duration = std::time::Duration::from_millis(120);

/// What `update_prepare` decided, and what the frontend does next.
#[derive(serde::Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// `"inplace"` — staged and ready for `update_commit`; `"installer"` —
    /// nothing was downloaded, run the installer path instead.
    pub mode: &'static str,
    /// The version staged, when one is.
    pub version: Option<String>,
    /// How many files the swap will move. A release that changes only the
    /// executable moves one.
    pub files: usize,
    /// Why the installer was chosen. For the log, not for the interface: the
    /// viewer is getting an update either way.
    pub reason: Option<String>,
}

impl Plan {
    /// Run the installer, which is what an ordinary installation falls back to.
    fn installer(reason: impl Into<String>) -> Self {
        Self {
            mode: "installer",
            version: None,
            files: 0,
            reason: Some(reason.into()),
        }
    }

    /// Send the viewer to the download page, which is what a **portable** copy
    /// falls back to. Running the installer there would be actively wrong: it
    /// would install a second, ordinary copy into the profile and leave the
    /// portable one exactly as old as it was, which is the failure this whole
    /// mode exists to avoid.
    fn download(reason: impl Into<String>) -> Self {
        Self {
            mode: "download",
            version: None,
            files: 0,
            reason: Some(reason.into()),
        }
    }

    /// Whichever of the two this copy has, which is a question about **how it
    /// was delivered** and not about where it keeps its state. An installation
    /// that keeps its state beside itself — the installer offers that — has an
    /// installer to fall back on and must be sent to it; only a copy nobody
    /// installed has nothing to run.
    fn no_swap(app: &AppHandle, reason: impl Into<String>) -> Self {
        if crate::portable::installed(app) {
            Self::installer(reason)
        } else {
            Self::download(reason)
        }
    }
}

// ---------------------------------------------------------------------------
// The commands. Present on every platform so that the handler list stays one
// list; on anything but Windows they say "use the installer", which is what the
// macOS path does anyway.
// ---------------------------------------------------------------------------

/// What a release announces, as the update button shows it.
#[derive(serde::Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Announcement {
    pub version: String,
    /// The release notes, as the manifest's `notes` field carries them.
    pub body: Option<String>,
    /// Whether this release has an in-place payload at all. For the log: the
    /// button looks the same either way, and whether the swap is *possible* is
    /// not settled until `update_prepare` has looked at the installation.
    pub in_place: bool,
}

/// Whether a newer release is waiting. Windows only, and the reason it is not
/// simply the plugin's own check is that the whole Windows update — what is
/// announced, what is downloaded, what is applied — then goes through one
/// place, with one way to point it at a test manifest.
///
/// The in-place key is asked for first and the installer key second, so a
/// release that published only the installer is still announced.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<Announcement>, String> {
    #[cfg(windows)]
    {
        imp::check(app).await
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Err("the plugin's own check is what macOS uses".into())
    }
}

/// Download the in-place payload and stage it. Returns without downloading
/// anything when the swap cannot be used, and the caller then runs the
/// installer exactly as before.
#[tauri::command]
pub async fn update_prepare(app: AppHandle) -> Result<Plan, String> {
    #[cfg(windows)]
    {
        imp::prepare(app).await
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Ok(Plan::no_swap(&app, "in-place updates are Windows-only"))
    }
}

/// Apply what `update_prepare` staged, then relaunch. Does not return on
/// success: the process is replaced by the new one.
#[tauri::command]
pub async fn update_commit(app: AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    {
        imp::commit(app).await
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Err("in-place updates are Windows-only".into())
    }
}

/// Called from `run()` before the app is built: if this process is the relaunch
/// of an update, wait for the copy that started it to exit.
///
/// Two things need that. The single-instance guard is a named mutex plus a
/// window, and a second process that finds the window hands over its argv and
/// exits — so starting while the predecessor still holds it would make the
/// player simply vanish after updating. And the renamed images cannot be
/// deleted until the process that had them mapped is gone, which is what the
/// sweep below is waiting for.
pub fn wait_for_predecessor() {
    #[cfg(windows)]
    imp::wait_for_predecessor();
}

/// Remove what the previous update left: the renamed files and the staging
/// directory. On a thread, because it is the only thing in startup that touches
/// a dozen files and nothing waits for its answer.
pub fn sweep_in_background() {
    #[cfg(windows)]
    imp::sweep_in_background();
}

/// `FP_UPDATE_AUTO=<seconds>` drives the whole update through the same two
/// commands the button calls — how the swap is tested with the libraries loaded
/// and a video playing, and without a hand on the mouse. The only thing it
/// leaves out is the frontend's resume snapshot.
///
/// Debug builds only, like the endpoint override it is used with; see
/// `scripts/update-test.ps1`, which sets all three.
pub fn selftest(app: &AppHandle) {
    #[cfg(all(windows, debug_assertions))]
    {
        let seconds = std::env::var("FP_UPDATE_AUTO")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(8);
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            // Long enough for the window to be up and for whatever is playing
            // to have mpv's libraries mapped, which is the state the swap has
            // to survive.
            tokio::time::sleep(std::time::Duration::from_secs(seconds)).await;
            eprintln!("[update] selftest: preparing");
            match imp::prepare(app.clone()).await {
                Ok(plan) if plan.mode == "inplace" => {
                    eprintln!("[update] selftest: committing {} file(s)", plan.files);
                    if let Err(e) = imp::commit(app).await {
                        eprintln!("[update] selftest: commit failed: {e}");
                    }
                }
                Ok(plan) => eprintln!("[update] selftest: {:?}", plan.reason),
                Err(e) => eprintln!("[update] selftest: prepare failed: {e}"),
            }
        });
    }
    #[cfg(not(all(windows, debug_assertions)))]
    {
        let _ = app;
    }
}

// ---------------------------------------------------------------------------
// The manifest, the file-set comparison and the naming rules. Pure, and tested:
// every one of them is a decision whose failure is a plausible-looking update
// that eats an installation.
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize, Debug, Clone)]
pub struct Manifest {
    pub version: String,
    /// The zip's single top-level directory: the installation itself.
    pub root: String,
    pub files: Vec<Entry>,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct Entry {
    /// Relative to the installation directory, `/`-separated.
    pub path: String,
    pub size: u64,
    /// Lower-case hex.
    pub sha256: String,
}

/// One manifest path as a path we are willing to write to. Everything about an
/// absolute path, a drive, a `..` or a backslash is refused rather than
/// normalised: the payload is signed, so anything surprising in it means
/// something is wrong upstream, not that we should be clever.
pub fn safe_rel(rel: &str) -> Result<std::path::PathBuf, String> {
    if rel.is_empty() || rel.contains('\\') || rel.contains(':') || rel.starts_with('/') {
        return Err(format!("not a relative path: {rel}"));
    }
    let mut out = std::path::PathBuf::new();
    for part in rel.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(format!("not a plain relative path: {rel}"));
        }
        out.push(part);
    }
    Ok(out)
}

/// The manifest as a whole: a root to strip, paths we can write, no duplicates
/// once Windows' indifference to case is taken into account.
pub fn check_manifest(m: &Manifest) -> Result<(), String> {
    if m.root.is_empty() || m.root.contains('/') || m.root.contains('\\') {
        return Err(format!("manifest root is not one directory: {}", m.root));
    }
    if m.files.is_empty() {
        return Err("manifest lists no files".into());
    }
    let mut seen = std::collections::HashSet::new();
    for e in &m.files {
        safe_rel(&e.path)?;
        if e.sha256.len() != 64 || !e.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("{} has no digest", e.path));
        }
        if !seen.insert(e.path.to_lowercase()) {
            return Err(format!("{} is listed twice", e.path));
        }
    }
    Ok(())
}

/// Whether the swap may run at all: the paths on disk against the paths coming
/// in. Returns the reason for the log when they differ, naming the first path
/// each way — a reason that says only "they differ" is a bug report nobody can
/// act on.
pub fn file_sets_match(installed: &[String], incoming: &[String]) -> Result<(), String> {
    let on_disk: std::collections::BTreeSet<String> =
        installed.iter().map(|p| p.to_lowercase()).collect();
    let arriving: std::collections::BTreeSet<String> =
        incoming.iter().map(|p| p.to_lowercase()).collect();
    let added: Vec<&String> = arriving.difference(&on_disk).collect();
    let dropped: Vec<&String> = on_disk.difference(&arriving).collect();
    if added.is_empty() && dropped.is_empty() {
        return Ok(());
    }
    let mut parts = Vec::new();
    if let Some(p) = added.first() {
        parts.push(format!("{} would be added ({} in all)", p, added.len()));
    }
    if let Some(p) = dropped.first() {
        parts.push(format!("{} would be dropped ({} in all)", p, dropped.len()));
    }
    Err(format!(
        "the file set changes: {} - only the installer can rewrite the uninstaller's list",
        parts.join(", ")
    ))
}

/// A name left by a previous update, which the sweep may delete: the suffix
/// exactly, or the suffix with a number after it.
pub fn is_leftover(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let Some(at) = lower.rfind(OLD_SUFFIX) else {
        return false;
    };
    let tail = &lower[at + OLD_SUFFIX.len()..];
    if tail.is_empty() {
        return true;
    }
    tail.len() > 1 && tail.starts_with('-') && tail[1..].bytes().all(|b| b.is_ascii_digit())
}

/// Where a file being replaced goes. Numbered when the plain name is taken,
/// because the previous update's leftover may still be mapped by a process that
/// has not exited and therefore cannot be deleted yet.
pub fn old_name(
    target: &std::path::Path,
    taken: impl Fn(&std::path::Path) -> bool,
) -> std::path::PathBuf {
    let with = |suffix: &str| {
        let mut name = target.as_os_str().to_os_string();
        name.push(suffix);
        std::path::PathBuf::from(name)
    };
    let mut candidate = with(OLD_SUFFIX);
    let mut n = 2;
    // 99 is not a limit anyone reaches; it is the point at which reporting the
    // rename failure is more useful than trying another name.
    while taken(&candidate) && n < 100 {
        candidate = with(&format!("{OLD_SUFFIX}-{n}"));
        n += 1;
    }
    candidate
}

/// Whether a name found directly in the installation directory belongs to
/// somebody else and must stay out of the file-set comparison.
///
/// `uninstall.exe` is the installer's and is never swapped. A leading dot is
/// room left on purpose: the staging directory is one, and a portable copy will
/// want a marker file of its own — if such a marker counted as part of the
/// installation, a portable copy could never satisfy the file-set rule and
/// would never update in place.
///
/// **That room is a file's worth, and a portable copy will want a directory
/// too.** Whatever it keeps beside the executable — its own `mpv.conf`, the
/// webview's data store, a torrent root — has to sit under a dot-prefixed
/// top-level name for this rule to pass, because anything else reads as a path
/// the payload dropped and sends the update to the installer, which for a
/// portable copy is precisely the wrong answer: it would install a second,
/// ordinary copy into the profile and leave the portable one where it was. If
/// that layout is chosen differently, two places have to learn the name — here
/// and `sweep_leftovers` — and it should come from the marker rather than be
/// written in again. The recursion is the other reason: a torrent root inside
/// the installation would be walked on every update and swept on every launch.
pub fn is_foreign_top_level(name: &str) -> bool {
    name.starts_with('.') || name.eq_ignore_ascii_case("uninstall.exe") || is_leftover(name)
}

// ---------------------------------------------------------------------------

#[cfg(windows)]
mod imp {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    use std::path::{Path, PathBuf};
    use tauri::Emitter;
    use tauri_plugin_updater::UpdaterExt;

    /// What `update_prepare` left for `update_commit`. A static rather than
    /// managed state: nothing else needs to see it, and it must survive a
    /// frontend that reloads between the two calls.
    static STAGED: std::sync::Mutex<Option<Staged>> = std::sync::Mutex::new(None);

    /// Held by the sweep and by both commands, so that a viewer who presses the
    /// update button while the startup sweep is still running does not have the
    /// staging directory deleted out from under the download.
    static WORK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct Staged {
        install: PathBuf,
        staging: PathBuf,
        version: String,
        /// Relative, `/`-separated, in manifest order.
        files: Vec<String>,
    }

    fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        m.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The directory the swap operates on, and the one the relaunch spawns out
    /// of — the same answer `portable` builds its own layout on, and one
    /// definition rather than two.
    fn install_dir() -> Result<PathBuf, String> {
        crate::portable::app_dir()
    }

    fn exe_name() -> Result<std::ffi::OsString, String> {
        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        exe.file_name()
            .map(|n| n.to_os_string())
            .ok_or_else(|| "the executable has no name".to_string())
    }

    /// Can we write here at all? A per-machine installation under Program
    /// Files cannot be, and neither can a copy on read-only media. Creating a
    /// file and renaming it inside the directory is what a swap needs; asking
    /// the permissions directly would mean reading an access-control list and
    /// guessing at the answer Windows would give.
    fn writable(staging: &Path) -> bool {
        if std::fs::create_dir_all(staging).is_err() {
            return false;
        }
        let probe = staging.join("write.probe");
        let moved = staging.join("write.probe.moved");
        let ok = std::fs::write(&probe, b"frameplayer").is_ok()
            && std::fs::rename(&probe, &moved).is_ok()
            && std::fs::remove_file(&moved).is_ok();
        let _ = std::fs::remove_file(&probe);
        let _ = std::fs::remove_file(&moved);
        ok
    }

    /// The installation as it is on disk: relative, `/`-separated, sorted.
    fn walk_installed(install: &Path) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        collect(install, install, true, &mut out)?;
        out.sort();
        Ok(out)
    }

    fn collect(
        root: &Path,
        dir: &Path,
        top: bool,
        out: &mut Vec<String>,
    ) -> Result<(), String> {
        let entries =
            std::fs::read_dir(dir).map_err(|e| format!("reading {}: {e}", dir.display()))?;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy().to_string();
            if top && is_foreign_top_level(&name) {
                continue;
            }
            if is_leftover(&name) {
                continue;
            }
            let path = entry.path();
            let kind = match entry.file_type() {
                Ok(k) => k,
                Err(_) => continue,
            };
            if kind.is_dir() {
                collect(root, &path, false, out)?;
            } else {
                let rel = path
                    .strip_prefix(root)
                    .map_err(|_| format!("{} is outside {}", path.display(), root.display()))?;
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
        Ok(())
    }

    fn sha256_file(path: &Path) -> std::io::Result<String> {
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(hex(&hasher.finalize()))
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// The plugin's updater, pointed at one manifest key. `None` is the
    /// plugin's own default, which on Windows resolves to the installer key.
    fn updater(
        app: &AppHandle,
        target: Option<&str>,
    ) -> Result<tauri_plugin_updater::Updater, String> {
        #[allow(unused_mut)]
        let mut builder = app.updater_builder();
        if let Some(target) = target {
            builder = builder.target(target);
        }
        // The test seam, and only in a debug build: a release binary cannot be
        // pointed at another endpoint or another signing key by anything in the
        // environment. See "Testing the Windows update" in docs/distribution.md
        // for the whole procedure; an `http://` endpoint is accepted here for
        // the same reason, the plugin allowing it only under debug_assertions.
        #[cfg(debug_assertions)]
        {
            if let Ok(url) = std::env::var("FP_UPDATE_ENDPOINT") {
                let parsed = url.parse().map_err(|e| format!("FP_UPDATE_ENDPOINT: {e}"))?;
                builder = builder
                    .endpoints(vec![parsed])
                    .map_err(|e| format!("FP_UPDATE_ENDPOINT: {e}"))?;
                eprintln!("[update] endpoint overridden: {url}");
            }
            if let Ok(key) = std::env::var("FP_UPDATE_PUBKEY") {
                builder = builder.pubkey(key);
                eprintln!("[update] signing key overridden");
            }
        }
        builder.build().map_err(|e| e.to_string())
    }

    pub async fn check(app: AppHandle) -> Result<Option<Announcement>, String> {
        // The in-place key first. `Ok(None)` from it is a complete answer —
        // the manifest was read and holds nothing newer — so only the error of
        // the key being absent falls through to the installer key.
        match updater(&app, Some(ZIP_TARGET))?.check().await {
            Ok(Some(update)) => {
                return Ok(Some(Announcement {
                    version: update.version,
                    body: update.body,
                    in_place: true,
                }))
            }
            Ok(None) => return Ok(None),
            Err(e) => eprintln!("[update] no in-place payload announced: {e}"),
        }
        let update = updater(&app, None)?
            .check()
            .await
            .map_err(|e| e.to_string())?;
        Ok(update.map(|update| Announcement {
            version: update.version,
            body: update.body,
            in_place: false,
        }))
    }

    pub async fn prepare(app: AppHandle) -> Result<Plan, String> {
        let install = install_dir()?;
        let staging = install.join(STAGING_DIR);

        if !writable(&staging) {
            let _ = std::fs::remove_dir_all(&staging);
            return Ok(Plan::no_swap(&app, format!(
                "{} is not writable",
                install.display()
            )));
        }

        // Every way out of here that is not the swap takes the staging
        // directory with it: the probe above created it, and an empty `.update`
        // left in somebody's installation is a thing to wonder about. A swap
        // that goes ahead clears it in `commit` instead.
        let update = match updater(&app, Some(ZIP_TARGET))?.check().await {
            Ok(Some(u)) => u,
            Ok(None) => {
                let _ = std::fs::remove_dir_all(&staging);
                return Ok(Plan::no_swap(&app, "the manifest announces no newer version"));
            }
            // A release whose zip failed to build has no such key, and the
            // check reports exactly that. The installer still has one.
            Err(e) => {
                let _ = std::fs::remove_dir_all(&staging);
                return Ok(Plan::no_swap(&app, format!("no in-place payload: {e}")));
            }
        };
        let announced = update.version.clone();

        let app_for_progress = app.clone();
        let mut total = 0u64;
        let mut done = 0u64;
        let mut last = u64::MAX;
        let bytes = update
            .download(
                |chunk, content_length| {
                    if total == 0 {
                        total = content_length.unwrap_or(0);
                    }
                    done += chunk as u64;
                    // Every whole percent, not every chunk: a hundred megabytes
                    // arrive in thousands of them and each emit crosses into
                    // the webview. `checked_div` covers the first chunk, which
                    // is the one that learns the length.
                    if let Some(pct) = (done * 100).checked_div(total) {
                        if pct != last {
                            last = pct;
                            let _ = app_for_progress.emit(PROGRESS_EVENT, pct);
                        }
                    }
                },
                || {},
            )
            .await
            .map_err(|e| format!("downloading the update: {e}"))?;

        // From here on nothing touches the network, and every failure is a
        // reason to use the installer rather than a reason to stop: the viewer
        // asked for an update and there is one to be had.
        //
        // On a thread of its own, and not only because a command's future has
        // to be `Send`: this digests two hundred and sixty megabytes and writes
        // what changed, which is seconds of one core and has no business on the
        // runtime that is also carrying the player's own commands.
        let staged = tauri::async_runtime::spawn_blocking(move || {
            let _work = lock(&WORK);
            match stage(bytes, &install, &staging, &announced) {
                Ok(staged) => Ok(staged),
                Err(reason) => {
                    let _ = std::fs::remove_dir_all(&staging);
                    Err(reason)
                }
            }
        })
        .await
        .map_err(|e| format!("staging the update: {e}"))?;
        let staged = match staged {
            Ok(staged) => staged,
            Err(reason) => {
                eprintln!("[update] falling back to the installer: {reason}");
                return Ok(Plan::no_swap(&app, reason));
            }
        };

        let plan = Plan {
            mode: "inplace",
            version: Some(staged.version.clone()),
            files: staged.files.len(),
            reason: None,
        };
        eprintln!(
            "[update] staged {} ({} file(s) to swap)",
            staged.version,
            staged.files.len()
        );
        *lock(&STAGED) = Some(staged);
        Ok(plan)
    }

    /// Unpack what has to change, and nothing else. A file whose digest already
    /// matches is not extracted at all: the executable alone changes in most
    /// releases, and the two hundred and sixty megabytes behind it do not need
    /// to be written to disk to be left where they are.
    fn stage(
        bytes: Vec<u8>,
        install: &Path,
        staging: &Path,
        announced: &str,
    ) -> Result<Staged, String> {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
            .map_err(|e| format!("the payload is not a zip: {e}"))?;

        let manifest: Manifest = {
            let mut file = zip
                .by_name(MANIFEST_NAME)
                .map_err(|e| format!("the payload has no {MANIFEST_NAME}: {e}"))?;
            let mut text = String::new();
            file.read_to_string(&mut text)
                .map_err(|e| format!("reading {MANIFEST_NAME}: {e}"))?;
            serde_json::from_str(&text).map_err(|e| format!("{MANIFEST_NAME}: {e}"))?
        };
        check_manifest(&manifest)?;

        // The payload and the release have to be the same version, or a
        // mismatched pair of artifacts would install one version under another
        // one's name.
        if manifest.version != announced {
            return Err(format!(
                "the manifest announces {announced} and the payload carries {}",
                manifest.version
            ));
        }

        // Hygiene rather than safety — every extraction below is by a name
        // built from the manifest, so a stray entry could not be written
        // anywhere. A payload with one is still a payload built wrong.
        let prefix = format!("{}/", manifest.root);
        for name in zip.file_names() {
            if name == MANIFEST_NAME || name.starts_with(&prefix) {
                continue;
            }
            return Err(format!("the payload has {name} outside {prefix}"));
        }

        let incoming: Vec<String> = manifest.files.iter().map(|e| e.path.clone()).collect();
        let installed = walk_installed(install)?;
        file_sets_match(&installed, &incoming)?;

        // A fresh staging directory: what a previous attempt left could be a
        // different version's files under the same names.
        let _ = std::fs::remove_dir_all(staging);
        std::fs::create_dir_all(staging)
            .map_err(|e| format!("creating {}: {e}", staging.display()))?;

        let mut needed = Vec::new();
        for entry in &manifest.files {
            let rel = safe_rel(&entry.path)?;
            let current = install.join(&rel);
            // Size first: a digest costs a read of the whole file, and a
            // changed file almost always changed size.
            let unchanged = std::fs::metadata(&current)
                .map(|m| m.len() == entry.size)
                .unwrap_or(false)
                && sha256_file(&current)
                    .map(|h| h == entry.sha256)
                    .unwrap_or(false);
            if unchanged {
                continue;
            }
            let target = staging.join(&rel);
            extract(&mut zip, &format!("{prefix}{}", entry.path), &target, entry)?;
            needed.push(entry.path.clone());
        }

        Ok(Staged {
            install: install.to_path_buf(),
            staging: staging.to_path_buf(),
            version: manifest.version,
            files: needed,
        })
    }

    /// One entry out of the payload, hashed as it is written. Verifying what
    /// landed rather than what was read is the point: a full disk writes a
    /// short file and reports nothing.
    fn extract<R: std::io::Read + std::io::Seek>(
        zip: &mut zip::ZipArchive<R>,
        name: &str,
        target: &Path,
        entry: &Entry,
    ) -> Result<(), String> {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        let mut source = zip
            .by_name(name)
            .map_err(|e| format!("the payload has no {name}: {e}"))?;
        let mut file = std::fs::File::create(target)
            .map_err(|e| format!("creating {}: {e}", target.display()))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut written = 0u64;
        loop {
            let n = source
                .read(&mut buf)
                .map_err(|e| format!("reading {name}: {e}"))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n])
                .map_err(|e| format!("writing {}: {e}", target.display()))?;
            written += n as u64;
        }
        file.flush()
            .map_err(|e| format!("writing {}: {e}", target.display()))?;
        drop(file);
        if written != entry.size {
            return Err(format!(
                "{} unpacked to {written} bytes, the manifest says {}",
                entry.path, entry.size
            ));
        }
        let digest = hex(&hasher.finalize());
        if digest != entry.sha256 {
            return Err(format!("{} does not match its digest", entry.path));
        }
        Ok(())
    }

    /// On a thread, like the staging above: this one is renames rather than
    /// bytes, but it ends in `exit(0)` and must not be doing that from inside
    /// the runtime's own future.
    pub async fn commit(app: AppHandle) -> Result<(), String> {
        tauri::async_runtime::spawn_blocking(move || commit_now(app))
            .await
            .map_err(|e| format!("applying the update: {e}"))?
    }

    fn commit_now(app: AppHandle) -> Result<(), String> {
        let _work = lock(&WORK);
        let staged = lock(&STAGED)
            .take()
            .ok_or_else(|| "nothing has been staged".to_string())?;

        let journal = staged.staging.join(JOURNAL_NAME);
        // Before the first rename, so that a crash in the middle of the swap is
        // something the next launch can finish rather than half a version.
        let _ = std::fs::write(&journal, staged.files.join("\n"));

        if let Err(e) = swap(&staged.install, &staged.staging, &staged.files) {
            let _ = std::fs::remove_file(&journal);
            let _ = std::fs::remove_dir_all(&staged.staging);
            return Err(e);
        }
        let _ = std::fs::remove_file(&journal);
        // What is left of staging is the directories the moved files came out
        // of. The sweep on the next launch would get it either way; doing it
        // here means the installation is clean the moment the swap is.
        let _ = std::fs::remove_dir_all(&staged.staging);

        // After the files and never before: an installation that says it is the
        // new version while holding the old one is worse than the other way
        // round. And never fatal — the swap has already happened, and a stale
        // line in Apps & Features is not worth refusing an update over.
        if let Err(e) = registry_version(&app, &staged.install, &staged.version) {
            eprintln!("[update] could not update the uninstall entry: {e}");
        }

        // Nothing was different, so this process is already running the
        // payload's own files and there is nothing for a restart to load. Only
        // a release that republished identical binaries under a new version
        // reaches this, and relaunching for it would be a window closing and
        // reopening for no reason. (It is also what a test rig that announces a
        // version its binary does not report hits, where the relaunch turns
        // into a loop: measured, one restart every twenty seconds.)
        if staged.files.is_empty() {
            eprintln!("[update] nothing to swap; the installation already matches {}", staged.version);
            return Ok(());
        }

        let exe = staged.install.join(exe_name()?);
        // Explicitly, rather than whatever `current_exe` now reports: the name
        // still resolves (the old image was renamed out of the way, not this
        // path), but saying so beats relying on it.
        std::process::Command::new(&exe)
            .arg(format!("{AFTER_UPDATE_ARG}{}", std::process::id()))
            .current_dir(&staged.install)
            .spawn()
            .map_err(|e| format!("relaunching {}: {e}", exe.display()))?;

        eprintln!("[update] swapped {} file(s), relaunching", staged.files.len());
        app.cleanup_before_exit();
        std::process::exit(0);
    }

    /// Target to `*.fp-old`, incoming into its place, one file at a time, and
    /// every move recorded so that a failure part-way through puts back what it
    /// moved. Idempotent on purpose: an entry whose incoming file is no longer
    /// in staging has already been applied, which is what lets the next launch
    /// run the same list again after a crash.
    pub(super) fn swap(install: &Path, staging: &Path, files: &[String]) -> Result<(), String> {
        let mut undo: Vec<Undo> = Vec::new();

        for rel in files {
            let rel = safe_rel(rel)?;
            let target = install.join(&rel);
            let incoming = staging.join(&rel);
            if !incoming.exists() {
                continue;
            }
            if target.exists() {
                let old = old_name(&target, |p| p.exists());
                if let Err(e) = rename_retry(&target, &old) {
                    return Err(rolled_back(undo, format!("{}: {e}", target.display())));
                }
                undo.push(Undo {
                    from: old,
                    to: target.clone(),
                });
            }
            if let Err(e) = rename_retry(&incoming, &target) {
                return Err(rolled_back(undo, format!("{}: {e}", target.display())));
            }
            undo.push(Undo {
                from: target,
                to: incoming,
            });
        }
        Ok(())
    }

    /// One move the rollback would have to make: `from` back to `to`.
    struct Undo {
        from: PathBuf,
        to: PathBuf,
    }

    /// Undo in reverse and report why. Best effort by necessity: the thing that
    /// stopped the swap may equally stop the undo, and there is nothing further
    /// to try.
    fn rolled_back(undo: Vec<Undo>, cause: String) -> String {
        let mut failures = 0;
        for step in undo.into_iter().rev() {
            if rename_retry(&step.from, &step.to).is_err() {
                failures += 1;
            }
        }
        if failures > 0 {
            eprintln!("[update] rollback left {failures} file(s) out of place");
            format!("the swap failed and could not be fully undone ({cause})")
        } else {
            format!("the swap failed and was undone ({cause})")
        }
    }

    fn rename_retry(from: &Path, to: &Path) -> std::io::Result<()> {
        let mut last = None;
        for attempt in 0..RENAME_TRIES {
            match std::fs::rename(from, to) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last = Some(e);
                    if attempt + 1 < RENAME_TRIES {
                        std::thread::sleep(RENAME_PAUSE);
                    }
                }
            }
        }
        Err(last.unwrap_or_else(|| std::io::Error::other("rename was never attempted")))
    }

    /// `DisplayVersion` and `EstimatedSize` in the uninstall entry, which is
    /// what Apps & Features shows. Only the entry that points at *this*
    /// directory is touched, and a portable copy has none to touch.
    fn registry_version(app: &AppHandle, install: &Path, version: &str) -> Result<(), String> {
        let mut size_kb = 0u64;
        for rel in walk_installed(install)? {
            if let Ok(meta) = std::fs::metadata(install.join(rel.replace('/', "\\"))) {
                size_kb += meta.len() / 1024;
            }
        }

        // The entry that names this directory, located once in portable.rs —
        // the same lookup that answers "did an installer put this copy here",
        // so the two cannot disagree. A per-machine installation keeps its
        // entry in the machine's hive and needs elevation to write it, so a
        // failure there is reported and nothing more.
        let Some((hive, path)) = crate::portable::uninstall_entry(app) else {
            return Err(format!("no uninstall entry points at {}", install.display()));
        };
        let key = hive
            .create(&path)
            .map_err(|e| format!("opening the uninstall entry for writing: {e}"))?;
        key.set_string("DisplayVersion", version)
            .map_err(|e| format!("DisplayVersion: {e}"))?;
        key.set_u32("EstimatedSize", size_kb as u32)
            .map_err(|e| format!("EstimatedSize: {e}"))?;
        Ok(())
    }

    // -----------------------------------------------------------------------

    pub fn wait_for_predecessor() {
        let Some(pid) = std::env::args_os().find_map(|arg| {
            arg.to_str()
                .and_then(|a| a.strip_prefix(AFTER_UPDATE_ARG))
                .and_then(|p| p.parse::<u32>().ok())
        }) else {
            return;
        };
        use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
        // SYNCHRONIZE is a standard access right that every kind of object
        // takes, and the binding generator files it under the file rights
        // rather than the process ones. It is the same constant.
        use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
        use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};
        // SAFETY: SYNCHRONIZE alone, on a handle this function owns and closes.
        unsafe {
            let handle = OpenProcess(SYNCHRONIZE, 0, pid);
            if handle.is_null() {
                // Already gone, which is the common case: the predecessor exits
                // the moment it has spawned this process.
                return;
            }
            let waited = WaitForSingleObject(handle, PREDECESSOR_WAIT_MS);
            CloseHandle(handle);
            if waited != WAIT_OBJECT_0 {
                eprintln!("[update] the previous copy (pid {pid}) is still running");
            }
        }
    }

    pub fn sweep_in_background() {
        std::thread::spawn(|| {
            let _work = lock(&WORK);
            let Ok(install) = install_dir() else { return };
            let staging = install.join(STAGING_DIR);

            // A swap that was interrupted gets finished before anything is
            // deleted: its staging files are the only copy of the new version.
            // The process keeps running on the images it already has mapped,
            // exactly as it would after an ordinary swap, and the next launch
            // is the new version — so there is nothing to relaunch for, and not
            // relaunching is also what keeps a swap that cannot finish from
            // looping.
            let journal = staging.join(JOURNAL_NAME);
            if let Ok(text) = std::fs::read_to_string(&journal) {
                let files: Vec<String> = text
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(str::to_string)
                    .collect();
                eprintln!(
                    "[update] an interrupted swap left {} file(s) to finish",
                    files.len()
                );
                match swap(&install, &staging, &files) {
                    Ok(()) => eprintln!("[update] the interrupted swap is finished"),
                    Err(e) => eprintln!("[update] could not finish the swap: {e}"),
                }
                let _ = std::fs::remove_file(&journal);
            }

            let mut removed = 0usize;
            let mut kept = 0usize;
            sweep_leftovers(&install, &mut removed, &mut kept);
            if staging.exists() && !remove_either(&staging) {
                kept += 1;
            }
            if removed > 0 || kept > 0 {
                eprintln!("[update] swept {removed} leftover(s), {kept} still in use");
            }
        });
    }

    /// Whatever is at this path, gone. A directory is the only thing we ever
    /// put there, so this is for the one that is not: something else holding
    /// the name must not make the sweep report a file as still in use.
    fn remove_either(path: &Path) -> bool {
        if path.is_dir() {
            std::fs::remove_dir_all(path).is_ok()
        } else {
            std::fs::remove_file(path).is_ok()
        }
    }

    fn sweep_leftovers(dir: &Path, removed: &mut usize, kept: &mut usize) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if is_leftover(&name) {
                let gone = remove_either(&path);
                // Still mapped by a process that has not exited; the launch
                // after this one gets it.
                if gone {
                    *removed += 1;
                } else {
                    *kept += 1;
                }
                continue;
            }
            // The staging directory is swept whole, by its own name.
            if name == STAGING_DIR {
                continue;
            }
            if entry.file_type().map(|k| k.is_dir()).unwrap_or(false) {
                sweep_leftovers(&path, removed, kept);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(paths: &[&str]) -> Manifest {
        Manifest {
            version: "1.25.0".into(),
            root: "Frame Player".into(),
            files: paths
                .iter()
                .map(|p| Entry {
                    path: (*p).into(),
                    size: 1,
                    sha256: "a".repeat(64),
                })
                .collect(),
        }
    }

    #[test]
    fn relative_paths_only() {
        assert!(safe_rel("frameplayer.exe").is_ok());
        assert!(safe_rel("lib/libmpv-2.dll").is_ok());
        assert!(safe_rel("lib/.libs-key").is_ok());
        for bad in [
            "",
            "/etc/passwd",
            "C:/Windows/system32/kernel32.dll",
            "..\\..\\evil.dll",
            "../evil.dll",
            "lib/../../evil.dll",
            "lib//x.dll",
            "lib\\x.dll",
            "./x.dll",
        ] {
            assert!(safe_rel(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn manifest_has_to_describe_one_installation() {
        assert!(check_manifest(&manifest(&["frameplayer.exe"])).is_ok());

        let mut empty = manifest(&[]);
        empty.files.clear();
        assert!(check_manifest(&empty).is_err());

        let mut nested = manifest(&["frameplayer.exe"]);
        nested.root = "a/b".into();
        assert!(check_manifest(&nested).is_err());

        let mut no_root = manifest(&["frameplayer.exe"]);
        no_root.root = String::new();
        assert!(check_manifest(&no_root).is_err());

        // Windows does not distinguish these two, so neither may we.
        let twice = manifest(&["lib/x.dll", "lib/X.DLL"]);
        assert!(check_manifest(&twice).is_err());

        let mut bad_digest = manifest(&["frameplayer.exe"]);
        bad_digest.files[0].sha256 = "nope".into();
        assert!(check_manifest(&bad_digest).is_err());
    }

    #[test]
    fn a_changed_file_set_is_refused_and_says_which_way() {
        let installed: Vec<String> = ["frameplayer.exe", "lib/libmpv-2.dll"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(file_sets_match(&installed, &installed).is_ok());

        // Case alone is not a change.
        let shouted: Vec<String> = ["FramePlayer.exe", "lib/LIBMPV-2.dll"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(file_sets_match(&installed, &shouted).is_ok());

        let added: Vec<String> = ["frameplayer.exe", "lib/libmpv-2.dll", "lib/new.dll"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let why = file_sets_match(&installed, &added).unwrap_err();
        assert!(why.contains("lib/new.dll"), "{why}");
        assert!(why.contains("added"), "{why}");

        let dropped: Vec<String> = ["frameplayer.exe"].iter().map(|s| s.to_string()).collect();
        let why = file_sets_match(&installed, &dropped).unwrap_err();
        assert!(why.contains("lib/libmpv-2.dll"), "{why}");
        assert!(why.contains("dropped"), "{why}");
    }

    #[test]
    fn leftovers_are_recognised_by_their_suffix_only() {
        assert!(is_leftover("frameplayer.exe.fp-old"));
        assert!(is_leftover("frameplayer.exe.fp-old-2"));
        assert!(is_leftover("frameplayer.exe.FP-OLD-17"));
        assert!(!is_leftover("frameplayer.exe"));
        assert!(!is_leftover("notes.fp-older"));
        assert!(!is_leftover("x.fp-old-"));
        assert!(!is_leftover("x.fp-old-2a"));
    }

    #[test]
    fn a_leftover_that_cannot_be_deleted_gets_a_number() {
        let target = std::path::Path::new("D:\\app\\frameplayer.exe");
        assert_eq!(
            old_name(target, |_| false),
            std::path::PathBuf::from("D:\\app\\frameplayer.exe.fp-old")
        );
        let taken = |p: &std::path::Path| {
            p.to_string_lossy().ends_with(".fp-old") || p.to_string_lossy().ends_with(".fp-old-2")
        };
        assert_eq!(
            old_name(target, taken),
            std::path::PathBuf::from("D:\\app\\frameplayer.exe.fp-old-3")
        );
        // And every name it produces is one the sweep will recognise.
        for name in [
            old_name(target, |_| false),
            old_name(target, taken),
        ] {
            let name = name.file_name().unwrap().to_string_lossy().to_string();
            assert!(is_leftover(&name), "{name}");
        }
    }

    #[test]
    fn the_installers_own_files_are_not_part_of_the_installation() {
        assert!(is_foreign_top_level("uninstall.exe"));
        assert!(is_foreign_top_level("Uninstall.exe"));
        assert!(is_foreign_top_level(".update"));
        // Room for the marker a portable copy will carry.
        assert!(is_foreign_top_level(".portable"));
        assert!(is_foreign_top_level("frameplayer.exe.fp-old"));
        assert!(!is_foreign_top_level("frameplayer.exe"));
        assert!(!is_foreign_top_level("lib"));
    }

    /// The whole of the swap, against a real directory: the renames, the
    /// rollback when one of them cannot be made, and running the same list
    /// twice, which is what the next launch does after an interrupted one.
    #[cfg(windows)]
    mod swapping {
        use std::fs;
        use std::path::{Path, PathBuf};

        fn scratch(name: &str) -> PathBuf {
            let dir = std::env::temp_dir().join(format!("fp-update-test-{name}"));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("install/lib")).unwrap();
            fs::create_dir_all(dir.join("install/.update/lib")).unwrap();
            fs::write(dir.join("install/frameplayer.exe"), b"old exe").unwrap();
            fs::write(dir.join("install/lib/libmpv-2.dll"), b"old dll").unwrap();
            fs::write(dir.join("install/.update/frameplayer.exe"), b"new exe").unwrap();
            fs::write(dir.join("install/.update/lib/libmpv-2.dll"), b"new dll").unwrap();
            dir
        }

        fn files() -> Vec<String> {
            vec!["frameplayer.exe".to_string(), "lib/libmpv-2.dll".to_string()]
        }

        fn read(p: &Path) -> String {
            fs::read_to_string(p).unwrap_or_default()
        }

        #[test]
        fn swaps_every_file_and_keeps_the_old_one_beside_it() {
            let dir = scratch("plain");
            let install = dir.join("install");
            let staging = install.join(".update");
            super::super::imp::swap(&install, &staging, &files()).unwrap();

            assert_eq!(read(&install.join("frameplayer.exe")), "new exe");
            assert_eq!(read(&install.join("lib/libmpv-2.dll")), "new dll");
            assert_eq!(read(&install.join("frameplayer.exe.fp-old")), "old exe");
            assert_eq!(read(&install.join("lib/libmpv-2.dll.fp-old")), "old dll");
            let _ = fs::remove_dir_all(&dir);
        }

        #[test]
        fn running_the_same_list_again_changes_nothing() {
            let dir = scratch("again");
            let install = dir.join("install");
            let staging = install.join(".update");
            super::super::imp::swap(&install, &staging, &files()).unwrap();
            // What the next launch does with the journal it found.
            super::super::imp::swap(&install, &staging, &files()).unwrap();
            assert_eq!(read(&install.join("frameplayer.exe")), "new exe");
            // And it did not push the new file aside as if it were the old one.
            assert!(!install.join("frameplayer.exe.fp-old-2").exists());
            let _ = fs::remove_dir_all(&dir);
        }

        /// The failure the issue asked to be forced: a file held open by
        /// another handle, half way through the list.
        #[test]
        fn a_file_that_cannot_be_moved_puts_everything_back() {
            let dir = scratch("rollback");
            let install = dir.join("install");
            let staging = install.join(".update");

            // A handle opened without FILE_SHARE_DELETE, which is what makes a
            // rename fail with a sharing violation — an antivirus scanner or
            // another program holding the file. Note that it is *not* what a
            // loaded image does: `File::open` shares all three rights, and a
            // mapped image can be renamed and only refuses to be deleted
            // (measured; see the table at the top of this file). So the thing
            // this forces is the failure the rollback exists for, and not the
            // one the `.fp-old` naming exists for.
            use std::os::windows::fs::OpenOptionsExt;
            const FILE_SHARE_READ: u32 = 0x0000_0001;
            let held = fs::OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ)
                .open(install.join("lib/libmpv-2.dll"))
                .unwrap();

            let err = super::super::imp::swap(&install, &staging, &files()).unwrap_err();
            assert!(err.contains("undone"), "{err}");
            assert_eq!(read(&install.join("frameplayer.exe")), "old exe");
            assert_eq!(read(&install.join("lib/libmpv-2.dll")), "old dll");
            assert!(!install.join("frameplayer.exe.fp-old").exists());
            assert_eq!(read(&staging.join("frameplayer.exe")), "new exe");

            drop(held);
            let _ = fs::remove_dir_all(&dir);
        }
    }
}
