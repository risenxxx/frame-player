//! Copied from the player's src-tauri/src/subtitle_fonts.rs (main d6ea334), with Tauri taken out:
//! the fonts live in version 2's data directory, and the commands are plain functions.

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


const FONT_EXTENSIONS: &[&str] = &["otc", "otf", "ttc", "ttf"];
const MAX_FONT_BYTES: u64 = 100 * 1024 * 1024;
const FONT_DIR: &str = "subtitle-fonts";

/// The errors the settings sheet tells apart; anything else is shown as a
/// general failure and its text goes to the console.
const ERR_EXISTS: &str = "font_exists";
const ERR_INVALID: &str = "font_invalid";
const ERR_SIZE: &str = "font_size";

fn font_dir() -> Result<PathBuf, String> {
    let dir = crate::commands::data_dir().join(FONT_DIR);
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


pub fn subtitle_fonts_dir() -> Result<String, String> {
    Ok(font_dir()?.to_string_lossy().into_owned())
}

pub fn subtitle_fonts_list() -> Result<serde_json::Value, String> {
    let fonts = list_fonts(&font_dir()?)?;
    serde_json::to_value(fonts).map_err(|e| e.to_string())
}

pub fn subtitle_font_add(path: &str) -> Result<serde_json::Value, String> {
    let font = copy_font(Path::new(path), &font_dir()?)?;
    serde_json::to_value(font).map_err(|e| e.to_string())
}

pub fn subtitle_font_remove(name: &str) -> Result<(), String> {
    remove_font(&font_dir()?, name)
}
