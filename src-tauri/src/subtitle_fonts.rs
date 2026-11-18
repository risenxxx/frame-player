//! Viewer-managed fonts for mpv's subtitle renderer.
//!
//! The files live in `subtitle-fonts/` under the app's data directory, which
//! `initPlayer` hands to mpv as `sub-fonts-dir` (and `sublines.lua` copies to
//! `osd-fonts-dir`, so the second line and the overlay that measures the
//! first see the same fonts). What the viewer picks is a *family*, not a
//! file — `sub-font` is matched by libass against the families a font
//! declares — so every file is listed with the family names read out of its
//! `name` table, and a file that declares none is refused as not a font.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::AppHandle;

use crate::portable;

const FONT_EXTENSIONS: &[&str] = &["otc", "otf", "ttc", "ttf"];
const MAX_FONT_BYTES: u64 = 100 * 1024 * 1024;
const FONT_DIR: &str = "subtitle-fonts";

/// The errors the settings sheet tells apart; anything else is shown as a
/// general failure and its text goes to the console.
const ERR_EXISTS: &str = "font_exists";
const ERR_INVALID: &str = "font_invalid";
const ERR_SIZE: &str = "font_size";

fn font_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = portable::data_dir(app)?.join(FONT_DIR);
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create custom font directory: {e}"))?;
    Ok(dir)
}

fn supported_font_name(name: &str) -> bool {
    let path = Path::new(name);
    path.file_name().and_then(|part| part.to_str()) == Some(name)
        && path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| FONT_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

// ---- Family names -----------------------------------------------------------
//
// Only what libass matches on is read: name ID 1, the family as GDI knows it
// (ass_fontselect.c collects every ID 1 record plus the full and PostScript
// names). The typographic family (ID 16) is deliberately not used — for
// "Noto Sans SemiBold" it says "Noto Sans", which would select the regular
// weight instead. Reading seeks to the tables it needs: `name` usually sits
// after `glyf`, at the end of a file that may be tens of megabytes.

fn be16(data: &[u8], at: usize) -> Option<u16> {
    data.get(at..at + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_at<R: Read + Seek>(file: &mut R, offset: u64, len: usize) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = vec![0; len];
    file.read_exact(&mut buf).ok()?;
    Some(buf)
}

/// The family of one face, from its `name` table: an English Windows record
/// first, then any Windows record, then Unicode, then a Mac Roman one that is
/// plain ASCII.
fn name_table_family(table: &[u8]) -> Option<String> {
    let count = usize::from(be16(table, 2)?);
    let storage = usize::from(be16(table, 4)?);
    let mut best: Option<(u8, String)> = None;
    for i in 0..count {
        let rec = 6 + 12 * i;
        let (Some(platform), Some(encoding), Some(language), Some(id), Some(len), Some(off)) = (
            be16(table, rec),
            be16(table, rec + 2),
            be16(table, rec + 4),
            be16(table, rec + 6),
            be16(table, rec + 8),
            be16(table, rec + 10),
        ) else {
            break;
        };
        if id != 1 {
            continue;
        }
        let start = storage + usize::from(off);
        let Some(bytes) = table.get(start..start + usize::from(len)) else {
            continue;
        };
        let utf16 = || {
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_be_bytes(*b))
                .collect();
            String::from_utf16(&units).ok()
        };
        let (rank, text) = match platform {
            3 => (if language == 0x409 { 0 } else { 1 }, utf16()),
            0 => (2, utf16()),
            1 if encoding == 0 && bytes.is_ascii() => {
                (3, Some(String::from_utf8_lossy(bytes).into_owned()))
            }
            _ => continue,
        };
        let Some(text) = text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) else {
            continue;
        };
        if best.as_ref().is_none_or(|(r, _)| rank < *r) {
            best = Some((rank, text));
        }
    }
    best.map(|(_, text)| text)
}

fn face_family<R: Read + Seek>(file: &mut R, face: u64) -> Option<String> {
    let head = read_at(file, face, 12)?;
    let tables = usize::from(be16(&head, 4)?);
    let dir = read_at(file, face + 12, 16 * tables)?;
    let rec = (0..tables)
        .map(|i| 16 * i)
        .find(|&r| dir.get(r..r + 4) == Some(b"name".as_slice()))?;
    let offset = be32(&dir, rec + 8)?;
    let len = be32(&dir, rec + 12)? as usize;
    if len > 1 << 20 {
        return None;
    }
    name_table_family(&read_at(file, u64::from(offset), len)?)
}

/// Every family a font file declares, once each and in file order — a
/// collection (TTC/OTC) usually holds several.
fn font_families<R: Read + Seek>(file: &mut R) -> Vec<String> {
    let Some(head) = read_at(file, 0, 12) else {
        return Vec::new();
    };
    let faces: Vec<u64> = if head.starts_with(b"ttcf") {
        let count = be32(&head, 8).unwrap_or(0).min(64) as usize;
        read_at(file, 12, 4 * count)
            .map(|dir| {
                (0..count)
                    .filter_map(|i| be32(&dir, 4 * i))
                    .map(u64::from)
                    .collect()
            })
            .unwrap_or_default()
    } else {
        vec![0]
    };
    let mut families = Vec::new();
    for face in faces {
        if let Some(family) = face_family(file, face) {
            if !families.contains(&family) {
                families.push(family);
            }
        }
    }
    families
}

fn file_families(path: &Path) -> Vec<String> {
    File::open(path)
        .map(|mut file| font_families(&mut file))
        .unwrap_or_default()
}

// ---- Import, list, remove ---------------------------------------------------

fn cleanup_partial_file(path: &Path, failure: String) -> String {
    match fs::remove_file(path) {
        Ok(()) => failure,
        Err(error) => format!("{failure}; cannot remove incomplete font file: {error}"),
    }
}

fn copy_font(source: &Path, destination: &Path) -> Result<SubtitleFont, String> {
    let source = fs::canonicalize(source).map_err(|e| e.to_string())?;
    if !source.is_file() {
        return Err(ERR_INVALID.to_string());
    }

    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| supported_font_name(name))
        .ok_or_else(|| ERR_INVALID.to_string())?
        .to_string();
    let size = fs::metadata(&source).map_err(|e| e.to_string())?.len();
    if size == 0 || size > MAX_FONT_BYTES {
        return Err(ERR_SIZE.to_string());
    }
    let mut input = File::open(&source).map_err(|e| e.to_string())?;
    // A file named .ttf that declares no family is not something libass can
    // ever select, and it would sit in the list as a font that does nothing.
    let families = font_families(&mut input);
    if families.is_empty() {
        return Err(ERR_INVALID.to_string());
    }
    input.rewind().map_err(|e| e.to_string())?;

    fs::create_dir_all(destination)
        .map_err(|e| format!("cannot create custom font directory: {e}"))?;
    let target = destination.join(&name);
    let mut output = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
    {
        Ok(output) => output,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Err(ERR_EXISTS.to_string()),
        Err(e) => return Err(format!("cannot install custom font: {e}")),
    };
    match io::copy(&mut input.take(MAX_FONT_BYTES + 1), &mut output) {
        Ok(copied) if copied == size => {}
        Ok(_) => {
            drop(output);
            return Err(cleanup_partial_file(
                &target,
                "font file changed while being copied".to_string(),
            ));
        }
        Err(error) => {
            drop(output);
            return Err(cleanup_partial_file(
                &target,
                format!("cannot copy selected font: {error}"),
            ));
        }
    }
    Ok(SubtitleFont { name, families })
}

#[derive(Debug, Serialize)]
pub struct SubtitleFont {
    /// The file name — what removing it is addressed by.
    name: String,
    /// What `sub-font` can be set to for this file; empty for a file that
    /// was put into the folder by hand and is not a readable font.
    families: Vec<String>,
}

fn list_fonts(dir: &Path) -> Result<Vec<SubtitleFont>, String> {
    let mut fonts = Vec::new();
    let entries =
        fs::read_dir(dir).map_err(|e| format!("cannot read custom font directory: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if supported_font_name(&name) {
            let families = file_families(&entry.path());
            fonts.push(SubtitleFont { name, families });
        }
    }
    fonts.sort_by_key(|font| {
        let label = font.families.first().unwrap_or(&font.name);
        (label.to_lowercase(), font.name.to_lowercase())
    });
    Ok(fonts)
}

fn remove_font(dir: &Path, name: &str) -> Result<(), String> {
    if !supported_font_name(name) {
        return Err("invalid font name".to_string());
    }
    let path = dir.join(name);
    if !path.is_file() {
        return Err("font does not exist".to_string());
    }
    fs::remove_file(&path).map_err(|e| format!("cannot remove custom font: {e}"))
}

#[tauri::command]
pub fn subtitle_fonts_dir(app: AppHandle) -> Result<String, String> {
    Ok(font_dir(&app)?.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn subtitle_fonts_list(app: AppHandle) -> Result<Vec<SubtitleFont>, String> {
    let dir = font_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || list_fonts(&dir))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn subtitle_font_add(app: AppHandle, path: String) -> Result<SubtitleFont, String> {
    let dir = font_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || copy_font(Path::new(&path), &dir))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn subtitle_font_remove(app: AppHandle, name: String) -> Result<(), String> {
    remove_font(&font_dir(&app)?, &name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("frameplayer-font-test-{nonce}"))
    }

    /// A `name` table holding the given (platform, encoding, language, id,
    /// string bytes) records.
    fn name_table(records: &[(u16, u16, u16, u16, Vec<u8>)]) -> Vec<u8> {
        let storage = 6 + 12 * records.len();
        let mut head = Vec::new();
        let mut strings = Vec::new();
        head.extend_from_slice(&0u16.to_be_bytes());
        head.extend_from_slice(&(records.len() as u16).to_be_bytes());
        head.extend_from_slice(&(storage as u16).to_be_bytes());
        for (platform, encoding, language, id, bytes) in records {
            for v in [*platform, *encoding, *language, *id, bytes.len() as u16] {
                head.extend_from_slice(&v.to_be_bytes());
            }
            head.extend_from_slice(&(strings.len() as u16).to_be_bytes());
            strings.extend_from_slice(bytes);
        }
        head.extend(strings);
        head
    }

    fn utf16(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_be_bytes).collect()
    }

    /// A face at `at` in a file: the offset table, one directory entry for
    /// `name`, then the table itself.
    fn face(at: usize, table: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        out.extend_from_slice(b"name");
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&((at + 28) as u32).to_be_bytes());
        out.extend_from_slice(&(table.len() as u32).to_be_bytes());
        out.extend_from_slice(table);
        out
    }

    fn font_bytes(family: &str) -> Vec<u8> {
        face(0, &name_table(&[(3, 1, 0x409, 1, utf16(family))]))
    }

    #[test]
    fn reads_the_gdi_family_preferring_english_windows_records() {
        let table = name_table(&[
            (1, 0, 0, 1, b"Mac Name".to_vec()),
            (3, 1, 0x419, 1, utf16("Русское имя")),
            (3, 1, 0x409, 16, utf16("Typographic")),
            (3, 1, 0x409, 1, utf16("Example Sans SemiBold")),
        ]);
        let families = font_families(&mut Cursor::new(face(0, &table)));
        assert_eq!(families, ["Example Sans SemiBold"]);

        let mac_only = name_table(&[(1, 0, 0, 1, b"Mac Name".to_vec())]);
        assert_eq!(
            font_families(&mut Cursor::new(face(0, &mac_only))),
            ["Mac Name"]
        );
    }

    #[test]
    fn reads_every_face_of_a_collection_once() {
        let first = name_table(&[(3, 1, 0x409, 1, utf16("Example Sans"))]);
        let second = name_table(&[(3, 1, 0x409, 1, utf16("Example Serif"))]);
        let header = 12 + 4 * 3;
        let a = header;
        let b = a + face(a, &first).len();
        let c = b + face(b, &second).len();
        let mut file = Vec::new();
        file.extend_from_slice(b"ttcf");
        file.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        file.extend_from_slice(&3u32.to_be_bytes());
        for at in [a, b, c] {
            file.extend_from_slice(&(at as u32).to_be_bytes());
        }
        file.extend(face(a, &first));
        file.extend(face(b, &second));
        file.extend(face(c, &first));
        assert_eq!(
            font_families(&mut Cursor::new(file)),
            ["Example Sans", "Example Serif"]
        );
    }

    #[test]
    fn garbage_declares_no_family() {
        assert!(font_families(&mut Cursor::new(b"font data".to_vec())).is_empty());
        assert!(font_families(&mut Cursor::new(Vec::new())).is_empty());
    }

    #[test]
    fn imports_supported_fonts_without_overwriting() {
        let root = temp_dir();
        let source_dir = root.join("source");
        let font_dir = root.join("fonts");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("Example.TTF");
        let bytes = font_bytes("Example");
        fs::write(&source, &bytes).unwrap();

        let added = copy_font(&source, &font_dir).unwrap();
        assert_eq!(added.name, "Example.TTF");
        assert_eq!(added.families, ["Example"]);
        assert_eq!(fs::read(font_dir.join("Example.TTF")).unwrap(), bytes);
        assert_eq!(copy_font(&source, &font_dir).unwrap_err(), ERR_EXISTS);
        assert_eq!(fs::read(font_dir.join("Example.TTF")).unwrap(), bytes);

        let listed = list_fonts(&font_dir).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].families, ["Example"]);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_unsupported_and_path_like_font_names() {
        assert!(!supported_font_name("font.exe"));
        assert!(!supported_font_name("../font.ttf"));
        assert!(supported_font_name("font.otc"));
    }

    #[test]
    fn rejects_files_that_are_not_fonts() {
        let root = temp_dir();
        fs::create_dir_all(&root).unwrap();

        let empty = root.join("empty.ttf");
        fs::write(&empty, b"").unwrap();
        assert_eq!(
            copy_font(&empty, &root.join("fonts")).unwrap_err(),
            ERR_SIZE
        );

        let oversized = root.join("oversized.ttf");
        File::create(&oversized)
            .unwrap()
            .set_len(MAX_FONT_BYTES + 1)
            .unwrap();
        assert_eq!(
            copy_font(&oversized, &root.join("fonts")).unwrap_err(),
            ERR_SIZE
        );

        let renamed = root.join("renamed.ttf");
        fs::write(&renamed, b"not a font at all").unwrap();
        assert_eq!(
            copy_font(&renamed, &root.join("fonts")).unwrap_err(),
            ERR_INVALID
        );

        fs::remove_dir_all(root).unwrap();
    }
}
