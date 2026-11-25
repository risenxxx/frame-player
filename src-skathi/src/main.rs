//! Frame Player version 2: version 1's interface (`../src`) on Skathi, with mpv drawing through its
//! render API into the page (`<sk-surface>`) or into a native layer under it (`<sk-layer>`).
//!
//!   frameplayer [file …]
//!
//! The files on the command line are what the interface's `take_pending_files` gets at start, as an
//! "Open with" would hand them over. The environment switches are listed in `../README.md`.
//!
//! Modules shared by both platforms sit at the top; each platform has its own renderer and its own
//! `app.rs` (the `native:mpv` module and the start). `color`, `crop` and `power` are version 1's own
//! files, included as they are, so a fix to them reaches both versions.

// No console window for a Windows release build (an installed copy flashed one, and its errors went
// nowhere). A redirected stderr still works; without one an installed copy logs to a file (velo.rs).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod cast;
mod cast_module;
mod commands;
mod dlna;
#[path = "../../src-tauri/src/lan_sweep.rs"]
mod lan_sweep;
mod logs;
mod subtitle_fonts;
mod thumbs;

#[path = "../../src-tauri/src/color.rs"]
mod color;
#[path = "../../src-tauri/src/crop.rs"]
mod crop;
#[path = "../../src-tauri/src/power.rs"]
mod power;

#[cfg(target_os = "macos")]
#[path = "macos/app.rs"]
mod app;
#[cfg(target_os = "macos")]
#[path = "macos/child.rs"]
mod child;
#[cfg(target_os = "macos")]
#[path = "macos/fullscreen.rs"]
mod fullscreen;
#[cfg(target_os = "macos")]
#[path = "macos/live_resize.rs"]
mod live_resize;
#[cfg(target_os = "macos")]
#[path = "macos/mpv.rs"]
mod mpv;
#[cfg(target_os = "macos")]
#[path = "macos/now_playing.rs"]
mod now_playing;
#[cfg(target_os = "macos")]
#[path = "macos/quit.rs"]
mod quit;
#[cfg(target_os = "macos")]
#[path = "macos/sys.rs"]
mod sys;
#[cfg(target_os = "macos")]
#[path = "macos/velo.rs"]
mod velo;
#[cfg(target_os = "macos")]
#[path = "macos/vk.rs"]
mod vk;

#[cfg(windows)]
#[path = "windows/app.rs"]
mod app;
#[cfg(windows)]
#[path = "windows/mpv.rs"]
mod mpv;
#[cfg(windows)]
#[path = "windows/sys.rs"]
mod sys;
#[cfg(windows)]
#[path = "windows/velo.rs"]
mod velo;

#[cfg(not(any(target_os = "macos", windows)))]
compile_error!("version 2 runs on macOS and Windows only");

fn main() {
    app::main()
}
