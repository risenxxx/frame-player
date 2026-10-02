//! Where a release keeps its external subtitles and dubs.
//!
//! mpv finds external tracks by itself (`sub-auto`, `audio-file-auto`), but only
//! beside the video and in the directories `sub-file-paths`/`audio-file-paths`
//! name — and it does not descend into them. Releases nest: `RUS Sound/<studio>/`,
//! `Subs/<group>/`, `RUS Subs/`, one folder per dub. A fixed list of names cannot
//! cover that, and the studio names are not a list anybody could write.
//!
//! So this answers one question — which folders under the video's own folder
//! hold subtitle or audio files — and the frontend hands the answer to mpv's own
//! search before the file is loaded. That is deliberate rather than a
//! `sub-add` after the fact: a track mpv finds itself is guessed a language from
//! its name and takes part in `alang`/`slang` selection, and one added afterwards
//! does neither.
//!
//! The answer is a property of the folder, not of the episode, which is what
//! makes it safe for a queue that advances without asking again.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// mpv's own default `sub-auto-exts`.
const SUB_EXTS: &[&str] = &[
    "ass", "idx", "lrc", "mks", "pgs", "rt", "sbv", "scc", "smi", "srt", "ssa", "sub", "sup",
    "utf", "utf-8", "utf8", "vtt",
];

/// mpv's own default `audio-exts`.
const AUDIO_EXTS: &[&str] = &[
    "3gp", "aac", "ac3", "aif", "aiff", "amr", "ape", "au", "awb", "dts", "eac3", "flac", "m4a",
    "mka", "mp3", "oga", "ogg", "ogm", "opus", "thd", "wav", "wma", "wv",
];

/// Three levels covers `RUS Sound/<studio>/<variant>/` with one to spare.
const MAX_DEPTH: usize = 3;
/// Directory entries read in total. A video kept loose in a downloads folder
/// would otherwise have its whole tree walked before it could play; a release
/// folder is a few hundred entries at most.
const MAX_ENTRIES: usize = 4000;

#[derive(serde::Serialize, Default, Debug, PartialEq)]
pub struct ExternalDirs {
    /// Relative to the video's folder, `/`-separated, shallowest first.
    pub subs: Vec<String>,
    pub audio: Vec<String>,
}

fn ext_of(name: &str) -> Option<String> {
    Path::new(name).extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase())
}

/// Breadth-first, so when the entry budget runs out it is the deep corners
/// that go unread rather than a sibling at the top.
pub fn scan(root: &Path) -> ExternalDirs {
    let mut out = ExternalDirs::default();
    let mut queue: VecDeque<(PathBuf, String, usize)> = VecDeque::new();
    let mut budget = MAX_ENTRIES;

    // The video's own folder is mpv's already: only what is below it is ours.
    let Ok(top) = std::fs::read_dir(root) else { return out };
    for entry in top.flatten() {
        if budget == 0 {
            break;
        }
        budget -= 1;
        if let Some(name) = dir_name(&entry) {
            queue.push_back((entry.path(), name, 1));
        }
    }

    while let Some((dir, rel, depth)) = queue.pop_front() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let (mut has_sub, mut has_audio) = (false, false);
        for entry in rd.flatten() {
            if budget == 0 {
                break;
            }
            budget -= 1;
            if let Some(name) = dir_name(&entry) {
                if depth < MAX_DEPTH {
                    queue.push_back((entry.path(), format!("{rel}/{name}"), depth + 1));
                }
                continue;
            }
            let Some(ext) = entry.file_name().to_str().and_then(ext_of) else { continue };
            has_sub |= SUB_EXTS.contains(&ext.as_str());
            has_audio |= AUDIO_EXTS.contains(&ext.as_str());
        }
        if has_sub {
            out.subs.push(rel.clone());
        }
        if has_audio {
            out.audio.push(rel);
        }
    }
    out
}

/// A directory worth descending into: not hidden, not a symlink (a link back
/// up the tree would be walked until the budget ran out), and with a name mpv
/// can take in a path list — `:` and `;` are the separators there.
fn dir_name(entry: &std::fs::DirEntry) -> Option<String> {
    let ft = entry.file_type().ok()?;
    if !ft.is_dir() || ft.is_symlink() {
        return None;
    }
    let name = entry.file_name().to_str()?.to_string();
    if name.starts_with('.') || name.contains(':') || name.contains(';') {
        return None;
    }
    Some(name)
}

#[tauri::command]
pub async fn external_track_dirs(path: String) -> Result<ExternalDirs, String> {
    let p = PathBuf::from(&path);
    let Some(parent) = p.parent().map(Path::to_path_buf) else {
        return Ok(ExternalDirs::default());
    };
    tauri::async_runtime::spawn_blocking(move || scan(&parent))
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(root: &Path, rel: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"").unwrap();
    }

    #[test]
    fn finds_nested_release_folders() {
        let root = std::env::temp_dir().join(format!("fp-extdirs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        touch(&root, "Show - 01.mkv");
        touch(&root, "RUS Sound/StudioA/Show - 01.mka");
        touch(&root, "RUS Sound/StudioB/Show - 01.mka");
        touch(&root, "Subs/Show - 01.rus.ass");
        touch(&root, "Subs/Fonts/font.ttf");
        touch(&root, ".hidden/Show - 01.ass");
        touch(&root, "Extras/a/b/c/Show - 01.ass");

        let mut got = scan(&root);
        got.audio.sort();
        assert_eq!(got.audio, vec!["RUS Sound/StudioA", "RUS Sound/StudioB"]);
        // Depth 4 is past the limit, the hidden folder is skipped, and a folder
        // of fonts is not a subtitle folder.
        assert_eq!(got.subs, vec!["Subs"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
