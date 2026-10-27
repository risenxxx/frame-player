// ---- Commands --------------------------------------------------------------

use std::sync::Arc;

use super::dirs::{folder_for, is_info_hash, meta_path, Dirs};
use super::session::{Encryption, SessionPrefs};
use super::storage::{clear_all, file_disk_size};
use super::{LocalFile, TorrentInfo, TorrentOnDisk, TorrentService, TorrentStatus};

#[tauri::command]
pub async fn torrent_add(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<TorrentService>>,
    source: String,
    prefs: SessionPrefs,
) -> Result<TorrentInfo, String> {
    let dirs = TorrentService::download_dir(&app)?;
    service.inner().clone().add(&dirs, source, &prefs).await
}

/// Returns true if a running session had to be torn down to apply this — the
/// frontend turns that into "the current torrent stopped", since it did.
#[tauri::command]
pub async fn torrent_set_seeding(
    service: tauri::State<'_, Arc<TorrentService>>,
    seeding: bool,
) -> Result<bool, String> {
    Ok(service.set_seeding(seeding).await)
}

/// Returns true if a running session had to be torn down — see `set_port_forward`.
#[tauri::command]
pub async fn torrent_set_port_forward(
    service: tauri::State<'_, Arc<TorrentService>>,
    on: bool,
) -> Result<bool, String> {
    Ok(service.set_port_forward(on).await)
}

/// Returns true if a running session had to be torn down — see `set_proxy`.
#[tauri::command]
pub async fn torrent_set_proxy(
    service: tauri::State<'_, Arc<TorrentService>>,
    url: String,
) -> Result<bool, String> {
    Ok(service.set_proxy(url).await)
}

/// Returns true if a running session had to be torn down — see `set_encryption`.
#[tauri::command]
pub async fn torrent_set_encryption(
    service: tauri::State<'_, Arc<TorrentService>>,
    mode: Encryption,
) -> Result<bool, String> {
    Ok(service.set_encryption(mode).await)
}

/// Returns true if a running session had to be torn down — see `set_route`.
#[tauri::command]
pub async fn torrent_set_route(
    service: tauri::State<'_, Arc<TorrentService>>,
    route: crate::net_route::Route,
) -> Result<bool, String> {
    Ok(service.set_route(route).await)
}

/// What the router says about the BitTorrent port.
///
/// Asked rather than assumed: librqbit's forwarder is fire-and-forget, so
/// "the switch is on" says nothing about whether a packet ever reached the
/// router — and on a router with UPnP disabled, which is common, it never does.
#[tauri::command]
pub async fn torrent_port_status(
    service: tauri::State<'_, Arc<TorrentService>>,
    on: bool,
) -> Result<crate::upnp::PortStatus, String> {
    if !on {
        return Ok(crate::upnp::PortStatus {
            state: "off".into(),
            ..Default::default()
        });
    }
    let port = service.listen_port().await;
    if port == 0 {
        // Nothing is mapped and nothing failed: the session that would ask for
        // a mapping is built on the first torrent, and there has not been one.
        return Ok(crate::upnp::PortStatus {
            state: "no_session".into(),
            ..Default::default()
        });
    }
    Ok(crate::upnp::check(port).await)
}

#[tauri::command]
pub async fn torrent_status(
    service: tauri::State<'_, Arc<TorrentService>>,
    info_hash: String,
    index: usize,
) -> Result<TorrentStatus, String> {
    Ok(service.status(&info_hash, index).await)
}

#[tauri::command]
pub async fn torrent_buffered(
    service: tauri::State<'_, Arc<TorrentService>>,
    info_hash: String,
    index: usize,
) -> Result<Vec<(f64, f64)>, String> {
    Ok(service.buffered(&info_hash, index).await)
}

#[tauri::command]
pub async fn torrent_local_path(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<TorrentService>>,
    info_hash: String,
    index: usize,
) -> Result<Option<LocalFile>, String> {
    let dirs = TorrentService::download_dir(&app)?;
    Ok(service.local_path(&dirs, &info_hash, index).await)
}

#[tauri::command]
pub async fn torrent_prefetch(
    service: tauri::State<'_, Arc<TorrentService>>,
    info_hash: String,
    index: usize,
) -> Result<(), String> {
    service.prefetch(&info_hash, index).await
}

#[tauri::command]
pub async fn torrent_release(
    service: tauri::State<'_, Arc<TorrentService>>,
    info_hash: String,
) -> Result<(), String> {
    service.release(&info_hash).await;
    Ok(())
}

#[tauri::command]
pub async fn torrent_list(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<TorrentService>>,
) -> Result<Vec<TorrentOnDisk>, String> {
    let dirs = TorrentService::download_dir(&app)?;
    Ok(service.list(&dirs))
}

#[tauri::command]
pub async fn torrent_relocate(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<TorrentService>>,
    old_hash: String,
    new_hash: String,
) -> Result<(), String> {
    let dirs = TorrentService::download_dir(&app)?;
    service.relocate(&dirs, &old_hash, &new_hash).await
}

#[tauri::command]
pub async fn torrent_forget(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<TorrentService>>,
    path: String,
) -> Result<u64, String> {
    let dirs = TorrentService::download_dir(&app)?;
    service.forget(&dirs, &path).await
}

/// Delete everything the torrent cache holds.
///
/// Streaming writes the pieces to disk, so a few films fill a cache directory
/// the viewer never chose to fill. Offered next to the thumbnail cache in
/// settings, for the same reason.
///
/// **This used to delete every entry in the directory, files included**, which
/// was exactly right while that directory was ours alone and is a way to erase
/// somebody's film library the moment they can point it at one. It now removes
/// only what `is_ours` vouches for: a torrent folder we created, in a root we
/// know. Loose files are swept in `state` and nowhere else — in the cache
/// directory a stray file is our own leftover, and in a chosen root it is
/// theirs.
#[tauri::command]
pub async fn torrent_clear_cache(app: tauri::AppHandle) -> Result<u64, String> {
    Ok(clear_all(&TorrentService::download_dir(&app)?))
}

/// Delete some episodes of a torrent and keep the rest.
#[tauri::command]
pub async fn torrent_forget_files(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<TorrentService>>,
    path: String,
    names: Vec<String>,
) -> Result<u64, String> {
    let dirs = TorrentService::download_dir(&app)?;
    service.forget_files(&dirs, &path, &names).await
}

/// Where new torrents go. `null` puts them back in the cache directory.
///
/// The previous root is remembered rather than forgotten, so a season
/// downloaded before the change stays findable and deletable — nothing is
/// moved by choosing a new one.
#[tauri::command]
pub fn torrent_set_dir(app: tauri::AppHandle, path: Option<String>) -> Result<(), String> {
    Dirs::set_root(&app, path.as_deref())
}

/// The current root and whether it is the default, for the settings row.
#[tauri::command]
pub fn torrent_dir(app: tauri::AppHandle) -> Result<(String, bool), String> {
    let dirs = Dirs::load(&app)?;
    let default = dirs.root == dirs.state;
    Ok((dirs.root.to_string_lossy().into_owned(), default))
}

/// The file of a torrent as it sits on disk, **without loading the session**.
///
/// The start screen needs this and only this: a path it can decode a poster
/// from, for a card whose entry is a `torrent:` id. Going through
/// `torrent_local_path` would mean adding the torrent — a DHT-joining,
/// peer-connecting session — to draw a thumbnail, which is the opposite of the
/// rule that nothing opens until it is asked for.
///
/// Everything needed is already on disk: the cached `.torrent` gives the file's
/// relative name and its length, the hash gives the folder, and completeness is
/// the same allocated-blocks measure `torrent_list` uses to report size. An
/// incomplete file is refused rather than returned, because its holes read back
/// as **zeros** — a poster decoded from one is a black rectangle presented as a
/// frame of the film.
#[tauri::command]
pub fn torrent_offline_file(
    app: tauri::AppHandle,
    info_hash: String,
    index: usize,
) -> Option<LocalFile> {
    if !is_info_hash(&info_hash) {
        return None;
    }
    let hash = info_hash.to_ascii_lowercase();
    let dirs = TorrentService::download_dir(&app).ok()?;
    let meta = meta_path(&dirs.state, &hash);
    let bytes = match std::fs::read(&meta) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[poster] no cached metadata for {hash}: {e}");
            return None;
        }
    };
    let parsed = match librqbit::torrent_from_bytes(&bytes) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[poster] cached metadata for {hash} unreadable: {e:#}");
            return None;
        }
    };
    // `validate` is what turns the parsed info dict into something that can be
    // walked: it computes the piece lengths, detects the name encoding and
    // refuses a torrent whose file names climb out of their own folder — the
    // same check librqbit runs before it will add one.
    let info = parsed.info.data.validate().ok()?;

    // Single-file torrents have no file list; the torrent itself is the file.
    let f = info.iter_file_details().nth(index)?;
    let rel = f
        .filename
        .iter_components()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(std::path::MAIN_SEPARATOR_STR);
    let length = f.len;
    let path = folder_for(&dirs, &hash, None).join(&rel);
    let Ok(meta) = std::fs::metadata(&path) else {
        eprintln!("[poster] {hash}/{index}: no file at {}", path.display());
        return None;
    };
    if meta.len() != length {
        eprintln!(
            "[poster] {hash}/{index}: {} is {} bytes, torrent says {length}",
            path.display(),
            meta.len()
        );
        return None;
    }
    // On Windows librqbit never marks the file sparse, so the allocated size is
    // the full length whatever has arrived and this test cannot tell an empty
    // file from a finished one — hence the caller treats a poster that fails to
    // decode as "no poster yet" rather than as an error.
    let complete = file_disk_size(&meta) + (length / 64) >= length;
    if !complete {
        eprintln!(
            "[poster] {hash}/{index}: only {} of {length} bytes allocated — incomplete",
            file_disk_size(&meta)
        );
    }
    complete.then(|| LocalFile {
        path: path.to_string_lossy().into_owned(),
        complete,
    })
}
