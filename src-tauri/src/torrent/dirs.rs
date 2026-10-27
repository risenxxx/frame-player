//! Where the data lives: the roots, the hash-named folders, the metadata cache.

use std::path::PathBuf;

use serde::Serialize;

use super::TorrentService;

/// Which directories this feature may read, write and delete in.
///
/// **Once the viewer can choose where the files go, "the torrent directory" is
/// three different questions** and answering them with one path is how a media
/// player deletes somebody's films. They are kept apart here so that every rule
/// below can be stated against the right one.
///
/// `state` is ours unconditionally: the metadata cache, librqbit's session store
/// and the record of the roots themselves. It stays in the app cache directory
/// whatever the viewer picks, which keeps the chosen folder pure video, leaves
/// `prune_orphaned_store` and the clear-cache button pointed where they always
/// were, and means a removable drive going missing costs data and not the
/// player's own bookkeeping.
///
/// `root` is where **new** torrents go. `roots` is everywhere data may already
/// be — the current root, every root used before it, and `state` — because
/// changing the setting must not orphan a season downloaded last week. Lookups
/// scan all of them; nothing is ever moved by a change of setting.
///
/// **The dangerous half is what may be deleted**, and it is `is_ours` that
/// decides. In `state` the player owns the whole directory, so a folder from the
/// older name-based layout is still measurable and deletable there. In a root
/// the viewer chose, the only folders that exist are `<Name> [<infohash>]` ones
/// we created, and anything else is theirs: it is not listed, not measured, not
/// counted and above all not deleted. Rather than trusting the UI to send only
/// what it was shown, the check lives here, in front of every destructive path.
#[derive(Clone, Debug)]
pub struct Dirs {
    pub state: PathBuf,
    pub root: PathBuf,
    pub roots: Vec<PathBuf>,
}

/// The chosen root, recorded beside the state rather than in localStorage.
///
/// A dot-name so `list` skips it, and read from disk on every use rather than
/// pushed in at startup: `download_dir` is reached by commands that can arrive
/// before the frontend has had a chance to say anything, and a preference that
/// is only sometimes in effect would put files in two places.
const ROOTS_FILE: &str = ".roots.json";

#[derive(Serialize, serde::Deserialize, Default)]
struct RootsRecord {
    /// Absent means the default: `state` itself.
    root: Option<String>,
    /// Roots used before, so their data stays findable.
    #[serde(default)]
    seen: Vec<String>,
}

impl Dirs {
    /// Everything in one directory, which is what a test wants and what the
    /// player did before the root became a choice.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn single(dir: PathBuf) -> Self {
        Self {
            state: dir.clone(),
            root: dir.clone(),
            roots: vec![dir],
        }
    }

    /// The cache directory rather than Downloads: watching a torrent is a side
    /// effect, not a download the viewer asked to keep, and a cache directory is
    /// the one place an OS and a user both understand as disposable.
    fn state_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
        let dir = crate::portable::cache_dir(app)?.join("torrents");
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {dir:?}: {e}"))?;
        Ok(dir)
    }

    pub(super) fn load(app: &tauri::AppHandle) -> Result<Self, String> {
        let state = Self::state_dir(app)?;
        let record: RootsRecord = std::fs::read(state.join(ROOTS_FILE))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();

        let root = record
            .root
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| state.clone());

        // `state` last and always: it is the one root that cannot go missing,
        // and the one where a legacy folder can still be found.
        let mut roots = vec![root.clone()];
        roots.extend(record.seen.iter().map(PathBuf::from));
        roots.push(state.clone());
        roots.dedup_by(|a, b| a == b);
        let mut seen = std::collections::HashSet::new();
        roots.retain(|r| seen.insert(r.clone()) && r.is_dir());

        Ok(Self { state, root, roots })
    }

    /// Record a new root, keeping the previous one in `seen` so its data stays
    /// findable. Creating it is part of choosing it: a root that cannot be
    /// written to is a setting that fails at the next torrent instead of now.
    pub(super) fn set_root(app: &tauri::AppHandle, root: Option<&str>) -> Result<(), String> {
        let state = Self::state_dir(app)?;
        let path = state.join(ROOTS_FILE);
        let mut record: RootsRecord = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();

        if let Some(root) = root {
            let root = PathBuf::from(root);
            std::fs::create_dir_all(&root)
                .map_err(|e| format!("cannot use {}: {e}", root.display()))?;
            // Refuse the state directory as a "chosen" root: it is already a
            // root, and recording it as one would make `is_ours` treat the whole
            // of it as a viewer's folder.
            let root = (root != state).then(|| root.to_string_lossy().into_owned());
            if let Some(previous) = record.root.replace(root.clone().unwrap_or_default()) {
                if !previous.is_empty() && Some(&previous) != root.as_ref() {
                    record.seen.push(previous);
                }
            }
            if root.is_none() {
                record.root = None;
            }
        } else if let Some(previous) = record.root.take() {
            record.seen.push(previous);
        }

        record.seen.sort();
        record.seen.dedup();
        record.seen.retain(|s| Some(s) != record.root.as_ref());

        let bytes = serde_json::to_vec(&record).map_err(|e| format!("{e}"))?;
        std::fs::write(&path, bytes).map_err(|e| format!("{e}"))
    }

    /// Is this directory one the player created, and may therefore delete?
    ///
    /// **The whole safety of a viewer-chosen root rests here.** Inside `state`
    /// the answer is yes for anything: the player owns that directory, and a
    /// folder from the older name-based layout has to stay deletable. Anywhere
    /// else the name must parse back to an info hash, which is a folder this
    /// build wrote and nothing a person would have made.
    pub(super) fn is_ours(&self, path: &std::path::Path) -> bool {
        let Some(parent) = path.parent() else {
            return false;
        };
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return false;
        };
        if !self.roots.iter().any(|r| same_dir(r, parent)) {
            return false;
        }
        same_dir(&self.state, parent) || folder_hash(name).is_some()
    }
}

/// Two paths naming the same directory.
///
/// Canonicalised, so a root recorded as `/Users/x/Films` and a parent arriving
/// as `/Users/x/./Films` — or through a symlinked home, which macOS hands out
/// routinely — are one directory rather than two. A path that cannot be
/// canonicalised does not exist, and nothing may be deleted under it.
pub(super) fn same_dir(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

impl TorrentService {
    pub(super) fn download_dir(app: &tauri::AppHandle) -> Result<Dirs, String> {
        Dirs::load(app)
    }
}

/// The torrent's own name, out of the metadata cached beside its data.
///
/// Cheap enough to do for every row of the storage list: the parse borrows from
/// the buffer rather than copying it, and the bulk of a `.torrent` is the piece
/// hashes, which are one byte string it never looks inside. Measured warm on a
/// 227 KB file (a ten-episode 4K season): **0.16 ms**, and a torrent cache holds
/// a handful of entries, not hundreds.
///
/// A file that will not parse gives no name and no error — it is a cache, the
/// row still measures and deletes, and `add` already falls back to the magnet
/// when these bytes turn out to be unusable.
pub(super) fn cached_name(dir: &std::path::Path, info_hash: &str) -> Option<String> {
    let bytes = std::fs::read(meta_path(dir, info_hash)).ok()?;
    let meta = librqbit::torrent_from_bytes(&bytes).ok()?;
    let name = meta.info.data.name?;
    let name = String::from_utf8_lossy(name.as_ref()).trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Put a `.torrent` fetched from somewhere else where `add` looks first.
///
/// For the RSS path (`feed.rs`): a feed hands over a `.torrent` URL, which is
/// the one source `add` cannot name a folder for — so the bytes are validated,
/// cached under their own info hash, and the torrent is then opened as a magnet
/// built from that hash, which finds this file and reaches no network at all.
/// Returns the hash and the torrent's own name.
pub(crate) fn cache_metadata(
    app: &tauri::AppHandle,
    bytes: &[u8],
) -> Result<(String, Option<String>), String> {
    let meta = librqbit::torrent_from_bytes(bytes).map_err(|_| "not_torrent".to_string())?;
    let hash = meta.info_hash.as_string();
    let name = meta
        .info
        .data
        .name
        .as_ref()
        .map(|n| String::from_utf8_lossy(n.as_ref()).trim().to_string())
        .filter(|n| !n.is_empty());
    let dirs = TorrentService::download_dir(app)?;
    let path = meta_path(&dirs.state, &hash);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{e}"))?;
    }
    std::fs::write(&path, bytes).map_err(|e| format!("{e}"))?;
    Ok((hash, name))
}

/// Where the cached torrent metadata for an info hash lives.
///
/// Beside the data folders rather than inside one, under a dot-name so
/// `torrent_list` skips it — it is ours, not a torrent, and it must not show up
/// as something occupying space that can be deleted.
pub(super) fn meta_path(dir: &std::path::Path, info_hash: &str) -> PathBuf {
    dir.join(".meta").join(format!("{info_hash}.torrent"))
}

/// A directory name this build wrote, as opposed to one from the older
/// name-based layout. Exactly 40 hex characters, which no torrent name is by
/// accident.
pub(super) fn is_info_hash(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The torrent name, made safe to be one path component on both platforms.
///
/// Everything here is a Windows rule; none of it costs anything on macOS, and
/// two of them are the kind of failure that only appears on somebody else's
/// machine. **Reserved device names** (`CON`, `NUL`, `COM1`…) cannot be a file
/// or a folder in any directory, with or without an extension, so a release
/// literally called `AUX` would be undeletable rather than merely odd — hence
/// the suffix that keeps the stem from being one. **A trailing dot or space is
/// silently dropped** by the Win32 layer, so a folder created as `Show.` is
/// afterwards addressed as `Show`, and every path we build from the name we
/// asked for misses it.
///
/// The length cap is the other half. A release name of ninety characters is
/// ordinary, the info hash adds 43 more, and inside the torrent there is often
/// another directory and a long file name on top — all of it against a path
/// limit that is still 260 by default on Windows. The readable part is a
/// convenience; the hash carries the identity, so it is the readable part that
/// gets cut. Cut on a **character** boundary, never a byte one, or a Cyrillic
/// name loses half a code point and stops being valid UTF-8.
fn sanitize_name(name: &str) -> String {
    const MAX_CHARS: usize = 80;
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];

    let mut out = String::with_capacity(name.len().min(MAX_CHARS * 4));
    for c in name.chars().take(MAX_CHARS) {
        // The Windows set, plus the control range, plus the separators — a name
        // is one path component and may not grow another.
        let safe = match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => ' ',
            // Ours, not the platform's: the hash is written in brackets and a
            // name carrying its own would make the suffix unparseable.
            '[' | ']' => ' ',
            c if (c as u32) < 0x20 => ' ',
            c => c,
        };
        out.push(safe);
    }

    // Runs of spaces collapse, because the substitutions above make them.
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut trimmed = collapsed.trim_end_matches('.').trim().to_string();
    // **The stem is what makes a name reserved, not the whole of it**:
    // `com1.2026` addresses the serial port exactly as `COM1` does, extension
    // or no extension. So the escape goes at the end of the stem rather than at
    // the end of the name, which is where it was first written and where it
    // achieves nothing.
    let stem_len = trimmed.find('.').unwrap_or(trimmed.len());
    if RESERVED
        .iter()
        .any(|r| trimmed[..stem_len].eq_ignore_ascii_case(r))
    {
        trimmed.insert(stem_len, '_');
    }
    trimmed
}

/// `<Name> [<infohash>]`, or the bare hash when there is no name to use.
///
/// **The folder keeps naming itself**, which is the whole reason this is a
/// suffix rather than an index file mapping hash to directory.
/// `ManagedTorrentOptions.output_folder` is `pub(crate)`, so the mapping from a
/// torrent to its directory can never be read back out of librqbit and has to
/// be recoverable from what is on disk — the same constraint that made the
/// folder a bare info hash in the first place, and the same reason `cached_name`
/// reads the torrent's own metadata instead of trusting our localStorage.
///
/// A side file would answer it too, and would be a second source of truth to
/// keep in step with the directory: a rename, a half-written index or a folder
/// restored from a backup all put the two out of agreement, and the failure is
/// a torrent that cannot be found or, worse, a delete aimed at the wrong path.
pub(super) fn folder_name(hash: &str, name: Option<&str>) -> String {
    let hash = hash.to_ascii_lowercase();
    match name.map(sanitize_name).filter(|n| !n.is_empty()) {
        Some(name) => format!("{name} [{hash}]"),
        None => hash,
    }
}

/// The info hash a folder belongs to, or `None` for one this build never wrote.
///
/// Both layouts: the bare hash, and the readable `<Name> [<hash>]`. Anything
/// else is a folder from the older name-based layout, which can be measured and
/// deleted but never resumed.
pub(super) fn folder_hash(folder: &str) -> Option<String> {
    if is_info_hash(folder) {
        return Some(folder.to_ascii_lowercase());
    }
    let inner = folder.strip_suffix(']')?.rsplit_once(" [")?.1;
    is_info_hash(inner).then(|| inner.to_ascii_lowercase())
}

/// The readable half of a folder name, when it has one.
pub(super) fn folder_label(folder: &str) -> Option<String> {
    let (label, hash) = folder.strip_suffix(']')?.rsplit_once(" [")?;
    (is_info_hash(hash) && !label.is_empty()).then(|| label.to_string())
}

/// Where this torrent's data is, or is about to go.
///
/// **A folder that already exists always wins**, whatever it is called. That is
/// what lets the readable name arrive without a migration: a season downloaded
/// under the old layout keeps its hash-named directory and keeps working, and
/// nothing on disk is moved by an upgrade.
/// Existing data is found in **any** root the player has ever used; new data
/// goes to the current one. That is what keeps changing the setting from
/// orphaning a season downloaded last week — nothing is moved, and the old
/// folder answers to the same info hash it always did.
pub(super) fn folder_for(dirs: &Dirs, hash: &str, name: Option<&str>) -> PathBuf {
    find_folder(&dirs.roots, hash).unwrap_or_else(|| dirs.root.join(folder_name(hash, name)))
}

/// The existing folder for an info hash, found by reading the directory.
///
/// A scan rather than a lookup, and cheap enough: a torrent cache holds a
/// handful of entries, and the alternative is the index file `folder_name`
/// exists to avoid.
pub(super) fn find_folder(roots: &[PathBuf], hash: &str) -> Option<PathBuf> {
    let hash = hash.to_ascii_lowercase();
    roots.iter().find_map(|dir| {
        std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
            let name = e.file_name().to_str()?.to_owned();
            (folder_hash(&name)? == hash && e.path().is_dir()).then(|| e.path())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Which directory names count as ours. The folder is the info hash (see
    /// `add`), and this is also what `torrent_forget` validates against, so it
    /// must not accept anything a torrent could be named.
    #[test]
    fn info_hash_folders() {
        assert!(is_info_hash("08ada5a7a6183aae1e09d831df6748d566095a10"));
        assert!(is_info_hash("0AEA879D465BF5169AE9AC370C33B69834713D92"));
        // 39 and 41 characters, and a non-hex letter in the right place.
        assert!(!is_info_hash("08ada5a7a6183aae1e09d831df6748d566095a1"));
        assert!(!is_info_hash("08ada5a7a6183aae1e09d831df6748d566095a100"));
        assert!(!is_info_hash("08ada5a7a6183aae1e09d831df6748d566095g10"));
        // Real torrent names, which must never be mistaken for ours.
        assert!(!is_info_hash("Dutton.Ranch.S01.2026.WEB-DLRip-AVC.x264.se"));
        assert!(!is_info_hash(""));
        assert!(!is_info_hash(".."));
    }

    /// A folder names its torrent **and** identifies it, and this is the pair
    /// of functions that has to agree about the second half. A name that does
    /// not round-trip is a torrent whose data cannot be found — the folder is
    /// still there, but nothing maps it back to an info hash, so it reads as a
    /// leftover from a layout nobody uses and offers only to be deleted.
    #[test]
    fn folders_name_their_torrent_and_still_identify_it() {
        const HASH: &str = "08ada5a7a6183aae1e09d831df6748d566095a10";

        // Both layouts answer, and the old one keeps working with no migration.
        assert_eq!(folder_hash(HASH).as_deref(), Some(HASH));
        assert_eq!(
            folder_hash("Dutton.Ranch.S01 [08ADA5A7A6183AAE1E09D831DF6748D566095A10]").as_deref(),
            Some(HASH),
            "the hash is the identity and its case is not part of it"
        );

        // Anything that is not ours must not be read as ours: `forget` deletes
        // by this answer, and `find_folder` hands data to whoever asks.
        assert_eq!(folder_hash("Dutton.Ranch.S01.2026.WEB-DLRip"), None);
        assert_eq!(folder_hash(""), None);
        assert_eq!(folder_hash(".."), None);
        // Brackets a *name* could carry, with nothing that parses inside them.
        assert_eq!(folder_hash("Some Show [1080p]"), None);
        assert_eq!(folder_hash("[08ada5a7a6183aae1e09d831df6748d566095a10]"), None);

        for name in [
            "Dutton.Ranch.S01.2026.WEB-DLRip-AVC.x264",
            // Every character Windows forbids, plus the separators, plus our
            // own brackets — none of which may reach a path.
            "A<B>C:D\"E/F\\G|H?I*J [K]",
            // A trailing dot is dropped by Win32 after the folder is created,
            // so the path we build afterwards would miss it.
            "Show Name...",
            // A reserved device name is not a legal folder in any directory.
            "NUL",
            "com1.2026",
            // Long, and not ASCII: a byte-wise cut here is invalid UTF-8.
            &"Сезон первый в очень длинном названии ".repeat(4),
        ] {
            let folder = folder_name(HASH, Some(name));
            assert_eq!(
                folder_hash(&folder).as_deref(),
                Some(HASH),
                "{name:?} produced {folder:?}, which does not identify its torrent"
            );
            let stem = folder.strip_suffix(&format!(" [{HASH}]")).unwrap();
            assert!(
                !stem.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*', '[', ']']),
                "{folder:?} carries a character that cannot be in a path"
            );
            assert!(!stem.ends_with(['.', ' ']), "{folder:?} ends in a dropped character");
            assert!(stem.chars().count() <= 81, "{folder:?} is not capped");
            for reserved in ["CON", "PRN", "AUX", "NUL", "COM1"] {
                assert!(
                    !stem.split('.').next().unwrap().eq_ignore_ascii_case(reserved),
                    "{folder:?} is a reserved device name"
                );
            }
        }

        // Nothing to name it with — a `.torrent` URL, or metadata that will not
        // parse — falls back to the layout that needs no name at all.
        assert_eq!(folder_name(HASH, None), HASH);
        assert_eq!(folder_name(HASH, Some("   ")), HASH);
    }
}
