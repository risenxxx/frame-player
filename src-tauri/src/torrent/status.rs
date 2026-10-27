//! What the player reads back: the status panel, the buffer map, the local path.

use std::sync::Arc;

use librqbit::ManagedTorrent;

use super::dirs::{folder_for, is_info_hash, Dirs};
use super::{LocalFile, TorrentService, TorrentStatus};

impl TorrentService {
    /// Which stretches of a file are already on disk, as fractions of it.
    ///
    /// This is what turns a torrent's seekbar from a promise into a map: the
    /// viewer can see where a jump lands instantly and where it means waiting,
    /// which is the question the bar is actually being read for. It is also what
    /// makes previews possible before the file is complete — a position inside
    /// one of these ranges can be decoded, one outside cannot.
    ///
    /// **No fork of librqbit was needed for it, contrary to first appearances.**
    /// `with_chunk_tracker` is `pub(crate)`, but `Api::api_dump_haves` is
    /// public and hands the same bitmap out. Under librqbit 8 it did so as a
    /// `format!("{:?}")` of a `BitSlice`, which this scanned back into a
    /// `Vec<bool>` — undignified rather than fragile, since the format was
    /// pinned by Cargo.lock and a parse failure cost the map and nothing else.
    /// 9.x returns the bitfield itself with the piece count beside it, so the
    /// scanner and its test are gone.
    pub async fn buffered(
        &self,
        info_hash: &str,
        index: usize,
    ) -> Vec<(f64, f64)> {
        let (session, handle) = {
            let inner = self.inner.lock().await;
            let Some(session) = inner.session.clone() else {
                return Vec::new();
            };
            let Some(entry) = inner.torrents.get(&info_hash.to_ascii_lowercase()) else {
                return Vec::new();
            };
            (session, entry.handle.clone())
        };

        let dump = librqbit::Api::new(session, None)
            .api_dump_haves(librqbit::api::TorrentIdOrHash::Id(handle.id()));
        let Ok((bits, pieces)) = dump else {
            return Vec::new();
        };
        // A bitfield is rounded up to a whole byte, so anything past the piece
        // count is padding rather than a piece nobody has.
        let have: Vec<bool> = bits.iter().take(pieces as usize).map(|b| *b).collect();

        handle
            .with_metadata(|m| {
                let Some(fi) = m.file_infos.get(index) else {
                    return Vec::new();
                };
                file_ranges(
                    &have,
                    m.lengths().default_piece_length() as u64,
                    fi.offset_in_torrent,
                    fi.len,
                )
            })
            .unwrap_or_default()
    }

    /// The local path of a torrent file, and whether all of it is there.
    ///
    /// This is what lets the seekbar storyboard and the start-screen posters
    /// work for a torrent at all. They were gated off for every network source
    /// on the grounds that decoding the whole file in the background would pull
    /// it down a second time in an order unrelated to playback — true for a
    /// stream, and no longer true here: the data is an ordinary file on this
    /// disk, so decoding it costs nothing.
    ///
    /// `complete` is not a nicety, and the caller must respect it. librqbit
    /// creates every file at full length and the filesystem makes it **sparse**,
    /// so reading a stretch that has not arrived returns *zeros* rather than
    /// failing — decoding one would not hang, it would quietly produce black
    /// frames and green mush. So the background storyboard, which walks a file
    /// start to end, may only run on a complete file; single frames may be
    /// decoded from an incomplete one, but only at a position `buffered()` says
    /// is there.
    pub async fn local_path(&self, dirs: &Dirs, info_hash: &str, index: usize) -> Option<LocalFile> {
        if !is_info_hash(info_hash) {
            return None;
        }
        let handle = {
            let inner = self.inner.lock().await;
            inner.torrents.get(&info_hash.to_ascii_lowercase())?.handle.clone()
        };
        let complete = file_complete(&handle, index);
        let rel = handle
            .with_metadata(|m| m.file_infos.get(index).map(|f| f.relative_filename.clone()))
            .ok()
            .flatten()?;
        let path = folder_for(dirs, info_hash, None).join(rel);
        path.is_file().then(|| LocalFile {
            path: path.to_string_lossy().into_owned(),
            complete,
        })
    }

    pub async fn status(&self, info_hash: &str, index: usize) -> TorrentStatus {
        let handle = {
            let inner = self.inner.lock().await;
            match inner.torrents.get(info_hash) {
                Some(e) => e.handle.clone(),
                None => {
                    return TorrentStatus {
                        state: "gone".into(),
                        ..Default::default()
                    }
                }
            }
        };

        let stats = handle.stats();
        let (peers, peers_seen, down_bps, up_bps) = match &stats.live {
            Some(live) => (
                live.snapshot.peer_stats.live,
                live.snapshot.peer_stats.seen,
                // librqbit reports megabits per second; the panel wants bytes.
                live.download_speed.mbps * 125_000.0,
                live.upload_speed.mbps * 125_000.0,
            ),
            None => (0, 0, 0.0, 0.0),
        };

        let file_size = handle
            .with_metadata(|m| m.file_infos.get(index).map(|f| f.len).unwrap_or(0))
            .unwrap_or(0);

        TorrentStatus {
            state: stats.state.to_string(),
            error: stats.error.clone(),
            peers: peers as usize,
            peers_seen: peers_seen as usize,
            down_bps,
            up_bps,
            file_done: stats.file_progress.get(index).copied().unwrap_or(0),
            file_size,
            total_done: stats.progress_bytes,
            total_size: stats.total_bytes,
        }
    }
}

/// Turn a piece bitmap into the fractions of ONE file that are present.
///
/// Pieces are laid over the whole torrent, so the first and last piece of a file
/// are usually shared with its neighbours — the intersection below is what stops
/// a shared piece from claiming bytes that belong to the file next door.
/// Adjacent runs are merged, because a seekbar wants a handful of bands rather
/// than two thousand hairlines.
fn file_ranges(have: &[bool], piece_len: u64, offset: u64, len: u64) -> Vec<(f64, f64)> {
    if piece_len == 0 || len == 0 {
        return Vec::new();
    }
    let first = (offset / piece_len) as usize;
    let last = ((offset + len - 1) / piece_len) as usize;
    let mut out: Vec<(f64, f64)> = Vec::new();
    for p in first..=last {
        if !have.get(p).copied().unwrap_or(false) {
            continue;
        }
        let ps = p as u64 * piece_len;
        let pe = ps + piece_len;
        // Clamp into the file, then make it file-relative.
        let s = ps.max(offset) - offset;
        let e = pe.min(offset + len) - offset;
        if e <= s {
            continue;
        }
        let (sf, ef) = (s as f64 / len as f64, e as f64 / len as f64);
        match out.last_mut() {
            Some(prev) if sf <= prev.1 + 1e-9 => prev.1 = ef,
            _ => out.push((sf, ef)),
        }
    }
    out
}

/// Is this file of the torrent completely on disk?
///
/// Read from the verified piece state that initialization has just produced —
/// `file_progress` is populated in the paused state too, so this is answerable
/// before the torrent has spoken to anyone.
pub(super) fn file_complete(handle: &Arc<ManagedTorrent>, index: usize) -> bool {
    let Ok(Some(len)) = handle.with_metadata(|m| m.file_infos.get(index).map(|f| f.len)) else {
        return false;
    };
    // A zero-length file is not "complete", it is nothing worth streaming.
    len > 0 && handle.stats().file_progress.get(index).copied().unwrap_or(0) >= len
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pieces cover the whole torrent, so a file's first and last are usually
    /// shared with its neighbours. Getting that intersection wrong would draw
    /// buffer that belongs to another file — and, worse, let a thumbnail be
    /// decoded from a hole.
    #[test]
    fn buffered_ranges() {
        // 10-byte pieces. The file sits at 15..35, so it spans pieces 1..=3 and
        // owns only half of piece 1 and half of piece 3.
        let (piece, off, len) = (10u64, 15u64, 20u64);

        // Nothing.
        assert_eq!(file_ranges(&[false; 4], piece, off, len), vec![]);

        // Only piece 2 — the middle of the file, and neither end.
        let r = file_ranges(&[false, false, true, false], piece, off, len);
        assert_eq!(r.len(), 1);
        assert!((r[0].0 - 0.25).abs() < 1e-9, "{r:?}");
        assert!((r[0].1 - 0.75).abs() < 1e-9, "{r:?}");

        // Pieces 1 and 2 are adjacent and must merge into one band, clamped to
        // the start of the file rather than the start of piece 1.
        let r = file_ranges(&[false, true, true, false], piece, off, len);
        assert_eq!(r.len(), 1, "adjacent pieces did not merge: {r:?}");
        assert!((r[0].0).abs() < 1e-9, "{r:?}");
        assert!((r[0].1 - 0.75).abs() < 1e-9, "{r:?}");

        // A gap stays a gap.
        let r = file_ranges(&[false, true, false, true], piece, off, len);
        assert_eq!(r.len(), 2, "{r:?}");

        // Everything: exactly 0..1, not past the end of the file.
        let r = file_ranges(&[true; 4], piece, off, len);
        assert_eq!(r.len(), 1);
        assert!((r[0].0).abs() < 1e-9 && (r[0].1 - 1.0).abs() < 1e-9, "{r:?}");

        // Degenerate inputs must not panic or divide by zero.
        assert_eq!(file_ranges(&[true], 0, 0, 10), vec![]);
        assert_eq!(file_ranges(&[true], 10, 0, 0), vec![]);
    }
}
