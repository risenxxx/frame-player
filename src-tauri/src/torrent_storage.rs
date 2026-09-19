//! Torrent storage that opens a file when it is read and closes it when the
//! budget says so.
//!
//! **librqbit's own storage holds one open file descriptor per file of the
//! torrent, for as long as the torrent is in the session** — see
//! `FilesystemStorage::init` in the vendored crate, which walks every entry of
//! `file_infos` and keeps what it opened. That is a fine trade for a headless
//! client with one torrent in flight, and a bad one here: persistence restores
//! *every* torrent ever added when the session is built, and each of them pays
//! its whole file list forever. Measured on one real installation: 9 torrents,
//! 135 files, 144 descriptors held — one of them a Blu-ray release whose BDMV
//! folder is 79 `.clpi`/`.mpls`/`.m2ts` files on its own, none of which anybody
//! was watching.
//!
//! What that cost is in `raise_fd_limit` (lib.rs), and it is worth reading
//! before touching this file: the ceiling is not a torrent problem, it is the
//! moment the process loses hardware decoding and its audio clock, permanently.
//! Raising the limit removes the cliff; this removes the waste, so the budget
//! is spent on what is actually being watched rather than on nine seasons
//! sitting still.
//!
//! The shape is the smallest one that can do that. A file is a path until
//! somebody reads or writes it; the descriptor it then gets is registered with
//! a pool, and when the pool is over its budget the oldest one is closed. A
//! closed file reopens on its next access — positional reads and writes carry
//! their own offset, so nothing is lost across that and nobody has to be told.
//!
//! Three properties this deliberately keeps, because each of them is load
//! bearing somewhere else in the player:
//!
//! - **The files are still created at `init`**, exactly as upstream creates
//!   them. `only_files: Some(vec![])` means nothing is selected and nothing is
//!   preallocated, but every file of the torrent still comes into existence as
//!   an empty file — which is what `torrent_offline_file` and `dir_size` have
//!   been reading all along. Opening lazily must change how many descriptors
//!   are held, never what is on the disk.
//! - **Windows still gets `FSCTL_SET_SPARSE` before the first write.** NTFS has
//!   no sparse file unless somebody asks, and upstream 9.x asks on the first
//!   write rather than at creation; this does the same, once per descriptor's
//!   lifetime, or a nine-episode season occupies all of itself again.
//! - **`take()` leaves the old storage useless**, which is what librqbit means
//!   by it — it is how a torrent being paused or deleted stops writing through
//!   a storage somebody else may still hold.

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock, TryLockError, Weak};

use anyhow::Context as _;
use librqbit::storage::filesystem::{FilesystemStorageFactory, OurFileExt};
use librqbit::storage::{BoxStorageFactory, StorageFactory, TorrentStorage};
use librqbit::{ManagedTorrentShared, TorrentMetadata};

/// How many torrent files may hold a descriptor at once, across every torrent
/// in the session.
///
/// It is a budget rather than a limit on anything real: a file that loses its
/// descriptor reopens on the next read, so being wrong here costs one `open`
/// syscall and never an error. The number is chosen from what the player
/// actually does at once — one file streaming to mpv, one being prefetched, one
/// being checked against its resume data, and a peer set writing into whatever
/// of a season is selected — with room to spare, against a limit that
/// `raise_fd_limit` has by then taken into five figures.
const OPEN_FILE_BUDGET: usize = 64;

/// Whether an access needs the file to be writable on the platform's terms.
///
/// The distinction exists for exactly one platform: Windows wants to be told a
/// file is sparse before anything is written into it.
#[derive(Clone, Copy)]
enum Access {
    Read,
    Write,
}

/// One file of one torrent: a path, and a descriptor only while it is worth
/// holding.
struct LazyFile {
    /// `None` for a BitTorrent v2 padding entry, which has no file behind it —
    /// librqbit still indexes it, and reading one is a bug rather than a
    /// condition to recover from.
    path: Option<PathBuf>,
    fd: RwLock<Option<File>>,
    #[cfg(windows)]
    sparse_marked: AtomicBool,
}

impl LazyFile {
    fn new(path: Option<PathBuf>) -> Arc<Self> {
        Arc::new(Self {
            path,
            fd: RwLock::new(None),
            #[cfg(windows)]
            sparse_marked: AtomicBool::new(false),
        })
    }

    /// Close whatever descriptor this file holds. Used by the pool's eviction
    /// and by `remove_file`, where the handle has to go before the name does —
    /// Windows refuses to unlink an open file, and macOS agrees to it, which is
    /// worse: the writes then go to a file with no name.
    fn close(&self) {
        let mut fd = self.fd.write().unwrap_or_else(|e| e.into_inner());
        *fd = None;
    }
}

#[cfg(windows)]
fn mark_file_sparse(f: &File) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Ioctl::FSCTL_SET_SPARSE;
    use windows_sys::Win32::System::IO::DeviceIoControl;

    // Same call librqbit makes, for the same reason and at the same moment —
    // see the module comment. A refusal is not an error: the file simply
    // occupies what it is given, which is how this behaved before 9.x.
    unsafe {
        DeviceIoControl(
            f.as_raw_handle() as _,
            FSCTL_SET_SPARSE,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }
}

/// The descriptor budget, shared by every storage in the process.
///
/// **FIFO rather than a true LRU, and that is a considered choice.** Recency
/// would have to be recorded on every read — a few hundred thousand of them per
/// gigabyte — to improve a decision whose entire cost is one `open` syscall.
/// What the queue is really for is a ceiling, and a ceiling does not need to
/// know which file is hottest.
struct FdPool {
    budget: usize,
    open: Mutex<VecDeque<Weak<LazyFile>>>,
}

impl FdPool {
    fn new(budget: usize) -> Self {
        Self {
            budget,
            open: Mutex::new(VecDeque::new()),
        }
    }

    /// Register a freshly opened file and close whatever the budget no longer
    /// covers.
    ///
    /// **Eviction never blocks, and that is what makes it safe.** A victim is
    /// taken with `try_write` and skipped when somebody is mid-read: two
    /// threads each opening a file while holding another's lock would otherwise
    /// be a deadlock with no way to see it coming, and the recovery here is to
    /// close somebody else instead, which costs nothing.
    fn admit(&self, file: &Arc<LazyFile>) {
        let mut open = self.open.lock().unwrap_or_else(|e| e.into_inner());
        open.push_back(Arc::downgrade(file));

        // Bounded by the queue's own length: a busy victim goes to the back, so
        // without this a session where everything is in use would spin.
        let mut attempts = open.len();
        while open.len() > self.budget && attempts > 0 {
            attempts -= 1;
            let Some(weak) = open.pop_front() else { break };
            // The storage that owned it is gone; the descriptor went with it.
            let Some(victim) = weak.upgrade() else { continue };
            if Arc::ptr_eq(&victim, file) {
                open.push_back(weak);
                continue;
            }
            match victim.fd.try_write() {
                Ok(mut fd) => *fd = None,
                Err(TryLockError::Poisoned(p)) => *p.into_inner() = None,
                Err(TryLockError::WouldBlock) => open.push_back(weak),
            };
        }
    }
}

fn pool() -> &'static Arc<FdPool> {
    static POOL: OnceLock<Arc<FdPool>> = OnceLock::new();
    POOL.get_or_init(|| Arc::new(FdPool::new(OPEN_FILE_BUDGET)))
}

/// Read or write one file, opening it first if it is not open.
fn with_file<R>(
    file: &Arc<LazyFile>,
    pool: &FdPool,
    access: Access,
    f: impl FnOnce(&File) -> anyhow::Result<R>,
) -> anyhow::Result<R> {
    let path = file
        .path
        .as_ref()
        .context("bug: this torrent entry is padding and has no file")?;

    // The common case: already open, and a read lock is all it costs.
    {
        let fd = file.fd.read().unwrap_or_else(|e| e.into_inner());
        if let Some(open) = fd.as_ref() {
            prepare(file, open, access);
            return f(open).with_context(|| format!("error accessing {path:?}"));
        }
    }

    let mut fd = file.fd.write().unwrap_or_else(|e| e.into_inner());
    let fresh = fd.is_none();
    if fresh {
        // Never `create`: `init` is what brings a file into existence, and a
        // read that silently conjures an empty one would turn a file somebody
        // deleted behind our back into a file of zeros.
        let open = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .with_context(|| format!("error opening {path:?}"))?;
        #[cfg(windows)]
        file.sparse_marked.store(false, Ordering::Release);
        *fd = Some(open);
    }
    let open = fd.as_ref().expect("just opened");
    prepare(file, open, access);
    let result = f(open).with_context(|| format!("error accessing {path:?}"));
    drop(fd);

    // After the lock, or the pool could pick this very file as its victim and
    // find it held.
    if fresh {
        pool.admit(file);
    }
    result
}

#[allow(unused_variables)]
fn prepare(file: &LazyFile, open: &File, access: Access) {
    #[cfg(windows)]
    if matches!(access, Access::Write) && !file.sparse_marked.swap(true, Ordering::AcqRel) {
        mark_file_sparse(open);
    }
}

pub struct LazyStorage {
    output_folder: PathBuf,
    files: Vec<Arc<LazyFile>>,
    pool: Arc<FdPool>,
    /// Set by `take()`. See the module comment: the point of `take` is that
    /// whoever still holds the old storage can no longer write through it.
    dead: AtomicBool,
}

impl LazyStorage {
    fn file(&self, file_id: usize) -> anyhow::Result<&Arc<LazyFile>> {
        if self.dead.load(Ordering::Acquire) {
            anyhow::bail!("this storage has been taken over");
        }
        self.files.get(file_id).context("no such file")
    }

    /// Create every file of the torrent and remember where it is, holding none
    /// of them open.
    ///
    /// Separate from `init` so it can be tested without a librqbit session —
    /// everything interesting about this storage is here and in the pool.
    fn create_files(
        &mut self,
        paths: impl IntoIterator<Item = Option<PathBuf>>,
        allow_overwrite: bool,
    ) -> anyhow::Result<()> {
        let mut files = Vec::new();
        for path in paths {
            let Some(path) = path else {
                files.push(LazyFile::new(None));
                continue;
            };
            std::fs::create_dir_all(path.parent().context("bug: no parent")?)?;
            // Opened only to be created, and dropped at the end of the branch.
            // `allow_overwrite` is honoured even though every add in this player
            // passes it — the refusal is the whole meaning of the other value,
            // and inheriting it for free is cheaper than re-deciding it later.
            if allow_overwrite {
                OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .read(true)
                    .write(true)
                    .open(&path)
                    .with_context(|| format!("error opening {path:?} in read/write mode"))?;
            } else {
                OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&path)
                    .with_context(|| {
                        format!("error creating a new file (allow_overwrite = false) {path:?}")
                    })?;
            }
            files.push(LazyFile::new(Some(path)));
        }
        self.files = files;
        Ok(())
    }
}

impl TorrentStorage for LazyStorage {
    fn init(
        &mut self,
        shared: &ManagedTorrentShared,
        metadata: &TorrentMetadata,
    ) -> anyhow::Result<()> {
        self.output_folder = shared.output_folder().to_owned();
        let folder = self.output_folder.clone();
        let paths = metadata.file_infos.iter().map(|f| {
            (!f.attrs.padding).then(|| folder.join(&f.relative_filename))
        });
        let paths: Vec<_> = paths.collect();
        self.create_files(paths, shared.allow_overwrite())
    }

    fn pread_exact(&self, file_id: usize, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        let file = self.file(file_id)?;
        with_file(file, &self.pool, Access::Read, |f| f.pread_exact(offset, buf))
    }

    fn pwrite_all(&self, file_id: usize, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        let file = self.file(file_id)?;
        with_file(file, &self.pool, Access::Write, |f| f.pwrite_all(offset, buf))
    }

    /// Forwarded rather than left to the trait's default, which would split one
    /// chunk into two writes. librqbit hands a piece over as two slices of one
    /// buffer, and its own storage answers with a single `pwritev`.
    fn pwrite_all_vectored(
        &self,
        file_id: usize,
        offset: u64,
        bufs: [std::io::IoSlice<'_>; 2],
    ) -> anyhow::Result<usize> {
        let file = self.file(file_id)?;
        with_file(file, &self.pool, Access::Write, |f| {
            f.pwrite_all_vectored(offset, bufs)
        })
    }

    fn remove_file(&self, file_id: usize, filename: &Path) -> anyhow::Result<()> {
        // The handle goes before the name — see `LazyFile::close`. A file id
        // out of range is not worth failing over: the removal below is the
        // point, and it addresses the file by path.
        if let Some(file) = self.files.get(file_id) {
            file.close();
        }
        Ok(std::fs::remove_file(self.output_folder.join(filename))?)
    }

    fn ensure_file_length(&self, file_id: usize, length: u64) -> anyhow::Result<()> {
        let file = self.file(file_id)?;
        with_file(file, &self.pool, Access::Write, |f| Ok(f.set_len(length)?))
    }

    fn remove_directory_if_empty(&self, path: &Path) -> anyhow::Result<()> {
        let path = self.output_folder.join(path);
        if !path.is_dir() {
            anyhow::bail!("cannot remove dir: {path:?} is not a directory")
        }
        if std::fs::read_dir(&path)?.count() == 0 {
            std::fs::remove_dir(&path).with_context(|| format!("error removing {path:?}"))
        } else {
            Ok(())
        }
    }

    fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
        // The file table is shared rather than copied: a descriptor open in one
        // is open in the other, which is what upstream achieves by moving the
        // handles out. What is not shared is the verdict — the new storage is
        // alive and this one is not.
        let next = LazyStorage {
            output_folder: self.output_folder.clone(),
            files: self.files.clone(),
            pool: self.pool.clone(),
            dead: AtomicBool::new(false),
        };
        self.dead.store(true, Ordering::Release);
        Ok(Box::new(next))
    }
}

#[derive(Clone)]
struct LazyStorageFactory {
    pool: Arc<FdPool>,
}

impl StorageFactory for LazyStorageFactory {
    // `Box<dyn TorrentStorage>` rather than `LazyStorage` plus `.boxed()`, and
    // that is not a style choice: `StorageFactoryExt::boxed` wraps the factory
    // in a type whose `is_type_id` answers for the *wrapper* and never asks the
    // factory inside it, so the override below would be silently discarded —
    // as librqbit's own `timing`/`slow` middlewares' overrides already are.
    // Boxing here keeps this factory the one that answers.
    type Storage = Box<dyn TorrentStorage>;

    fn create(
        &self,
        shared: &ManagedTorrentShared,
        _metadata: &TorrentMetadata,
    ) -> anyhow::Result<Box<dyn TorrentStorage>> {
        Ok(Box::new(LazyStorage {
            output_folder: shared.output_folder().to_owned(),
            files: Vec::new(),
            pool: self.pool.clone(),
            dead: AtomicBool::new(false),
        }))
    }

    /// **This is a filesystem storage, and it has to say so.**
    /// `JsonSessionPersistenceStore::update_db` refuses to record a torrent
    /// whose factory is not `FilesystemStorageFactory` — reasonably, since
    /// restoring one means re-adding it against an output folder and letting
    /// the default storage find the files again. That is exactly what this
    /// does: same folder, same relative paths, same files on disk, and the only
    /// difference is how many of them are open at once. Answering to the id is
    /// how librqbit passes an underlying storage's identity through a wrapper
    /// (`storage::middleware::timing`), so this is the mechanism rather than a
    /// way around it. Without it every torrent is dropped from the session
    /// store the moment it is added — no resume data, no restore, and a full
    /// re-hash of everything on every open, which is what the five offline
    /// torrent tests in torrent.rs caught.
    fn is_type_id(&self, type_id: std::any::TypeId) -> bool {
        type_id == std::any::TypeId::of::<Self>()
            || type_id == std::any::TypeId::of::<FilesystemStorageFactory>()
    }

    fn clone_box(&self) -> BoxStorageFactory {
        Box::new(self.clone())
    }
}

/// What `SessionOptions::default_storage_factory` is given.
pub fn storage_factory() -> BoxStorageFactory {
    Box::new(LazyStorageFactory {
        pool: pool().clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("frameplayer-storage-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn storage(dir: &Path, budget: usize, count: usize) -> LazyStorage {
        let pool = Arc::new(FdPool::new(budget));
        let mut s = LazyStorage {
            output_folder: dir.to_owned(),
            files: Vec::new(),
            pool,
            dead: AtomicBool::new(false),
        };
        let paths = (0..count).map(|i| Some(dir.join(format!("f{i}.bin"))));
        s.create_files(paths, true).unwrap();
        s
    }

    fn open_count(s: &LazyStorage) -> usize {
        s.files
            .iter()
            .filter(|f| f.fd.read().unwrap().is_some())
            .count()
    }

    /// The whole point: a torrent with more files than the budget holds the
    /// budget, not the file list. Without eviction this is 40.
    #[test]
    fn the_open_file_budget_is_a_ceiling() {
        let dir = scratch("budget");
        let s = storage(&dir, 4, 40);
        for i in 0..40 {
            s.ensure_file_length(i, 8).unwrap();
            s.pwrite_all(i, 0, &[i as u8; 8]).unwrap();
            assert!(
                open_count(&s) <= 4,
                "held {} descriptors at file {i}",
                open_count(&s)
            );
        }
        assert!(open_count(&s) <= 4);
    }

    /// Creating the files must not open them — this is what the restored
    /// torrents were paying, one descriptor each, for the life of the session.
    #[test]
    fn creating_the_files_opens_none_of_them() {
        let dir = scratch("create");
        let s = storage(&dir, 4, 12);
        assert_eq!(open_count(&s), 0);
        for i in 0..12 {
            assert!(dir.join(format!("f{i}.bin")).is_file(), "f{i} was not created");
        }
    }

    /// A file that lost its descriptor reads back exactly what was written
    /// through the one it had before. Positional I/O is what makes reopening
    /// free of consequences, and this is the assertion that says so.
    #[test]
    fn data_survives_being_evicted() {
        let dir = scratch("evict");
        let s = storage(&dir, 2, 8);
        for i in 0..8u8 {
            s.pwrite_all(i as usize, 0, &[i; 16]).unwrap();
        }
        for i in 0..8u8 {
            let mut buf = [0u8; 16];
            s.pread_exact(i as usize, 0, &mut buf).unwrap();
            assert_eq!(buf, [i; 16], "file {i} came back wrong");
        }
    }

    /// Writes at an offset land where they are aimed even when the descriptor
    /// in between belonged to somebody else.
    #[test]
    fn offsets_are_absolute_across_reopens() {
        let dir = scratch("offsets");
        let s = storage(&dir, 1, 3);
        s.ensure_file_length(0, 32).unwrap();
        s.pwrite_all(0, 16, b"tail").unwrap();
        s.pwrite_all(1, 0, b"other").unwrap();
        s.pwrite_all(0, 0, b"head").unwrap();
        let mut buf = [0u8; 4];
        s.pread_exact(0, 16, &mut buf).unwrap();
        assert_eq!(&buf, b"tail");
        s.pread_exact(0, 0, &mut buf).unwrap();
        assert_eq!(&buf, b"head");
    }

    /// `take()` hands the storage over and leaves nothing usable behind — the
    /// contract librqbit relies on when a torrent is paused or deleted.
    #[test]
    fn take_leaves_the_old_storage_dead() {
        let dir = scratch("take");
        let s = storage(&dir, 4, 2);
        s.pwrite_all(0, 0, b"x").unwrap();
        let next = s.take().unwrap();
        assert!(s.pwrite_all(0, 0, b"y").is_err(), "the old storage still writes");
        assert!(s.pread_exact(0, 0, &mut [0u8; 1]).is_err());
        let mut buf = [0u8; 1];
        next.pread_exact(0, 0, &mut buf).unwrap();
        assert_eq!(&buf, b"x");
    }

    /// Removing a file closes the descriptor first. On Windows the unlink
    /// fails outright without it; on macOS it succeeds and leaves writes going
    /// to a file with no name, which is the worse of the two.
    #[test]
    fn removing_a_file_closes_it_first() {
        let dir = scratch("remove");
        let s = storage(&dir, 4, 2);
        s.pwrite_all(0, 0, b"x").unwrap();
        assert_eq!(open_count(&s), 1);
        s.remove_file(0, Path::new("f0.bin")).unwrap();
        assert_eq!(open_count(&s), 0);
        assert!(!dir.join("f0.bin").exists());
    }
}
