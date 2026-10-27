//! What may be deleted: the storage list, forgetting, pruning, relocating, the sweep.

use std::path::PathBuf;
use std::time::Duration;

use super::dirs::{
    cached_name, find_folder, folder_hash, folder_label, is_info_hash, meta_path, same_dir, Dirs,
};
use super::{TorrentOnDisk, TorrentService};

/// How long a torrent may take to leave the session before its files are
/// touched anyway. See `drop_from_session` for why that is a deadline and not a
/// wait: everything on the other side of it is a deletion the viewer asked for,
/// and a step that only ever *helps* must not be able to cancel one.
const SESSION_DROP_TIMEOUT: Duration = Duration::from_secs(10);

impl TorrentService {
    /// Everything the torrent cache is holding, measured from disk.
    ///
    /// Deliberately does not consult the session or any record of ours: a folder
    /// left by a previous run, or by the older name-based layout, still has to
    /// be visible and deletable. That is what keeps storage management working
    /// when history is switched off and there is nothing remembered at all.
    ///
    /// It does not report session membership either, and that is the correction
    /// rather than an omission: a torrent stays in the session, paused, after
    /// the viewer closes the film, so "the session holds it" answered "playing
    /// right now" with yes for the rest of the run and the delete button was
    /// dead from the moment anything had been watched. Deleting one that is
    /// merely loaded is perfectly safe — `forget` takes it out of the session
    /// before removing the directory — so the only row that must refuse is the
    /// one mpv is streaming from, and that is the frontend's own fact.
    /// **Every root, and in a chosen one only what we created.** A viewer who
    /// points this at their film library must not find their own folders listed
    /// here with a delete button beside them — nor even measured, since walking
    /// somebody's whole media drive to print a number is its own kind of rude.
    /// `is_ours` is the rule, and it is the same one `forget` and
    /// `torrent_clear_cache` are written against.
    pub fn list(&self, dirs: &Dirs) -> Vec<TorrentOnDisk> {
        let mut out = Vec::new();
        for dir in &dirs.roots {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let Some(folder) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                if folder.starts_with('.') || !entry.path().is_dir() {
                    continue;
                }
                if !dirs.is_ours(&entry.path()) {
                    continue;
                }
                let info_hash = folder_hash(&folder);
                // The metadata's own name first — the folder's is sanitized and
                // truncated, so it is what a person reads in a file manager
                // rather than what the torrent is called. But it is a far better
                // fallback than nothing when the cache entry is missing: that
                // row used to read "Раздача без названия" beside a directory
                // that says on its face what it holds.
                let name = info_hash
                    .as_deref()
                    .and_then(|h| cached_name(&dirs.state, h))
                    .or_else(|| folder_label(&folder));
                out.push(TorrentOnDisk {
                    size: dir_size(&entry.path()),
                    path: entry.path().to_string_lossy().into_owned(),
                    info_hash,
                    name,
                    folder,
                });
            }
        }
        out.sort_by_key(|t| std::cmp::Reverse(t.size));
        out
    }

    /// Take a torrent out of the session — and never let that step decide
    /// whether its files may be touched.
    ///
    /// Everything below this has to get librqbit's hands off the data first: on
    /// Windows an open file cannot be removed at all, and on macOS it can, which
    /// is worse, since it leaves pieces being written through a handle to a path
    /// that no longer has a name. What none of those callers may do is make the
    /// disk work *conditional* on this succeeding, and two properties of
    /// `Session::delete` are why.
    ///
    /// It blocks on the torrent's own state lock, which a stream read holds for
    /// the whole of its `pread`, so on a torrent something is still reading it
    /// can sit there — and a bare `.await` sits with it, holding nothing but
    /// costing the caller everything. And it carries an `expect` on metadata
    /// that a magnet still resolving does not have; **a panic inside a Tauri
    /// command never resolves the promise at all**, so the frontend waits for
    /// ever, and the row it disabled while it waited stays disabled for the rest
    /// of the run. Both failures look identical from outside, and identical to
    /// each other: a delete button that does nothing, silently.
    ///
    /// Hence a spawned task with a deadline — spawning is what turns a panic
    /// into an error and a blocking call into somebody else's stuck thread — and
    /// a failure that is logged rather than returned. The directory is ours to
    /// remove whatever librqbit thinks of it, and a torrent left in the session
    /// store with no folder on disk is precisely what `prune_orphaned_store`
    /// collects at the next launch.
    pub(super) async fn drop_from_session(&self, hash: &str) {
        let session = {
            let mut inner = self.inner.lock().await;
            inner.torrents.remove(hash);
            inner.session.clone()
        };
        // Lazy session, and deleting from the start screen rarely follows
        // opening something — so this is the common path, and it is free.
        let Some(session) = session else {
            return;
        };
        // **By hash, never by the handle we happen to be holding.** `torrents`
        // is filled by `add` alone, so a handle covers a torrent opened in *this*
        // run and nothing else — a torrent the session restored at startup was
        // invisible to it, and deleting one therefore left librqbit's own record
        // behind, which restored the folder at the next launch as a nameless
        // zero-byte row. `Session::delete` resolves a hash against its own db,
        // which holds the restored ones too.
        //
        // `delete_files: false` — the files are the caller's to remove or to
        // keep, and doing it here would depend on librqbit agreeing with us
        // about which of them belong to this torrent.
        let Ok(id) = librqbit::api::TorrentIdOrHash::parse(hash) else {
            return;
        };
        let task = tauri::async_runtime::spawn(async move { session.delete(id, false).await });
        match tokio::time::timeout(SESSION_DROP_TIMEOUT, task).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(e))) => eprintln!("[torrent] {hash} could not leave the session: {e:#}"),
            Ok(Err(e)) => eprintln!("[torrent] {hash} panicked leaving the session: {e}"),
            Err(_) => eprintln!("[torrent] {hash} did not leave the session in time"),
        }
    }

    /// Hand one torrent's data over to its replacement.
    ///
    /// **BitTorrent has no way to add a file to a torrent** — the info hash
    /// covers the file list and every piece hash, so an uploader who adds an
    /// episode publishes a *different* torrent. Left alone that means
    /// re-downloading a season to gain one episode, because our folders are
    /// named after the info hash and the new one gets an empty one.
    ///
    /// The whole fix is a rename, done **before** the new torrent is added: the
    /// data ends up in the folder the new hash will ask for, and `overwrite`
    /// makes librqbit verify it and mark those pieces as held. Nothing else in
    /// `add` changes, and the "the folder carries the info hash" invariant that
    /// `torrent_list` and `torrent_forget` rely on survives intact — which it
    /// would not if the two were merged into one folder instead. That is also
    /// what keeps deletion unambiguous, the thing qBittorrent's shared-save-path
    /// approach gives up.
    ///
    /// The destination is deliberately the **bare hash** rather than a readable
    /// name: the replacement's own name is not known until its metadata is
    /// resolved, which is the very next thing `add` does — and `add` renames a
    /// hash-named folder the moment it can read a name for it. So the readable
    /// name arrives one step later, through the path that already does it,
    /// instead of this function guessing with the superseded torrent's name.
    pub async fn relocate(
        &self,
        dirs: &Dirs,
        old_hash: &str,
        new_hash: &str,
    ) -> Result<(), String> {
        if !is_info_hash(old_hash) || !is_info_hash(new_hash) {
            return Err("bad hash".into());
        }
        let (old_hash, new_hash) = (old_hash.to_ascii_lowercase(), new_hash.to_ascii_lowercase());
        if old_hash == new_hash {
            return Ok(());
        }
        let Some(from) = find_folder(&dirs.roots, &old_hash) else {
            return Err("nothing to move".into());
        };
        // **Beside the folder it replaces, not in the current root.** The two
        // may be different roots now, and `fs::rename` does not cross a volume —
        // so aiming at the chosen root would turn a rename of a 7.5 GB season
        // into a failure, or into a copy if somebody later "fixed" it that way.
        // The data stays where it already is, and `find_folder` looks in every
        // root regardless.
        let to = from.with_file_name(&new_hash);
        if !from.is_dir() {
            return Err("nothing to move".into());
        }
        // Asked by hash, not by path: the replacement may already have a folder
        // under its readable name, which `to` would not collide with and we
        // would then hand it a second one.
        if find_folder(&dirs.roots, &new_hash).is_some() {
            // The replacement already has data of its own. Merging two folders
            // is a different, riskier operation than a rename, and the caller
            // loses nothing by simply opening the new torrent normally.
            return Err("target exists".into());
        }

        // The old torrent must not be holding the files open across the rename.
        self.drop_from_session(&old_hash).await;

        std::fs::rename(&from, &to).map_err(|e| format!("{e}"))?;
        // The cached metadata describes the torrent that no longer owns this
        // folder. Leaving it would hand the *old* file list to the next open.
        let _ = std::fs::remove_file(meta_path(&dirs.state, &old_hash));
        Ok(())
    }

    /// Drop a torrent from the session and delete its data.
    ///
    /// Both halves, in that order: removing the directory while librqbit still
    /// holds the files open would leave it writing pieces into a folder that no
    /// longer exists, which on some filesystems recreates it.
    /// **This is the one command in the player that removes a directory tree,
    /// and it now points wherever the viewer told the player to put files.** So
    /// the guard is not "the name looks alright" but three separate facts, none
    /// of which the caller supplies: the path's parent is a root the player
    /// knows, the folder is one the player created (`is_ours`), and the whole
    /// thing resolves — a path that cannot be canonicalised is refused rather
    /// than guessed at. The UI only ever sends back a row it was given, but a
    /// destructive command must not be safe *because* of what the UI does.
    pub async fn forget(&self, dirs: &Dirs, path: &str) -> Result<u64, String> {
        let path = PathBuf::from(path);
        // Cheap and first: nothing below can turn a traversal into something
        // safe, and refusing it here keeps the rest reading as one rule.
        if path.components().any(|c| c == std::path::Component::ParentDir) {
            return Err("bad folder".into());
        }
        let Some(folder) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
            return Err("bad folder".into());
        };
        if !path.is_dir() || !dirs.is_ours(&path) {
            return Err("not a torrent folder".into());
        }

        // Parsed out of the folder rather than tested against the whole of it:
        // the name now carries the hash in brackets, and a folder with no
        // readable hash at all is one from the older name-based layout, which
        // can be measured and deleted but was never in any session.
        if let Some(hash) = folder_hash(&folder) {
            // Bounded, and its failure is only ever logged — see
            // `drop_from_session`. What must not happen is that a torrent the
            // session cannot let go of makes the data undeletable: the folder
            // below is ours by every check above, and a store entry pointing at
            // a folder that is gone is collected at the next launch.
            self.drop_from_session(&hash).await;
            // The cached metadata describes a torrent that is being thrown away.
            // Left behind it is a couple of hundred kilobytes per deleted
            // torrent, in a directory whose whole point is that its size is
            // accounted for.
            //
            // When there is no session at all — the common case, since the
            // session is lazy and deleting from the start screen rarely follows
            // opening something — the store entry is left for
            // `prune_orphaned_store` to collect the next time one is built. It
            // has to run there anyway, so doing it twice would be two places
            // that must agree about librqbit's file format instead of one.
            let _ = std::fs::remove_file(meta_path(&dirs.state, &hash));
        }

        let size = dir_size(&path);
        std::fs::remove_dir_all(&path).map_err(|e| format!("{e}"))?;
        Ok(size)
    }

    /// Delete some files of a torrent and keep the rest.
    ///
    /// **What makes this safe is `validate_fastresume`, not care on our part.**
    /// librqbit verifies at least one piece of every file its resume data claims
    /// to hold, plus a random sample of the others, and a single failure throws
    /// the whole bitfield away (`initializing.rs`). So a file removed behind its
    /// back cannot be served as zeros: the next open re-checks and finds it
    /// missing. The price is that re-check — measured at ~3.2 s for a 7.5 GB
    /// season — paid once, on the next open of this torrent.
    ///
    /// **The order is the part that has to be right.** The torrent leaves the
    /// session before anything is unlinked: on Windows an open file cannot be
    /// deleted at all, and on macOS it can, which is worse — librqbit keeps
    /// writing pieces through a handle to a file that no longer has a name.
    ///
    /// `.meta` is deliberately kept. The torrent is not being forgotten, only
    /// pruned, and that cached metadata is what makes reopening it cost 4.5 ms
    /// instead of a DHT lookup.
    ///
    /// Matching is by **file name against what is actually inside the folder**,
    /// which is both the simplest thing the caller can supply — it has names,
    /// not indices, and the two disagree the moment an uploader inserts an
    /// episode — and the reason no name can escape: nothing is joined onto the
    /// folder path, only entries found within it are considered.
    pub async fn forget_files(
        &self,
        dirs: &Dirs,
        path: &str,
        names: &[String],
    ) -> Result<u64, String> {
        let path = PathBuf::from(path);
        if path.components().any(|c| c == std::path::Component::ParentDir) {
            return Err("bad folder".into());
        }
        if !path.is_dir() || !dirs.is_ours(&path) {
            return Err("not a torrent folder".into());
        }
        if names.is_empty() {
            return Ok(0);
        }
        let wanted: std::collections::HashSet<&str> = names.iter().map(String::as_str).collect();

        if let Some(hash) = path.file_name().and_then(|n| n.to_str()).and_then(folder_hash) {
            self.drop_from_session(&hash).await;
        }

        let mut freed = 0u64;
        remove_named_files(&path, &wanted, &mut freed);
        Ok(freed)
    }
}

/// Delete every file under `dir` whose name is wanted, and any directory left
/// empty by doing so. `dir` itself stays: the torrent is being pruned, not
/// forgotten, and an absent folder would read as a torrent that vanished.
fn remove_named_files(
    dir: &std::path::Path,
    wanted: &std::collections::HashSet<&str>,
    freed: &mut u64,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Never followed: a link inside the folder points somewhere we were not
        // given permission to delete from.
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_symlink() {
            continue;
        }
        if meta.is_dir() {
            remove_named_files(&path, wanted, freed);
            // Best-effort and non-recursive: `remove_dir` refuses a directory
            // that still holds anything, which is exactly the check wanted here.
            let _ = std::fs::remove_dir(&path);
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !wanted.contains(name) {
            continue;
        }
        let size = file_disk_size(&meta);
        if std::fs::remove_file(&path).is_ok() {
            *freed += size;
        }
    }
}

/// The sweep itself, split from the command so a test can point it at a
/// directory holding somebody's films and prove they survive it.
pub(super) fn clear_all(dirs: &Dirs) -> u64 {
    let mut freed = 0u64;
    for dir in &dirs.roots {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let in_state = same_dir(&dirs.state, dir);
        for entry in entries.flatten() {
            let path = entry.path();
            let removed = if path.is_dir() {
                // `.meta` and `.session` are dot-named and `is_ours` refuses
                // them for the same reason `list` skips them: they are the
                // player's bookkeeping, not a torrent occupying space.
                let named = entry.file_name();
                let named = named.to_string_lossy();
                if named.starts_with('.') || !dirs.is_ours(&path) {
                    continue;
                }
                let size = dir_size(&path);
                std::fs::remove_dir_all(&path).is_ok().then_some(size)
            } else if in_state {
                let size = dir_size(&path);
                std::fs::remove_file(&path).is_ok().then_some(size)
            } else {
                None
            };
            freed += removed.unwrap_or(0);
        }
    }
    freed
}

/// Bytes a file actually occupies, which is **not** its length.
///
/// librqbit creates every file of a torrent at full size up front, and on a
/// filesystem with sparse files (APFS, and ext4/btrfs) only the blocks written
/// are allocated. Measured on a nine-episode season with one episode partly
/// fetched: `len()` reports **7.45 GB**, the blocks hold **142 MB**. Reporting
/// the former in a storage list would be worse than not having the list — it
/// tells the viewer to delete something to reclaim space that was never taken.
///
/// Windows keeps `len()`: NTFS needs an explicit `FSCTL_SET_SPARSE` to make a
/// file sparse and librqbit does not ask for one, so there the length *is* what
/// is allocated. Erring toward the honest figure on each platform rather than
/// one formula that is wrong on both.
#[cfg(unix)]
pub(super) fn file_disk_size(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    // `blocks()` is in 512-byte units by POSIX definition, regardless of the
    // filesystem's own block size.
    meta.blocks().saturating_mul(512)
}

#[cfg(not(unix))]
pub(super) fn file_disk_size(meta: &std::fs::Metadata) -> u64 {
    meta.len()
}

pub(crate) fn dir_size(path: &std::path::Path) -> u64 {
    // `symlink_metadata`: a symlink is measured as the link, never followed —
    // otherwise a link pointing out of the cache would be counted as ours, and
    // a cycle would not terminate.
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_symlink() {
        return 0;
    }
    if meta.is_file() {
        return file_disk_size(&meta);
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries.flatten().map(|e| dir_size(&e.path())).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// **The viewer points the player at their film library, and nothing of
    /// theirs is listed, measured or deleted.**
    ///
    /// This is the test the whole chosen-root feature exists behind. Every
    /// destructive path in the module is aimed at a directory that is now
    /// somebody's media drive, and each of them is asked here to leave alone a
    /// folder it did not create — including the sweep that used to remove every
    /// entry it found, which was correct while the directory was ours alone and
    /// is a way to erase a film collection the moment it is not.
    #[test]
    fn a_chosen_root_protects_what_it_did_not_create() {
        const HASH: &str = "08ada5a7a6183aae1e09d831df6748d566095a10";
        let base = std::env::temp_dir().join("frameplayer-chosen-root");
        let _ = std::fs::remove_dir_all(&base);

        let state = base.join("cache");
        let library = base.join("Films");
        // Ours, in the chosen root, exactly as `add` would have written it.
        let ours = library.join(format!("Season [{HASH}]"));
        // Theirs: a folder, and a loose file beside it.
        let theirs = library.join("Holiday 2019");
        let their_file = library.join("notes.txt");
        // A folder from the older name-based layout, which lives in the state
        // directory and must stay deletable there.
        let legacy = state.join("Some.Release.2024.WEB-DL");
        for d in [&state, &library, &ours, &theirs, &legacy] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(ours.join("ep1.mkv"), b"ours").unwrap();
        std::fs::write(theirs.join("beach.mp4"), b"theirs").unwrap();
        std::fs::write(&their_file, b"theirs").unwrap();
        std::fs::write(legacy.join("film.mkv"), b"legacy").unwrap();

        let dirs = Dirs {
            state: state.clone(),
            root: library.clone(),
            roots: vec![library.clone(), state.clone()],
        };
        let service = Arc::new(TorrentService::default());

        // Listing: ours from the chosen root, the legacy folder from the state
        // directory, and nothing of theirs — not even measured, since walking a
        // media drive to print a number is its own kind of rude.
        let listed = service.list(&dirs);
        let folders: Vec<&str> = listed.iter().map(|r| r.folder.as_str()).collect();
        assert!(folders.contains(&format!("Season [{HASH}]").as_str()));
        assert!(folders.contains(&"Some.Release.2024.WEB-DL"));
        assert!(!folders.contains(&"Holiday 2019"), "listed a folder we did not create");
        assert_eq!(folders.len(), 2, "listed something unexpected: {folders:?}");

        // Deleting: refused for theirs, and the data is still there afterwards.
        let forget = |p: &std::path::Path| {
            tauri::async_runtime::block_on(service.forget(&dirs, &p.to_string_lossy()))
        };
        assert!(forget(&theirs).is_err(), "offered to delete a folder we did not create");
        assert!(theirs.join("beach.mp4").is_file());
        // Not a root of ours at all, and a traversal that lands inside one.
        assert!(forget(&base).is_err());
        assert!(forget(&library.join("..").join("Films").join("Holiday 2019")).is_err());
        assert!(theirs.is_dir());

        // The sweep: same rule, and the loose file in a chosen root is theirs
        // too. Only the state directory is ours to tidy file by file.
        clear_all(&dirs);
        assert!(theirs.join("beach.mp4").is_file(), "cleared a folder we did not create");
        assert!(their_file.is_file(), "cleared a file we did not write");
        assert!(!ours.exists(), "did not clear our own torrent");
        assert!(!legacy.exists(), "did not clear the older layout in our own directory");

        let _ = std::fs::remove_dir_all(&base);
    }

    /// **Deleting the episodes you have watched keeps the ones you have not**,
    /// and keeps the torrent openable afterwards — which is the whole point of
    /// pruning rather than forgetting.
    #[test]
    fn watched_episodes_go_and_the_rest_stays() {
        const HASH: &str = "08ada5a7a6183aae1e09d831df6748d566095a10";
        let base = std::env::temp_dir().join("frameplayer-prune-files");
        let _ = std::fs::remove_dir_all(&base);

        let folder = base.join(format!("Season [{HASH}]"));
        let extras = folder.join("extras");
        std::fs::create_dir_all(&extras).unwrap();
        for (path, bytes) in [
            (folder.join("ep1.mkv"), &b"watched"[..]),
            (folder.join("ep2.mkv"), &b"watched"[..]),
            (folder.join("ep3.mkv"), &b"not yet"[..]),
            // In a subfolder, to prove the walk reaches it and then takes the
            // empty directory with it.
            (extras.join("ep0.mkv"), &b"watched"[..]),
        ] {
            std::fs::write(path, bytes).unwrap();
        }
        // Ours, and it must survive: the torrent is being pruned, not forgotten,
        // and this is what keeps reopening it free.
        let meta = meta_path(&base, HASH);
        std::fs::create_dir_all(meta.parent().unwrap()).unwrap();
        std::fs::write(&meta, b"metadata").unwrap();

        let dirs = Dirs::single(base.clone());
        let service = Arc::new(TorrentService::default());
        let names = ["ep1.mkv".to_string(), "ep2.mkv".to_string(), "ep0.mkv".to_string()];
        let freed = tauri::async_runtime::block_on(service.forget_files(
            &dirs,
            &folder.to_string_lossy(),
            &names,
        ))
        .unwrap();

        assert!(!folder.join("ep1.mkv").exists());
        assert!(!folder.join("ep2.mkv").exists());
        assert!(folder.join("ep3.mkv").is_file(), "deleted an episode nobody watched");
        assert!(!extras.exists(), "left an empty directory behind");
        assert!(folder.is_dir(), "removed the torrent instead of pruning it");
        assert!(meta.is_file(), "threw away the metadata that makes reopening free");
        assert!(freed > 0);

        // The same guards as `forget`: a folder we did not create, and a
        // traversal, are refused before anything is unlinked.
        let outside = base.join("Their Films");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("ep1.mkv"), b"theirs").unwrap();
        let dirs = Dirs {
            state: base.join("cache"),
            root: base.clone(),
            roots: vec![base.clone()],
        };
        std::fs::create_dir_all(&dirs.state).unwrap();
        let refused = tauri::async_runtime::block_on(service.forget_files(
            &dirs,
            &outside.to_string_lossy(),
            &names,
        ));
        assert!(refused.is_err(), "pruned a folder we did not create");
        assert!(outside.join("ep1.mkv").is_file());

        let _ = std::fs::remove_dir_all(&base);
    }

    /// `torrent_forget` deletes a directory by name, so the name is the only
    /// thing standing between it and the rest of the disk. Escapes are refused
    /// before anything is touched.
    #[test]
    fn forget_refuses_escapes() {
        let base = std::env::temp_dir().join("frameplayer-forget-guard");
        std::fs::create_dir_all(base.join("victim")).unwrap();
        let service = Arc::new(TorrentService::default());

        for bad in [
            "",
            "..",
            "../victim",
            "../../etc",
            "a/b",
            "a\\b",
            ".hidden",
            "sub/../../victim",
        ] {
            let r = tauri::async_runtime::block_on(service.forget(&Dirs::single(base.clone()), &base.join(bad).to_string_lossy()));
            assert!(r.is_err(), "{bad:?} should have been refused");
        }
        // The guard is not merely returning errors — nothing was removed.
        assert!(base.join("victim").is_dir());
        let _ = std::fs::remove_dir_all(&base);
    }
}
