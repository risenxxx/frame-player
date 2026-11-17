//! Viewer-managed fonts for mpv's subtitle renderer.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::AppHandle;

use crate::portable;

const FONT_EXTENSIONS: &[&str] = &["otc", "otf", "ttc", "ttf"];
const MAX_FONT_BYTES: u64 = 100 * 1024 * 1024;
const FONT_DIR: &str = "subtitle-fonts";

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

fn cleanup_partial_file(path: &Path, failure: String) -> String {
    match fs::remove_file(path) {
        Ok(()) => failure,
        Err(error) => format!("{failure}; cannot remove incomplete font file: {error}"),
    }
}

fn copy_font(source: &Path, destination: &Path) -> Result<String, String> {
    let source = fs::canonicalize(source).map_err(|e| e.to_string())?;
    if !source.is_file() {
        return Err("selected path is not a file".to_string());
    }

    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| supported_font_name(name))
        .ok_or_else(|| "unsupported font file".to_string())?
        .to_string();
    let size = fs::metadata(&source).map_err(|e| e.to_string())?.len();
    if size == 0 || size > MAX_FONT_BYTES {
        return Err("font file is empty or exceeds 100 MB".to_string());
    }

    fs::create_dir_all(destination)
        .map_err(|e| format!("cannot create custom font directory: {e}"))?;
    let target = destination.join(&name);
    let mut input = File::open(&source).map_err(|e| e.to_string())?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|e| format!("cannot install custom font: {e}"))?;
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
    Ok(name)
}

#[derive(Serialize)]
pub struct SubtitleFont {
    name: String,
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
            fonts.push(SubtitleFont { name });
        }
    }
    fonts.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
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
pub fn subtitle_fonts_list(app: AppHandle) -> Result<Vec<SubtitleFont>, String> {
    list_fonts(&font_dir(&app)?)
}

#[tauri::command]
pub async fn subtitle_font_add(app: AppHandle, path: String) -> Result<String, String> {
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
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("frameplayer-font-test-{nonce}"))
    }

    #[test]
    fn imports_supported_fonts_without_overwriting() {
        let root = temp_dir();
        let source_dir = root.join("source");
        let font_dir = root.join("fonts");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("Example.TTF");
        fs::write(&source, b"font data").unwrap();

        assert_eq!(copy_font(&source, &font_dir).unwrap(), "Example.TTF");
        assert_eq!(
            fs::read(font_dir.join("Example.TTF")).unwrap(),
            b"font data"
        );
        assert!(copy_font(&source, &font_dir).is_err());
        assert_eq!(
            fs::read(font_dir.join("Example.TTF")).unwrap(),
            b"font data"
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_unsupported_and_path_like_font_names() {
        assert!(!supported_font_name("font.exe"));
        assert!(!supported_font_name("../font.ttf"));
        assert!(supported_font_name("font.otc"));
    }

    #[test]
    fn rejects_empty_and_oversized_fonts() {
        let root = temp_dir();
        fs::create_dir_all(&root).unwrap();

        let empty = root.join("empty.ttf");
        fs::write(&empty, b"").unwrap();
        assert!(copy_font(&empty, &root.join("fonts")).is_err());

        let oversized = root.join("oversized.ttf");
        File::create(&oversized)
            .unwrap()
            .set_len(MAX_FONT_BYTES + 1)
            .unwrap();
        assert!(copy_font(&oversized, &root.join("fonts")).is_err());

        fs::remove_dir_all(root).unwrap();
    }
}
