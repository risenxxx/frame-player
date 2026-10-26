//! What the player says about itself, kept for a bug report.
//!
//! Three things live here, and they belong together because a report needs all
//! three at once:
//!
//! - **The verbose flag.** One switch for every wire and protocol log the native
//!   side has (Cast, DLNA, the LAN server, AirPlay). It used to be
//!   `FP_CAST_DEBUG` read once into a `OnceLock`, which only somebody starting
//!   the player from a terminal could ever turn on. The variable still turns it
//!   on from the first second; the settings turn it on and off at runtime, and
//!   nothing remembers it across a restart — a protocol log quietly left on is
//!   a log quietly growing, and a report quietly carrying more than anyone
//!   meant to send.
//! - **The capture of stderr.** ~140 `eprintln!`s, and whatever the C libraries
//!   in the process write, go to a pipe whose reader keeps the last lines in a
//!   ring and passes every byte on to the original stderr — so `tauri dev`
//!   still prints to the terminal, and an installed copy launched from the
//!   Finder or the Start menu, whose stderr goes nowhere, keeps them too.
//! - **The rest of the report's native half**: the OS, the copy, the mpv log's
//!   tail, and the names this process has learned that a report must not carry
//!   (devices, the user, the machine) — handed to the frontend, which owns the
//!   redaction because it owns the assembled text (`src/lib/redact.ts`).
//!
//! Nothing here sends anything anywhere. The frontend shows the whole report
//! before it can be copied, saved or attached.

use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

// ---- The verbose flag --------------------------------------------------------

static VERBOSE: AtomicBool = AtomicBool::new(false);
/// When it was last turned on, in ms since the epoch; 0 while off.
static VERBOSE_SINCE: AtomicU64 = AtomicU64::new(0);
static FROM_ENV: AtomicBool = AtomicBool::new(false);

/// The wire logs are on. Read at every call site, never cached: a value taken
/// once at startup is exactly what this replaced.
///
/// Off by default, and not for the disk's sake: a playing HLS session is a
/// request every few seconds and a status frame every second, which buries
/// everything else — in the dev console, and now in the ring a report is made
/// of. The same reasoning as the `log::set_level(Error)` next to
/// `ffmpeg_the_third::init()`.
pub fn verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

/// `FP_CAST_DEBUG=1` still means "verbose from the first line" — the only way
/// to have it for discovery that starts before the window exists.
fn init_from_env() {
    if std::env::var("FP_CAST_DEBUG").map(|v| v != "0").unwrap_or(false) {
        FROM_ENV.store(true, Ordering::Relaxed);
        set_verbose(true);
    }
}

fn set_verbose(on: bool) {
    let was = VERBOSE.swap(on, Ordering::Relaxed);
    if on && !was {
        VERBOSE_SINCE.store(now_ms(), Ordering::Relaxed);
    }
    if !on {
        VERBOSE_SINCE.store(0, Ordering::Relaxed);
        FROM_ENV.store(false, Ordering::Relaxed);
        // Back to the ordinary budget at once rather than at the next line.
        if let Some(ring) = RING.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
            ring.trim(caps());
        }
    }
}

#[derive(Serialize)]
pub struct LogState {
    verbose: bool,
    /// Milliseconds since the epoch; null while off.
    since: Option<u64>,
    /// Turned on by the environment variable rather than by the viewer.
    from_env: bool,
}

fn state() -> LogState {
    let since = VERBOSE_SINCE.load(Ordering::Relaxed);
    LogState {
        verbose: verbose(),
        since: (since > 0).then_some(since),
        from_env: FROM_ENV.load(Ordering::Relaxed),
    }
}

#[tauri::command]
pub fn debug_log_set(on: bool) -> LogState {
    set_verbose(on);
    // Into the log itself, so a report says where the detail begins.
    eprintln!("[logs] verbose log {}", if on { "on" } else { "off" });
    state()
}

#[tauri::command]
pub fn debug_log_state() -> LogState {
    state()
}

// ---- The ring ----------------------------------------------------------------

struct Caps {
    lines: usize,
    bytes: usize,
}

/// 5 000 lines is several hours of an ordinary session and a few seconds of a
/// Cast session with the wire log on — a status frame is a line every second
/// and one LOAD answer alone is a few kilobytes. Bytes cap both, so twenty
/// thousand lines of XML cannot become a hundred megabytes of RAM.
const NORMAL: Caps = Caps { lines: 5_000, bytes: 1 << 20 };
const VERBOSE_CAPS: Caps = Caps { lines: 20_000, bytes: 4 << 20 };
/// A longer line is cut: a DLNA description or a Cast media status is a few
/// kilobytes, FFmpeg's `\r`-separated progress is unbounded.
const LINE_MAX: usize = 4096;

fn caps() -> &'static Caps {
    if verbose() {
        &VERBOSE_CAPS
    } else {
        &NORMAL
    }
}

#[derive(Default)]
struct Ring {
    lines: VecDeque<String>,
    bytes: usize,
    dropped: u64,
}

impl Ring {
    fn push(&mut self, line: String) {
        self.bytes += line.len();
        self.lines.push_back(line);
        self.trim(caps());
    }

    fn trim(&mut self, caps: &Caps) {
        while self.lines.len() > caps.lines || (self.bytes > caps.bytes && self.lines.len() > 1) {
            if let Some(old) = self.lines.pop_front() {
                self.bytes -= old.len();
                self.dropped += 1;
            }
        }
    }
}

static RING: Mutex<Option<Ring>> = Mutex::new(None);

fn record(text: &str) {
    let text = text.trim_end();
    if text.is_empty() {
        return;
    }
    let mut line = format!("{} {}", clock(now_ms()), text);
    if line.len() > LINE_MAX {
        let mut cut = LINE_MAX;
        while !line.is_char_boundary(cut) {
            cut -= 1;
        }
        let extra = line.len() - cut;
        line.truncate(cut);
        line.push_str(&format!(" …[+{extra} bytes]"));
    }
    RING.lock()
        .unwrap_or_else(|p| p.into_inner())
        .get_or_insert_with(Ring::default)
        .push(line);
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `HH:MM:SS.mmm` in UTC — the frontend stamps its lines the same way, so the
/// two halves of a report can be read side by side.
fn clock(ms: u64) -> String {
    let day = ms % 86_400_000;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        day / 3_600_000,
        day / 60_000 % 60,
        day / 1000 % 60,
        day % 1000
    )
}

/// Turns the bytes coming out of the pipe into lines. `\r` ends a line too —
/// a progress meter rewrites one line with it, and as one line it would be
/// cut at `LINE_MAX` long before it said anything.
#[derive(Default)]
struct Lines {
    partial: Vec<u8>,
    overflow: bool,
}

impl Lines {
    fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if b == b'\n' || b == b'\r' {
                if !self.partial.is_empty() {
                    record(&String::from_utf8_lossy(&self.partial));
                }
                self.partial.clear();
                self.overflow = false;
            } else if self.partial.len() < LINE_MAX {
                self.partial.push(b);
            } else if !self.overflow {
                self.overflow = true;
                self.partial.extend_from_slice(" …".as_bytes());
            }
        }
    }
}

// ---- Capturing stderr --------------------------------------------------------

/// What the capture managed, in words a report can print.
static CAPTURE: Mutex<String> = Mutex::new(String::new());

fn note_capture(what: String) {
    *CAPTURE.lock().unwrap_or_else(|p| p.into_inner()) = what;
}

/// The first thing `run()` does after raising the descriptor limit. Costs one
/// pipe, one duplicate of the original stderr and one thread.
pub fn install() {
    init_from_env();
    match capture() {
        Ok(what) => note_capture(what),
        Err(e) => note_capture(format!("not captured: {e}")),
    }
}

/// The reader. **It must never write to stderr itself**: stderr is the pipe it
/// drains, and a write that fills the pipe from the only thread emptying it
/// never returns.
fn pump(mut input: std::fs::File, mut passthrough: Passthrough) {
    let mut buf = [0u8; 8192];
    let mut lines = Lines::default();
    loop {
        let n = match input.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return,
        };
        passthrough.write(&buf[..n]);
        lines.feed(&buf[..n]);
    }
}

#[cfg(target_os = "macos")]
struct Passthrough(Option<std::fs::File>);

#[cfg(target_os = "macos")]
impl Passthrough {
    fn write(&mut self, bytes: &[u8]) {
        use std::io::Write;
        // A terminal that went away (the dev run's shell closed) is not a
        // reason to stop keeping the lines.
        if let Some(out) = self.0.as_mut() {
            if out.write_all(bytes).is_err() {
                self.0 = None;
            }
        }
    }
}

/// `pipe` + `dup2` onto descriptor 2. Every writer in the process reaches the
/// same descriptor — Rust's `eprintln!`, the C libraries' `fprintf(stderr)`,
/// AppKit's `NSLog` — so one redirect covers all of them, and child processes
/// inherit it as their stderr like they inherited the original.
#[cfg(target_os = "macos")]
fn capture() -> Result<String, String> {
    use std::os::fd::FromRawFd;

    let last = || std::io::Error::last_os_error().to_string();
    // SAFETY: plain descriptor calls on descriptors this function owns; 2 is
    // replaced, never closed, so nothing holding it sees it vanish.
    unsafe {
        let mut fds = [0; 2];
        if libc::pipe(fds.as_mut_ptr()) != 0 {
            return Err(format!("pipe: {}", last()));
        }
        let (read_fd, write_fd) = (fds[0], fds[1]);
        // -1 when there was no stderr at all; the lines are still kept.
        let original = libc::dup(2);
        if libc::dup2(write_fd, 2) < 0 {
            let e = last();
            libc::close(read_fd);
            libc::close(write_fd);
            if original >= 0 {
                libc::close(original);
            }
            return Err(format!("dup2: {e}"));
        }
        libc::close(write_fd);
        // Ours alone: a child process inherits descriptor 2, not these.
        libc::fcntl(read_fd, libc::F_SETFD, libc::FD_CLOEXEC);
        if original >= 0 {
            libc::fcntl(original, libc::F_SETFD, libc::FD_CLOEXEC);
        }
        let input = std::fs::File::from_raw_fd(read_fd);
        let out = (original >= 0).then(|| std::fs::File::from_raw_fd(original));
        let passthrough = Passthrough(out);
        std::thread::Builder::new()
            .name("stderr-ring".into())
            .spawn(move || pump(input, passthrough))
            .map_err(|e| format!("thread: {e}"))?;
        Ok(if original >= 0 {
            "captured (pipe on fd 2, passed through)".into()
        } else {
            "captured (pipe on fd 2; the process had no stderr)".into()
        })
    }
}

/// The original stderr, if there was one — a console in a debug build, nothing
/// at all in a release build (a GUI-subsystem process gets no standard handles
/// unless its parent hands it some).
#[cfg(windows)]
enum Passthrough {
    None,
    /// A console wants UTF-16: the same bytes through `WriteFile` come out in
    /// the console's code page, which is how Rust's own stderr avoided
    /// mojibake before it was redirected.
    Console(isize, Vec<u8>),
    File(std::fs::File),
}

#[cfg(windows)]
impl Passthrough {
    fn write(&mut self, bytes: &[u8]) {
        use std::io::Write;
        match self {
            Passthrough::None => {}
            Passthrough::File(f) => {
                if f.write_all(bytes).is_err() {
                    *self = Passthrough::None;
                }
            }
            Passthrough::Console(handle, carry) => {
                use windows_sys::Win32::System::Console::WriteConsoleW;
                carry.extend_from_slice(bytes);
                // Whole characters only; a sequence split across two reads
                // waits for its second half.
                let valid = match std::str::from_utf8(carry) {
                    Ok(s) => s.len(),
                    Err(e) if e.error_len().is_none() => e.valid_up_to(),
                    Err(_) => carry.len(),
                };
                let text = String::from_utf8_lossy(&carry[..valid]).into_owned();
                carry.drain(..valid);
                let wide: Vec<u16> = text.encode_utf16().collect();
                let mut written = 0u32;
                // SAFETY: a handle duplicated for this thread, a live buffer.
                let ok = unsafe {
                    WriteConsoleW(
                        *handle as _,
                        wide.as_ptr().cast(),
                        wide.len() as u32,
                        &mut written,
                        std::ptr::null(),
                    )
                };
                if ok == 0 {
                    *self = Passthrough::None;
                }
            }
        }
    }
}

/// Windows has two stderrs in one process, and they are redirected separately.
///
/// **Rust's** is the process's standard handle: `eprintln!` asks
/// `GetStdHandle` on every write, so `SetStdHandle` to a pipe is all it takes.
/// **The C runtime's** is descriptor 2 and the `stderr` stream, bound when the
/// runtime started — in a GUI process to nothing at all — and `SetStdHandle`
/// does not touch either. The universal CRT is shared by every module that
/// links it, so `freopen` + `_dup2` there reaches all of them; a DLL built
/// against the old `msvcrt.dll` has a runtime of its own and is not reached.
/// The CRT half is resolved at runtime rather than linked, so a CRT that does
/// not have one of these is a line in the report and not a build that fails.
#[cfg(windows)]
fn capture() -> Result<String, String> {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::{
        DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::System::Console::{
        GetConsoleMode, GetStdHandle, SetStdHandle, STD_ERROR_HANDLE,
    };
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    // SAFETY: handle bookkeeping on handles this function creates or
    // duplicates; the original standard handle is duplicated, never closed.
    unsafe {
        let me = GetCurrentProcess();
        let original = GetStdHandle(STD_ERROR_HANDLE);
        let mut passthrough = Passthrough::None;
        if !original.is_null() && original != INVALID_HANDLE_VALUE {
            let mut dup: HANDLE = std::ptr::null_mut();
            if DuplicateHandle(me, original, me, &mut dup, 0, 0, DUPLICATE_SAME_ACCESS) != 0 {
                let mut mode = 0u32;
                passthrough = if GetConsoleMode(dup, &mut mode) != 0 {
                    Passthrough::Console(dup as isize, Vec::new())
                } else {
                    Passthrough::File(std::fs::File::from_raw_handle(dup as _))
                };
            }
        }
        let mut read: HANDLE = std::ptr::null_mut();
        let mut write: HANDLE = std::ptr::null_mut();
        if CreatePipe(&mut read, &mut write, std::ptr::null(), 0) == 0 {
            return Err(format!("CreatePipe: {}", std::io::Error::last_os_error()));
        }
        if SetStdHandle(STD_ERROR_HANDLE, write) == 0 {
            return Err(format!("SetStdHandle: {}", std::io::Error::last_os_error()));
        }
        let crt = redirect_crt(write);
        let input = std::fs::File::from_raw_handle(read as _);
        std::thread::Builder::new()
            .name("stderr-ring".into())
            .spawn(move || pump(input, passthrough))
            .map_err(|e| format!("thread: {e}"))?;
        Ok(format!("captured (std handle; ucrt: {crt})"))
    }
}

/// Point the universal CRT's descriptor 2 and its `stderr` stream at `pipe`.
/// Returns what happened, for the report.
#[cfg(windows)]
unsafe fn redirect_crt(pipe: windows_sys::Win32::Foundation::HANDLE) -> String {
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE};
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    type IobFunc = unsafe extern "C" fn(u32) -> *mut c_void;
    type FreopenS =
        unsafe extern "C" fn(*mut *mut c_void, *const u8, *const u8, *mut c_void) -> i32;
    type Fileno = unsafe extern "C" fn(*mut c_void) -> i32;
    type OpenOsfhandle = unsafe extern "C" fn(isize, i32) -> i32;
    type Dup2 = unsafe extern "C" fn(i32, i32) -> i32;
    type Setvbuf = unsafe extern "C" fn(*mut c_void, *mut u8, i32, usize) -> i32;
    const O_BINARY: i32 = 0x8000;
    const IONBF: i32 = 0x0004;

    let name: Vec<u16> = "ucrtbase.dll\0".encode_utf16().collect();
    let module = GetModuleHandleW(name.as_ptr());
    if module.is_null() {
        return "not loaded".into();
    }
    macro_rules! sym {
        ($name:literal, $ty:ty) => {
            match GetProcAddress(module, concat!($name, "\0").as_ptr()) {
                Some(f) => std::mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(f),
                None => return format!("no {}", $name),
            }
        };
    }
    let iob = sym!("__acrt_iob_func", IobFunc);
    let freopen_s = sym!("freopen_s", FreopenS);
    let fileno = sym!("_fileno", Fileno);
    let open_osfhandle = sym!("_open_osfhandle", OpenOsfhandle);
    let dup2 = sym!("_dup2", Dup2);
    let setvbuf = sym!("setvbuf", Setvbuf);

    // The CRT closes what it is given, so it gets a handle of its own.
    let me = GetCurrentProcess();
    let mut own: HANDLE = std::ptr::null_mut();
    if DuplicateHandle(me, pipe, me, &mut own, 0, 0, DUPLICATE_SAME_ACCESS) == 0 {
        return "DuplicateHandle failed".into();
    }
    let fd = open_osfhandle(own as isize, O_BINARY);
    if fd < 0 {
        return "_open_osfhandle failed".into();
    }
    let stderr = iob(2);
    // In a GUI process the stream belongs to no descriptor (`_fileno` is -2),
    // and `_dup2` onto 2 would not change that: reopening it on NUL first is
    // what gives it one to redirect.
    let mut reopened: *mut c_void = std::ptr::null_mut();
    let mut stream_fd = fileno(stderr);
    if stream_fd < 0 && freopen_s(&mut reopened, c"NUL".as_ptr().cast(), c"w".as_ptr().cast(), stderr) == 0 {
        stream_fd = fileno(stderr);
    }
    let fd2 = dup2(fd, 2) == 0;
    let stream = if stream_fd == 2 { fd2 } else { stream_fd >= 0 && dup2(fd, stream_fd) == 0 };
    if stream {
        setvbuf(stderr, std::ptr::null_mut(), IONBF, 0);
    }
    match (fd2, stream) {
        (true, true) => "fd 2 and stderr".into(),
        (true, false) => "fd 2 only".into(),
        (false, true) => "stderr only".into(),
        (false, false) => "failed".into(),
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
struct Passthrough;

#[cfg(not(any(target_os = "macos", windows)))]
impl Passthrough {
    fn write(&mut self, _: &[u8]) {}
}

#[cfg(not(any(target_os = "macos", windows)))]
fn capture() -> Result<String, String> {
    let _ = pump;
    Err("not implemented on this platform".into())
}

// ---- Names a report must not carry ---------------------------------------------

/// What a report must not name, learned where it was learned: a television's
/// own name the moment discovery reads it, the user and the machine once.
/// Handed to the redaction as literals, because a name is not a pattern — the
/// line `[cast] unicast: Living Room at 192.168.1.20` has nothing around the
/// name that says where it starts.
static SECRETS: Mutex<Vec<Secret>> = Mutex::new(Vec::new());
const SECRETS_MAX: usize = 512;

#[derive(Serialize, Clone, PartialEq)]
pub struct Secret {
    kind: &'static str,
    value: String,
}

/// Remember something a report has to replace wherever it appears. Kinds the
/// frontend knows: `device`, `user`, `home`, `host`.
pub fn remember(kind: &'static str, value: &str) {
    let value = value.trim();
    // Too short to replace safely: three letters are part of every other word.
    if value.chars().count() < 3 {
        return;
    }
    let secret = Secret { kind, value: value.to_string() };
    let mut all = SECRETS.lock().unwrap_or_else(|p| p.into_inner());
    if !all.contains(&secret) && all.len() < SECRETS_MAX {
        all.push(secret);
    }
}

fn remember_person() {
    for var in ["HOME", "USERPROFILE"] {
        if let Ok(v) = std::env::var(var) {
            remember("home", &v);
        }
    }
    for var in ["USER", "USERNAME", "LOGNAME"] {
        if let Ok(v) = std::env::var(var) {
            remember("user", &v);
        }
    }
    if let Some(host) = hostname() {
        // "Someones-MacBook-Pro.local" also appears without its suffix.
        if let Some(short) = host.strip_suffix(".local") {
            remember("host", short);
        }
        remember("host", &host);
    }
}

#[cfg(target_os = "macos")]
fn hostname() -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer outlives the call and its length is passed.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return None;
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    Some(String::from_utf8_lossy(&buf[..end]).into_owned())
}

#[cfg(not(target_os = "macos"))]
fn hostname() -> Option<String> {
    std::env::var("COMPUTERNAME").ok()
}

// ---- The report's native half ----------------------------------------------------

/// Where mpv writes its log for this session (`log-file`, set by the frontend
/// before `init`). In the cache directory: it is worth nothing after the
/// session, and mpv truncates it when it opens it.
fn mpv_log_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = crate::portable::cache_dir(app)?.join("logs");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    Ok(dir.join("mpv.log"))
}

#[tauri::command]
pub fn diag_mpv_log_path(app: tauri::AppHandle) -> Result<String, String> {
    Ok(mpv_log_path(&app)?.to_string_lossy().into_owned())
}

/// The file mpv is writing its log to. A `log-file` in the viewer's mpv.conf
/// wins over ours, as every line of theirs does (`initPlayer`), so that one is
/// read instead — from mpv.conf itself, never from a path the webview names,
/// so a report reads one of two files a person put there and nothing else.
/// `conf_refused`: mpv started without the viewer's file, and ours is in force.
fn mpv_log_in_force(app: &tauri::AppHandle, conf_refused: bool) -> Option<(std::path::PathBuf, &'static str)> {
    if !conf_refused {
        if let Ok(conf) = crate::user_mpv_conf(app.clone()) {
            if let Some((_, v)) = conf.options.iter().rev().find(|(k, _)| k == "log-file") {
                let path = match v.strip_prefix("~/") {
                    Some(rest) => std::path::PathBuf::from(std::env::var_os("HOME")?).join(rest),
                    None => std::path::PathBuf::from(v),
                };
                return Some((path, "mpv.conf"));
            }
        }
    }
    Some((mpv_log_path(app).ok()?, "player"))
}

#[derive(Serialize)]
pub struct MpvLog {
    /// Whose `log-file` it is: `player` or `mpv.conf`.
    source: &'static str,
    tail: String,
    /// The file's size and how much of its end is in `tail`.
    total: u64,
    shown: u64,
}

#[derive(Serialize)]
pub struct Collected {
    native: Vec<String>,
    /// Lines that fell off the front of the ring.
    native_dropped: u64,
    capture: String,
    mpv_log: Option<MpvLog>,
    os: String,
    arch: &'static str,
    /// An x86_64 build running under Rosetta.
    translated: bool,
    portable: bool,
    installed: bool,
    /// Our own FFmpeg (thumbnails, posters, frame probes), which is not mpv's.
    ffmpeg: String,
    secrets: Vec<Secret>,
    state: LogState,
}

/// The tail of mpv's log. 256 KiB is the last few minutes of a debug-level
/// log; with the verbose log on it is a megabyte, since the point of turning it
/// on was to reproduce something and collect at once.
fn mpv_tail(app: &tauri::AppHandle, conf_refused: bool) -> Option<MpvLog> {
    let (path, source) = mpv_log_in_force(app, conf_refused)?;
    let mut file = std::fs::File::open(&path).ok()?;
    let total = file.metadata().ok()?.len();
    let want: u64 = if verbose() { 1 << 20 } else { 256 << 10 };
    let start = total.saturating_sub(want);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.take(want).read_to_end(&mut bytes).ok()?;
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    // Begin on a whole line.
    if start > 0 {
        if let Some(nl) = text.find('\n') {
            text.drain(..=nl);
        }
    }
    Some(MpvLog { source, shown: text.len() as u64, tail: text, total })
}

#[tauri::command]
pub fn diag_collect(app: tauri::AppHandle, conf_refused: bool) -> Collected {
    remember_person();
    let (native, native_dropped) = {
        let ring = RING.lock().unwrap_or_else(|p| p.into_inner());
        match ring.as_ref() {
            Some(r) => (r.lines.iter().cloned().collect(), r.dropped),
            None => (Vec::new(), 0),
        }
    };
    let av = ffmpeg_the_third::codec::version();
    Collected {
        native,
        native_dropped,
        capture: CAPTURE.lock().unwrap_or_else(|p| p.into_inner()).clone(),
        mpv_log: mpv_tail(&app, conf_refused),
        os: os_version(),
        arch: std::env::consts::ARCH,
        translated: translated(),
        portable: crate::portable::is_portable(),
        installed: crate::portable::installed(&app),
        ffmpeg: format!("libavcodec {}.{}.{}", av >> 16, (av >> 8) & 0xff, av & 0xff),
        secrets: SECRETS.lock().unwrap_or_else(|p| p.into_inner()).clone(),
        state: state(),
    }
}

/// Write the report where the save dialog said. The text is what the viewer was
/// shown, byte for byte.
#[tauri::command]
pub fn diag_save(path: String, text: String) -> Result<(), String> {
    std::fs::write(&path, text).map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn sysctl_string(name: &str) -> Option<String> {
    let name = std::ffi::CString::new(name).ok()?;
    let mut buf = [0u8; 128];
    let mut len = buf.len();
    // SAFETY: the buffer and its length outlive the call.
    let rc = unsafe {
        libc::sysctlbyname(name.as_ptr(), buf.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0)
    };
    if rc != 0 {
        return None;
    }
    let end = buf[..len].iter().position(|&b| b == 0).unwrap_or(len);
    Some(String::from_utf8_lossy(&buf[..end]).into_owned())
}

#[cfg(target_os = "macos")]
fn os_version() -> String {
    let version = sysctl_string("kern.osproductversion").unwrap_or_else(|| "?".into());
    match sysctl_string("kern.osversion") {
        Some(build) => format!("macOS {version} ({build})"),
        None => format!("macOS {version}"),
    }
}

#[cfg(target_os = "macos")]
fn translated() -> bool {
    let mut value: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>();
    // SAFETY: an int-sized out parameter with its size passed.
    let rc = unsafe {
        libc::sysctlbyname(
            c"sysctl.proc_translated".as_ptr(),
            (&mut value as *mut libc::c_int).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    rc == 0 && value == 1
}

/// The registry rather than `GetVersionEx`, which answers whatever the manifest
/// claims compatibility with. `ProductName` still says "Windows 10" on 11, so
/// the build number decides that.
#[cfg(windows)]
fn os_version() -> String {
    let Ok(key) = windows_registry::LOCAL_MACHINE.open(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
    else {
        return "Windows".into();
    };
    let build: u32 = key
        .get_string("CurrentBuildNumber")
        .ok()
        .and_then(|b| b.parse().ok())
        .unwrap_or(0);
    let ubr = key.get_u32("UBR").unwrap_or(0);
    let display = key.get_string("DisplayVersion").unwrap_or_default();
    let name = if build >= 22000 { "Windows 11" } else { "Windows 10" };
    format!("{name} {display} (build {build}.{ubr})")
}

#[cfg(windows)]
fn translated() -> bool {
    false
}

#[cfg(not(any(target_os = "macos", windows)))]
fn os_version() -> String {
    std::env::consts::OS.into()
}

#[cfg(not(any(target_os = "macos", windows)))]
fn translated() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_is_utc_wall_time() {
        assert_eq!(clock(20_000 * 86_400_000 + 45_296_789), "12:34:56.789");
        assert_eq!(clock(0), "00:00:00.000");
    }

    #[test]
    fn ring_keeps_the_newest_within_both_caps() {
        let caps = Caps { lines: 3, bytes: 10 };
        let mut ring = Ring::default();
        for line in ["aaaa", "bbbb", "cccc"] {
            ring.lines.push_back(line.into());
            ring.bytes += 4;
        }
        ring.trim(&caps);
        // 12 bytes > 10: the oldest goes.
        assert_eq!(ring.lines, ["bbbb", "cccc"]);
        assert_eq!(ring.dropped, 1);
        assert_eq!(ring.bytes, 8);
    }

    #[test]
    fn carriage_returns_end_lines_and_long_lines_are_cut() {
        let mut lines = Lines::default();
        lines.feed(b"one\rtwo");
        assert_eq!(lines.partial, b"two");
        lines.feed(&vec![b'x'; LINE_MAX * 2]);
        assert!(lines.partial.len() <= LINE_MAX + " …".len());
    }
}
